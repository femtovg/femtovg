//! Headless GPU regression test: a multi-stop gradient whose last stop sits
//! below 1.0 clamps to that stop's color for the rest of the ramp (SVG pad /
//! Canvas semantics). The LUT texels past the last stop used to be left
//! unwritten - transparent on a fresh texture, stale on a recycled one - which
//! cut a wedge out of the Firefox logo's flame (splash-logo.svg,
//! mr-settodefault.svg).
//!
//! The tail pixels alone cannot tell "unpadded LUT" from "the draw never
//! landed" or "the wrong texture was read": all three come back as the clear
//! color. So the frame carries controls that pin each of those down before
//! the tail is judged: a white gap proves the clear and the readback row
//! mapping, a solid magenta strip proves draws land in the target, and
//! samples inside the ramp prove the LUT is the one being sampled and holds
//! the right colors. Render and readback go to the queue in one submission.
//!
//! Set `FEMTOVG_REQUIRE_GPU=1` to fail instead of skip when no adapter is
//! found, or `FEMTOVG_REQUIRE_GPU=metal` (or another backend name) to also
//! insist on that backend; CI's Mac job sets `metal` so the test cannot pass
//! by not running.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, Paint, Path};

mod common;
use common::headless_device;

const W: u32 = 220;
const H: u32 = 64;

/// The gradient fills rows 0..40; row 20 is sampled.
const RAMP_BOTTOM: f32 = 40.0;
const RAMP_ROW: u32 = 20;
/// Rows 40..48 stay at the clear color; row 44 is sampled.
const GAP_ROW: u32 = 44;
/// A solid strip fills rows 48..64; row 56 is sampled.
const STRIP_TOP: f32 = 48.0;
const STRIP_ROW: u32 = 56;

const RED: [u8; 3] = [220, 40, 40];
const GREEN: [u8; 3] = [40, 200, 40];
const BLUE: [u8; 3] = [40, 80, 220];
const WHITE: [u8; 3] = [255, 255, 255];
/// Unlike anything in the ramp or the clear, so a strip pixel cannot be
/// mistaken for either.
const MAGENTA: [u8; 3] = [200, 0, 200];

