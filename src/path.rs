use std::{
    cell::{RefCell, RefMut},
    f32::consts::PI,
    slice,
};

use crate::geometry::{Position, Transform2D, Vector};
#[cfg(feature = "textlayout")]
use rustybuzz::ttf_parser;

mod cache;
pub use cache::{Convexity, PathCache};

// Length proportional to radius of a cubic bezier handle for 90deg arcs.
const KAPPA90: f32 = 0.552_284_8; // 0.552_284_749_3;

/// Specifies whether a shape is solid or a hole when adding it to a path.
///
/// The default value is `Solid`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Default)]
pub enum Solidity {
    /// The shape is solid (filled).
    #[default]
    Solid = 1,
    /// The shape is a hole (not filled).
    Hole = 2,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PackedVerb {
    MoveTo,
    LineTo,
    BezierTo,
    Solid,
    Hole,
    Close,
}

/// A verb describes how to interpret one or more points to continue the countour
/// of a [`Path`].
#[derive(Copy, Clone, Debug)]
pub enum Verb {
    /// Terminates the current sub-path and defines the new current point by the
    /// given x/y f32 coordinates.
    MoveTo(f32, f32),
    /// Describes that the contour of the path should continue as a line from the
    /// current point to the given x/y f32 coordinates.
    LineTo(f32, f32),
    /// Describes that the contour of the path should continue as a cubie bezier segment from the
    /// current point via two control points (as f32 pairs) to the point in the last f32 pair.
    BezierTo(f32, f32, f32, f32, f32, f32),
    /// Sets the current sub-path winding to be solid.
    Solid,
    /// Sets the current sub-path winding to be hole.
    Hole,
    /// Closes the current sub-path.
    Close,
}

impl Verb {
    fn num_coordinates(&self) -> usize {
        match *self {
            Self::MoveTo(..) => 1,
            Self::LineTo(..) => 1,
            Self::BezierTo(..) => 3,
            Self::Solid => 0,
            Self::Hole => 0,
            Self::Close => 0,
        }
    }

    fn from_packed(packed: &PackedVerb, coords: &[Position]) -> Self {
        match *packed {
            PackedVerb::MoveTo => Self::MoveTo(coords[0].x, coords[0].y),
            PackedVerb::LineTo => Self::LineTo(coords[0].x, coords[0].y),
            PackedVerb::BezierTo => Self::BezierTo(
                coords[0].x,
                coords[0].y,
                coords[1].x,
                coords[1].y,
                coords[2].x,
                coords[2].y,
            ),
            PackedVerb::Solid => Self::Solid,
            PackedVerb::Hole => Self::Hole,
            PackedVerb::Close => Self::Close,
        }
    }
}

/// A collection of verbs (`move_to()`, `line_to()`, `bezier_to()`, etc.)
/// describing one or more contours.
#[derive(Default, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Path {
    verbs: Vec<PackedVerb>,
    coords: Vec<Position>,
    last_pos: Position,
    dist_tol: f32,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) cache: RefCell<Option<(u64, PathCache)>>,
}

impl Path {
    /// Creates a new empty path with a distance tolerance of 0.01.
    pub fn new() -> Self {
        Self {
            dist_tol: 0.01,
            ..Default::default()
        }
    }

    /// Returns the memory size in bytes used by the path.
    pub fn size(&self) -> usize {
        std::mem::size_of::<PackedVerb>() * self.verbs.len() + std::mem::size_of::<f32>() * self.coords.len()
    }

