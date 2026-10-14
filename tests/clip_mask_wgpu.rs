//! Headless GPU tests for clips taken as coverage masks: a `clip_path` whose
//! path is no rectangle, rounded rectangle or ellipse is rasterized on the
//! CPU into one byte a pixel and read by the fragment shader of each draw
//! under it. Each pixel of such a clip's edge must take the share of it the
//! path covers; several paths clip to their union, each under its own rule;
//! masks nest, gate every kind of draw and belong to the target they were
//! taken on. The scenarios written for the stencil clip run on masks too, in
//! tests/path_clip_wgpu.rs and tests/wpt_clip_wgpu.rs. Skips without a GPU
//! adapter.
#![cfg(feature = "wgpu")]

use femtovg::{
    renderer::WGPURenderer, Canvas, Color, CompositeOperation, FillRule, ImageFlags, LayerEffects, Paint, Path,
    PixelFormat, RenderTarget, Transform2D,
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

fn fill_everything(canvas: &mut Canvas<WGPURenderer>) {
    let mut everything = Path::new();
    everything.rect(-8.0, -8.0, W as f32 + 16.0, H as f32 + 16.0);
    canvas.fill_path(&everything, &red());
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2]]
}

/// Red over white: the green channel is what was left uncovered.
fn covered(buf: &[u8], x: u32, y: u32) -> f32 {
    1.0 - f32::from(px(buf, x, y)[1]) / 255.0
}

fn render(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut Canvas<WGPURenderer>)) -> Vec<u8> {
    render_rgba(device, queue, W, H, Color::white(), draw)
}

fn polygon(points: &[[f32; 2]]) -> Path {
    let mut path = Path::new();
    path.move_to(points[0][0], points[0][1]);
    for point in &points[1..] {
        path.line_to(point[0], point[1]);
    }
    path.close();
    path
}

/// The area of a simple polygon inside the pixel at (`px`, `py`): the
/// polygon cut to the pixel's four sides.
fn area_inside(polygon: &[[f32; 2]], px: u32, py: u32) -> f32 {
    let mut points: Vec<[f64; 2]> = polygon.iter().map(|p| [f64::from(p[0]), f64::from(p[1])]).collect();
    let (x0, y0) = (f64::from(px), f64::from(py));
    for (axis, bound, below) in [(0, x0, false), (0, x0 + 1.0, true), (1, y0, false), (1, y0 + 1.0, true)] {
        let inside = |p: &[f64; 2]| if below { p[axis] <= bound } else { p[axis] >= bound };
        let mut out = Vec::with_capacity(points.len() + 4);
        for (index, current) in points.iter().enumerate() {
            let previous = &points[(index + points.len() - 1) % points.len()];
            let crossing = || {
                let t = (bound - previous[axis]) / (current[axis] - previous[axis]);
                [
                    previous[0] + t * (current[0] - previous[0]),
                    previous[1] + t * (current[1] - previous[1]),
                ]
            };
            match (inside(previous), inside(current)) {
                (true, true) => out.push(*current),
                (true, false) => out.push(crossing()),
                (false, true) => {
                    out.push(crossing());
                    out.push(*current);
                }
                (false, false) => {}
            }
        }
        points = out;
        if points.is_empty() {
            return 0.0;
        }
    }
    let twice: f64 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum();
    (twice.abs() / 2.0) as f32
}

const TRIANGLE: [[f32; 2]; 3] = [[48.3, 9.4], [86.7, 80.2], [11.6, 71.9]];
// An arrow: concave, with a notch in its tail.
const ARROW: [[f32; 2]; 7] = [
    [12.4, 40.2],
    [50.3, 40.2],
    [50.3, 22.6],
    [84.9, 49.1],
    [50.3, 76.3],
    [50.3, 58.7],
    [22.4, 49.3],
];
// A sliver a third of a pixel thick at its base and 60 long.
const SLIVER: [[f32; 2]; 3] = [[18.2, 30.3], [78.4, 50.8], [18.2, 30.63]];

