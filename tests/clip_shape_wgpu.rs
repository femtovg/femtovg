//! Headless GPU tests for clips taken as shapes: a `clip_path` whose path
//! outlines a rectangle, a rounded rectangle or an ellipse is evaluated by
//! the fragment shader, so its edge takes coverage. Each shape as a clip
//! must cover what the same shape covers as a fill, gate every kind of draw,
//! nest, and belong to the target it was taken on. Skips without a GPU
//! adapter.
#![cfg(feature = "wgpu")]

use femtovg::{
    renderer::WGPURenderer, Canvas, Color, CompositeOperation, FillRule, ImageFlags, LayerEffects, LineJoin, Paint,
    Path, PixelFormat, RenderTarget, Transform2D,
};

mod common;
use common::{headless_device, render_rgba};

const W: u32 = 96;
const H: u32 = 96;
const WHITE: [u8; 3] = [255, 255, 255];
const RED: [u8; 3] = [255, 0, 0];

fn red() -> Paint {
    Paint::color(Color::rgb(255, 0, 0))
}

/// Fills the canvas with a path that is no rect, so that the clip's coverage
/// is what cuts it: a rect under an upright rect clip would be drawn as the
/// rect the two share, its edge the fill's own antialiasing.
fn fill_everything(canvas: &mut Canvas<WGPURenderer>) {
    let (w, h) = (W as f32, H as f32);
    let mut everything = Path::new();
    everything.move_to(-8.0, -8.0);
    everything.line_to(w + 8.0, -8.0);
    everything.line_to(w + 8.0, h + 8.0);
    everything.line_to(-8.0, h + 8.0);
    everything.line_to(-24.0, h * 0.5);
    everything.close();
    canvas.fill_path(&everything, &red());
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2]]
}

fn render(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut Canvas<WGPURenderer>)) -> Vec<u8> {
    render_rgba(device, queue, W, H, Color::white(), draw)
}

/// The largest channel difference between two frames, and how many pixels
/// of the first are partly covered (neither white nor red).
fn compare(a: &[u8], b: &[u8]) -> (u8, usize) {
    let worst = a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    let partial = a.chunks_exact(4).filter(|p| p[1] != 0 && p[1] != 255).count();
    (worst, partial)
}

fn shapes() -> Vec<(&'static str, Path, f32)> {
    let mut circle = Path::new();
    circle.circle(48.3, 47.6, 30.4);
    let mut ellipse = Path::new();
    ellipse.ellipse(48.0, 48.5, 40.25, 17.5);
    let mut rounded = Path::new();
    rounded.rounded_rect(12.4, 20.7, 70.0, 50.5, 14.0);
    let mut rect = Path::new();
    rect.rect(20.3, 30.6, 50.5, 30.25);
    vec![
        ("circle", circle, 0.0),
        ("ellipse", ellipse, 0.0),
        ("rounded rect", rounded, 0.0),
        ("rect", rect.clone(), 0.0),
        ("rotated rect", rect, 0.4),
    ]
}

/// A shape as a clip covers what it covers as a fill, edge pixels included:
/// as many partly covered pixels, none further from the fill than the
/// quarter pixel a fill's flattened outline may stray. Under a rotation and
/// at a device pixel ratio of two as well.
#[test]
fn a_shape_clip_covers_its_edge_as_the_fill_of_the_shape_does() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    for dpr in [1.0, 2.0] {
        for (name, shape, angle) in shapes() {
            let place = |canvas: &mut Canvas<WGPURenderer>| {
                canvas.set_size(W, H, dpr);
                canvas.translate(48.0, 48.0);
                canvas.rotate(angle);
                canvas.translate(-48.0, -48.0);
            };
            let clipped = render(&device, &queue, |canvas| {
                place(canvas);
                canvas.clip_path(&shape, FillRule::NonZero);
                canvas.reset_transform();
                fill_everything(canvas);
            });
            let filled = render(&device, &queue, |canvas| {
                place(canvas);
                canvas.fill_path(&shape, &red());
            });
            let (worst, partial) = compare(&clipped, &filled);
            let (_, partial_fill) = compare(&filled, &clipped);
            assert!(worst <= 64, "{name} at dpr {dpr}: a pixel differs by {worst}");
            assert!(partial > 60, "{name} at dpr {dpr}: only {partial} edge pixels");
            assert!(
                partial.abs_diff(partial_fill) * 20 <= partial_fill,
                "{name} at dpr {dpr}: {partial} edge pixels against the fill's {partial_fill}"
            );
        }
    }
}

/// The coverage is the share of the pixel inside the shape: three quarters
/// of a column, half a row, their product at the corner, and across a
/// circle's edge the distance from it.
#[test]
fn clip_coverage_is_the_share_of_the_pixel_inside() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    // Red over white: the green channel is what the clip left uncovered.
    let covered = |buf: &[u8], x: u32, y: u32| 1.0 - f32::from(px(buf, x, y)[1]) / 255.0;
    let near = |got: f32, want: f32| (got - want).abs() < 0.02;

    let rect = render(&device, &queue, |canvas| {
        let mut clip = Path::new();
        clip.rect(20.25, 30.5, 40.5, 20.25);
        canvas.clip_path(&clip, FillRule::NonZero);
        fill_everything(canvas);
    });
    for (x, y, want) in [
        (20, 40, 0.75),
        (60, 40, 0.75),
        (40, 30, 0.5),
        (40, 50, 0.75),
        (20, 30, 0.375),
        (60, 50, 0.5625),
        (40, 40, 1.0),
        (19, 40, 0.0),
        (40, 51, 0.0),
    ] {
        assert!(
            near(covered(&rect, x, y), want),
            "rect ({x},{y}): {}",
            covered(&rect, x, y)
        );
    }

    let circle = render(&device, &queue, |canvas| {
        let mut clip = Path::new();
        clip.circle(48.5, 47.5, 30.2);
        canvas.clip_path(&clip, FillRule::NonZero);
        fill_everything(canvas);
    });
    // Along the row through the center the edge is at x = 78.7 and 18.3.
    for (x, want) in [(77, 1.0), (78, 0.7), (79, 0.0), (18, 0.7), (17, 0.0), (48, 1.0)] {
        assert!(
            near(covered(&circle, x, 47), want),
            "circle x {x}: {}",
            covered(&circle, x, 47)
        );
    }
}

