//! Live render pipeline counts for the tests of the WGPU pipeline cache, read
//! from wgpu's internal counters, enabled by the `counters` feature on the
//! `wgpu` dev-dependency.
use femtovg::{renderer::WGPURenderer, Canvas, Path};

/// Large enough to hold the 48 px square at (8, 8); the tests count pipelines, not pixels.
pub const SIZE: u32 = 64;

pub fn target(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pipeline test target"),
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

pub fn rect() -> Path {
    let mut path = Path::new();
    path.rect(8.0, 8.0, 48.0, 48.0);
    path
}

/// Flushes `canvas` into `target` and returns how many render pipelines are alive on `device`.
pub fn live_pipelines_after_flush(
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
