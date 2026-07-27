//! Conformance tests for rounded rectangle corner radii.
//!
//! When the radii along a side are larger than that side can hold, both CSS
//! Backgrounds and Borders Level 3 ("Overlapping curves") and the Canvas
//! `roundRect()` algorithm reduce every radius by one common factor
//! `f = min(side length / sum of that side's two radii)`. Scaling them together
//! is what keeps a corner circular. Reducing each axis on its own instead
//! (`rx = min(r, w/2)`, `ry = min(r, h/2)`) fits the box but flattens the corner
//! into an ellipse, which is the shape of this bug in several engines.
//!
//! Expected geometry here was cross-checked against Chrome's `roundRect()`.

use femtovg::{Path, Verb};

/// The two radii of one corner, recovered from the emitted path.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Corner {
    rx: f32,
    ry: f32,
}

#[derive(Debug, Clone, Copy)]
struct Corners {
    top_left: Corner,
    top_right: Corner,
    bottom_right: Corner,
    bottom_left: Corner,
}

/// Reads the corner radii back out of a rounded rectangle sub-path.
///
/// The shape is emitted as `MoveTo`, then four `LineTo`/`BezierTo` pairs walking
/// the sides counter-clockwise from the left edge, so each radius is the distance
/// from a corner of the bounding box to the point where the straight edge ends.
/// Only the on-curve endpoints are read, never the Bézier control points, so the
/// values are exact rather than approximations of the arc.
fn corners_of(path: &Path, x: f32, y: f32, w: f32, h: f32) -> Corners {
    let mut points = Vec::new();
    for verb in path.verbs() {
        match verb {
            Verb::MoveTo(px, py) => points.push((px, py)),
            Verb::LineTo(px, py) => points.push((px, py)),
            Verb::BezierTo(_, _, _, _, px, py) => points.push((px, py)),
            _ => {}
        }
    }
    assert_eq!(points.len(), 9, "expected a move, four lines and four curves");

    Corners {
        // MoveTo starts on the left edge at the end of the top left corner.
        top_left: Corner {
            rx: points[7].0 - x,
            ry: points[0].1 - y,
        },
        bottom_left: Corner {
            rx: points[2].0 - x,
            ry: (y + h) - points[1].1,
        },
        bottom_right: Corner {
            rx: (x + w) - points[3].0,
            ry: (y + h) - points[4].1,
        },
        top_right: Corner {
            rx: (x + w) - points[6].0,
            ry: points[5].1 - y,
        },
    }
}

#[track_caller]
fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() < 0.01,
        "{what}: expected {expected}, got {actual}"
    );
}

#[track_caller]
fn assert_circular(corner: Corner, radius: f32, what: &str) {
    assert_close(corner.rx, radius, &format!("{what} rx"));
    assert_close(corner.ry, radius, &format!("{what} ry"));
}

/// A single radius larger than half the height must stay circular, giving a
/// stadium. This is the shape reported against Slint (slint-ui/slint#6506): a
/// 300x75 box with `border-radius: self.height`. Clamping the axes separately
/// gives `rx = 75`, `ry = 37.5`, an ellipse, and a visibly squashed end.
#[test]
fn a_radius_larger_than_the_box_stays_circular() {
    let mut path = Path::new();
    path.rounded_rect(0.0, 0.0, 300.0, 75.0, 75.0);
    let c = corners_of(&path, 0.0, 0.0, 300.0, 75.0);

    // f = min(300/150, 300/150, 75/150, 75/150) = 0.5, so every radius halves.
    for (corner, name) in [
        (c.top_left, "top left"),
        (c.top_right, "top right"),
        (c.bottom_right, "bottom right"),
        (c.bottom_left, "bottom left"),
    ] {
        assert_circular(corner, 37.5, name);
    }
}

/// The same rule stated as the property that actually matters: for a single
/// scalar radius the two axes of every corner stay equal, whatever the box.
#[test]
fn a_scalar_radius_never_produces_an_ellipse() {
    for (w, h, r) in [
        (300.0, 75.0, 75.0),
        (75.0, 300.0, 75.0),
        (100.0, 10.0, 400.0),
        (10.0, 100.0, 400.0),
        (50.0, 50.0, 25.0),
        (120.0, 40.0, 20.0),
    ] {
        let mut path = Path::new();
        path.rounded_rect(0.0, 0.0, w, h, r);
        let c = corners_of(&path, 0.0, 0.0, w, h);
        for (corner, name) in [
            (c.top_left, "top left"),
            (c.top_right, "top right"),
            (c.bottom_right, "bottom right"),
            (c.bottom_left, "bottom left"),
        ] {
            assert!(
                (corner.rx - corner.ry).abs() < 0.01,
                "{w}x{h} r={r}: {name} corner is elliptical, rx={} ry={}",
                corner.rx,
                corner.ry
            );
            // It must also still fit inside the box.
            assert!(corner.rx <= w / 2.0 + 0.01 || r > w, "{name} rx overflows");
        }
    }
}