/// The share of each pixel inside a box with round corners under
/// `transform`, from 16 x 16 samples.
fn share_inside(transform: &Transform2D, center: [f32; 2], extent: [f32; 2], radius: f32) -> Vec<f32> {
    let inverse = transform.inverse();
    let inside = |x: f32, y: f32| {
        let (x, y) = inverse.transform_point(x, y);
        let side = [(x - center[0]).abs() - extent[0], (y - center[1]).abs() - extent[1]];
        let corner = [side[0] + radius, side[1] + radius];
        if corner[0] > 0.0 && corner[1] > 0.0 {
            corner[0].hypot(corner[1]) <= radius
        } else {
            side[0] <= 0.0 && side[1] <= 0.0
        }
    };
    (0..W * H)
        .map(|i| {
            let (left, top) = ((i % W) as f32, (i / W) as f32);
            let hits = (0..256)
                .filter(|s| {
                    inside(
                        left + ((s % 16) as f32 + 0.5) / 16.0,
                        top + ((s / 16) as f32 + 0.5) / 16.0,
                    )
                })
                .count();
            hits as f32 / 256.0
        })
        .collect()
}

/// The coverage is the pixel's share inside the shape under a transform
/// that skews its frame too - turned and then stretched, or sheared, so a
/// round corner lies askew between sides that are not at a right angle -
/// for corners tighter than a pixel, which are taken as square, and for a
/// box down to one pixel thick.
#[test]
fn clip_coverage_holds_under_a_skew_and_down_to_a_pixel() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    type Place = fn(&mut Canvas<WGPURenderer>);
    let turned_then_stretched: Place = |canvas| {
        canvas.translate(48.0, 48.0);
        canvas.scale(1.4, 0.6);
        canvas.rotate(0.5);
        canvas.translate(-48.0, -48.0);
    };
    let sheared: Place = |canvas| {
        canvas.translate(48.0, 48.0);
        canvas.skew_x(0.6);
        canvas.translate(-48.0, -48.0);
    };
    let turned: Place = |canvas| {
        canvas.translate(48.0, 48.0);
        canvas.rotate(0.4);
        canvas.translate(-48.0, -48.0);
    };
    let in_place: Place = |_| {};
    for (name, place, center, extent, radius) in [
        (
            "circle, turned then stretched",
            turned_then_stretched,
            [48.3, 47.6],
            [30.4, 30.4],
            30.4,
        ),
        (
            "rounded rect, turned then stretched",
            turned_then_stretched,
            [47.4, 45.95],
            [27.0, 20.25],
            12.0,
        ),
        ("rounded rect, sheared", sheared, [47.4, 45.95], [22.0, 28.25], 12.0),
        ("circle, sheared", sheared, [48.3, 47.6], [24.4, 24.4], 24.4),
        ("corners of 0.3 px", in_place, [47.4, 45.95], [35.0, 25.25], 0.3),
        ("a box one pixel tall", in_place, [47.4, 40.3], [35.0, 0.5], 0.0),
        ("a box 1.2 px wide, turned", turned, [47.45, 46.2], [0.6, 36.0], 0.0),
    ] {
        let mut expected = Vec::new();
        let frame = render(&device, &queue, |canvas| {
            place(canvas);
            expected = share_inside(&canvas.transform(), center, extent, radius);
            let mut clip = Path::new();
            clip.rounded_rect(
                center[0] - extent[0],
                center[1] - extent[1],
                2.0 * extent[0],
                2.0 * extent[1],
                radius,
            );
            canvas.clip_path(&clip, FillRule::NonZero);
            canvas.reset_transform();
            fill_everything(canvas);
        });
        // Red over white: the green channel is what the clip left uncovered.
        let (worst, at) = expected
            .iter()
            .enumerate()
            .map(|(i, want)| ((1.0 - f32::from(frame[i * 4 + 1]) / 255.0 - want).abs(), i))
            .fold((0.0, 0), |a, b| if b.0 > a.0 { b } else { a });
        let partial = expected.iter().filter(|share| **share > 0.0 && **share < 1.0).count();
        assert!(partial > 60, "{name}: only {partial} edge pixels");
        assert!(
            worst < 0.08,
            "{name}: ({}, {}) is {worst} from its share inside",
            at as u32 % W,
            at as u32 / W
        );
    }
}

/// The share of each pixel where `inside` holds, from 16 x 16 samples.
fn share_where(inside: impl Fn(f32, f32) -> bool) -> Vec<f32> {
    (0..W * H)
        .map(|i| {
            let (left, top) = ((i % W) as f32, (i / W) as f32);
            let hits = (0..256)
                .filter(|s| {
                    inside(
                        left + ((s % 16) as f32 + 0.5) / 16.0,
                        top + ((s / 16) as f32 + 0.5) / 16.0,
                    )
                })
                .count();
            hits as f32 / 256.0
        })
        .collect()
}

