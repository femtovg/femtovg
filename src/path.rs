use std::f32::consts::PI;
use std::slice;

use crate::geometry::{self, Transform2D};
use crate::vector::Vector;

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
            Self::MoveTo(..) => 1,
            Self::LineTo(..) => 1,
            Self::BezierTo(..) => 3,
            Self::Solid => 0,
            Self::Hole => 0,
            Self::Close => 0,
        }
    }

    fn from_packed(packed: &PackedVerb, coords: &[Vector]) -> Self {
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
    coords: Vec<Vector>,
    last_pos: Vector,
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
        self.append(&[PackedVerb::MoveTo], &[Vector { x, y }]);
    }

    /// Adds line segment from the last point in the path to the specified point.
    pub fn line_to(&mut self, x: f32, y: f32) {
        self.append(&[PackedVerb::LineTo], &[Vector { x, y }]);
    }

    /// Adds cubic bezier segment from last point in the path via two control points to the specified point.
    pub fn bezier_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.append(
            &[PackedVerb::BezierTo],
            &[Vector { x: c1x, y: c1y }, Vector { x: c2x, y: c2y }, Vector { x, y }],
        );
    }

    /// Adds quadratic bezier segment from last point in the path via a control point to the specified point.
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let pos0 = self.last_pos;
        let cpos = Vector { x: cx, y: cy };
        let pos = Vector { x, y };
        let pos1 = pos0 + (cpos - pos0) * (2.0 / 3.0);
        let pos2 = pos + (cpos - pos) * (2.0 / 3.0);

        self.append(&[PackedVerb::BezierTo], &[pos1, pos2, pos]);
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
        let cpos = Vector { x: cx, y: cy };

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

        let (mut ppos, mut ptanpos) = (Vector { x: 0f32, y: 0f32 }, Vector { x: 0f32, y: 0f32 });

        for i in 0..=ndivs {
            let a = a0 + da * (i as f32 / ndivs as f32);
            let dpos = Vector::from_angle(a);
            let pos = cpos + dpos * r;
            let tanpos = -dpos.orthogonal() * r * kappa;

            if i == 0 {
                let first_move = if !self.verbs.is_empty() {
                    PackedVerb::LineTo
                } else {
                    PackedVerb::MoveTo
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

    /// Adds an arc segment at the corner defined by the last path point, and two specified points.
    pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) {
        if self.verbs.is_empty() {
            return;
        }

        let pos0 = self.last_pos;
        let pos1 = Vector { x: x1, y: y1 };
        let pos2 = Vector { x: x2, y: y2 };

        // Handle degenerate cases.
        if geometry::pt_equals(pos0.x, pos0.y, pos1.x, pos1.y, self.dist_tol)
            || geometry::pt_equals(pos1.x, pos1.y, pos2.x, pos2.y, self.dist_tol)
            || geometry::dist_pt_segment(pos1.x, pos1.y, pos0.x, pos0.y, pos2.x, pos2.y) < self.dist_tol * self.dist_tol
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

        if geometry::cross(dpos0.x, dpos0.y, dpos1.x, dpos1.y) > 0.0 {
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
            &{
                let hoffset = Vector { x: w, y: 0.0 };
                let voffset = Vector { x: 0.0, y: h };

                let tl = Vector { x, y };
                let tr = tl + hoffset;
                let br = tr + voffset;
                let bl = tl + voffset;

                [tl, tr, br, bl]
            },
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
                    Vector { x, y: y + ry_tl },
                    Vector { x, y: y + h - ry_bl },
                    //
                    Vector {
                        x,
                        y: y + h - ry_bl * (1.0 - KAPPA90),
                    },
                    Vector {
                        x: x + rx_bl * (1.0 - KAPPA90),
                        y: y + h,
                    },
                    Vector { x: x + rx_bl, y: y + h },
                    //
                    Vector {
                        x: x + w - rx_br,
                        y: y + h,
                    },
                    //
                    Vector {
                        x: x + w - rx_br * (1.0 - KAPPA90),
                        y: y + h,
                    },
                    Vector {
                        x: x + w,
                        y: y + h - ry_br * (1.0 - KAPPA90),
                    },
                    Vector {
                        x: x + w,
                        y: y + h - ry_br,
                    },
                    //
                    Vector { x: x + w, y: y + ry_tr },
                    //
                    Vector {
                        x: x + w,
                        y: y + ry_tr * (1.0 - KAPPA90),
                    },
                    Vector {
                        x: x + w - rx_tr * (1.0 - KAPPA90),
                        y,
                    },
                    Vector { x: x + w - rx_tr, y },
                    //
                    Vector { x: x + rx_tl, y },
                    //
                    Vector {
                        x: x + rx_tl * (1.0 - KAPPA90),
                        y,
                    },
                    Vector {
                        x,
                        y: y + ry_tl * (1.0 - KAPPA90),
                    },
                    Vector { x, y: y + ry_tl },
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
            &{
                let cpos = Vector { x: cx, y: cy };
                let hoffset = Vector { x: rx, y: 0.0 };
                let voffset = Vector { x: 0.0, y: ry };
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

    /// Creates new circle shaped sub-path.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32) {
        self.ellipse(cx, cy, r, r);
    }

    /// Appends a slice of verbs to the path
    fn append(&mut self, verbs: &[PackedVerb], coords: &[Vector]) {
        if coords.len() > 1 {
            self.last_pos = coords[coords.len() - 1];
        }

        self.verbs.extend_from_slice(verbs);
        self.coords.extend_from_slice(coords);
    }
}

pub struct PathIter<'a> {
    verbs: slice::Iter<'a, PackedVerb>,
    coords: &'a [Vector],
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
