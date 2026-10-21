//! The WGPU backend makes a bind group for each set of resources a draw binds
//! - its image, its glyph atlas and its clip mask - and keeps it for the rest
//! of the flush. Under a soft clip mask a concave fill's winding pass binds no
//! mask and its cover binds one, so every fill changes the bind group twice:
//! made at each change, a flush of n such fills made 2n bind groups.
//!
//! Live bind groups are read from wgpu's internal counters, enabled by the
//! `counters` feature on the `wgpu` dev-dependency. A command buffer keeps
//! the bind groups its passes use alive until it is submitted, so the count
//! taken between the flush and the submission is the flush's.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, FillRule, Paint, Path};

mod common;
use common::headless_device;
use common::pipelines::{target, SIZE};

/// A seven-pointed star across the target: no rectangle, rounded rectangle
/// or ellipse, so the clip is taken as a coverage mask.
fn star() -> Path {
    let mut path = Path::new();
    let c = SIZE as f32 / 2.0;
    for i in 0..14 {
        let a = i as f32 * std::f32::consts::PI / 7.0;
        let r = if i % 2 == 0 { c - 2.0 } else { c / 2.0 };
        let (x, y) = (c + r * a.sin(), c - r * a.cos());
        if i == 0 {
            path.move_to(x, y);
        } else {
            path.line_to(x, y);
        }
    }
    path.close();
    path
}

/// An L, which is concave: a winding pass, then a cover.
fn l_shape(x: f32, y: f32) -> Path {
    let mut path = Path::new();
    path.move_to(x, y);
    path.line_to(x + 4.0, y);
    path.line_to(x + 4.0, y + 8.0);
    path.line_to(x + 12.0, y + 8.0);
    path.line_to(x + 12.0, y + 12.0);
    path.line_to(x, y + 12.0);
    path.close();
    path
}

/// How many bind groups a flush of `fills` Ls under the star makes, with
/// source-over: the star is a soft mask.
fn bind_groups_made(device: &wgpu::Device, queue: &wgpu::Queue, fills: usize) -> isize {
    let target = target(device);
    let mut canvas = Canvas::new(WGPURenderer::new(device.clone(), queue.clone())).expect("canvas");
    canvas.set_size(SIZE, SIZE, 1.0);
    canvas.clip_path(&star(), FillRule::NonZero);
    for i in 0..fills {
        let (x, y) = ((i % 4) as f32 * 14.0 + 4.0, (i / 4 % 4) as f32 * 14.0 + 4.0);
        canvas.fill_path(&l_shape(x, y), &Paint::color(Color::rgb((i * 40 % 256) as u8, 0, 255)));
    }
    let before = device.get_internal_counters().hal.bind_groups.read();
    let commands = canvas
        .flush_to_output(&target)
        .expect("flush_to_output produced no command buffer for a frame with draws");
    let made = device.get_internal_counters().hal.bind_groups.read() - before;
    queue.submit([commands]);
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll failed");
    made
}

/// Ten times the fills under a soft clip mask make no more bind groups.
#[test]
fn concave_fills_under_a_soft_clip_mask_share_their_bind_groups() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let few = bind_groups_made(&device, &queue, 4);
    let many = bind_groups_made(&device, &queue, 40);
    // Without the `counters` feature every counter reads zero, and the comparison would pass vacuously.
    assert!(few > 0, "no bind groups counted; is wgpu's `counters` feature on?");
    assert_eq!(many, few, "40 fills made {many} bind groups where 4 made {few}");
}
