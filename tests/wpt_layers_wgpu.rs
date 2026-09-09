//! Ports of the web-platform-tests Canvas 2D layer suite
//! (`html/canvas/tools/yaml/layers.yaml`) that femtovg can express: each
//! WPT case draws once through `begin_layer`/`end_layer` and once the way its
//! `reference:` script does - through an offscreen canvas and `drawImage`,
//! which here is an image render target and an image paint - and the two
//! must agree pixel for pixel (WPT's fuzzy tolerances where it grants them).
//! Cases the crate cannot express are listed at the bottom with the reason.
#![cfg(feature = "wgpu")]

use femtovg::{
    renderer::WGPURenderer, Canvas, Color, CompositeOperation, ImageFilter, ImageFlags, ImageId, LayerEffects, Paint,
    Path, PixelFormat, RenderTarget, Transform2D,
};

mod common;
use common::headless_device;

type C = Canvas<WGPURenderer>;

fn render(device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, draw: impl FnOnce(&mut C)) -> Vec<u8> {
    // A WPT canvas starts transparent; readbacks here are premultiplied RGBA8.
    common::render_rgba(device, queue, w, h, Color::rgba(0, 0, 0, 0), draw)
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::rgb(r, g, b)
}

fn fill_rect(c: &mut C, x: f32, y: f32, w: f32, h: f32, color: Color) {
    let mut p = Path::new();
    p.rect(x, y, w, h);
    c.fill_path(&p, &Paint::color(color));
}

/// The suite's reference idiom: draw into a fresh transparent canvas of the
/// frame's size under default state, then `drawImage` it at (dx, dy) under
/// the *current* state - alpha, composite, shadow and transform all apply to
/// the image, as they do to a layer's result.
fn offscreen(c: &mut C, w: u32, h: u32, draw: impl FnOnce(&mut C)) -> ImageId {
    let image = c
        .create_image_empty(
            w as usize,
            h as usize,
            PixelFormat::Rgba8,
            ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y,
        )
        .expect("offscreen image");
    c.save();
    c.set_render_target(RenderTarget::Image(image));
    c.clear_rect(0, 0, w, h, Color::rgba(0, 0, 0, 0));
    c.reset();
    draw(c);
    c.set_render_target(RenderTarget::Screen);
    c.restore();
    image
}

fn draw_image(c: &mut C, image: ImageId, dx: f32, dy: f32, w: u32, h: u32) {
    let mut p = Path::new();
    p.rect(dx, dy, w as f32, h as f32);
    c.fill_path(&p, &Paint::image(image, dx, dy, w as f32, h as f32, 0.0, 1.0));
}

fn shadow(c: &mut C, dx: f32, dy: f32, blur: f32, color: Color) {
    c.set_shadow_offset(dx, dy);
    c.set_shadow_blur(blur);
    c.set_shadow_color(color);
}

/// Compares two renders: no channel may differ by more than `tolerance`,
/// except on at most `fuzzy_pixels` pixels (WPT's `fuzzy: maxDifference;
/// totalPixels` allowance, used where its own references need it).
fn assert_matches(name: &str, code: &[u8], reference: &[u8], tolerance: i32, fuzzy_pixels: usize) {
    assert_eq!(code.len(), reference.len());
    let mut worst = 0;
    let mut over = 0usize;
    for (px, (a, b)) in code.chunks_exact(4).zip(reference.chunks_exact(4)).enumerate() {
        let d = (0..4).map(|i| (a[i] as i32 - b[i] as i32).abs()).max().unwrap();
        worst = worst.max(d);
        if d > tolerance {
            over += 1;
            if over <= 3 {
                eprintln!("{name}: pixel {px} code {a:?} reference {b:?}");
            }
        }
    }
    assert!(
        over <= fuzzy_pixels,
        "{name}: {over} pixels differ by more than {tolerance}/255 (worst {worst}); allowed {fuzzy_pixels}"
    );
}

