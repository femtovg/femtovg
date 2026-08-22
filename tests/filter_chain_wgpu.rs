//! Headless GPU tests for filter-chain execution (`Canvas::filter_image_chain`),
//! locking the regression classes browsers shipped in multi-filter chains:
//! fold-equals-sequential, order sensitivity, orientation stability across
//! chain shapes, alpha clamping between passes (Firefox bug 1577566), and
//! degenerate blur parameters (Firefox bugs 619968 / 441368). Skips without a
//! GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, ImageFilter, ImageFlags, Paint, Path, PixelFormat};

const W: u32 = 32;
const H: u32 = 32;

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
        label: Some("femtovg filter chain test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .ok()?;
    Some((device, queue))
}

/// Uploads `source_pixels` (row-major RGBA), runs `chain` into a FLIP_Y
/// target, composites onto white, and returns the full RGBA readback.
fn run_chain(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source_pixels: &[femtovg::rgb::RGBA8],
    chain: &[ImageFilter],
) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("chain out"),
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

    let source = canvas
        .create_image(
            femtovg::imgref::Img::new(source_pixels, W as usize, H as usize),
            ImageFlags::empty(),
        )
        .expect("source image");
    let filtered = canvas
        .create_image_empty(
            W as usize,
            H as usize,
            PixelFormat::Rgba8,
            ImageFlags::FLIP_Y | ImageFlags::PREMULTIPLIED,
        )
        .expect("target image");
    canvas.filter_image_chain(filtered, chain, source);

    canvas.clear_rect(0, 0, W, H, Color::white());
    let mut p = Path::new();
    p.rect(0.0, 0.0, W as f32, H as f32);
    canvas.fill_path(&p, &Paint::image(filtered, 0.0, 0.0, W as f32, H as f32, 0.0, 1.0));

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

fn solid(color: femtovg::rgb::RGBA8) -> Vec<femtovg::rgb::RGBA8> {
    vec![color; (W * H) as usize]
}

fn close(a: u8, b: u8, tol: i32) -> bool {
    (a as i32 - b as i32).abs() <= tol
}

/// A folded color run must render identically to executing the same run as a
/// chain - the property that lets N color ops cost one pass.
#[test]
fn chained_color_matrices_match_the_fold() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let src = solid(femtovg::rgb::RGBA8::new(180, 90, 40, 255));
    let first = ImageFilter::sepia(0.8);
    let second = ImageFilter::hue_rotate(1.1);
    let folded = first.fold_with(&second).expect("folds");

    let chained = run_chain(&device, &queue, &src, &[first, second]);
    let one_pass = run_chain(&device, &queue, &src, &[folded]);
    let a = px(&chained, 16, 16);
    let b = px(&one_pass, 16, 16);
    for c in 0..3 {
        assert!(close(a[c], b[c], 1), "chain {a:?} vs folded {b:?}");
    }
}

/// blur-then-brighten is not brighten-then-blur: the order of passes must be
/// preserved across the fold boundaries.
#[test]
fn chain_order_is_preserved_across_pass_boundaries() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    // Left half dark, right half bright: blurring first smears before the
    // brightness clamp, brightening first clamps before the smear - the
    // blurred boundary column differs.
    let mut src = solid(femtovg::rgb::RGBA8::new(40, 40, 40, 255));
    for y in 0..H as usize {
        for x in (W / 2) as usize..W as usize {
            src[y * W as usize + x] = femtovg::rgb::RGBA8::new(200, 200, 200, 255);
        }
    }
    let blur = ImageFilter::GaussianBlur { sigma: 3.0 };
    let bright = ImageFilter::brightness(1.8);
    let ab = run_chain(&device, &queue, &src, &[blur, bright]);
    let ba = run_chain(&device, &queue, &src, &[bright, blur]);
    let a = px(&ab, W / 2, H / 2);
    let b = px(&ba, W / 2, H / 2);
    assert!(
        (a[0] as i32 - b[0] as i32).abs() > 8,
        "blur-then-brighten {a:?} should differ from brighten-then-blur {b:?} at the boundary"
    );
}

