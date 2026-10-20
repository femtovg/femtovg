//! Headless GPU tests for `Canvas::blur_path`: a path blurred in place over
//! a checkerboard matches `blur_rect` over the same region where the path
//! covers a pixel whole, keeps the checkerboard where it covers none, and
//! blends the two along its antialiased edge. The clip and global alpha
//! apply as they do to a fill. Runs within one shader pass and down the
//! pyramid, on the screen and on an image target, opaque or translucent.
//! Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, FillRule, ImageFlags, Paint, Path};

mod common;
use common::{headless_device, render_on_image, render_rgba};

const W: u32 = 256;
const H: u32 = 192;
/// The rounded rect blurred: x, y, width, height and corner radius.
const SHAPE: [f32; 5] = [64.0, 48.0, 128.0, 96.0, 24.0];
/// The region `blur_path` blurs for `SHAPE`: its bounds and a pixel of
/// fringe. `blur_rect` over it reads the same footprint, so the pyramid's
/// halvings line up.
const REGION: [u32; 4] = [63, 47, 130, 98];
const SIGMAS: [f32; 3] = [3.0, 12.0, 20.0];

#[derive(Clone, Copy, Debug)]
enum Target {
    Screen,
    Image,
    /// An image whose white squares are transparent.
    TranslucentImage,
}

/// 8 px black and white squares, without antialiasing.
fn draw_checkerboard(canvas: &mut Canvas<WGPURenderer>) {
    let mut path = Path::new();
    for y in (0..H).step_by(8) {
        for x in (0..W).step_by(8) {
            if (x / 8 + y / 8) % 2 == 0 {
                path.rect(x as f32, y as f32, 8.0, 8.0);
            }
        }
    }
    let mut paint = Paint::color(Color::black());
    paint.set_anti_alias(false);
    canvas.fill_path(&path, &paint);
}

fn shape() -> Path {
    let [x, y, w, h, r] = SHAPE;
    let mut path = Path::new();
    path.rounded_rect(x, y, w, h, r);
    path
}

/// The signed distance from the center of pixel (x, y) to `SHAPE`'s outline,
/// negative inside.
fn shape_distance(x: u32, y: u32) -> f32 {
    let [sx, sy, w, h, r] = SHAPE;
    let (cx, cy) = (sx + w / 2.0, sy + h / 2.0);
    let qx = (x as f32 + 0.5 - cx).abs() - (w / 2.0 - r);
    let qy = (y as f32 + 0.5 - cy).abs() - (h / 2.0 - r);
    qx.max(qy).min(0.0) + qx.max(0.0).hypot(qy.max(0.0)) - r
}

/// Renders the checkerboard on `target`, then `draw`, and reads the screen
/// back.
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: Target,
    draw: impl FnOnce(&mut Canvas<WGPURenderer>),
) -> Vec<u8> {
    match target {
        Target::Screen => render_rgba(device, queue, W, H, Color::white(), |canvas| {
            draw_checkerboard(canvas);
            draw(canvas);
        }),
        Target::Image | Target::TranslucentImage => {
            let clear = match target {
                Target::TranslucentImage => Color::rgba(0, 0, 0, 0),
                _ => Color::white(),
            };
            render_on_image(device, queue, W, H, ImageFlags::empty(), clear, |canvas| {
                draw_checkerboard(canvas);
                draw(canvas);
            })
        }
    }
}

/// The checkerboard, and the checkerboard with `REGION` blurred by `sigma`.
fn references(device: &wgpu::Device, queue: &wgpu::Queue, target: Target, sigma: f32) -> (Vec<u8>, Vec<u8>) {
    let sharp = render(device, queue, target, |_| {});
    let blurred = render(device, queue, target, |canvas| {
        let [x, y, w, h] = REGION;
        canvas.blur_rect(x, y, w, h, sigma).expect("blur_rect");
    });
    (sharp, blurred)
}

fn pixel(out: &[u8], x: u32, y: u32) -> &[u8] {
    common::pixel(out, W, x, y)
}

fn assert_close(out: &[u8], expected: &[u8], x: u32, y: u32, tolerance: u8, what: &str) {
    let (got, want) = (pixel(out, x, y), pixel(expected, x, y));
    assert!(
        got.iter().zip(want).all(|(a, b)| a.abs_diff(*b) <= tolerance),
        "{what}: pixel ({x}, {y}) is {got:?}, expected {want:?}"
    );
}

