#![cfg(feature = "wgpu")]

use femtovg::{Color, FillRule, ImageFlags, Paint, Path, PixelFormat, RenderTarget};

mod common;
use common::{headless_device, render_rgba};

const W: u32 = 64;
const H: u32 = 64;

fn rect(x: f32, y: f32, w: f32, h: f32) -> Path {
    let mut path = Path::new();
    path.rect(x, y, w, h);
    path
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * W + x) * 4) as usize;
    pixels[offset..offset + 4].try_into().unwrap()
}

#[test]
fn restore_underflow_is_a_no_op() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let pixels = render_rgba(&device, &queue, W, H, Color::white(), |canvas| {
        canvas.translate(8.0, 0.0);
        canvas.set_global_alpha(0.5);
        canvas.clip_path(&rect(0.0, 0.0, 24.0, H as f32), FillRule::NonZero);
        for _ in 0..16 {
            canvas.restore();
        }
        canvas.fill_path(
            &rect(0.0, 0.0, W as f32, H as f32),
            &Paint::color(Color::rgb(255, 0, 0)).with_anti_alias(false),
        );
    });

    assert_eq!(pixel(&pixels, 4, 32), [255, 255, 255, 255], "transform");
    assert_eq!(pixel(&pixels, 12, 32), [255, 128, 128, 255], "alpha");
    assert_eq!(pixel(&pixels, 40, 32), [255, 255, 255, 255], "clip");
}

#[test]
fn deleting_a_queued_image_target_does_not_poison_the_frame() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let pixels = render_rgba(&device, &queue, W, H, Color::white(), |canvas| {
        let image = canvas
            .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::PREMULTIPLIED)
            .unwrap();
        canvas.set_render_target(RenderTarget::Image(image));
        canvas.clear_rect(0, 0, 16, 16, Color::black());
        canvas.delete_image(image);
        assert!(canvas.image_info(image).is_err());
        canvas.set_render_target(RenderTarget::Image(image));
        canvas.fill_path(
            &rect(0.0, 0.0, W as f32, H as f32),
            &Paint::color(Color::rgb(0, 255, 0)).with_anti_alias(false),
        );
    });
    assert_eq!(pixel(&pixels, 32, 32), [0, 255, 0, 255]);
}
