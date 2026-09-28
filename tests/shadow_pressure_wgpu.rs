//! Headless GPU pressure tests on a device limited to 2048 px textures,
//! what a VideoCore IV reports: a shadowed layer whose reach would push its
//! store past the limit keeps its visible capture - its opacity applies and
//! its shadow is cast - instead of passing through with every effect
//! dropped. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, LayerEffects, Paint, Path};

mod common;

fn pixel(px: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * width + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2]]
}

/// Draws a blue box in a half-opacity layer that casts a hard black shadow
/// `offset` to the right, on a `width` x `height` canvas.
fn shadowed_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    offset: f32,
    blur: f32,
    rect: (f32, f32, f32, f32),
) -> Vec<u8> {
    common::render_rgba(
        device,
        queue,
        width,
        height,
        Color::white(),
        |canvas: &mut Canvas<WGPURenderer>| {
            // Fifteen quadrature passes over a Full HD coverage image pass the
            // default filter-work budget; only the texture limit is under test.
            canvas.set_filter_work_budget(u64::MAX);
            canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
            canvas.set_shadow_offset(offset, 0.0);
            canvas.set_shadow_blur(blur);
            assert!(
                canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)),
                "the layer captures instead of passing through"
            );
            let mut path = Path::new();
            path.rect(rect.0, rect.1, rect.2, rect.3);
            canvas.fill_path(&path, &Paint::color(Color::rgb(0, 0, 255)));
            canvas.end_layer();
        },
    )
}

/// The limited device's picture against an unlimited one's: the same, to a
/// count of rounding, because the reach the limit takes away only ever
/// covered ground past the canvas edge.
fn assert_same_picture(limited: &[u8], unlimited: &[u8], width: u32, sample: (u32, u32)) {
    assert_ne!(
        pixel(unlimited, width, sample.0, sample.1),
        [255, 255, 255],
        "the box is drawn"
    );
    let (worst, at) = limited
        .iter()
        .zip(unlimited)
        .enumerate()
        .map(|(i, (a, b))| ((*a as i32 - *b as i32).abs(), i / 4))
        .max()
        .unwrap_or((0, 0));
    let (x, y) = (at as u32 % width, at as u32 / width);
    assert!(
        worst <= 1,
        "the limited device's frame differs by up to {worst}/255 at ({x}, {y}): {:?} vs {:?}",
        pixel(limited, width, x, y),
        pixel(unlimited, width, x, y)
    );
}

/// A 2048 px store with a 1 px shadow offset used to round to 2112 and pass
/// through: the box drew opaque and cast nothing.
#[test]
fn a_full_width_layer_with_a_one_pixel_shadow_offset_keeps_its_opacity() {
    let (Some((device, queue)), Some((limited, limited_queue))) =
        (common::headless_device(), common::headless_device_limited(2048))
    else {
        return;
    };
    let rect = (100.0, 16.0, 200.0, 32.0);
    let full = shadowed_layer(&device, &queue, 2048, 64, 1.0, 0.0, rect);
    let small = shadowed_layer(&limited, &limited_queue, 2048, 64, 1.0, 0.0, rect);
    assert_same_picture(&small, &full, 2048, (200, 32));
}

/// A 1920 x 1080 store under a 60 px blur wants 2100 px: the reach is cut
/// to what the limit leaves and the layer still composites at its opacity,
/// its shadow spread around the box.
#[test]
fn a_full_hd_layer_with_a_wide_blur_keeps_its_opacity() {
    let (Some((device, queue)), Some((limited, limited_queue))) =
        (common::headless_device(), common::headless_device_limited(2048))
    else {
        return;
    };
    let rect = (800.0, 400.0, 320.0, 280.0);
    let full = shadowed_layer(&device, &queue, 1920, 1080, 0.0, 60.0, rect);
    let small = shadowed_layer(&limited, &limited_queue, 1920, 1080, 0.0, 60.0, rect);
    assert_same_picture(&small, &full, 1920, (960, 540));
}
