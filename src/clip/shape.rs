//! Clips the fragment shader can evaluate: a path that outlines a
//! parallelogram, a rounded rectangle or an ellipse is a box with elliptical
//! corners in a frame of its own, and its coverage is a few lines of
//! arithmetic per fragment - no stencil work, and an antialiased edge.

use crate::{geometry::Transform2D, Path, Verb};

/// How far an outline may stray from the box it is taken for, as a share of
/// the corner radius (the box's smaller half-size on a straight side). A
/// circle of four cubics is 0.03 % off the true one and one of eight
/// quadratics, as tiny-skia builds it, 0.31 %; a share keeps the answer the
/// same at every zoom.
const FIT_TOLERANCE: f32 = 0.005;

/// How far a polygon's corners may be from a parallelogram's, as a share of
/// its size: rounding in the coordinates, no more - straight sides are not
/// approximations of anything.
const CORNER_TOLERANCE: f32 = 1e-4;

/// The cubic handle length of a quarter circle of radius one.
const KAPPA90: f32 = 0.552_284_8;

/// Interior points sampled on each curve when checking a fit.
const CURVE_SAMPLES: usize = 7;

/// Points taken along each corner when asking whether a box lies in another.
const CORNER_SAMPLES: usize = 9;

/// How far, in fringe widths, the one box nested clips are combined into
/// may stray from what they cover together: half a pixel, no further than
/// the stencil the pair would otherwise go to resolves an edge.
const CONTAINMENT_SLACK: f32 = 0.5;

/// The corner radius, in fringe widths, under which a corner is taken as
/// square: a distance no longer stands in for coverage that tight (it is
/// off by up to a fifth of a pixel, the square corner by a sixteenth).
/// Skia's analytic clips square off at the same radius (`kRadiusMin`).
const SQUARE_CORNER_BELOW: f32 = 0.5;

/// How close to parallel a frame's axes may come, as the squared sine of the
/// angle between them: under half a degree apart the box is a sliver whose
/// inverse map loses the precision the shader needs, and goes to the stencil.
const MIN_AXIS_SPREAD: f32 = 1e-4;

/// The half extent, in fringe widths, under which a box goes to the stencil:
/// thinner than a pixel, its two sides no longer cover a pixel one at a time.
const MIN_HALF_EXTENT: f32 = 0.5;

/// Where the uniform rows put the corners' centers of a box with square
/// corners: so far out that no fragment is past them, yet inside the range
/// GLSL ES guarantees a highp float (2^62).
const NO_CORNERS: f32 = 1e18;

/// A box with elliptical corners: zero radii make a rectangle, radii equal
/// to the extent an ellipse.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct RoundedBox {
    /// Maps the box - centered on the origin, sides along the axes - to the
    /// space its outline was given in.
    pub(crate) frame: Transform2D,
    pub(crate) extent: [f32; 2],
    pub(crate) radii: [f32; 2],
}

/// A [`RoundedBox`] as a draw's fragment shader takes it: the map from
/// device pixels to the box's frame scaled so that one unit is one fringe
/// width across each side, and the box in those units - its corners square
/// when they are too tight to tell from square ([`SQUARE_CORNER_BELOW`]).
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub(crate) struct ClipCoverage {
    /// Rows of the linear part.
    pub(crate) linear: [f32; 4],
    pub(crate) offset: [f32; 2],
    pub(crate) extent: [f32; 2],
    pub(crate) radii: [f32; 2],
}

