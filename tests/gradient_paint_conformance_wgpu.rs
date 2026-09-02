//! Headless GPU conformance tests for gradient *painting* — the coverage the
//! existing gradient tests miss: gradient on strokes and on text, multi-stop
//! LINEAR and RADIAL fills (distinct from the conic LUT path), and — the
//! important one — that gradient coordinates live in absolute user space, not
//! the painted shape's bounding box.
//!
//! Scenarios mirror WPT `html/canvas/element/fill-and-stroke-styles/2d.gradient.*`
//! and pin regressions shipped by other engines: bbox-remapping (WPT
//! `linear.transform.1` / `interpolate.outside`), radial edge clamping
//! (Mozilla bug 687188), gradient-on-text collapsing to a solid/per-glyph
//! (WebKit bug 24687, Mozilla 424586), and gradient-on-stroke parity with fill
//! (ThorVG #191/#501). Values cross-checked against Chrome. Each test skips when
//! no GPU adapter is available.
#![cfg(all(feature = "wgpu", feature = "textlayout"))]

use femtovg::{renderer::WGPURenderer, Align, Baseline, Canvas, Color, Paint, Path, Transform2D};

const W: u32 = 300;
const H: u32 = 120;
const FONT: &[u8] = include_bytes!("../examples/assets/RobotoFlex-VariableFont.ttf");

mod common;
use common::headless_device;

fn render(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut Canvas<WGPURenderer>)) -> Vec<u8> {
    common::render_rgba(device, queue, W, H, Color::white(), draw)
}

fn px(pixels: &[u8], x: usize, y: usize) -> [i32; 3] {
    let i = (y * W as usize + x) * 4;
    [pixels[i] as i32, pixels[i + 1] as i32, pixels[i + 2] as i32]
}
fn near(a: [i32; 3], b: [i32; 3], tol: i32) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() <= tol)
}

/// Gradient coordinates are in absolute user space, NOT the shape's bounding box
/// (WPT `2d.gradient.interpolate.outside` / `linear.transform.1`). A gradient
/// defined only over x in [100, 200] must clamp to its end colours everywhere
/// else, and two shapes at different positions must show different slices — a
/// bbox-remapping renderer would paint the full gradient in every shape.
#[test]
fn gradient_is_positioned_in_absolute_user_space() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let red = [230, 40, 40];
    let blue = [40, 60, 230];
    let pixels = render(&device, &queue, |c| {
        // Gradient lives on x in [100, 200]; fill the whole canvas.
        let g = Paint::linear_gradient(100.0, 0.0, 200.0, 0.0, Color::rgb(230, 40, 40), Color::rgb(40, 60, 230));
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    let y = H as usize / 2;
    assert!(
        near(px(&pixels, 40, y), red, 4),
        "left of the gradient line must clamp to the start colour, got {:?}",
        px(&pixels, 40, y)
    );
    assert!(
        near(px(&pixels, 260, y), blue, 4),
        "right of the gradient line must clamp to the end colour, got {:?}",
        px(&pixels, 260, y)
    );
    // Middle of [100,200] is a blend, distinct from both ends.
    let mid = px(&pixels, 150, y);
    assert!(
        !near(mid, red, 30) && !near(mid, blue, 30),
        "middle of the gradient must be a blend, got {mid:?}"
    );
    // A bbox-remap bug would show the same full sweep in every column; instead
    // x=40 and x=260 are the two clamped ends (already asserted distinct).
    assert_ne!(px(&pixels, 40, y), px(&pixels, 260, y));
}

/// Multi-stop LINEAR fill (the `renderImageGradient` LUT path). Mirrors WPT
/// `2d.gradient.interpolate.multiple`: yellow -> cyan -> magenta at 0/0.5/1,
/// with the interpolated midpoints straight (non-premultiplied), matching Chrome.
#[test]
fn multi_stop_linear_interpolates_like_canvas() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        let g = Paint::linear_gradient_stops(
            0.0,
            0.0,
            200.0,
            0.0,
            [
                (0.0, Color::rgb(255, 255, 0)),
                (0.5, Color::rgb(0, 255, 255)),
                (1.0, Color::rgb(255, 0, 255)),
            ],
        );
        let mut p = Path::new();
        p.rect(0.0, 0.0, 200.0, H as f32);
        c.fill_path(&p, &g);
    });
    let y = H as usize / 2;
    // WPT expected (+/- a few for the dither): 127,255,127 | 0,255,255 | 127,127,255.
    assert!(
        near(px(&pixels, 50, y), [127, 255, 127], 4),
        "yellow->cyan mid, got {:?}",
        px(&pixels, 50, y)
    );
    assert!(
        near(px(&pixels, 100, y), [0, 255, 255], 4),
        "cyan stop, got {:?}",
        px(&pixels, 100, y)
    );
    assert!(
        near(px(&pixels, 150, y), [127, 127, 255], 4),
        "cyan->magenta mid, got {:?}",
        px(&pixels, 150, y)
    );
}

