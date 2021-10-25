use std::f32::consts::PI;
use std::slice;

use crate::geometry::{self, Transform2D};
use crate::position::Position;

mod cache;
pub use cache::{Convexity, PathCache};

// Length proportional to radius of a cubic bezier handle for 90deg arcs.
const KAPPA90: f32 = 0.5522847493;

/// Used to specify Solid/Hole when adding shapes to a path.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd)]
pub enum Solidity {
    Solid = 1,
    Hole = 2,
}

impl Default for Solidity {
    fn default() -> Self {
        Self::Solid
    }
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

#[derive(Copy, Clone, Debug)]
pub enum Verb {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    BezierTo(f32, f32, f32, f32, f32, f32),
    Solid,
    Hole,
    Close,
}

impl Verb {
    fn num_coordinates(&self) -> usize {
        match *self {
            Self::MoveTo(..) => 2,
            Self::LineTo(..) => 2,
            Self::BezierTo(..) => 6,
            Self::Solid => 0,
            Self::Hole => 0,
            Self::Close => 0,
        }
    }

    fn from_packed(packed: &PackedVerb, coords: &[f32]) -> Self {
        match *packed {
            PackedVerb::MoveTo => Self::MoveTo(coords[0], coords[1]),
            PackedVerb::LineTo => Self::LineTo(coords[0], coords[1]),
            PackedVerb::BezierTo => Self::BezierTo(coords[0], coords[1], coords[2], coords[3], coords[4], coords[5]),
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
    coords: Vec<f32>,
    last_pos: Position,
    dist_tol: f32,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) cache: Option<(u64, PathCache)>,
}

impl Path {
    pub fn new() -> Self {
        Self {
            dist_tol: 0.01,
            ..Default::default()
        }
    }

    /// Memory usage in bytes
    pub fn size(&self) -> usize {
        std::mem::size_of::<PackedVerb>() * self.verbs.len() + std::mem::size_of::<f32>() * self.coords.len()
    }

    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }

    pub fn set_distance_tolerance(&mut self, value: f32) {
        self.dist_tol = value;
    }

    pub fn verbs(&self) -> PathIter<'_> {
        PathIter {
            verbs: self.verbs.iter(),
            coords: &self.coords,
        }
    }

