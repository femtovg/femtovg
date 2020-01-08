
use std::f32::consts::PI;

use bitflags::bitflags;

use crate::math::{self, Transform2D};
use crate::{Vertex, Contour, Command, Winding, LineCap, LineJoin};

const KAPPA90: f32 = 0.5522847493; // Length proportional to radius of a cubic bezier handle for 90deg arcs.

// Point flags
bitflags! {
    #[derive(Default)]
    struct PointFlags: u8 {
        const CORNER        = 0x01;
        const LEFT          = 0x02;
        const BEVEL         = 0x04;
        const INNERBEVEL    = 0x08;
    }
}

#[derive(Copy, Clone, Debug, Default)]
struct Point {
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    len: f32,
    dmx: f32,
    dmy: f32,
    flags: PointFlags
}

impl Point {

    pub fn poly_area(points: &[Point]) -> f32 {
        let mut area = 0.0;

        for i in 2..points.len() {
            let p0 = points[0];
            let p1 = points[i-1];
            let p2 = points[i];

            area += math::triarea2(p0.x, p0.y, p1.x, p1.y, p2.x, p2.y);
        }

        area * 0.5
    }

}

#[derive(Clone, Default)]
pub struct Path {
    pub(crate) commands: Vec<Command>,
    commandx: f32,
    commandy: f32,
    dist_tol: f32
}

impl Path {

    pub fn new() -> Self {
        Self {
            commands: Default::default(),
            commandx: Default::default(),
            commandy: Default::default(),
            dist_tol: 0.01,
        }
    }