/// Flushes the canvas into `target` and reads the target back. The render
/// command buffer and the copy are submitted together, in that order, so the
/// result cannot depend on the ordering of two separate submissions, and a
/// failed map is an error rather than silence.
fn render_and_read(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    canvas: &mut Canvas<WGPURenderer>,
    target: &wgpu::Texture,
) -> Vec<u8> {
    // A frame with nothing to draw yields no command buffer; here that would
    // itself be the bug, so it is an error rather than a quiet empty submit.
    let render = canvas
        .flush_to_output(target)
        .expect("flush_to_output produced no command buffer for a frame with draws");

    let unpadded = W * 4;
    let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("tail pad readback"),
        size: (padded * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("tail pad readback copy"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
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
    queue.submit([render, encoder.finish()]);

    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        sender.send(result).expect("map result receiver dropped");
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll failed while waiting for the readback");
    receiver
        .recv()
        .expect("map_async callback never ran")
        .expect("readback buffer map failed");
    let mapped = slice.get_mapped_range().expect("mapped range");
    let mut pixels = vec![0u8; (W * H * 4) as usize];
    for row in 0..H as usize {
        let src = row * padded as usize;
        let dst = row * unpadded as usize;
        pixels[dst..dst + unpadded as usize].copy_from_slice(&mapped[src..src + unpadded as usize]);
    }
    pixels
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

/// Linear mix of two ramp colors at `s` in 0..=1, in the un-premultiplied
/// 8-bit space the ramp is built in.
fn mix(a: [u8; 3], b: [u8; 3], s: f32) -> [u8; 3] {
    let lerp = |p: u8, q: u8| (p as f32 + (q as f32 - p as f32) * s).round() as u8;
    [lerp(a[0], b[0]), lerp(a[1], b[1]), lerp(a[2], b[2])]
}

fn within(got: [u8; 3], want: [u8; 3], tolerance: i32) -> bool {
    got.iter()
        .zip(want.iter())
        .all(|(g, w)| (*g as i32 - *w as i32).abs() <= tolerance)
}

#[test]
fn multi_stop_gradient_pads_past_last_stop() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tail pad out"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let renderer = WGPURenderer::new(device.clone(), queue.clone());
    let mut canvas = Canvas::new(renderer).expect("canvas");
    canvas.set_size(W, H, 1.0);
    canvas.clear_rect(0, 0, W, H, Color::white());

    // Three stops ending at 0.6 across the width; the right 40% must be blue.
    let color = |c: [u8; 3]| Color::rgb(c[0], c[1], c[2]);
    let stops = [(0.0, color(RED)), (0.3, color(GREEN)), (0.6, color(BLUE))];
    let paint = Paint::linear_gradient_stops(0.0, 0.0, W as f32, 0.0, stops);
    let mut ramp = Path::new();
    ramp.rect(0.0, 0.0, W as f32, RAMP_BOTTOM);
    canvas.fill_path(&ramp, &paint);

    // Solid control strip, drawn after the gradient through the ordinary fill
    // path with a plain color paint.
    let mut strip = Path::new();
    strip.rect(0.0, STRIP_TOP, W as f32, H as f32 - STRIP_TOP);
    canvas.fill_path(&strip, &Paint::color(color(MAGENTA)));

    let pixels = render_and_read(&device, &queue, &mut canvas, &target);

    let mut failures = Vec::new();
    let mut check = |x: u32, y: u32, want: [u8; 3], tolerance: i32, what: &str| {
        let got = pixel(&pixels, x, y);
        if !within(got, want, tolerance) {
            failures.push(format!("{what}: ({x},{y}) expected {want:?} ±{tolerance}, got {got:?}"));
        }
    };

    // Controls first: each one rules out a way the tail could be white for a
    // reason that has nothing to do with the ramp.
    for x in [5, 110, 214] {
        check(
            x,
            GAP_ROW,
            WHITE,
            1,
            "clear color in the untouched gap (clear + row mapping)",
        );
        check(x, STRIP_ROW, MAGENTA, 1, "solid strip (draws land in the target)");
    }
    // Inside the ramp: the LUT is being sampled, and holds the stops. The
    // ramp is quantized to 256 texels and sampled with filtering, hence the
    // tolerance; a stale or foreign texture is nowhere near it.
    let t = |x: u32| x as f32 / W as f32;
    check(1, RAMP_ROW, RED, 8, "first stop (ramp start)");
    check(
        33,
        RAMP_ROW,
        mix(RED, GREEN, t(33) / 0.3),
        8,
        "red-green interior of the ramp",
    );
    check(
        99,
        RAMP_ROW,
        mix(GREEN, BLUE, (t(99) - 0.3) / 0.3),
        8,
        "green-blue interior of the ramp",
    );
    check(
        126,
        RAMP_ROW,
        mix(GREEN, BLUE, (t(126) - 0.3) / 0.3),
        8,
        "approach to the last stop",
    );

    // The regression: past the last stop (t > 0.6, x > 132) the ramp must hold
    // the last stop's color, not the unwritten-texel transparent that shows
    // as the clear color.
    for x in [140, 150, 180, 210, 219] {
        check(
            x,
            RAMP_ROW,
            BLUE,
            2,
            "tail past the last stop must clamp to the last stop",
        );
    }

    if !failures.is_empty() {
        let row: Vec<String> = (0..W)
            .step_by(10)
            .map(|x| format!("{x}:{:?}", pixel(&pixels, x, RAMP_ROW)))
            .collect();
        eprintln!("ramp row {RAMP_ROW}: {}", row.join(" "));
        panic!("{} check(s) failed:\n  {}", failures.len(), failures.join("\n  "));
    }
}