/// Radial fill clamps to the true stop colours outside its ring (Mozilla bug
/// 687188): a shape entirely inside the inner radius is uniformly the first
/// stop; a point outside the outer radius is the last stop.
#[test]
fn radial_gradient_clamps_to_stop_colors() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let (cx, cy) = (W as f32 / 2.0, H as f32 / 2.0);
    let pixels = render(&device, &queue, |c| {
        // Green at r=40, red at r=50: a tight ring far from the centre and edges.
        let g = Paint::radial_gradient(cx, cy, 40.0, 50.0, Color::rgb(0, 200, 0), Color::rgb(220, 0, 0));
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    // Centre (r=0, inside r=40) clamps to the first stop (green).
    assert!(
        near(px(&pixels, cx as usize, cy as usize), [0, 200, 0], 6),
        "inside the inner radius must be the first stop (green)"
    );
    // A corner (far outside r=50) clamps to the last stop (red).
    assert!(
        near(px(&pixels, 5, 5), [220, 0, 0], 6),
        "outside the outer radius must be the last stop (red), got {:?}",
        px(&pixels, 5, 5)
    );
}

/// A gradient painted on a STROKE samples the same user-space coordinates as the
/// same gradient painted as a FILL — it is not re-fit to the thin stroke outline
/// (ThorVG #191/#501). A horizontal gradient stroked across the canvas is red on
/// the left, blue on the right, just like a fill would be.
#[test]
fn gradient_stroke_uses_user_space_like_fill() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        let mut g = Paint::linear_gradient(
            0.0,
            0.0,
            W as f32,
            0.0,
            Color::rgb(230, 40, 40),
            Color::rgb(40, 60, 230),
        );
        g.set_line_width(24.0);
        // A thick horizontal line across the middle.
        let mut p = Path::new();
        p.move_to(0.0, H as f32 / 2.0);
        p.line_to(W as f32, H as f32 / 2.0);
        c.stroke_path(&p, &g);
    });
    let y = H as usize / 2;
    let left = px(&pixels, 20, y);
    let right = px(&pixels, W as usize - 20, y);
    println!("stroke left={left:?} right={right:?}");
    // Left end reddish, right end bluish — the gradient rides absolute x across
    // the stroke, not compressed to the ~24px stroke's bbox.
    assert!(left[0] > left[2] + 60, "stroke left must be reddish, got {left:?}");
    assert!(right[2] > right[0] + 60, "stroke right must be bluish, got {right:?}");
}