    /// Checks if the path is empty (contains no verbs).
    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }

    /// Sets the distance tolerance used for path operations.
    pub fn set_distance_tolerance(&mut self, value: f32) {
        self.dist_tol = value;
    }

    /// Returns an iterator over the path's verbs.
    pub fn verbs(&self) -> PathIter<'_> {
        PathIter {
            verbs: self.verbs.iter(),
            coords: &self.coords,
        }
    }

    pub(crate) fn cache<'a>(&'a self, transform: &Transform2D, tess_tol: f32, dist_tol: f32) -> RefMut<'a, PathCache> {
        // The path cache saves a flattened and transformed version of the path. If client code calls
        // (fill|stroke)_path repeatedly with the same Path under the same transform circumstances then it will be
        // retrieved from cache. I'm not sure if transform.cache_key() is actually good enough for this
        // and if it will produce the correct cache keys under different float edge cases.

        let key = transform.cache_key();

        // this shouldn't need a bool once non lexic lifetimes are stable
        let needs_rebuild = if let Some((transform_cache_key, _cache)) = &*self.cache.borrow() {
            key != *transform_cache_key
        } else {
            true
        };

        if needs_rebuild {
            let path_cache = PathCache::new(self.verbs(), transform, tess_tol, dist_tol);
            *self.cache.borrow_mut() = Some((key, path_cache));
        }

        RefMut::map(self.cache.borrow_mut(), |cache| &mut cache.as_mut().unwrap().1)
    }

    /// Returns a path containing only the visible segments of this path when
    /// stroked with the given dash pattern and offset.
    ///
    /// Empty patterns, patterns whose entries sum to zero, and patterns with
    /// non-finite or negative entries return this path unchanged. Odd-length
    /// patterns are repeated once, matching SVG and Canvas 2D behavior.
    pub fn dashed(&self, dash: &[f32], offset: f32) -> Self {
        self.dashed_with_tolerance(dash, offset, self.dist_tol)
    }

    pub(crate) fn dashed_with_tolerance(&self, dash: &[f32], offset: f32, tess_tol: f32) -> Self {
        let dash = normalize_dash_pattern(dash);
        if dash.is_empty() {
            return self.clone();
        }

        let contours = self.flattened_contours(tess_tol, self.dist_tol);
        let mut dashed = Self::new();
        dashed.dist_tol = self.dist_tol;

        for contour in contours {
            if contour.points.len() < 2 {
                continue;
            }

            let mut cursor = DashCursor::new(&dash, offset);
            for segment in contour.points.windows(2) {
                dash_line_segment(&mut dashed, segment[0], segment[1], &mut cursor, self.dist_tol);
            }

            if contour.closed {
                dash_line_segment(
                    &mut dashed,
                    *contour.points.last().unwrap(),
                    contour.points[0],
                    &mut cursor,
                    self.dist_tol,
                );
            }
        }

        dashed
    }

    fn flattened_contours(&self, tess_tol: f32, dist_tol: f32) -> Vec<FlattenedContour> {
        let mut contours = Vec::new();
        let mut current = FlattenedContour::default();
        let mut current_pos = Position::default();
        let mut first_pos = None;

        let finish_current = |contours: &mut Vec<FlattenedContour>, current: &mut FlattenedContour| {
            if current.points.len() >= 2 {
                contours.push(std::mem::take(current));
            } else {
                current.points.clear();
                current.closed = false;
            }
        };

        for verb in self.verbs() {
            match verb {
                Verb::MoveTo(x, y) => {
                    finish_current(&mut contours, &mut current);
                    current_pos = Position { x, y };
                    first_pos = Some(current_pos);
                    current.points.push(current_pos);
                }
                Verb::LineTo(x, y) => {
                    current_pos = Position { x, y };
                    push_flattened_point(&mut current.points, current_pos, dist_tol);
                }
                Verb::BezierTo(c1x, c1y, c2x, c2y, x, y) => {
                    let c1 = Position { x: c1x, y: c1y };
                    let c2 = Position { x: c2x, y: c2y };
                    let end = Position { x, y };
                    flatten_bezier(&mut current.points, current_pos, c1, c2, end, 0, tess_tol, dist_tol);
                    current_pos = end;
                }
                Verb::Close => {
                    if let Some(first) = first_pos {
                        current.closed = true;
                        current_pos = first;
                    }
                }
                Verb::Solid | Verb::Hole => {}
            }
        }

        finish_current(&mut contours, &mut current);
        contours
    }

    // Path funcs

    /// Starts a new sub-path with the specified point as the first point.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.append(&[PackedVerb::MoveTo], &[Position { x, y }]);
    }

    /// Adds a line segment from the last point in the path to the specified point.
    pub fn line_to(&mut self, x: f32, y: f32) {
        self.append(&[PackedVerb::LineTo], &[Position { x, y }]);
    }

    /// Adds a cubic bezier segment from the last point in the path via two control points to the specified point.
    pub fn bezier_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.append(
            &[PackedVerb::BezierTo],
            &[
                Position { x: c1x, y: c1y },
                Position { x: c2x, y: c2y },
                Position { x, y },
            ],
        );
    }

    /// Adds a quadratic bezier segment from the last point in the path via a control point to the specified point.
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let pos0 = self.last_pos;
        let cpos = Position { x: cx, y: cy };
        let pos = Position { x, y };
        let pos1 = pos0 + (cpos - pos0) * (2.0 / 3.0);
        let pos2 = pos + (cpos - pos) * (2.0 / 3.0);

        self.append(&[PackedVerb::BezierTo], &[pos1, pos2, pos]);
    }

    /// Closes the current sub-path with a line segment.
    pub fn close(&mut self) {
        self.append(&[PackedVerb::Close], &[]);
    }

    /// Sets the current sub-path winding, see [`Solidity`].
    pub fn solidity(&mut self, solidity: Solidity) {
        match solidity {
            Solidity::Solid => self.append(&[PackedVerb::Solid], &[]),
            Solidity::Hole => self.append(&[PackedVerb::Hole], &[]),
        }
    }

    /// Creates new circle arc shaped sub-path. The arc center is at `cx`,`cy`, the arc radius is `r`,
    /// and the arc is drawn from angle `a0` to `a1`, and swept in direction `dir` (Winding)
    /// Angles are specified in radians.
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, dir: Solidity) {
        let cpos = Position { x: cx, y: cy };

        let mut da = a1 - a0;

        if dir == Solidity::Hole {
            if da.abs() >= PI * 2.0 {
                da = PI * 2.0;
            } else {
                while da < 0.0 {
                    da += PI * 2.0
                }
            }
        } else if da.abs() >= PI * 2.0 {
            da = -PI * 2.0;
        } else {
            while da > 0.0 {
                da -= PI * 2.0
            }
        }

        // Split arc into max 90 degree segments.
        let ndivs = ((da.abs() / (PI * 0.5) + 0.5) as i32).clamp(1, 5);
        let hda = (da / ndivs as f32) / 2.0;
        let mut kappa = (4.0 / 3.0 * (1.0 - hda.cos()) / hda.sin()).abs();

        let mut commands = Vec::with_capacity(ndivs as usize);
        let mut coords = Vec::with_capacity(ndivs as usize);

        if dir == Solidity::Solid {
            kappa = -kappa;
        }

        let (mut ppos, mut ptanpos) = (Position { x: 0.0, y: 0.0 }, Vector::zero());

        for i in 0..=ndivs {
            let a = a0 + da * (i as f32 / ndivs as f32);
            let dpos = Vector::from_angle(a);
            let pos = cpos + dpos * r;
            let tanpos = -dpos.orthogonal() * r * kappa;

            if i == 0 {
                let first_move = if self.verbs.is_empty() {
                    PackedVerb::MoveTo
                } else {
                    PackedVerb::LineTo
                };

                commands.push(first_move);
                coords.extend_from_slice(&[pos]);
            } else {
                commands.push(PackedVerb::BezierTo);
                let pos1 = ppos + ptanpos;
                let pos2 = pos - tanpos;
                coords.extend_from_slice(&[pos1, pos2, pos]);
            }

            ppos = pos;
            ptanpos = tanpos;
        }

        self.append(&commands, &coords);
    }

    /// Adds an arc segment at the corner defined by the last path point and two specified points.
    pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) {
        if self.verbs.is_empty() {
            return;
        }

        let pos0 = self.last_pos;
        let pos1 = Position { x: x1, y: y1 };
        let pos2 = Position { x: x2, y: y2 };

        // Handle degenerate cases.
        if Position::equals(pos0, pos1, self.dist_tol)
            || Position::equals(pos1, pos2, self.dist_tol)
            || Position::segment_distance(pos1, pos0, pos2) < self.dist_tol * self.dist_tol
            || radius < self.dist_tol
        {
            self.line_to(pos1.x, pos1.y);
        }

        let mut dpos0 = pos0 - pos1;
        let mut dpos1 = pos2 - pos1;

        dpos0.normalize();
        dpos1.normalize();

        let a = dpos0.dot(dpos1).acos();
        let d = radius / (a / 2.0).tan();

        if d > 10000.0 {
            return self.line_to(pos1.x, pos1.y);
        }

        let (cpos, a0, a1, dir);

        if dpos0.cross(dpos1) > 0.0 {
            cpos = pos1 + dpos0 * d + dpos0.orthogonal() * radius;
            a0 = dpos0.angle();
            a1 = (-dpos1).angle();
            dir = Solidity::Hole;
        } else {
            cpos = pos1 + dpos0 * d - dpos0.orthogonal() * radius;
            a0 = (-dpos0).angle();
            a1 = dpos1.angle();
            dir = Solidity::Solid;
        }

        self.arc(cpos.x, cpos.y, radius, a0 + PI / 2.0, a1 + PI / 2.0, dir);
    }

    /// Adds an SVG elliptical arc (the path data `A`/`a` command) from the
    /// current point to (`x`, `y`), emitting cubic bezier segments.
    ///
    /// `rx`/`ry` are the ellipse radii, `x_axis_rotation` is the rotation of the
    /// ellipse's x-axis relative to the current coordinate system **in radians**
    /// (the SVG attribute is in degrees), `large_arc` selects the larger of the
    /// two possible arc sweeps, and `sweep` selects the positive-angle
    /// (clockwise in SVG's y-down system) direction.
    ///
    /// If the path is empty the arc starts from the origin, matching how the
    /// other arc builders treat an empty current point. Following the W3C SVG
    /// implementation notes (section F.6): identical start/end points omit the
    /// arc, a zero radius degrades to a straight line, negative radii use their
    /// absolute value, and radii too small to span the endpoints are scaled up.
    pub fn svg_arc_to(&mut self, rx: f32, ry: f32, x_axis_rotation: f32, large_arc: bool, sweep: bool, x: f32, y: f32) {
        // The HTML Canvas path primitives (`ellipse()`, `arc()`, `arcTo()`)
        // return early without changing the path "if any of the arguments are
        // infinite or NaN"; apply the same rule here so a single bad value
        // cannot poison every later coordinate derived from the current point.
        if !rx.is_finite() || !ry.is_finite() || !x_axis_rotation.is_finite() || !x.is_finite() || !y.is_finite() {
            return;
        }

        let start = if self.verbs.is_empty() {
            let origin = Position { x: 0.0, y: 0.0 };
            self.move_to(origin.x, origin.y);
            origin
        } else {
            self.last_pos
        };
        let end = Position { x, y };

        // F.6.2: identical endpoints omit the arc entirely.
        if start.x == end.x && start.y == end.y {
            return;
        }

        // F.6.6 step 1 / F.6.2: a zero radius degrades to a straight line.
        // F.6.2: negative radii drop their sign.
        //
        // The endpoint-to-center conversion below runs in f64: the squared
        // terms of F.6.6's lambda and the F.6.5.2 quotient overflow f32's
        // exponent range long before the resulting arc geometry itself leaves
        // f32 range (e.g. lambda for rx = 1e-30 radii that merely need to be
        // scaled up, or (1e30)^2 midpoint terms for large translations).
        // Reference SVG arc implementations (resvg via kurbo, Batik) perform
        // this conversion in f64 for the same reason.
        let mut rx = f64::from(rx).abs();
        let mut ry = f64::from(ry).abs();
        if rx == 0.0 || ry == 0.0 {
            self.line_to(end.x, end.y);
            return;
        }

        let (sin_phi, cos_phi) = f64::from(x_axis_rotation).sin_cos();

        // F.6.5.1: midpoint translation followed by rotation by -phi.
        let dx2 = (f64::from(start.x) - f64::from(end.x)) * 0.5;
        let dy2 = (f64::from(start.y) - f64::from(end.y)) * 0.5;
        let x1p = cos_phi * dx2 + sin_phi * dy2;
        let y1p = -sin_phi * dx2 + cos_phi * dy2;

        // F.6.6 step 3: scale the radii up if they cannot span the endpoints.
        let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
        if lambda > 1.0 {
            let s = lambda.sqrt();
            rx *= s;
            ry *= s;
        }

        // F.6.5.2: center in the rotated/translated frame.
        let rx2 = rx * rx;
        let ry2 = ry * ry;
        let x1p2 = x1p * x1p;
        let y1p2 = y1p * y1p;
        let numerator = (rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2).max(0.0);
        let denominator = rx2 * y1p2 + ry2 * x1p2;
        let mut coef = (numerator / denominator).sqrt();
        // The sign is positive when the flags differ, negative when they match.
        if large_arc == sweep {
            coef = -coef;
        }
        let cxp = coef * (rx * y1p / ry);
        let cyp = coef * -(ry * x1p / rx);

        // F.6.5.3: transform the center back to the original frame.
        let cx = cos_phi * cxp - sin_phi * cyp + (f64::from(start.x) + f64::from(end.x)) * 0.5;
        let cy = sin_phi * cxp + cos_phi * cyp + (f64::from(start.y) + f64::from(end.y)) * 0.5;

        // F.6.5.5 / F.6.5.6: starting angle and swept angle via the angle helper.
        let ux = (x1p - cxp) / rx;
        let uy = (y1p - cyp) / ry;
        let vx = (-x1p - cxp) / rx;
        let vy = (-y1p - cyp) / ry;

        let theta1 = svg_arc_angle(1.0, 0.0, ux, uy);
        let mut delta = svg_arc_angle(ux, uy, vx, vy);

        // Enforce the sweep flag direction (F.6.5.6 modulo rule).
        if !sweep && delta > 0.0 {
            delta -= std::f64::consts::PI * 2.0;
        } else if sweep && delta < 0.0 {
            delta += std::f64::consts::PI * 2.0;
        }

        // Split into segments of at most 90 degrees and emit cubic beziers.
        let ndivs = (delta.abs() / (std::f64::consts::PI * 0.5)).ceil().max(1.0) as i32;
        let seg = delta / f64::from(ndivs);
        // Maisonobe per-segment handle length matching the kappa technique used
        // by arc()/ellipse(): alpha = sin(seg) * (sqrt(4 + 3 tan(seg/2)^2) - 1) / 3.
        let half = seg * 0.5;
        let alpha = seg.sin() * ((4.0 + 3.0 * half.tan() * half.tan()).sqrt() - 1.0) / 3.0;

        let mut commands = Vec::with_capacity(ndivs as usize);
        let mut coords = Vec::with_capacity(ndivs as usize * 3);

        // Point and derivative of the rotated ellipse at parametric angle t.
        let point = |t: f64| -> (f64, f64) {
            let (sin_t, cos_t) = t.sin_cos();
            (
                cx + rx * cos_phi * cos_t - ry * sin_phi * sin_t,
                cy + rx * sin_phi * cos_t + ry * cos_phi * sin_t,
            )
        };
        let derivative = |t: f64| -> (f64, f64) {
            let (sin_t, cos_t) = t.sin_cos();
            (
                -rx * cos_phi * sin_t - ry * sin_phi * cos_t,
                -rx * sin_phi * sin_t + ry * cos_phi * cos_t,
            )
        };
        let to_position = |p: (f64, f64)| Position {
            x: p.0 as f32,
            y: p.1 as f32,
        };

        for i in 0..ndivs {
            let t1 = theta1 + seg * f64::from(i);
            let t2 = t1 + seg;
            let p1 = point(t1);
            let p2 = point(t2);
            let d1 = derivative(t1);
            let d2 = derivative(t2);
            let c1 = (p1.0 + d1.0 * alpha, p1.1 + d1.1 * alpha);
            let c2 = (p2.0 - d2.0 * alpha, p2.1 - d2.1 * alpha);

            commands.push(PackedVerb::BezierTo);
            coords.extend_from_slice(&[to_position(c1), to_position(c2), to_position(p2)]);
        }

        // Guarantee the path endpoint lands exactly on the requested point.
        if let Some(last) = coords.last_mut() {
            *last = end;
        }

        // Extreme (but finite) inputs can describe arc geometry that exceeds
        // f32 range even though both endpoints are representable (e.g. F.6.6
        // scaling of wildly mismatched radii yields a control point beyond
        // f32::MAX). Degrade to the chord rather than emit non-finite vertices
        // that would poison downstream tessellation; this preserves the
        // invariant that the path always continues to the requested endpoint
        // with finite geometry.
        if coords
            .iter()
            .any(|position| !position.x.is_finite() || !position.y.is_finite())
        {
            self.line_to(end.x, end.y);
            return;
        }

        self.append(&commands, &coords);
    }

    /// Creates a new rectangle shaped sub-path.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.append(
            &[
                PackedVerb::MoveTo,
                PackedVerb::LineTo,
                PackedVerb::LineTo,
                PackedVerb::LineTo,
                PackedVerb::Close,
            ],
            &{
                let hoffset = Vector::x(w);
                let voffset = Vector::y(h);

                let tl = Position { x, y };
                let tr = tl + hoffset;
                let br = tr + voffset;
                let bl = tl + voffset;

                [tl, bl, br, tr]
            },
        );
    }

    /// Creates a new rounded rectangle shaped sub-path.
    pub fn rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        self.rounded_rect_varying(x, y, w, h, r, r, r, r);
    }

    /// Creates a new rounded rectangle shaped sub-path with varying radii for each corner.
    pub fn rounded_rect_varying(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rad_top_left: f32,
        rad_top_right: f32,
        rad_bottom_right: f32,
        rad_bottom_left: f32,
    ) {
        if rad_top_left < 0.1 && rad_top_right < 0.1 && rad_bottom_right < 0.1 && rad_bottom_left < 0.1 {
            self.rect(x, y, w, h);
        } else {
            let halfw = w.abs() * 0.5;
            let halfh = h.abs() * 0.5;

            let rx_bl = rad_bottom_left.min(halfw) * w.signum();
            let ry_bl = rad_bottom_left.min(halfh) * h.signum();

            let rx_br = rad_bottom_right.min(halfw) * w.signum();
            let ry_br = rad_bottom_right.min(halfh) * h.signum();

            let rx_tr = rad_top_right.min(halfw) * w.signum();
            let ry_tr = rad_top_right.min(halfh) * h.signum();

            let rx_tl = rad_top_left.min(halfw) * w.signum();
            let ry_tl = rad_top_left.min(halfh) * h.signum();

            self.append(
                &[
                    PackedVerb::MoveTo,
                    PackedVerb::LineTo,
                    PackedVerb::BezierTo,
                    PackedVerb::LineTo,
                    PackedVerb::BezierTo,
                    PackedVerb::LineTo,
                    PackedVerb::BezierTo,
                    PackedVerb::LineTo,
                    PackedVerb::BezierTo,
                    PackedVerb::Close,
                ],
                &[
                    Position { x, y: y + ry_tl },
                    Position { x, y: y + h - ry_bl },
                    //
                    Position {
                        x,
                        y: y + h - ry_bl * (1.0 - KAPPA90),
                    },
                    Position {
                        x: x + rx_bl * (1.0 - KAPPA90),
                        y: y + h,
                    },
                    Position { x: x + rx_bl, y: y + h },
                    //
                    Position {
                        x: x + w - rx_br,
                        y: y + h,
                    },
                    //
                    Position {
                        x: x + w - rx_br * (1.0 - KAPPA90),
                        y: y + h,
                    },
                    Position {
                        x: x + w,
                        y: y + h - ry_br * (1.0 - KAPPA90),
                    },
                    Position {
                        x: x + w,
                        y: y + h - ry_br,
                    },
                    //
                    Position { x: x + w, y: y + ry_tr },
                    //
                    Position {
                        x: x + w,
                        y: y + ry_tr * (1.0 - KAPPA90),
                    },
                    Position {
                        x: x + w - rx_tr * (1.0 - KAPPA90),
                        y,
                    },
                    Position { x: x + w - rx_tr, y },
                    //
                    Position { x: x + rx_tl, y },
                    //
                    Position {
                        x: x + rx_tl * (1.0 - KAPPA90),
                        y,
                    },
                    Position {
                        x,
                        y: y + ry_tl * (1.0 - KAPPA90),
                    },
                    Position { x, y: y + ry_tl },
                ],
            );
        }
    }

    /// Creates a new ellipse shaped sub-path.
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32) {
        self.append(
            &[
                PackedVerb::MoveTo,
                PackedVerb::BezierTo,
                PackedVerb::BezierTo,
                PackedVerb::BezierTo,
                PackedVerb::BezierTo,
                PackedVerb::Close,
            ],
            &{
                let cpos = Position { x: cx, y: cy };
                let hoffset = Vector::x(rx);
                let voffset = Vector::y(ry);
                [
                    cpos - hoffset,
                    cpos - hoffset + voffset * KAPPA90,
                    cpos - hoffset * KAPPA90 + voffset,
                    cpos + voffset,
                    cpos + hoffset * KAPPA90 + voffset,
                    cpos + hoffset + voffset * KAPPA90,
                    cpos + hoffset,
                    cpos + hoffset - voffset * KAPPA90,
                    cpos + hoffset * KAPPA90 - voffset,
                    cpos - voffset,
                    cpos - hoffset * KAPPA90 - voffset,
                    cpos - hoffset - voffset * KAPPA90,
                    cpos - hoffset,
                ]
            },
        );
    }

    /// Creates a new circle shaped sub-path.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32) {
        self.ellipse(cx, cy, r, r);
    }

    /// Appends a slice of verbs and coordinates to the path.
    fn append(&mut self, verbs: &[PackedVerb], coords: &[Position]) {
        if !coords.is_empty() {
            self.last_pos = coords[coords.len() - 1];
        }

        self.verbs.extend_from_slice(verbs);
        self.coords.extend_from_slice(coords);
    }
}