/// A rect clip that cuts a clip with round corners across its straight
/// sides, and a rectangle rounded on one side only, as a clip: each pixel is
/// covered by its share inside what both leave - upright, and turned - with
/// no stencil to take a pixel whole or not at all, and an edge the two share
/// once; an upright rect filled under them whose side lies on the cut takes
/// that edge's coverage once too.
#[test]
fn a_rounded_clip_cut_by_a_rect_covers_each_pixel_by_its_share_inside_both() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let rounded = |x: f32, y: f32, center: [f32; 2], extent: [f32; 2], r: f32| {
        let side = [(x - center[0]).abs() - extent[0], (y - center[1]).abs() - extent[1]];
        let corner = [side[0] + r, side[1] + r];
        if corner[0] > 0.0 && corner[1] > 0.0 {
            corner[0].hypot(corner[1]) <= r
        } else {
            side[0] <= 0.0 && side[1] <= 0.0
        }
    };
    type Place = fn(&mut Canvas<WGPURenderer>);
    let turned: Place = |canvas| {
        canvas.translate(48.0, 48.0);
        canvas.rotate(0.35);
        canvas.translate(-48.0, -48.0);
    };
    let in_place: Place = |_| {};
    for (name, place) in [("upright", in_place), ("turned", turned)] {
        // A window and the rect its content is cut to: the rect shares the
        // window's sides and leaves its bottom, and its top crosses the
        // window's straight sides.
        let (center, extent, r) = ([48.25, 47.5], [36.5, 30.25], 9.5);
        let mut expected = Vec::new();
        let cut = render(&device, &queue, |canvas| {
            place(canvas);
            let inverse = canvas.transform().inverse();
            expected = share_where(|x, y| {
                let (x, y) = inverse.transform_point(x, y);
                rounded(x, y, center, extent, r) && y >= 31.3
            });
            let mut window = Path::new();
            window.rounded_rect(
                center[0] - extent[0],
                center[1] - extent[1],
                2.0 * extent[0],
                2.0 * extent[1],
                r,
            );
            canvas.clip_path(&window, FillRule::NonZero);
            let mut content = Path::new();
            content.rect(center[0] - extent[0], 31.3, 2.0 * extent[0], 80.0);
            canvas.clip_path(&content, FillRule::NonZero);
            canvas.reset_transform();
            fill_everything(canvas);
        });
        // The content itself: a rect whose top lies on the cut, which it
        // shares - the edge is the rect's, once, not its coverage squared.
        let mut content_expected = Vec::new();
        let content = render(&device, &queue, |canvas| {
            place(canvas);
            let inverse = canvas.transform().inverse();
            content_expected = share_where(|x, y| {
                let (x, y) = inverse.transform_point(x, y);
                rounded(x, y, center, extent, r) && y >= 31.3
            });
            let mut window = Path::new();
            window.rounded_rect(
                center[0] - extent[0],
                center[1] - extent[1],
                2.0 * extent[0],
                2.0 * extent[1],
                r,
            );
            canvas.clip_path(&window, FillRule::NonZero);
            let mut cut = Path::new();
            cut.rect(center[0] - extent[0], 31.3, 2.0 * extent[0], 80.0);
            canvas.clip_path(&cut, FillRule::NonZero);
            let mut rect = Path::new();
            rect.rect(-20.0, 31.3, 140.0, 100.0);
            canvas.fill_path(&rect, &red());
        });
        // A title bar: round top corners, a square bottom.
        let (left, top, width, height) = (12.25, 18.5, 70.5, 26.75);
        let mut bar_expected = Vec::new();
        let bar = render(&device, &queue, |canvas| {
            place(canvas);
            let inverse = canvas.transform().inverse();
            bar_expected = share_where(|x, y| {
                let (x, y) = inverse.transform_point(x, y);
                let tall = [left + width * 0.5, top + height];
                rounded(x, y, tall, [width * 0.5, height], r) && y <= top + height
            });
            let mut clip = Path::new();
            clip.rounded_rect_varying(left, top, width, height, r, r, 0.0, 0.0);
            canvas.clip_path(&clip, FillRule::NonZero);
            canvas.reset_transform();
            fill_everything(canvas);
        });
        // A rect is drawn as the rect it shares with a box only upright.
        let content_case = (name == "upright").then_some(("content on the cut", &content, &content_expected));
        for (what, frame, expected) in [
            Some(("cut window", &cut, &expected)),
            content_case,
            Some(("title bar", &bar, &bar_expected)),
        ]
        .into_iter()
        .flatten()
        {
            // Red over white: the green channel is what the clip left uncovered.
            let (worst, at) = expected
                .iter()
                .enumerate()
                .map(|(i, want)| ((1.0 - f32::from(frame[i * 4 + 1]) / 255.0 - want).abs(), i))
                .fold((0.0, 0), |a, b| if b.0 > a.0 { b } else { a });
            let partial = expected.iter().filter(|share| **share > 0.0 && **share < 1.0).count();
            assert!(partial > 60, "{what}, {name}: only {partial} edge pixels");
            assert!(
                worst < 0.08,
                "{what}, {name}: ({}, {}) is {worst} from its share inside",
                at as u32 % W,
                at as u32 / W
            );
        }
    }
}