/// A path clip covers each pixel by the area of the path inside it, to a
/// level and a half of 255 - in place, scaled and turned, down to a sliver
/// thinner than a pixel - where the stencil took a pixel whole or not at
/// all, by its center.
#[test]
fn a_path_clip_covers_each_pixel_by_the_area_inside_it() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let transforms = [
        ("in place", Transform2D::identity()),
        ("scaled", Transform2D::new(0.7, 0.0, 0.0, 1.21, 9.3, -11.7)),
        ("turned", {
            let mut turned = Transform2D::translation(-48.0, -48.0);
            turned.rotate(0.37);
            turned.translate(46.6, 49.2);
            turned
        }),
    ];
    let shapes: [(&str, &[[f32; 2]]); 3] = [("triangle", &TRIANGLE), ("arrow", &ARROW), ("sliver", &SLIVER)];
    for (placed, transform) in &transforms {
        for (name, points) in shapes {
            let frame = render(&device, &queue, |canvas| {
                canvas.set_transform(transform);
                canvas.clip_path(&polygon(points), FillRule::NonZero);
                canvas.reset_transform();
                fill_everything(canvas);
            });
            let placed_points: Vec<[f32; 2]> = points
                .iter()
                .map(|p| {
                    let (x, y) = transform.transform_point(p[0], p[1]);
                    [x, y]
                })
                .collect();
            let (mut worst, mut partial) = (0.0f32, 0);
            for y in 0..H {
                for x in 0..W {
                    let exact = area_inside(&placed_points, x, y);
                    partial += usize::from(exact > 0.02 && exact < 0.98);
                    worst = worst.max((covered(&frame, x, y) - exact).abs());
                }
            }
            assert!(partial > 40, "{name}, {placed}: only {partial} edge pixels");
            assert!(
                worst <= 1.5 / 255.0 + 1e-3,
                "{name}, {placed}: a pixel is {worst} from its share"
            );
        }
    }
}

/// Curves are flattened before they are rasterized, finer than a fill's: a
/// ring - two circles under even-odd, which is no box - stays within two
/// levels of each pixel's share inside it on average and eight at worst,
/// where the same ring as a fill is seventeen and fifty-four off.
#[test]
fn a_curved_path_clip_stays_close_to_the_area_inside_it() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let (cx, cy, outer, inner) = (48.3f32, 47.6f32, 38.4f32, 17.7f32);
    let frame = render(&device, &queue, |canvas| {
        let mut ring = Path::new();
        ring.circle(cx, cy, outer);
        ring.circle(cx, cy, inner);
        canvas.clip_path(&ring, FillRule::EvenOdd);
        fill_everything(canvas);
    });
    let (mut worst, mut sum, mut edges) = (0.0f32, 0.0f32, 0);
    for y in 0..H {
        for x in 0..W {
            let hits = (0..4096)
                .filter(|s| {
                    let sx = x as f32 + ((s % 64) as f32 + 0.5) / 64.0 - cx;
                    let sy = y as f32 + ((s / 64) as f32 + 0.5) / 64.0 - cy;
                    let r = sx.hypot(sy);
                    r <= outer && r >= inner
                })
                .count();
            let share = hits as f32 / 4096.0;
            if share > 0.0 && share < 1.0 {
                let error = (covered(&frame, x, y) - share).abs();
                worst = worst.max(error);
                sum += error;
                edges += 1;
            }
        }
    }
    assert!(edges > 300, "{edges} edge pixels");
    assert!(sum / edges as f32 <= 2.0 / 255.0, "mean error {}", sum / edges as f32);
    assert!(worst <= 8.0 / 255.0, "worst error {worst}");
    assert_eq!(px(&frame, 48, 48), WHITE, "the hole");
    assert_eq!(px(&frame, 48, 20), RED, "the ring");
}