/// The orientation contract holds for every chain shape: an asymmetric source
/// keeps red on top and blue on the bottom through odd and even numbers of
/// flipping passes, with and without blurs (the render-target FLIP_Y class).
#[test]
fn chain_orientation_is_stable_across_shapes() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let mut src = solid(femtovg::rgb::RGBA8::new(0, 0, 255, 255));
    for i in 0..(W * H / 2) as usize {
        src[i] = femtovg::rgb::RGBA8::new(255, 0, 0, 255);
    }
    let cm = ImageFilter::brightness(1.0);
    let blur = ImageFilter::GaussianBlur { sigma: 0.5 };
    let chains: [&[ImageFilter]; 5] = [&[], &[cm], &[cm, cm], &[blur], &[blur, cm, blur]];
    for (i, chain) in chains.iter().enumerate() {
        let out = run_chain(&device, &queue, &src, chain);
        let top = px(&out, W / 2, 3);
        let bottom = px(&out, W / 2, H - 4);
        assert!(
            top[0] > 200 && top[2] < 60,
            "chain #{i}: top should stay red, got {top:?}"
        );
        assert!(
            bottom[2] > 200 && bottom[0] < 60,
            "chain #{i}: bottom should stay blue, got {bottom:?} - a swapped pair means a mirrored chain"
        );
    }
}

/// Semi-transparent content keeps its color through multi-pass chains: the
/// scratches and target carry the premultiplied convention, so alpha is not
/// re-applied at every pass boundary (which darkened content per pass).
#[test]
fn semitransparent_content_survives_chains() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    // Premultiplied half-alpha green.
    let src = solid(femtovg::rgb::RGBA8::new(20, 90, 20, 128));
    let out = run_chain(
        &device,
        &queue,
        &src,
        &[ImageFilter::brightness(1.0), ImageFilter::GaussianBlur { sigma: 1.0 }],
    );
    let center = px(&out, 16, 16);
    // Over white: 0.5*(40,180,40) + 0.5*255 = (147, 217, 147).
    assert!(
        close(center[0], 147, 8) && close(center[1], 217, 8) && close(center[2], 147, 8),
        "half-alpha green through a two-pass chain should stay green, got {center:?}"
    );
}

/// An alpha-amplifying matrix feeding a blur must clamp between passes
/// instead of blowing out to white (the Firefox bug 1577566 class).
#[test]
fn alpha_amplifying_matrix_clamps_between_passes() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    // Premultiplied source (femtovg's image convention): color 60/120/200 at
    // alpha 0.5 stores as 30/60/100.
    let src = solid(femtovg::rgb::RGBA8::new(30, 60, 100, 128));
    // Alpha row scales by 100; color rows pass through.
    let mut m = [0.0f32; 20];
    m[0] = 1.0;
    m[6] = 1.0;
    m[12] = 1.0;
    m[18] = 100.0;
    let amplify = ImageFilter::ColorMatrix { matrix: m };
    let out = run_chain(
        &device,
        &queue,
        &src,
        &[amplify, ImageFilter::GaussianBlur { sigma: 1.0 }],
    );
    let center = px(&out, 16, 16);
    // Alpha clamps to 1.0, so the composite over white shows the source color
    // itself; unclamped alpha would wash the color toward white or blow out.
    assert!(
        close(center[0], 60, 12) && close(center[1], 120, 12) && close(center[2], 200, 12),
        "amplified-alpha chain should clamp and show the source color, got {center:?}"
    );
}

/// Degenerate blur parameters flow through chains without killing the output:
/// sigma 0 passes through (Firefox bug 619968) and a huge sigma stays finite
/// and bounded rather than overflowing (Firefox bug 441368).
#[test]
fn degenerate_blur_parameters_stay_bounded() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let src = solid(femtovg::rgb::RGBA8::new(200, 60, 60, 255));
    for chain in [
        &[ImageFilter::GaussianBlur { sigma: 0.0 }, ImageFilter::brightness(1.0)][..],
        &[
            ImageFilter::GaussianBlur { sigma: 2147483648.0 },
            ImageFilter::brightness(1.0),
        ][..],
    ] {
        let out = run_chain(&device, &queue, &src, chain);
        let center = px(&out, 16, 16);
        assert!(
            close(center[0], 200, 20) && close(center[1], 60, 20),
            "degenerate-sigma chain must keep the solid color, got {center:?}"
        );
    }
}