/// Radii that already fit are left exactly as given.
#[test]
fn radii_that_fit_are_not_reduced() {
    let mut path = Path::new();
    path.rounded_rect(0.0, 0.0, 100.0, 100.0, 20.0);
    let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);
    assert_circular(c.top_left, 20.0, "top left");
    assert_circular(c.bottom_right, 20.0, "bottom right");

    // Exactly filling the side is still a fit: 50 + 50 == 100.
    let mut path = Path::new();
    path.rounded_rect(0.0, 0.0, 100.0, 100.0, 50.0);
    let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);
    assert_circular(c.top_left, 50.0, "top left at the limit");
    assert_circular(c.bottom_right, 50.0, "bottom right at the limit");
}

/// A corner may use the whole side when the corner next to it uses none of it.
/// The sum along each side is what is bounded, not the individual radius, so a
/// 100x100 box takes a top left radius of 100. An implementation that clamps to
/// half the dimension wrongly cuts this to 50.
#[test]
fn one_large_corner_beside_a_square_one_keeps_its_radius() {
    let mut path = Path::new();
    path.rounded_rect_varying(0.0, 0.0, 100.0, 100.0, 100.0, 0.0, 0.0, 0.0);
    let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);

    assert_circular(c.top_left, 100.0, "top left");
    assert_circular(c.top_right, 0.0, "top right");
    assert_circular(c.bottom_right, 0.0, "bottom right");
    assert_circular(c.bottom_left, 0.0, "bottom left");
}

/// Only one side overflowing still scales every corner, so their proportions to
/// one another are preserved. Cross-checked against Chrome.
#[test]
fn overflow_on_one_side_scales_every_corner() {
    let mut path = Path::new();
    // Top side needs 80 + 80 = 160 out of 100, so f = 0.625.
    path.rounded_rect_varying(0.0, 0.0, 100.0, 100.0, 80.0, 80.0, 20.0, 20.0);
    let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);

    assert_circular(c.top_left, 50.0, "top left");
    assert_circular(c.top_right, 50.0, "top right");
    assert_circular(c.bottom_right, 12.5, "bottom right");
    assert_circular(c.bottom_left, 12.5, "bottom left");
}

/// The factor is taken from the worst side, not the first one that overflows.
#[test]
fn the_scale_comes_from_the_most_overflowing_side() {
    let mut path = Path::new();
    // Left side needs 60 + 60 = 120 out of 40 (f = 1/3); the top needs
    // 60 + 10 = 70 out of 200 and does not overflow.
    path.rounded_rect_varying(0.0, 0.0, 200.0, 40.0, 60.0, 10.0, 10.0, 60.0);
    let c = corners_of(&path, 0.0, 0.0, 200.0, 40.0);

    assert_circular(c.top_left, 20.0, "top left");
    assert_circular(c.bottom_left, 20.0, "bottom left");
    assert_circular(c.top_right, 10.0 / 3.0, "top right");
    assert_circular(c.bottom_right, 10.0 / 3.0, "bottom right");
}

/// A radius that is negative or not finite does not describe a corner. It must
/// leave that corner square instead of reaching the geometry, where a negative
/// value bulges the corner outwards and a NaN spreads through every coordinate
/// and silently drops the shape in the tessellator.
#[test]
fn unusable_radii_leave_the_corner_square() {
    for bad in [-10.0f32, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut path = Path::new();
        path.rounded_rect_varying(0.0, 0.0, 100.0, 100.0, bad, 20.0, 20.0, 20.0);
        let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);

        assert_circular(c.top_left, 0.0, &format!("top left for {bad}"));
        assert_circular(c.top_right, 20.0, &format!("top right for {bad}"));

        for verb in path.verbs() {
            if let Verb::BezierTo(a, b, cc, d, e, f) = verb {
                for v in [a, b, cc, d, e, f] {
                    assert!(v.is_finite(), "{bad} produced a non-finite coordinate");
                }
            }
        }
    }
}

/// Rectangles given with a negative width or height are drawn from the opposite
/// corner; the radii follow the sides rather than escaping the box.
#[test]
fn reversed_rectangles_keep_their_radii_inside() {
    let mut path = Path::new();
    path.rounded_rect(100.0, 100.0, -300.0, -75.0, 75.0);
    let c = corners_of(&path, 100.0, 100.0, -300.0, -75.0);

    // The magnitudes match the positive case; the signs follow the direction.
    for (corner, name) in [
        (c.top_left, "top left"),
        (c.top_right, "top right"),
        (c.bottom_right, "bottom right"),
        (c.bottom_left, "bottom left"),
    ] {
        assert_close(corner.rx.abs(), 37.5, &format!("{name} rx magnitude"));
        assert_close(corner.ry.abs(), 37.5, &format!("{name} ry magnitude"));
    }
}

