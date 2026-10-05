//! Headless GPU tests for `ImageFilter::Morphology` (SVG `feMorphology`)
//! and `ImageFilter::Offset` (`feOffset`): a dilation grows an opaque
//! square by its radius on every side and an erosion shrinks it, to the
//! pixel, per axis and across the per-draw bound; an offset moves it by
//! whole and fractional pixels whichever way up its source is stored; the
//! border reads transparent; and the shadow chain a design tool exports -
//! `SourceAlpha`, dilate, offset, color matrix - renders the spread shadow
//! alone. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{
    imgref::Img, renderer::WGPURenderer, rgb::RGBA8, Canvas, Color, ImageFilter, ImageFlags, ImageId, LayerEffects,
    MorphologyOperator, Paint, Path, PixelFormat,
};

mod common;
use common::{headless_device, render_rgba};

const W: u32 = 160;
const H: u32 = 160;
/// The opaque red square of the source image: `SQUARE` pixels from
/// (`AT`, `AT`), so its last pixel is `AT + SQUARE - 1`.
const AT: u32 = 72;
const SQUARE: u32 = 16;

/// The source: transparent but for the red square.
fn square_image(canvas: &mut Canvas<WGPURenderer>) -> ImageId {
    let pixels: Vec<RGBA8> = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if (AT..AT + SQUARE).contains(&x) && (AT..AT + SQUARE).contains(&y) {
                RGBA8::new(255, 0, 0, 255)
            } else {
                RGBA8::new(0, 0, 0, 0)
            }
        })
        .collect();
    canvas
        .create_image(Img::new(pixels.as_slice(), W as usize, H as usize), ImageFlags::empty())
        .expect("source image")
}

/// A filter target under the chain's storage convention.
fn chain_target(canvas: &mut Canvas<WGPURenderer>) -> ImageId {
    canvas
        .create_image_empty(
            W as usize,
            H as usize,
            PixelFormat::Rgba8,
            ImageFlags::FLIP_Y | ImageFlags::PREMULTIPLIED,
        )
        .expect("target image")
}

