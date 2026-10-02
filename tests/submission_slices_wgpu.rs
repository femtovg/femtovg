#![cfg(feature = "wgpu")]
//! `WGPURenderer::set_submission_slicing` submits a frame in slices of render
//! passes: wgpu opens a Metal command buffer per pass and the driver keeps
//! about 2.3 MiB for each until it completes (gfx-rs/wgpu#5738), and one
//! submission per frame reached gigabytes (1,600 opacity layers failed buffer
//! creation on an M4 Max; a 176-layer portrait was jetsammed on an iPhone
//! 12). A slice must hold at most the configured passes wherever the passes
//! come from, every slice's uniforms and draws must land in it, in order,
//! and the picture must not depend on where the frame was cut. A pass is
//! only begun by a draw, so the counts here are of passes that draw.

use femtovg::{
    renderer::{SubmissionSlicing, WGPURenderer},
    BlendMode, Canvas, Color, FillRule, ImageFilter, ImageFlags, LayerEffects, MaskKind, Paint, Path, PixelFormat,
    RenderTarget,
};

mod common;
use common::{headless_device, render_rgba_on};

type C = Canvas<WGPURenderer>;

fn slices_of(passes: u32) -> Option<SubmissionSlicing> {
    Some(SubmissionSlicing { passes })
}

/// Renders `draw` with the given slicing and returns the pixels and the
/// passes per command buffer.
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    slicing: Option<SubmissionSlicing>,
    draw: impl FnOnce(&mut C),
) -> (Vec<u8>, Vec<u32>) {
    let mut renderer = WGPURenderer::new(device.clone(), queue.clone());
    renderer.set_submission_slicing(slicing);
    let (pixels, canvas) = render_rgba_on(device, queue, width, height, Color::white(), renderer, draw);
    (pixels, canvas.renderer().last_frame_slices().to_vec())
}

fn fill(c: &mut C, x: f32, y: f32, w: f32, h: f32, color: Color) {
    let mut p = Path::new();
    p.rect(x, y, w, h);
    let mut paint = Paint::color(color);
    paint.set_anti_alias(false);
    c.fill_path(&p, &paint);
}

fn cell(i: usize) -> [u8; 3] {
    [(i * 37 % 256) as u8, (i * 91 % 256) as u8, (i * 53 % 256) as u8]
}

fn half_over_white(c: u8) -> i32 {
    (i32::from(c) + 255 + 1) / 2
}

/// One half-opacity layer per pixel of a `width`-wide canvas: two passes each.
fn one_layer_per_pixel(c: &mut C, layers: usize, width: u32) {
    let half = LayerEffects::new().with_opacity(0.5);
    for i in 0..layers {
        assert!(c.begin_layer(&half));
        let [r, g, b] = cell(i);
        fill(
            c,
            (i % width as usize) as f32,
            (i / width as usize) as f32,
            1.0,
            1.0,
            Color::rgb(r, g, b),
        );
        c.end_layer();
    }
}

fn assert_same_pixels(sliced: &[u8], unsliced: &[u8], what: &str) {
    assert_eq!(sliced.len(), unsliced.len());
    if let Some(i) = (0..sliced.len()).find(|&i| sliced[i] != unsliced[i]) {
        panic!(
            "{what}: byte {i} differs, sliced {} vs unsliced {}",
            sliced[i], unsliced[i]
        );
    }
}

#[test]
fn every_layer_of_a_long_frame_lands_in_its_slice() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    const W: u32 = 64;
    const H: u32 = 5;
    // One pass on the screen for the clear, then two per layer, its content
    // and the screen pass its composite is drawn in: 320 layers are 641
    // passes, ten full slices and a tail of one.
    let layers = (W * H) as usize;
    let (out, slices) = render(&device, &queue, W, H, Some(SubmissionSlicing::default()), |c| {
        one_layer_per_pixel(c, layers, W)
    });
    assert_eq!(slices, [[64; 10].as_slice(), &[1]].concat());
    for i in 0..layers {
        let got = &out[i * 4..i * 4 + 3];
        let want = cell(i).map(half_over_white);
        for ch in 0..3 {
            assert!(
                (i32::from(got[ch]) - want[ch]).abs() <= 1,
                "layer {i}: pixel {got:?}, expected {want:?} at half opacity over white"
            );
        }
    }
}