enum Segment {
    Line([f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
}

impl RoundedBox {
    /// The box `path` outlines, if it is one: a single contour that goes
    /// once around a parallelogram, or around an axis-aligned rounded
    /// rectangle or ellipse within [`FIT_TOLERANCE`].
    pub(crate) fn fit(path: &Path) -> Option<Self> {
        let (start, segments) = contour(path)?;
        if segments.iter().all(|segment| matches!(segment, Segment::Line(_))) {
            return Self::parallelogram(start, &segments);
        }
        Self::rounded(start, &segments)
    }

    fn parallelogram(start: [f32; 2], segments: &[Segment]) -> Option<Self> {
        let mut points = vec![start];
        for segment in segments {
            if let Segment::Line(end) = segment {
                points.push(*end);
            }
        }
        points.pop(); // the contour is closed: the last point is the start again
        let size = points
            .iter()
            .map(|p| (p[0] - start[0]).abs().max((p[1] - start[1]).abs()))
            .fold(0.0, f32::max);
        // Corners only: a point on the line through its neighbours adds nothing.
        let turns = |points: &[[f32; 2]]| -> Vec<[f32; 2]> {
            let n = points.len();
            (0..n)
                .filter(|&i| {
                    let (a, b, c) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
                    let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
                    cross.abs() > CORNER_TOLERANCE * size * size
                })
                .map(|i| points[i])
                .collect()
        };
        let &[p0, p1, p2, p3] = turns(&points).as_slice() else {
            return None;
        };
        let close = |a: f32, b: f32| (a - b).abs() <= CORNER_TOLERANCE * size;
        if !close(p0[0] + p2[0], p1[0] + p3[0]) || !close(p0[1] + p2[1], p1[1] + p3[1]) {
            return None;
        }
        let u = [(p1[0] - p0[0]) * 0.5, (p1[1] - p0[1]) * 0.5];
        let v = [(p3[0] - p0[0]) * 0.5, (p3[1] - p0[1]) * 0.5];
        let extent = [u[0].hypot(u[1]), v[0].hypot(v[1])];
        Some(Self {
            frame: Transform2D::new(
                u[0] / extent[0],
                u[1] / extent[0],
                v[0] / extent[1],
                v[1] / extent[1],
                (p0[0] + p2[0]) * 0.5,
                (p0[1] + p2[1]) * 0.5,
            ),
            extent,
            radii: [0.0, 0.0],
        })
    }

    fn rounded(start: [f32; 2], segments: &[Segment]) -> Option<Self> {
        // The outline in order - each segment's interior samples, then its
        // end - and the segment ends alone, where straight sides meet corners.
        let mut outline = vec![start];
        let mut anchors = vec![start];
        let mut from = start;
        for segment in segments {
            match *segment {
                Segment::Line(end) => {
                    outline.push([(from[0] + end[0]) * 0.5, (from[1] + end[1]) * 0.5]);
                    outline.push(end);
                    from = end;
                }
                Segment::Cubic(c1, c2, end) => {
                    for i in 1..=CURVE_SAMPLES {
                        let t = i as f32 / (CURVE_SAMPLES + 1) as f32;
                        let s = 1.0 - t;
                        let at = |k: usize| {
                            s * s * s * from[k] + 3.0 * s * s * t * c1[k] + 3.0 * s * t * t * c2[k] + t * t * t * end[k]
                        };
                        outline.push([at(0), at(1)]);
                    }
                    outline.push(end);
                    from = end;
                }
            }
            anchors.push(from);
        }

        let bound = |k: usize, pick: fn(f32, f32) -> f32, seed: f32| outline.iter().map(|p| p[k]).fold(seed, pick);
        let (min, max) = (
            [bound(0, f32::min, f32::INFINITY), bound(1, f32::min, f32::INFINITY)],
            [
                bound(0, f32::max, f32::NEG_INFINITY),
                bound(1, f32::max, f32::NEG_INFINITY),
            ],
        );
        let center = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];
        let extent = [(max[0] - min[0]) * 0.5, (max[1] - min[1]) * 0.5];
        let sized = extent.iter().all(|half| *half > 0.0 && half.is_finite());
        if !sized {
            return None;
        }
        let slack = FIT_TOLERANCE * extent[0].min(extent[1]);

        // A corner's radius along an axis is what the straight side on the
        // other axis leaves of the extent: the side ends where its farthest
        // anchor is. No anchor on a side means no straight side - an ellipse.
        let radius = |along: usize| {
            let across = 1 - along;
            anchors
                .iter()
                .filter(|p| ((p[across] - center[across]).abs() - extent[across]).abs() <= slack)
                .map(|p| (p[along] - center[along]).abs())
                .reduce(f32::max)
                .map_or(extent[along], |reach| extent[along] - reach)
        };
        let mut radii = [radius(0), radius(1)].map(|r| r.max(0.0));
        for (radius, half) in radii.iter_mut().zip(extent) {
            if half - *radius <= slack {
                *radius = half;
            }
        }
        if radii[0] <= slack && radii[1] <= slack {
            radii = [0.0, 0.0];
        } else if radii[0] <= slack || radii[1] <= slack {
            return None;
        }

        let on_box = outline.iter().all(|p| {
            let side = [
                (p[0] - center[0]).abs() - extent[0],
                (p[1] - center[1]).abs() - extent[1],
            ];
            let corner = [side[0] + radii[0], side[1] + radii[1]];
            if radii[0] > 0.0 && corner[0] > 0.0 && corner[1] > 0.0 {
                ((corner[0] / radii[0]).hypot(corner[1] / radii[1]) - 1.0).abs() <= FIT_TOLERANCE
            } else {
                side[0].max(side[1]).abs() <= slack
            }
        });
        // Once around, never turning back: twice around is another region
        // under even-odd, and a doubled-back outline is not the box's.
        let angle = |p: &[f32; 2]| ((p[1] - center[1]) / extent[1]).atan2((p[0] - center[0]) / extent[0]);
        let mut turned = 0.0;
        let mut forward = 0usize;
        let mut backward = 0usize;
        for pair in outline.windows(2) {
            let mut step = angle(&pair[1]) - angle(&pair[0]);
            if step > std::f32::consts::PI {
                step -= std::f32::consts::TAU;
            } else if step < -std::f32::consts::PI {
                step += std::f32::consts::TAU;
            }
            turned += step;
            forward += usize::from(step > 1e-4);
            backward += usize::from(step < -1e-4);
        }
        let once = (turned.abs() - std::f32::consts::TAU).abs() < 0.01 && (forward == 0 || backward == 0);
        (on_box && once).then_some(Self {
            frame: Transform2D::translation(center[0], center[1]),
            extent,
            radii,
        })
    }

    /// The box under `transform`: its frame carried along.
    pub(crate) fn transformed(self, transform: &Transform2D) -> Self {
        Self {
            frame: self.frame * *transform,
            ..self
        }
    }

    /// The box for a fragment shader whose positions are in the space the
    /// frame maps to; `None` when the frame collapses that space or all but
    /// does ([`MIN_AXIS_SPREAD`]), or the box is thinner than a pixel there.
    pub(crate) fn coverage(&self, fringe_width: f32) -> Option<ClipCoverage> {
        let Transform2D([a, b, c, d, ..]) = self.frame;
        let determinant = a * d - c * b;
        let spans = (a * a + b * b) * (c * c + d * d);
        let invertible = determinant * determinant > MIN_AXIS_SPREAD * spans && spans.is_finite();
        if !invertible || fringe_width <= 0.0 {
            return None;
        }
        // Each row of the inverse is the gradient of one box coordinate, so
        // dividing by its length makes that coordinate count device pixels
        // across the side it measures - under a skew too.
        let Transform2D([ia, ib, ic, id, ix, iy]) = self.frame.inverse();
        let per_unit = [ia.hypot(ic) * fringe_width, ib.hypot(id) * fringe_width];
        let extent = [self.extent[0] / per_unit[0], self.extent[1] / per_unit[1]];
        if extent[0] < MIN_HALF_EXTENT || extent[1] < MIN_HALF_EXTENT {
            return None;
        }
        let mut radii = [self.radii[0] / per_unit[0], self.radii[1] / per_unit[1]];
        if radii[0] < SQUARE_CORNER_BELOW || radii[1] < SQUARE_CORNER_BELOW {
            radii = [0.0, 0.0];
        }
        Some(ClipCoverage {
            linear: [ia / per_unit[0], ic / per_unit[0], ib / per_unit[1], id / per_unit[1]],
            offset: [ix / per_unit[0], iy / per_unit[1]],
            extent,
            radii,
        })
    }

    /// What both this box and `other` cover, as one box, when their sides
    /// are parallel: exactly for two parallelograms; with round corners, the
    /// box with the nearer of each pair of sides and the corners of one of
    /// the two, when its outline stays within [`CONTAINMENT_SLACK`] of the
    /// real intersection's - one box inside the other, or two that all but
    /// coincide. `None` for any other overlap, or none.
    pub(crate) fn intersection(&self, other: &Self, fringe_width: f32) -> Option<Self> {
        // `other` in this box's frame: its center, and its half sides and
        // radii, which must each run along one of this frame's axes.
        let to_frame = other.frame / self.frame;
        let Transform2D([a, b, c, d, x, y]) = to_frame;
        let scale = a.abs().max(b.abs()).max(c.abs()).max(d.abs());
        let aligned = |along: f32, across: f32| across.abs() <= 1e-4 * scale && along.abs() > 0.0;
        let (half, radii) = if aligned(a, b) && aligned(d, c) {
            (
                [a.abs() * other.extent[0], d.abs() * other.extent[1]],
                [a.abs() * other.radii[0], d.abs() * other.radii[1]],
            )
        } else if aligned(b, a) && aligned(c, d) {
            (
                [c.abs() * other.extent[1], b.abs() * other.extent[0]],
                [c.abs() * other.radii[1], b.abs() * other.radii[0]],
            )
        } else {
            return None;
        };
        let low = [(x - half[0]).max(-self.extent[0]), (y - half[1]).max(-self.extent[1])];
        let high = [(x + half[0]).min(self.extent[0]), (y + half[1]).min(self.extent[1])];
        if low[0] >= high[0] || low[1] >= high[1] {
            return None;
        }
        let center = Transform2D::translation((low[0] + high[0]) * 0.5, (low[1] + high[1]) * 0.5);
        let extent = [(high[0] - low[0]) * 0.5, (high[1] - low[1]) * 0.5];
        let with_corners = |radii: [f32; 2]| Self {
            frame: center * self.frame,
            extent,
            radii: [radii[0].min(extent[0]), radii[1].min(extent[1])],
        };
        if self.radii == [0.0, 0.0] && other.radii == [0.0, 0.0] {
            return Some(with_corners([0.0, 0.0]));
        }
        // On the real intersection's outline the further of the two boxes'
        // edges is the one a point is on.
        let (mine, theirs) = (self.coverage(fringe_width)?, other.coverage(fringe_width)?);
        [radii, self.radii].into_iter().map(with_corners).find(|both| {
            both.outline()
                .all(|point| mine.distance(point).max(theirs.distance(point)).abs() <= CONTAINMENT_SLACK)
        })
    }

    /// The box as a path in its own frame, for the stencil.
    pub(crate) fn path(&self) -> Path {
        let ([ex, ey], [rx, ry]) = (self.extent, self.radii);
        let mut path = Path::new();
        if rx <= 0.0 {
            path.rect(-ex, -ey, 2.0 * ex, 2.0 * ey);
            return path;
        }
        // A quarter ellipse as one cubic: handles this far short of the corner.
        let (hx, hy) = (rx * (1.0 - KAPPA90), ry * (1.0 - KAPPA90));
        path.move_to(-ex + rx, -ey);
        path.line_to(ex - rx, -ey);
        path.bezier_to(ex - hx, -ey, ex, -ey + hy, ex, -ey + ry);
        path.line_to(ex, ey - ry);
        path.bezier_to(ex, ey - hy, ex - hx, ey, ex - rx, ey);
        path.line_to(-ex + rx, ey);
        path.bezier_to(-ex + hx, ey, -ex, ey - hy, -ex, ey - ry);
        path.line_to(-ex, -ey + ry);
        path.bezier_to(-ex, -ey + hy, -ex + hx, -ey, -ex + rx, -ey);
        path.close();
        path
    }

    /// The box's corners in the space its frame maps to.
    #[cfg(test)]
    fn corners(&self) -> [[f32; 2]; 4] {
        [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].map(|[x, y]| {
            let (x, y) = self.frame.transform_point(x * self.extent[0], y * self.extent[1]);
            [x, y]
        })
    }

    /// Points along the box's outline in the space its frame maps to: each
    /// corner's arc from one side to the next, which for a sharp corner is
    /// the corner itself.
    fn outline(&self) -> impl Iterator<Item = [f32; 2]> + '_ {
        [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
            .into_iter()
            .flat_map(move |[sx, sy]: [f32; 2]| {
                (0..CORNER_SAMPLES).map(move |i| {
                    let angle = i as f32 / (CORNER_SAMPLES - 1) as f32 * std::f32::consts::FRAC_PI_2;
                    let x = self.extent[0] - self.radii[0] * (1.0 - angle.cos());
                    let y = self.extent[1] - self.radii[1] * (1.0 - angle.sin());
                    let (x, y) = self.frame.transform_point(x * sx, y * sy);
                    [x, y]
                })
            })
    }
}

impl ClipCoverage {
    /// The ten floats both backends' shaders read (`clipMask`): where the
    /// corners' ellipses are centered, the linear part by rows, the offset,
    /// and the half extents grown by the half fringe a side's ramp reaches.
    pub(crate) fn uniform_rows(&self) -> [f32; 10] {
        let inner = if self.radii[0] > 0.0 {
            [self.extent[0] - self.radii[0], self.extent[1] - self.radii[1]]
        } else {
            [NO_CORNERS; 2]
        };
        let [a, b, c, d] = self.linear;
        let outer = [self.extent[0] + 0.5, self.extent[1] + 0.5];
        [
            inner[0],
            inner[1],
            a,
            b,
            c,
            d,
            self.offset[0],
            self.offset[1],
            outer[0],
            outer[1],
        ]
    }