/// 2d.layer.global-states: alpha, composite and shadow set outside the layer
/// apply to the layer as a whole, exactly as they apply to a drawImage of the
/// same content. Draws inside use a non-source-over composite to prove they
/// do not composite with the backdrop one by one. (WPT's inner op is
/// `screen`; femtovg has Porter-Duff only, so both sides use `lighter`.)
#[test]
fn wpt_global_states() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let alphas = [None, Some(0.75f32)];
    let composites = [None, Some(CompositeOperation::SourceIn), Some(CompositeOperation::Copy)];
    let shadows = [false, true];
    let rotations = [false, true];
    for &alpha in &alphas {
        for &composite in &composites {
            for &shadowed in &shadows {
                for &rotated in &rotations {
                    let setup = move |c: &mut C| {
                        if rotated {
                            c.translate(45.0, 45.0);
                            c.rotate(0.6);
                            c.translate(-45.0, -45.0);
                        }
                        fill_rect(c, 20.0, 15.0, 50.0, 50.0, rgb(128, 128, 128));
                        if let Some(a) = alpha {
                            c.set_global_alpha(a);
                        }
                        if let Some(op) = composite {
                            c.global_composite_operation(op);
                        }
                        if shadowed {
                            shadow(c, -7.0, 7.0, 0.0, Color::rgba(255, 165, 0, 128));
                        }
                    };
                    let content = |c: &mut C| {
                        c.global_composite_operation(CompositeOperation::Lighter);
                        fill_rect(c, 10.0, 25.0, 40.0, 45.0, rgb(255, 0, 0));
                        fill_rect(c, 30.0, 5.0, 45.0, 40.0, rgb(0, 255, 0));
                    };
                    let code = render(&device, &queue, 90, 90, |c| {
                        setup(c);
                        assert!(c.begin_layer(&LayerEffects::new()));
                        content(c);
                        c.end_layer();
                    });
                    let reference = render(&device, &queue, 90, 90, |c| {
                        let image = offscreen(c, 90, 90, content);
                        setup(c);
                        draw_image(c, image, 0.0, 0.0, 90, 90);
                    });
                    let name = format!(
                        "global-states alpha={alpha:?} composite={composite:?} shadow={shadowed} rotated={rotated}"
                    );
                    // Rotation resamples the image reference; the layer composites in device space.
                    let (tol, fuzzy) = if rotated { (48, 90 * 90 / 20) } else { (2, 0) };
                    assert_matches(&name, &code, &reference, tol, fuzzy);
                }
            }
        }
    }
}

/// 2d.layer.globalCompositeOperation: every Porter-Duff operation applies to
/// the layer as a whole, under a rotated, scaled transform, global alpha and
/// a shadow. (WPT's multiply/screen/overlay/darken/lighten variants need
/// separable blend modes femtovg does not have - femtovg/femtovg#332.)
#[test]
fn wpt_global_composite_operation() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    use CompositeOperation::*;
    for op in [
        SourceOver,
        SourceIn,
        SourceOut,
        Atop,
        DestinationOver,
        DestinationIn,
        DestinationOut,
        DestinationAtop,
        Lighter,
        Copy,
        Xor,
    ] {
        let setup = move |c: &mut C| {
            c.translate(50.0, 50.0);
            c.scale(2.0, 2.0);
            c.rotate(std::f32::consts::PI);
            c.translate(-25.0, -25.0);
            fill_rect(c, 15.0, 15.0, 25.0, 25.0, Color::rgba(0, 0, 255, 204));
            c.set_global_alpha(0.75);
            c.global_composite_operation(op);
            shadow(c, 7.0, 7.0, 0.0, Color::rgba(255, 165, 0, 128));
        };
        let content = |c: &mut C| {
            fill_rect(c, 10.0, 25.0, 25.0, 20.0, rgb(204, 0, 0));
            fill_rect(c, 25.0, 10.0, 20.0, 25.0, rgb(0, 204, 0));
        };
        let code = render(&device, &queue, 90, 90, |c| {
            setup(c);
            assert!(c.begin_layer(&LayerEffects::new()));
            content(c);
            c.end_layer();
        });
        let reference = render(&device, &queue, 90, 90, |c| {
            let image = offscreen(c, 90, 90, content);
            setup(c);
            draw_image(c, image, 0.0, 0.0, 90, 90);
        });
        // The reference resamples a rotated, 2x scaled image (WPT disables
        // smoothing for it); edges may differ by a pixel ring.
        assert_matches(
            &format!("globalCompositeOperation {op:?}"),
            &code,
            &reference,
            48,
            90 * 90 / 15,
        );
    }
}