/// A gradient painted on TEXT spans the whole run continuously, not per-glyph and
/// not collapsed to a solid colour (WebKit bug 24687, Mozilla 424586). The ink
/// colour must move red -> blue monotonically across the word.
#[test]
fn gradient_text_is_continuous_across_the_run() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        let f = c.add_font_mem(FONT).unwrap();
        let paint = Paint::linear_gradient(
            20.0,
            0.0,
            W as f32 - 20.0,
            0.0,
            Color::rgb(230, 40, 40),
            Color::rgb(40, 60, 230),
        )
        .with_font(&[f])
        .with_font_size(70.0)
        .with_text_align(Align::Center)
        .with_text_baseline(Baseline::Middle);
        // Uniform glyphs so sampling is stable across the run.
        c.fill_text(W as f32 / 2.0, H as f32 / 2.0, "MMMM", &paint).unwrap();
    });
    // Find the inkiest pixel in a vertical band at several x positions.
    let cy = H as usize / 2;
    let ink_at = |x: usize| -> Option<[i32; 3]> {
        let mut best: Option<[i32; 3]> = None;
        for y in cy.saturating_sub(28)..(cy + 28).min(H as usize) {
            let c = px(&pixels, x, y);
            if c[0] + c[1] + c[2] < 600 && best.map_or(true, |b| c.iter().sum::<i32>() < b.iter().sum()) {
                best = Some(c);
            }
        }
        best
    };
    let samples: Vec<[i32; 3]> = [70usize, 120, 180, 230].iter().filter_map(|&x| ink_at(x)).collect();
    println!("text ink samples: {samples:?}");
    assert!(samples.len() >= 3, "expected ink at several positions across the word");
    // Red decreases and blue increases from left to right — continuous gradient.
    assert!(
        samples.first().unwrap()[0] > samples.last().unwrap()[0] + 40,
        "red must fall left->right: {samples:?}"
    );
    assert!(
        samples.last().unwrap()[2] > samples.first().unwrap()[2] + 40,
        "blue must rise left->right: {samples:?}"
    );
}

/// Two-point (independently centered) radial gradient — the general Canvas
/// `createRadialGradient(x0,y0,r0, x1,y1,r1)`. A small start circle nested inside a
/// large end circle is a focal gradient: the first stop sits at the focus and the
/// colour ramps out to the last stop at the rim. Anchor values cross-checked
/// against an independent parametric-search reference implementing the WHATWG
/// "draw circles from +inf to -inf" definition (which picks the largest offset
/// whose circle has a non-negative radius, matching Chrome's Skia).
#[test]
fn two_point_radial_focal_matches_reference() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        // Focus (110,60) r=10 nested inside end circle (150,60) r=90.
        let g = Paint::two_point_radial_gradient(
            110.0,
            60.0,
            10.0,
            150.0,
            60.0,
            90.0,
            Color::rgb(255, 0, 0),
            Color::rgb(0, 0, 255),
        );
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    let y = 60usize;
    // The focus is the first stop; the far side of the rim is the last stop.
    assert!(
        near(px(&pixels, 110, y), [255, 0, 0], 4),
        "focus must be red, got {:?}",
        px(&pixels, 110, y)
    );
    assert!(
        near(px(&pixels, 250, y), [0, 0, 255], 4),
        "past the rim must clamp to blue, got {:?}",
        px(&pixels, 250, y)
    );
    // Anchors from the independent reference (tol covers dither + edge AA).
    assert!(
        near(px(&pixels, 90, y), [191, 0, 64], 6),
        "x90 vs reference, got {:?}",
        px(&pixels, 90, y)
    );
    assert!(
        near(px(&pixels, 190, y), [106, 0, 149], 5),
        "x190 vs reference, got {:?}",
        px(&pixels, 190, y)
    );
    assert!(
        near(px(&pixels, 230, y), [21, 0, 234], 5),
        "x230 vs reference, got {:?}",
        px(&pixels, 230, y)
    );
    // The focus is the reddest point and blue rises away from it.
    assert!(
        px(&pixels, 110, y)[0] > px(&pixels, 190, y)[0] + 60,
        "colour must fall off from the focus"
    );
}

