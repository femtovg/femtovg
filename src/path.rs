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

/// A verb describes how to interpret one or more points to continue the contour
/// of a [`Path`].
#[derive(Copy, Clone, Debug)]
pub enum Verb<const N: usize = 2> {
    /// Terminates the current sub-path and defines the new current point.
    MoveTo([f32; N]),
    /// Describes that the contour of the path should continue as a line from the
    /// current point to the given point.
    LineTo([f32; N]),
    /// Describes that the contour of the path should continue as a cubic bezier segment from the
    /// current point via two control points to the endpoint.
    BezierTo([f32; N], [f32; N], [f32; N]),
    /// Sets the current sub-path winding to be solid.
    Solid,
    /// Sets the current sub-path winding to be hole.
    Hole,
    /// Closes the current sub-path.
    Close,
}

impl<const N: usize> Verb<N> {
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

    fn from_packed(packed: &PackedVerb, coords: &[[f32; N]]) -> Self {
        match *packed {
            PackedVerb::MoveTo => Self::MoveTo(coords[0]),
            PackedVerb::LineTo => Self::LineTo(coords[0]),
            PackedVerb::BezierTo => Self::BezierTo(coords[0], coords[1], coords[2]),
            PackedVerb::Solid => Self::Solid,
            PackedVerb::Hole => Self::Hole,
            PackedVerb::Close => Self::Close,
        }
    }
}

/// A collection of verbs (`move_to()`, `line_to()`, `bezier_to()`, etc.)
/// describing one or more contours.
///
/// The const generic `N` specifies the number of dimensions (default 2).
/// Use `Path` (or `Path<2>`) for standard 2D drawing, and `Path<3>` for 3D
/// geometry that can be projected to 2D via [`map()`](Path::map).
///
/// # 3D Example
/// ```
/// use femtovg::Path;
///
/// let mut path3d = Path::<3>::new();
/// path3d.move_to([0.0, 0.0, 0.0]);
/// path3d.line_to([1.0, 1.0, 1.0]);
/// path3d.bezier_to([0.5, 0.0, 0.5], [0.5, 1.0, 0.5], [1.0, 1.0, 0.0]);
/// path3d.close();
///
/// let path2d = path3d.map(|[x, y, z]| {
///     let scale = 400.0 / (400.0 + z);
///     [x * scale, y * scale]
/// });
/// ```
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Path<const N: usize = 2> {
    verbs: Vec<PackedVerb>,
    coords: Vec<[f32; N]>,
    last_pos: [f32; N],
    dist_tol: f32,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) cache: RefCell<Option<(u64, PathCache)>>,
}

impl<const N: usize> Default for Path<N> {
    fn default() -> Self {
        Self {
            verbs: Vec::new(),
            coords: Vec::new(),
            last_pos: [0.0; N],
            dist_tol: 0.01,
            cache: RefCell::new(None),
        }
    }
}

impl<const N: usize> Path<N> {
    /// Creates a new empty path with a distance tolerance of 0.01.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the memory size in bytes used by the path.
    pub fn size(&self) -> usize {
        std::mem::size_of::<PackedVerb>() * self.verbs.len() + std::mem::size_of::<[f32; N]>() * self.coords.len()
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
    pub fn verbs(&self) -> PathIter<'_, N> {
        PathIter {
            verbs: self.verbs.iter(),
            coords: &self.coords,
        }
    }

    /// Starts a new sub-path with the specified point as the first point.
    pub fn move_to(&mut self, pos: impl Into<[f32; N]>) {
        let pos = pos.into();
        self.append(&[PackedVerb::MoveTo], &[pos]);
    }

    /// Adds a line segment from the last point in the path to the specified point.
    pub fn line_to(&mut self, pos: impl Into<[f32; N]>) {
        let pos = pos.into();
        self.append(&[PackedVerb::LineTo], &[pos]);
    }

    /// Adds a cubic bezier segment from the last point in the path via two control points to the specified point.
    pub fn bezier_to(
        &mut self,
        control1: impl Into<[f32; N]>,
        control2: impl Into<[f32; N]>,
        pos: impl Into<[f32; N]>,
    ) {
        self.append(&[PackedVerb::BezierTo], &[control1.into(), control2.into(), pos.into()]);
    }

