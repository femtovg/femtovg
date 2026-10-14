//! `ImageFlags::GENERATE_MIPMAPS` on the WGPU backend: an image drawn far
//! smaller than its size samples a box-filtered level rather than the base
//! level, as it does on OpenGL, the levels follow every update, and a
//! fractional level of detail leans half a level toward the larger mip.
//! Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{imgref::ImgVec, rgb::RGBA8, Color, ImageFlags, ImageSource, Paint, Path};

mod common;
use common::{headless_device, render_rgba};

const SIZE: usize = 64;
const FRAME: u32 = 16;

/// Columns `white` of every eight are white: `white / 8` of the image is
/// white, so a box-filtered level reads that share of 255 everywhere.
fn stripes(white: usize) -> ImgVec<RGBA8> {
    let (on, off) = (RGBA8::new(255, 255, 255, 255), RGBA8::new(0, 0, 0, 255));
    let pixels = (0..SIZE * SIZE)
        .map(|i| if (i % SIZE) % 8 < white { on } else { off })
        .collect();
    ImgVec::new(pixels, SIZE, SIZE)
}

fn solid(value: u8) -> ImgVec<RGBA8> {
    ImgVec::new(vec![RGBA8::new(value, value, value, 255); SIZE * SIZE], SIZE, SIZE)
}

/// Draws `image` into `drawn` pixels starting at `x` (the same for y) and
/// returns the mean red of the drawn pixels' middle row; `update` replaces
/// the image after creation.
fn drawn(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    flags: ImageFlags,
    image: ImgVec<RGBA8>,
    update: Option<ImgVec<RGBA8>>,
    x: f32,
    drawn: usize,
) -> f32 {
    let pixels = render_rgba(device, queue, FRAME, FRAME, Color::rgb(255, 0, 255), |canvas| {
        let image = canvas
            .create_image(ImageSource::from(image.as_ref()), flags)
            .expect("image");
        if let Some(update) = update {
            canvas
                .update_image(image, ImageSource::from(update.as_ref()), 0, 0)
                .expect("update");
        }
        let size = drawn as f32;
        let mut rect = Path::new();
        rect.rect(x, x, size, size);
        let mut paint = Paint::image(image, x, x, size, size, 0.0, 1.0);
        paint.set_anti_alias(false);
        canvas.fill_path(&rect, &paint);
    });
    let first = x as usize;
    let row = first + drawn / 2;
    let red: u32 = (first..first + drawn)
        .map(|px| pixels[(row * FRAME as usize + px) * 4] as u32)
        .sum();
    red as f32 / drawn as f32
}

/// Eight times smaller: without mipmaps every drawn pixel's centre lands
/// between two black columns of the base level; with them the levels in
/// reach all read the eighth of white, 64.
#[test]
fn a_mipmapped_image_minifies_to_the_box_average() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let base_only = drawn(&device, &queue, ImageFlags::empty(), stripes(2), None, 4.0, 8);
    let mipmapped = drawn(&device, &queue, ImageFlags::GENERATE_MIPMAPS, stripes(2), None, 4.0, 8);
    assert!(
        base_only < 8.0,
        "the base level between the stripes reads black, got {base_only}"
    );
    assert!(
        (mipmapped - 64.0).abs() < 6.0,
        "an eighth-white image minified reads 64, got {mipmapped}"
    );
}

#[test]
fn the_levels_follow_an_update() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let updated = drawn(
        &device,
        &queue,
        ImageFlags::GENERATE_MIPMAPS,
        stripes(2),
        Some(solid(200)),
        4.0,
        8,
    );
    assert!(
        (updated - 200.0).abs() < 3.0,
        "the levels are regenerated from the updated base, got {updated}"
    );
}

/// Sixteen times smaller, where both levels a nearest selection can pick
/// are uniform.
#[test]
fn nearest_with_mipmaps_picks_a_minified_level() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let flags = ImageFlags::GENERATE_MIPMAPS | ImageFlags::NEAREST;
    let nearest = drawn(&device, &queue, flags, stripes(2), None, 4.0, 4);
    assert!(
        (nearest - 64.0).abs() < 6.0,
        "a nearest-level sample reads the level's texels, got {nearest}"
    );
}

/// Half-and-half stripes eight times smaller, at a quarter-pixel phase that
/// puts every drawn pixel on a white texel of the 16 px level: the 8 px
/// level is a flat 128, so plain trilinear at exactly that level would read
/// 128, and the half-level lean toward the 16 px level reads 191.
#[test]
fn a_fractional_level_leans_toward_the_larger_mip() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let leaned = drawn(&device, &queue, ImageFlags::GENERATE_MIPMAPS, stripes(4), None, 4.25, 8);
    assert!(
        (leaned - 191.0).abs() < 8.0,
        "half of the sharper level over the flat one reads 191, got {leaned}"
    );
}