/// Several paths clip to their union, each under its own rule: circles one
/// inside the other under even-odd fill the disc, where one path of both
/// has a hole; rects that share an edge inside a pixel leave no seam along
/// it; a child wound the other way takes nothing away; and a child fills
/// another's even-odd hole.
#[test]
fn clip_children_clip_to_their_union() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let circle = |r: f32| {
        let mut path = Path::new();
        path.circle(48.0, 48.0, r);
        path
    };
    let discs = render(&device, &queue, |canvas| {
        canvas.clip_paths(&[(&circle(40.0), FillRule::EvenOdd), (&circle(20.0), FillRule::EvenOdd)]);
        fill_everything(canvas);
    });
    assert_eq!(px(&discs, 48, 48), RED, "children: the inner disc is inside the union");
    assert_eq!(px(&discs, 48, 18), RED);
    assert_eq!(px(&discs, 4, 4), WHITE);
    let ring = render(&device, &queue, |canvas| {
        let mut both = circle(40.0);
        both.circle(48.0, 48.0, 20.0);
        canvas.clip_path(&both, FillRule::EvenOdd);
        fill_everything(canvas);
    });
    assert_eq!(px(&ring, 48, 48), WHITE, "one path: the rule runs across its contours");
    assert_eq!(px(&ring, 48, 18), RED);

    let rect = |x0: f32, x1: f32| polygon(&[[x0, 20.0], [x1, 20.0], [x1, 70.0], [x0, 70.0]]);
    let abutting = render(&device, &queue, |canvas| {
        canvas.clip_paths(&[
            (&rect(16.0, 47.4), FillRule::NonZero),
            (&rect(47.4, 80.0), FillRule::NonZero),
        ]);
        fill_everything(canvas);
    });
    for x in 16..80 {
        assert_eq!(px(&abutting, x, 40), RED, "no seam at x = {x}");
    }
    assert_eq!(px(&abutting, 12, 40), WHITE);

    let reversed = polygon(&[[30.0, 30.0], [30.0, 60.0], [60.0, 60.0], [60.0, 30.0]]);
    let opposite = render(&device, &queue, |canvas| {
        canvas.clip_paths(&[(&rect(16.0, 80.0), FillRule::NonZero), (&reversed, FillRule::NonZero)]);
        fill_everything(canvas);
    });
    assert_eq!(
        px(&opposite, 45, 45),
        RED,
        "a child wound the other way does not cut a hole"
    );

    let filled_hole = render(&device, &queue, |canvas| {
        let mut ring = circle(40.0);
        ring.circle(48.0, 48.0, 20.0);
        canvas.clip_paths(&[(&ring, FillRule::EvenOdd), (&circle(24.0), FillRule::NonZero)]);
        fill_everything(canvas);
    });
    assert_eq!(px(&filled_hole, 48, 48), RED);
    assert_eq!(
        px(&filled_hole, 48, 26),
        RED,
        "where the ring and the disc overlap: full, no more"
    );

    let none = render(&device, &queue, |canvas| {
        canvas.clip_paths(&[]);
        fill_everything(canvas);
    });
    assert!(
        none.chunks_exact(4).all(|p| p[..3] == WHITE),
        "no path: nothing is inside"
    );
}

/// Masks nest as the product of their coverages, a mask taken twice over
/// counts once, and a mask beside a clip shape cuts with it.
#[test]
fn masks_nest_and_meet_shapes() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let triangle = polygon(&TRIANGLE);
    let arrow = polygon(&ARROW);
    let nested = render(&device, &queue, |canvas| {
        canvas.clip_path(&triangle, FillRule::NonZero);
        canvas.clip_path(&arrow, FillRule::NonZero);
        fill_everything(canvas);
    });
    let (mut worst, mut both_partial) = (0.0f32, 0);
    for y in 0..H {
        for x in 0..W {
            let (a, b) = (area_inside(&TRIANGLE, x, y), area_inside(&ARROW, x, y));
            both_partial += usize::from(a > 0.0 && a < 1.0 && b > 0.0 && b < 1.0);
            worst = worst.max((covered(&nested, x, y) - a * b).abs());
        }
    }
    assert!(both_partial > 0, "the two edges cross somewhere");
    assert!(
        worst <= 2.5 / 255.0,
        "a pixel is {worst} from the product of the two shares"
    );

    let once = render(&device, &queue, |canvas| {
        canvas.clip_path(&triangle, FillRule::NonZero);
        fill_everything(canvas);
    });
    let twice = render(&device, &queue, |canvas| {
        canvas.clip_path(&triangle, FillRule::NonZero);
        canvas.save();
        canvas.clip_path(&triangle, FillRule::NonZero);
        fill_everything(canvas);
        canvas.restore();
    });
    assert!(once == twice, "a clip nested in its twin covers its edge once");

    let with_shape = render(&device, &queue, |canvas| {
        let mut circle = Path::new();
        circle.circle(48.0, 48.0, 30.0);
        canvas.clip_path(&circle, FillRule::NonZero);
        canvas.clip_path(&triangle, FillRule::NonZero);
        fill_everything(canvas);
    });
    assert_eq!(px(&with_shape, 48, 48), RED, "inside both");
    assert_eq!(
        px(&with_shape, 48, 14),
        WHITE,
        "inside the triangle, outside the circle"
    );
    assert_eq!(
        px(&with_shape, 22, 40),
        WHITE,
        "inside the circle, outside the triangle"
    );
    // On the circle's left edge, inside the triangle: the circle's coverage alone.
    let alone = render(&device, &queue, |canvas| {
        let mut circle = Path::new();
        circle.circle(48.0, 48.0, 30.0);
        canvas.clip_path(&circle, FillRule::NonZero);
        fill_everything(canvas);
    });
    let on_the_circle = px(&alone, 20, 60);
    assert!(on_the_circle != WHITE && on_the_circle != RED, "{on_the_circle:?}");
    assert_eq!(px(&with_shape, 20, 60), on_the_circle);
}