    /// Adds a quadratic bezier segment from the last point in the path via a control point to the specified point.
    pub fn quad_to(&mut self, control: impl Into<[f32; N]>, pos: impl Into<[f32; N]>) {
        let control = control.into();
        let pos = pos.into();
        let pos0 = self.last_pos;
        let pos1 = std::array::from_fn(|i| pos0[i] + (control[i] - pos0[i]) * (2.0 / 3.0));
        let pos2 = std::array::from_fn(|i| pos[i] + (control[i] - pos[i]) * (2.0 / 3.0));
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

    /// Transforms all coordinates using the given function, producing a path in a
    /// (potentially different) number of dimensions. This is useful for projecting
    /// 3D paths to 2D for rendering.
    ///
    /// ```
    /// use femtovg::Path;
    ///
    /// let mut path3d = Path::<3>::new();
    /// path3d.move_to([10.0, 20.0, 30.0]);
    /// path3d.line_to([40.0, 50.0, 60.0]);
    ///
    /// let path2d: Path<2> = path3d.map(|[x, y, _z]| [x, y]);
    /// ```
    pub fn map<const M: usize>(&self, f: impl Fn([f32; N]) -> [f32; M]) -> Path<M> {
        Path {
            verbs: self.verbs.clone(),
            coords: self.coords.iter().map(|c| f(*c)).collect(),
            last_pos: f(self.last_pos),
            dist_tol: self.dist_tol,
            cache: RefCell::new(None),
        }
    }

    fn append(&mut self, verbs: &[PackedVerb], coords: &[[f32; N]]) {
        if !coords.is_empty() {
            self.last_pos = coords[coords.len() - 1];
        }

        self.verbs.extend_from_slice(verbs);
        self.coords.extend_from_slice(coords);
    }
}

impl Path<2> {
    pub(crate) fn cache<'a>(&'a self, transform: &Transform2D, tess_tol: f32, dist_tol: f32) -> RefMut<'a, PathCache> {
        let key = transform.cache_key();

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

    /// Creates new circle arc shaped sub-path. The arc center is at `center`, the arc radius is `r`,
    /// and the arc is drawn from angle `a0` to `a1`, and swept in direction `dir` (Winding)
    /// Angles are specified in radians.
    pub fn arc(&mut self, center: impl Into<[f32; 2]>, r: f32, a0: f32, a1: f32, dir: Solidity) {
        let [cx, cy] = center.into();
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

        if dir == Solidity::Solid {
            kappa = -kappa;
        }

        let (mut ppos, mut ptanpos) = (Position { x: 0.0, y: 0.0 }, Vector::zero());

        let mut pos_coords: Vec<Position> = Vec::with_capacity(ndivs as usize);

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
                pos_coords.push(pos);
            } else {
                commands.push(PackedVerb::BezierTo);
                pos_coords.extend_from_slice(&[ppos + ptanpos, pos - tanpos, pos]);
            }