    /// Returns iterator over Commands
    fn commands<'a>(&'a self) -> impl Iterator<Item = &'a Command> {
        self.commands.iter()
    }

    /// Starts new sub-path with specified point as first point.
    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.append_commands(&[Command::MoveTo(x, y)]);
        self
    }

    /// Adds line segment from the last point in the path to the specified point.
    pub fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.append_commands(&[Command::LineTo(x, y)]);
        self
    }

    /// Adds cubic bezier segment from last point in the path via two control points to the specified point.
    pub fn bezier_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) -> &mut Self {
        self.append_commands(&[Command::BezierTo(c1x, c1y, c2x, c2y, x, y)]);
        self
    }

    /// Adds quadratic bezier segment from last point in the path via a control point to the specified point.
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) -> &mut Self {
        let x0 = self.commandx;
        let y0 = self.commandy;

        self.append_commands(&[
            Command::BezierTo(
                x0 + 2.0/3.0*(cx - x0), y0 + 2.0/3.0*(cy - y0),
                x + 2.0/3.0*(cx - x), y + 2.0/3.0*(cy - y),
                x, y
            )
        ]);

        self
    }

    /// Closes current sub-path with a line segment.
    pub fn close(&mut self) -> &mut Self {
        self.append_commands(&[Command::Close]);
        self
    }

    /// Sets the current sub-path winding, see Winding and Solidity
    pub fn set_winding(&mut self, winding: Winding) -> &mut Self {
        self.append_commands(&[Command::Winding(winding)]);
        self
    }

    /// Creates new circle arc shaped sub-path. The arc center is at cx,cy, the arc radius is r,
    /// and the arc is drawn from angle a0 to a1, and swept in direction dir (Winding)
    /// Angles are specified in radians.
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, dir: Winding) -> &mut Self {
        // TODO: Maybe use small stack vec here
        let mut commands = Vec::new();

        let mut da = a1 - a0;

        if dir == Winding::CW {
            if da.abs() >= PI * 2.0 {
                da = PI * 2.0;
            } else {
                while da < 0.0 { da += PI * 2.0 }
            }
        } else {
            if da.abs() >= PI * 2.0 {
                da = -PI * 2.0;
            } else {
                while da > 0.0 { da -= PI * 2.0 }
            }
        }

        // Split arc into max 90 degree segments.
        let ndivs = ((da.abs() / (PI * 0.5) + 0.5) as i32).min(5).max(1);
        let hda = (da / ndivs as f32) / 2.0;
        let mut kappa = (4.0 / 3.0 * (1.0 - hda.cos()) / hda.sin()).abs();

        if dir == Winding::CCW {
            kappa = -kappa;
        }

        let (mut px, mut py, mut ptanx, mut ptany) = (0f32, 0f32, 0f32, 0f32);

        for i in 0..=ndivs {
            let a = a0 + da * (i as f32 / ndivs as f32);
            let dx = a.cos();
            let dy = a.sin();
            let x = cx + dx*r;
            let y = cy + dy*r;
            let tanx = -dy*r*kappa;
            let tany = dx*r*kappa;

            if i == 0 {
                let first_move = if self.commands.len() > 0 { Command::LineTo(x, y) } else { Command::MoveTo(x, y) };
                commands.push(first_move);
            } else {
                commands.push(Command::BezierTo(px+ptanx, py+ptany, x-tanx, y-tany, x, y));
            }

            px = x;
            py = y;
            ptanx = tanx;
            ptany = tany;
        }

        self.append_commands(&mut commands);

        self
    }

    /// Adds an arc segment at the corner defined by the last path point, and two specified points.
    pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) -> &mut Self {
        if self.commands.len() == 0 {
            return self;
        }

        let x0 = self.commandx;
        let y0 = self.commandy;

        // Handle degenerate cases.
        if math::pt_equals(x0, y0, x1, y1, self.dist_tol) ||
            math::pt_equals(x1, y1, x2, y2, self.dist_tol) ||
            math::dist_pt_segment(x1, y1, x0, y0, x2, y2) < self.dist_tol * self.dist_tol ||
            radius < self.dist_tol {
            return self.line_to(x1, y1);
        }

        let mut dx0 = x0 - x1;
        let mut dy0 = y0 - y1;
        let mut dx1 = x2 - x1;
        let mut dy1 = y2 - y1;

        math::normalize(&mut dx0, &mut dy0);
        math::normalize(&mut dx1, &mut dy1);

        let a = (dx0*dx1 + dy0*dy1).acos();
        let d = radius / (a/2.0).tan();

        if d > 10000.0 {
            return self.line_to(x1, y1);
        }

        let (cx, cy, a0, a1, dir);

        if math::cross(dx0, dy0, dx1, dy1) > 0.0 {
            cx = x1 + dx0*d + dy0*radius;
            cy = y1 + dy0*d + -dx0*radius;
            a0 = dx0.atan2(-dy0);
            a1 = -dx1.atan2(dy1);
            dir = Winding::CW;
        } else {
            cx = x1 + dx0*d + -dy0*radius;
            cy = y1 + dy0*d + dx0*radius;
            a0 = -dx0.atan2(dy0);
            a1 = dx1.atan2(-dy1);
            dir = Winding::CCW;
        }

        self.arc(cx, cy, radius, a0, a1, dir);

        self
    }

    /// Creates new rectangle shaped sub-path.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) -> &mut Self {
        self.append_commands(&mut [
            Command::MoveTo(x, y),
            Command::LineTo(x, y + h),
            Command::LineTo(x + w, y + h),
            Command::LineTo(x + w, y),
            Command::Close
        ]);

        self
    }

    /// Creates new rounded rectangle shaped sub-path.
    pub fn rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) -> &mut Self {
        self.rounded_rect_varying(x, y, w, h, r, r, r, r);
        self
    }

    /// Creates new rounded rectangle shaped sub-path with varying radii for each corner.
    pub fn rounded_rect_varying(&mut self, x: f32, y: f32, w: f32, h: f32, rad_top_left: f32, rad_top_right: f32, rad_bottom_right: f32, rad_bottom_left: f32) -> &mut Self {
        if rad_top_left < 0.1 && rad_top_right < 0.1 && rad_bottom_right < 0.1 && rad_bottom_left < 0.1 {
            self.rect(x, y, w, h);
        } else {
            let halfw = w.abs()*0.5;
            let halfh = h.abs()*0.5;

            let rx_bl = rad_bottom_left.min(halfw) * w.signum();
            let ry_bl = rad_bottom_left.min(halfh) * h.signum();

            let rx_br = rad_bottom_right.min(halfw) * w.signum();
            let ry_br = rad_bottom_right.min(halfh) * h.signum();

            let rx_tr = rad_top_right.min(halfw) * w.signum();
            let ry_tr = rad_top_right.min(halfh) * h.signum();

            let rx_tl = rad_top_left.min(halfw) * w.signum();
            let ry_tl = rad_top_left.min(halfh) * h.signum();

            self.append_commands(&mut [
                Command::MoveTo(x, y + ry_tl),
                Command::LineTo(x, y + h - ry_bl),
                Command::BezierTo(x, y + h - ry_bl*(1.0 - KAPPA90), x + rx_bl*(1.0 - KAPPA90), y + h, x + rx_bl, y + h),
                Command::LineTo(x + w - rx_br, y + h),
                Command::BezierTo(x + w - rx_br*(1.0 - KAPPA90), y + h, x + w, y + h - ry_br*(1.0 - KAPPA90), x + w, y + h - ry_br),
                Command::LineTo(x + w, y + ry_tr),
                Command::BezierTo(x + w, y + ry_tr*(1.0 - KAPPA90), x + w - rx_tr*(1.0 - KAPPA90), y, x + w - rx_tr, y),
                Command::LineTo(x + rx_tl, y),
                Command::BezierTo(x + rx_tl*(1.0 - KAPPA90), y, x, y + ry_tl*(1.0 - KAPPA90), x, y + ry_tl),
                Command::Close
            ]);
        }

        self
    }

    /// Creates new ellipse shaped sub-path.
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32) -> &mut Self {
        self.append_commands(&mut [
            Command::MoveTo(cx-rx, cy),
            Command::BezierTo(cx-rx, cy+ry*KAPPA90, cx-rx*KAPPA90, cy+ry, cx, cy+ry),
            Command::BezierTo(cx+rx*KAPPA90, cy+ry, cx+rx, cy+ry*KAPPA90, cx+rx, cy),
            Command::BezierTo(cx+rx, cy-ry*KAPPA90, cx+rx*KAPPA90, cy-ry, cx, cy-ry),
            Command::BezierTo(cx-rx*KAPPA90, cy-ry, cx-rx, cy-ry*KAPPA90, cx-rx, cy),
            Command::Close
        ]);

        self
    }

    /// Creates new circle shaped sub-path.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32) -> &mut Self {
        self.ellipse(cx, cy, r, r);
        self
    }

    fn append_commands(&mut self, commands: &[Command]) {
        for cmd in commands.iter() {
            match cmd {
                Command::MoveTo(x, y) => {
                    self.commandx = *x;
                    self.commandy = *y;
                }
                Command::LineTo(x, y) => {
                    self.commandx = *x;
                    self.commandy = *y;
                }
                Command::BezierTo(_c1x, _c1y, _c2x, _c2y, x, y) => {
                    self.commandx = *x;
                    self.commandy = *y;
                }
                _ => ()
            }
        }

        self.commands.extend_from_slice(commands);
    }
}