    /// Signed distance from the box's edge at a device position, in fringe
    /// widths and negative inside: what the fragment shaders' coverage is
    /// half a fringe minus.
    pub(crate) fn distance(&self, [x, y]: [f32; 2]) -> f32 {
        let u = [
            self.linear[0] * x + self.linear[1] * y + self.offset[0],
            self.linear[2] * x + self.linear[3] * y + self.offset[1],
        ];
        let side = [u[0].abs() - self.extent[0], u[1].abs() - self.extent[1]];
        let corner = [side[0] + self.radii[0], side[1] + self.radii[1]];
        if self.radii[0] > 0.0 && corner[0] > 0.0 && corner[1] > 0.0 {
            // k1 is one on the corner's ellipse; its gradient in device
            // pixels is g along the frame's rows, which are unit vectors but
            // stand at a right angle only without a skew.
            let k = [corner[0] / self.radii[0], corner[1] / self.radii[1]];
            let k1 = k[0].hypot(k[1]);
            let g = [k[0] / self.radii[0], k[1] / self.radii[1]];
            let lean =
                (self.linear[0] * self.linear[2] + self.linear[1] * self.linear[3]) * u[0].signum() * u[1].signum();
            (k1 - 1.0) * k1 / (g[0] * g[0] + g[1] * g[1] + 2.0 * g[0] * g[1] * lean).sqrt()
        } else {
            side[0].max(side[1])
        }
    }