/// Every kind of draw is gated by a mask: convex and concave fills, strokes
/// through the stencil and without it, an image, and a layer's composite.
#[test]
fn a_mask_gates_every_kind_of_draw() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let clip = polygon(&TRIANGLE);
    let inside_and_out = |buf: &[u8], what: &str| {
        assert_eq!(px(buf, 48, 50), RED, "{what}: inside");
        for (x, y) in [(4, 4), (90, 6), (6, 90), (90, 90), (20, 30), (76, 30)] {
            assert_eq!(px(buf, x, y), WHITE, "{what}: ({x},{y}) is outside the triangle");
        }
    };
    let convex = render(&device, &queue, |canvas| {
        canvas.clip_path(&clip, FillRule::NonZero);
        fill_everything(canvas);
    });
    inside_and_out(&convex, "a convex fill");
    let concave = render(&device, &queue, |canvas| {
        canvas.clip_path(&clip, FillRule::NonZero);
        let mut frame = Path::new();
        frame.rect(-8.0, -8.0, 112.0, 112.0);
        frame.rect(-4.0, -4.0, 104.0, 104.0);
        canvas.fill_path(&frame, &red());
        fill_everything(canvas);
    });
    inside_and_out(&concave, "a concave fill");
    for stencil_strokes in [true, false] {
        let stroked = render(&device, &queue, |canvas| {
            canvas.clip_path(&clip, FillRule::NonZero);
            let mut line = Path::new();
            line.move_to(-10.0, 48.0);
            line.line_to(106.0, 48.0);
            let paint = red().with_line_width(120.0).with_stencil_strokes(stencil_strokes);
            canvas.stroke_path(&line, &paint);
        });
        inside_and_out(&stroked, "a stroke");
    }
    let image = render(&device, &queue, |canvas| {
        let source = canvas
            .create_image_empty(W as usize, H as usize, PixelFormat::Rgba8, ImageFlags::empty())
            .unwrap();
        canvas.set_render_target(RenderTarget::Image(source));
        canvas.clear_rect(0, 0, W, H, Color::rgb(255, 0, 0));
        canvas.set_render_target(RenderTarget::Screen);
        canvas.clip_path(&clip, FillRule::NonZero);
        let mut everything = Path::new();
        everything.rect(0.0, 0.0, W as f32, H as f32);
        let paint = Paint::image(source, 0.0, 0.0, W as f32, H as f32, 0.0, 1.0);
        canvas.fill_path(&everything, &paint);
    });
    inside_and_out(&image, "an image");
    let layer = render(&device, &queue, |canvas| {
        canvas.clip_path(&clip, FillRule::NonZero);
        assert!(canvas.begin_layer(&LayerEffects::new()));
        fill_everything(canvas);
        canvas.end_layer();
    });
    inside_and_out(&layer, "a layer's composite");
    assert!(layer == convex, "the layer's composite takes the mask as the fill does");
}

/// A mask belongs to the target it was taken on: it gates a layer's
/// composite and not its content, a mask taken inside a layer gates only
/// that, and one taken on an image does not gate the screen.
#[test]
fn a_mask_belongs_to_the_target_it_was_taken_on() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let clip = polygon(&TRIANGLE);
    let layered = render(&device, &queue, |canvas| {
        canvas.clip_path(&clip, FillRule::NonZero);
        assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
        fill_everything(canvas);
        canvas.end_layer();
    });
    assert_eq!(
        px(&layered, 48, 50),
        [255, 128, 128],
        "the layer, at half opacity, inside"
    );
    assert_eq!(px(&layered, 4, 4), WHITE, "its composite is clipped");

    let inside = render(&device, &queue, |canvas| {
        assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
        canvas.clip_path(&clip, FillRule::NonZero);
        fill_everything(canvas);
        canvas.end_layer();
        let mut corner = Path::new();
        corner.rect(0.0, 0.0, 8.0, 8.0);
        canvas.fill_path(&corner, &red());
    });
    assert_eq!(px(&inside, 48, 50), [255, 128, 128]);
    assert_eq!(px(&inside, 90, 90), WHITE, "the layer's content was clipped");
    assert_eq!(px(&inside, 4, 4), RED, "the clip went with the layer");

    let on_image = render(&device, &queue, |canvas| {
        let image = canvas
            .create_image_empty(W as usize, H as usize, PixelFormat::Rgba8, ImageFlags::empty())
            .unwrap();
        canvas.set_render_target(RenderTarget::Image(image));
        canvas.clip_path(&clip, FillRule::NonZero);
        canvas.set_render_target(RenderTarget::Screen);
        fill_everything(canvas);
    });
    assert_eq!(px(&on_image, 4, 4), RED, "the image's clip does not gate the screen");
}