/// With coincident circle centres the two-point solver must reproduce the ordinary
/// concentric radial exactly. The concentric path uses a different shader (the
/// box-gradient SDF) that is already validated against Chrome, so agreement here
/// pins the two-point quadratic to that reference for the concentric case.
#[test]
fn two_point_radial_reduces_to_concentric() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let (cx, cy) = (150.0f32, 60.0f32);
    let two_point = render(&device, &queue, |c| {
        let g =
            Paint::two_point_radial_gradient(cx, cy, 20.0, cx, cy, 80.0, Color::rgb(0, 200, 0), Color::rgb(220, 0, 0));
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    let concentric = render(&device, &queue, |c| {
        let g = Paint::radial_gradient(cx, cy, 20.0, 80.0, Color::rgb(0, 200, 0), Color::rgb(220, 0, 0));
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    // Sample across the ring: inside, on the ramp, and outside.
    for (x, y) in [(150usize, 60usize), (150, 30), (185, 60), (110, 60), (150, 5), (5, 60)] {
        let (t, r) = (px(&two_point, x, y), px(&concentric, x, y));
        assert!(
            near(t, r, 3),
            "two-point vs concentric mismatch at ({x},{y}): {t:?} vs {r:?}"
        );
    }
}

/// Degenerate cone where the centre offset equals the radius delta (|Δc| == Δr),
/// i.e. the end centre sits exactly on the start circle. The quadratic's leading
/// coefficient is zero here, so the shader must fall back to the linear solution
/// rather than divide by zero. The covered band still ramps smoothly start->end.
#[test]
fn two_point_radial_degenerate_linear_branch() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        // |Δc| = 60, Δr = 80 - 20 = 60.
        let g = Paint::two_point_radial_gradient(
            100.0,
            60.0,
            20.0,
            160.0,
            60.0,
            80.0,
            Color::rgb(255, 0, 0),
            Color::rgb(0, 0, 255),
        );
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    let y = 60usize;
    // No NaN blow-out: values are finite and the band is not uniformly transparent.
    assert!(
        near(px(&pixels, 100, y), [255, 0, 0], 4),
        "start region must be red, got {:?}",
        px(&pixels, 100, y)
    );
    assert!(
        near(px(&pixels, 260, y), [0, 0, 255], 4),
        "end must be blue, got {:?}",
        px(&pixels, 260, y)
    );
    // Reference anchors from the independent parametric search.
    assert!(
        near(px(&pixels, 130, y), [233, 0, 22], 5),
        "x130 vs reference, got {:?}",
        px(&pixels, 130, y)
    );
    assert!(
        near(px(&pixels, 200, y), [84, 0, 171], 5),
        "x200 vs reference, got {:?}",
        px(&pixels, 200, y)
    );
    // Monotonic across the covered band.
    assert!(
        px(&pixels, 130, y)[0] > px(&pixels, 200, y)[0] + 60,
        "red must fall across the band"
    );
}

/// Where two interpolated circles pass through a fragment, Canvas draws them from
/// the largest offset down without overpainting, so the LARGEST offset wins. Two
/// equal-radius circles translated apart put every fragment on two circles, which
/// discriminates the selection rule: at the midpoint the winning offset is ~0.79
/// (blue-ward), not ~0.21 (red-ward). A smallest-offset implementation fails here.
#[test]
fn two_point_radial_selects_largest_offset() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        // Equal radii (40) with centres 140 apart: |Δc| > 0, Δr = 0.
        let g = Paint::two_point_radial_gradient(
            80.0,
            60.0,
            40.0,
            220.0,
            60.0,
            40.0,
            Color::rgb(255, 0, 0),
            Color::rgb(0, 0, 255),
        );
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    let y = 60usize;
    // The rule discriminator: the geometric midpoint is blue-ward (offset ~0.79),
    // which only holds if the largest covered offset wins.
    let mid = px(&pixels, 150, y);
    assert!(
        mid[2] > mid[0] + 100,
        "midpoint must be blue-ward (largest offset wins), got {mid:?}"
    );
    // Anchors from the independent reference.
    assert!(
        near(px(&pixels, 90, y), [164, 0, 91], 5),
        "x90 vs reference, got {:?}",
        px(&pixels, 90, y)
    );
    assert!(
        near(px(&pixels, 110, y), [127, 0, 128], 5),
        "x110 vs reference, got {:?}",
        px(&pixels, 110, y)
    );
    assert!(
        near(px(&pixels, 150, y), [55, 0, 200], 5),
        "x150 vs reference, got {:?}",
        px(&pixels, 150, y)
    );
}

/// A fragment reached by no interpolated circle with a non-negative radius is
/// transparent (leaves the background), not filled with a stop colour. Two small
/// disjoint circles leave the far side of the start circle uncovered.
#[test]
fn two_point_radial_leaves_uncovered_transparent() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        let g = Paint::two_point_radial_gradient(
            80.0,
            60.0,
            10.0,
            200.0,
            60.0,
            90.0,
            Color::rgb(255, 0, 0),
            Color::rgb(0, 0, 255),
        );
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    let y = 60usize;
    // Far left of the small start circle is reached by no valid circle -> background.
    assert!(
        near(px(&pixels, 20, y), [255, 255, 255], 3),
        "uncovered region must stay background, got {:?}",
        px(&pixels, 20, y)
    );
    // The interior near the end circle is painted (not background).
    assert!(
        !near(px(&pixels, 200, y), [255, 255, 255], 8),
        "covered region must be painted"
    );
}