#[derive(Clone, Default, Debug)]
pub struct CachedPath {
    pub(crate) contours: Vec<Contour>,
    pub(crate) bounds: [f32; 4],
    points: Vec<Point>,
}

impl CachedPath {

    pub fn new(path: &Path, transform: Transform2D, tess_tol: f32, dist_tol: f32) -> Self {
        let mut cache = CachedPath::default();

        // Convert commands to a set of contours
        for cmd in path.commands() {
            match cmd {
                Command::MoveTo(x, y) => {
                    cache.add_contour();

                    let mut tx = *x;
                    let mut ty = *y;

                    transform.transform_point(&mut tx, &mut ty, *x, *y);

                    cache.add_point(tx, ty, PointFlags::CORNER, dist_tol);
                }
                Command::LineTo(x, y) => {
                    let mut tx = *x;
                    let mut ty = *y;

                    transform.transform_point(&mut tx, &mut ty, *x, *y);

                    cache.add_point(tx, ty, PointFlags::CORNER, dist_tol);
                }
                Command::BezierTo(c1x, c1y, c2x, c2y, x, y) => {
                    if let Some(last) = cache.last_point() {
                        let (mut tc1x, mut tc1y, mut tc2x, mut tc2y, mut tx, mut ty) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

                        transform.transform_point(&mut tc1x, &mut tc1y, *c1x, *c1y);
                        transform.transform_point(&mut tc2x, &mut tc2y, *c2x, *c2y);
                        transform.transform_point(&mut tx, &mut ty, *x, *y);

                        cache.tesselate_bezier(last.x, last.y, tc1x, tc1y, tc2x, tc2y, tx, ty, 0, PointFlags::CORNER, tess_tol, dist_tol);
                    }
                }
                Command::Close => {
                    cache.last_contour().map(|contour| contour.closed = true);
                }
                Command::Winding(winding) => {
                    cache.last_contour().map(|contour| contour.winding = *winding);
                }
            }
        }

        cache.bounds[0] = 1e6;
        cache.bounds[1] = 1e6;
        cache.bounds[2] = -1e6;
        cache.bounds[3] = -1e6;

        for contour in &mut cache.contours {
            let mut points = &mut cache.points[contour.first..(contour.first + contour.count)];

            let p0 = points.last().copied().unwrap();
            let p1 = points.first().copied().unwrap();

            // If the first and last points are the same, remove the last, mark as closed path.
            if math::pt_equals(p0.x, p0.y, p1.x, p1.y, dist_tol) {
                contour.count -= 1;
                //p0 = points[path.count-1];
                contour.closed = true;
                points = &mut cache.points[contour.first..(contour.first + contour.count)];
            }

            // Enforce winding.
            if contour.count > 2 {
                let area = Point::poly_area(points);

                if contour.winding == Winding::CCW && area < 0.0 {
                    points.reverse();
                }

                if contour.winding == Winding::CW && area > 0.0 {
                    points.reverse();
                }
            }

            // TODO: this is doggy and fishy.
            for i in 0..contour.count {
                let p1 = points[i];

                let p0 = if i == 0 {
                    points.last_mut().unwrap()
                } else {
                    points.get_mut(i-1).unwrap()
                };

                p0.dx = p1.x - p0.x;
                p0.dy = p1.y - p0.y;
                p0.len = math::normalize(&mut p0.dx, &mut p0.dy);

                cache.bounds[0] = cache.bounds[0].min(p0.x);
                cache.bounds[1] = cache.bounds[1].min(p0.y);
                cache.bounds[2] = cache.bounds[2].max(p0.x);
                cache.bounds[3] = cache.bounds[3].max(p0.y);
            }
        }

        cache
    }

