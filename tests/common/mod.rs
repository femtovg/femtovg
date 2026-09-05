//! Shared helpers for the headless WGPU tests: adapter/device setup and an
//! offscreen render that returns tightly packed RGBA8 rows.
#![allow(dead_code)]

use femtovg::{renderer::WGPURenderer, Canvas, Color};

/// Set `FEMTOVG_REQUIRE_GPU` to turn "no adapter" from a skip into a failure:
/// any value (`1`, `true`, `any`) requires that some adapter was found, and a
/// backend name (`metal`, `vulkan`, `dx12`, `gl`) additionally requires that
/// backend. CI's GPU job sets it, so the job cannot report success by quietly
/// skipping every test on a runner where adapter creation failed.
fn gpu_requirement() -> Option<String> {
    std::env::var("FEMTOVG_REQUIRE_GPU")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

pub fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let require = gpu_requirement();
    let instance = wgpu::Instance::default();
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
        ..Default::default()
    })) {
        Ok(adapter) => adapter,
        Err(err) => {
            assert!(
                require.is_none(),
                "FEMTOVG_REQUIRE_GPU is set but no wgpu adapter was found: {err}"
            );
            eprintln!("skipping: no wgpu adapter available ({err})");
            return None;
        }
    };

    // Which GPU ran the suite is the first thing anyone reading a CI failure
    // needs; print it once per test binary rather than once per test.
    let info = adapter.get_info();
    static ANNOUNCE: std::sync::Once = std::sync::Once::new();
    ANNOUNCE.call_once(|| {
        eprintln!(
            "wgpu adapter: {} | backend {:?} | type {:?} | driver {} {}",
            info.name, info.backend, info.device_type, info.driver, info.driver_info
        );
    });
    if let Some(required) = require.as_deref() {
        let backend = format!("{:?}", info.backend).to_ascii_lowercase();
        assert!(
            matches!(required, "1" | "true" | "any") || required == backend,
            "FEMTOVG_REQUIRE_GPU={required} but the adapter's backend is {backend}"
        );
    }

    let (device, queue) = match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("femtovg test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    })) {
        Ok(pair) => pair,
        Err(err) => {
            assert!(
                require.is_none(),
                "FEMTOVG_REQUIRE_GPU is set but the device request failed: {err}"
            );
            eprintln!("skipping: wgpu device request failed ({err})");
            return None;
        }
    };
    // A validation error must fail the test that provoked it, not scroll past
    // in the log while the test reads back plausible-looking pixels.
    device.on_uncaptured_error(std::sync::Arc::new(|err| panic!("wgpu uncaptured error: {err}")));
    Some((device, queue))
}

/// Renders `draw` on a fresh canvas of `width` x `height` cleared to `clear`
/// and returns the RGBA8 pixels, rows tightly packed.
pub fn render_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    clear: Color,
    draw: impl FnOnce(&mut Canvas<WGPURenderer>),
) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("femtovg test target"),
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
    });
    let renderer = WGPURenderer::new(device.clone(), queue.clone());
    let mut canvas = Canvas::new(renderer).expect("canvas");
    canvas.set_size(width, height, 1.0);
    canvas.clear_rect(0, 0, width, height, clear);
    draw(&mut canvas);
    // The render and the copy that reads it back go to the queue together, so
    // the pixels cannot depend on the ordering of two separate submissions.
    let commands = canvas
        .flush_to_output(&target)
        .expect("flush_to_output produced no command buffer for a frame with draws");

    let unpadded = width * 4;
    let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded * height) as u64,
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
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([commands, enc.finish()]);
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
    let mapped = slice.get_mapped_range().expect("readback");
    let mut pixels = vec![0u8; (unpadded * height) as usize];
    for row in 0..height as usize {
        let s = row * padded as usize;
        let d = row * unpadded as usize;
        pixels[d..d + unpadded as usize].copy_from_slice(&mapped[s..s + unpadded as usize]);
    }
    drop(mapped);
    readback.unmap();
    pixels
}