#[test]
fn a_slice_holds_at_most_the_configured_passes_at_63_64_and_65() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    // One pass on the screen for the clear and two per layer; an image
    // drawn into is one more, and blurring it and drawing the result three
    // more: the blur's two directions and the screen pass of the fill.
    let scenes: [(&str, usize, bool, bool, Vec<u32>); 3] = [
        ("29 layers and a blur", 29, true, true, vec![63]),
        ("31 layers and an image drawn into", 31, true, false, vec![64]),
        ("30 layers and a blur", 30, true, true, vec![64, 1]),
    ];
    for (what, layers, image, blur, want) in scenes {
        let draw = |c: &mut C| {
            one_layer_per_pixel(c, layers, 32);
            if image {
                let source = c
                    .create_image_empty(8, 8, PixelFormat::Rgba8, ImageFlags::empty())
                    .unwrap();
                c.with_render_target(RenderTarget::Image(source), |c| {
                    c.clear_rect(0, 0, 8, 8, Color::rgba(0, 0, 0, 0));
                    fill(c, 2.0, 2.0, 4.0, 4.0, Color::rgb(0, 0, 255));
                });
                if blur {
                    let blurred = c
                        .create_image_empty(8, 8, PixelFormat::Rgba8, ImageFlags::empty())
                        .unwrap();
                    c.filter_image(blurred, ImageFilter::GaussianBlur { sigma: 2.0 }, source);
                    let mut p = Path::new();
                    p.rect(20.0, 2.0, 8.0, 8.0);
                    c.fill_path(&p, &Paint::image(blurred, 20.0, 2.0, 8.0, 8.0, 0.0, 1.0));
                }
            }
        };
        let (unsliced, whole) = render(&device, &queue, 32, 4, None, draw);
        assert_eq!(whole, [want.iter().sum::<u32>()], "{what}: passes in the frame");
        let (sliced, slices) = render(&device, &queue, 32, 4, Some(SubmissionSlicing::default()), draw);
        assert_eq!(slices, want, "{what}");
        assert_same_pixels(&sliced, &unsliced, what);
    }
}

/// A render pass begins with the first draw into its target, so a target
/// that is set and left again undrawn gets none: the screen when nothing is
/// drawn there after a filter, and the target every filter hands back
/// before the next one takes over.
#[test]
fn a_target_left_undrawn_gets_no_pass() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let passes = |draw: &dyn Fn(&mut C)| render(&device, &queue, 32, 32, None, draw).1;
    // The clear and the fill share the screen's one pass.
    assert_eq!(passes(&|c| fill(c, 2.0, 2.0, 8.0, 8.0, Color::rgb(255, 0, 0))), [1]);
    // The screen, the layer's content, and the screen again for its composite.
    assert_eq!(passes(&|c| one_layer_per_pixel(c, 1, 32)), [3]);
    // The screen, the image drawn into, the blur's two directions and the
    // color matrix.
    let filter_chain = |c: &mut C| {
        let image = |c: &mut C| {
            c.create_image_empty(8, 8, PixelFormat::Rgba8, ImageFlags::empty())
                .unwrap()
        };
        let (source, blurred, tinted) = (image(c), image(c), image(c));
        c.with_render_target(RenderTarget::Image(source), |c| {
            c.clear_rect(0, 0, 8, 8, Color::rgba(0, 0, 0, 0));
            fill(c, 2.0, 2.0, 4.0, 4.0, Color::rgb(0, 0, 255));
        });
        c.filter_image(blurred, ImageFilter::GaussianBlur { sigma: 2.0 }, source);
        let mut matrix = [0.0; 20];
        (matrix[0], matrix[6], matrix[12], matrix[18]) = (1.0, 1.0, 1.0, 1.0);
        c.filter_image(tinted, ImageFilter::ColorMatrix { matrix }, blurred);
    };
    assert_eq!(passes(&filter_chain), [5]);
}