    fn add_contour(&mut self) {
        let mut contour = Contour::default();

        contour.first = self.points.len();

        self.contours.push(contour);
    }

    fn last_contour(&mut self) -> Option<&mut Contour> {
        self.contours.last_mut()
    }

    // TODO: Revise if this needs to return &mut or just Point
    fn last_point(&mut self) -> Option<Point> {
        self.points.last_mut().copied()
    }

    fn add_point(&mut self, x: f32, y: f32, flags: PointFlags, dist_tol: f32) {
        if self.contours.len() == 0 { return }

        let count = &mut self.contours.last_mut().unwrap().count;

        if *count > 0 {
            if let Some(point) = self.points.last_mut() {
                if math::pt_equals(point.x, point.y, x, y, dist_tol) {
                    point.flags |= flags;
                    return;
                }
            }
        }

        let mut point = Point::default();
        point.x = x;
        point.y = y;
        point.flags = flags;

        self.points.push(point);
        *count += 1;
    }

    fn tesselate_bezier(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32, x4: f32, y4: f32, level: usize, atype: PointFlags, tess_tol: f32, dist_tol: f32) {
        if level > 10 { return; }

        let x12 = (x1+x2)*0.5;
        let y12 = (y1+y2)*0.5;
        let x23 = (x2+x3)*0.5;
        let y23 = (y2+y3)*0.5;
        let x34 = (x3+x4)*0.5;
        let y34 = (y3+y4)*0.5;
        let x123 = (x12+x23)*0.5;
        let y123 = (y12+y23)*0.5;

        let dx = x4 - x1;
        let dy = y4 - y1;
        let d2 = (((x2 - x4) * dy - (y2 - y4) * dx)).abs();
        let d3 = (((x3 - x4) * dy - (y3 - y4) * dx)).abs();

        if (d2 + d3)*(d2 + d3) < tess_tol * (dx*dx + dy*dy) {
            self.add_point(x4, y4, atype, dist_tol);
            return;
        }

        let x234 = (x23+x34)*0.5;
        let y234 = (y23+y34)*0.5;
        let x1234 = (x123+x234)*0.5;
        let y1234 = (y123+y234)*0.5;

        self.tesselate_bezier(x1,y1, x12,y12, x123,y123, x1234,y1234, level+1, PointFlags::empty(), tess_tol, dist_tol);
        self.tesselate_bezier(x1234,y1234, x234,y234, x34,y34, x4,y4, level+1, atype, tess_tol, dist_tol);
    }

    pub(crate) fn expand_fill(&mut self, w: f32, line_join: LineJoin, miter_limit: f32, fringe_width: f32) {

        let fringe = w > 0.0;
        let aa = fringe_width;

        self.calculate_joins(w, line_join, miter_limit);

        // Calculate max vertex usage.
        /*
        let mut vertex_count = 0;

        for path in &self.cache.paths {
            vertex_count += path.count + path.bevel + 1;

            if fringe {
                vertex_count += (path.count + path.bevel*5 + 1) * 2;// plus one for loop
            }
        }*/

        //self.cache.verts.clear();
        //self.cache.verts.reserve(vertex_count);

        let convex = self.contours.len() == 1 && self.contours[0].convex;

        for contour in &mut self.contours {
            let points = &self.points[contour.first..(contour.first + contour.count)];

            contour.stroke.clear();

            let woff = 0.5 * aa;

            if fringe {
                for i in 0..contour.count {
                    let p1 = points[i];

                    let p0 = if i == 0 {
                        points.last().unwrap()
                    } else {
                        points.get(i-1).unwrap()
                    };

                    if p1.flags.contains(PointFlags::BEVEL) {
                        // TODO: why do we need these variables.. just use p0.. and p1 directly down there
                        let dlx0 = p0.dy;
                        let dly0 = -p0.dx;
                        let dlx1 = p1.dy;
                        let dly1 = -p1.dx;

                        if p1.flags.contains(PointFlags::LEFT) {
                            let lx = p1.x + p1.dmx * woff;
                            let ly = p1.y + p1.dmy * woff;
                            contour.fill.push(Vertex::new(lx, ly, 0.5, 1.0));
                        } else {
                            let lx0 = p1.x + dlx0 * woff;
                            let ly0 = p1.y + dly0 * woff;
                            let lx1 = p1.x + dlx1 * woff;
                            let ly1 = p1.y + dly1 * woff;
                            contour.fill.push(Vertex::new(lx0, ly0, 0.5, 1.0));
                            contour.fill.push(Vertex::new(lx1, ly1, 0.5, 1.0));
                        }
                    } else {
                        contour.fill.push(Vertex::new(p1.x + (p1.dmx * woff), p1.y + (p1.dmy * woff), 0.5, 1.0));
                    }
                }
            } else {
                for i in 0..contour.count {
                    contour.fill.push(Vertex::new(points[i].x, points[i].y, 0.5, 1.0));
                }
            }

            if fringe {
                let mut lw = w + woff;
                let rw = w - woff;
                let mut lu = 0.0;
                let ru = 1.0;

                // Create only half a fringe for convex shapes so that
                // the shape can be rendered without stenciling.
                if convex {
                    lw = woff;    // This should generate the same vertex as fill inset above.
                    lu = 0.5;    // Set outline fade at middle.
                }

                for i in 0..contour.count {
                    let p1 = points[i];

                    let p0 = if i == 0 {
                        points.last().unwrap()
                    } else {
                        points.get(i-1).unwrap()
                    };

                    if p1.flags.contains(PointFlags::BEVEL | PointFlags::INNERBEVEL) {
                        bevel_join(&mut contour.stroke, p0, &p1, lw, rw, lu, rw, fringe_width);
                    } else {
                        contour.stroke.push(Vertex::new(p1.x + (p1.dmx * lw), p1.y + (p1.dmy * lw), lu, 1.0));
                        contour.stroke.push(Vertex::new(p1.x - (p1.dmx * rw), p1.y - (p1.dmy * rw), ru, 1.0));
                    }
                }

                // Loop it
                let p0 = contour.stroke[0];
                let p1 = contour.stroke[1];
                contour.stroke.push(Vertex::new(p0.x, p0.y, lu, 1.0));
                contour.stroke.push(Vertex::new(p1.x, p1.y, ru, 1.0));
            }
        }
    }