fn blit(canvas: &mut Canvas<WGPURenderer>, image: ImageId) {
    let mut p = Path::new();
    p.rect(0.0, 0.0, W as f32, H as f32);
    canvas.fill_path(&p, &Paint::image(image, 0.0, 0.0, W as f32, H as f32, 0.0, 1.0));
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

/// The bounds `(x0, y0, x1, y1)` (inclusive) of the pixels that are not
/// the white canvas, and whether every pixel inside those bounds is the
/// solid red square (an opaque square must have no fringe at all).
fn painted_bounds(buf: &[u8]) -> ((u32, u32, u32, u32), bool) {
    let painted = |x, y| px(buf, x, y) != [255, 255, 255, 255];
    let (mut x0, mut y0, mut x1, mut y1) = (W, H, 0, 0);
    for y in 0..H {
        for x in 0..W {
            if painted(x, y) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    let solid = (y0..=y1).all(|y| (x0..=x1).all(|x| px(buf, x, y) == [255, 0, 0, 255]));
    ((x0, y0, x1, y1), solid)
}

/// The red square run through `filters` as a chain over the uploaded
/// image, blitted onto a white canvas.
fn chain(device: &wgpu::Device, queue: &wgpu::Queue, filters: &[ImageFilter]) -> Vec<u8> {
    render_rgba(device, queue, W, H, Color::white(), |canvas| {
        let source = square_image(canvas);
        let target = chain_target(canvas);
        canvas.filter_image_chain(target, filters, source).expect("chain");
        blit(canvas, target);
    })
}

fn dilate(radius_x: f32, radius_y: f32) -> ImageFilter {
    ImageFilter::Morphology {
        radius_x,
        radius_y,
        operator: MorphologyOperator::Dilate,
    }
}

fn erode(radius_x: f32, radius_y: f32) -> ImageFilter {
    ImageFilter::Morphology {
        radius_x,
        radius_y,
        operator: MorphologyOperator::Erode,
    }
}

/// A dilation grows the square by its radius on every side and an erosion
/// shrinks it as much, per axis, with no fringe: the radius 30 case runs as
/// two draws per axis (24 and 6) and still lands to the pixel, and a radius
/// rounds to whole pixels.
#[test]
fn a_morphology_grows_or_shrinks_a_square_by_its_radius() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let last = AT + SQUARE - 1;
    for (name, filter, expected) in [
        ("dilate 3", dilate(3.0, 3.0), (AT - 3, AT - 3, last + 3, last + 3)),
        ("erode 2", erode(2.0, 2.0), (AT + 2, AT + 2, last - 2, last - 2)),
        ("dilate 4 by 0", dilate(4.0, 0.0), (AT - 4, AT, last + 4, last)),
        ("dilate 0 by 5", dilate(0.0, 5.0), (AT, AT - 5, last, last + 5)),
        (
            "dilate 30",
            dilate(30.0, 30.0),
            (AT - 30, AT - 30, last + 30, last + 30),
        ),
        (
            "dilate 2.4 rounds down",
            dilate(2.4, 2.4),
            (AT - 2, AT - 2, last + 2, last + 2),
        ),
        (
            "dilate 2.6 rounds up",
            dilate(2.6, 2.6),
            (AT - 3, AT - 3, last + 3, last + 3),
        ),
        ("dilate 0 copies", dilate(0.0, 0.0), (AT, AT, last, last)),
    ] {
        let out = chain(&device, &queue, &[filter]);
        let (bounds, solid) = painted_bounds(&out);
        assert_eq!(bounds, expected, "{name}");
        assert!(solid, "{name}: the square must stay solid red with no fringe");
    }
}

/// An offset moves the square by its shift, whichever way up the source is
/// stored: straight from the upload, after a color-matrix pass (whose
/// output is stored the other way up), in a layer (a render target) and in
/// the single-pass `filter_image`; a fractional shift lands between pixels.
#[test]
fn an_offset_moves_a_square_whichever_way_up_its_source_is() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let last = AT + SQUARE - 1;
    let shift = ImageFilter::Offset { dx: 5.0, dy: 7.0 };
    let moved = (AT + 5, AT + 7, last + 5, last + 7);

    let (bounds, solid) = painted_bounds(&chain(&device, &queue, &[shift]));
    assert_eq!(bounds, moved, "offset alone");
    assert!(solid, "offset alone");

    let back = ImageFilter::Offset { dx: -3.0, dy: -2.0 };
    let (bounds, solid) = painted_bounds(&chain(&device, &queue, &[back]));
    assert_eq!(bounds, (AT - 3, AT - 2, last - 3, last - 2), "offset back");
    assert!(solid, "offset back");

    // Grayscale first: its pass leaves the image stored the other way up,
    // and the offset must still move the (now gray) square down.
    let gray_then_shift = chain(&device, &queue, &[ImageFilter::saturate(0.0), shift]);
    let (bounds, _) = painted_bounds(&gray_then_shift);
    assert_eq!(bounds, moved, "offset after a color matrix");
    let center = px(&gray_then_shift, AT + 5 + SQUARE / 2, AT + 7 + SQUARE / 2);
    assert!(
        center[0] == center[1] && center[1] == center[2] && center[3] == 255,
        "gray, got {center:?}"
    );

    let layered = render_rgba(&device, &queue, W, H, Color::white(), |canvas| {
        assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[shift])));
        let mut p = Path::new();
        p.rect(AT as f32, AT as f32, SQUARE as f32, SQUARE as f32);
        let mut paint = Paint::color(Color::rgb(255, 0, 0));
        paint.set_anti_alias(false);
        canvas.fill_path(&p, &paint);
        canvas.end_layer();
    });
    let (bounds, solid) = painted_bounds(&layered);
    assert_eq!(bounds, moved, "offset in a layer");
    assert!(solid, "offset in a layer");

    // A single offset pass is one draw, which stores its result the other
    // way up like a color-matrix pass, so its target samples through FLIP_Y.
    let single = render_rgba(&device, &queue, W, H, Color::white(), |canvas| {
        let source = square_image(canvas);
        let target = chain_target(canvas);
        canvas.filter_image(target, shift, source);
        blit(canvas, target);
    });
    let (bounds, solid) = painted_bounds(&single);
    assert_eq!(bounds, moved, "single pass");
    assert!(solid, "single pass");

    let half = chain(&device, &queue, &[ImageFilter::Offset { dx: 0.5, dy: 0.0 }]);
    let edge = px(&half, AT, AT + SQUARE / 2);
    let beyond = px(&half, AT + SQUARE, AT + SQUARE / 2);
    assert!(
        (120..=136).contains(&edge[1]) && (120..=136).contains(&beyond[1]),
        "a half-pixel shift leaves both edge columns half covered, got {edge:?} and {beyond:?}"
    );
}

