//! Headless GPU tests for `ImageFilter::Blend` (SVG `feBlend`): every blend
//! mode of the Compositing and Blending specification against a CPU
//! implementation of its formulas, the backdrop's placement rect, the
//! orientation rule for an upload and for a layer capture (with a pass in
//! front of the blend), and a layer whose store does not start at the
//! canvas origin. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{
    imgref::Img, renderer::WGPURenderer, rgb::RGBA8, BlendMode, Canvas, Color, ErrorKind, ImageFilter, ImageFlags,
    ImageId, LayerEffects, Paint, Path, PixelFormat,
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

/// Runs `filters` from `source` into a fresh chain target and draws that
/// target 1:1 onto a transparent output, so the readback is the result.
fn chain_to_screen(canvas: &mut C, filters: &[ImageFilter], source: ImageId) {
    let target = canvas
        .create_image_empty(
            W as usize,
            H as usize,
            PixelFormat::Rgba8,
            ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y | ImageFlags::NEAREST,
        )
        .expect("target");
    canvas.filter_image_chain(target, filters, source).expect("chain");
    let mut p = Path::new();
    p.rect(0.0, 0.0, W as f32, H as f32);
    let mut paint = Paint::image(target, 0.0, 0.0, W as f32, H as f32, 0.0, 1.0);
    paint.set_anti_alias(false);
    canvas.fill_path(&p, &paint);
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

/// Every mode, every pixel, within rounding of the specification's formulas.
#[test]
fn blend_modes_match_the_specification() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for mode in ALL_MODES {
        let out = render_rgba(&device, &queue, W, H, Color::rgba(0, 0, 0, 0), |c| {
            let source = upload(c, W, H, source_pixel);
            let backdrop = upload(c, W, H, backdrop_pixel);
            let blend = ImageFilter::Blend {
                mode,
                backdrop,
                x: 0.0,
                y: 0.0,
                width: W as f32,
                height: H as f32,
            };
            chain_to_screen(c, &[blend], source);
        });
        let mut worst = 0i32;
        for y in 0..H {
            for x in 0..W {
                let got = px(&out, x, y);
                let want = expected(mode, source_pixel(x, y), backdrop_pixel(x, y));
                for i in 0..4 {
                    worst = worst.max((got[i] as i32 - want[i] as i32).abs());
                }
            }
        }
        assert!(worst <= 3, "{mode:?}: worst channel delta {worst}/255");
    }
}

/// Outside its placement rect the backdrop is transparent, so the image
/// passes through unchanged; inside, it blends.
#[test]
fn the_backdrop_is_transparent_outside_its_rect() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render_rgba(&device, &queue, W, H, Color::rgba(0, 0, 0, 0), |c| {
        let source = upload(c, W, H, |_, _| [204, 102, 51, 255]);
        let backdrop = upload(c, 8, 8, |_, _| [128, 128, 255, 255]);
        chain_to_screen(
            c,
            &[ImageFilter::Blend {
                mode: BlendMode::Multiply,
                backdrop,
                x: 4.0,
                y: 4.0,
                width: 8.0,
                height: 8.0,
            }],
            source,
        );
    });
    assert_eq!(
        px(&out, 1, 1),
        [204, 102, 51, 255],
        "outside the rect: the image itself"
    );
    assert_eq!(
        px(&out, 14, 14),
        [204, 102, 51, 255],
        "outside the rect: the image itself"
    );
    let inside = px(&out, 8, 8);
    for (got, want) in inside.into_iter().zip([102u8, 51, 51, 255]) {
        assert!(
            (got as i32 - want as i32).abs() <= 1,
            "inside the rect: {inside:?} vs multiply"
        );
    }
}

/// An upload is stored upright and the placed backdrop is a render target,
/// so the pass samples the backdrop the other way up: the backdrop's top
/// half lands on the result's top half.
#[test]
fn the_backdrop_keeps_its_orientation_against_an_upload() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render_rgba(&device, &queue, W, H, Color::rgba(0, 0, 0, 0), |c| {
        let source = upload(c, W, H, |_, _| [255, 255, 255, 255]);
        let backdrop = upload(
            c,
            W,
            H,
            |_, y| if y < H / 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] },
        );
        chain_to_screen(
            c,
            &[ImageFilter::Blend {
                mode: BlendMode::Multiply,
                backdrop,
                x: 0.0,
                y: 0.0,
                width: W as f32,
                height: H as f32,
            }],
            source,
        );
    });
    assert_eq!(px(&out, 8, 2), [255, 0, 0, 255], "top half: red backdrop");
    assert_eq!(px(&out, 8, 13), [0, 0, 255, 255], "bottom half: blue backdrop");
}