/// Nested layers under a rotated scissor, an armed path clip, an image
/// target drawn and restored, a blur into a color matrix, and a masked
/// multiply layer over it.
fn mixed_scene(c: &mut C) {
    c.save();
    c.translate(48.0, 48.0);
    c.rotate(0.3);
    c.translate(-48.0, -48.0);
    c.scissor(12.0, 12.0, 72.0, 72.0);
    assert!(c.begin_layer(&LayerEffects::new().with_opacity(0.7)));
    fill(c, 0.0, 0.0, 96.0, 48.0, Color::rgb(220, 40, 40));
    assert!(c.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    fill(c, 24.0, 24.0, 48.0, 48.0, Color::rgb(40, 220, 40));
    c.end_layer();
    c.end_layer();
    c.restore();

    c.save();
    let mut clip = Path::new();
    clip.circle(48.0, 60.0, 30.0);
    c.clip_path(&clip, FillRule::NonZero);
    fill(c, 0.0, 40.0, 96.0, 56.0, Color::rgba(40, 40, 220, 160));
    c.restore();

    let source = c
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    c.with_render_target(RenderTarget::Image(source), |c| {
        c.clear_rect(0, 0, 32, 32, Color::rgba(0, 0, 0, 0));
        fill(c, 4.0, 4.0, 24.0, 24.0, Color::rgb(250, 200, 30));
        fill(c, 12.0, 12.0, 8.0, 8.0, Color::rgb(30, 30, 30));
    });
    let mut p = Path::new();
    p.rect(60.0, 4.0, 32.0, 32.0);
    c.fill_path(&p, &Paint::image(source, 60.0, 4.0, 32.0, 32.0, 0.0, 1.0));

    let blurred = c
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    c.filter_image(blurred, ImageFilter::GaussianBlur { sigma: 3.0 }, source);
    let tinted = c
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let sepia = [
        0.393, 0.769, 0.189, 0.0, 0.0, 0.349, 0.686, 0.168, 0.0, 0.0, 0.272, 0.534, 0.131, 0.0, 0.0, 0.0, 0.0, 0.0,
        1.0, 0.0,
    ];
    c.filter_image(tinted, ImageFilter::ColorMatrix { matrix: sepia }, blurred);

    let mask = c
        .create_image_empty(96, 96, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    c.with_render_target(RenderTarget::Image(mask), |c| {
        c.clear_rect(0, 0, 96, 96, Color::rgba(0, 0, 0, 0));
        fill(c, 0.0, 0.0, 96.0, 64.0, Color::rgba(255, 255, 255, 255));
        fill(c, 0.0, 64.0, 96.0, 32.0, Color::rgba(255, 255, 255, 96));
    });
    assert!(c.begin_layer(
        &LayerEffects::new()
            .with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 96.0, 96.0)
            .with_blend(BlendMode::Multiply)
    ));
    let mut p = Path::new();
    p.rect(0.0, 0.0, 96.0, 96.0);
    c.fill_path(&p, &Paint::image(tinted, 0.0, 0.0, 96.0, 96.0, 0.0, 1.0));
    c.end_layer();
}

#[test]
fn a_mixed_scene_renders_the_same_in_slices_of_four() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (unsliced, whole) = render(&device, &queue, 96, 96, None, mixed_scene);
    assert_eq!(whole.len(), 1);
    let (sliced, slices) = render(&device, &queue, 96, 96, slices_of(4), mixed_scene);
    assert!(slices.len() >= 3, "slices {slices:?}");
    assert!(slices.iter().all(|&passes| passes <= 4), "slices {slices:?}");
    assert_eq!(slices.iter().sum::<u32>(), whole[0]);
    assert_same_pixels(&sliced, &unsliced, "mixed scene");
    // The cut moves with the slice size; the picture must not.
    for passes in [1, 3, 7, 13] {
        let (sliced, _) = render(&device, &queue, 96, 96, slices_of(passes), mixed_scene);
        assert_same_pixels(&sliced, &unsliced, &format!("mixed scene in slices of {passes}"));
    }
}

/// Where the caller's own command buffer lands relative to the frame. The
/// canvas draws a red square, a half-red layer and a blue layer, five passes,
/// in slices of three: the square and the first layer are in the slice, the
/// blue layer in the returned tail.
enum Prepass {
    /// `queue.submit([prepass]); flush; queue.submit([tail])`.
    BeforeTheFlush,
    /// `flush; queue.submit([prepass, tail])`.
    WithTheTail,
}