/// An edge takes coverage once where a draw stays inside its clip along it,
/// where an upright rect clip cuts an upright rect, where an upright rect
/// covers a clip with round corners, and where a scissor lies on the clip -
/// and the same under a scissor alone, which is a clip box like any other:
/// each edge pixel gets its share inside what is left - not that share
/// squared, or cubed, which the coverages of the scissor, the clip and the
/// fill's own antialiasing come to one over the other.
#[test]
fn an_edge_shared_with_the_clip_takes_coverage_once() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    type Place = fn(&mut Canvas<WGPURenderer>);
    type Shape = ([f32; 2], [f32; 2], f32);
    let in_place: Place = |_| {};
    // Puts the small shape's left side and top on half a pixel.
    let scaled: Place = |canvas| {
        canvas.translate(2.875, -2.625);
        canvas.scale(1.5625, 1.5625);
    };
    let rect: Shape = ([47.5, 45.75], [30.0, 20.5], 0.0);
    let rounded: Shape = ([47.5, 45.75], [30.0, 20.5], 9.0);
    let small: Shape = ([28.0, 30.0], [18.0, 12.0], 0.0);
    let small_rounded: Shape = ([28.0, 30.0], [18.0, 12.0], 5.0);
    let everything: Shape = ([48.0, 48.0], [60.0, 60.0], 0.0);
    // The left half of `rect`: three of its sides lie on the clip's.
    let left_half: Shape = ([32.5, 45.75], [15.0, 20.5], 0.0);
    // Reaches past `rect` to the right and below, its left side and top on the clip's.
    let past: Shape = ([62.5, 60.75], [45.0, 35.5], 0.0);
    // Inside `rounded` along its left side, clear of its corners.
    let along: Shape = ([27.5, 45.75], [10.0, 10.5], 0.0);
    // (what, placement, scissor, clip, fill, what is left of the fill)
    type Case = (&'static str, Place, Option<Shape>, Option<Shape>, Shape, Shape);
    let cases: [Case; 18] = [
        ("a rect and its twin", in_place, None, Some(rect), rect, rect),
        (
            "a rect on a rounded clip's sides",
            in_place,
            None,
            Some(rounded),
            rect,
            rounded,
        ),
        (
            "a rect around a rounded clip, scaled",
            scaled,
            None,
            Some(small_rounded),
            everything,
            small_rounded,
        ),
        ("a rect and its twin, scaled", scaled, None, Some(small), small, small),
        (
            "a rect inside, on three of the clip's sides",
            in_place,
            None,
            Some(rect),
            left_half,
            left_half,
        ),
        (
            "a rect past the clip, on two of its sides",
            in_place,
            None,
            Some(rect),
            past,
            rect,
        ),
        (
            "a rect inside a rounded clip, along its side",
            in_place,
            None,
            Some(rounded),
            along,
            along,
        ),
        (
            "a scissor on the clip",
            in_place,
            Some(rect),
            Some(rect),
            everything,
            rect,
        ),
        (
            "a scissor on a rounded clip's sides",
            in_place,
            Some(rect),
            Some(rounded),
            everything,
            rounded,
        ),
        (
            "a scissor, a clip and a fill with one outline",
            in_place,
            Some(rect),
            Some(rect),
            rect,
            rect,
        ),
        ("the same, scaled", scaled, Some(small), Some(small), small, small),
        (
            "a scissor and a rect with one outline",
            in_place,
            Some(rect),
            None,
            rect,
            rect,
        ),
        ("the same, scaled", scaled, Some(small), None, small, small),
        (
            "a rect inside a scissor, on three of its sides",
            in_place,
            Some(rect),
            None,
            left_half,
            left_half,
        ),
        (
            "a rect past a scissor, on two of its sides",
            in_place,
            Some(rect),
            None,
            past,
            rect,
        ),
        (
            "a rounded rect inside a scissor, on its sides",
            in_place,
            Some(rect),
            None,
            rounded,
            rounded,
        ),
        (
            "a rect on a rounded scissor's sides",
            in_place,
            Some(rounded),
            None,
            rect,
            rounded,
        ),
        (
            "a rect around a rounded scissor, scaled",
            scaled,
            Some(small_rounded),
            None,
            everything,
            small_rounded,
        ),
    ];
    for (name, place, scissor, clip, fill, covered) in cases {
        let path = |(center, extent, radius): Shape| {
            let mut path = Path::new();
            path.rounded_rect(
                center[0] - extent[0],
                center[1] - extent[1],
                2.0 * extent[0],
                2.0 * extent[1],
                radius,
            );
            path
        };
        let mut expected = Vec::new();
        let frame = render(&device, &queue, |canvas| {
            place(canvas);
            expected = share_inside(&canvas.transform(), covered.0, covered.1, covered.2);
            if let Some((center, extent, radius)) = scissor {
                canvas.rounded_scissor(
                    center[0] - extent[0],
                    center[1] - extent[1],
                    2.0 * extent[0],
                    2.0 * extent[1],
                    radius,
                );
            }
            if let Some(clip) = clip {
                canvas.clip_path(&path(clip), FillRule::NonZero);
            }
            canvas.fill_path(&path(fill), &red());
        });
        // What a fill's own antialiasing is off by, at a corner pixel most.
        let plain = render(&device, &queue, |canvas| {
            place(canvas);
            canvas.fill_path(&path(covered), &red());
        });
        // Red over white: the green channel is what was left uncovered.
        let errors = |frame: &[u8]| -> Vec<f32> {
            expected
                .iter()
                .enumerate()
                .map(|(i, want)| (1.0 - f32::from(frame[i * 4 + 1]) / 255.0 - want).abs())
                .collect()
        };
        let worst = |errors: &[f32]| errors.iter().copied().fold(0.0, f32::max);
        let (clipped, plain) = (errors(&frame), errors(&plain));
        // Where a pixel is half inside, a square is a quarter off.
        let halves: Vec<f32> = expected
            .iter()
            .zip(&clipped)
            .filter(|(share, _)| **share > 0.35 && **share < 0.65)
            .map(|(_, error)| *error)
            .collect();
        assert!(
            halves.len() > 20,
            "{name}: only {} pixels near half covered",
            halves.len()
        );
        let mean = halves.iter().sum::<f32>() / halves.len() as f32;
        assert!(
            mean < 0.04,
            "{name}: half-covered pixels are {mean} from their share on average"
        );
        assert!(
            worst(&clipped) <= worst(&plain).max(0.08) + 0.02,
            "{name}: a pixel is {} from its share inside, the fill alone {}",
            worst(&clipped),
            worst(&plain)
        );
    }
}

