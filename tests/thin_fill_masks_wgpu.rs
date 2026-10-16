//! Headless GPU tests for fills thinner than a pixel, drawn through coverage
//! masks (femtovg/femtovg#327): a sliver's pixels hold its area, where the
//! fringes of its two edges each add their own ramp; a thin ring holds the
//! ring's; the scissor and the clips bound the mask's quad as they bound any
//! draw, and the paint is the fill's own. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, FillRule, Paint, Path, Solidity};

mod common;
use common::{headless_device, render_frames, render_rgba};

const W: u32 = 96;
const H: u32 = 96;

fn red() -> Paint {
    Paint::color(Color::rgb(255, 0, 0))
}

/// Red over white: the pixels' worth of red, from what green lost.
fn ink(buf: &[u8]) -> f32 {
    buf.chunks_exact(4).map(|p| 1.0 - f32::from(p[1]) / 255.0).sum()
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    masks: bool,
    draw: impl FnOnce(&mut Canvas<WGPURenderer>),
) -> Vec<u8> {
    render_rgba(device, queue, W, H, Color::white(), |canvas| {
        canvas.set_thin_fill_masks(masks);
        draw(canvas);
    })
}

fn render_frames_sized(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    frames: usize,
    mut draw: impl FnMut(&mut Canvas<WGPURenderer>),
) -> Vec<Vec<u8>> {
    render_frames(device, queue, width, height, Color::white(), frames, |canvas, _| {
        draw(canvas)
    })
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

/// A strip `width` across from (18, 20) to (78, 76), and its area.
fn sliver(width: f32) -> (Path, f32) {
    let ([x0, y0], [x1, y1]) = ([18.0f32, 20.0], [78.0f32, 76.0]);
    let length = (x1 - x0).hypot(y1 - y0);
    let [nx, ny] = [-(y1 - y0) / length * width / 2.0, (x1 - x0) / length * width / 2.0];
    let path = polygon(&[
        [x0 + nx, y0 + ny],
        [x1 + nx, y1 + ny],
        [x1 - nx, y1 - ny],
        [x0 - nx, y0 - ny],
    ]);
    (path, width * length)
}

#[test]
fn a_sliver_holds_its_area_where_its_fringes_overink_it() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for width in [0.1, 0.25, 0.5, 0.9] {
        let (path, area) = sliver(width);
        let masked = ink(&render(&device, &queue, true, |canvas| canvas.fill_path(&path, &red())));
        let fringed = ink(&render(&device, &queue, false, |canvas| {
            canvas.fill_path(&path, &red())
        }));
        assert!(
            (masked - area).abs() < 0.02 * area + 0.5,
            "a sliver {width} px wide: {masked:.1} px of ink, its area {area:.1}"
        );
        // Each edge's fringe ramps over a pixel of its own: 63 px of ink for
        // 8 at a tenth of a pixel, a little over the area by 0.9.
        if width <= 0.5 {
            assert!(
                fringed > 1.25 * area,
                "the fringes over-ink it: {fringed:.1} for {area:.1}"
            );
        }
    }
}

/// A ring 0.4 px wide holds the ring's area: its hole wound against the
/// outer circle under non-zero, or both circles wound alike under even-odd,
/// where the hole's area adds to the outer one's if signs alone decide.
#[test]
fn a_thin_ring_holds_its_area() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let area = std::f32::consts::PI * (30.0f32.powi(2) - 29.6f32.powi(2));
    let mut wound_against = Path::new();
    wound_against.circle(48.0, 48.0, 30.0);
    wound_against.circle(48.0, 48.0, 29.6);
    wound_against.solidity(Solidity::Hole);
    let mut wound_alike = Path::new();
    wound_alike.circle(48.0, 48.0, 30.0);
    wound_alike.circle(48.0, 48.0, 29.6);
    for (ring, rule) in [(wound_against, FillRule::NonZero), (wound_alike, FillRule::EvenOdd)] {
        let paint = red().with_fill_rule(rule);
        let masked = ink(&render(&device, &queue, true, |canvas| canvas.fill_path(&ring, &paint)));
        assert!(
            (masked - area).abs() < 0.03 * area,
            "{rule:?}: {masked:.1} px of ink, the ring's area {area:.1}"
        );
    }
}

/// Masks made in a later frame are uploaded around the ones the atlas keeps:
/// a frame that draws a sliver kept from the frame before and three made in
/// it draws as a frame that makes all four does.
#[test]
fn masks_made_later_leave_the_kept_ones_whole() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (strip, _) = sliver(0.3);
    let bars: Vec<Path> = [(4.0, 88.3, 80.0), (86.2, 4.0, 0.4), (30.0, 6.6, 50.0)]
        .iter()
        .map(|&(x, y, long)| {
            let mut bar = Path::new();
            if long < 1.0 {
                bar.rect(x, y, long, 80.0);
            } else {
                bar.rect(x, y, long, 0.4);
            }
            bar
        })
        .collect();
    let frames = render_frames(&device, &queue, W, H, Color::white(), 2, |canvas, frame| {
        canvas.fill_path(&strip, &red());
        if frame == 1 {
            for bar in &bars {
                canvas.fill_path(bar, &red());
            }
        }
    });
    let at_once = render(&device, &queue, true, |canvas| {
        canvas.fill_path(&strip, &red());
        for bar in &bars {
            canvas.fill_path(bar, &red());
        }
    });
    assert!(
        frames[1] == at_once,
        "the masks kept and made later draw as made at once"
    );
    assert!(ink(&at_once) > ink(&frames[0]) + 0.4 * (80.0 + 80.0 + 50.0) - 2.0);
}