/// Beyond the image is transparent: an erosion of an image opaque to its
/// border eats the border in by its radius, where clamped sampling would
/// have left it opaque.
#[test]
fn an_erosion_eats_into_the_image_border() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render_rgba(&device, &queue, W, H, Color::white(), |canvas| {
        let pixels = vec![RGBA8::new(255, 0, 0, 255); (W * H) as usize];
        let source = canvas
            .create_image(Img::new(pixels.as_slice(), W as usize, H as usize), ImageFlags::empty())
            .expect("source image");
        let target = chain_target(canvas);
        canvas
            .filter_image_chain(target, &[erode(3.0, 3.0)], source)
            .expect("chain");
        blit(canvas, target);
    });
    let (bounds, solid) = painted_bounds(&out);
    assert_eq!(bounds, (3, 3, W - 4, H - 4));
    assert!(solid);
}

/// The shadow chain Sketch exports for a spread shadow: the source's alpha
/// dilated, offset, (blurred,) and coloured through a color matrix, with the
/// source itself drawn by a second `use` - so a layer running the chain
/// shows the spread shadow alone: a black square 2 px larger on every side
/// at a quarter opacity, over white a 191 gray, and nothing of the red.
#[test]
fn a_spread_shadow_chain_renders_the_shadow_alone() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let source_alpha = ImageFilter::ColorMatrix {
        matrix: [
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 1.0, 0.0,
        ],
    };
    let quarter_black = ImageFilter::ColorMatrix {
        matrix: [
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.25, 0.0,
        ],
    };
    let shadow = [
        source_alpha,
        dilate(2.0, 2.0),
        ImageFilter::Offset { dx: 0.0, dy: 0.0 },
        quarter_black,
    ];
    let out = render_rgba(&device, &queue, W, H, Color::white(), |canvas| {
        assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&shadow)));
        let mut p = Path::new();
        p.rect(AT as f32, AT as f32, SQUARE as f32, SQUARE as f32);
        let mut paint = Paint::color(Color::rgb(255, 0, 0));
        paint.set_anti_alias(false);
        canvas.fill_path(&p, &paint);
        canvas.end_layer();
    });
    let last = AT + SQUARE - 1;
    let (bounds, _) = painted_bounds(&out);
    assert_eq!(bounds, (AT - 2, AT - 2, last + 2, last + 2));
    for (x, y) in [
        (AT - 2, AT - 2),
        (AT + SQUARE / 2, AT + SQUARE / 2),
        (last + 2, last + 2),
    ] {
        let p = px(&out, x, y);
        assert!(
            p[0] == p[1] && p[1] == p[2] && (189..=193).contains(&p[0]),
            "({x}, {y}): a quarter of black over white, got {p:?}"
        );
    }
}