/// Two circles that coincide exactly take the far end of the ramp across the
/// whole plane. The Canvas algorithm says to paint nothing for this geometry,
/// but the SVG working group resolved the opposite in 2019 — pad takes the last
/// stop, repeat and reflect take the average of the stops — and WPT's
/// `svg/pservers/reftests/radialgradient-fully-overlapping.svg` was changed to
/// assert the gradient is drawn. SVG 2's prose still says transparent black and
/// was never updated to match its own resolution. This follows SVG.
#[test]
fn two_point_radial_identical_circles_take_the_last_stop() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        let g = Paint::two_point_radial_gradient(
            150.0,
            60.0,
            40.0,
            150.0,
            60.0,
            40.0,
            Color::rgb(255, 0, 0),
            Color::rgb(0, 0, 255),
        );
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    for (x, y) in [(150usize, 60usize), (150, 30), (20, 20), (280, 100)] {
        assert!(
            near(px(&pixels, x, y), [0, 0, 255], 4),
            "({x}, {y}) should take the last stop, got {:?}",
            px(&pixels, x, y)
        );
    }
}

/// The point at the centre of a zero-radius start circle has to be painted: it
/// is reached only by the interpolated circle of radius exactly zero, so a
/// solver that requires a strictly positive radius leaves a hole there. From
/// Skia's `test_two_point_conical_zero_radius` (skia bug 5023), whose comment
/// reads "We should still shade pixels for which the radius is exactly 0."
#[test]
fn two_point_radial_zero_radius_centre_is_painted() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        let g = Paint::two_point_radial_gradient(
            150.0,
            60.0,
            0.0,
            152.0,
            62.0,
            80.0,
            Color::rgb(0, 200, 0),
            Color::rgb(0, 0, 255),
        );
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    let at_focus = px(&pixels, 150, 60);
    assert!(
        near(at_focus, [0, 200, 0], 6),
        "the focus must take the first stop, got {at_focus:?}"
    );
}

/// A focal point outside the end circle is drawn as the cone it describes, with
/// the region the cone does not reach left untouched. SVG 1.1 required moving
/// such a focus onto the circle; SVG 2 removed that to match Canvas, and every
/// engine followed: Gecko dropped its clamp in bug 1638051, WebKit in bug 97986,
/// resvg in #562. Geometry from resvg's `focal-point-correction` test.
#[test]
fn two_point_radial_focus_outside_the_end_circle_is_a_cone() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        let g = Paint::two_point_radial_gradient(
            233.0,
            105.0,
            0.0,
            160.0,
            40.0,
            75.0,
            Color::rgb(255, 0, 0),
            Color::rgb(0, 0, 255),
        );
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    // The end circle's centre is well inside the cone and takes a late offset.
    let inside = px(&pixels, 160, 40);
    assert!(
        inside[2] > inside[0] + 60,
        "the end circle centre should be blue-ward, got {inside:?}"
    );
    // Behind the focus, away from the cone, nothing is painted.
    let behind = px(&pixels, 292, 118);
    assert!(
        near(behind, [255, 255, 255], 3),
        "behind the focus must stay untouched, got {behind:?}"
    );
}