/// An atlas that grows in a frame keeps its masks: a sliver whose mask was
/// made before a bar too wide for the first atlas holds its area, as does the
/// bar, and the next frame, every mask found again, draws the same.
#[test]
fn an_atlas_that_grows_keeps_its_masks() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (strip, area) = sliver(0.3);
    let mut bar = Path::new();
    // Past the first atlas's 512 pixels, through a target wide enough.
    bar.rect(4.0, 90.3, 600.0, 0.4);
    let frames = render_frames_sized(&device, &queue, 640, H, 2, |canvas| {
        canvas.fill_path(&strip, &red());
        canvas.fill_path(&bar, &red());
    });
    let bar_ink = |frame: &[u8]| ink(&frame[(88 * 640 * 4) as usize..]);
    let strip_ink = |frame: &[u8]| ink(&frame[..(88 * 640 * 4) as usize]);
    assert!(
        (strip_ink(&frames[0]) - area).abs() < 0.02 * area + 0.5,
        "the sliver: {:.1} for {area:.1}",
        strip_ink(&frames[0])
    );
    assert!(
        (bar_ink(&frames[0]) - 240.0).abs() < 0.02 * 240.0,
        "the bar: {:.1} for 240",
        bar_ink(&frames[0])
    );
    assert!(frames[0] == frames[1], "the next frame draws the same");
}

/// The scissor and a clip to a path bound the mask's quad: the sliver's
/// ink outside them is none.
#[test]
fn the_scissor_and_clips_bound_a_thin_fill() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (path, area) = sliver(0.3);
    let scissored = render(&device, &queue, true, |canvas| {
        canvas.scissor(0.0, 0.0, W as f32, 48.0);
        canvas.fill_path(&path, &red());
    });
    let below: f32 = ink(&scissored[(48 * W * 4) as usize..]);
    assert!(below < 0.01, "{below:.2} px of ink below the scissor");
    let above = ink(&scissored);
    assert!(
        (above - area * 0.5).abs() < 0.1 * area,
        "about half the sliver is above the scissor: {above:.1} of {area:.1}"
    );

    let clipped = render(&device, &queue, true, |canvas| {
        // A triangle clip: no box, so a mask.
        canvas.clip_path(&polygon(&[[0.0, 0.0], [96.0, 0.0], [0.0, 96.0]]), FillRule::NonZero);
        canvas.fill_path(&path, &red());
    });
    for y in 0..H {
        for x in 0..W {
            if x + y > 98 {
                let i = ((y * W + x) * 4 + 1) as usize;
                assert_eq!(clipped[i], 255, "outside the clip at ({x}, {y})");
            }
        }
    }
    assert!(ink(&clipped) > 0.2 * area, "inside the clip the sliver draws");
}

/// The quad takes the fill's paint: a gradient along a thin bar runs from
/// red at one end to blue at the other.
#[test]
fn a_thin_fill_takes_its_paint() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let buf = render(&device, &queue, true, |canvas| {
        let mut bar = Path::new();
        bar.rect(8.0, 40.0, 80.0, 0.5);
        let paint = Paint::linear_gradient(8.0, 0.0, 88.0, 0.0, Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
        canvas.fill_path(&bar, &paint);
    });
    let at = |x: u32| {
        let i = ((40 * W + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2]]
    };
    let (left, right) = (at(10), at(85));
    // Half covered over white: the far channel stays near 255, the other drops.
    assert!(left[0] > 240 && left[2] < 160, "red at the left end: {left:?}");
    assert!(right[2] > 240 && right[0] < 160, "blue at the right end: {right:?}");
}

/// A mask taller than it is wide, kept transposed, draws as the same fill
/// turned on its side: the tall sliver's pixels are the wide one's mirrored
/// across the diagonal, every one.
#[test]
fn a_tall_sliver_draws_as_a_wide_one_turned_on_its_side() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let tall = polygon(&[[20.0, 4.0], [30.0, 90.0], [30.3, 90.0], [20.3, 4.0]]);
    let wide = polygon(&[[4.0, 20.0], [90.0, 30.0], [90.0, 30.3], [4.0, 20.3]]);
    let tall = render(&device, &queue, true, |canvas| canvas.fill_path(&tall, &red()));
    let wide = render(&device, &queue, true, |canvas| canvas.fill_path(&wide, &red()));
    let pixel = |buf: &[u8], x: u32, y: u32| buf[((y * W + x) * 4) as usize..][..4].to_vec();
    for y in 0..H {
        for x in 0..W {
            assert_eq!(pixel(&tall, x, y), pixel(&wide, y, x), "({x}, {y})");
        }
    }
    assert!(ink(&tall) > 20.0, "the sliver is drawn");
}
