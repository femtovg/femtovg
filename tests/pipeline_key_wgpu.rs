//! The WGPU backend builds a render pipeline only from fixed-function state:
//! blend, topology, cull mode, stencil and render target. The shader type and
//! the glyph texture reach the shader through the uniform and the bind group,
//! so a pipeline cache keyed on them built identical pipelines once per shader
//! type, and again for glyph quads.
//!
//! Live pipelines are read from wgpu's internal counters, which the `counters`
//! feature on the `wgpu` dev-dependency turns on.
#![cfg(feature = "wgpu")]

use femtovg::{
    renderer::WGPURenderer, Canvas, Color, DrawCommand, GlyphDrawCommands, ImageFlags, Paint, Path, PixelFormat, Quad,
};

mod common;
use common::headless_device;

/// Large enough to hold the 48 px square at (8, 8); the tests count pipelines, not pixels.
const SIZE: u32 = 64;

fn target(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pipeline key target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

fn rect() -> Path {
    let mut path = Path::new();
    path.rect(8.0, 8.0, 48.0, 48.0);
    path
}

/// Flushes `canvas` into `target` and returns how many render pipelines are alive on `device`.
fn live_pipelines_after_flush(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    canvas: &mut Canvas<WGPURenderer>,
    target: &wgpu::Texture,
) -> isize {
    let commands = canvas
        .flush_to_output(target)
        .expect("flush_to_output produced no command buffer for a frame with draws");
    queue.submit([commands]);
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll failed");
    let live = device.get_internal_counters().hal.render_pipelines.read();
    // Without the `counters` feature every counter reads zero, and the tests' comparisons would pass vacuously.
    assert!(
        live > 0,
        "no live render pipelines counted; is wgpu's `counters` feature on?"
    );
    live
}

/// Filling one rectangle with each kind of paint, eight shader types between
/// them, needs no pipeline beyond those a solid fill of it needs. Before the fix
/// the solid fill left 2 pipelines alive and the ten paints 16.
#[test]
fn fills_that_differ_only_in_paint_share_their_pipelines() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let target = target(&device);
    let mut canvas = Canvas::new(WGPURenderer::new(device.clone(), queue.clone())).expect("canvas");
    canvas.set_size(SIZE, SIZE, 1.0);
    let pattern = canvas
        .create_image_empty(4, 4, PixelFormat::Rgba8, ImageFlags::empty())
        .expect("pattern image");

    let (red, blue) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
    let stops = [(0.0, red), (0.5, Color::rgb(0, 255, 0)), (1.0, blue)];
    let solid = Paint::color(red);

    canvas.fill_path(&rect(), &solid);
    let solid_only = live_pipelines_after_flush(&device, &queue, &mut canvas, &target);

    let paints = [
        solid,
        Paint::linear_gradient(8.0, 0.0, 56.0, 0.0, red, blue),
        Paint::linear_gradient_stops(8.0, 0.0, 56.0, 0.0, stops),
        Paint::radial_gradient(32.0, 32.0, 4.0, 24.0, red, blue),
        Paint::box_gradient(16.0, 16.0, 32.0, 32.0, 4.0, 8.0, red, blue),
        Paint::conic_gradient(32.0, 32.0, red, blue),
        Paint::conic_gradient_stops(32.0, 32.0, stops),
        Paint::two_point_radial_gradient(24.0, 32.0, 4.0, 40.0, 32.0, 24.0, red, blue),
        Paint::two_point_radial_gradient_stops(24.0, 32.0, 4.0, 40.0, 32.0, 24.0, stops),
        Paint::image(pattern, 8.0, 8.0, 4.0, 4.0, 0.0, 1.0),
    ];
    for paint in &paints {
        canvas.fill_path(&rect(), paint);
    }
    let every_paint = live_pipelines_after_flush(&device, &queue, &mut canvas, &target);

    assert_eq!(
        every_paint, solid_only,
        "a solid fill needs {solid_only} pipelines, but the same fill in every kind of paint left {every_paint} alive"
    );
}

/// A glyph quad is drawn with the same fixed-function state as the interior of
/// a convex fill, so it shares that fill's pipeline. Before the fix the solid
/// fill left 2 pipelines alive and the fill plus a glyph quad 3.
#[test]
fn glyph_quads_share_the_pipeline_of_a_plain_fill() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let target = target(&device);
    let mut canvas = Canvas::new(WGPURenderer::new(device.clone(), queue.clone())).expect("canvas");
    canvas.set_size(SIZE, SIZE, 1.0);
    let atlas = canvas
        .create_image_empty(8, 8, PixelFormat::Gray8, ImageFlags::empty())
        .expect("glyph atlas");
    let solid = Paint::color(Color::rgb(255, 0, 0));

    canvas.fill_path(&rect(), &solid);
    let fill_only = live_pipelines_after_flush(&device, &queue, &mut canvas, &target);

    canvas.fill_path(&rect(), &solid);
    let glyph = Quad {
        x0: 8.0,
        y0: 8.0,
        s0: 0.0,
        t0: 0.0,
        x1: 16.0,
        y1: 16.0,
        s1: 1.0,
        t1: 1.0,
    };
    canvas.draw_glyph_commands(
        GlyphDrawCommands {
            alpha_glyphs: vec![DrawCommand {
                image_id: atlas,
                quads: vec![glyph],
            }],
            color_glyphs: Vec::new(),
        },
        &solid,
    );
    let with_glyphs = live_pipelines_after_flush(&device, &queue, &mut canvas, &target);

    assert_eq!(
        with_glyphs, fill_only,
        "a solid fill needs {fill_only} pipelines, but adding a glyph quad in the same paint left {with_glyphs} alive"
    );
}