/// The corners outside the radius keep the checkerboard, the inside is
/// blurred, and the arc's edge lies between the two.
#[test]
fn a_rounded_rect_blurs_inside_and_blends_along_its_edge() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for target in [Target::Screen, Target::Image, Target::TranslucentImage] {
        for sigma in SIGMAS {
            let what = format!("{target:?} sigma {sigma}");
            let (sharp, blurred) = references(&device, &queue, target, sigma);
            let out = render(&device, &queue, target, |canvas| {
                canvas.blur_path(&shape(), FillRule::NonZero, sigma).expect("blur_path");
            });
            let mut partial = 0;
            for y in 0..H {
                for x in 0..W {
                    let distance = shape_distance(x, y);
                    if distance >= 1.0 {
                        assert_close(&out, &sharp, x, y, 0, &what);
                    } else if distance <= -1.0 {
                        assert_close(&out, &blurred, x, y, 1, &what);
                    } else {
                        let (lo, hi) = (pixel(&sharp, x, y)[0], pixel(&blurred, x, y)[0]);
                        let (lo, hi) = (lo.min(hi), lo.max(hi));
                        let got = pixel(&out, x, y)[0];
                        assert!(
                            (lo.saturating_sub(1)..=hi.saturating_add(1)).contains(&got),
                            "{what}: edge pixel ({x}, {y}) is {got}, outside {lo}..={hi}"
                        );
                        if hi - lo > 32 && got > lo + 8 && got < hi - 8 {
                            partial += 1;
                        }
                    }
                }
            }
            assert!(partial > 0, "{what}: no edge pixel is partly blurred");
        }
    }
}

/// Under a clip, nothing outside it changes, and inside both the clip and
/// the path the pixels are blurred: a clip on the stencil, in a coverage
/// mask and as a shape.
#[test]
fn a_path_blur_changes_nothing_outside_the_clip() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    // A triangle, through (80, 40), (200, 100) and (80, 160).
    let mut triangle = Path::new();
    triangle.move_to(80.0, 40.0);
    triangle.line_to(200.0, 100.0);
    triangle.line_to(80.0, 160.0);
    triangle.close();
    // The signed distance to the triangle's sides, negative inside: the
    // largest of the distances past each side.
    let triangle_distance = |x: u32, y: u32| {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        [
            ((80.0, 40.0), (200.0, 100.0)),
            ((200.0, 100.0), (80.0, 160.0)),
            ((80.0, 160.0), (80.0, 40.0)),
        ]
        .into_iter()
        .map(|((ax, ay), (bx, by))| {
            let (dx, dy) = (bx - ax, by - ay);
            // Along the outward normal (dy, -dx) of a clockwise outline, y down.
            ((px - ax) * dy - (py - ay) * dx) / dx.hypot(dy)
        })
        .fold(f32::NEG_INFINITY, f32::max)
    };
    for sigma in [3.0, 12.0] {
        let (sharp, blurred) = references(&device, &queue, Target::Screen, sigma);
        for budget in [0, usize::MAX] {
            let what = format!("sigma {sigma}, mask budget {budget}");
            let out = render(&device, &queue, Target::Screen, |canvas| {
                canvas.set_clip_mask_budget(budget);
                canvas.clip_path(&triangle, FillRule::NonZero);
                canvas.blur_path(&shape(), FillRule::NonZero, sigma).expect("blur_path");
            });
            for y in 0..H {
                for x in 0..W {
                    let (clip, path) = (triangle_distance(x, y), shape_distance(x, y));
                    if clip >= 1.0 || path >= 1.0 {
                        assert_close(&out, &sharp, x, y, 0, &what);
                    } else if clip <= -1.0 && path <= -1.0 {
                        assert_close(&out, &blurred, x, y, 1, &what);
                    }
                }
            }
        }

        // A rect under the rounded rect as a clip is the rounded rect.
        let out = render(&device, &queue, Target::Screen, |canvas| {
            canvas.clip_path(&shape(), FillRule::NonZero);
            let [x, y, w, h] = REGION.map(|v| v as f32);
            let mut rect = Path::new();
            rect.rect(x - 8.0, y - 8.0, w + 16.0, h + 16.0);
            canvas.blur_path(&rect, FillRule::NonZero, sigma).expect("blur_path");
        });
        let what = format!("sigma {sigma}, rounded clip");
        for y in 0..H {
            for x in 0..W {
                let distance = shape_distance(x, y);
                if distance >= 1.0 {
                    assert_close(&out, &sharp, x, y, 0, &what);
                } else if distance <= -1.0 {
                    assert_close(&out, &blurred, x, y, 1, &what);
                }
            }
        }
    }
}

