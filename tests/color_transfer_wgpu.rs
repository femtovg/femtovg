//! Headless GPU tests for the color-space transfer filters
//! (`ImageFilter::LinearRgbToSrgb` / `SrgbToLinearRgb`): the sRGB transfer
//! curve of IEC 61966-2-1, checked at known points, on alpha, and as a
//! round trip that the chain folds away.
#![cfg(feature = "wgpu")]

use femtovg::{Color, ImageFilter, ImageFlags, Paint, Path, PixelFormat, RenderTarget};

mod common;
use common::headless_device;

const W: u32 = 16;
const H: u32 = 16;

/// Fills a source image with `src`, runs `filters` as a chain into a target
/// under the documented chain convention, draws the target 1:1 onto a
/// transparent output and returns the centre pixel, premultiplied RGBA8.
fn chained_center(device: &wgpu::Device, queue: &wgpu::Queue, src: Color, filters: &[ImageFilter]) -> [u8; 4] {
    let px = common::render_rgba(device, queue, W, H, Color::rgba(0, 0, 0, 0), |canvas| {
        let source = canvas
            .create_image_empty(W as usize, H as usize, PixelFormat::Rgba8, ImageFlags::PREMULTIPLIED)
            .expect("source image");
        canvas.set_render_target(RenderTarget::Image(source));
        canvas.clear_rect(0, 0, W, H, src);
        canvas.set_render_target(RenderTarget::Screen);

        let target = canvas
            .create_image_empty(
                W as usize,
                H as usize,
                PixelFormat::Rgba8,
                ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y | ImageFlags::NEAREST,
            )
            .expect("target image");
        canvas.filter_image_chain(target, filters, source);

        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&p, &Paint::image(target, 0.0, 0.0, W as f32, H as f32, 0.0, 1.0));
    });
    let at = (((H / 2) * W + W / 2) * 4) as usize;
    [px[at], px[at + 1], px[at + 2], px[at + 3]]
}

fn srgb_to_linear(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(x: f64) -> f64 {
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

/// A straight color as the premultiplied RGBA8 the output holds.
fn rgba8(c: Color) -> [u8; 4] {
    let q = |v: f32| (v * 255.0).round() as u8;
    [q(c.r * c.a), q(c.g * c.a), q(c.b * c.a), q(c.a)]
}

fn close(got: u8, want: f64, what: &str) {
    let want = want * 255.0;
    assert!(
        (got as f64 - want).abs() <= 1.5,
        "{what}: got {got}, expected {want:.2}"
    );
}

/// Opaque mid-tones through each direction land on the curve.
#[test]
fn transfer_curves_match_iec_61966() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for (r, g, b) in [(0.5, 0.25, 0.75), (0.02, 0.9, 0.1), (0.0, 1.0, 0.5)] {
        let src = Color::rgbf(r, g, b);
        let to_srgb = chained_center(&device, &queue, src, &[ImageFilter::LinearRgbToSrgb]);
        close(to_srgb[0], linear_to_srgb(r as f64), "linear->sRGB r");
        close(to_srgb[1], linear_to_srgb(g as f64), "linear->sRGB g");
        close(to_srgb[2], linear_to_srgb(b as f64), "linear->sRGB b");
        assert_eq!(to_srgb[3], 255);

        let to_linear = chained_center(&device, &queue, src, &[ImageFilter::SrgbToLinearRgb]);
        close(to_linear[0], srgb_to_linear(r as f64), "sRGB->linear r");
        close(to_linear[1], srgb_to_linear(g as f64), "sRGB->linear g");
        close(to_linear[2], srgb_to_linear(b as f64), "sRGB->linear b");
        assert_eq!(to_linear[3], 255);
    }
    // The endpoints are fixed points of both curves.
    for c in [Color::black(), Color::white()] {
        let (a, b) = (
            chained_center(&device, &queue, c, &[ImageFilter::LinearRgbToSrgb]),
            chained_center(&device, &queue, c, &[ImageFilter::SrgbToLinearRgb]),
        );
        let want = rgba8(c);
        assert_eq!(a, want);
        assert_eq!(b, want);
    }
}

/// The curve applies to unpremultiplied color, so a half-transparent pixel
/// converts the same color as its opaque twin and keeps its alpha.
#[test]
fn transfer_is_on_unpremultiplied_color_and_keeps_alpha() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let opaque = chained_center(
        &device,
        &queue,
        Color::rgbf(0.5, 0.2, 0.8),
        &[ImageFilter::LinearRgbToSrgb],
    );
    let half = chained_center(
        &device,
        &queue,
        Color::rgbaf(0.5, 0.2, 0.8, 0.5),
        &[ImageFilter::LinearRgbToSrgb],
    );
    assert!((half[3] as i32 - 128).abs() <= 1, "alpha changed: {}", half[3]);
    for c in 0..3 {
        // Premultiplied output: half the opaque value, within rounding.
        let want = opaque[c] as f64 * half[3] as f64 / 255.0;
        assert!(
            (half[c] as f64 - want).abs() <= 2.0,
            "channel {c}: got {}, expected {want:.1} (opaque {})",
            half[c],
            opaque[c]
        );
    }
}

/// A transfer followed by its inverse is folded to the identity pass, so the
/// input comes back exactly - no 8-bit round trip through linear space.
#[test]
fn transfer_round_trip_folds_to_identity() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let src = Color::rgbf(0.02, 0.5, 0.97);
    let want = rgba8(src);
    let a = chained_center(
        &device,
        &queue,
        src,
        &[ImageFilter::SrgbToLinearRgb, ImageFilter::LinearRgbToSrgb],
    );
    let b = chained_center(
        &device,
        &queue,
        src,
        &[ImageFilter::LinearRgbToSrgb, ImageFilter::SrgbToLinearRgb],
    );
    assert_eq!(a, want);
    assert_eq!(b, want);
    // The fold is the documented no-op, not a coincidence of the shader.
    assert!(matches!(
        ImageFilter::SrgbToLinearRgb.fold_with(ImageFilter::LinearRgbToSrgb),
        Some(ImageFilter::ColorMatrix { .. })
    ));
    assert!(ImageFilter::LinearRgbToSrgb
        .fold_with(ImageFilter::LinearRgbToSrgb)
        .is_none());
}