    pub(crate) fn cache<'a>(&'a mut self, transform: &Transform2D, tess_tol: f32, dist_tol: f32) -> &'a mut PathCache {
        // The path cache saves a flattened and transformed version of the path. If client code calls
        // (fill|stroke)_path repeatedly with the same Path under the same transform circumstances then it will be
        // retrieved from cache. I'm not sure if transform.cache_key() is actually good enough for this
        // and if it will produce the correct cache keys under different float edge cases.

        let key = transform.cache_key();

        // this shouldn't need a bool once non lexic lifetimes are stable
        let mut needs_rebuild = true;

        if let Some((transform_cache_key, _cache)) = self.cache.as_ref() {
            needs_rebuild = key != *transform_cache_key;
        }

        if needs_rebuild {
            let path_cache = PathCache::new(self.verbs(), &transform, tess_tol, dist_tol);
            self.cache = Some((key, path_cache));
        }

        &mut self.cache.as_mut().unwrap().1
    }

    // Path funcs

    /// Starts new sub-path with specified point as first point.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.append(&[PackedVerb::MoveTo], &[x, y]);
    }

    /// Adds line segment from the last point in the path to the specified point.
    pub fn line_to(&mut self, x: f32, y: f32) {
        self.append(&[PackedVerb::LineTo], &[x, y]);
    }

    /// Adds cubic bezier segment from last point in the path via two control points to the specified point.
    pub fn bezier_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.append(&[PackedVerb::BezierTo], &[c1x, c1y, c2x, c2y, x, y]);
    }

    /// Adds quadratic bezier segment from last point in the path via a control point to the specified point.
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let Position { x: x0, y: y0 } = self.last_pos;

        self.append(
            &[PackedVerb::BezierTo],
            &[
                x0 + 2.0 / 3.0 * (cx - x0),
                y0 + 2.0 / 3.0 * (cy - y0),
                x + 2.0 / 3.0 * (cx - x),
                y + 2.0 / 3.0 * (cy - y),
                x,
                y,
            ],
        );
    }

    /// Closes current sub-path with a line segment.
    pub fn close(&mut self) {
        self.append(&[PackedVerb::Close], &[]);
    }

    /// Sets the current sub-path winding, see Solidity
    pub fn solidity(&mut self, solidity: Solidity) {
        match solidity {
            Solidity::Solid => self.append(&[PackedVerb::Solid], &[]),
            Solidity::Hole => self.append(&[PackedVerb::Hole], &[]),
        }
    }

    /// Creates new circle arc shaped sub-path. The arc center is at cx,cy, the arc radius is r,
    /// and the arc is drawn from angle a0 to a1, and swept in direction dir (Winding)
    /// Angles are specified in radians.
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, dir: Solidity) {
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
        let ndivs = ((da.abs() / (PI * 0.5) + 0.5) as i32).min(5).max(1);
        let hda = (da / ndivs as f32) / 2.0;
        let mut kappa = (4.0 / 3.0 * (1.0 - hda.cos()) / hda.sin()).abs();

        let mut commands = Vec::with_capacity(ndivs as usize);
        let mut coords = Vec::with_capacity(ndivs as usize);

        if dir == Solidity::Solid {
            kappa = -kappa;
        }

        let (mut px, mut py, mut ptanx, mut ptany) = (0f32, 0f32, 0f32, 0f32);

        for i in 0..=ndivs {
            let a = a0 + da * (i as f32 / ndivs as f32);
            let dx = a.cos();
            let dy = a.sin();
            let x = cx + dx * r;
            let y = cy + dy * r;
            let tanx = -dy * r * kappa;
            let tany = dx * r * kappa;

            if i == 0 {
                let first_move = if !self.verbs.is_empty() {
                    PackedVerb::LineTo
                } else {
                    PackedVerb::MoveTo
                };

                commands.push(first_move);
                coords.extend_from_slice(&[x, y]);
            } else {
                commands.push(PackedVerb::BezierTo);
                coords.extend_from_slice(&[px + ptanx, py + ptany, x - tanx, y - tany, x, y]);
            }

            px = x;
            py = y;
            ptanx = tanx;
            ptany = tany;
        }

        self.append(&commands, &coords);
    }

    /// Adds an arc segment at the corner defined by the last path point, and two specified points.
    pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) {
        if self.verbs.is_empty() {
            return;
        }

        let Position { x: x0, y: y0 } = self.last_pos;

        // Handle degenerate cases.
        if geometry::pt_equals(x0, y0, x1, y1, self.dist_tol)
            || geometry::pt_equals(x1, y1, x2, y2, self.dist_tol)
            || geometry::dist_pt_segment(x1, y1, x0, y0, x2, y2) < self.dist_tol * self.dist_tol
            || radius < self.dist_tol
        {
            self.line_to(x1, y1);
        }

        let mut dx0 = x0 - x1;
        let mut dy0 = y0 - y1;
        let mut dx1 = x2 - x1;
        let mut dy1 = y2 - y1;

        geometry::normalize(&mut dx0, &mut dy0);
        geometry::normalize(&mut dx1, &mut dy1);

        let a = (dx0 * dx1 + dy0 * dy1).acos();
        let d = radius / (a / 2.0).tan();

        if d > 10000.0 {
            return self.line_to(x1, y1);
        }

        let (cx, cy, a0, a1, dir);

        if geometry::cross(dx0, dy0, dx1, dy1) > 0.0 {
            cx = x1 + dx0 * d + dy0 * radius;
            cy = y1 + dy0 * d + -dx0 * radius;
            a0 = dx0.atan2(-dy0);
            a1 = -dx1.atan2(dy1);
            dir = Solidity::Hole;
        } else {
            cx = x1 + dx0 * d + -dy0 * radius;
            cy = y1 + dy0 * d + dx0 * radius;
            a0 = -dx0.atan2(dy0);
            a1 = dx1.atan2(-dy1);
            dir = Solidity::Solid;
        }

        self.arc(cx, cy, radius, a0, a1, dir);
    }

    /// Creates new rectangle shaped sub-path.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.append(
            &[
                PackedVerb::MoveTo,
                PackedVerb::LineTo,
                PackedVerb::LineTo,
                PackedVerb::LineTo,
                PackedVerb::Close,
            ],
            &[x, y, x, y + h, x + w, y + h, x + w, y],
        );
    }

    /// Creates new rounded rectangle shaped sub-path.
    pub fn rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        self.rounded_rect_varying(x, y, w, h, r, r, r, r);
    }

    /// Creates new rounded rectangle shaped sub-path with varying radii for each corner.
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
                    x,
                    y + ry_tl,
                    x,
                    y + h - ry_bl,
                    //
                    x,
                    y + h - ry_bl * (1.0 - KAPPA90),
                    x + rx_bl * (1.0 - KAPPA90),
                    y + h,
                    x + rx_bl,
                    y + h,
                    //
                    x + w - rx_br,
                    y + h,
                    //
                    x + w - rx_br * (1.0 - KAPPA90),
                    y + h,
                    x + w,
                    y + h - ry_br * (1.0 - KAPPA90),
                    x + w,
                    y + h - ry_br,
                    //
                    x + w,
                    y + ry_tr,
                    //
                    x + w,
                    y + ry_tr * (1.0 - KAPPA90),
                    x + w - rx_tr * (1.0 - KAPPA90),
                    y,
                    x + w - rx_tr,
                    y,
                    //
                    x + rx_tl,
                    y,
                    //
                    x + rx_tl * (1.0 - KAPPA90),
                    y,
                    x,
                    y + ry_tl * (1.0 - KAPPA90),
                    x,
                    y + ry_tl,
                ],
            );
        }
    }

    /// Creates new ellipse shaped sub-path.
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
            &[
                cx - rx,
                cy,
                cx - rx,
                cy + ry * KAPPA90,
                cx - rx * KAPPA90,
                cy + ry,
                cx,
                cy + ry,
                cx + rx * KAPPA90,
                cy + ry,
                cx + rx,
                cy + ry * KAPPA90,
                cx + rx,
                cy,
                cx + rx,
                cy - ry * KAPPA90,
                cx + rx * KAPPA90,
                cy - ry,
                cx,
                cy - ry,
                cx - rx * KAPPA90,
                cy - ry,
                cx - rx,
                cy - ry * KAPPA90,
                cx - rx,
                cy,
            ],
        );
    }

    /// Creates new circle shaped sub-path.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32) {
        self.ellipse(cx, cy, r, r);
    }

    /// Appends a slice of verbs to the path
    fn append(&mut self, verbs: &[PackedVerb], coords: &[f32]) {
        if coords.len() > 1 {
            let x = coords[coords.len() - 2];
            let y = coords[coords.len() - 1];
            self.last_pos = Position { x, y };
        }

        self.verbs.extend_from_slice(verbs);
        self.coords.extend_from_slice(coords);
    }
}

pub struct PathIter<'a> {
    verbs: slice::Iter<'a, PackedVerb>,
    coords: &'a [f32],
}

impl<'a> Iterator for PathIter<'a> {
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