/// Where a fill reaches past a clip that is not an upright rect and has an
/// edge on the clip's edge elsewhere, the clip's coverage multiplies the
/// fill's own along that edge - as it does in Chromium, WebKit and Firefox: a
/// pixel half inside both is a quarter covered. Along the side the fill
/// reaches past, the clip's coverage is all there is.
#[test]
fn a_clip_multiplies_the_coverage_of_a_fill_that_shares_part_of_its_edge() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    type Place = fn(&mut Canvas<WGPURenderer>);
    let in_place: Place = |_| {};
    let turned: Place = |canvas| {
        canvas.translate(48.0, 48.0);
        canvas.rotate(0.4);
        canvas.translate(-48.0, -48.0);
    };
    for (name, place, radius) in [("a rounded clip", in_place, 9.0), ("a turned clip", turned, 0.0)] {
        let (center, extent) = ([47.5, 45.75], [30.0, 20.5]);
        let mut expected = Vec::new();
        let mut to_local = Transform2D::identity();
        let frame = render(&device, &queue, |canvas| {
            place(canvas);
            expected = share_inside(&canvas.transform(), center, extent, radius);
            to_local = canvas.transform().inverse();
            let (x, y) = (center[0] - extent[0], center[1] - extent[1]);
            let mut clip = Path::new();
            clip.rounded_rect(x, y, 2.0 * extent[0], 2.0 * extent[1], radius);
            // The clip's outline with its right side sixty units further out.
            let mut wider = Path::new();
            wider.rounded_rect(x, y, 2.0 * extent[0] + 60.0, 2.0 * extent[1], radius);
            canvas.clip_path(&clip, FillRule::NonZero);
            canvas.fill_path(&wider, &red());
        });
        let (mut shared, mut passed) = (Vec::new(), Vec::new());
        for (i, share) in expected.iter().enumerate() {
            if *share > 0.45 && *share < 0.55 {
                let (px, py) = ((i as u32 % W) as f32 + 0.5, (i as u32 / W) as f32 + 0.5);
                let (local_x, _) = to_local.transform_point(px, py);
                let covered = 1.0 - f32::from(frame[i * 4 + 1]) / 255.0;
                if local_x > center[0] + extent[0] - 1.5 {
                    passed.push(covered);
                } else if local_x < center[0] + extent[0] - radius - 1.5 {
                    shared.push(covered);
                }
            }
        }
        let mean = |values: &[f32]| values.iter().sum::<f32>() / values.len() as f32;
        assert!(
            shared.len() > 10 && passed.len() > 2,
            "{name}: {} and {} pixels",
            shared.len(),
            passed.len()
        );
        assert!(
            (mean(&shared) - 0.25).abs() < 0.05,
            "{name}: on the shared edge {}",
            mean(&shared)
        );
        assert!(
            (mean(&passed) - 0.5).abs() < 0.05,
            "{name}: on the side passed {}",
            mean(&passed)
        );
    }
}

/// A draw whose outline lies inside the clip is drawn as without it, to the
/// bit, though its bounds stand past the clip's corners: the clip's own
/// outline filled under it - rounded, turned, an ellipse - and a stroke set
/// in from that outline by half its width. An edge the two have in common is
/// antialiased once, by the draw.
#[test]
fn a_twin_of_the_clip_is_drawn_as_without_it() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let turn = |canvas: &mut Canvas<WGPURenderer>, angle: f32| {
        canvas.translate(48.0, 48.0);
        canvas.rotate(angle);
        canvas.translate(-48.0, -48.0);
    };
    let mut rounded = Path::new();
    rounded.rounded_rect(17.5, 25.25, 60.0, 41.0, 9.0);
    let mut rect = Path::new();
    rect.rect(17.5, 25.25, 60.0, 41.0);
    let mut ellipse = Path::new();
    ellipse.ellipse(48.3, 47.6, 38.4, 21.7);
    let mut inset = Path::new();
    inset.rounded_rect(19.5, 27.25, 56.0, 37.0, 7.0);
    // Corners wide enough for their cubics to stray from the ellipse the
    // clip is by more than a 64th of a pixel, and an ellipse of eight
    // quadratics, which strays ten times as far.
    let mut wide = Path::new();
    wide.rounded_rect(-60.0, -70.0, 150.0, 160.0, 72.0);
    let mut quadratics = Path::new();
    let on = |i: usize, scale: f32| {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        (48.3 + 42.0 * scale * a.cos(), 47.6 + 31.0 * scale * a.sin())
    };
    quadratics.move_to(on(0, 1.0).0, on(0, 1.0).1);
    for i in 0..8 {
        let a = (i as f32 + 0.5) * std::f32::consts::FRAC_PI_4;
        let reach = 1.0 / std::f32::consts::FRAC_PI_8.cos();
        let (x, y) = on(i + 1, 1.0);
        quadratics.quad_to(48.3 + 42.0 * reach * a.cos(), 47.6 + 31.0 * reach * a.sin(), x, y);
    }
    quadratics.close();
    // A round join: a miter's limit counts toward how far a stroke reaches.
    let line = red().with_line_width(4.0).with_line_join(LineJoin::Round);
    for (name, clip, angle) in [
        ("a rounded twin", &rounded, 0.0),
        ("a rounded twin, turned", &rounded, 0.4),
        ("a turned twin", &rect, 0.4),
        ("an ellipse twin", &ellipse, 0.0),
        ("a twin with wide corners", &wide, 0.0),
        ("an ellipse of quadratics", &quadratics, 0.0),
    ] {
        let draw = |clipped: bool| {
            render(&device, &queue, |canvas| {
                turn(canvas, angle);
                if clipped {
                    canvas.clip_path(clip, FillRule::NonZero);
                }
                canvas.fill_path(clip, &red());
                if std::ptr::eq(clip, &rounded) {
                    canvas.stroke_path(&inset, &line);
                }
            })
        };
        let (under, without) = (draw(true), draw(false));
        let partial = without.chunks_exact(4).filter(|p| p[1] != 0 && p[1] != 255).count();
        assert!(partial > 60, "{name}: only {partial} edge pixels");
        assert!(under == without, "{name}: the clip changed its twin");
    }
}

