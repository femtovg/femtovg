use std::f32::consts::PI;

use crate::geometry::{self, Transform2D};

mod cache;
pub use cache::{Convexity, PathCache};

// Length proportional to radius of a cubic bezier handle for 90deg arcs.
const KAPPA90: f32 = 0.5522847493;

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub enum Solidity {
    Solid = 1,
    Hole = 2,
}

impl Default for Solidity {
    fn default() -> Self {
        Self::Solid
    }
}

// TODO: Maybe to avoid confusion solid/hole should be true/false as a last param to rect, circle etc

#[derive(Copy, Clone, Debug)]
pub enum Verb {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    BezierTo(f32, f32, f32, f32, f32, f32),
    Close,
    Solidity(Solidity),
}

/// A collection of verbs (MoveTo, LineTo, BezierTo) describing a one or more contours.
#[derive(Default, Clone, Debug)]
pub struct Path {
    transform: Transform2D,
    verbs: Vec<Verb>,
    lastx: f32,
    lasty: f32,
    dist_tol: f32,
    pub(crate) cache: Option<(u64, PathCache)>,
}

impl Path {
    pub fn new() -> Self {
        Self {
            dist_tol: 0.01,
            ..Default::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }

    pub fn set_distance_tolerance(&mut self, value: f32) {
        self.dist_tol = value;
    }

    pub fn verbs(&self) -> impl Iterator<Item = &Verb> {
        self.verbs.iter()
    }

    pub fn verbs_mut(&mut self) -> impl Iterator<Item = &mut Verb> {
        self.verbs.iter_mut()
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
            let path_cache = PathCache::new(&self.verbs, &transform, tess_tol, dist_tol);
            self.cache = Some((key, path_cache));
        }

        &mut self.cache.as_mut().unwrap().1
    }

    // Path funcs

    /// Starts new sub-path with specified point as first point.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.append(&[Verb::MoveTo(x, y)]);
    }

    /// Adds line segment from the last point in the path to the specified point.
    pub fn line_to(&mut self, x: f32, y: f32) {
        self.append(&[Verb::LineTo(x, y)]);
    }

    /// Adds cubic bezier segment from last point in the path via two control points to the specified point.
    pub fn bezier_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.append(&[Verb::BezierTo(c1x, c1y, c2x, c2y, x, y)]);
    }

    /// Adds quadratic bezier segment from last point in the path via a control point to the specified point.
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let x0 = self.lastx;
        let y0 = self.lasty;

        self.append(&[Verb::BezierTo(
            x0 + 2.0 / 3.0 * (cx - x0),
            y0 + 2.0 / 3.0 * (cy - y0),
            x + 2.0 / 3.0 * (cx - x),
            y + 2.0 / 3.0 * (cy - y),
            x,
            y,
        )]);
    }

    /// Closes current sub-path with a line segment.
    pub fn close(&mut self) {
        self.append(&[Verb::Close]);
    }

    /// Sets the current sub-path winding, see Solidity
    pub fn solidity(&mut self, solidity: Solidity) {
        self.append(&[Verb::Solidity(solidity)]);
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

        // TODO: Maybe use small stack vec here
        let mut commands = Vec::with_capacity(ndivs as usize);

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
                    Verb::LineTo(x, y)
                } else {
                    Verb::MoveTo(x, y)
                };
                commands.push(first_move);
            } else {
                commands.push(Verb::BezierTo(px + ptanx, py + ptany, x - tanx, y - tany, x, y));
            }

            px = x;
            py = y;
            ptanx = tanx;
            ptany = tany;
        }

        self.append(&commands);
    }

    /// Adds an arc segment at the corner defined by the last path point, and two specified points.
    pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) {
        if self.verbs.is_empty() {
            return;
        }

        let x0 = self.lastx;
        let y0 = self.lasty;

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
        self.append(&[
            Verb::MoveTo(x, y),
            Verb::LineTo(x, y + h),
            Verb::LineTo(x + w, y + h),
            Verb::LineTo(x + w, y),
            Verb::Close,
        ]);
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

            self.append(&[
                Verb::MoveTo(x, y + ry_tl),
                Verb::LineTo(x, y + h - ry_bl),
                Verb::BezierTo(
                    x,
                    y + h - ry_bl * (1.0 - KAPPA90),
                    x + rx_bl * (1.0 - KAPPA90),
                    y + h,
                    x + rx_bl,
                    y + h,
                ),
                Verb::LineTo(x + w - rx_br, y + h),
                Verb::BezierTo(
                    x + w - rx_br * (1.0 - KAPPA90),
                    y + h,
                    x + w,
                    y + h - ry_br * (1.0 - KAPPA90),
                    x + w,
                    y + h - ry_br,
                ),
                Verb::LineTo(x + w, y + ry_tr),
                Verb::BezierTo(
                    x + w,
                    y + ry_tr * (1.0 - KAPPA90),
                    x + w - rx_tr * (1.0 - KAPPA90),
                    y,
                    x + w - rx_tr,
                    y,
                ),
                Verb::LineTo(x + rx_tl, y),
                Verb::BezierTo(
                    x + rx_tl * (1.0 - KAPPA90),
                    y,
                    x,
                    y + ry_tl * (1.0 - KAPPA90),
                    x,
                    y + ry_tl,
                ),
                Verb::Close,
            ]);
        }
    }

    /// Creates new ellipse shaped sub-path.
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32) {
        self.append(&[
            Verb::MoveTo(cx - rx, cy),
            Verb::BezierTo(cx - rx, cy + ry * KAPPA90, cx - rx * KAPPA90, cy + ry, cx, cy + ry),
            Verb::BezierTo(cx + rx * KAPPA90, cy + ry, cx + rx, cy + ry * KAPPA90, cx + rx, cy),
            Verb::BezierTo(cx + rx, cy - ry * KAPPA90, cx + rx * KAPPA90, cy - ry, cx, cy - ry),
            Verb::BezierTo(cx - rx * KAPPA90, cy - ry, cx - rx, cy - ry * KAPPA90, cx - rx, cy),
            Verb::Close,
        ]);
    }

    /// Creates new circle shaped sub-path.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32) {
        self.ellipse(cx, cy, r, r);
    }

    /// Appends a slice of verbs to the path
    pub fn append(&mut self, verbs: &[Verb]) {
        for cmd in verbs.iter().rev() {
            match cmd {
                Verb::MoveTo(x, y) => {
                    self.lastx = *x;
                    self.lasty = *y;
                    break;
                }
                Verb::LineTo(x, y) => {
                    self.lastx = *x;
                    self.lasty = *y;
                    break;
                }
                Verb::BezierTo(_c1x, _c1y, _c2x, _c2y, x, y) => {
                    self.lastx = *x;
                    self.lasty = *y;
                    break;
                }
                _ => (),
            }
        }

        self.verbs.extend_from_slice(verbs);
    }
}

impl owned_ttf_parser::OutlineBuilder for Path {
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