/// The same shape at two very different scales must be classified the same way.
/// The leading coefficient of the quadratic is a squared length, so it grows
/// with the square of the scale factor; testing it against a fixed constant
/// makes a focal point that sits on the end circle read as a degenerate at one
/// size and as a cone at another. This is the case tiny-skia tightened its own
/// threshold for after real SVG content regressed.
#[test]
fn two_point_radial_degenerate_test_is_scale_invariant() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    // Focus on the end circle: |dc| == dr, so the quadratic degenerates.
    let sample = |scale: f32| {
        let cx = 150.0;
        let cy = 60.0;
        let d = 30.0 * scale;
        render(&device, &queue, |c| {
            let g = Paint::two_point_radial_gradient(
                cx - d,
                cy,
                0.0,
                cx,
                cy,
                d,
                Color::rgb(255, 0, 0),
                Color::rgb(0, 0, 255),
            );
            let mut p = Path::new();
            p.rect(0.0, 0.0, W as f32, H as f32);
            c.fill_path(&p, &g);
        })
    };
    // At scale 1 the geometry fits the canvas; the ratio of the sample point to
    // the shape is what has to stay put, so read the end centre in both.
    let small = sample(1.0);
    let large = sample(1000.0);
    let a = px(&small, 150, 60);
    let b = px(&large, 150, 60);
    assert!(
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= 8),
        "scaling the shape must not change how it is classified: {a:?} vs {b:?}"
    );
}

/// When one circle strictly contains the other and the radius shrinks along the
/// sweep, the larger of the two roots always has a negative interpolated radius
/// and the smaller one is the answer. Skia hit this as a rendering bug and fixed
/// it by choosing the root by sign rather than always taking the larger; its
/// `Make2ConicalInsideFlip` case has the smaller root winning at every pixel.
#[test]
fn two_point_radial_shrinking_contained_circle_uses_the_other_root() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        // Start circle contains the end circle, and the radius decreases.
        let g = Paint::two_point_radial_gradient(
            150.0,
            60.0,
            70.0,
            170.0,
            45.0,
            20.0,
            Color::rgb(230, 40, 40),
            Color::rgb(40, 60, 230),
        );
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    // The end circle's centre is the far end of the sweep.
    let end = px(&pixels, 170, 45);
    assert!(
        end[2] > end[0] + 60,
        "end circle centre should be blue-ward, got {end:?}"
    );
    // Well outside the start circle the sweep has run out and clamps to the first stop.
    let outside = px(&pixels, 10, 110);
    assert!(
        outside[0] > outside[2] + 60,
        "outside the start circle should clamp to the first stop, got {outside:?}"
    );
    // Nothing may be left unpainted: one circle contains the other, so the
    // gradient covers the whole plane.
    for (x, y) in [(2usize, 2usize), (297, 2), (2, 117), (297, 117), (150, 60)] {
        assert!(
            !near(px(&pixels, x, y), [255, 255, 255], 6),
            "({x}, {y}) should be covered, got {:?}",
            px(&pixels, x, y)
        );
    }
}

/// A gradient transform scales the gradient's own axes without moving the shape,
/// which is how an elliptical radial gradient is expressed. The circle and
/// radius parameters describe a circle by construction, so without this the
/// shape is unreachable at any call site.
///
/// This is the form every design tool emits: a unit circle at the origin plus a
/// `gradientTransform` that translates and scales it. The Reddit mark carries
/// eight of them and the WebKit mark six.
#[test]
fn gradient_transform_makes_a_radial_gradient_elliptical() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    // Unit circle at the origin, stretched three times as wide as it is tall.
    // Scale the axes by 90 and 30, then move the centre to (150, 60).
    let t = Transform2D([90.0, 0.0, 0.0, 30.0, 150.0, 60.0]);

    let pixels = render(&device, &queue, |c| {
        let g = Paint::two_point_radial_gradient(
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            Color::rgb(230, 40, 40),
            Color::rgb(40, 60, 230),
        )
        .with_gradient_transform(t);
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });

    // The centre is the first stop on both axes.
    assert!(
        near(px(&pixels, 150, 60), [230, 40, 40], 6),
        "centre should be the first stop, got {:?}",
        px(&pixels, 150, 60)
    );
    // Three quarters of the way out along each axis the ramp has reached the
    // same place, which is what makes the gradient elliptical rather than round:
    // 67px horizontally is the same offset as 22px vertically.
    let along_x = px(&pixels, 150 + 67, 60);
    let along_y = px(&pixels, 150, 60 + 22);
    assert!(
        along_x.iter().zip(along_y.iter()).all(|(a, b)| (a - b).abs() <= 12),
        "the ramp should reach the same colour at proportional distances on each axis: {along_x:?} vs {along_y:?}"
    );
    // A circular gradient would still be near the first stop at 67px across;
    // the stretched one is well past halfway.
    assert!(
        along_x[2] > along_x[0],
        "67px out along the wide axis should be past the midpoint, got {along_x:?}"
    );
}