/// 2d.layer.restore-style: fill style set inside a layer does not leak out.
#[test]
fn wpt_restore_style() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let code = render(&device, &queue, 200, 200, |c| {
        fill_rect(c, 50.0, 50.0, 75.0, 50.0, rgb(0, 0, 255));
        c.set_global_alpha(0.5);
        assert!(c.begin_layer(&LayerEffects::new()));
        fill_rect(c, 60.0, 60.0, 75.0, 50.0, rgb(225, 0, 0));
        c.end_layer();
        fill_rect(c, 70.0, 70.0, 75.0, 50.0, rgb(0, 0, 255));
    });
    let reference = render(&device, &queue, 200, 200, |c| {
        fill_rect(c, 50.0, 50.0, 75.0, 50.0, rgb(0, 0, 255));
        c.set_global_alpha(0.5);
        let image = offscreen(c, 200, 200, |c| fill_rect(c, 60.0, 60.0, 75.0, 50.0, rgb(225, 0, 0)));
        draw_image(c, image, 0.0, 0.0, 200, 200);
        fill_rect(c, 70.0, 70.0, 75.0, 50.0, rgb(0, 0, 255));
    });
    assert_matches("restore-style", &code, &reference, 1, 950);
}

/// 2d.layer.nested: a layer inside a layer, each with its own alpha, under a
/// source-in composite set outside both.
#[test]
fn wpt_nested() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let circle = |c: &mut C| {
        let mut p = Path::new();
        p.circle(90.0, 90.0, 40.0);
        c.fill_path(&p, &Paint::color(rgb(0, 0, 0)));
        c.global_composite_operation(CompositeOperation::SourceIn);
    };
    let inner = |c: &mut C| {
        fill_rect(c, 50.0, 50.0, 75.0, 50.0, rgb(225, 0, 0));
        fill_rect(c, 70.0, 70.0, 75.0, 50.0, rgb(0, 255, 0));
    };
    let code = render(&device, &queue, 200, 200, |c| {
        circle(c);
        assert!(c.begin_layer(&LayerEffects::new()));
        fill_rect(c, 60.0, 60.0, 75.0, 50.0, rgb(0, 0, 255));
        c.set_global_alpha(0.5);
        assert!(c.begin_layer(&LayerEffects::new()));
        inner(c);
        c.end_layer();
        c.end_layer();
    });
    let reference = render(&device, &queue, 200, 200, |c| {
        circle(c);
        let canvas3 = offscreen(c, 200, 200, inner);
        let canvas2 = offscreen(c, 200, 200, |c| {
            fill_rect(c, 60.0, 60.0, 75.0, 50.0, rgb(0, 0, 255));
            c.set_global_alpha(0.5);
            draw_image(c, canvas3, 0.0, 0.0, 200, 200);
        });
        draw_image(c, canvas2, 0.0, 0.0, 200, 200);
    });
    assert_matches("nested", &code, &reference, 2, 0);
}

/// 2d.layer.several-complex: five consecutive layers under alpha, a blurred
/// shadow and no filter; each is one image with one shadow.
#[test]
fn wpt_several_complex() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let setup = |c: &mut C| {
        fill_rect(c, 50.0, 50.0, 95.0, 70.0, rgb(0, 0, 255));
        c.set_global_alpha(0.5);
        shadow(c, -10.0, 10.0, 3.0, rgb(255, 165, 0));
    };
    let content = |c: &mut C, i: f32| {
        fill_rect(c, 60.0 + i, 40.0 + i, 75.0, 50.0, rgb(225, 0, 0));
        fill_rect(c, 80.0 + i, 60.0 + i, 75.0, 50.0, rgb(0, 255, 0));
    };
    let code = render(&device, &queue, 500, 500, |c| {
        setup(c);
        for i in 0..5 {
            assert!(c.begin_layer(&LayerEffects::new()));
            content(c, i as f32);
            c.end_layer();
        }
    });
    let reference = render(&device, &queue, 500, 500, |c| {
        let images: Vec<ImageId> = (0..5)
            .map(|i| offscreen(c, 500, 500, |c| content(c, i as f32)))
            .collect();
        setup(c);
        for image in images {
            draw_image(c, image, 0.0, 0.0, 500, 500);
        }
    });
    assert_matches("several-complex", &code, &reference, 3, 6318);
}