/// A layer's blend places its backdrop in root device space: with the store
/// scissored to (20, 10)-(60, 50) its origin is not the canvas origin, and
/// the backdrop must still land where the rect says. The capture is a
/// render target, the opposite orientation from an upload, which the pass
/// must also get right.
fn layer_case(with_pass_in_front: bool) -> Vec<u8> {
    let (device, queue) = headless_device().expect("gpu");
    render_rgba(&device, &queue, 64, 64, Color::white(), |c| {
        let backdrop = upload(
            c,
            40,
            40,
            |_, y| if y < 20 { [255, 0, 0, 255] } else { [0, 0, 255, 255] },
        );
        c.scissor(20.0, 10.0, 40.0, 40.0);
        let mut filters = Vec::new();
        if with_pass_in_front {
            filters.push(ImageFilter::identity());
        }
        filters.push(ImageFilter::Blend {
            mode: BlendMode::Multiply,
            backdrop,
            x: 20.0,
            y: 10.0,
            width: 40.0,
            height: 40.0,
        });
        assert!(c.begin_layer(&LayerEffects::new().with_filters(&filters)));
        let mut p = Path::new();
        p.rect(20.0, 10.0, 40.0, 40.0);
        let mut paint = Paint::color(Color::white());
        paint.set_anti_alias(false);
        c.fill_path(&p, &paint);
        c.end_layer();
    })
}

fn px64(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * 64 + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

#[test]
fn a_layer_blends_over_a_backdrop_placed_in_device_space() {
    if headless_device().is_none() {
        return;
    }
    let out = layer_case(false);
    assert_eq!(px64(&out, 40, 15), [255, 0, 0, 255], "top of the placed backdrop: red");
    assert_eq!(
        px64(&out, 40, 45),
        [0, 0, 255, 255],
        "bottom of the placed backdrop: blue"
    );
    assert_eq!(px64(&out, 5, 5), [255, 255, 255, 255], "outside the layer: untouched");
}

#[test]
fn a_blend_after_another_pass_keeps_the_backdrop_upright() {
    if headless_device().is_none() {
        return;
    }
    let out = layer_case(true);
    assert_eq!(px64(&out, 40, 15), [255, 0, 0, 255], "top of the placed backdrop: red");
    assert_eq!(
        px64(&out, 40, 45),
        [0, 0, 255, 255],
        "bottom of the placed backdrop: blue"
    );
}

/// A blend whose backdrop does not exist runs nothing and says so.
#[test]
fn a_missing_backdrop_is_an_error_for_a_chain() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    render_rgba(&device, &queue, W, H, Color::white(), |c| {
        let source = upload(c, W, H, |_, _| [255, 255, 255, 255]);
        let target = c
            .create_image_empty(W as usize, H as usize, PixelFormat::Rgba8, ImageFlags::PREMULTIPLIED)
            .unwrap();
        let backdrop = upload(c, 2, 2, |_, _| [0, 0, 0, 255]);
        // Deletion is deferred to the flush that ends the frame the image
        // was used in; a flush with nothing recorded runs it right away.
        c.delete_image(backdrop);
        c.flush_to_output(&device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        }));
        let result = c.filter_image_chain(
            target,
            &[ImageFilter::Blend {
                mode: BlendMode::Normal,
                backdrop,
                x: 0.0,
                y: 0.0,
                width: 2.0,
                height: 2.0,
            }],
            source,
        );
        assert!(matches!(result, Err(ErrorKind::ImageIdNotFound)), "{result:?}");
        // Something for the frame the helper flushes.
        let mut p = Path::new();
        p.rect(0.0, 0.0, 4.0, 4.0);
        c.fill_path(&p, &Paint::color(Color::black()));
    });
}