fn frame_after_a_green_prepass(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    slicing: Option<SubmissionSlicing>,
    order: Prepass,
) -> Vec<u8> {
    const W: u32 = 24;
    const H: u32 = 8;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("prepass test target"),
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
    let view = target.create_view(&Default::default());
    let mut prepass = device.create_command_encoder(&Default::default());
    drop(prepass.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("the caller's clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }));
    let prepass = prepass.finish();

    let mut renderer = WGPURenderer::new(device.clone(), queue.clone());
    renderer.set_submission_slicing(slicing);
    let mut canvas = Canvas::new(renderer).expect("canvas");
    canvas.set_size(W, H, 1.0);
    fill(&mut canvas, 0.0, 0.0, 8.0, 8.0, Color::rgb(255, 0, 0));
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    fill(&mut canvas, 8.0, 0.0, 8.0, 8.0, Color::rgb(255, 0, 0));
    canvas.end_layer();
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(1.0)));
    fill(&mut canvas, 16.0, 0.0, 8.0, 8.0, Color::rgb(0, 0, 255));
    canvas.end_layer();

    let mut prepass = Some(prepass);
    if matches!(order, Prepass::BeforeTheFlush) {
        queue.submit(prepass.take());
    }
    let tail = canvas
        .flush_to_output(&target)
        .expect("a frame with draws produces a command buffer");
    match order {
        Prepass::BeforeTheFlush => queue.submit([tail]),
        Prepass::WithTheTail => queue.submit([prepass.take().unwrap(), tail]),
    };
    if slicing.is_some() {
        assert_eq!(canvas.renderer().last_frame_slices(), [3, 2]);
    }

    let padded = (W * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
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
    queue.submit([enc.finish()]);
    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        sender.send(result).expect("map result receiver dropped");
    });
    device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
    receiver.recv().expect("map_async callback never ran").expect("map");
    let mapped = slice.get_mapped_range().expect("readback");
    // The first row, tightly packed.
    let row = mapped[..(W * 4) as usize].to_vec();
    drop(mapped);
    row
}

fn px(row: &[u8], x: usize) -> [u8; 3] {
    [row[x * 4], row[x * 4 + 1], row[x * 4 + 2]]
}

const GREEN: [u8; 3] = [0, 255, 0];
const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];

fn half_red_over_green(p: [u8; 3]) -> bool {
    (i32::from(p[0]) - 128).abs() <= 1 && (i32::from(p[1]) - 128).abs() <= 1 && p[2] == 0
}

#[test]
fn without_slicing_the_frame_runs_after_a_prepass_submitted_with_it() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let row = frame_after_a_green_prepass(&device, &queue, None, Prepass::WithTheTail);
    assert_eq!(px(&row, 4), RED);
    assert!(half_red_over_green(px(&row, 12)), "{:?}", px(&row, 12));
    assert_eq!(px(&row, 20), BLUE);
}

#[test]
fn with_slicing_a_prepass_submitted_before_the_flush_runs_first() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let row = frame_after_a_green_prepass(&device, &queue, slices_of(3), Prepass::BeforeTheFlush);
    assert_eq!(px(&row, 4), RED);
    assert!(half_red_over_green(px(&row, 12)), "{:?}", px(&row, 12));
    assert_eq!(px(&row, 20), BLUE);
}

#[test]
fn with_slicing_a_prepass_submitted_with_the_tail_erases_the_slices_before_it() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    // The documented hazard: the slice ran during the flush, the caller's
    // clear ran after it, only the tail's draws survive.
    let row = frame_after_a_green_prepass(&device, &queue, slices_of(3), Prepass::WithTheTail);
    assert_eq!(px(&row, 4), GREEN);
    assert_eq!(px(&row, 12), GREEN);
    assert_eq!(px(&row, 20), BLUE);
}

/// The frame that was jetsammed on the iPhone 12: 176 layers, six of them
/// blurred, at 460x260.
fn portrait(c: &mut C) {
    let blurred = LayerEffects::new()
        .with_opacity(0.8)
        .with_filters(&[ImageFilter::GaussianBlur { sigma: 12.0 }]);
    let plain = LayerEffects::new().with_opacity(0.6);
    for i in 0..176 {
        let effects = if i % 30 == 15 { &blurred } else { &plain };
        assert!(c.begin_layer(effects));
        let [r, g, b] = cell(i);
        let x = (i * 53 % 400) as f32;
        let y = (i * 29 % 200) as f32;
        fill(c, x, y, 60.0, 60.0, Color::rgb(r, g, b));
        c.end_layer();
    }
}