/// 2d.layer.clearRect.partial / .full: clearRect inside a layer clears the
/// layer's content, not the backdrop.
#[test]
fn wpt_clear_rect_in_layer() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let partial = render(&device, &queue, 100, 100, |c| {
        fill_rect(c, 10.0, 10.0, 80.0, 50.0, rgb(0, 0, 255));
        assert!(c.begin_layer(&LayerEffects::new()));
        fill_rect(c, 20.0, 20.0, 80.0, 50.0, rgb(255, 0, 0));
        c.clear_rect(30, 30, 60, 30, Color::rgba(0, 0, 0, 0));
        c.end_layer();
    });
    let partial_reference = render(&device, &queue, 100, 100, |c| {
        fill_rect(c, 10.0, 10.0, 80.0, 50.0, rgb(0, 0, 255));
        fill_rect(c, 20.0, 20.0, 80.0, 10.0, rgb(255, 0, 0));
        fill_rect(c, 20.0, 60.0, 80.0, 10.0, rgb(255, 0, 0));
        fill_rect(c, 20.0, 20.0, 10.0, 50.0, rgb(255, 0, 0));
        fill_rect(c, 90.0, 20.0, 10.0, 50.0, rgb(255, 0, 0));
    });
    assert_matches("clearRect.partial", &partial, &partial_reference, 1, 0);

    let full = render(&device, &queue, 100, 100, |c| {
        fill_rect(c, 10.0, 10.0, 80.0, 50.0, rgb(0, 0, 255));
        assert!(c.begin_layer(&LayerEffects::new()));
        fill_rect(c, 20.0, 20.0, 80.0, 50.0, rgb(255, 0, 0));
        c.clear_rect(0, 0, 100, 100, Color::rgba(0, 0, 0, 0));
        c.end_layer();
    });
    let full_reference = render(&device, &queue, 100, 100, |c| {
        fill_rect(c, 10.0, 10.0, 80.0, 50.0, rgb(0, 0, 255));
    });
    assert_matches("clearRect.full", &full, &full_reference, 1, 0);
}

/// 2d.layer.ctm.setTransform / resetTransform: transforms set inside nested
/// layers are absolute, and the outer transform returns after the layers.
#[test]
fn wpt_ctm_set_and_reset_transform() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let code = render(&device, &queue, 200, 200, |c| {
        c.translate(80.0, 0.0);
        assert!(c.begin_layer(&LayerEffects::new()));
        c.rotate(2.0);
        assert!(c.begin_layer(&LayerEffects::new()));
        c.scale(5.0, 6.0);
        c.reset_transform();
        c.set_transform(&Transform2D::new(4.0, 0.0, 0.0, 2.0, 20.0, 10.0));
        fill_rect(c, 0.0, 0.0, 10.0, 10.0, rgb(0, 0, 255));
        c.end_layer();
        c.end_layer();
        fill_rect(c, 0.0, 0.0, 20.0, 20.0, rgb(0, 128, 0));
    });
    let reference = render(&device, &queue, 200, 200, |c| {
        c.translate(80.0, 0.0);
        fill_rect(c, 0.0, 0.0, 20.0, 20.0, rgb(0, 128, 0));
        c.reset_transform();
        c.set_transform(&Transform2D::new(4.0, 0.0, 0.0, 2.0, 20.0, 10.0));
        fill_rect(c, 0.0, 0.0, 10.0, 10.0, rgb(0, 0, 255));
    });
    assert_matches("ctm.setTransform", &code, &reference, 1, 0);

    let code = render(&device, &queue, 200, 200, |c| {
        c.translate(40.0, 0.0);
        assert!(c.begin_layer(&LayerEffects::new()));
        c.rotate(2.0);
        assert!(c.begin_layer(&LayerEffects::new()));
        c.scale(5.0, 6.0);
        c.reset_transform();
        fill_rect(c, 0.0, 0.0, 20.0, 20.0, rgb(0, 0, 255));
        c.end_layer();
        c.end_layer();
        fill_rect(c, 0.0, 0.0, 20.0, 20.0, rgb(0, 128, 0));
    });
    let reference = render(&device, &queue, 200, 200, |c| {
        fill_rect(c, 0.0, 0.0, 20.0, 20.0, rgb(0, 0, 255));
        c.translate(40.0, 0.0);
        fill_rect(c, 0.0, 0.0, 20.0, 20.0, rgb(0, 128, 0));
    });
    assert_matches("ctm.resetTransform", &code, &reference, 1, 0);
}

