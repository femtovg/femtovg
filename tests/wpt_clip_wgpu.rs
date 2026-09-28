//! The web-platform-tests clip cases (`html/canvas/tools/yaml/path-objects.yaml`
//! `2d.path.clip.*`, `the-canvas-state.yaml` `2d.state.saverestore.clip*`,
//! `drawing-rectangles-to-the-canvas.yaml` `2d.clearRect.clip`) on
//! `Canvas::clip_path`, drawn on the suite's 100x50 canvas and asserted at
//! the suite's pixels. Canvas 2D's current path is caller-owned here, so
//! `2d.path.clip.unaffected` (clip must not consume the path) has no
//! equivalent; `2d.path.clip.scale` runs at the suite's semantics on the
//! smaller canvas. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, CompositeOperation, FillRule, Paint, Path};

mod common;
use common::headless_device;

const W: u32 = 100;
const H: u32 = 50;

fn render(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut Canvas<WGPURenderer>)) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("wpt clip test target"),
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
    queue.submit(canvas.flush_to_output(&target));

    let unpadded = W * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = unpadded.div_ceil(align) * align;
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
    let mut out = vec![0u8; (unpadded * H) as usize];
    for row in 0..H as usize {
        let src = row * padded as usize;
        let dst = row * unpadded as usize;
        out[dst..dst + unpadded as usize].copy_from_slice(&mapped[src..src + unpadded as usize]);
    }
    out
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2]]
}

const GREEN: [u8; 3] = [0, 255, 0];
const RED: [u8; 3] = [255, 0, 0];

fn fill_rect(canvas: &mut Canvas<WGPURenderer>, x: f32, y: f32, w: f32, h: f32, color: [u8; 3]) {
    let mut p = Path::new();
    p.rect(x, y, w, h);
    canvas.fill_path(&p, &Paint::color(Color::rgb(color[0], color[1], color[2])));
}

fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Path {
    let mut p = Path::new();
    p.rect(x, y, w, h);
    p
}

/// Runs one case and checks the suite's pixel at (50, 25).
fn case(name: &str, expected: [u8; 3], draw: impl FnOnce(&mut Canvas<WGPURenderer>)) {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, draw);
    assert_eq!(px(&out, 50, 25), expected, "{name}: pixel 50,25");
}

/// 2d.path.clip.empty: an empty path clips everything out.
#[test]
fn wpt_clip_empty() {
    case("2d.path.clip.empty", GREEN, |c| {
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, GREEN);
        c.clip_path(&Path::new(), FillRule::NonZero);
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, RED);
    });
}

/// 2d.path.clip.basic.1 / .2: a covering clip lets the fill through, a
/// clip off-canvas keeps everything out.
#[test]
fn wpt_clip_basic() {
    case("2d.path.clip.basic.1", GREEN, |c| {
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, RED);
        c.clip_path(&rect_path(0.0, 0.0, 100.0, 50.0), FillRule::NonZero);
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, GREEN);
    });
    case("2d.path.clip.basic.2", GREEN, |c| {
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, GREEN);
        c.clip_path(&rect_path(-100.0, 0.0, 100.0, 50.0), FillRule::NonZero);
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, RED);
    });
}

/// 2d.path.clip.intersect: two disjoint clips intersect to nothing.
#[test]
fn wpt_clip_intersect() {
    case("2d.path.clip.intersect", GREEN, |c| {
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, GREEN);
        c.clip_path(&rect_path(0.0, 0.0, 50.0, 50.0), FillRule::NonZero);
        c.clip_path(&rect_path(50.0, 0.0, 50.0, 50.0), FillRule::NonZero);
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, RED);
    });
}

/// 2d.path.clip.winding.evenodd.1: the same rect twice under evenodd is a
/// hole, so the clip is empty.
#[test]
fn wpt_clip_winding_evenodd() {
    case("2d.path.clip.winding.evenodd.1", GREEN, |c| {
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, GREEN);
        let mut p = Path::new();
        p.rect(0.0, 0.0, 100.0, 50.0);
        p.rect(0.0, 0.0, 100.0, 50.0);
        c.clip_path(&p, FillRule::EvenOdd);
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, RED);
    });
}

