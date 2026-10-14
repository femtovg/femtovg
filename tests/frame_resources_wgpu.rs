//! What the WGPU backend sets up for a frame. The viewport uniform and bind
//! group of a render target size outlive the flush that made them, for later
//! flushes to the same size, and the renderer keeps those of only so many
//! sizes.
//!
//! Live bind groups are read from wgpu's internal counters, enabled by the
//! `counters` feature on the `wgpu` dev-dependency.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, ImageFlags, Paint, Path, PixelFormat, RenderTarget};

mod common;
use common::headless_device;

const W: u32 = 64;
const H: u32 = 64;

fn target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frame resources target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// Clears the canvas to black and fills a square of `color` at `x`.
fn frame_with_square(canvas: &mut Canvas<WGPURenderer>, x: f32, color: Color) {
    canvas.clear_rect(0, 0, W, H, Color::black());
    let mut square = Path::new();
    square.rect(x, 16.0, 24.0, 32.0);
    canvas.fill_path(&square, &Paint::color(color));
}

/// Flushes `canvas`, a clear and a square, into `target`, waits for the GPU
/// and returns how many bind groups are alive on `device`.
fn live_bind_groups_after_flush(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    canvas: &mut Canvas<WGPURenderer>,
    target: &wgpu::Texture,
) -> isize {
    frame_with_square(canvas, 4.0, Color::rgb(255, 0, 0));
    queue.submit(canvas.flush_to_output(target));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll failed");
    let live = device.get_internal_counters().hal.bind_groups.read();
    assert!(
        live > 0,
        "no live bind groups counted; is wgpu's `counters` feature on?"
    );
    live
}

/// Flushes to two outputs of different sizes in turn: the second keeps the
/// first size's viewport bind group alive beside its own, and flushing to
/// either again makes no more.
#[test]
fn a_viewport_bind_group_outlives_flushes_to_other_sizes() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (large, small) = (target(&device, W, H), target(&device, W / 2, H / 2));
    let mut canvas = Canvas::new(WGPURenderer::new(device.clone(), queue.clone())).expect("canvas");
    canvas.set_size(W, H, 1.0);

    let live: Vec<isize> = [&large, &small, &large, &small]
        .into_iter()
        .map(|output| live_bind_groups_after_flush(&device, &queue, &mut canvas, output))
        .collect();
    assert_eq!(
        live,
        [live[0], live[0] + 1, live[0] + 1, live[0] + 1],
        "bind groups alive after flushes to {W} x {H}, {} x {}, then both again",
        W / 2,
        H / 2
    );
}

/// Each frame fills an image target of a size no frame used before, then the
/// screen. The renderer keeps the viewport bind groups of a bounded number of
/// sizes, so past that bound the live bind groups stop growing.
#[test]
fn viewport_bind_groups_past_a_bound_are_dropped_least_recently_used_first() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let output = target(&device, W, H);
    let mut canvas = Canvas::new(WGPURenderer::new(device.clone(), queue.clone())).expect("canvas");
    canvas.set_size(W, H, 1.0);

    // Twice as many sizes as the renderer keeps.
    let frames = 256;
    let live: Vec<isize> = (0..frames)
        .map(|frame| {
            let side = 8 + frame as u32;
            let image = canvas
                .create_image_empty(
                    side as usize,
                    side as usize,
                    PixelFormat::Rgba8,
                    ImageFlags::PREMULTIPLIED,
                )
                .expect("image target");
            canvas.set_render_target(RenderTarget::Image(image));
            canvas.clear_rect(0, 0, side, side, Color::white());
            canvas.set_render_target(RenderTarget::Screen);
            let live = live_bind_groups_after_flush(&device, &queue, &mut canvas, &output);
            canvas.delete_image(image);
            live
        })
        .collect();
    let last = live[frames - 1];
    assert!(
        last > live[0],
        "the bind groups of earlier sizes were not kept: {} after the first frame, {last} after the last",
        live[0]
    );
    assert!(
        live[frames - 16..].iter().all(|&count| count == last),
        "bind groups alive after the last frames, each with an image target of a new size: {:?}",
        &live[frames - 16..]
    );
}
