//! Headless GPU tests for a layer's blend mode (`LayerEffects::with_blend`):
//! every mode against the specification, opacity before the blend, the
//! backdrop from the enclosing layer, a chain and a mask in front, the
//! composite operation and scissor, and the source-over fallbacks (screen,
//! budget). Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{
    imgref::Img, renderer::WGPURenderer, rgb::RGBA8, BlendMode, Canvas, Color, CompositeOperation, ImageFilter,
    ImageFlags, ImageId, LayerEffects, MaskKind, Paint, Path,
};

mod common;
use common::blend::{backdrop_pixel, expected, source_pixel, ALL_MODES};
use common::{headless_device, render_rgba};

const W: u32 = 16;
const H: u32 = 16;

type C = Canvas<WGPURenderer>;

/// Uploads a premultiplied RGBA8 image whose pixel (x, y) is `f(x, y)`.
fn upload(canvas: &mut C, w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> ImageId {
    let f = &f;
    let pixels: Vec<RGBA8> = (0..h)
        .flat_map(|y| (0..w).map(move |x| f(x, y)))
        .map(|[r, g, b, a]| RGBA8::new(r, g, b, a))
        .collect();
    canvas
        .create_image(
            Img::new(pixels.as_slice(), w as usize, h as usize),
            ImageFlags::PREMULTIPLIED,
        )
        .expect("upload")
}

/// Draws `image` 1:1 at (x, y), without antialiasing.
fn blit(canvas: &mut C, image: ImageId, x: f32, y: f32, w: f32, h: f32) {
    let mut p = Path::new();
    p.rect(x, y, w, h);
    let mut paint = Paint::image(image, x, y, w, h, 0.0, 1.0);
    paint.set_anti_alias(false);
    canvas.fill_path(&p, &paint);
}

fn fill(canvas: &mut C, x: f32, y: f32, w: f32, h: f32, color: Color) {
    let mut p = Path::new();
    p.rect(x, y, w, h);
    let mut paint = Paint::color(color);
    paint.set_anti_alias(false);
    canvas.fill_path(&p, &paint);
}

fn px(buf: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

fn assert_close(got: [u8; 4], want: [u8; 4], what: &str) {
    let worst = (0..4).map(|i| (got[i] as i32 - want[i] as i32).abs()).max().unwrap();
    assert!(worst <= 2, "{what}: {got:?}, expected {want:?}");
}

fn gray() -> Color {
    Color::rgb(128, 128, 128)
}

fn red() -> Color {
    Color::rgb(255, 0, 0)
}

/// The largest channel difference over the image between `out` and `want`.
fn worst_delta(out: &[u8], want: impl Fn(u32, u32) -> [u8; 4]) -> i32 {
    let mut worst = 0;
    for y in 0..H {
        for x in 0..W {
            let (got, want) = (px(out, W, x, y), want(x, y));
            for i in 0..4 {
                worst = worst.max((got[i] as i32 - want[i] as i32).abs());
            }
        }
    }
    worst
}

/// Varying source over varying backdrop, the backdrop in an enclosing plain
/// layer (the screen cannot be read), the source blended with `mode`.
fn composite(mode: BlendMode, opacity: f32) -> Vec<u8> {
    let (device, queue) = headless_device().expect("gpu");
    render_rgba(&device, &queue, W, H, Color::rgba(0, 0, 0, 0), |c| {
        let backdrop = upload(c, W, H, backdrop_pixel);
        let source = upload(c, W, H, source_pixel);
        assert!(c.begin_layer(&LayerEffects::new()));
        blit(c, backdrop, 0.0, 0.0, W as f32, H as f32);
        assert!(c.begin_layer(&LayerEffects::new().with_blend(mode).with_opacity(opacity)));
        blit(c, source, 0.0, 0.0, W as f32, H as f32);
        c.end_layer();
        c.end_layer();
    })
}

/// Every mode, every pixel, within rounding of the specification.
#[test]
fn every_mode_composites_like_the_specification() {
    if headless_device().is_none() {
        return;
    }
    for mode in ALL_MODES {
        let out = composite(mode, 1.0);
        let worst = worst_delta(&out, |x, y| expected(mode, source_pixel(x, y), backdrop_pixel(x, y)));
        assert!(worst <= 4, "{mode:?}: worst channel delta {worst}/255");
    }
}

/// Opacity scales the source before the blend (Skia, Chromium, WebKit).
/// The orders differ by `cb * opacity * (1 - as) * (1 - ab)`, so both sides
/// here are translucent.
#[test]
fn opacity_applies_before_the_blend() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let mode = BlendMode::ColorBurn;
    let opacity = 0.5;
    // Premultiplied: a source of color (0.9, 0.6, 0.3) at alpha 0.1, a
    // backdrop of color (1, 0.5, 0.2) at alpha 0.5.
    let source = [23u8, 15, 8, 26];
    let backdrop = [128u8, 64, 26, 128];
    let faded = |p: [u8; 4]| p.map(|v| (v as f64 * opacity).round() as u8);
    let right = expected(mode, faded(source), backdrop);
    // The other order: the blend at full opacity, faded over the backdrop.
    let wrong = {
        let full = faded(expected(mode, source, backdrop));
        let cover = 1.0 - full[3] as f64 / 255.0;
        [0, 1, 2, 3].map(|i| (full[i] as f64 + backdrop[i] as f64 * cover).round() as u8)
    };
    let apart = (0..4).map(|i| (right[i] as i32 - wrong[i] as i32).abs()).max().unwrap();
    assert!(apart > 16, "the two orders must be told apart: {apart}/255");
    let out = render_rgba(&device, &queue, W, H, Color::rgba(0, 0, 0, 0), |c| {
        let backdrop = upload(c, W, H, |_, _| backdrop);
        let source = upload(c, W, H, |_, _| source);
        assert!(c.begin_layer(&LayerEffects::new()));
        blit(c, backdrop, 0.0, 0.0, W as f32, H as f32);
        assert!(c.begin_layer(&LayerEffects::new().with_blend(mode).with_opacity(opacity as f32)));
        blit(c, source, 0.0, 0.0, W as f32, H as f32);
        c.end_layer();
        c.end_layer();
    });
    let worst = worst_delta(&out, |_, _| right);
    assert!(worst <= 4, "opacity before the blend: worst channel delta {worst}/255");
}

/// A blend inside a layer reads that layer's store at its own origin:
/// green under gray multiplies to dark green, while a copy from the white
/// canvas, or from the wrong place, would be gray.
#[test]
fn the_backdrop_is_the_enclosing_layer_at_its_origin() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render_rgba(&device, &queue, 64, 64, Color::white(), |c| {
        c.scissor(20.0, 10.0, 40.0, 40.0);
        assert!(c.begin_layer(&LayerEffects::new()));
        fill(c, 20.0, 10.0, 40.0, 40.0, Color::rgb(0, 255, 0));
        c.scissor(30.0, 20.0, 20.0, 20.0);
        assert!(c.begin_layer(&LayerEffects::new().with_blend(BlendMode::Multiply)));
        fill(c, 30.0, 20.0, 20.0, 20.0, gray());
        c.end_layer();
        c.end_layer();
    });
    assert_close(px(&out, 64, 35, 25), [0, 128, 0, 255], "green multiplied by gray");
    assert_close(px(&out, 64, 45, 35), [0, 128, 0, 255], "green multiplied by gray");
    assert_close(px(&out, 64, 25, 15), [0, 255, 0, 255], "outside the blending layer");
    assert_close(px(&out, 64, 5, 5), [255, 255, 255, 255], "outside the enclosing layer");
}