/// Vectors ported from WPT `css/css-backgrounds/border-radius-sum-of-radii-001.htm`,
/// whose border box is 100x100. Each case there overlays the reduced shape on a
/// reference drawn with the already-reduced radii and passes when nothing shows
/// through, so the reference values are the used radii the rule must produce.
///
/// Case 4 is the one worth reading twice: the bottom and right sides both fit on
/// their own, yet their radii still shrink from 30 to 25, because the factor
/// comes from the overflowing top side and applies to every corner. An
/// implementation that reduces only what overflows leaves them at 30.
#[test]
fn wpt_css_border_radius_sum_of_radii() {
    // [top left, top right, bottom right, bottom left], as the CSS shorthand orders them.
    let cases: [([f32; 4], [f32; 4]); 8] = [
        ([60.0, 150.0, 30.0, 30.0], [28.5714, 71.4286, 14.2857, 14.2857]),
        ([0.0, 150.0, 0.0, 30.0], [0.0, 100.0, 0.0, 20.0]),
        ([150.0, 0.0, 30.0, 0.0], [100.0, 0.0, 20.0, 0.0]),
        ([50.0, 70.0, 30.0, 30.0], [41.6667, 58.3333, 25.0, 25.0]),
        ([150.0, 150.0, 30.0, 30.0], [50.0, 50.0, 10.0, 10.0]),
        ([30.0, 50.0, 70.0, 30.0], [25.0, 41.6667, 58.3333, 25.0]),
        ([30.0, 30.0, 50.0, 70.0], [25.0, 25.0, 41.6667, 58.3333]),
        ([70.0, 30.0, 30.0, 50.0], [58.3333, 25.0, 25.0, 41.6667]),
    ];

    for (input, expected) in cases {
        let mut path = Path::new();
        path.rounded_rect_varying(0.0, 0.0, 100.0, 100.0, input[0], input[1], input[2], input[3]);
        let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);
        let label = format!("radii {input:?}");
        assert_circular(c.top_left, expected[0], &format!("{label} top left"));
        assert_circular(c.top_right, expected[1], &format!("{label} top right"));
        assert_circular(c.bottom_right, expected[2], &format!("{label} bottom right"));
        assert_circular(c.bottom_left, expected[3], &format!("{label} bottom left"));
    }
}

/// From WPT `css/css-backgrounds/border-radius-sum-of-radii-002.htm`: radii that
/// exactly fill a side give a factor of one, and the rule only applies below one,
/// so nothing is reduced. Reducing here would shrink shapes that are already
/// correct.
#[test]
fn wpt_a_factor_of_exactly_one_reduces_nothing() {
    let mut path = Path::new();
    path.rounded_rect_varying(0.0, 0.0, 100.0, 100.0, 50.0, 50.0, 30.0, 30.0);
    let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);

    assert_circular(c.top_left, 50.0, "top left");
    assert_circular(c.top_right, 50.0, "top right");
    assert_circular(c.bottom_right, 30.0, "bottom right");
    assert_circular(c.bottom_left, 30.0, "bottom left");
}

/// The geometry behind WPT `2d.path.roundrect.radius.intersecting.1` and `.2`,
/// which fill a 100x50 canvas with radii of 40 and then 1000. Both reduce to a
/// circular 25, giving a stadium.
///
/// Those two tests only sample pixels well inside the shape, so per-axis
/// clamping — 40x25 and 50x25 ellipses — passes them both. Asserting the radii
/// directly is what separates the two behaviours, so this covers the intent of
/// the WPT cases rather than reproducing assertions that cannot fail.
#[test]
fn wpt_canvas_intersecting_radii_reduce_to_a_circle() {
    for radius in [40.0f32, 1000.0] {
        let mut path = Path::new();
        path.rounded_rect(0.0, 0.0, 100.0, 50.0, radius);
        let c = corners_of(&path, 0.0, 0.0, 100.0, 50.0);
        for (corner, name) in [
            (c.top_left, "top left"),
            (c.top_right, "top right"),
            (c.bottom_right, "bottom right"),
            (c.bottom_left, "bottom left"),
        ] {
            assert_circular(corner, 25.0, &format!("radius {radius} {name}"));
        }
    }
}

/// A box with no room at all collapses the radii rather than emitting geometry
/// outside it or dividing by zero.
#[test]
fn a_degenerate_box_produces_finite_geometry() {
    for (w, h) in [(0.0f32, 100.0f32), (100.0, 0.0), (0.0, 0.0)] {
        let mut path = Path::new();
        path.rounded_rect(0.0, 0.0, w, h, 40.0);
        for verb in path.verbs() {
            let coords = match verb {
                Verb::MoveTo(a, b) | Verb::LineTo(a, b) => vec![a, b],
                Verb::BezierTo(a, b, c, d, e, f) => vec![a, b, c, d, e, f],
                _ => vec![],
            };
            for v in coords {
                assert!(v.is_finite(), "{w}x{h} produced a non-finite coordinate");
            }
        }
    }
}