/// 2d.path.clip.winding.1: one contour winding the outer rect one way and
/// the inner rect the other is a hole under nonzero.
#[test]
fn wpt_clip_winding_nonzero_hole() {
    case("2d.path.clip.winding.1", GREEN, |c| {
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, GREEN);
        let mut p = Path::new();
        p.move_to(-10.0, -10.0);
        p.line_to(110.0, -10.0);
        p.line_to(110.0, 60.0);
        p.line_to(-10.0, 60.0);
        p.line_to(-10.0, -10.0);
        p.line_to(0.0, 0.0);
        p.line_to(0.0, 50.0);
        p.line_to(100.0, 50.0);
        p.line_to(100.0, 0.0);
        c.clip_path(&p, FillRule::NonZero);
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, RED);
    });
}

/// 2d.path.clip.winding.2: the same two loops as two successive clips
/// intersect to the inner rect instead.
#[test]
fn wpt_clip_winding_successive() {
    case("2d.path.clip.winding.2", GREEN, |c| {
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, RED);
        let mut outer = Path::new();
        outer.move_to(-10.0, -10.0);
        outer.line_to(110.0, -10.0);
        outer.line_to(110.0, 60.0);
        outer.line_to(-10.0, 60.0);
        outer.line_to(-10.0, -10.0);
        c.clip_path(&outer, FillRule::NonZero);
        let mut inner = Path::new();
        inner.move_to(0.0, 0.0);
        inner.line_to(0.0, 50.0);
        inner.line_to(100.0, 50.0);
        inner.line_to(100.0, 0.0);
        inner.line_to(0.0, 0.0);
        c.clip_path(&inner, FillRule::NonZero);
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, GREEN);
    });
}

/// 2d.path.clip.scale: a clip taken under scale(2) is stored transformed
/// (the Servo regression the case pins). The suite fills a 200x200 canvas;
/// here the 100x50 one: the clip's right edge lands at device x = 50, so
/// the fill reaches 50,25 only if the transform applied to the clip.
#[test]
fn wpt_clip_scale() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |c| {
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, RED);
        c.scale(2.0, 2.0);
        c.clip_path(&rect_path(0.0, 0.0, 30.0, 25.0), FillRule::NonZero); // device 0..60
        fill_rect(c, 0.0, 0.0, 50.0, 25.0, GREEN); // device 0..100
    });
    assert_eq!(px(&out, 50, 25), GREEN, "inside the scaled clip");
    assert_eq!(px(&out, 70, 25), RED, "past the scaled clip's edge (would be green if the clip were unscaled and covered 0..30 only... or the fill were unclipped)");
}

/// 2d.state.saverestore.clip and .clip.2: restore() drops the clips taken
/// since the save, one or two of them.
#[test]
fn wpt_saverestore_clip() {
    for (name, clips) in [("2d.state.saverestore.clip", 1), ("2d.state.saverestore.clip.2", 2)] {
        case(name, GREEN, move |c| {
            fill_rect(c, 0.0, 0.0, 100.0, 50.0, RED);
            c.save();
            for _ in 0..clips {
                c.clip_path(&rect_path(0.0, 0.0, 1.0, 1.0), FillRule::NonZero);
            }
            c.restore();
            fill_rect(c, 0.0, 0.0, 100.0, 50.0, GREEN);
        });
    }
}

/// 2d.clearRect.clip: Canvas 2D's clearRect is affected by the clip. Its
/// femtovg form is an opaque DestinationOut fill (`Canvas::clear_rect` is a
/// raw clear that the clip does not affect - see that method's docs).
#[test]
fn wpt_clear_rect_clip_as_destination_out() {
    case("2d.clearRect.clip", GREEN, |c| {
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, GREEN);
        c.clip_path(&rect_path(0.0, 0.0, 16.0, 16.0), FillRule::NonZero);
        c.global_composite_operation(CompositeOperation::DestinationOut);
        fill_rect(c, 0.0, 0.0, 100.0, 50.0, [0, 0, 0]);
        c.global_composite_operation(CompositeOperation::SourceOver);
        fill_rect(c, 0.0, 0.0, 16.0, 16.0, GREEN);
    });
}
