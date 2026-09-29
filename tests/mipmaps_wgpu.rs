//! `ImageFlags::GENERATE_MIPMAPS` on the WGPU backend: an image drawn far
//! smaller than its size samples a box-filtered level rather than the base
//! level, as it does on OpenGL, and the levels follow every update. Skips
//! without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{imgref::ImgVec, rgb::RGBA8, Color, ImageFlags, ImageSource, Paint, Path};

mod common;
use common::{headless_device, render_rgba};

const SIZE: usize = 64;
const FRAME: u32 = 16;
/// The image is drawn into this many pixels: eight times smaller.
const DRAWN: usize = 8;

/// Columns 0 and 1 of every eight are white, so an eighth of the image is
/// white: a level minified eight times reads 64/255 everywhere, while the
/// base level sampled at those pixels' centres lands between two black
/// columns and reads 0.
fn stripes() -> ImgVec<RGBA8> {
    let white = RGBA8::new(255, 255, 255, 255);
    let black = RGBA8::new(0, 0, 0, 255);
    let pixels = (0..SIZE * SIZE)
        .map(|i| if (i % SIZE) % 8 < 2 { white } else { black })
        .collect();
    ImgVec::new(pixels, SIZE, SIZE)
}

fn solid(value: u8) -> ImgVec<RGBA8> {
    ImgVec::new(vec![RGBA8::new(value, value, value, 255); SIZE * SIZE], SIZE, SIZE)
}

/// Draws the stripes eight times smaller and returns the mean red of the
/// drawn pixels' middle row; `update` replaces the image first.
fn drawn_small(device: &wgpu::Device, queue: &wgpu::Queue, flags: ImageFlags, update: Option<ImgVec<RGBA8>>) -> f32 {
    let pixels = render_rgba(device, queue, FRAME, FRAME, Color::rgb(255, 0, 255), |canvas| {
        let image = canvas
            .create_image(ImageSource::from(stripes().as_ref()), flags)
            .expect("image");
        if let Some(update) = update {
            canvas
                .update_image(image, ImageSource::from(update.as_ref()), 0, 0)
                .expect("update");
        }
        let (x, y, size) = (4.0, 4.0, DRAWN as f32);
        let mut rect = Path::new();
        rect.rect(x, y, size, size);
        let mut paint = Paint::image(image, x, y, size, size, 0.0, 1.0);
        paint.set_anti_alias(false);
        canvas.fill_path(&rect, &paint);
    });
    let row = 4 + DRAWN / 2;
    let red: u32 = (4..4 + DRAWN)
        .map(|x| pixels[(row * FRAME as usize + x) * 4] as u32)
        .sum();
    red as f32 / DRAWN as f32
}

#[test]
fn a_mipmapped_image_minifies_to_the_box_average() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let base_only = drawn_small(&device, &queue, ImageFlags::empty(), None);
    let mipmapped = drawn_small(&device, &queue, ImageFlags::GENERATE_MIPMAPS, None);
    assert!(
        base_only < 8.0,
        "without mipmaps the base level reads black between the stripes, got {base_only}"
    );
    assert!(
        (mipmapped - 64.0).abs() < 6.0,
        "with mipmaps an eighth-white image minified eight times reads 64, got {mipmapped}"
    );
}

#[test]
fn the_levels_follow_an_update() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let updated = drawn_small(&device, &queue, ImageFlags::GENERATE_MIPMAPS, Some(solid(200)));
    assert!(
        (updated - 200.0).abs() < 3.0,
        "the levels must be regenerated from the updated base level, got {updated}"
    );
}

#[test]
fn nearest_with_mipmaps_picks_the_minified_level() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let nearest = drawn_small(
        &device,
        &queue,
        ImageFlags::GENERATE_MIPMAPS | ImageFlags::NEAREST,
        None,
    );
    assert!(
        (nearest - 64.0).abs() < 6.0,
        "NEAREST_MIPMAP_NEAREST reads the minified level's texels, got {nearest}"
    );
}