/// Red over blue, white over the top half only: multiply leaves red on top
/// and the blue untouched, for both storage orientations the pass sees.
fn halves(with_chain: bool) -> Vec<u8> {
    let (device, queue) = headless_device().expect("gpu");
    render_rgba(&device, &queue, W, H, Color::rgba(0, 0, 0, 0), |c| {
        assert!(c.begin_layer(&LayerEffects::new()));
        fill(c, 0.0, 0.0, W as f32, H as f32 / 2.0, red());
        fill(c, 0.0, H as f32 / 2.0, W as f32, H as f32 / 2.0, Color::rgb(0, 0, 255));
        let mut effects = LayerEffects::new().with_blend(BlendMode::Multiply);
        if with_chain {
            effects = effects.with_filters(&[ImageFilter::identity()]);
        }
        assert!(c.begin_layer(&effects));
        fill(c, 0.0, 0.0, W as f32, H as f32 / 2.0, Color::white());
        c.end_layer();
        c.end_layer();
    })
}

#[test]
fn the_blend_lands_upright_with_and_without_a_chain() {
    if headless_device().is_none() {
        return;
    }
    for with_chain in [false, true] {
        let out = halves(with_chain);
        assert_close(px(&out, W, 8, 2), [255, 0, 0, 255], "top: white multiplied by red");
        assert_close(px(&out, W, 8, 13), [0, 0, 255, 255], "bottom: the backdrop untouched");
    }
}