    // TODO: instead of passing 3 paint values here we can just pass the paint struct as a parameter
    pub(crate) fn expand_stroke(&mut self, w: f32, fringe: f32, line_cap: LineCap, line_join: LineJoin, miter_limit: f32, tess_tol: f32) {
        let aa = fringe;
        let mut u0 = 0.0;
        let mut u1 = 1.0;
        let ncap = curve_divisions(w, PI, tess_tol);

        let w = w + (aa * 0.5);

        // Disable the gradient used for antialiasing when antialiasing is not used.
        if aa == 0.0 {
            u0 = 0.5;
            u1 = 0.5;
        }

        self.calculate_joins(w, line_join, miter_limit);

        for contour in &mut self.contours {
            let points = &self.points[contour.first..(contour.first + contour.count)];

            contour.stroke.clear();

            // TODO: this is horrible - make a pretty configurable iterator that takes into account if the path is closed or not and gives correct p0 p1

            if contour.closed {

                for i in 0..contour.count {
                    let p1 = points[i];

                    let p0 = if i == 0 {
                        points[contour.count-1]
                    } else {
                        points[i-1]
                    };

                    if p1.flags.contains(PointFlags::BEVEL | PointFlags::INNERBEVEL) {
                        if line_join == LineJoin::Round {
                            round_join(&mut contour.stroke, &p0, &p1, w, w, u0, u1, ncap as usize, aa);
                        } else {
                            bevel_join(&mut contour.stroke, &p0, &p1, w, w, u0, u1, aa);
                        }
                    } else {
                        contour.stroke.push(Vertex::new(p1.x + (p1.dmx * w), p1.y + (p1.dmy * w), u0, 1.0));
                        contour.stroke.push(Vertex::new(p1.x - (p1.dmx * w), p1.y - (p1.dmy * w), u1, 1.0));
                    }
                }

                contour.stroke.push(Vertex::new(contour.stroke[0].x, contour.stroke[0].y, u0, 1.0));
                contour.stroke.push(Vertex::new(contour.stroke[1].x, contour.stroke[1].y, u1, 1.0));

            } else {
                let mut p0 = points[0];
                let mut p1 = points[1];

                // Add cap
                let mut dx = p1.x - p0.x;
                let mut dy = p1.y - p0.y;

                math::normalize(&mut dx, &mut dy);

                match line_cap {
                    LineCap::Butt => butt_cap_start(&mut contour.stroke, &p0, dx, dy, w, -aa*0.5, aa, u0, u1),
                    LineCap::Square => butt_cap_start(&mut contour.stroke, &p0, dx, dy, w, w-aa, aa, u0, u1),
                    LineCap::Round => round_cap_start(&mut contour.stroke, &p0, dx, dy, w, ncap as usize, aa, u0, u1),
                }

                // loop
                for i in 1..(contour.count - 1) {
                    p1 = points[i];
                    p0 = points[i-1];

                    if p1.flags.contains(PointFlags::BEVEL | PointFlags::INNERBEVEL) {
                        if line_join == LineJoin::Round {
                            round_join(&mut contour.stroke, &p0, &p1, w, w, u0, u1, ncap as usize, aa);
                        } else {
                            bevel_join(&mut contour.stroke, &p0, &p1, w, w, u0, u1, aa);
                        }
                    } else {
                        contour.stroke.push(Vertex::new(p1.x + (p1.dmx * w), p1.y + (p1.dmy * w), u0, 1.0));
                        contour.stroke.push(Vertex::new(p1.x - (p1.dmx * w), p1.y - (p1.dmy * w), u1, 1.0));
                    }
                }

                // Add cap
                p0 = points[contour.count - 2];
                p1 = points[contour.count - 1];

                let mut dx = p1.x - p0.x;
                let mut dy = p1.y - p0.y;

                math::normalize(&mut dx, &mut dy);

                match line_cap {
                    LineCap::Butt => butt_cap_end(&mut contour.stroke, &p1, dx, dy, w, -aa*0.5, aa, u0, u1),
                    LineCap::Square => butt_cap_end(&mut contour.stroke, &p1, dx, dy, w, w-aa, aa, u0, u1),
                    LineCap::Round => round_cap_end(&mut contour.stroke, &p1, dx, dy, w, ncap as usize, aa, u0, u1),
                }
            }
        }
    }