/// 2d.layer.ctm.ctx-filter: a drop-shadow set outside a layer shadows the
/// layer's result in device space regardless of the transform; one set
/// inside shadows each draw. The shadow state is femtovg's `drop-shadow`.
#[test]
fn wpt_ctm_drop_shadow_in_transformed_layers() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let code = render(&device, &queue, 200, 200, |c| {
        c.translate(30.0, 90.0);
        c.scale(2.0, 2.0);
        c.rotate(std::f32::consts::FRAC_PI_2);
        c.save();
        shadow(c, 10.0, 10.0, 0.0, rgb(255, 0, 0));
        fill_rect(c, -30.0, -5.0, 60.0, 10.0, rgb(128, 128, 128));
        c.restore();
        c.save();
        shadow(c, 10.0, 10.0, 0.0, rgb(0, 128, 0));
        assert!(c.begin_layer(&LayerEffects::new()));
        fill_rect(c, -30.0, -25.0, 60.0, 10.0, rgb(128, 128, 128));
        c.end_layer();
        c.restore();
        assert!(c.begin_layer(&LayerEffects::new()));
        shadow(c, 5.0, 5.0, 0.0, rgb(0, 0, 255));
        fill_rect(c, -30.0, -45.0, 60.0, 10.0, rgb(128, 128, 128));
        c.end_layer();
    });
    let reference = render(&device, &queue, 200, 200, |c| {
        fill_rect(c, 30.0, 40.0, 20.0, 120.0, rgb(255, 0, 0));
        fill_rect(c, 20.0, 30.0, 20.0, 120.0, rgb(128, 128, 128));
        fill_rect(c, 70.0, 40.0, 20.0, 120.0, rgb(0, 128, 0));
        fill_rect(c, 60.0, 30.0, 20.0, 120.0, rgb(128, 128, 128));
        fill_rect(c, 105.0, 35.0, 20.0, 120.0, rgb(0, 0, 255));
        fill_rect(c, 100.0, 30.0, 20.0, 120.0, rgb(128, 128, 128));
    });
    assert_matches("ctm.ctx-filter (drop shadows)", &code, &reference, 2, 400);
}

/// 2d.layer.ctm.shadow-in-transformed-layer: a shadow set inside a
/// transformed layer applies per draw, image draws included, exactly as it
/// would without the layer.
#[test]
fn wpt_ctm_shadow_in_transformed_layer() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let scene = |c: &mut C, layered: bool| {
        let sprite = offscreen(c, 100, 100, |c| fill_rect(c, 0.0, 0.0, 40.0, 10.0, rgb(0, 0, 255)));
        c.translate(80.0, 90.0);
        c.scale(2.0, 2.0);
        c.rotate(std::f32::consts::FRAC_PI_2);
        if layered {
            assert!(c.begin_layer(&LayerEffects::new()));
        }
        shadow(c, 10.0, 10.0, 0.0, rgb(128, 128, 128));
        fill_rect(c, -30.0, -5.0, 60.0, 10.0, rgb(0, 0, 0));
        draw_image(c, sprite, -30.0, -30.0, 100, 100);
        if layered {
            c.end_layer();
        }
    };
    let code = render(&device, &queue, 200, 200, |c| scene(c, true));
    let reference = render(&device, &queue, 200, 200, |c| scene(c, false));
    assert_matches("ctm.shadow-in-transformed-layer", &code, &reference, 2, 0);
}