#[derive(Default)]
struct FlattenedContour {
    points: Vec<Position>,
    closed: bool,
}

struct DashCursor<'a> {
    dash: &'a [f32],
    index: usize,
    remaining: f32,
    drawing: bool,
}

impl<'a> DashCursor<'a> {
    fn new(dash: &'a [f32], offset: f32) -> Self {
        let total = dash.iter().sum::<f32>();
        let mut normalized_offset = if offset.is_finite() { offset % total } else { 0.0 };
        if normalized_offset < 0.0 {
            normalized_offset += total;
        }

        let mut index = 0;
        for (dash_index, interval) in dash.iter().copied().enumerate() {
            if normalized_offset > interval || (normalized_offset == interval && interval > 0.0) {
                normalized_offset -= interval;
                index = (dash_index + 1) % dash.len();
            } else {
                index = dash_index;
                break;
            }
        }

        let mut cursor = Self {
            dash,
            index,
            remaining: (dash[index] - normalized_offset).max(0.0),
            drawing: index % 2 == 0,
        };
        cursor.skip_empty_entries();
        cursor
    }

    fn advance(&mut self) {
        self.index = (self.index + 1) % self.dash.len();
        self.remaining = self.dash[self.index];
        self.drawing = self.index % 2 == 0;
        self.skip_empty_entries();
    }