/// A draw the clip holds whole is drawn as it is without the clip, to the
/// bit: a fill, a stroke with a miter and a square cap, and a fill that
/// touches the clip's edge from inside - first under the clip, and after a
/// draw that the clip did cut, when they carry a coverage of one.
#[test]
fn a_draw_inside_the_clip_is_drawn_as_without_it() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let draw = |canvas: &mut Canvas<WGPURenderer>| {
        let mut blob = Path::new();
        blob.move_to(30.3, 28.6);
        blob.bezier_to(50.0, 20.0, 66.0, 40.0, 60.4, 58.2);
        blob.line_to(34.7, 62.9);
        blob.close();
        canvas.fill_path(&blob, &red());
        let mut zigzag = Path::new();
        zigzag.move_to(26.0, 70.0);
        zigzag.line_to(40.5, 40.25);
        zigzag.line_to(52.0, 68.0);
        let mut pen = Paint::color(Color::rgb(0, 0, 255));
        pen.set_line_width(5.5);
        pen.set_line_cap(femtovg::LineCap::Square);
        canvas.stroke_path(&zigzag, &pen);
        let mut edge = Path::new();
        edge.rect(12.5, 30.25, 20.0, 10.5);
        canvas.fill_path(&edge, &Paint::color(Color::rgb(0, 128, 0)));
    };
    for rounded in [false, true] {
        let clip = |canvas: &mut Canvas<WGPURenderer>| {
            let mut clip = Path::new();
            if rounded {
                clip.rounded_rect(12.5, 14.25, 70.0, 68.5, 16.0);
            } else {
                clip.rect(12.5, 14.25, 70.0, 68.5);
            }
            canvas.clip_path(&clip, FillRule::NonZero);
        };
        let clipped = render(&device, &queue, |canvas| {
            clip(canvas);
            draw(canvas);
        });
        let plain = render(&device, &queue, draw);
        assert!(clipped == plain, "rounded {rounded}: the clip changed a draw it holds");
        assert!(plain.chunks_exact(4).any(|p| p[..3] != WHITE), "something was drawn");

        let cut = |canvas: &mut Canvas<WGPURenderer>| {
            canvas.save();
            clip(canvas);
            canvas.set_global_alpha(0.25);
            fill_everything(canvas);
            canvas.set_global_alpha(1.0);
        };
        let after_a_cut = render(&device, &queue, |canvas| {
            cut(canvas);
            draw(canvas);
        });
        let after_the_restore = render(&device, &queue, |canvas| {
            cut(canvas);
            canvas.restore();
            draw(canvas);
        });
        assert!(
            after_a_cut == after_the_restore,
            "rounded {rounded}: a coverage of one changed a draw the clip holds"
        );
        assert!(after_a_cut != plain, "the cut draw shows");
    }
}

/// A box thinner than a pixel goes to the stencil, where it leaves a pixel
/// only if it holds the pixel's center: no ramp of a side reaches a row the
/// box does not.
#[test]
fn a_clip_thinner_than_a_pixel_leaves_no_ghost_of_the_fill() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let sliver = |top: f32| {
        render(&device, &queue, |canvas| {
            let mut clip = Path::new();
            clip.rect(10.0, top, 70.0, 0.3);
            canvas.clip_path(&clip, FillRule::NonZero);
            fill_everything(canvas);
        })
    };
    let between_centers = sliver(40.6);
    assert!(
        between_centers.chunks_exact(4).all(|p| p[..3] == WHITE),
        "a sliver that holds no pixel center leaves nothing"
    );
    let on_a_center = sliver(40.4);
    assert_eq!(px(&on_a_center, 40, 40), RED, "the row whose centers it holds");
    assert_eq!(px(&on_a_center, 40, 39), WHITE);
    assert_eq!(px(&on_a_center, 40, 41), WHITE);
}

/// Fills of either kind, strokes and image blits are all gated by the shape.
#[test]
fn a_shape_clip_gates_every_kind_of_draw() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let clip_circle = |canvas: &mut Canvas<WGPURenderer>| {
        let mut clip = Path::new();
        clip.circle(48.0, 48.0, 30.0);
        canvas.clip_path(&clip, FillRule::NonZero);
    };
    let inside_and_out = |buf: &[u8], what: &str| {
        assert_eq!(px(buf, 48, 48), RED, "{what}: inside");
        for (x, y) in [(4, 4), (90, 6), (6, 90), (90, 90), (48, 10)] {
            assert_eq!(px(buf, x, y), WHITE, "{what}: ({x},{y}) is outside the circle");
        }
    };

    let concave = render(&device, &queue, |canvas| {
        clip_circle(canvas);
        let mut bowtie = Path::new();
        bowtie.move_to(0.0, 0.0);
        bowtie.line_to(96.0, 96.0);
        bowtie.line_to(96.0, 0.0);
        bowtie.line_to(0.0, 96.0);
        bowtie.close();
        canvas.fill_path(&bowtie, &red());
    });
    assert_eq!(px(&concave, 60, 48), RED, "the right wing, inside the circle");
    assert_eq!(px(&concave, 90, 48), WHITE, "the right wing, outside the circle");
    assert_eq!(px(&concave, 48, 30), WHITE, "between the wings");

    let stroked = render(&device, &queue, |canvas| {
        clip_circle(canvas);
        let mut cross = Path::new();
        cross.move_to(0.0, 48.0);
        cross.line_to(96.0, 48.0);
        cross.move_to(4.0, 4.0);
        cross.line_to(92.0, 92.0);
        let mut paint = red();
        paint.set_line_width(10.0);
        canvas.stroke_path(&cross, &paint);
    });
    inside_and_out(&stroked, "stroke");
    assert_eq!(px(&stroked, 6, 48), WHITE, "the horizontal stroke stops at the circle");

    let blitted = render(&device, &queue, |canvas| {
        let image = canvas
            .create_image_empty(W as usize, H as usize, PixelFormat::Rgba8, ImageFlags::empty())
            .unwrap();
        canvas.set_render_target(RenderTarget::Image(image));
        canvas.clear_rect(0, 0, W, H, Color::rgb(255, 0, 0));
        canvas.set_render_target(RenderTarget::Screen);
        clip_circle(canvas);
        let mut blit = Path::new();
        blit.rect(0.0, 0.0, W as f32, H as f32);
        let mut paint = Paint::image(image, 0.0, 0.0, W as f32, H as f32, 0.0, 1.0);
        paint.set_anti_alias(false);
        canvas.fill_path(&blit, &paint);
    });
    inside_and_out(&blitted, "image blit");
}