/// 2d.layer.clip-outside / clip-inside / clip-inside-and-outside with a
/// rect clip (a scissor here): a clip set outside the layer clips the
/// filtered result, one set inside clips the content before the filter.
#[test]
fn wpt_clip_and_filtered_layers() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let blur = LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 12.0 }]);
    let clip = |c: &mut C| c.intersect_scissor(15.0, 15.0, 70.0, 70.0);

    // Outside: clip, then a blurred layer of an unclipped rect.
    let code = render(&device, &queue, 100, 100, |c| {
        clip(c);
        assert!(c.begin_layer(&blur));
        fill_rect(c, 10.0, 10.0, 80.0, 80.0, rgb(0, 0, 255));
        c.end_layer();
    });
    let reference = render(&device, &queue, 100, 100, |c| {
        // The reference blurs on a larger unclipped canvas, then clips the draw.
        let blurred = offscreen(c, 100, 100, |c| {
            assert!(c.begin_layer(&blur));
            fill_rect(c, 10.0, 10.0, 80.0, 80.0, rgb(0, 0, 255));
            c.end_layer();
        });
        clip(c);
        draw_image(c, blurred, 0.0, 0.0, 100, 100);
    });
    assert_matches("clip-outside", &code, &reference, 2, 0);

    // Inside: the clip applies to the content, the blur spreads past it.
    let code = render(&device, &queue, 100, 100, |c| {
        assert!(c.begin_layer(&blur));
        clip(c);
        fill_rect(c, 10.0, 10.0, 80.0, 80.0, rgb(0, 0, 255));
        c.end_layer();
    });
    let reference = render(&device, &queue, 100, 100, |c| {
        let clipped = offscreen(c, 100, 100, |c| {
            clip(c);
            fill_rect(c, 10.0, 10.0, 80.0, 80.0, rgb(0, 0, 255));
        });
        assert!(c.begin_layer(&blur));
        draw_image(c, clipped, 0.0, 0.0, 100, 100);
        c.end_layer();
    });
    assert_matches("clip-inside", &code, &reference, 2, 0);

    // Both.
    let code = render(&device, &queue, 100, 100, |c| {
        clip(c);
        assert!(c.begin_layer(&blur));
        clip(c);
        fill_rect(c, 10.0, 10.0, 80.0, 80.0, rgb(0, 0, 255));
        c.end_layer();
    });
    let reference = render(&device, &queue, 100, 100, |c| {
        let clipped = offscreen(c, 100, 100, |c| {
            clip(c);
            fill_rect(c, 10.0, 10.0, 80.0, 80.0, rgb(0, 0, 255));
        });
        let blurred = offscreen(c, 100, 100, |c| {
            assert!(c.begin_layer(&blur));
            draw_image(c, clipped, 0.0, 0.0, 100, 100);
            c.end_layer();
        });
        clip(c);
        draw_image(c, blurred, 0.0, 0.0, 100, 100);
    });
    assert_matches("clip-inside-and-outside", &code, &reference, 2, 0);
}

/// 2d.layer.blur-from-outside-canvas: content drawn outside the canvas still
/// contributes to a blur that reaches inside. The reference draws the same
/// content on a larger canvas, blurs it, and shows the frame-sized window.
#[test]
fn wpt_blur_from_outside_canvas() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let blur = LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 8.0 }]);
    let content = |c: &mut C| {
        fill_rect(c, 201.0, 50.0, 100.0, 100.0, rgb(64, 224, 208));
        fill_rect(c, 50.0, 201.0, 100.0, 100.0, rgb(75, 0, 130));
        fill_rect(c, -101.0, 50.0, 100.0, 100.0, rgb(255, 165, 0));
        fill_rect(c, 50.0, -101.0, 100.0, 100.0, rgb(165, 42, 42));
    };
    for clipped in [false, true] {
        let code = render(&device, &queue, 200, 200, |c| {
            if clipped {
                c.intersect_scissor(20.0, 20.0, 160.0, 160.0);
            }
            assert!(c.begin_layer(&blur));
            content(c);
            c.end_layer();
        });
        let reference = render(&device, &queue, 200, 200, |c| {
            // A 328 px canvas with the frame at (64, 64): the content lands
            // where it would outside the real canvas, gets blurred whole, and
            // the window is drawn back at the origin.
            let big = c
                .create_image_empty(
                    328,
                    328,
                    PixelFormat::Rgba8,
                    ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y,
                )
                .unwrap();
            c.save();
            c.set_render_target(RenderTarget::Image(big));
            c.clear_rect(0, 0, 328, 328, Color::rgba(0, 0, 0, 0));
            c.reset();
            c.translate(64.0, 64.0);
            assert!(c.begin_layer(&blur));
            content(c);
            c.end_layer();
            c.set_render_target(RenderTarget::Screen);
            c.restore();
            if clipped {
                c.intersect_scissor(20.0, 20.0, 160.0, 160.0);
            }
            let mut p = Path::new();
            p.rect(0.0, 0.0, 200.0, 200.0);
            c.fill_path(&p, &Paint::image(big, -64.0, -64.0, 328.0, 328.0, 0.0, 1.0));
        });
        assert_matches(
            &format!("blur-from-outside-canvas clipped={clipped}"),
            &code,
            &reference,
            3,
            0,
        );
    }
}