    fn calculate_joins(&mut self, w: f32, line_join: LineJoin, miter_limit: f32) {
        let iw = if w > 0.0 { 1.0 / w } else { 0.0 };

        for contour in &mut self.contours {
            let points = &mut self.points[contour.first..(contour.first+contour.count)];
            let mut nleft = 0;

            contour.bevel = 0;

            for i in 0..contour.count {

                let p0 = if i == 0 {
                    points.get(contour.count-1).cloned().unwrap()
                } else {
                    points.get(i-1).cloned().unwrap()
                };

                let p1 = &mut points[i];

                let dlx0 = p0.dy;
                let dly0 = -p0.dx;
                let dlx1 = p1.dy;
                let dly1 = -p1.dx;

                // Calculate extrusions
                p1.dmx = (dlx0 + dlx1) * 0.5;
                p1.dmy = (dly0 + dly1) * 0.5;
                let dmr2 = p1.dmx * p1.dmx + p1.dmy * p1.dmy;

                if dmr2 > 0.000001 {
                    let scale = (1.0 / dmr2).min(600.0);

                    p1.dmx *= scale;
                    p1.dmy *= scale;
                }

                // Clear flags, but keep the corner.
                p1.flags = if p1.flags.contains(PointFlags::CORNER) { PointFlags::CORNER } else { PointFlags::empty() };

                // Keep track of left turns.
                let cross = p1.dx * p0.dy - p0.dx * p1.dy;

                if cross > 0.0 {
                    nleft += 1;
                    p1.flags |= PointFlags::LEFT;
                }

                // Calculate if we should use bevel or miter for inner join.
                let limit = (p0.len.min(p1.len) * iw).max(1.01);

                if (dmr2 * limit * limit) < 1.0 {
                    p1.flags |= PointFlags::INNERBEVEL;
                }

                // Check to see if the corner needs to be beveled.
                if p1.flags.contains(PointFlags::CORNER) {
                    if (dmr2 * miter_limit * miter_limit) < 1.0 || line_join == LineJoin::Bevel || line_join == LineJoin::Round {
                        p1.flags |= PointFlags::BEVEL;
                    }
                }

                if p1.flags.contains(PointFlags::BEVEL | PointFlags::INNERBEVEL) {
                    contour.bevel += 1;
                }
            }

            contour.convex = nleft == contour.count;
        }
    }
}

fn curve_divisions(radius: f32, arc: f32, tol: f32) -> u32 {
    let da = (radius / (radius + tol)).acos() * 2.0;

    ((arc / da).ceil() as u32).max(2)
}

fn butt_cap_start(verts: &mut Vec<Vertex>, point: &Point, dx: f32, dy: f32, w: f32, d: f32, aa: f32, u0: f32, u1: f32) {
    let px = point.x - dx*d;
    let py = point.y - dy*d;
    let dlx = dy;
    let dly = -dx;

    verts.push(Vertex::new(px + dlx*w - dx*aa, py + dly*w - dy*aa, u0, 0.0));
    verts.push(Vertex::new(px - dlx*w - dx*aa, py - dly*w - dy*aa, u1, 0.0));
    verts.push(Vertex::new(px + dlx*w, py + dly*w, u0, 1.0));
    verts.push(Vertex::new(px - dlx*w, py - dly*w, u1, 1.0));
}