    fn skip_empty_entries(&mut self) {
        for _ in 0..self.dash.len() {
            if self.remaining > f32::EPSILON {
                break;
            }
            self.index = (self.index + 1) % self.dash.len();
            self.remaining = self.dash[self.index];
            self.drawing = self.index % 2 == 0;
        }
    }
}

/// Signed angle between vectors `(ux, uy)` and `(vx, vy)` as defined by the W3C
/// SVG implementation notes F.6.5.4: `±arccos((u·v)/(|u||v|))` where the sign is
/// `+` when `ux*vy − uy*vx ≥ 0` and `−` otherwise.
///
/// The cross product is treated as a strict two-way sign so that a zero cross
/// product takes the positive branch. For collinear-opposite vectors the cross
/// product is zero and F.6.5.4 requires `+π` (not 0). `signum()` is unsuitable
/// here: it never returns 0 but returns `-1.0` for a `-0.0` input, and the cross
/// product of e.g. `(-1, 0)` and `(1, 0)` evaluates to `-0.0`, which would give
/// `-π` and violate the spec for that semicircle. The `< 0.0 → -1 else +1` rule
/// treats both `+0.0` and `-0.0` as positive, yielding `+π`; the F.6.5.6
/// sweep-modulo rule downstream then flips `+π` to `−π` when the sweep flag is
/// unset, so both semicircle directions render correctly.
///
/// Operates in f64 like the rest of the endpoint-to-center conversion.
fn svg_arc_angle(ux: f64, uy: f64, vx: f64, vy: f64) -> f64 {
    let dot = ux * vx + uy * vy;
    let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
    let mut cos = if len == 0.0 { 0.0 } else { dot / len };
    cos = cos.clamp(-1.0, 1.0);
    let sign = if (ux * vy - uy * vx) < 0.0 { -1.0 } else { 1.0 };
    sign * cos.acos()
}

fn normalize_dash_pattern(dash: &[f32]) -> Vec<f32> {
    if dash.is_empty() || dash.iter().any(|value| !value.is_finite() || *value < 0.0) {
        return Vec::new();
    }

    let sum = dash.iter().sum::<f32>();
    if sum <= f32::EPSILON {
        return Vec::new();
    }

    let mut normalized = dash.to_vec();
    if normalized.len() % 2 == 1 {
        normalized.extend_from_slice(dash);
    }

    normalized
}

fn push_flattened_point(points: &mut Vec<Position>, point: Position, dist_tol: f32) {
    if points
        .last()
        .is_some_and(|last| Position::equals(*last, point, dist_tol))
    {
        return;
    }

    points.push(point);
}