/// A draw under a mask is scissored to the pixels the mask spans, its
/// stencil draws with it: a concave fill that spans the target leaves no
/// winding outside the mask for the next fill to take as its own, and the
/// scissor ends with the draw. On the screen and in a layer, whose image is
/// stored bottom up.
#[test]
fn a_fill_under_a_small_mask_leaves_the_rest_of_the_target_alone() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let scene = |canvas: &mut Canvas<WGPURenderer>| {
        let clip = polygon(&[[8.0, 10.0], [38.0, 12.0], [34.0, 30.0], [10.0, 28.0]]);
        let dented = polygon(&[[-8.0, -8.0], [104.0, -8.0], [104.0, 104.0], [-8.0, 104.0], [4.0, 48.0]]);
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
        assert_eq!(px(frame, 22, 20), red, "{on}: the fill, inside the mask");
        assert_eq!(px(frame, 22, 60), WHITE, "{on}: nothing of it below the mask");
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
/// cannot hold it to a clip: under a mask it takes each pixel whole or not
/// at all - the pixels the clip covers by half or more - and nothing
/// outside changes.
#[test]
fn an_operation_coverage_cannot_bound_takes_a_mask_whole_or_not_at_all() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let frame = render_rgba(&device, &queue, W, H, Color::rgb(0, 0, 255), |canvas| {
        canvas.clip_path(&polygon(&TRIANGLE), FillRule::NonZero);
        canvas.global_composite_operation(CompositeOperation::Copy);
        fill_everything(canvas);
    });
    let (mut whole, mut untouched) = (0, 0);
    for y in 0..H {
        for x in 0..W {
            let share = area_inside(&TRIANGLE, x, y);
            let pixel = px(&frame, x, y);
            assert!(
                pixel == RED || pixel == [0, 0, 255],
                "({x}, {y}) is {pixel:?}: neither the fill nor what was there"
            );
            if share > 0.55 {
                assert_eq!(pixel, RED, "({x}, {y}), {share} inside");
                whole += 1;
            } else if share < 0.45 {
                assert_eq!(pixel, [0, 0, 255], "({x}, {y}), {share} inside");
                untouched += 1;
            }
        }
    }
    assert!(whole > 1000 && untouched > 1000);
}

/// A clip that comes again a whole number of pixels away reads the same
/// mask: both places get the same pixels. With no budget for masks a path
/// clips on the stencil, as before.
#[test]
fn a_mask_is_found_again_and_needs_a_budget() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let small: Vec<[f32; 2]> = TRIANGLE.iter().map(|p| [p[0] * 0.4, p[1] * 0.4]).collect();
    let frame = render(&device, &queue, |canvas| {
        for (dx, dy) in [(0.0, 0.0), (47.0, 9.0), (21.0, 52.0)] {
            canvas.save();
            canvas.translate(dx, dy);
            canvas.clip_path(&polygon(&small), FillRule::NonZero);
            canvas.reset_transform();
            fill_everything(canvas);
            canvas.restore();
        }
    });
    let mut edges = 0;
    for y in 0..40 {
        for x in 0..40 {
            let first = px(&frame, x, y);
            edges += usize::from(first != WHITE && first != RED);
            assert_eq!(first, px(&frame, x + 47, y + 9), "({x}, {y}) and the second place");
            assert_eq!(first, px(&frame, x + 21, y + 52), "({x}, {y}) and the third place");
        }
    }
    assert!(edges > 40, "{edges} edge pixels");

    let on_the_stencil = render(&device, &queue, |canvas| {
        canvas.set_clip_mask_budget(0);
        canvas.clip_path(&polygon(&TRIANGLE), FillRule::NonZero);
        fill_everything(canvas);
    });
    assert!(
        on_the_stencil.chunks_exact(4).all(|p| p[..3] == WHITE || p[..3] == RED),
        "no budget: the stencil's edge, a pixel whole or not at all"
    );
    assert_eq!(px(&on_the_stencil, 48, 50), RED);
    let too_small = render(&device, &queue, |canvas| {
        canvas.set_clip_mask_budget(64);
        canvas.clip_path(&polygon(&TRIANGLE), FillRule::NonZero);
        fill_everything(canvas);
    });
    assert!(too_small == on_the_stencil, "a budget the mask does not fit");
}