fn butt_cap_end(verts: &mut Vec<Vertex>, point: &Point, dx: f32, dy: f32, w: f32, d: f32, aa: f32, u0: f32, u1: f32) {
    let px = point.x + dx*d;
    let py = point.y + dy*d;
    let dlx = dy;
    let dly = -dx;

    verts.push(Vertex::new(px + dlx*w, py + dly*w, u0, 1.0));
    verts.push(Vertex::new(px - dlx*w, py - dly*w, u1, 1.0));
    verts.push(Vertex::new(px + dlx*w + dx*aa, py + dly*w + dy*aa, u0, 0.0));
    verts.push(Vertex::new(px - dlx*w + dx*aa, py - dly*w + dy*aa, u1, 0.0));
}

fn round_cap_start(verts: &mut Vec<Vertex>, point: &Point, dx: f32, dy: f32, w: f32, ncap: usize, _aa: f32, u0: f32, u1: f32) {
    let px = point.x;
    let py = point.y;
    let dlx = dy;
    let dly = -dx;

    for i in 0..ncap {
        let a = i as f32/(ncap as f32 - 1.0)*PI;
        let ax = a.cos() * w;
        let ay = a.sin() * w;

        verts.push(Vertex::new(px - dlx*ax - dx*ay, py - dly*ax - dy*ay, u0, 1.0));
        verts.push(Vertex::new(px, py, 0.5, 1.0));
    }

    verts.push(Vertex::new(px + dlx*w, py + dly*w, u0, 1.0));
    verts.push(Vertex::new(px - dlx*w, py - dly*w, u1, 1.0));
}

fn round_cap_end(verts: &mut Vec<Vertex>, point: &Point, dx: f32, dy: f32, w: f32, ncap: usize, _aa: f32, u0: f32, u1: f32) {
    let px = point.x;
    let py = point.y;
    let dlx = dy;
    let dly = -dx;

    verts.push(Vertex::new(px + dlx*w, py + dly*w, u0, 1.0));
    verts.push(Vertex::new(px - dlx*w, py - dly*w, u1, 1.0));

    for i in 0..ncap {
        let a = i as f32/(ncap as f32 - 1.0)*PI;
        let ax = a.cos() * w;
        let ay = a.sin() * w;

        verts.push(Vertex::new(px, py, 0.5, 1.0));
        verts.push(Vertex::new(px - dlx*ax + dx*ay, py - dly*ax + dy*ay, u0, 1.0));
    }
}

fn choose_bevel(bevel: bool, p0: &Point, p1: &Point, w: f32) -> (f32, f32, f32, f32) {
    if bevel {
        (p1.x + p0.dy * w, p1.y - p0.dx * w, p1.x + p1.dy * w, p1.y - p1.dx * w)
    } else {
        (p1.x + p1.dmx * w, p1.y + p1.dmy * w, p1.x + p1.dmx * w, p1.y + p1.dmy * w)
    }
}

fn round_join(verts: &mut Vec<Vertex>, p0: &Point, p1: &Point, lw: f32, rw: f32, lu: f32, ru: f32, ncap: usize, _fringe: f32) {
    let dlx0 = p0.dy;
    let dly0 = -p0.dx;
    let dlx1 = p1.dy;
    let dly1 = -p1.dx;

    let a0;
    let mut a1;

    // TODO: this if else arms are almost identical, maybe they can be combined
    if p1.flags.contains(PointFlags::LEFT) {
        let (lx0, ly0, lx1, ly1) = choose_bevel(p1.flags.contains(PointFlags::INNERBEVEL), p0, p1, lw);
        a0 = (-dly0).atan2(-dlx0);
        a1 = (-dly1).atan2(-dlx1);

        if a1 > a0 {
            a1 -= PI * 2.0;
        }

        verts.push(Vertex::new(lx0, ly0, lu, 1.0));
        verts.push(Vertex::new(p1.x - dlx0*rw, p1.y - dly0*rw, ru, 1.0));

        let n = ((((a0 - a1) / PI) * ncap as f32).ceil() as usize).max(2).min(ncap);

        for i in 0..n {
            let u = i as f32 / (n-1) as f32;
            let a = a0 + u*(a1-a0);
            let rx = p1.x + a.cos() * rw;
            let ry = p1.y + a.sin() * rw;

            verts.push(Vertex::new(p1.x, p1.y, 0.5, 1.0));
            verts.push(Vertex::new(rx, ry, ru, 1.0));
        }

        verts.push(Vertex::new(lx1, ly1, lu, 1.0));
        verts.push(Vertex::new(p1.x - dlx1*rw, p1.y - dly1*rw, ru, 1.0));
    } else {
        let (rx0, ry0, rx1, ry1) = choose_bevel(p1.flags.contains(PointFlags::INNERBEVEL), p0, p1, -rw);
        a0 = dly0.atan2(dlx0);
        a1 = dly1.atan2(dlx1);

        if a1 < a0 {
            a1 += PI * 2.0;
        }

        verts.push(Vertex::new(p1.x + dlx0*rw, p1.y + dly0*rw, lu, 1.0));
        verts.push(Vertex::new(rx0, ry0, ru, 1.0));

        let n = ((((a1 - a0) / PI) * ncap as f32).ceil() as usize).max(2).min(ncap);

        for i in 0..n {
            let u = i as f32 / (n-1) as f32;
            let a = a0 + u*(a1-a0);
            let lx = p1.x + a.cos() * lw;
            let ly = p1.y + a.sin() * lw;

            verts.push(Vertex::new(lx, ly, lu, 1.0));
            verts.push(Vertex::new(p1.x, p1.y, 0.5, 1.0));
        }

        verts.push(Vertex::new(p1.x + dlx1*rw, p1.y + dly1*rw, lu, 1.0));
        verts.push(Vertex::new(rx1, ry1, ru, 1.0));
    }
}

