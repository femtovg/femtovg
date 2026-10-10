//! The WGPU backend keeps its render pipelines across flushes. A flush that
//! binds only some of them, such as a lone clear, must leave the rest cached,
//! or the next flush that draws builds them again. Past its capacity the cache
//! evicts the pipelines used longest ago, but never one used in the last few
//! flushes; that boundary is tested without a GPU in src/renderer/wgpu.rs.
#![cfg(feature = "wgpu")]

use femtovg::{
    renderer::WGPURenderer, BlendFactor, Canvas, Color, DrawCommand, FillRule, GlyphDrawCommands, ImageFilter,
    ImageFlags, LayerEffects, Paint, Path, PixelFormat, Quad,
};

mod common;
use common::headless_device;
use common::pipelines::{live_pipelines_after_flush, rect, target, SIZE};

/// Distinct pipelines the drawing flush binds, the clear pipeline included. Far
/// below the renderer's capacity, so nothing can be evicted.
const STATES: usize = 100;

/// Each round flushes `STATES - 1` fills in distinct blend states, then a
/// clear-only flush. Every pipeline stays alive, so an unchanged count after
/// the first round means no later flush built one. Before the fix the
/// clear-only flush left one pipeline alive.
#[test]
fn pipelines_stay_alive_across_flushes_that_do_not_use_them() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let target = target(&device);
    let factors = [
        BlendFactor::One,
        BlendFactor::OneMinusSrcAlpha,
        BlendFactor::Zero,
        BlendFactor::SrcColor,
        BlendFactor::OneMinusSrcColor,
        BlendFactor::DstColor,
        BlendFactor::OneMinusDstColor,
        BlendFactor::SrcAlpha,
        BlendFactor::DstAlpha,
        BlendFactor::OneMinusDstAlpha,
    ];
    // No anti-aliasing: each fill's blend state needs one pipeline, and the clear one more.
    let red = Paint::color(Color::rgb(255, 0, 0)).with_anti_alias(false);
    let mut canvas = Canvas::new(WGPURenderer::new(device.clone(), queue.clone())).expect("canvas");
    canvas.set_size(SIZE, SIZE, 1.0);
    for round in 0..2 {
        canvas.save();
        for i in 0..STATES - 1 {
            canvas.global_composite_blend_func_separate(
                factors[i % 10],
                factors[i / 10],
                BlendFactor::One,
                BlendFactor::OneMinusSrcAlpha,
            );
            canvas.fill_path(&rect(), &red);
        }
        canvas.restore();
        let drawn = live_pipelines_after_flush(&device, &queue, &mut canvas, &target);
        canvas.clear_rect(0, 0, SIZE, SIZE, Color::black());
        let cleared = live_pipelines_after_flush(&device, &queue, &mut canvas, &target);

        let expected_drawn = if round == 0 { STATES - 1 } else { STATES };
        assert_eq!(
            drawn, expected_drawn as isize,
            "round {round}: pipelines alive after the drawing flush"
        );
        assert_eq!(
            cleared, STATES as isize,
            "round {round}: pipelines alive after the clear-only flush"
        );
    }
}

/// Glyph, clipped-layer, filter, screen and clear-only flushes in turn. Their
/// pipelines stay far below the capacity, so nothing can be evicted, and every
/// flush after the first round leaving the count unchanged means none built a
/// pipeline.
#[test]
fn alternating_flushes_reuse_their_pipelines_after_the_first_round() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let target = target(&device);
    let mut canvas = Canvas::new(WGPURenderer::new(device.clone(), queue.clone())).expect("canvas");
    canvas.set_size(SIZE, SIZE, 1.0);
    let atlas = canvas
        .create_image_empty(8, 8, PixelFormat::Gray8, ImageFlags::empty())
        .expect("glyph atlas");
    let red = Paint::color(Color::rgb(255, 0, 0));
    let mut circle = Path::new();
    circle.circle(32.0, 32.0, 24.0);
    let mut concave = Path::new();
    concave.move_to(0.0, 0.0);
    concave.line_to(60.0, 10.0);
    concave.line_to(10.0, 60.0);
    concave.line_to(50.0, 50.0);
    concave.close();

    let mut rounds = Vec::new();
    for _ in 0..2 {
        let mut alive = Vec::new();

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
            &red,
        );
        alive.push(live_pipelines_after_flush(&device, &queue, &mut canvas, &target));

        canvas.save();
        canvas.clip_path(&circle, FillRule::NonZero);
        assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
        canvas.fill_path(&rect(), &red);
        canvas.end_layer();
        canvas.restore();
        alive.push(live_pipelines_after_flush(&device, &queue, &mut canvas, &target));

        assert!(
            canvas.begin_layer(&LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur {
                sigma_x: 3.0,
                sigma_y: 3.0
            }]))
        );
        canvas.fill_path(&rect(), &red);
        canvas.end_layer();
        alive.push(live_pipelines_after_flush(&device, &queue, &mut canvas, &target));

        canvas.fill_path(&rect(), &red);
        canvas.fill_path(
            &circle,
            &Paint::linear_gradient(0.0, 0.0, 64.0, 64.0, Color::black(), Color::white()),
        );
        canvas.stroke_path(&circle, &Paint::color(Color::rgb(0, 0, 255)).with_line_width(3.0));
        canvas.save();
        canvas.scissor(0.0, 0.0, 30.0, 30.0);
        canvas.fill_path(&concave, &red);
        canvas.restore();
        alive.push(live_pipelines_after_flush(&device, &queue, &mut canvas, &target));

        canvas.clear_rect(0, 0, SIZE, SIZE, Color::black());
        alive.push(live_pipelines_after_flush(&device, &queue, &mut canvas, &target));

        rounds.push(alive);
    }

    let settled = rounds[0][rounds[0].len() - 1];
    assert!(
        rounds[1].iter().all(|&alive| alive == settled),
        "{settled} pipelines were alive after the first round, but the second round's flushes left {:?}",
        rounds[1]
    );
}