#[cfg(target_os = "macos")]
fn phys_footprint() -> u64 {
    extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut u8) -> i32;
        fn getpid() -> i32;
    }
    // rusage_info_v2: a 16-byte uuid, then u64 fields; ri_phys_footprint is
    // the eighth.
    let mut buffer = [0u8; 256];
    let rc = unsafe { proc_pid_rusage(getpid(), 2, buffer.as_mut_ptr()) };
    assert_eq!(rc, 0, "proc_pid_rusage");
    u64::from_ne_bytes(buffer[72..80].try_into().unwrap())
}

/// The peak physical footprint of this process while `work` runs, sampled
/// from another thread. On macOS the footprint includes the GPU driver's
/// memory, which resident-size counters miss.
#[cfg(target_os = "macos")]
fn peak_footprint_during(work: impl FnOnce()) -> u64 {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    let stop = Arc::new(AtomicBool::new(false));
    let peak = Arc::new(AtomicU64::new(0));
    let sampler = {
        let (stop, peak) = (stop.clone(), peak.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                peak.fetch_max(phys_footprint(), Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_micros(500));
            }
        })
    };
    work();
    stop.store(true, Ordering::Relaxed);
    sampler.join().unwrap();
    peak.load(Ordering::Relaxed)
}

/// The footprint is the whole process's, so the frames are measured in a
/// child process running only this test; the parent asserts on its report.
/// Slices lower the peak by letting command buffers complete while the rest
/// of the frame is encoded; they do not bound it, since nothing waits.
#[cfg(target_os = "macos")]
#[test]
fn slices_hold_no_more_driver_memory_than_one_submission_on_metal() {
    const CHILD: &str = "FEMTOVG_FOOTPRINT_CHILD";
    let mib = |bytes: u64| bytes / (1024 * 1024);
    if std::env::var_os(CHILD).is_some() {
        let Some((device, queue)) = headless_device() else {
            println!("footprint skip");
            return;
        };
        // The driver keeps its pool for a second or two once grown, so both
        // peaks are measured against the footprint before any frame, and
        // the sliced frame runs first.
        let baseline = phys_footprint();
        let measure = |slicing: Option<SubmissionSlicing>, draw: fn(&mut C)| {
            let peak = peak_footprint_during(|| {
                let (_, slices) = render(&device, &queue, 460, 260, slicing, draw);
                device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
                assert!(slices
                    .iter()
                    .all(|&passes| passes <= slicing.map_or(u32::MAX, |s| s.passes)));
            });
            peak.saturating_sub(baseline)
        };
        println!(
            "footprint sliced {}",
            mib(measure(Some(SubmissionSlicing::default()), portrait))
        );
        println!("footprint whole {}", mib(measure(None, portrait)));
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "slices_hold_no_more_driver_memory_than_one_submission_on_metal",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .expect("run the measuring child");
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "the measuring child failed:\n{report}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if report.contains("footprint skip") {
        return;
    }
    let held = |kind: &str| -> u64 {
        let line = report
            .lines()
            .find_map(|line| line.strip_prefix(&format!("footprint {kind} ")))
            .unwrap_or_else(|| panic!("no {kind} line in the child's report:\n{report}"));
        line.trim().parse().expect("a footprint in MiB")
    };
    let (sliced, whole) = (held("sliced"), held("whole"));
    eprintln!(
        "176-layer portrait: the footprint peaked {sliced} MiB above the baseline in slices, {whole} MiB in one submission"
    );
    // On an M4 Max with wgpu 30: 1,050 MiB in slices, 1,499 MiB in one
    // submission; about 445 MiB of either is a pool the driver grows for
    // the first frame of any size. A wgpu that encodes the passes of an
    // encoder into one Metal command buffer (gfx-rs/wgpu#10506) leaves
    // both at that pool, 490 and 492 MiB, with nothing for slices to lower:
    // what must hold on either is that slicing never costs memory.
    assert!(
        sliced <= whole + whole / 20 + 16,
        "{sliced} MiB held in slices, {whole} MiB in one submission"
    );
}