#[allow(clippy::too_many_arguments)]
fn flatten_bezier(
    points: &mut Vec<Position>,
    p0: Position,
    p1: Position,
    p2: Position,
    p3: Position,
    level: usize,
    tess_tol: f32,
    dist_tol: f32,
) {
    if level > 10 {
        push_flattened_point(points, p3, dist_tol);
        return;
    }

    let p01 = Position {
        x: (p0.x + p1.x) * 0.5,
        y: (p0.y + p1.y) * 0.5,
    };
    let p12 = Position {
        x: (p1.x + p2.x) * 0.5,
        y: (p1.y + p2.y) * 0.5,
    };
    let p23 = Position {
        x: (p2.x + p3.x) * 0.5,
        y: (p2.y + p3.y) * 0.5,
    };
    let p012 = Position {
        x: (p01.x + p12.x) * 0.5,
        y: (p01.y + p12.y) * 0.5,
    };
    let p123 = Position {
        x: (p12.x + p23.x) * 0.5,
        y: (p12.y + p23.y) * 0.5,
    };
    let p0123 = Position {
        x: (p012.x + p123.x) * 0.5,
        y: (p012.y + p123.y) * 0.5,
    };

    let dx = p3.x - p0.x;
    let dy = p3.y - p0.y;
    let d1 = ((p1.x - p3.x) * dy - (p1.y - p3.y) * dx).abs();
    let d2 = ((p2.x - p3.x) * dy - (p2.y - p3.y) * dx).abs();

    if (d1 + d2) * (d1 + d2) < tess_tol * (dx * dx + dy * dy) {
        push_flattened_point(points, p3, dist_tol);
        return;
    }

    flatten_bezier(points, p0, p01, p012, p0123, level + 1, tess_tol, dist_tol);
    flatten_bezier(points, p0123, p123, p23, p3, level + 1, tess_tol, dist_tol);
}

fn dash_line_segment(path: &mut Path, start: Position, end: Position, cursor: &mut DashCursor<'_>, dist_tol: f32) {
    let delta = end - start;
    let length = delta.mag2().sqrt();
    if length <= dist_tol {
        return;
    }

    let direction = delta * (1.0 / length);
    let mut travelled = 0.0;
    while travelled < length {
        if cursor.remaining <= f32::EPSILON {
            cursor.advance();
            continue;
        }

        let step = cursor.remaining.min(length - travelled);
        if cursor.drawing && step > dist_tol {
            let dash_start = start + direction * travelled;
            let dash_end = start + direction * (travelled + step);
            path.move_to(dash_start.x, dash_start.y);
            path.line_to(dash_end.x, dash_end.y);
        }

        travelled += step;
        cursor.remaining -= step;
    }
}

/// An iterator over the verbs and coordinates of a path.
#[derive(Debug)]
pub struct PathIter<'a> {
    verbs: slice::Iter<'a, PackedVerb>,
    coords: &'a [Position],
}

impl Iterator for PathIter<'_> {
    type Item = Verb;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(verb) = self.verbs.next() {
            let verb = Verb::from_packed(verb, self.coords);
            let num_coords = verb.num_coordinates();
            self.coords = &self.coords[num_coords..];
            Some(verb)
        } else {
            None
        }
    }
}