/// Design-tool SVGs (Sketch, Illustrator) author linear gradients with
/// endpoints a fraction of a unit apart under a gradientTransform whose
/// translation is 1e5..1e6. That must render exactly like the equivalent
/// user-space gradient: the transform folds into the endpoints, so the
/// nanovg `large` offset never rides through a huge translation where f32
/// would quantize `t` into visible stairs. Regression for Mozilla's
/// splash-logo.svg (Firefox logo, `gradientTransform="matrix(480 0 0 -496
/// 428684 1098420)"`).
#[test]
fn huge_gradient_transform_translation_stays_smooth() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let stops = || {
        vec![
            (0.0, Color::rgb(255, 244, 79)),
            (0.5, Color::rgb(255, 54, 71)),
            (1.0, Color::rgb(227, 21, 135)),
        ]
    };
    // The transform maps the tiny authored span onto a 300-unit diagonal
    // across the canvas: user-space start (10, 10) -> end (290, 110).
    let (a, d, e, f) = (480.0f32, -496.0f32, 428_684.0f32, 1_098_420.0f32);
    let (sx, sy) = ((10.0 - e) / a, (10.0 - f) / d);
    let (ex, ey) = ((290.0 - e) / a, (110.0 - f) / d);
    let transformed = render(&device, &queue, |canvas| {
        let paint = Paint::linear_gradient_stops(sx, sy, ex, ey, stops())
            .with_gradient_transform(Transform2D([a, 0.0, 0.0, d, e, f]));
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&p, &paint);
    });
    let folded = render(&device, &queue, |canvas| {
        let paint = Paint::linear_gradient_stops(10.0, 10.0, 290.0, 110.0, stops());
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&p, &paint);
    });
    // Every pixel agrees to within f32 rounding of the folded endpoints
    // (dither is seeded by position, so it cancels); before the fold the
    // transformed render quantized into stairs dozens of levels off.
    let mut worst = 0;
    for y in (2..H as usize - 2).step_by(3) {
        for x in (2..W as usize - 2).step_by(3) {
            let t = px(&transformed, x, y);
            let g = px(&folded, x, y);
            worst = worst.max((0..3).map(|i| (t[i] - g[i]).abs()).max().unwrap());
        }
    }
    assert!(
        worst <= 4,
        "transformed and folded gradients must agree; worst channel delta {worst}"
    );
}

/// An anisotropic gradientTransform is still a linear gradient in user
/// space, but its direction is the image of the iso-line normal (A^-T v),
/// not the image of the endpoint vector. Pin the exact mapping: under
/// scale(2, 1) a gradient along (1, 1) has user-space t = (x/2 + y) / 2, so
/// the iso-line t = 0.5 passes through (2, 0) and (0, 1) — points that the
/// naive "map both endpoints" answer would color differently.
#[test]
fn anisotropic_gradient_transform_keeps_exact_iso_lines() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    // Gradient space: start (0, 0), end (1, 1); transform scale(2, 1) then
    // place it at (60, 20) with a 100x magnification for readable pixels.
    let s = 100.0f32;
    let out = render(&device, &queue, |canvas| {
        let paint = Paint::linear_gradient(0.0, 0.0, 1.0, 1.0, Color::rgb(0, 0, 0), Color::rgb(255, 255, 255))
            .with_gradient_transform(Transform2D([2.0 * s, 0.0, 0.0, s, 60.0, 10.0]));
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&p, &paint);
    });
    // t(x, y) = ((x - 60) / (2 s) + (y - 10) / s) / 2.
    let t_at = |x: f32, y: f32| ((x - 60.0) / (2.0 * s) + (y - 10.0) / s) / 2.0;
    for (x, y) in [(160.0f32, 10.0f32), (60.0, 60.0), (110.0, 35.0), (210.0, 60.0)] {
        let expect = (t_at(x, y).clamp(0.0, 1.0) * 255.0).round() as i32;
        let got = px(&out, x as usize, y as usize)[1];
        assert!(
            (got - expect).abs() <= 3,
            "at ({x}, {y}) expected gray {expect}, got {got}"
        );
    }
}
