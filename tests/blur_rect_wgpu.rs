//! Headless GPU tests for `Canvas::blur_rect`: a step edge blurred in
//! place, on the screen and on an image target, follows the analytic
//! Gaussian profile 0.5 * (1 + erf(d / (sigma * sqrt(2)))) inside the rect,
//! within one shader pass and down the pyramid (an odd and an even number of
//! halvings), and every pixel outside the rect keeps its value. The edge runs
//! along each axis in turn, so a write-back turned over or shifted shows.
//! A rect at the target's edges keeps a flat colour: the blur repeats the
//! edge pixels outward rather than fading to transparent.
//! Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, ImageFlags, Paint, Path};

mod common;
use common::{headless_device, render_on_image, render_rgba};

const W: u32 = 256;
const H: u32 = 192;
/// The step along x and along y: black before it, white from it on, through
/// the middle of the rect.
const EDGE: [u32; 2] = [128, 96];
/// The blurred rect, more than 3 * 20 px from the canvas's edges on every
/// side, so the blur's reach stays on the canvas.
const REGION: [u32; 4] = [96, 72, 64, 48];
/// The allowed deviation from the analytic profile, in 8-bit counts.
const TOLERANCE: f64 = 3.0;

#[derive(Clone, Copy, Debug)]
enum Axis {
    X,
    Y,
}

impl Axis {
    fn edge(self) -> u32 {
        EDGE[self as usize]
    }
}

fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let y = 1.0 - poly * (-x * x).exp();
    if x < 0.0 {
        -y
    } else {
        y
    }
}

/// What the unblurred step holds at `at`, the pixel's coordinate along `axis`.
fn step(axis: Axis, at: u32) -> f64 {
    if at < axis.edge() {
        0.0
    } else {
        255.0
    }
}

/// What a Gaussian of `sigma` leaves at `at`, sampled at the pixel center.
fn profile(axis: Axis, sigma: f64, at: u32) -> f64 {
    255.0 * 0.5 * (1.0 + erf((at as f64 + 0.5 - axis.edge() as f64) / (sigma * 2f64.sqrt())))
}

/// Black up to the edge along `axis`, without antialiasing so the edge is
/// exactly at the pixel boundary; the canvas is cleared to white.
fn draw_step(canvas: &mut Canvas<WGPURenderer>, axis: Axis) {
    let mut path = Path::new();
    match axis {
        Axis::X => path.rect(0.0, 0.0, axis.edge() as f32, H as f32),
        Axis::Y => path.rect(0.0, 0.0, W as f32, axis.edge() as f32),
    }
    let mut paint = Paint::color(Color::black());
    paint.set_anti_alias(false);
    canvas.fill_path(&path, &paint);
}

/// Checks every pixel: the profile inside the rect, the step outside it.
fn assert_blurred(out: &[u8], axis: Axis, sigma: f64) {
    let [rx, ry, rw, rh] = REGION;
    let mut worst = (0.0, 0, 0);
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 4) as usize;
            let at = match axis {
                Axis::X => x,
                Axis::Y => y,
            };
            let inside = (rx..rx + rw).contains(&x) && (ry..ry + rh).contains(&y);
            if !inside {
                assert_eq!(
                    out[i] as f64,
                    step(axis, at),
                    "pixel ({x}, {y}) outside the rect changed"
                );
                continue;
            }
            assert_eq!(out[i + 3], 255, "pixel ({x}, {y}) lost its alpha");
            let deviation = (out[i] as f64 - profile(axis, sigma, at)).abs();
            if deviation > worst.0 {
                worst = (deviation, x, y);
            }
        }
    }
    assert!(
        worst.0 <= TOLERANCE,
        "{axis:?} sigma {sigma}: pixel ({}, {}) is {} counts off the profile",
        worst.1,
        worst.2,
        worst.0
    );
}

fn blur(canvas: &mut Canvas<WGPURenderer>, sigma: f32) {
    let [x, y, w, h] = REGION;
    canvas.blur_rect(x, y, w, h, sigma).expect("blur_rect");
}

fn on_screen(axis: Axis, sigma: f32) {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render_rgba(&device, &queue, W, H, Color::white(), |canvas| {
        draw_step(canvas, axis);
        blur(canvas, sigma);
    });
    assert_blurred(&out, axis, sigma as f64);
}

/// The same blur on an image target with `flags`.
fn on_image(flags: ImageFlags, axis: Axis, sigma: f32) {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render_on_image(&device, &queue, W, H, flags, Color::white(), |canvas| {
        draw_step(canvas, axis);
        blur(canvas, sigma);
    });
    assert_blurred(&out, axis, sigma as f64);
}