#[cfg(feature = "textlayout")]
impl ttf_parser::OutlineBuilder for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        self.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.bezier_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::{svg_arc_angle, Path, Verb, PI};

    fn line_path(length: f32) -> Path {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(length, 0.0);
        path
    }

    fn dashed_line_segments(path: &Path) -> Vec<((f32, f32), (f32, f32))> {
        let mut segments = Vec::new();
        let mut current = None;

        for verb in path.verbs() {
            match verb {
                Verb::MoveTo(x, y) => current = Some((x, y)),
                Verb::LineTo(x, y) => {
                    let start = current.expect("line segment should start with move_to");
                    let end = (x, y);
                    segments.push((start, end));
                    current = Some(end);
                }
                other => panic!("unexpected dashed path verb: {other:?}"),
            }
        }

        segments
    }

    fn assert_segment(segment: ((f32, f32), (f32, f32)), expected: ((f32, f32), (f32, f32))) {
        let epsilon = 0.001;
        assert!((segment.0 .0 - expected.0 .0).abs() < epsilon);
        assert!((segment.0 .1 - expected.0 .1).abs() < epsilon);
        assert!((segment.1 .0 - expected.1 .0).abs() < epsilon);
        assert!((segment.1 .1 - expected.1 .1).abs() < epsilon);
    }

    #[test]
    fn dashed_line_splits_visible_intervals() {
        let dashed = line_path(10.0).dashed(&[2.0, 1.0], 0.0);
        let segments = dashed_line_segments(&dashed);

        assert_eq!(segments.len(), 4);
        assert_segment(segments[0], ((0.0, 0.0), (2.0, 0.0)));
        assert_segment(segments[1], ((3.0, 0.0), (5.0, 0.0)));
        assert_segment(segments[2], ((6.0, 0.0), (8.0, 0.0)));
        assert_segment(segments[3], ((9.0, 0.0), (10.0, 0.0)));
    }

    #[test]
    fn dashed_line_applies_offset() {
        let dashed = line_path(8.0).dashed(&[2.0, 2.0], 1.0);
        let segments = dashed_line_segments(&dashed);

        assert_eq!(segments.len(), 3);
        assert_segment(segments[0], ((0.0, 0.0), (1.0, 0.0)));
        assert_segment(segments[1], ((3.0, 0.0), (5.0, 0.0)));
        assert_segment(segments[2], ((7.0, 0.0), (8.0, 0.0)));
    }

    #[test]
    fn odd_dash_pattern_repeats() {
        let dashed = line_path(12.0).dashed(&[2.0, 1.0, 3.0], 0.0);
        let segments = dashed_line_segments(&dashed);

        assert_eq!(segments.len(), 3);
        assert_segment(segments[0], ((0.0, 0.0), (2.0, 0.0)));
        assert_segment(segments[1], ((3.0, 0.0), (6.0, 0.0)));
        assert_segment(segments[2], ((8.0, 0.0), (9.0, 0.0)));
    }

    #[test]
    fn invalid_dash_pattern_keeps_path_solid() {
        let path = line_path(10.0);
        let dashed = path.dashed(&[0.0, 0.0], 0.0);

        assert_eq!(
            format!("{:?}", path.verbs().collect::<Vec<_>>()),
            format!("{:?}", dashed.verbs().collect::<Vec<_>>())
        );
    }

    // ---- SVG elliptical arc (svg_arc_to) tests ----

    /// Evaluates a cubic bezier at parameter `s` in [0, 1].
    fn bezier_point(p0: (f32, f32), c1: (f32, f32), c2: (f32, f32), p3: (f32, f32), s: f32) -> (f32, f32) {
        let u = 1.0 - s;
        let w0 = u * u * u;
        let w1 = 3.0 * u * u * s;
        let w2 = 3.0 * u * s * s;
        let w3 = s * s * s;
        (
            w0 * p0.0 + w1 * c1.0 + w2 * c2.0 + w3 * p3.0,
            w0 * p0.1 + w1 * c1.1 + w2 * c2.1 + w3 * p3.1,
        )
    }

    /// Densely samples every bezier segment of `path`, returning the sampled
    /// points along with the final endpoint of the path.
    fn sample_path(path: &Path) -> (Vec<(f32, f32)>, (f32, f32)) {
        let mut points = Vec::new();
        let mut cur = (0.0_f32, 0.0_f32);
        let mut endpoint = (0.0_f32, 0.0_f32);

        for verb in path.verbs() {
            match verb {
                Verb::MoveTo(x, y) => {
                    cur = (x, y);
                    endpoint = cur;
                }
                Verb::LineTo(x, y) => {
                    for i in 0..=16 {
                        let s = i as f32 / 16.0;
                        points.push((cur.0 + (x - cur.0) * s, cur.1 + (y - cur.1) * s));
                    }
                    cur = (x, y);
                    endpoint = cur;
                }
                Verb::BezierTo(c1x, c1y, c2x, c2y, x, y) => {
                    let p3 = (x, y);
                    for i in 0..=32 {
                        let s = i as f32 / 32.0;
                        points.push(bezier_point(cur, (c1x, c1y), (c2x, c2y), p3, s));
                    }
                    cur = p3;
                    endpoint = cur;
                }
                Verb::Close | Verb::Solid | Verb::Hole => {}
            }
        }

        (points, endpoint)
    }

    /// Implicit-form residual of a rotated ellipse for the point (px, py): zero
    /// when the point lies exactly on the ellipse boundary.
    ///
    /// Cubic beziers only approximate an elliptical arc; with the per-segment
    /// (Maisonobe) handle length used here a full 90-degree segment deviates from
    /// the true ellipse by at most ~4e-3 in this residual (≈0.12px on a 60px
    /// radius), matching the accuracy of the existing `arc()`/`ellipse()`
    /// builders. Tests allow a little headroom over that worst case.
    const ELLIPSE_RESIDUAL_TOL: f32 = 5e-3;

    fn ellipse_residual(px: f32, py: f32, cx: f32, cy: f32, rx: f32, ry: f32, phi: f32) -> f32 {
        let (sin_phi, cos_phi) = phi.sin_cos();
        let dx = px - cx;
        let dy = py - cy;
        // Rotate the offset back into the ellipse's local axes.
        let u = cos_phi * dx + sin_phi * dy;
        let v = -sin_phi * dx + cos_phi * dy;
        (u * u) / (rx * rx) + (v * v) / (ry * ry) - 1.0
    }

    #[test]
    fn svg_arc_endpoint_is_exact() {
        let mut path = Path::new();
        path.move_to(100.0, 100.0);
        path.svg_arc_to(60.0, 40.0, 0.5, true, false, 240.0, 180.0);

        let (_, endpoint) = sample_path(&path);
        assert!((endpoint.0 - 240.0).abs() < 1e-3, "x endpoint {}", endpoint.0);
        assert!((endpoint.1 - 180.0).abs() < 1e-3, "y endpoint {}", endpoint.1);
    }

    #[test]
    fn svg_arc_points_lie_on_axis_aligned_ellipse() {
        let cx = 100.0;
        let cy = 100.0;
        let rx = 60.0;
        let ry = 40.0;

        // Start on the ellipse at angle 0, end at the top; the arc bulges out.
        let start = (cx + rx, cy);
        let end = (cx, cy - ry);

        let mut path = Path::new();
        path.move_to(start.0, start.1);
        path.svg_arc_to(rx, ry, 0.0, false, false, end.0, end.1);

        let (points, _) = sample_path(&path);
        assert!(!points.is_empty());
        for (px, py) in points {
            let r = ellipse_residual(px, py, cx, cy, rx, ry, 0.0);
            assert!(
                r.abs() < ELLIPSE_RESIDUAL_TOL,
                "point ({px}, {py}) off ellipse, residual {r}"
            );
        }
    }

    #[test]
    fn svg_arc_points_lie_on_rotated_ellipse() {
        let cx = 50.0;
        let cy = 70.0;
        let rx = 80.0;
        let ry = 30.0;
        let phi = 0.7_f32; // radians

        // Derive endpoints that genuinely lie on the rotated ellipse.
        let on_ellipse = |t: f32| -> (f32, f32) {
            let (sin_phi, cos_phi) = phi.sin_cos();
            let (sin_t, cos_t) = t.sin_cos();
            (
                cx + rx * cos_phi * cos_t - ry * sin_phi * sin_t,
                cy + rx * sin_phi * cos_t + ry * cos_phi * sin_t,
            )
        };
        let start = on_ellipse(0.3);
        let end = on_ellipse(2.4);

        let mut path = Path::new();
        path.move_to(start.0, start.1);
        path.svg_arc_to(rx, ry, phi, false, true, end.0, end.1);

        let (points, _) = sample_path(&path);
        assert!(!points.is_empty());
        for (px, py) in points {
            let r = ellipse_residual(px, py, cx, cy, rx, ry, phi);
            assert!(
                r.abs() < ELLIPSE_RESIDUAL_TOL,
                "point ({px}, {py}) off rotated ellipse, residual {r}"
            );
        }
    }

    /// Signed perpendicular offset of the arc's midpoint from the chord
    /// (start -> end). Its sign tells us which side of the chord the arc bulges
    /// toward; positive and negative are the two half-planes.
    fn chord_bulge_side(start: (f32, f32), end: (f32, f32), mid: (f32, f32)) -> f32 {
        let cx = end.0 - start.0;
        let cy = end.1 - start.1;
        let mx = mid.0 - start.0;
        let my = mid.1 - start.1;
        cx * my - cy * mx
    }

    #[test]
    fn svg_arc_flag_combinations_are_distinct() {
        let start = (100.0, 100.0);
        let end = (200.0, 150.0);
        let rx = 80.0;
        let ry = 80.0;

        let cases = [(false, false), (false, true), (true, false), (true, true)];
        let mut midpoints = Vec::new();
        let mut sides = Vec::new();
        for &(large, sweep) in &cases {
            let mut path = Path::new();
            path.move_to(start.0, start.1);
            path.svg_arc_to(rx, ry, 0.0, large, sweep, end.0, end.1);
            let (points, _) = sample_path(&path);
            let mid = points[points.len() / 2];
            midpoints.push(mid);
            sides.push(chord_bulge_side(start, end, mid));
        }

        // The four arcs must reach four distinct midpoints.
        for i in 0..cases.len() {
            for j in (i + 1)..cases.len() {
                let d = (midpoints[i].0 - midpoints[j].0).hypot(midpoints[i].1 - midpoints[j].1);
                assert!(
                    d > 1.0,
                    "arcs {:?} and {:?} have coincident midpoints",
                    cases[i],
                    cases[j]
                );
            }
        }

        // The sweep flag controls which side of the chord the arc bulges to, so
        // for a fixed large_arc flag the two sweep values must land on opposite
        // half-planes. sides indices: 0=(F,F) 1=(F,T) 2=(T,F) 3=(T,T).
        assert!(
            sides[0] * sides[1] < 0.0,
            "small arcs should bulge to opposite sides: {:?}",
            sides
        );
        assert!(
            sides[2] * sides[3] < 0.0,
            "large arcs should bulge to opposite sides: {:?}",
            sides
        );
    }

    #[test]
    fn svg_arc_large_flag_selects_longer_sweep() {
        let start = (100.0, 100.0);
        let end = (160.0, 100.0);
        let rx = 50.0;
        let ry = 50.0;

        let arc_length = |large: bool| -> f32 {
            let mut path = Path::new();
            path.move_to(start.0, start.1);
            path.svg_arc_to(rx, ry, 0.0, large, true, end.0, end.1);
            let (points, _) = sample_path(&path);
            points
                .windows(2)
                .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
                .sum()
        };

        let small = arc_length(false);
        let large = arc_length(true);
        assert!(
            large > small,
            "large arc ({large}) should be longer than small ({small})"
        );
    }

    #[test]
    fn svg_arc_out_of_range_radii_are_scaled() {
        // Endpoints 200 apart but radius only 10: F.6.6 must scale the radii up
        // so the arc still reaches the endpoint exactly.
        let start = (0.0, 0.0);
        let end = (200.0, 0.0);

        let mut path = Path::new();
        path.move_to(start.0, start.1);
        path.svg_arc_to(10.0, 10.0, 0.0, false, true, end.0, end.1);

        let (points, endpoint) = sample_path(&path);
        assert!((endpoint.0 - end.0).abs() < 1e-3);
        assert!((endpoint.1 - end.1).abs() < 1e-3);

        // With radii scaled to exactly span the chord, the arc is a half-circle of
        // radius 100 centered at (100, 0); every sampled point must lie on it.
        for (px, py) in points {
            let r = ellipse_residual(px, py, 100.0, 0.0, 100.0, 100.0, 0.0);
            assert!(
                r.abs() < ELLIPSE_RESIDUAL_TOL,
                "scaled-radii point ({px}, {py}) off circle, residual {r}"
            );
        }
    }

    #[test]
    fn svg_arc_zero_radius_is_straight_line() {
        let mut path = Path::new();
        path.move_to(10.0, 20.0);
        path.svg_arc_to(0.0, 40.0, 0.0, true, true, 80.0, 90.0);

        let verbs: Vec<_> = path.verbs().collect();
        // move_to + a single line_to, no bezier segments.
        assert_eq!(verbs.len(), 2);
        assert!(matches!(verbs[0], Verb::MoveTo(10.0, 20.0)));
        assert!(matches!(verbs[1], Verb::LineTo(80.0, 90.0)));
    }

    #[test]
    fn svg_arc_identical_endpoints_add_nothing() {
        let mut path = Path::new();
        path.move_to(30.0, 30.0);
        let before = path.verbs().count();
        path.svg_arc_to(50.0, 50.0, 0.0, true, true, 30.0, 30.0);
        let after = path.verbs().count();
        assert_eq!(before, after, "identical endpoints must not add any verbs");
    }

    #[test]
    fn svg_arc_negative_radii_use_absolute_value() {
        let rx = 70.0;
        let ry = 45.0;
        let start = (rx, 0.0);
        let end = (0.0, ry);

        // Negative radii must drop their sign (F.6.2), producing geometry
        // identical to the equivalent positive-radii arc.
        let mut neg = Path::new();
        neg.move_to(start.0, start.1);
        neg.svg_arc_to(-rx, -ry, 0.0, false, true, end.0, end.1);

        let mut pos = Path::new();
        pos.move_to(start.0, start.1);
        pos.svg_arc_to(rx, ry, 0.0, false, true, end.0, end.1);

        let (neg_pts, _) = sample_path(&neg);
        let (pos_pts, _) = sample_path(&pos);
        assert_eq!(neg_pts.len(), pos_pts.len());
        for (n, p) in neg_pts.iter().zip(pos_pts.iter()) {
            assert!((n.0 - p.0).abs() < 1e-5 && (n.1 - p.1).abs() < 1e-5);
        }

        // ...and that geometry lies on the origin-centered ellipse.
        for (px, py) in pos_pts {
            let r = ellipse_residual(px, py, 0.0, 0.0, rx, ry, 0.0);
            assert!(
                r.abs() < ELLIPSE_RESIDUAL_TOL,
                "negative-radii point ({px}, {py}) off ellipse, residual {r}"
            );
        }
    }

    #[test]
    fn svg_arc_on_empty_path_starts_at_origin() {
        let mut path = Path::new();
        path.svg_arc_to(50.0, 50.0, 0.0, false, true, 100.0, 0.0);

        let verbs: Vec<_> = path.verbs().collect();
        // An implicit move_to(0, 0) is inserted before the bezier segments.
        assert!(matches!(verbs[0], Verb::MoveTo(0.0, 0.0)));
        assert!(verbs.len() > 1);

        let (_, endpoint) = sample_path(&path);
        assert!((endpoint.0 - 100.0).abs() < 1e-3);
        assert!((endpoint.1).abs() < 1e-3);
    }

    #[test]
    fn svg_arc_angle_collinear_opposite_is_positive_pi() {
        // F.6.5.4 boundary: for collinear-opposite vectors the cross product
        // ux*vy - uy*vx is zero and the spec mandates the POSITIVE branch, i.e.
        // +PI. This is exactly the start->end radius-vector pair of the standard
        // `move_to(0,0); svg_arc_to(50,50,0,_,_,100,0)` semicircle, where the
        // start vector is (-1, 0) and the end vector is (1, 0).
        //
        // The product `(-1)*0 - 0*1` evaluates to floating-point -0.0, so a
        // `signum()`-based sign returns -1.0 and yields -PI here, violating the
        // spec. The corrected `< 0.0 -> -1 else +1` rule treats -0.0 as the
        // positive branch and returns +PI. (The reciprocal pair (1,0)->(-1,0)
        // has a +0.0 cross product and is +PI under both rules.)
        let angle = svg_arc_angle(-1.0, 0.0, 1.0, 0.0);
        assert!(
            (angle - std::f64::consts::PI).abs() < 1e-6,
            "collinear-opposite angle should be +PI, got {angle}"
        );

        let reciprocal = svg_arc_angle(1.0, 0.0, -1.0, 0.0);
        assert!(
            (reciprocal - std::f64::consts::PI).abs() < 1e-6,
            "reciprocal collinear-opposite angle should be +PI, got {reciprocal}"
        );
    }

    #[test]
    fn svg_arc_small_semicircle_reaches_apex_both_directions() {
        // A 180-degree arc is the boundary case where the start and end radius
        // vectors are collinear and opposite, so the F.6.5.4 cross product is
        // zero (see svg_arc_angle_collinear_opposite_is_positive_pi). Both sweep
        // directions of `move_to(0,0); svg_arc_to(50,50,0,false,_,100,0)` must
        // trace a genuine semicircle of radius 50 centered at (50, 0) rather than
        // collapsing onto the chord.
        let start = (0.0, 0.0);
        let end = (100.0, 0.0);
        let r = 50.0;
        let center = (50.0, 0.0);

        let semicircle = |sweep: bool| -> (Vec<(f32, f32)>, (f32, f32), (f32, f32)) {
            let mut path = Path::new();
            path.move_to(start.0, start.1);
            path.svg_arc_to(r, r, 0.0, false, sweep, end.0, end.1);
            let (points, endpoint) = sample_path(&path);
            let mid = points[points.len() / 2];
            (points, mid, endpoint)
        };

        let (sweep_pts, sweep_mid, sweep_end) = semicircle(true);
        let (nsweep_pts, nsweep_mid, nsweep_end) = semicircle(false);

        // Both directions must actually reach the requested endpoint.
        assert!((sweep_end.0 - end.0).abs() < 1e-3 && (sweep_end.1 - end.1).abs() < 1e-3);
        assert!((nsweep_end.0 - end.0).abs() < 1e-3 && (nsweep_end.1 - end.1).abs() < 1e-3);

        // The apex of a true semicircle is the chord midpoint offset by the
        // radius perpendicular to the chord: (50, +-50). A collapsed (chord)
        // arc would instead leave the midpoint at (50, 0), so the apex distance
        // from the chord guards against any future regression to a degenerate
        // 180-degree arc. The sweep flag picks the half-plane: sweep=true bulges
        // to -y, sweep=false to +y.
        assert!(
            (sweep_mid.0 - center.0).abs() < 0.5 && (sweep_mid.1 + r).abs() < 0.5,
            "sweep=true semicircle midpoint {sweep_mid:?} should reach apex (50, -50)"
        );
        assert!(
            (nsweep_mid.0 - center.0).abs() < 0.5 && (nsweep_mid.1 - r).abs() < 0.5,
            "sweep=false semicircle midpoint {nsweep_mid:?} should reach apex (50, 50)"
        );

        // The two sweep directions must bulge into opposite half-planes.
        let sweep_side = chord_bulge_side(start, end, sweep_mid);
        let nsweep_side = chord_bulge_side(start, end, nsweep_mid);
        assert!(
            sweep_side * nsweep_side < 0.0,
            "semicircles must bulge to opposite sides: {sweep_side} vs {nsweep_side}"
        );

        // Every sampled point of both arcs lies on the radius-50 circle.
        for (px, py) in sweep_pts.into_iter().chain(nsweep_pts.into_iter()) {
            let resid = ellipse_residual(px, py, center.0, center.1, r, r, 0.0);
            assert!(
                resid.abs() < ELLIPSE_RESIDUAL_TOL,
                "semicircle point ({px}, {py}) off radius-50 circle, residual {resid}"
            );
        }
    }

    #[test]
    fn svg_arc_tiny_radii_scale_up_without_overflow() {
        // Minimized from the deterministic fuzz (iteration 78 of seed
        // 0x5eed_1e57_ab1e_f00d): radii far smaller than the chord must be
        // scaled up per F.6.6, but computing lambda = (x1p/rx)^2 + (y1p/ry)^2
        // in f32 overflows to infinity for rx = ry = 1e-30, which then poisons
        // the center parametrization into NaN control points. The spec-correct
        // result is a half-circle spanning the chord, exactly as if the radii
        // had been given as chord/2.
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.svg_arc_to(1e-30, 1e-30, 0.0, false, true, 100.0, 0.0);

        let (points, endpoint) = sample_path(&path);
        assert!(!points.is_empty());
        assert!((endpoint.0 - 100.0).abs() < 1e-3 && endpoint.1.abs() < 1e-3);
        for (px, py) in points {
            assert!(px.is_finite() && py.is_finite(), "non-finite point ({px}, {py})");
            let residual = ellipse_residual(px, py, 50.0, 0.0, 50.0, 50.0, 0.0);
            assert!(
                residual.abs() < ELLIPSE_RESIDUAL_TOL,
                "tiny-radii point ({px}, {py}) off scaled half-circle, residual {residual}"
            );
        }
    }

    #[test]
    fn svg_arc_huge_coordinates_stay_finite() {
        // Squaring f32-range coordinates in the F.6.5.1 midpoint terms
        // overflows f32 ((1e30)^2 = 1e60 > f32::MAX) even though the resulting
        // arc geometry itself is comfortably representable. The center math
        // must not hand non-finite control points to the path.
        let mut path = Path::new();
        path.move_to(-1e30, 0.0);
        path.svg_arc_to(2e30, 2e30, 0.0, false, true, 1e30, 0.0);

        let (points, endpoint) = sample_path(&path);
        assert!(!points.is_empty());
        for (px, py) in &points {
            assert!(px.is_finite() && py.is_finite(), "non-finite point ({px}, {py})");
        }
        let tolerance = 1e30 * 1e-2;
        assert!((endpoint.0 - 1e30).abs() < tolerance && endpoint.1.abs() < tolerance);
    }

    #[test]
    fn svg_arc_unrepresentable_geometry_degrades_to_chord() {
        // F.6.6 scaling of wildly mismatched radii (rx = 1e30, ry = 1e-30 with
        // a ~141-unit chord) yields an effective rx of ~5e61: the arc's control
        // points exceed f32 range even though both endpoints are finite. The
        // builder must degrade to the chord rather than emit non-finite
        // vertices.
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.svg_arc_to(1e30, 1e-30, 0.0, true, true, 100.0, 100.0);

        let verbs: Vec<_> = path.verbs().collect();
        assert_eq!(verbs.len(), 2, "expected move_to + line_to, got {verbs:?}");
        assert!(matches!(verbs[0], Verb::MoveTo(0.0, 0.0)));
        assert!(matches!(verbs[1], Verb::LineTo(100.0, 100.0)));
    }

    #[test]
    fn svg_arc_non_finite_arguments_leave_path_unchanged() {
        // Canvas-spec rule: path methods return early, adding nothing, when any
        // argument is infinite or NaN. Exercise every float argument position
        // with every non-finite value.
        let bad_values = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];

        for arg_index in 0..5 {
            for &bad in &bad_values {
                let mut args = [50.0, 40.0, 0.3, 90.0, 60.0];
                args[arg_index] = bad;

                let mut path = Path::new();
                path.move_to(10.0, 20.0);
                let before = path.verbs().count();
                path.svg_arc_to(args[0], args[1], args[2], true, false, args[3], args[4]);
                assert_eq!(
                    before,
                    path.verbs().count(),
                    "non-finite arg {arg_index} ({bad}) must add no verbs"
                );

                // The path must remain fully usable afterwards: a follow-up
                // line_to continues from the pre-arc current point.
                path.line_to(70.0, 80.0);
                let verbs: Vec<_> = path.verbs().collect();
                assert_eq!(verbs.len(), before + 1);
                assert!(matches!(verbs.last(), Some(Verb::LineTo(70.0, 80.0))));

                // On an empty path the guard must also fire before the
                // implicit move_to(0, 0) is inserted.
                let mut empty = Path::new();
                empty.svg_arc_to(args[0], args[1], args[2], true, false, args[3], args[4]);
                assert!(
                    empty.is_empty(),
                    "non-finite arg {arg_index} ({bad}) must not add an implicit move_to"
                );
            }
        }
    }

    #[test]
    fn svg_arc_large_semicircle_sweeps_the_long_way() {
        // The SVG path `A 50 50 0 1 1 100 0` from (0,0): rx=ry=50 exactly spans
        // the 100-unit chord, so the large-arc flag still yields a 180-degree
        // semicircle, with the large+sweep flags forcing the long way around.
        // This exercises the same collinear-opposite F.6.5.4 boundary as the
        // small form. The apex must reach (50, -50) and the endpoint (100, 0).
        let start = (0.0, 0.0);
        let end = (100.0, 0.0);
        let r = 50.0;
        let center = (50.0, 0.0);

        let mut path = Path::new();
        path.move_to(start.0, start.1);
        path.svg_arc_to(r, r, 0.0, true, true, end.0, end.1);

        let (points, endpoint) = sample_path(&path);
        assert!(
            (endpoint.0 - end.0).abs() < 1e-3 && (endpoint.1 - end.1).abs() < 1e-3,
            "large-arc semicircle endpoint {endpoint:?} should reach (100, 0)"
        );

        let mid = points[points.len() / 2];
        assert!(
            (mid.0 - center.0).abs() < 0.5 && (mid.1 + r).abs() < 0.5,
            "large-arc semicircle midpoint {mid:?} should reach apex (50, -50)"
        );

        // Total polyline length must be close to a half-circumference (pi*r),
        // confirming it swept ~180 degrees rather than collapsing to the chord.
        let length: f32 = points
            .windows(2)
            .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
            .sum();
        let half_circumference = PI * r;
        assert!(
            (length - half_circumference).abs() < 1.0,
            "swept length {length} should be ~pi*r ({half_circumference})"
        );

        for (px, py) in points {
            let resid = ellipse_residual(px, py, center.0, center.1, r, r, 0.0);
            assert!(
                resid.abs() < ELLIPSE_RESIDUAL_TOL,
                "large semicircle point ({px}, {py}) off radius-50 circle, residual {resid}"
            );
        }
    }
}
