//! Headless GPU test: a multi-stop gradient whose last stop sits below 1.0
//! clamps to that stop's color for the rest of the ramp (SVG pad / Canvas
//! semantics). The LUT texels past the last stop used to be left unwritten -
//! transparent on a fresh texture, stale on a recycled one - which cut a wedge
//! out of the Firefox logo's flame (splash-logo.svg, mr-settodefault.svg).
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, Paint, Path};

const W: u32 = 220;
const H: u32 = 64;

fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
        ..Default::default()
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("femtovg gradient dither test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .ok()?;
    Some((device, queue))
}

#[test]
fn multi_stop_gradient_pads_past_last_stop() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
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
    let stops = vec![
        (0.0, Color::rgb(220, 40, 40)),
        (0.3, Color::rgb(40, 200, 40)),
        (0.6, Color::rgb(40, 80, 220)),
    ];
    let paint = Paint::linear_gradient_stops(0.0, 0.0, W as f32, 0.0, stops);
    let mut p = Path::new();
    p.rect(0.0, 0.0, W as f32, H as f32);
    canvas.fill_path(&p, &paint);
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
    let px = |x: usize| {
        let i = (H as usize / 2) * padded as usize + x * 4;
        [mapped[i] as i32, mapped[i + 1] as i32, mapped[i + 2] as i32]
    };
    for x in [150usize, 180, 210, 219] {
        let p = px(x);
        assert!(
            (p[0] - 40).abs() <= 2 && (p[1] - 80).abs() <= 2 && (p[2] - 220).abs() <= 2,
            "x={x} past the last stop must clamp to blue, got {p:?}"
        );
    }
}
