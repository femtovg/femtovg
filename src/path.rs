
use std::f32::consts::PI;

use crate::math::{self, Transform2D};
use crate::Winding;

// Length proportional to radius of a cubic bezier handle for 90deg arcs.
const KAPPA90: f32 = 0.5522847493;

//TODO: We have commands elsewhere - rename this to verb

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub enum Command {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    BezierTo(f32, f32, f32, f32, f32, f32),
    Close,
    Winding(Winding)
}

#[derive(Clone, Default)]
pub struct Path {
    commands: Vec<Command>,
    commandx: f32,
    commandy: f32,
    dist_tol: f32
}

impl Path {
    pub fn new() -> Self {
        Self {
            dist_tol: 0.01,
            ..Default::default()
        }
    }

    /// Returns iterator over Commands
    //TODO: Rename this to verbs()
    pub fn commands(&self) -> impl Iterator<Item = &Command> {
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
        } else if da.abs() >= PI * 2.0 {
            da = -PI * 2.0;
        } else {
            while da > 0.0 { da -= PI * 2.0 }
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
                let first_move = if !self.commands.is_empty() { Command::LineTo(x, y) } else { Command::MoveTo(x, y) };
                commands.push(first_move);
            } else {
                commands.push(Command::BezierTo(px+ptanx, py+ptany, x-tanx, y-tany, x, y));
            }

            px = x;
            py = y;
            ptanx = tanx;
            ptany = tany;
        }

        self.append_commands(&commands);

        self
    }

    /// Adds an arc segment at the corner defined by the last path point, and two specified points.
    pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) -> &mut Self {
        if self.commands.is_empty() {
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
        self.append_commands(&[
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

            self.append_commands(&[
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
        self.append_commands(&[
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

    pub fn transform(&mut self, transform: Transform2D) {
        // Convert commands to a set of contours
        for cmd in &mut self.commands {
            match cmd {
                Command::MoveTo(x, y) => {
                    transform.transform_point(x, y, *x, *y);
                }
                Command::LineTo(x, y) => {
                    transform.transform_point(x, y, *x, *y);
                }
                Command::BezierTo(c1x, c1y, c2x, c2y, x, y) => {
                    transform.transform_point(c1x, c1y, *c1x, *c1y);
					transform.transform_point(c2x, c2y, *c2x, *c2y);
					transform.transform_point(x, y, *x, *y);
                }
                _ => ()
            }
        }
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
