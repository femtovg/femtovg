//! Headless GPU tests for `ImageFilter::Blend` (SVG `feBlend`): every blend
//! mode of the Compositing and Blending specification against a CPU
//! implementation of its formulas, the backdrop's placement rect, the
//! orientation rule for an upload and for a layer capture (with a pass in
//! front of the blend), and a layer whose store does not start at the
//! canvas origin. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{
    imgref::Img, renderer::WGPURenderer, rgb::RGBA8, BlendMode, Canvas, Color, ErrorKind, ImageFilter, ImageFlags,
    ImageId, LayerEffects, Paint, Path, PixelFormat, RenderTarget,
};

mod common;
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

// ---- the specification's blend functions, on unpremultiplied colors ----

fn lum(c: [f64; 3]) -> f64 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f64; 3]) -> [f64; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut o = c;
    if n < 0.0 {
        o = o.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 {
        o = o.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    o
}

fn set_lum(c: [f64; 3], l: f64) -> [f64; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

fn sat(c: [f64; 3]) -> f64 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f64; 3], s: f64) -> [f64; 3] {
    let mn = c[0].min(c[1]).min(c[2]);
    let mx = c[0].max(c[1]).max(c[2]);
    if mx > mn {
        c.map(|v| (v - mn) * s / (mx - mn))
    } else {
        [0.0; 3]
    }
}

fn hard_light(cb: f64, cs: f64) -> f64 {
    if cs <= 0.5 {
        cb * 2.0 * cs
    } else {
        let s = 2.0 * cs - 1.0;
        cb + s - cb * s
    }
}

fn separable(mode: BlendMode, cb: f64, cs: f64) -> f64 {
    match mode {
        BlendMode::Normal => cs,
        BlendMode::Multiply => cb * cs,
        BlendMode::Screen => cb + cs - cb * cs,
        BlendMode::Overlay => hard_light(cs, cb),
        BlendMode::Darken => cb.min(cs),
        BlendMode::Lighten => cb.max(cs),
        BlendMode::ColorDodge => {
            if cb <= 0.0 {
                0.0
            } else if cs >= 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        BlendMode::ColorBurn => {
            if cb >= 1.0 {
                1.0
            } else if cs <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        BlendMode::HardLight => hard_light(cb, cs),
        BlendMode::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let d = if cb <= 0.25 {
                    ((16.0 * cb - 12.0) * cb + 4.0) * cb
                } else {
                    cb.sqrt()
                };
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        BlendMode::Difference => (cb - cs).abs(),
        BlendMode::Exclusion => cb + cs - 2.0 * cb * cs,
        _ => unreachable!("non-separable"),
    }
}

fn blend(mode: BlendMode, cb: [f64; 3], cs: [f64; 3]) -> [f64; 3] {
    match mode {
        BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        BlendMode::Color => set_lum(cs, lum(cb)),
        BlendMode::Luminosity => set_lum(cb, lum(cs)),
        _ => [0, 1, 2].map(|i| separable(mode, cb[i], cs[i])),
    }
}

/// The specification's composite of premultiplied `s` over premultiplied
/// `b` under `mode`, in RGBA8.
fn expected(mode: BlendMode, s: [u8; 4], b: [u8; 4]) -> [u8; 4] {
    let f = |v: u8| v as f64 / 255.0;
    let (sa, ba) = (f(s[3]), f(b[3]));
    let unpremul = |c: [u8; 4], a: f64| {
        if a > 0.0 {
            [f(c[0]) / a, f(c[1]) / a, f(c[2]) / a].map(|v| v.clamp(0.0, 1.0))
        } else {
            [0.0; 3]
        }
    };
    let (cs, cb) = (unpremul(s, sa), unpremul(b, ba));
    let bl = blend(mode, cb, cs).map(|v| v.clamp(0.0, 1.0));
    let ao = sa + ba - sa * ba;
    let co = [0, 1, 2].map(|i| f(s[i]) * (1.0 - ba) + f(b[i]) * (1.0 - sa) + sa * ba * bl[i]);
    [co[0], co[1], co[2], ao].map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
}

const ALL_MODES: [BlendMode; 16] = [
    BlendMode::Normal,
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
    BlendMode::Darken,
    BlendMode::Lighten,
    BlendMode::ColorDodge,
    BlendMode::ColorBurn,
    BlendMode::HardLight,
    BlendMode::SoftLight,
    BlendMode::Difference,
    BlendMode::Exclusion,
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
];

/// Premultiplied source pixel: color and alpha vary across the image so
/// every mode's branches get exercised.
fn source_pixel(x: u32, y: u32) -> [u8; 4] {
    let a = 0.35 + 0.65 * (x as f64 / 15.0);
    let (r, g, b) = (x as f64 / 15.0, y as f64 / 15.0, 0.6);
    [r, g, b]
        .map(|c| (c * a * 255.0).round() as u8)
        .into_iter()
        .chain([(a * 255.0).round() as u8])
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

fn backdrop_pixel(x: u32, y: u32) -> [u8; 4] {
    let a = 0.5 + 0.5 * (y as f64 / 15.0);
    let (r, g, b) = (0.8, 1.0 - y as f64 / 15.0, x as f64 / 15.0);
    [r, g, b]
        .map(|c| (c * a * 255.0).round() as u8)
        .into_iter()
        .chain([(a * 255.0).round() as u8])
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
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
