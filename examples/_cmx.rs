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

    // 64x64 blue/yellow checker, 8px cells.
    let mut buf = vec![femtovg::rgb::RGBA8::new(0, 0, 0, 255); 64 * 64];
    for y in 0..64usize {
        for x in 0..64usize {
            let on = ((x >> 3) + (y >> 3)) % 2 == 0;
            buf[y * 64 + x] = if on {
                femtovg::rgb::RGBA8::new(60, 120, 220, 255)
            } else {
                femtovg::rgb::RGBA8::new(240, 200, 60, 255)
            };
        }
    }
    let src = canvas
        .create_image(femtovg::imgref::Img::new(buf.as_slice(), 64, 64), ImageFlags::empty())
        .unwrap();

    canvas.clear_rect(0, 0, W, H, Color::white());
    canvas.save();
    canvas.translate(230.0, 130.0);
    canvas.scale(scale, scale);
    canvas.translate(-230.0, -130.0);

    // Six filters, one 64x64 tile each, drawn at native size.
    let filters: [(f32, f32, femtovg::ImageFilter); 6] = [
        (50.0, 30.0, femtovg::ImageFilter::sepia(1.0)),
        (180.0, 30.0, femtovg::ImageFilter::saturate(0.3)),
        (
            310.0,
            30.0,
            femtovg::ImageFilter::hue_rotate(std::f32::consts::FRAC_PI_2),
        ),
        (50.0, 150.0, femtovg::ImageFilter::grayscale(1.0)),
        (180.0, 150.0, femtovg::ImageFilter::brightness(1.4)),
        (310.0, 150.0, femtovg::ImageFilter::invert(1.0)),
    ];
    for (x, y, filter) in filters {
        // Filter render targets store content flipped; FLIP_Y samples upright.
        let dst = canvas
            .create_image_empty(64, 64, femtovg::PixelFormat::Rgba8, ImageFlags::FLIP_Y)
            .unwrap();
        canvas.filter_image(dst, filter, src);
        let paint = Paint::image(dst, x, y, 64.0, 64.0, 0.0, 1.0);
        let mut p = Path::new();
        p.rect(x, y, 64.0, 64.0);
        canvas.fill_path(&p, &paint);
    }

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
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
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
