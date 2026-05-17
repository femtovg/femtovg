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
    use super::{Path, Verb};

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
}