    /// Whether `other` lies inside this box, within [`CONTAINMENT_SLACK`],
    /// so that clipping to both is clipping to `other`.
    pub(crate) fn contains(&self, other: &RoundedBox) -> bool {
        other.outline().all(|point| self.distance(point) <= CONTAINMENT_SLACK)
    }
}

/// The path's single contour as its start and its segments, closed; `None`
/// for an empty path or one with a second contour.
fn contour(path: &Path) -> Option<([f32; 2], Vec<Segment>)> {
    let mut start: Option<[f32; 2]> = None;
    let mut last = [0.0, 0.0];
    let mut segments = Vec::new();
    let mut closed = false;
    for verb in path.verbs() {
        match verb {
            Verb::MoveTo(x, y) => {
                if !segments.is_empty() {
                    return None;
                }
                start = Some([x, y]);
                last = [x, y];
            }
            Verb::LineTo(x, y) => {
                if closed || start.is_none() {
                    return None;
                }
                if [x, y] != last {
                    segments.push(Segment::Line([x, y]));
                    last = [x, y];
                }
            }
            Verb::BezierTo(c1x, c1y, c2x, c2y, x, y) => {
                if closed || start.is_none() {
                    return None;
                }
                segments.push(Segment::Cubic([c1x, c1y], [c2x, c2y], [x, y]));
                last = [x, y];
            }
            Verb::Close => closed = true,
            Verb::Solid | Verb::Hole => {}
        }
    }
    let start = start?;
    if last != start {
        segments.push(Segment::Line(start));
    }
    (!segments.is_empty()).then_some((start, segments))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: [f32; 2], b: [f32; 2]) {
        assert!(
            (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3,
            "{a:?} vs {b:?}"
        );
    }

    /// An ellipse as tiny-skia builds one: eight quadratics, 0.31 % outside
    /// the true curve at their middles.
    fn quad_ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Path {
        let mut path = Path::new();
        let point = |i: usize, scale: f32| {
            let a = i as f32 * std::f32::consts::FRAC_PI_4;
            (cx + rx * scale * a.cos(), cy + ry * scale * a.sin())
        };
        let (x, y) = point(0, 1.0);
        path.move_to(x, y);
        for i in 0..8 {
            let a = (i as f32 + 0.5) * std::f32::consts::FRAC_PI_4;
            let reach = 1.0 / std::f32::consts::FRAC_PI_8.cos();
            let (x, y) = point(i + 1, 1.0);
            path.quad_to(cx + rx * reach * a.cos(), cy + ry * reach * a.sin(), x, y);
        }
        path.close();
        path
    }

    #[test]
    fn a_rect_and_any_parallelogram_fit_with_their_own_frame() {
        let mut rect = Path::new();
        rect.rect(10.0, 20.0, 30.0, 40.0);
        let fit = RoundedBox::fit(&rect).unwrap();
        assert_eq!(
            (fit.extent, fit.radii),
            ([20.0, 15.0], [0.0, 0.0]),
            "sides in path order"
        );
        assert_close(
            [
                fit.frame.transform_point(0.0, 0.0).0,
                fit.frame.transform_point(0.0, 0.0).1,
            ],
            [25.0, 40.0],
        );

        // Open, with a collinear point on a side, and skewed: still one.
        let mut skewed = Path::new();
        skewed.move_to(0.0, 0.0);
        skewed.line_to(5.0, 0.0);
        skewed.line_to(10.0, 0.0);
        skewed.line_to(14.0, 8.0);
        skewed.line_to(4.0, 8.0);
        let fit = RoundedBox::fit(&skewed).unwrap();
        assert_close(fit.extent, [5.0, 20.0_f32.sqrt()]);
        let mut corners = fit.corners().to_vec();
        corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut expected = vec![[0.0, 0.0], [10.0, 0.0], [14.0, 8.0], [4.0, 8.0]];
        expected.sort_by(|a: &[f32; 2], b| a.partial_cmp(b).unwrap());
        for (corner, expected) in corners.iter().zip(expected) {
            assert_close(*corner, expected);
        }
    }

    #[test]
    fn ellipses_and_rounded_rects_fit_whatever_they_are_built_from() {
        let mut circle = Path::new();
        circle.circle(50.0, 40.0, 25.0);
        let fit = RoundedBox::fit(&circle).unwrap();
        assert_close(fit.extent, [25.0, 25.0]);
        assert_close(fit.radii, [25.0, 25.0]);

        let mut ellipse = Path::new();
        ellipse.ellipse(0.0, 0.0, 30.0, 10.0);
        let fit = RoundedBox::fit(&ellipse).unwrap();
        assert_close(fit.extent, [30.0, 10.0]);
        assert_close(fit.radii, [30.0, 10.0]);

        let fit = RoundedBox::fit(&quad_ellipse(100.0, 100.0, 50.0, 30.0)).unwrap();
        assert!((fit.extent[0] - 50.0).abs() < 0.2 && (fit.extent[1] - 30.0).abs() < 0.2);
        assert_eq!(fit.radii, fit.extent, "no straight side: an ellipse");

        let mut rounded = Path::new();
        rounded.rounded_rect(10.0, 10.0, 80.0, 40.0, 12.0);
        let fit = RoundedBox::fit(&rounded).unwrap();
        assert_close(fit.extent, [40.0, 20.0]);
        assert_close(fit.radii, [12.0, 12.0]);
    }

    #[test]
    fn a_fit_does_not_depend_on_scale() {
        for scale in [0.01, 1.0, 400.0] {
            let fit = RoundedBox::fit(&quad_ellipse(0.0, 0.0, 50.0 * scale, 30.0 * scale));
            assert!(fit.is_some(), "scale {scale}");
            let mut rounded = Path::new();
            rounded.rounded_rect(0.0, 0.0, 80.0 * scale, 40.0 * scale, 12.0 * scale);
            assert!(RoundedBox::fit(&rounded).is_some(), "scale {scale}");
        }
    }

    #[test]
    fn other_outlines_do_not_fit() {
        let mut triangle = Path::new();
        triangle.move_to(0.0, 0.0);
        triangle.line_to(10.0, 0.0);
        triangle.line_to(5.0, 8.0);
        triangle.close();
        assert_eq!(RoundedBox::fit(&triangle), None);

        let mut trapezoid = Path::new();
        trapezoid.move_to(0.0, 0.0);
        trapezoid.line_to(10.0, 0.0);
        trapezoid.line_to(8.0, 8.0);
        trapezoid.line_to(2.0, 8.0);
        assert_eq!(RoundedBox::fit(&trapezoid), None);

        let mut two = Path::new();
        two.rect(0.0, 0.0, 10.0, 10.0);
        two.rect(20.0, 0.0, 10.0, 10.0);
        assert_eq!(RoundedBox::fit(&two), None, "two contours");

        let mut varying = Path::new();
        varying.rounded_rect_varying(0.0, 0.0, 80.0, 40.0, 4.0, 8.0, 12.0, 16.0);
        assert_eq!(RoundedBox::fit(&varying), None, "corners of different radii");

        // A circle of four quadratics is 6 % off: a blob, not a circle.
        let mut blob = Path::new();
        blob.move_to(10.0, 0.0);
        blob.quad_to(10.0, 10.0, 0.0, 10.0);
        blob.quad_to(-10.0, 10.0, -10.0, 0.0);
        blob.quad_to(-10.0, -10.0, 0.0, -10.0);
        blob.quad_to(10.0, -10.0, 10.0, 0.0);
        assert_eq!(RoundedBox::fit(&blob), None);

        // Twice around is empty under even-odd.
        let mut twice = Path::new();
        twice.move_to(10.0, 0.0);
        for _ in 0..2 {
            twice.arc(0.0, 0.0, 10.0, 0.0, std::f32::consts::TAU, crate::Solidity::Hole);
        }
        assert_eq!(RoundedBox::fit(&twice), None);

        // A corner a thousandth of the size off makes it another quadrilateral.
        let mut off = Path::new();
        off.move_to(0.0, 0.0);
        off.line_to(100.0, 0.0);
        off.line_to(100.0, 50.1);
        off.line_to(0.0, 50.0);
        assert_eq!(RoundedBox::fit(&off), None);

        // A polygon is not a circle, however many sides it has.
        let mut fine = Path::new();
        fine.move_to(10.0, 0.0);
        for i in 1..=200 {
            let a = i as f32 / 200.0 * std::f32::consts::TAU;
            fine.line_to(10.0 * a.cos(), 10.0 * a.sin());
        }
        assert_eq!(RoundedBox::fit(&fine), None);

        let mut line = Path::new();
        line.move_to(0.0, 0.0);
        line.line_to(10.0, 0.0);
        assert_eq!(RoundedBox::fit(&line), None);
        assert_eq!(RoundedBox::fit(&Path::new()), None);
    }

    #[test]
    fn coverage_counts_device_pixels_across_each_side() {
        // A 40 x 20 box at (30, 20), rotated a quarter turn and doubled, at
        // a device pixel ratio of two: a pixel is half a unit.
        let mut transform = Transform2D::rotation(std::f32::consts::FRAC_PI_2);
        transform.scale(2.0, 2.0);
        let shape = RoundedBox {
            frame: Transform2D::translation(30.0, 20.0),
            extent: [20.0, 10.0],
            radii: [0.0, 0.0],
        }
        .transformed(&transform);
        let coverage = shape.coverage(0.5).unwrap();
        assert_close(coverage.extent, [80.0, 40.0]);
        let (x, y) = transform.transform_point(30.0, 20.0);
        assert!(
            (coverage.distance([x, y]) + 40.0).abs() < 1e-3,
            "center: the nearer side is 40 px away"
        );
        let (x, y) = transform.transform_point(50.25, 20.0);
        assert!(
            (coverage.distance([x, y]) - 1.0).abs() < 1e-3,
            "a quarter unit out is one pixel"
        );

        // Under a skew a coordinate still counts pixels perpendicular to its side.
        let mut skew = Transform2D::identity();
        skew.skew_x(std::f32::consts::FRAC_PI_4);
        let coverage = shape_at_origin().transformed(&skew).coverage(1.0).unwrap();
        assert!((coverage.distance([10.0 + std::f32::consts::SQRT_2, 0.0]) - 1.0).abs() < 1e-3);

        let flat = shape_at_origin().transformed(&Transform2D::scaling(1.0, 0.0));
        assert_eq!(flat.coverage(1.0), None, "a collapsed frame has no coverage");
    }

    fn shape_at_origin() -> RoundedBox {
        RoundedBox {
            frame: Transform2D::identity(),
            extent: [10.0, 10.0],
            radii: [0.0, 0.0],
        }
    }

    #[test]
    fn corner_distance_is_exact_for_a_circle_and_first_order_for_an_ellipse() {
        let circle = RoundedBox {
            frame: Transform2D::identity(),
            extent: [10.0, 10.0],
            radii: [10.0, 10.0],
        }
        .coverage(1.0)
        .unwrap();
        for (point, distance) in [
            ([13.0, 0.0], 3.0),
            ([6.0, 8.0], 0.0),
            ([3.0, 4.0], -5.0),
            ([9.0, 12.0], 5.0),
        ] {
            assert!((circle.distance(point) - distance).abs() < 1e-3, "{point:?}");
        }
        let ellipse = RoundedBox {
            frame: Transform2D::identity(),
            extent: [30.0, 10.0],
            radii: [30.0, 10.0],
        }
        .coverage(1.0)
        .unwrap();
        // Half a pixel off the curve along its normal at 45 degrees of parameter.
        let (x, y) = (30.0 * 0.5_f32.sqrt(), 10.0 * 0.5_f32.sqrt());
        let normal = [x / 900.0, y / 100.0];
        let length = normal[0].hypot(normal[1]);
        let out = [x + 0.5 * normal[0] / length, y + 0.5 * normal[1] / length];
        assert!((ellipse.distance(out) - 0.5).abs() < 0.02, "{}", ellipse.distance(out));
    }

    #[test]
    fn corner_distance_counts_device_pixels_under_a_skew() {
        // A circle turned and then stretched: its frame's axes are no longer
        // at a right angle, and the corner is an ellipse that lies askew in it.
        let mut transform = Transform2D::rotation(0.5);
        transform.scale(1.6, 0.7);
        let circle = RoundedBox {
            frame: Transform2D::identity(),
            extent: [40.0, 40.0],
            radii: [40.0, 40.0],
        }
        .transformed(&transform);
        let coverage = circle.coverage(1.0).unwrap();
        for step in 0..16 {
            let angle = (step as f32 + 0.5) * std::f32::consts::FRAC_PI_8;
            let (x, y) = transform.transform_point(40.0 * angle.cos(), 40.0 * angle.sin());
            // The tangent is the image of the circle's; the normal points away from the center.
            let (tx, ty) = transform.transform_point(-angle.sin(), angle.cos());
            let length = tx.hypot(ty);
            let mut normal = [ty / length, -tx / length];
            if normal[0] * x + normal[1] * y < 0.0 {
                normal = [-normal[0], -normal[1]];
            }
            for offset in [-0.5, 0.0, 0.5] {
                let at = [x + offset * normal[0], y + offset * normal[1]];
                assert!(
                    (coverage.distance(at) - offset).abs() < 0.03,
                    "{offset} px off the outline at step {step}: {}",
                    coverage.distance(at)
                );
            }
        }
    }

    #[test]
    fn a_corner_under_half_a_pixel_is_square() {
        let rounded = |radius: f32| RoundedBox {
            frame: Transform2D::identity(),
            extent: [20.0, 10.0],
            radii: [radius, radius],
        };
        assert_eq!(rounded(0.4).coverage(1.0).unwrap().radii, [0.0, 0.0]);
        assert_eq!(rounded(0.6).coverage(1.0).unwrap().radii, [0.6, 0.6]);
        // Half a pixel is a quarter of a unit at a device pixel ratio of two.
        assert_eq!(rounded(0.4).coverage(0.5).unwrap().radii, [0.8, 0.8]);
        // The radius on screen decides: a wide corner zoomed far out is as tight.
        let far = rounded(4.0).transformed(&Transform2D::scaling(0.1, 0.1));
        assert_eq!(far.coverage(1.0).unwrap().radii, [0.0, 0.0]);
        // Either radius: a corner tight on one axis alone is squared too.
        let lopsided = RoundedBox {
            frame: Transform2D::identity(),
            extent: [20.0, 10.0],
            radii: [8.0, 0.3],
        };
        assert_eq!(lopsided.coverage(1.0).unwrap().radii, [0.0, 0.0]);
    }

    #[test]
    fn a_box_thinner_than_a_pixel_has_no_coverage() {
        let slab = |half: f32| RoundedBox {
            frame: Transform2D::identity(),
            extent: [20.0, half],
            radii: [0.0, 0.0],
        };
        assert!(slab(0.5).coverage(1.0).is_some(), "one pixel thick");
        assert_eq!(slab(0.4).coverage(1.0), None);
        assert!(
            slab(0.4).coverage(0.5).is_some(),
            "a pixel is half a unit at a ratio of two"
        );
        let far = slab(4.0).transformed(&Transform2D::scaling(0.1, 0.1));
        assert_eq!(far.coverage(1.0), None, "the thickness on screen decides");
    }

    #[test]
    fn uniform_rows_carry_the_corners_centers_and_the_sides_ramps() {
        let rounded = RoundedBox {
            frame: Transform2D::translation(30.0, 40.0),
            extent: [20.0, 10.0],
            radii: [4.0, 3.0],
        };
        assert_eq!(
            rounded.coverage(1.0).unwrap().uniform_rows(),
            [16.0, 7.0, 1.0, 0.0, 0.0, 1.0, -30.0, -40.0, 20.5, 10.5]
        );
        // Square corners have no center a fragment could be past.
        let rows = shape_at_origin().coverage(1.0).unwrap().uniform_rows();
        assert_eq!(rows[..2], [NO_CORNERS; 2]);
        assert_eq!(rows[8..], [10.5, 10.5]);
    }

    #[test]
    fn a_frame_skewed_to_a_sliver_has_no_coverage() {
        let skewed = |degrees: f32| {
            let mut skew = Transform2D::identity();
            skew.skew_x((90.0 - degrees).to_radians());
            shape_at_origin().transformed(&skew).coverage(1.0)
        };
        assert!(skewed(5.0).is_some(), "axes five degrees apart");
        assert_eq!(skewed(0.3), None, "axes a third of a degree apart");
        // The test is of the angle, not the size.
        let tiny = shape_at_origin().transformed(&Transform2D::scaling(1e-4, 1e-4));
        assert!(tiny.coverage(1.0).is_some());
    }

    #[test]
    fn containment_needs_the_margin_on_every_corner() {
        let outer = RoundedBox {
            frame: Transform2D::translation(50.0, 50.0),
            extent: [40.0, 40.0],
            radii: [40.0, 40.0],
        };
        let inner = |half: f32| RoundedBox {
            frame: Transform2D::translation(50.0, 50.0),
            extent: [half, half],
            radii: [0.0, 0.0],
        };
        let coverage = outer.coverage(1.0).unwrap();
        assert!(coverage.contains(&inner(28.0)), "corners 39.6 from the center");
        assert!(
            coverage.contains(&inner(28.5)),
            "corners 0.3 px out: within what a stencil resolves"
        );
        assert!(!coverage.contains(&inner(29.0)), "corners a pixel out");
        assert!(!inner(28.0).coverage(1.0).unwrap().contains(&outer));

        // Round corners are followed, not boxed: a rounded rect lies inside
        // its twin, and inside one shifted by less than half a pixel.
        let rounded = |x: f32| RoundedBox {
            frame: Transform2D::translation(x, 50.0),
            extent: [40.0, 30.0],
            radii: [12.0, 12.0],
        };
        let twin = rounded(50.0).coverage(1.0).unwrap();
        assert!(twin.contains(&rounded(50.0)));
        assert!(twin.contains(&rounded(50.3)));
        assert!(!twin.contains(&rounded(51.0)));
        // At a device pixel ratio of two the same shift is over half a pixel.
        assert!(!rounded(50.0).coverage(0.5).unwrap().contains(&rounded(50.3)));
    }

    #[test]
    fn parallelograms_with_parallel_sides_intersect_as_one() {
        let rect = |x: f32, y: f32, w: f32, h: f32| {
            let mut path = Path::new();
            path.rect(x, y, w, h);
            RoundedBox::fit(&path).unwrap()
        };
        let corners = |a: RoundedBox, b: RoundedBox| {
            let mut corners = a.intersection(&b, 1.0).unwrap().corners().to_vec();
            corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
            corners
        };
        let expected = [[40.0, 30.0], [40.0, 50.0], [60.0, 30.0], [60.0, 50.0]];
        // The same region whichever box is asked and however each was wound.
        let (a, b) = (rect(10.0, 10.0, 50.0, 40.0), rect(40.0, 30.0, 50.0, 40.0));
        let mut turned = Path::new();
        turned.move_to(90.0, 30.0);
        turned.line_to(90.0, 70.0);
        turned.line_to(40.0, 70.0);
        turned.line_to(40.0, 30.0);
        let turned = RoundedBox::fit(&turned).unwrap();
        for (first, second) in [(a, b), (b, a), (a, turned), (turned, a)] {
            for (corner, expected) in corners(first, second).iter().zip(expected) {
                assert_close(*corner, expected);
            }
        }
        // Under one transform the sides stay parallel.
        let mut spin = Transform2D::rotation(0.7);
        spin.scale(2.0, 0.5);
        let both = a.transformed(&spin).intersection(&b.transformed(&spin), 1.0).unwrap();
        let mut spun = both.corners().to_vec();
        spun.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut expected_spun = expected.map(|[x, y]| {
            let (x, y) = spin.transform_point(x, y);
            [x, y]
        });
        expected_spun.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for (corner, expected) in spun.iter().zip(expected_spun) {
            assert_close(*corner, expected);
        }

        assert_eq!(a.intersection(&rect(70.0, 10.0, 10.0, 10.0), 1.0), None, "apart");
        assert_eq!(
            a.intersection(&b.transformed(&Transform2D::rotation(0.3)), 1.0),
            None,
            "not parallel"
        );
        let mut circle = Path::new();
        circle.circle(55.0, 45.0, 20.0);
        assert_eq!(
            a.intersection(&RoundedBox::fit(&circle).unwrap(), 1.0),
            None,
            "a circle across a corner: what they share is no box"
        );
    }

    #[test]
    fn boxes_with_round_corners_intersect_as_one_when_one_all_but_holds_the_other() {
        let rounded = |x0: f32, y0: f32, x1: f32, y1: f32, radius: f32| RoundedBox {
            frame: Transform2D::translation((x0 + x1) * 0.5, (y0 + y1) * 0.5),
            extent: [(x1 - x0) * 0.5, (y1 - y0) * 0.5],
            radii: [radius, radius],
        };
        let sides = |shape: RoundedBox| {
            let (x, y) = shape.frame.transform_point(0.0, 0.0);
            [
                x - shape.extent[0],
                y - shape.extent[1],
                x + shape.extent[0],
                y + shape.extent[1],
            ]
        };
        let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-3);

        // An icon's frame and a second one a fraction of a pixel smaller and
        // a quarter pixel further right: each side of what they share is
        // the nearer one, which is neither frame.
        let outer = rounded(131.94, 33.23, 325.49, 226.78, 41.47);
        let inner = rounded(132.72, 33.23, 325.72, 226.22, 41.36);
        for (first, second) in [(outer, inner), (inner, outer)] {
            let both = first.intersection(&second, 1.0).unwrap();
            assert!(near(sides(both), [132.72, 33.23, 325.49, 226.22]), "{:?}", sides(both));
            assert!((41.3..41.5).contains(&both.radii[0]));
        }

        // One inside the other is the inner one, whichever is asked.
        let small = rounded(150.0, 60.0, 300.0, 200.0, 12.0);
        assert_eq!(outer.intersection(&small, 1.0), Some(small));
        assert_eq!(small.intersection(&outer, 1.0), Some(small));

        // A band across the straight part of a rounded box is a rect; one
        // that cuts through its corners shares no box with it.
        let band = rounded(100.0, 100.0, 400.0, 180.0, 0.0);
        let cut = outer.intersection(&band, 1.0).unwrap();
        assert!(near(sides(cut), [131.94, 100.0, 325.49, 180.0]) && cut.radii == [0.0, 0.0]);
        let across = rounded(100.0, 50.0, 400.0, 180.0, 0.0);
        assert_eq!(outer.intersection(&across, 1.0), None);
        // Nor do two frames a pixel apart at a corner.
        let shifted = rounded(132.94, 34.23, 326.49, 227.78, 41.47);
        assert_eq!(outer.intersection(&shifted, 1.0), None);
    }
}