            ppos = pos;
            ptanpos = tanpos;
        }

        let coords: Vec<[f32; 2]> = pos_coords.into_iter().map(Into::into).collect();
        self.append(&commands, &coords);
    }

    /// Adds an arc segment at the corner defined by the last path point and two specified points.
    pub fn arc_to(&mut self, pos1: impl Into<[f32; 2]>, pos2: impl Into<[f32; 2]>, radius: f32) {
        if self.verbs.is_empty() {
            return;
        }

        let pos0: Position = self.last_pos.into();
        let pos1: Position = pos1.into().into();
        let pos2: Position = pos2.into().into();

        if Position::equals(pos0, pos1, self.dist_tol)
            || Position::equals(pos1, pos2, self.dist_tol)
            || Position::segment_distance(pos1, pos0, pos2) < self.dist_tol * self.dist_tol
            || radius < self.dist_tol
        {
            self.line_to(<[f32; 2]>::from(pos1));
            return;
        }

        let mut dpos0 = pos0 - pos1;
        let mut dpos1 = pos2 - pos1;

        dpos0.normalize();
        dpos1.normalize();

        let a = dpos0.dot(dpos1).acos();
        let d = radius / (a / 2.0).tan();

        if d > 10000.0 {
            self.line_to(<[f32; 2]>::from(pos1));
            return;
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

        self.arc(<[f32; 2]>::from(cpos), radius, a0 + PI / 2.0, a1 + PI / 2.0, dir);
    }

    /// Creates a new rectangle shaped sub-path.
    pub fn rect(&mut self, pos: impl Into<[f32; 2]>, size: impl Into<[f32; 2]>) {
        let [x, y] = pos.into();
        let [w, h] = size.into();
        self.append(
            &[
                PackedVerb::MoveTo,
                PackedVerb::LineTo,
                PackedVerb::LineTo,
                PackedVerb::LineTo,
                PackedVerb::Close,
            ],
            &[[x, y], [x, y + h], [x + w, y + h], [x + w, y]],
        );
    }

    /// Creates a new rounded rectangle shaped sub-path.
    pub fn rounded_rect(&mut self, pos: impl Into<[f32; 2]>, size: impl Into<[f32; 2]>, r: f32) {
        let pos = pos.into();
        let size = size.into();
        self.rounded_rect_varying(pos, size, r, r, r, r);
    }

    /// Creates a new rounded rectangle shaped sub-path with varying radii for each corner.
    pub fn rounded_rect_varying(
        &mut self,
        pos: impl Into<[f32; 2]>,
        size: impl Into<[f32; 2]>,
        rad_top_left: f32,
        rad_top_right: f32,
        rad_bottom_right: f32,
        rad_bottom_left: f32,
    ) {
        let [x, y] = pos.into();
        let [w, h] = size.into();

        if rad_top_left < 0.1 && rad_top_right < 0.1 && rad_bottom_right < 0.1 && rad_bottom_left < 0.1 {
            self.rect([x, y], [w, h]);
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
                    [x, y + ry_tl],
                    [x, y + h - ry_bl],
                    [x, y + h - ry_bl * (1.0 - KAPPA90)],
                    [x + rx_bl * (1.0 - KAPPA90), y + h],
                    [x + rx_bl, y + h],
                    [x + w - rx_br, y + h],
                    [x + w - rx_br * (1.0 - KAPPA90), y + h],
                    [x + w, y + h - ry_br * (1.0 - KAPPA90)],
                    [x + w, y + h - ry_br],
                    [x + w, y + ry_tr],
                    [x + w, y + ry_tr * (1.0 - KAPPA90)],
                    [x + w - rx_tr * (1.0 - KAPPA90), y],
                    [x + w - rx_tr, y],
                    [x + rx_tl, y],
                    [x + rx_tl * (1.0 - KAPPA90), y],
                    [x, y + ry_tl * (1.0 - KAPPA90)],
                    [x, y + ry_tl],
                ],
            );
        }
    }

    /// Creates a new ellipse shaped sub-path.
    pub fn ellipse(&mut self, center: impl Into<[f32; 2]>, radii: impl Into<[f32; 2]>) {
        let [cx, cy] = center.into();
        let [rx, ry] = radii.into();
        let kx = rx * KAPPA90;
        let ky = ry * KAPPA90;
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
                [cx - rx, cy],
                [cx - rx, cy + ky],
                [cx - kx, cy + ry],
                [cx, cy + ry],
                [cx + kx, cy + ry],
                [cx + rx, cy + ky],
                [cx + rx, cy],
                [cx + rx, cy - ky],
                [cx + kx, cy - ry],
                [cx, cy - ry],
                [cx - kx, cy - ry],
                [cx - rx, cy - ky],
                [cx - rx, cy],
            ],
        );
    }

    /// Creates a new circle shaped sub-path.
    pub fn circle(&mut self, center: impl Into<[f32; 2]>, r: f32) {
        let center = center.into();
        self.ellipse(center, [r, r]);
    }
}

/// An iterator over the verbs and coordinates of a path.
#[derive(Debug)]
pub struct PathIter<'a, const N: usize = 2> {
    verbs: slice::Iter<'a, PackedVerb>,
    coords: &'a [[f32; N]],
}

impl<const N: usize> Iterator for PathIter<'_, N> {
    type Item = Verb<N>;

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
        self.move_to([x, y]);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.line_to([x, y]);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.quad_to([x1, y1], [x, y]);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.bezier_to([x1, y1], [x2, y2], [x, y]);
    }

    fn close(&mut self) {
        self.close();
    }
}