/// 2d.layer.css-filters: blur, drop-shadow, and blur then drop-shadow on a
/// layer, against the same effects applied to an image of the content.
#[test]
fn wpt_css_filters() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let teal = |c: &mut C| fill_rect(c, 50.0, 50.0, 100.0, 100.0, rgb(0, 128, 128));
    let blur = LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 5.0 }]);
    // blur(10px)
    let code = render(&device, &queue, 200, 200, |c| {
        assert!(c.begin_layer(&LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 10.0 }])));
        teal(c);
        c.end_layer();
    });
    let reference = render(&device, &queue, 200, 200, |c| {
        let image = offscreen(c, 200, 200, teal);
        let filtered = c
            .create_image_empty(
                200,
                200,
                PixelFormat::Rgba8,
                ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y,
            )
            .unwrap();
        c.filter_image_chain(filtered, &[ImageFilter::GaussianBlur { sigma: 10.0 }], image)
            .unwrap();
        draw_image(c, filtered, 0.0, 0.0, 200, 200);
    });
    assert_matches("css-filters.blur", &code, &reference, 2, 0);
    // drop-shadow(-10px -10px 5px purple): blur radius 5 is shadowBlur 10.
    let code = render(&device, &queue, 200, 200, |c| {
        shadow(c, -10.0, -10.0, 10.0, rgb(128, 0, 128));
        assert!(c.begin_layer(&LayerEffects::new()));
        teal(c);
        c.end_layer();
    });
    let reference = render(&device, &queue, 200, 200, |c| {
        let image = offscreen(c, 200, 200, teal);
        shadow(c, -10.0, -10.0, 10.0, rgb(128, 0, 128));
        draw_image(c, image, 0.0, 0.0, 200, 200);
    });
    assert_matches("css-filters.shadow", &code, &reference, 2, 0);
    // blur(5px) drop-shadow(10px 10px 5px orange): the shadow is of the blurred content.
    let code = render(&device, &queue, 200, 200, |c| {
        shadow(c, 10.0, 10.0, 10.0, rgb(255, 165, 0));
        assert!(c.begin_layer(&blur));
        teal(c);
        c.end_layer();
    });
    let reference = render(&device, &queue, 200, 200, |c| {
        let image = offscreen(c, 200, 200, teal);
        let filtered = c
            .create_image_empty(
                200,
                200,
                PixelFormat::Rgba8,
                ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y,
            )
            .unwrap();
        c.filter_image_chain(filtered, &[ImageFilter::GaussianBlur { sigma: 5.0 }], image)
            .unwrap();
        shadow(c, 10.0, 10.0, 10.0, rgb(255, 165, 0));
        draw_image(c, filtered, 0.0, 0.0, 200, 200);
    });
    assert_matches("css-filters.blur-and-shadow", &code, &reference, 2, 0);
}

/// 2d.layer.nested-filters: drop shadows on nested layers, each cast once by
/// its layer's result, yellow outside blue.
#[test]
fn wpt_nested_drop_shadows() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let code = render(&device, &queue, 400, 200, |c| {
        shadow(c, -20.0, -20.0, 0.0, rgb(255, 255, 0));
        assert!(c.begin_layer(&LayerEffects::new()));
        shadow(c, -10.0, -10.0, 0.0, rgb(0, 0, 255));
        assert!(c.begin_layer(&LayerEffects::new()));
        fill_rect(c, 50.0, 50.0, 100.0, 100.0, rgb(255, 0, 0));
        c.end_layer();
        c.end_layer();
        shadow(c, 20.0, 20.0, 0.0, rgb(0, 0, 255));
        assert!(c.begin_layer(&LayerEffects::new()));
        shadow(c, 10.0, 10.0, 0.0, rgb(255, 255, 0));
        assert!(c.begin_layer(&LayerEffects::new()));
        fill_rect(c, 250.0, 50.0, 100.0, 100.0, rgb(255, 0, 0));
        c.end_layer();
        c.end_layer();
    });
    let reference = render(&device, &queue, 400, 200, |c| {
        fill_rect(c, 20.0, 20.0, 100.0, 100.0, rgb(255, 255, 0));
        fill_rect(c, 30.0, 30.0, 100.0, 100.0, rgb(255, 255, 0));
        fill_rect(c, 40.0, 40.0, 100.0, 100.0, rgb(0, 0, 255));
        fill_rect(c, 50.0, 50.0, 100.0, 100.0, rgb(255, 0, 0));
        fill_rect(c, 280.0, 80.0, 100.0, 100.0, rgb(0, 0, 255));
        fill_rect(c, 270.0, 70.0, 100.0, 100.0, rgb(0, 0, 255));
        fill_rect(c, 260.0, 60.0, 100.0, 100.0, rgb(255, 255, 0));
        fill_rect(c, 250.0, 50.0, 100.0, 100.0, rgb(255, 0, 0));
    });
    assert_matches("nested-filters (drop shadows)", &code, &reference, 2, 0);
}