fn bevel_join(verts: &mut Vec<Vertex>, p0: &Point, p1: &Point, lw: f32, rw: f32, lu: f32, ru: f32, _fringe: f32) {
    let dlx0 = p0.dy;
    let dly0 = -p0.dx;
    let dlx1 = p1.dy;
    let dly1 = -p1.dx;

    // TODO: this if else arms are almost identical, maybe they can be combined
    if p1.flags.contains(PointFlags::LEFT) {
        let (lx0, ly0, lx1, ly1) = choose_bevel(p1.flags.contains(PointFlags::INNERBEVEL), p0, p1, lw);

        verts.push(Vertex::new(lx0, ly0, lu, 1.0));
        verts.push(Vertex::new(p1.x - dlx0*rw, p1.y - dly0*rw, ru, 1.0));

        if p1.flags.contains(PointFlags::BEVEL) {
            verts.push(Vertex::new(lx0, ly0, lu, 1.0));
            verts.push(Vertex::new(p1.x - dlx0*rw, p1.y - dly0*rw, ru, 1.0));

            verts.push(Vertex::new(lx1, ly1, lu, 1.0));
            verts.push(Vertex::new(p1.x - dlx1*rw, p1.y - dly1*rw, ru, 1.0));
        } else {
            let rx0 = p1.x - p1.dmx * rw;
            let ry0 = p1.y - p1.dmy * rw;

            verts.push(Vertex::new(p1.x, p1.y, 0.5, 1.0));
            verts.push(Vertex::new(p1.x - dlx0*rw, p1.y - dly0*rw, ru, 1.0));

            verts.push(Vertex::new(rx0, ry0, ru, 1.0));
            verts.push(Vertex::new(rx0, ry0, ru, 1.0));

            verts.push(Vertex::new(p1.x, p1.y, 0.5, 1.0));
            verts.push(Vertex::new(p1.x - dlx1*rw, p1.y - dly1*rw, ru, 1.0));
        }

        verts.push(Vertex::new(lx1, ly1, lu, 1.0));
        verts.push(Vertex::new(p1.x - dlx1*rw, p1.y - dly1*rw, ru, 1.0));
    } else {
        let (rx0, ry0, rx1, ry1) = choose_bevel(p1.flags.contains(PointFlags::INNERBEVEL), p0, p1, -rw);

        verts.push(Vertex::new(p1.x + dlx0*lw, p1.y + dly0*lw, lu, 1.0));
        verts.push(Vertex::new(rx0, ry0, ru, 1.0));

        if p1.flags.contains(PointFlags::BEVEL) {
            verts.push(Vertex::new(p1.x + dlx0*lw, p1.y + dly0*lw, lu, 1.0));
            verts.push(Vertex::new(rx0, ry0, ru, 1.0));

            verts.push(Vertex::new(p1.x + dlx1*lw, p1.y + dly1*lw, lu, 1.0));
            verts.push(Vertex::new(rx1, ry1, ru, 1.0));
        } else {
            let lx0 = p1.x + p1.dmx * lw;
            let ly0 = p1.y + p1.dmy * lw;

            verts.push(Vertex::new(p1.x + dlx0*lw, p1.y + dly0*lw, lu, 1.0));
            verts.push(Vertex::new(p1.x, p1.y, 0.5, 1.0));

            verts.push(Vertex::new(lx0, ly0, lu, 1.0));
            verts.push(Vertex::new(lx0, ly0, lu, 1.0));

            verts.push(Vertex::new(p1.x + dlx1*lw, p1.y + dly1*lw, lu, 1.0));
            verts.push(Vertex::new(p1.x, p1.y, 0.5, 1.0));
        }

        verts.push(Vertex::new(p1.x + dlx1*lw, p1.y + dly1*lw, lu, 1.0));
        verts.push(Vertex::new(rx1, ry1, ru, 1.0));
    }
}