#[test]
fn a_blur_within_one_pass_follows_the_profile_on_the_screen() {
    on_screen(Axis::X, 3.0);
    on_screen(Axis::Y, 3.0);
}

/// Sigma 12 halves once, so the pyramid's level is stored turned over from
/// the copy; sigma 20 halves twice, so it isn't.
#[test]
fn a_pyramid_blur_follows_the_profile_on_the_screen() {
    for sigma in [12.0, 20.0] {
        on_screen(Axis::X, sigma);
        on_screen(Axis::Y, sigma);
    }
}

#[test]
fn a_blur_follows_the_profile_on_an_image_target() {
    for sigma in [3.0, 12.0, 20.0] {
        on_image(ImageFlags::empty(), Axis::X, sigma);
        on_image(ImageFlags::empty(), Axis::Y, sigma);
    }
}

/// The image's own sampling doesn't apply to the first draw, the one that
/// reads the image target: repeat would wrap its taps past the left edge
/// around to the right, and a nearest lookup would make the pyramid's first
/// halving pick one of each pair of pixels instead of averaging them.
#[test]
fn a_blur_ignores_the_image_targets_sampling_flags() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let flags = ImageFlags::NEAREST | ImageFlags::REPEAT_X | ImageFlags::REPEAT_Y;
    let without_aa = |color| {
        let mut paint = Paint::color(color);
        paint.set_anti_alias(false);
        paint
    };

    // White, with black along the right that only a wrap reaches.
    let out = render_on_image(&device, &queue, W, H, flags, Color::white(), |canvas| {
        let mut path = Path::new();
        path.rect((W - 16) as f32, 0.0, 16.0, H as f32);
        canvas.fill_path(&path, &without_aa(Color::black()));
        canvas.blur_rect(0, 0, 64, 48, 3.0).expect("blur_rect");
    });
    for y in 0..48 {
        for x in 0..64 {
            let i = ((y * W + x) * 4) as usize;
            assert!(
                out[i] >= 254,
                "pixel ({x}, {y}) is {:?}: the blur wrapped",
                &out[i..i + 4]
            );
        }
    }

    // A one-pixel checkerboard: its pairs average to mid grey, while picking
    // one pixel of each would pick the same colour everywhere.
    let out = render_on_image(&device, &queue, W, H, flags, Color::white(), |canvas| {
        let mut path = Path::new();
        for y in 0..H {
            for x in (y % 2..W).step_by(2) {
                path.rect(x as f32, y as f32, 1.0, 1.0);
            }
        }
        canvas.fill_path(&path, &without_aa(Color::black()));
        blur(canvas, 12.0);
    });
    let [rx, ry, rw, rh] = REGION;
    for y in ry..ry + rh {
        for x in rx..rx + rw {
            let i = ((y * W + x) * 4) as usize;
            assert!(
                out[i].abs_diff(128) <= 3,
                "pixel ({x}, {y}) is {:?}: the halving didn't average",
                &out[i..i + 4]
            );
        }
    }
}

/// Rects in opposite corners, so the reach passes all four edges, over a flat
/// colour, on the screen and on an image target: a blur that read
/// transparent beyond the target would darken them.
#[test]
fn a_blur_at_the_targets_edges_repeats_them() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let color = Color::rgb(40, 120, 200);
    for sigma in [3.0, 12.0, 20.0] {
        let blur_corners = |canvas: &mut Canvas<WGPURenderer>| {
            canvas.blur_rect(0, 0, 64, 48, sigma).expect("blur_rect");
            canvas.blur_rect(W - 64, H - 48, 64, 48, sigma).expect("blur_rect");
        };
        let on_screen = render_rgba(&device, &queue, W, H, color, blur_corners);
        let on_image = render_on_image(&device, &queue, W, H, ImageFlags::empty(), color, blur_corners);
        for (target, out) in [("screen", on_screen), ("image", on_image)] {
            for (i, pixel) in out.chunks_exact(4).enumerate() {
                let (x, y) = (i as u32 % W, i as u32 / W);
                for (channel, expected) in pixel.iter().zip([40, 120, 200, 255]) {
                    assert!(
                        channel.abs_diff(expected) <= 1,
                        "{target} sigma {sigma}: pixel ({x}, {y}) is {pixel:?}"
                    );
                }
            }
        }
    }
}
