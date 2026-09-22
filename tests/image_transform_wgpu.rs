//! Headless GPU test: `Paint::set_image_transform` maps an image pattern
//! into user space before the canvas transform (SVG `patternTransform`,
//! Canvas 2D `CanvasPattern.setTransform`). A 2×2 checker is repeated,
//! scaled to 8×4 texels and shifted; each probe must land on the texel the
//! transform puts there.
#![cfg(feature = "wgpu")]

use femtovg::{Color, ImageFlags, ImageSource, Paint, Path, Transform2D};
use imgref::Img;
use rgb::RGBA8;

mod common;
use common::{headless_device, render_rgba};

const W: u32 = 48;
const H: u32 = 24;

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

fn render(transform: Option<Transform2D>) -> Option<Vec<u8>> {
    render_with(transform, true)
}

fn render_with(transform: Option<Transform2D>, anti_alias: bool) -> Option<Vec<u8>> {
    let (device, queue) = headless_device()?;
    Some(render_rgba(&device, &queue, W, H, Color::rgba(0, 0, 0, 0), |canvas| {
        let texels = [RED, GREEN, BLUE, WHITE].map(|[r, g, b, a]| RGBA8::new(r, g, b, a));
        let image = canvas
            .create_image(
                ImageSource::Rgba(Img::new(&texels[..], 2, 2)),
                ImageFlags::NEAREST | ImageFlags::REPEAT_X | ImageFlags::REPEAT_Y,
            )
            .expect("image");
        let mut paint = Paint::image(image, 0.0, 0.0, 2.0, 2.0, 0.0, 1.0).with_anti_alias(anti_alias);
        if let Some(t) = transform {
            paint.set_image_transform(t);
        }
        let mut path = Path::new();
        path.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&path, &paint);
    }))
}

fn pixel(rgba: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

#[test]
fn an_image_transform_scales_and_moves_a_repeating_pattern() {
    // Texels become 8×4 and the pattern starts at x = 3.
    let mut t = Transform2D::scaling(8.0, 4.0);
    t.translate(3.0, 0.0);
    let Some(rgba) = render(Some(t)) else { return };
    for (x, y, expected) in [
        (5, 1, RED),
        (13, 1, GREEN),
        (5, 5, BLUE),
        (13, 5, WHITE),
        // Repeats: one period (16 × 8) later, and before the origin.
        (21, 1, RED),
        (29, 9, GREEN),
        (1, 1, GREEN),
    ] {
        assert_eq!(pixel(&rgba, x, y), expected, "({x}, {y})");
    }
}

#[test]
fn without_a_transform_the_pattern_is_unchanged() {
    let Some(plain) = render(None) else { return };
    let Some(identity) = render(Some(Transform2D::identity())) else {
        return;
    };
    assert_eq!(plain, identity);
    // One-pixel texels.
    assert_eq!(pixel(&plain, 0, 0), RED);
    assert_eq!(pixel(&plain, 1, 0), GREEN);
    assert_eq!(pixel(&plain, 0, 1), BLUE);
}

#[test]
fn an_aliased_rectangle_does_not_bypass_the_image_transform() {
    // A non-anti-aliased rectangle filled with an image is drawn as a
    // blit whose texture coordinates come from the paint matrix, so it must
    // see the image transform too.
    let mut t = Transform2D::scaling(8.0, 4.0);
    t.translate(3.0, 0.0);
    let Some(aliased) = render_with(Some(t), false) else {
        return;
    };
    assert_eq!(pixel(&aliased, 13, 1), GREEN);
    assert_eq!(pixel(&aliased, 5, 5), BLUE);
}
