//! Headless GPU tests for how `save()`/`restore()` and
//! `begin_layer()`/`end_layer()` share one state stack (femtovg#335). A layer
//! is a save entry: `restore()` at a layer's boundary closes the layer, and
//! `end_layer()` restores to its layer's boundary, discarding saves left
//! open inside it. The observable is a marker rect drawn at the origin after
//! the sequence: the base transform is `translate(16, 0)`, so the marker
//! lands at x 16..32 whenever the base state survives.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, FillRule, LayerEffects, Paint, Path, RenderTarget};

mod common;
use common::{headless_device, render_rgba};

const W: u32 = 64;
const H: u32 = 64;

type C = Canvas<WGPURenderer>;

fn render(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut C)) -> Vec<u8> {
    render_rgba(device, queue, W, H, Color::white(), draw)
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2]]
}

fn fill(c: &mut C, x: f32, y: f32, w: f32, h: f32, color: Color) {
    let mut p = Path::new();
    p.rect(x, y, w, h);
    let mut paint = Paint::color(color);
    paint.set_anti_alias(false);
    c.fill_path(&p, &paint);
}

fn red() -> Color {
    Color::rgb(255, 0, 0)
}

fn green() -> Color {
    Color::rgb(0, 255, 0)
}

/// The x range of the green marker on row 8, as `start..end`.
fn marker(out: &[u8]) -> (u32, u32) {
    let xs: Vec<u32> = (0..W).filter(|&x| px(out, x, 8) == [0, 255, 0]).collect();
    match (xs.first(), xs.last()) {
        (Some(a), Some(b)) => (*a, *b + 1),
        _ => (0, 0),
    }
}

const HALF_RED_OVER_WHITE: [u8; 3] = [255, 128, 128];
const WHITE: [u8; 3] = [255, 255, 255];

fn half() -> LayerEffects {
    LayerEffects::new().with_opacity(0.5)
}

/// The layer's own content: a red rect at (-16, 32) under the base
/// translate, so device x 0..32, y 32..64, composited at opacity 0.5.
fn layer_content(c: &mut C) {
    fill(c, -16.0, 32.0, 32.0, 32.0, red());
}

fn marker_rect(c: &mut C) {
    fill(c, 0.0, 0.0, 16.0, 16.0, green());
}

/// `save(); begin_layer(); …; restore(); …; end_layer(); restore();`: the
/// inner restore closes the layer (it is the entry on top of the stack),
/// the end_layer finds no layer of its own and does nothing, and the
/// balancing restore pops the save, so the base transform survives.
#[test]
fn restore_inside_a_layer_closes_the_layer_and_keeps_the_outer_save() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render(&device, &queue, |c| {
        c.translate(16.0, 0.0);
        c.save();
        assert!(c.begin_layer(&half()));
        layer_content(c);
        c.restore();
        c.end_layer();
        c.restore();
        marker_rect(c);
    });
    assert_eq!(px(&out, 8, 48), HALF_RED_OVER_WHITE, "layer content composited at 0.5");
    assert_eq!(marker(&out), (16, 32), "base transform kept");
}

/// `begin_layer(); save(); …; end_layer(); restore();`: end_layer restores
/// to its layer's boundary, discarding the save left open inside it, and
/// the trailing restore is unmatched at the base level and does nothing.
#[test]
fn end_layer_discards_saves_left_open_inside_the_layer() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render(&device, &queue, |c| {
        c.translate(16.0, 0.0);
        assert!(c.begin_layer(&half()));
        c.save();
        c.translate(100.0, 100.0); // must not leak past end_layer
        layer_content(c);
        c.end_layer();
        c.restore();
        marker_rect(c);
    });
    assert_eq!(
        px(&out, 8 + 100 - 100, 48),
        WHITE,
        "inner translate applied inside the layer"
    );
    assert_eq!(marker(&out), (16, 32), "base transform kept");
}

/// `begin_layer(); …; restore(); end_layer();`: the stray restore closes the
/// layer, the end_layer has nothing to close.
#[test]
fn a_stray_restore_closes_the_layer_and_end_layer_is_then_a_no_op() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render(&device, &queue, |c| {
        c.translate(16.0, 0.0);
        assert!(c.begin_layer(&half()));
        layer_content(c);
        c.restore();
        c.end_layer();
        marker_rect(c);
    });
    assert_eq!(px(&out, 8, 48), HALF_RED_OVER_WHITE, "layer content composited at 0.5");
    assert_eq!(marker(&out), (16, 32), "base transform kept");
}