/// 2d.layer.reset: a reset discards pending layers; the next draw lands on
/// the canvas under default state.
#[test]
fn wpt_reset_discards_pending_layers() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for filtered in [false, true] {
        let code = render(&device, &queue, 200, 200, |c| {
            c.set_global_alpha(0.3);
            c.global_composite_operation(CompositeOperation::SourceIn);
            shadow(c, -3.0, 3.0, 3.0, Color::rgba(0, 30, 0, 77));
            if filtered {
                shadow(c, -3.0, 3.0, 0.0, rgb(0, 0, 0));
            }
            assert!(c.begin_layer(&LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 5.0 }])));
            c.set_global_alpha(0.6);
            shadow(c, -6.0, 6.0, 3.0, Color::rgba(0, 60, 0, 153));
            c.reset();
            fill_rect(c, 10.0, 10.0, 75.0, 50.0, rgb(0, 0, 0));
        });
        let reference = render(&device, &queue, 200, 200, |c| {
            fill_rect(c, 10.0, 10.0, 75.0, 50.0, rgb(0, 0, 0))
        });
        assert_matches(&format!("reset filtered={filtered}"), &code, &reference, 1, 0);
    }
}

/// 2d.layer.non-invertible-matrix: a layer opened under a non-invertible
/// transform draws nothing, even for content that sets a valid transform
/// inside it.
#[test]
fn wpt_non_invertible_matrix() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let code = render(&device, &queue, 200, 200, |c| {
        fill_rect(c, 30.0, 30.0, 50.0, 50.0, rgb(0, 0, 255));
        c.scale(1.0, 0.0);
        let _ = c.begin_layer(&LayerEffects::new());
        fill_rect(c, 40.0, 70.0, 50.0, 50.0, rgb(225, 0, 0));
        c.reset_transform();
        fill_rect(c, 70.0, 40.0, 50.0, 50.0, rgb(0, 128, 0));
        c.end_layer();
    });
    let reference = render(&device, &queue, 200, 200, |c| {
        fill_rect(c, 30.0, 30.0, 50.0, 50.0, rgb(0, 0, 255))
    });
    assert_matches("non-invertible-matrix", &code, &reference, 0, 0);
}

/// 2d.layer.valid-calls: lone and paired save/begin_layer/end_layer/restore
/// calls neither panic nor disturb drawing afterwards.
#[test]
fn wpt_valid_calls() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let sequences: [&dyn Fn(&mut C); 6] = [
        &|c| c.save(),
        &|c| {
            let _ = c.begin_layer(&LayerEffects::new());
        },
        &|c| c.restore(),
        &|c| {
            c.save();
            c.restore();
        },
        &|c| {
            let _ = c.begin_layer(&LayerEffects::new());
            c.end_layer();
        },
        &|c| c.end_layer(),
    ];
    let reference = render(&device, &queue, 100, 100, |c| {
        fill_rect(c, 10.0, 10.0, 50.0, 50.0, rgb(0, 128, 0))
    });
    for (i, sequence) in sequences.iter().enumerate() {
        let code = render(&device, &queue, 100, 100, |c| {
            sequence(c);
            // Close anything the sequence left open before checking the draw.
            c.end_layer();
            c.restore();
            fill_rect(c, 10.0, 10.0, 50.0, 50.0, rgb(0, 128, 0));
        });
        assert_matches(&format!("valid-calls #{i}"), &code, &reference, 0, 0);
    }
}

// Not portable, and why:
// - 2d.layer.invalid-calls / malformed-operations: femtovg raises no
//   exceptions; interleaved save/begin_layer/restore is femtovg/femtovg#335.
// - 2d.layer.anisotropic-blur: GaussianBlur is isotropic (one sigma).
// - 2d.layer.globalCompositeOperation multiply/screen/overlay/darken/lighten:
//   separable blend modes, femtovg/femtovg#332.
// - 2d.layer.drawImage / draw-in-filter / beginLayer-options /
//   layer-rendering-state-reset-in-layer / ctm.getTransform / opaque-canvas /
//   resize-canvas-in-filter: canvas-element API surface (drawing the canvas
//   into itself, filter option objects, state getters, alpha:false).
// - 2d.layer.shadow-from-outside-canvas: a shadow cast into the canvas by
//   content further outside it than the store's padding is lost (documented
//   limit of the scissor-sized store).
