//! Headless GPU tests: an even-odd fill must leave the stencil clean for the
//! next fill. The winding pass writes the full count, so overlaps hold 2 and a
//! wrapped -1 holds 0xff; the cover pass reads parity (bit 0) but must clear
//! every bit, or the next nonzero fill paints its whole bounding quad over the
//! leftovers. Regression for DuckDuckGo's logo (duckduckgo-com@2x.svg): the
//! even-odd white body followed by the nonzero bow tie grew a dark-green block.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, FillRule, Paint, Path};

mod common;
use common::headless_device;

const W: u32 = 128;
const H: u32 = 128;

fn render(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut Canvas<WGPURenderer>)) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("evenodd stencil out"),
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
    draw(&mut canvas);
    let commands = canvas.flush_to_output(&target);
    queue.submit(commands);

    let unpadded = W * 4;
    let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
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
    queue.submit(Some(enc.finish()));
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = slice.get_mapped_range().expect("readback");
    let mut out = vec![0u8; (W * H * 4) as usize];
    for y in 0..H as usize {
        let src = y * padded as usize;
        let dst = y * (W * 4) as usize;
        out[dst..dst + (W * 4) as usize].copy_from_slice(&mapped[src..src + (W * 4) as usize]);
    }
    out
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2]]
}

/// A concave nonzero shape (an L) drawn after an even-odd fill: pixels inside
/// the L's bounding box but outside the L must stay white, whatever the
/// even-odd predecessor left in the stencil.
fn l_shape() -> Path {
    let mut p = Path::new();
    p.move_to(20.0, 20.0);
    p.line_to(60.0, 20.0);
    p.line_to(60.0, 40.0);
    p.line_to(40.0, 40.0);
    p.line_to(40.0, 108.0);
    p.line_to(20.0, 108.0);
    p.close();
    p
}

fn assert_l_only(out: &[u8], label: &str) {
    assert_eq!(
        px(out, 30, 30),
        [0, 160, 0],
        "{label}: inside the L's top bar should be green"
    );
    assert_eq!(
        px(out, 30, 100),
        [0, 160, 0],
        "{label}: inside the L's stem should be green"
    );
    // Inside the L's bounding box, outside the L: must be untouched.
    assert_eq!(
        px(out, 55, 80),
        [255, 255, 255],
        "{label}: the L's concavity must stay white"
    );
    assert_eq!(
        px(out, 55, 100),
        [255, 255, 255],
        "{label}: the L's concavity must stay white"
    );
}

/// Two overlapping same-direction squares under even-odd: the overlap holds
/// winding 2 (parity 0, unfilled). The next fill must not see those bits.
#[test]
fn evenodd_overlap_leaves_stencil_clean() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        let mut p = Path::new();
        p.rect(10.0, 10.0, 80.0, 80.0);
        p.rect(30.0, 30.0, 90.0, 90.0);
        let mut white = Paint::color(Color::white());
        white.set_fill_rule(FillRule::EvenOdd);
        canvas.fill_path(&p, &white);
        canvas.fill_path(&l_shape(), &Paint::color(Color::rgb(0, 160, 0)));
    });
    assert_l_only(&out, "overlap");
}

/// A single contour wound each way under even-odd: one direction wraps the
/// stencil to 0xff, which only clearing bit 0 would leave as 0xfe. Both
/// orientations must leave the next fill clean.
#[test]
fn evenodd_single_contour_leaves_stencil_clean_both_windings() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    for clockwise in [false, true] {
        let out = render(&device, &queue, |canvas| {
            // A concave contour, so the fill takes the stencil path.
            let mut p = Path::new();
            let pts = [
                (5.0, 5.0),
                (120.0, 5.0),
                (120.0, 120.0),
                (70.0, 120.0),
                (70.0, 60.0),
                (5.0, 60.0),
            ];
            let order: Vec<(f32, f32)> = if clockwise {
                pts.to_vec()
            } else {
                pts.iter().rev().copied().collect()
            };
            p.move_to(order[0].0, order[0].1);
            for &(x, y) in &order[1..] {
                p.line_to(x, y);
            }
            p.close();
            let mut white = Paint::color(Color::white());
            white.set_fill_rule(FillRule::EvenOdd);
            canvas.fill_path(&p, &white);
            canvas.fill_path(&l_shape(), &Paint::color(Color::rgb(0, 160, 0)));
        });
        assert_l_only(&out, if clockwise { "clockwise" } else { "counter-clockwise" });
    }
}