/// Global alpha 0.5 over the opaque checkerboard lands halfway between it
/// and the blur.
#[test]
fn global_alpha_scales_a_path_blur() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for sigma in [3.0, 12.0] {
        let (sharp, blurred) = references(&device, &queue, Target::Screen, sigma);
        let out = render(&device, &queue, Target::Screen, |canvas| {
            canvas.set_global_alpha(0.5);
            canvas.blur_path(&shape(), FillRule::NonZero, sigma).expect("blur_path");
        });
        for y in 0..H {
            for x in 0..W {
                if shape_distance(x, y) > -1.0 {
                    continue;
                }
                let (got, a, b) = (pixel(&out, x, y)[0], pixel(&sharp, x, y)[0], pixel(&blurred, x, y)[0]);
                let half = (a as f32 + b as f32) / 2.0;
                assert!(
                    (got as f32 - half).abs() <= 1.5,
                    "sigma {sigma}: pixel ({x}, {y}) is {got}, expected {half}"
                );
            }
        }
    }
}

/// A square turned by 45 degrees blurs inside its diamond and nowhere else,
/// and the transform's scale scales sigma.
#[test]
fn a_path_blur_follows_the_transform() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    // The diamond's bounds and a pixel of fringe, as blur_path takes them.
    let region = [98, 66, 60, 60];
    let (sharp, blurred) = (
        render(&device, &queue, Target::Screen, |_| {}),
        render(&device, &queue, Target::Screen, |canvas| {
            let [x, y, w, h] = region;
            canvas.blur_rect(x, y, w, h, 6.0).expect("blur_rect");
        }),
    );
    let out = render(&device, &queue, Target::Screen, |canvas| {
        canvas.translate(128.0, 96.0);
        canvas.rotate(std::f32::consts::FRAC_PI_4);
        canvas.scale(2.0, 2.0);
        let mut square = Path::new();
        square.rect(-10.0, -10.0, 20.0, 20.0);
        canvas.blur_path(&square, FillRule::NonZero, 3.0).expect("blur_path");
    });
    let half_diagonal = 20.0 * std::f32::consts::SQRT_2;
    for y in 0..H {
        for x in 0..W {
            // The diamond's distance, scaled to device pixels.
            let (dx, dy) = ((x as f32 + 0.5 - 128.0).abs(), (y as f32 + 0.5 - 96.0).abs());
            let distance = (dx + dy - half_diagonal) / std::f32::consts::SQRT_2;
            if distance >= 1.0 {
                assert_close(&out, &sharp, x, y, 0, "outside the diamond");
            } else if distance <= -1.0 {
                assert_close(&out, &blurred, x, y, 1, "inside the diamond");
            }
        }
    }
}

/// Paths that take the stencil: a star, whose fringe folds at each inner
/// corner, a ring with a hole and two rects a pixel apart, whose fringes
/// meet in the gap.
fn concave_paths() -> Vec<(&'static str, Path)> {
    let mut star = Path::new();
    for i in 0..10 {
        let angle = std::f32::consts::PI * i as f32 / 5.0;
        let radius = if i % 2 == 0 { 60.0 } else { 12.0 };
        let (x, y) = (128.0 + radius * angle.sin(), 96.0 - radius * angle.cos());
        if i == 0 {
            star.move_to(x, y);
        } else {
            star.line_to(x, y);
        }
    }
    star.close();
    let mut ring = Path::new();
    ring.rect(80.0, 50.0, 96.0, 90.0);
    ring.rect(83.0, 53.0, 90.0, 84.0);
    ring.solidity(femtovg::Solidity::Hole);
    let mut gap = Path::new();
    gap.rect(60.0, 40.0, 60.0, 100.0);
    gap.rect(121.0, 40.0, 60.0, 100.0);
    vec![("star", star), ("ring", ring), ("gap", gap)]
}

/// A flat color blurred is the color: through the stencil, under either
/// fill rule and at any alpha, each pixel is mixed toward the blur once.
#[test]
fn a_path_blur_of_a_flat_color_leaves_it_alone() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let flat = Color::rgba(128, 128, 128, 255);
    for (name, path) in concave_paths() {
        for fill_rule in [FillRule::NonZero, FillRule::EvenOdd] {
            for alpha in [1.0, 0.5] {
                let out = render_rgba(&device, &queue, W, H, flat, |canvas| {
                    canvas.set_global_alpha(alpha);
                    canvas.blur_path(&path, fill_rule, 3.0).expect("blur_path");
                });
                for y in 0..H {
                    for x in 0..W {
                        let got = pixel(&out, x, y);
                        assert!(
                            got[..3].iter().all(|v| v.abs_diff(128) <= 2),
                            "{name} {fill_rule:?} alpha {alpha}: pixel ({x}, {y}) is {got:?}"
                        );
                    }
                }
            }
        }
    }
}