/// A shape inside the one in force replaces it until the restore; rects
/// that overlap clip to what both cover; any other overlap intersects
/// through the stencil; a scissor still applies on top.
#[test]
fn shape_clips_nest_and_intersect() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let circle = |cx: f32, cy: f32, r: f32| {
        let mut path = Path::new();
        path.circle(cx, cy, r);
        path
    };
    let nested = render(&device, &queue, |canvas| {
        canvas.clip_path(&circle(48.0, 48.0, 44.0), FillRule::NonZero);
        canvas.save();
        let mut inner = Path::new();
        inner.rect(40.0, 40.0, 16.0, 16.0);
        canvas.clip_path(&inner, FillRule::NonZero);
        fill_everything(canvas);
        canvas.restore();
        let mut band = Path::new();
        band.rect(0.0, 80.0, 96.0, 8.0);
        canvas.fill_path(&band, &red());
    });
    assert_eq!(px(&nested, 48, 48), RED, "inside the inner rect");
    assert_eq!(
        px(&nested, 30, 48),
        WHITE,
        "inside the circle only, under the inner clip"
    );
    assert_eq!(px(&nested, 48, 84), RED, "after the restore the circle is back");
    assert_eq!(px(&nested, 6, 84), WHITE, "the band stops at the circle");

    let crossed = render(&device, &queue, |canvas| {
        let rect = |x: f32, y: f32, w: f32, h: f32| {
            let mut path = Path::new();
            path.rect(x, y, w, h);
            path
        };
        canvas.clip_path(&rect(10.0, 30.25, 70.0, 30.0), FillRule::NonZero);
        canvas.clip_path(&rect(40.5, 10.0, 30.0, 70.0), FillRule::NonZero);
        fill_everything(canvas);
    });
    assert_eq!(px(&crossed, 55, 45), RED, "in both rects");
    assert_eq!(px(&crossed, 20, 45), WHITE, "in the first only");
    assert_eq!(px(&crossed, 55, 70), WHITE, "in the second only");
    assert!(
        px(&crossed, 40, 45)[1].abs_diff(128) <= 1,
        "half a column at the second rect's edge: {:?}",
        px(&crossed, 40, 45)
    );
    assert_eq!(
        px(&crossed, 55, 30),
        [255, 64, 64],
        "three quarters of a row at the first rect's"
    );

    let overlapping = render(&device, &queue, |canvas| {
        canvas.clip_path(&circle(36.0, 48.0, 30.0), FillRule::NonZero);
        canvas.clip_path(&circle(60.0, 48.0, 30.0), FillRule::NonZero);
        canvas.scissor(0.0, 0.0, 96.0, 48.0);
        fill_everything(canvas);
    });
    assert_eq!(
        px(&overlapping, 48, 40),
        RED,
        "in both circles, above the scissor's edge"
    );
    assert_eq!(px(&overlapping, 48, 56), WHITE, "in both circles, below the scissor");
    assert_eq!(px(&overlapping, 16, 40), WHITE, "in the first circle only");
    assert_eq!(px(&overlapping, 80, 40), WHITE, "in the second circle only");
}

/// Two frames that all but coincide clip to what both cover: where the
/// inner one reaches a fraction of a pixel past the outer, the outer's edge
/// is the one that counts, and on the other sides the inner's.
#[test]
fn nested_frames_that_nearly_coincide_clip_to_what_both_cover() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let frame = |x: f32, y: f32, w: f32, h: f32| {
        let mut path = Path::new();
        path.rounded_rect(x, y, w, h, 14.0);
        path
    };
    let buf = render(&device, &queue, |canvas| {
        // x 10..80 and y 10..80, then x 10.6..80.4 and y 10..79.6.
        canvas.clip_path(&frame(10.0, 10.0, 70.0, 70.0), FillRule::NonZero);
        canvas.clip_path(&frame(10.6, 10.0, 69.8, 69.6), FillRule::NonZero);
        fill_everything(canvas);
    });
    let covered = |x: u32, y: u32| 1.0 - f32::from(px(&buf, x, y)[1]) / 255.0;
    for (x, y, want, what) in [
        (80, 45, 0.0, "past the outer frame's right side"),
        (79, 45, 1.0, "inside both"),
        (10, 45, 0.4, "the inner frame's left side"),
        (45, 79, 0.6, "the inner frame's bottom side"),
        (45, 80, 0.0, "below the inner frame"),
        (45, 10, 1.0, "the top side they share"),
    ] {
        assert!(
            (covered(x, y) - want).abs() < 0.03,
            "{what}: ({x},{y}) is {}",
            covered(x, y)
        );
    }
}

