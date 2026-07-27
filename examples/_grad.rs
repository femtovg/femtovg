//! Offscreen zoom-ladder scene for the fork-PR cross-check GIFs.
#![cfg(feature = "wgpu")]

#[allow(unused_imports)]
use femtovg::{renderer::WGPURenderer, Canvas, Color, ImageFlags, Paint, Path};

const W: u32 = 460;
const H: u32 = 260;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let scale: f32 = args[1].parse().expect("scale");
    let out = &args[2];

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .unwrap();

    let renderer = WGPURenderer::new(device.clone(), queue.clone());
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(W, H, 1.0);

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    use femtovg::Transform2D;
    canvas.clear_rect(0, 0, W, H, Color::white());
    canvas.save();
    canvas.translate(230.0, 130.0);
    canvas.scale(scale, scale);
    canvas.translate(-230.0, -130.0);

    let red = Color::rgb(230, 60, 40);
    let blue = Color::rgb(40, 60, 230);
    let green = Color::rgb(30, 160, 60);
    let magenta = Color::rgb(200, 40, 180);

    // r1: Babylon-style two-point radial - independent centres, overlapping circles.
    let g1 = Paint::two_point_radial_gradient(80.0, 70.0, 25.0, 130.0, 70.0, 70.0, red, blue);
    let mut p = Path::new();
    p.rect(30.0, 20.0, 190.0, 100.0);
    canvas.fill_path(&p, &g1);

    // r2: focal cone - tiny start circle offset inside the big one (the case
    // concentric-only implementations cannot express).
    let g2 = Paint::two_point_radial_gradient(290.0, 45.0, 4.0, 330.0, 70.0, 55.0, green, magenta);
    let mut p = Path::new();
    p.rect(250.0, 20.0, 180.0, 100.0);
    canvas.fill_path(&p, &g2);

    // r3: the design-tool ellipse idiom - unit circle + gradientTransform.
    let g3 = Paint::two_point_radial_gradient(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, red, blue)
        .with_gradient_transform(Transform2D([90.0, 0.0, 0.0, 42.0, 125.0, 190.0]));
    let mut p = Path::new();
    p.rect(30.0, 140.0, 190.0, 100.0);
    canvas.fill_path(&p, &g3);

    // r4: gradientTransform on a linear gradient - unit-x axis rotated 25deg,
    // scaled to 160 and placed at (260,150): matrix = translate * rotate * scale.
    let (sin, cos) = (25.0f32).to_radians().sin_cos();
    let g4 = Paint::linear_gradient(0.0, 0.0, 1.0, 0.0, green, magenta)
        .with_gradient_transform(Transform2D([160.0 * cos, 160.0 * sin, -160.0 * sin, 160.0 * cos, 260.0, 150.0]));
    let mut p = Path::new();
    p.rect(250.0, 140.0, 180.0, 100.0);
    canvas.fill_path(&p, &g4);

    canvas.restore();

    let commands = canvas.flush_to_output(&target);
    queue.submit(commands);

    let unpadded = W * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = unpadded.div_ceil(align) * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
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
        wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
    );
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = slice.get_mapped_range().unwrap();

    let mut ppm = format!("P6\n{W} {H}\n255\n").into_bytes();
    for row in 0..H as usize {
        let src = row * padded as usize;
        for px in 0..W as usize {
            let i = src + px * 4;
            ppm.extend_from_slice(&mapped[i..i + 3]);
        }
    }
    std::fs::write(out, ppm).unwrap();
}
