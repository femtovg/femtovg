//! What the WGPU backend sets up for a frame. The viewport uniform and bind
//! group of a render target size outlive the flush that made them, for later
//! flushes to the same size, and the renderer keeps those of only so many
//! sizes. A frame's command buffer carries the vertices, fan indices and
//! uniforms it draws with, so frames recorded before any of them is submitted
//! each draw their own: a frame does not read data a later flush wrote.
//!
//! Live bind groups are read from wgpu's internal counters, enabled by the
//! `counters` feature on the `wgpu` dev-dependency.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, ImageFlags, Paint, Path, PixelFormat, RenderTarget};
use imgref::Img;
use rgb::RGBA8;

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

fn readback(device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::Texture) -> Vec<u8> {
    let unpadded = W * 4;
    let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll failed while waiting for the readback");
    let mapped = slice.get_mapped_range().expect("readback");
    let mut pixels = vec![0u8; (unpadded * H) as usize];
    for row in 0..H as usize {
        let (s, d) = (row * padded as usize, row * unpadded as usize);
        pixels[d..d + unpadded as usize].copy_from_slice(&mapped[s..s + unpadded as usize]);
    }
    pixels
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * W + x) * 4) as usize;
    pixels[offset..offset + 4].try_into().unwrap()
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

/// Two frames flushed one after the other and submitted together: each
/// target shows its own square, in its own color, where it was drawn.
#[test]
fn frames_flushed_before_a_submit_each_draw_their_own() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (left, right) = (target(&device, W, H), target(&device, W, H));
    let mut canvas = Canvas::new(WGPURenderer::new(device.clone(), queue.clone())).expect("canvas");
    canvas.set_size(W, H, 1.0);

    let (red, blue) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
    frame_with_square(&mut canvas, 4.0, red);
    let first = canvas.flush_to_output(&left).expect("a frame with draws");
    frame_with_square(&mut canvas, 36.0, blue);
    let second = canvas.flush_to_output(&right).expect("a frame with draws");
    queue.submit([first, second]);

    let (left, right) = (readback(&device, &queue, &left), readback(&device, &queue, &right));
    let black = [0, 0, 0, 255];
    assert_eq!(
        [pixel(&left, 16, 32), pixel(&left, 48, 32)],
        [[255, 0, 0, 255], black],
        "the first frame's target should hold its red square on the left and nothing on the right"
    );
    assert_eq!(
        [pixel(&right, 16, 32), pixel(&right, 48, 32)],
        [black, [0, 0, 255, 255]],
        "the second frame's target should hold its blue square on the right and nothing on the left"
    );
}

/// An image updated between two frames flushed one after the other and
/// submitted together: each frame draws the image as it was when that frame
/// was flushed, its upload carried in the frame's own command buffer.
#[test]
fn frames_flushed_before_a_submit_each_draw_the_image_they_were_given() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (first, second) = (target(&device, W, H), target(&device, W, H));
    let mut canvas = Canvas::new(WGPURenderer::new(device.clone(), queue.clone())).expect("canvas");
    canvas.set_size(W, H, 1.0);
    let red = vec![RGBA8::new(255, 0, 0, 255); 16];
    let blue = vec![RGBA8::new(0, 0, 255, 255); 16];
    let image = canvas
        .create_image(Img::new(red.as_slice(), 4, 4), ImageFlags::NEAREST)
        .expect("image");
    let mut whole = Path::new();
    whole.rect(0.0, 0.0, W as f32, H as f32);
    let paint = Paint::image(image, 0.0, 0.0, W as f32, H as f32, 0.0, 1.0);

    canvas.fill_path(&whole, &paint);
    let a = canvas.flush_to_output(&first);
    canvas
        .update_image(image, Img::new(blue.as_slice(), 4, 4), 0, 0)
        .expect("update");
    canvas.fill_path(&whole, &paint);
    let b = canvas.flush_to_output(&second);
    queue.submit(a.into_iter().chain(b));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll failed");

    assert_eq!(
        pixel(&readback(&device, &queue, &first), 32, 32),
        [255, 0, 0, 255],
        "the first frame's image"
    );
    assert_eq!(
        pixel(&readback(&device, &queue, &second), 32, 32),
        [0, 0, 255, 255],
        "the second frame's image"
    );
}

/// An image updated in a corner and then whole, the second upload larger
/// than a staging chunk, before the frame that draws it: the frame draws the
/// later upload. Every upload is copied in the frame's own command buffer in
/// the order it was made; the larger one, written on the queue, would have
/// been copied before the corner.
#[test]
fn an_upload_larger_than_a_staging_chunk_keeps_its_place_in_order() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let output = target(&device, W, H);
    let mut canvas = Canvas::new(WGPURenderer::new(device.clone(), queue.clone())).expect("canvas");
    canvas.set_size(W, H, 1.0);
    let side = 1024;
    let red = vec![RGBA8::new(255, 0, 0, 255); side * side];
    let image = canvas
        .create_image(Img::new(red.as_slice(), side, side), ImageFlags::NEAREST)
        .expect("image");
    let green = vec![RGBA8::new(0, 255, 0, 255); 16 * 16];
    canvas
        .update_image(image, Img::new(green.as_slice(), 16, 16), 0, 0)
        .expect("corner");
    let blue = vec![RGBA8::new(0, 0, 255, 255); side * side];
    canvas
        .update_image(image, Img::new(blue.as_slice(), side, side), 0, 0)
        .expect("whole");
    let mut whole = Path::new();
    whole.rect(0.0, 0.0, W as f32, H as f32);
    canvas.fill_path(&whole, &Paint::image(image, 0.0, 0.0, W as f32, H as f32, 0.0, 1.0));
    queue.submit(canvas.flush_to_output(&output));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll failed");

    assert_eq!(
        pixel(&readback(&device, &queue, &output), 0, 0),
        [0, 0, 255, 255],
        "the later, whole upload"
    );
}