/// A shape belongs to its target: on the canvas it gates a layer's
/// composite and not its content, inside the layer only the content, and
/// on an image nothing drawn to the screen.
#[test]
fn a_shape_clip_belongs_to_the_target_it_was_taken_on() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let mut circle = Path::new();
    circle.circle(48.0, 48.0, 40.0);
    let layered = render(&device, &queue, |canvas| {
        canvas.clip_path(&circle, FillRule::NonZero);
        assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
        fill_everything(canvas);
        canvas.end_layer();
    });
    assert_eq!(
        px(&layered, 48, 48),
        [255, 128, 128],
        "the layer, at half opacity, inside the circle"
    );
    assert_eq!(px(&layered, 4, 4), WHITE, "its composite is clipped to the circle");

    let inside = render(&device, &queue, |canvas| {
        assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
        canvas.clip_path(&circle, FillRule::NonZero);
        fill_everything(canvas);
        canvas.end_layer();
        let mut corner = Path::new();
        corner.rect(0.0, 0.0, 8.0, 8.0);
        canvas.fill_path(&corner, &red());
    });
    assert_eq!(px(&inside, 48, 48), [255, 128, 128]);
    assert_eq!(px(&inside, 90, 90), WHITE, "the layer's content was clipped");
    assert_eq!(px(&inside, 4, 4), RED, "the clip went with the layer");

    let on_image = render(&device, &queue, |canvas| {
        let image = canvas
            .create_image_empty(W as usize, H as usize, PixelFormat::Rgba8, ImageFlags::empty())
            .unwrap();
        canvas.set_render_target(RenderTarget::Image(image));
        canvas.clip_path(&circle, FillRule::NonZero);
        canvas.set_render_target(RenderTarget::Screen);
        fill_everything(canvas);
    });
    assert_eq!(px(&on_image, 4, 4), RED, "the image's clip does not gate the screen");
}

/// A draw under a small shape is scissored to the pixels the shape reaches,
/// its stencil draws with it: a concave fill that spans the target leaves
/// no winding outside the shape for the next fill to take as its own, and
/// the scissor ends with the draw. On the screen and in a layer, whose
/// image is stored bottom up.
#[test]
fn a_fill_under_a_small_shape_leaves_the_rest_of_the_target_alone() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let scene = |canvas: &mut Canvas<WGPURenderer>| {
        let mut clip = Path::new();
        clip.rounded_rect(8.0, 10.0, 30.0, 20.0, 6.0);
        let mut dented = Path::new();
        dented.move_to(-8.0, -8.0);
        dented.line_to(104.0, -8.0);
        dented.line_to(104.0, 104.0);
        dented.line_to(-8.0, 104.0);
        dented.line_to(4.0, 48.0);
        dented.close();
        let mut ring = Path::new();
        ring.rect(44.0, 34.0, 48.0, 58.0);
        ring.rect(54.0, 44.0, 28.0, 38.0);
        canvas.save();
        canvas.clip_path(&clip, FillRule::NonZero);
        canvas.fill_path(&dented, &red());
        canvas.restore();
        canvas.fill_path(
            &ring,
            &Paint::color(Color::rgb(0, 0, 255)).with_fill_rule(FillRule::EvenOdd),
        );
    };
    let check = |frame: &[u8], red: [u8; 3], blue: [u8; 3], on: &str| {
        assert_eq!(px(frame, 22, 20), red, "{on}: the fill, inside the shape");
        assert_eq!(px(frame, 22, 60), WHITE, "{on}: nothing of it below the shape");
        assert_eq!(px(frame, 48, 60), blue, "{on}: the ring, drawn whole after it");
        assert_eq!(px(frame, 88, 88), blue, "{on}: the ring's far corner");
        for y in 46..80 {
            for x in 56..80 {
                assert_eq!(px(frame, x, y), WHITE, "{on}: the ring's hole at {x},{y}");
            }
        }
    };
    let on_screen = render(&device, &queue, scene);
    check(&on_screen, RED, [0, 0, 255], "screen");
    let in_layer = render(&device, &queue, |canvas| {
        assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
        scene(canvas);
        canvas.end_layer();
    });
    check(&in_layer, [255, 128, 128], [128, 128, 255], "layer");
}

/// `Copy` replaces what it covers, transparent source included, so coverage
/// cannot hold it to a shape: under one the shape is cut on the stencil,
/// nothing outside it changes, and the winding a concave fill leaves there
/// is cleared for the draws that follow.
#[test]
fn an_operation_coverage_cannot_bound_stays_inside_the_shape() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let buf = render(&device, &queue, |canvas| {
        let mut clip = Path::new();
        clip.circle(48.0, 48.0, 30.0);
        canvas.clip_path(&clip, FillRule::NonZero);
        canvas.global_composite_operation(CompositeOperation::Copy);
        let mut bowtie = Path::new();
        bowtie.move_to(0.0, 0.0);
        bowtie.line_to(96.0, 96.0);
        bowtie.line_to(96.0, 0.0);
        bowtie.line_to(0.0, 96.0);
        bowtie.close();
        canvas.fill_path(&bowtie, &Paint::color(Color::rgba(255, 0, 0, 128)));
        canvas.global_composite_operation(CompositeOperation::SourceOver);
        let mut band = Path::new();
        band.rect(0.0, 44.0, 96.0, 8.0);
        canvas.fill_path(&band, &Paint::color(Color::rgb(0, 0, 255)));
    });
    assert_eq!(
        px(&buf, 66, 40),
        [128, 0, 0],
        "the copy replaced white with half-transparent red"
    );
    assert_eq!(px(&buf, 90, 40), WHITE, "outside the circle the copy changed nothing");
    assert_eq!(px(&buf, 48, 30), WHITE, "nor between the wings");
    for x in [24, 40, 56, 72] {
        assert_eq!(
            px(&buf, x, 48),
            [0, 0, 255],
            "the band at x {x}: no winding left behind"
        );
    }
    assert_eq!(px(&buf, 6, 48), WHITE, "the band stops at the circle");
}