/// Masked-out pixels show the backdrop unblended.
#[test]
fn a_mask_applies_before_the_blend() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render_rgba(&device, &queue, W, H, Color::rgba(0, 0, 0, 0), |c| {
        let mask = upload(c, W, H, |x, _| if x < W / 2 { [255; 4] } else { [0; 4] });
        assert!(c.begin_layer(&LayerEffects::new()));
        fill(c, 0.0, 0.0, W as f32, H as f32, red());
        let effects = LayerEffects::new().with_blend(BlendMode::Multiply).with_mask(
            mask,
            MaskKind::Alpha,
            0.0,
            0.0,
            W as f32,
            H as f32,
        );
        assert!(c.begin_layer(&effects));
        fill(c, 0.0, 0.0, W as f32, H as f32, gray());
        c.end_layer();
        c.end_layer();
    });
    assert_close(px(&out, W, 3, 8), [128, 0, 0, 255], "under the mask: multiplied");
    assert_close(px(&out, W, 12, 8), [255, 0, 0, 255], "masked out: the backdrop");
}

/// A blend mode is the composite operation: destination-over in effect at
/// `begin_layer` is not applied on top of it.
#[test]
fn the_composite_operation_is_the_blend() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render_rgba(&device, &queue, W, H, Color::rgba(0, 0, 0, 0), |c| {
        assert!(c.begin_layer(&LayerEffects::new()));
        fill(c, 0.0, 0.0, W as f32, H as f32, red());
        c.global_composite_operation(CompositeOperation::DestinationOver);
        assert!(c.begin_layer(&LayerEffects::new().with_blend(BlendMode::Multiply)));
        fill(c, 0.0, 0.0, W as f32, H as f32, gray());
        c.end_layer();
        c.global_composite_operation(CompositeOperation::SourceOver);
        c.end_layer();
    });
    assert_close(px(&out, W, 8, 8), [128, 0, 0, 255], "multiplied, not put underneath");
}

/// The outer scissor bounds the composite and places the backdrop copy.
#[test]
fn the_outer_scissor_bounds_the_blend() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render_rgba(&device, &queue, W, H, Color::rgba(0, 0, 0, 0), |c| {
        assert!(c.begin_layer(&LayerEffects::new()));
        fill(c, 0.0, 0.0, W as f32, H as f32, red());
        c.scissor(8.0, 0.0, 8.0, H as f32);
        assert!(c.begin_layer(&LayerEffects::new().with_blend(BlendMode::Multiply)));
        fill(c, 0.0, 0.0, W as f32, H as f32, gray());
        c.end_layer();
        c.end_layer();
    });
    assert_close(px(&out, W, 3, 8), [255, 0, 0, 255], "outside the scissor: the backdrop");
    assert_close(px(&out, W, 12, 8), [128, 0, 0, 255], "inside: multiplied");
}

/// On the screen the backdrop cannot be read: source-over at the opacity.
#[test]
fn on_the_screen_the_layer_composites_source_over() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render_rgba(&device, &queue, W, H, Color::white(), |c| {
        fill(c, 0.0, 0.0, W as f32, H as f32, red());
        assert!(c.begin_layer(&LayerEffects::new().with_blend(BlendMode::Multiply).with_opacity(0.5)));
        fill(c, 0.0, 0.0, W as f32, H as f32, gray());
        c.end_layer();
    });
    assert_close(px(&out, W, 8, 8), [191, 64, 64, 255], "gray at half opacity over red");
}

/// Gray multiplied over red in a plain layer, the budget `stores` stores.
fn budget_case(stores: usize) -> [u8; 4] {
    let (device, queue) = headless_device().expect("gpu");
    let out = render_rgba(&device, &queue, W, H, Color::rgba(0, 0, 0, 0), |c| {
        assert!(c.begin_layer(&LayerEffects::new()));
        let store = c.transient_image_bytes();
        assert!(store > 0, "the enclosing layer holds its store");
        c.set_transient_image_budget(store * stores);
        fill(c, 0.0, 0.0, W as f32, H as f32, red());
        assert!(c.begin_layer(&LayerEffects::new().with_blend(BlendMode::Multiply)));
        fill(c, 0.0, 0.0, W as f32, H as f32, gray());
        c.end_layer();
        c.end_layer();
    });
    px(&out, W, 8, 8)
}

/// Without room for the backdrop copy and the result (each reserved with a
/// capture's headroom), source-over; what was reserved first is given back.
#[test]
fn a_blend_the_budget_cannot_fit_composites_source_over() {
    if headless_device().is_none() {
        return;
    }
    assert_close(budget_case(5), [128, 0, 0, 255], "five stores: multiplied");
    assert_close(budget_case(4), [128, 128, 128, 255], "four: the result does not fit");
    assert_close(budget_case(3), [128, 128, 128, 255], "three: the backdrop does not fit");
}