/// A clip taken before the save survives the misnested sequence: the marker
/// is clipped to the clip's device range (16..48) and the layer composite
/// is clipped by it too.
#[test]
fn a_clip_before_the_save_survives_a_restore_inside_the_layer() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render(&device, &queue, |c| {
        c.translate(16.0, 0.0);
        let mut clip = Path::new();
        clip.rect(0.0, 0.0, 32.0, 64.0);
        c.clip_path(&clip, FillRule::NonZero);
        c.save();
        assert!(c.begin_layer(&half()));
        layer_content(c);
        c.restore();
        c.end_layer();
        c.restore();
        fill(c, 0.0, 0.0, 64.0, 16.0, green());
    });
    assert_eq!(px(&out, 8, 48), WHITE, "layer composite clipped left of x 16");
    assert_eq!(px(&out, 24, 48), HALF_RED_OVER_WHITE, "layer composite inside the clip");
    assert_eq!(
        marker(&out),
        (16, 48),
        "marker clipped to the clip taken before the save"
    );
}

/// A clip taken inside the layer goes with the layer when a stray restore
/// closes it: the layer's content is clipped, later drawing is not.
#[test]
fn a_clip_inside_the_layer_ends_with_the_layer() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render(&device, &queue, |c| {
        c.translate(16.0, 0.0);
        assert!(c.begin_layer(&half()));
        let mut clip = Path::new();
        clip.rect(-16.0, 0.0, 32.0, 64.0); // device x 0..32
        c.clip_path(&clip, FillRule::NonZero);
        fill(c, -16.0, 32.0, 48.0, 32.0, red()); // device x 0..48, clipped to 0..32
        c.restore();
        c.end_layer();
        fill(c, 0.0, 0.0, 64.0, 16.0, green());
    });
    assert_eq!(px(&out, 8, 48), HALF_RED_OVER_WHITE, "clipped layer content");
    assert_eq!(px(&out, 40, 48), WHITE, "the layer's clip cut its content at x 32");
    assert_eq!(marker(&out), (16, 64), "later drawing is not clipped");
}

/// Nested layers: a stray restore inside the inner layer closes only the
/// inner one; drawing continues in the outer layer, which end_layer then
/// closes.
#[test]
fn a_stray_restore_in_a_nested_layer_closes_only_the_inner_layer() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render(&device, &queue, |c| {
        c.translate(16.0, 0.0);
        assert!(c.begin_layer(&half()));
        assert!(c.begin_layer(&half()));
        layer_content(c);
        c.restore(); // closes the inner layer into the outer one
        fill(c, 16.0, 32.0, 16.0, 32.0, Color::rgb(0, 0, 255)); // device x 32..48, in the outer layer
        c.end_layer();
        marker_rect(c);
    });
    assert_eq!(
        px(&out, 8, 48),
        [255, 191, 191],
        "inner content at 0.5 x 0.5 over white"
    );
    assert_eq!(px(&out, 40, 48), [128, 128, 255], "outer content at 0.5");
    assert_eq!(marker(&out), (16, 32), "base transform kept");
}

/// A frame boundary of the same size keeps the layer open (WPT
/// 2d.layer.flush-on-frame-presentation); a stray restore after it still
/// closes the layer properly.
#[test]
fn a_stray_restore_after_a_same_size_frame_boundary_closes_the_layer() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render(&device, &queue, |c| {
        c.translate(16.0, 0.0);
        assert!(c.begin_layer(&half()));
        layer_content(c);
        c.set_size(W, H, 1.0);
        c.restore();
        marker_rect(c);
    });
    assert_eq!(px(&out, 8, 48), HALF_RED_OVER_WHITE, "layer content composited at 0.5");
    assert_eq!(marker(&out), (16, 32), "base transform kept");
}

/// A render-target switch inside the layer, then a stray restore: the layer
/// closes onto the target it was opened on, and drawing continues there.
#[test]
fn a_stray_restore_after_a_target_switch_closes_the_layer_onto_its_own_target() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render(&device, &queue, |c| {
        let image = c
            .create_image_empty(16, 16, femtovg::PixelFormat::Rgba8, femtovg::ImageFlags::PREMULTIPLIED)
            .unwrap();
        c.translate(16.0, 0.0);
        assert!(c.begin_layer(&half()));
        layer_content(c);
        c.set_render_target(RenderTarget::Image(image));
        c.restore();
        marker_rect(c);
    });
    assert_eq!(px(&out, 8, 48), HALF_RED_OVER_WHITE, "layer composited onto the screen");
    assert_eq!(
        marker(&out),
        (16, 32),
        "drawing continues on the screen with the base transform"
    );
}

/// `reset()` inside a layer discards every open layer and the state (WPT
/// 2d.layer.reset): the marker lands at the identity origin and the layer's
/// content is gone.
#[test]
fn reset_inside_a_layer_discards_it() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render(&device, &queue, |c| {
        c.translate(16.0, 0.0);
        assert!(c.begin_layer(&half()));
        layer_content(c);
        c.reset();
        c.end_layer();
        marker_rect(c);
    });
    assert_eq!(px(&out, 8, 48), WHITE, "discarded layer content");
    assert_eq!(marker(&out), (0, 16), "reset state: identity transform");
}
