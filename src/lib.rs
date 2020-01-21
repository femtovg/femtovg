use std::f32::consts::PI;

use std::path::Path as FilePath;
use std::{error::Error, fmt};

use image::DynamicImage;
use bitflags::bitflags;

mod color;
pub use color::Color;

pub mod renderer;
pub use renderer::Renderer;
use renderer::{
    Vertex,
    Params,
    Command,
    CommandType,
    ShaderType,
    Drawable
};

mod font_cache;
use font_cache::{FontCache, FontCacheError, GlyphRenderStyle};

pub(crate) mod geometry;
use crate::geometry::*;

mod paint;
pub use paint::Paint;
use paint::PaintFlavor;

mod path_cache;
use path_cache::{PathCache, Convexity};

// TODO: path_contains_point method
// TODO: Drawing works before the call to begin frame for some reason
// TODO: rethink image creation and resource creation in general, it's currently blocking,
//         it would be awesome if its non-blocking and maybe async. Or maybe resource creation
//         should be a functionality provided by the current renderer implementation, not by the canvas itself.

// Length proportional to radius of a cubic bezier handle for 90deg arcs.
const KAPPA90: f32 = 0.5522847493;

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub enum Winding {
    CCW = 1,
    CW = 2
}

impl Default for Winding {
    fn default() -> Self {
        Winding::CCW
    }
}

#[derive(Copy, Clone, Debug)]
pub enum Verb {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    BezierTo(f32, f32, f32, f32, f32, f32),
    Close,
    Winding(Winding)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FillRule {
    EvenOdd,
    NonZero
}

impl Default for FillRule {
    fn default() -> Self {
        Self::NonZero
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Baseline {
    /// The text baseline is the top of the em square.
    Top,
    /// The text baseline is the middle of the em square.
    Middle,
    /// The text baseline is the normal alphabetic baseline. Default value.
    Alphabetic
}

impl Default for Baseline {
    fn default() -> Self {
        Self::Alphabetic
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Default for Align {
    fn default() -> Self {
        Self::Left
    }
}

// Image flags
bitflags! {
    pub struct ImageFlags: u32 {
        const GENERATE_MIPMAPS = 1 << 0;// Generate mipmaps during creation of the image.
        const REPEAT_X = 1 << 1;        // Repeat image in X direction.
        const REPEAT_Y = 1 << 2;        // Repeat image in Y direction.
        const FLIP_Y = 1 << 3;          // Flips (inverses) image in Y direction when rendered.
        const PREMULTIPLIED = 1 << 4;   // Image data has premultiplied alpha.
        const NEAREST = 1 << 5;         // Image interpolation is Nearest instead Linear
    }
}

#[derive(Copy, Clone, Debug)]
struct Scissor {
    transform: Transform2D,
    extent: Option<[f32; 2]>,
}

impl Default for Scissor {
    fn default() -> Self {
        Self {
            transform: Default::default(),
            extent: None
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

impl Default for LineCap {
    fn default() -> Self {
        Self::Butt
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel
}

impl Default for LineJoin {
    fn default() -> Self {
        Self::Miter
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ImageId(pub u32);

#[derive(Copy, Clone)]
struct State {
    transform: Transform2D,
    scissor: Scissor,
    alpha: f32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            transform: Transform2D::identity(),
            scissor: Default::default(),
            alpha: 1.0,
        }
    }
}

pub struct Canvas<T> {
    width: f32,
    height: f32,
    renderer: T,
    font_cache: FontCache,
    state_stack: Vec<State>,
    verbs: Vec<Verb>,
    lastx: f32,
    lasty: f32,
    path_cache: PathCache,
    cmds: Vec<Command>,
    verts: Vec<Vertex>,
    fringe_width: f32,
    device_px_ratio: f32,
    tess_tol: f32,
    dist_tol: f32
}

impl<T> Canvas<T> where T: Renderer {

    pub fn new(renderer: T) -> Result<Self, CanvasError> {

        // TODO: Return result from this method instead of unwrapping
        let font_manager = FontCache::new()?;

        let mut canvas = Self {
            width: Default::default(),
            height: Default::default(),
            renderer: renderer,
            font_cache: font_manager,
            state_stack: Default::default(),
            verbs: Default::default(),
            lastx: Default::default(),
            lasty: Default::default(),
            path_cache: Default::default(),
            cmds: Default::default(),
            verts: Default::default(),
            fringe_width: 1.0,
            device_px_ratio: 1.0,
            tess_tol: 0.25,
            dist_tol: 0.01
        };

        canvas.save();
        canvas.reset();

        Ok(canvas)
    }

    pub fn set_size(&mut self, width: u32, height: u32, dpi: f32) {
        self.width = width as f32;
        self.height = height as f32;
        self.fringe_width = 1.0 / dpi;
        self.tess_tol = 0.25 / dpi;
        self.dist_tol = 0.01 / dpi;
        self.device_px_ratio = dpi;

        self.renderer.set_size(width, height, dpi);
    }

    pub fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        self.renderer.clear_rect(x, y, width, height, color);
    }

    /// Returns the with of the canvas
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Returns the height of the canvas
    pub fn height(&self) -> f32 {
        self.height
    }

    /// Tells the renderer to execute all drawing commands and clears the current internal state
    ///
    /// Call this at the end of rach frame.
    pub fn flush(&mut self) {
        self.renderer.render(&self.verts, &self.cmds);
        self.cmds.clear();
        self.verts.clear();
        self.state_stack.clear();
        self.path_cache.clear();
        self.save();
    }

    pub fn screenshot(&mut self) -> Option<DynamicImage> {
        self.renderer.screenshot()
    }

    // State Handling

    /// Pushes and saves the current render state into a state stack.
    ///
    /// A matching restore() must be used to restore the state.
    pub fn save(&mut self) {
        let state = self.state_stack.last().map_or_else(State::default, |state| *state);

        self.state_stack.push(state);
    }

    /// Restores the previous render state
    pub fn restore(&mut self) {
        if self.state_stack.len() > 1 {
            self.state_stack.pop();
        }
    }

    /// Resets current render state to default values. Does not affect the render state stack.
    pub fn reset(&mut self) {
        *self.state_mut() = Default::default();
    }

    // Render styles

    /// Sets the transparency applied to all rendered shapes.
    ///
    /// Already transparent paths will get proportionally more transparent as well.
    pub fn set_global_alpha(&mut self, alpha: f32) {
        self.state_mut().alpha = alpha;
    }

    // Images

    /// Creates image by loading it from the disk from specified file name.
    pub fn create_image_file<P: AsRef<FilePath>>(&mut self, filename: P, flags: ImageFlags) -> Result<ImageId, CanvasError> {
        let image = image::open(filename)?;

        Ok(self.create_image(&image, flags))
    }

    /// Creates image by loading it from the specified chunk of memory.
    pub fn create_image_mem(&mut self, data: &[u8], flags: ImageFlags) -> Result<ImageId, CanvasError> {
        let image = image::load_from_memory(data)?;

        Ok(self.create_image(&image, flags))
    }

    /// Creates image by loading it from the specified chunk of memory.
    pub fn create_image(&mut self, image: &DynamicImage, flags: ImageFlags) -> ImageId {
        self.renderer.create_image(image, flags)
    }

    /// Updates image data specified by image handle.
    pub fn update_image(&mut self, id: ImageId, image: &DynamicImage, x: u32, y: u32) {
        self.renderer.update_image(id, image, x, y);
    }

    /// Deletes created image.
    pub fn delete_image(&mut self, id: ImageId) {
        self.renderer.delete_image(id);
    }

    // Transforms

    /// Resets current transform to a identity matrix.
    pub fn reset_transform(&mut self) {
        self.state_mut().transform = Transform2D::identity();
    }

    /// Premultiplies current coordinate system by specified matrix.
    /// The parameters are interpreted as matrix as follows:
    ///   [a c e]
    ///   [b d f]
    ///   [0 0 1]
    pub fn transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        let transform = Transform2D([a, b, c, d, e, f]);
        self.state_mut().transform.premultiply(&transform);
    }

    /// Translates the current coordinate system.
    pub fn translate(&mut self, x: f32, y: f32) {
        let mut t = Transform2D::identity();
        t.translate(x, y);
        self.state_mut().transform.premultiply(&t);
    }

    /// Rotates the current coordinate system. Angle is specified in radians.
    pub fn rotate(&mut self, angle: f32) {
        let mut t = Transform2D::identity();
        t.rotate(angle);
        self.state_mut().transform.premultiply(&t);
    }

    /// Skews the current coordinate system along X axis. Angle is specified in radians.
    pub fn skew_x(&mut self, angle: f32) {
        let mut t = Transform2D::identity();
        t.skew_x(angle);
        self.state_mut().transform.premultiply(&t);
    }

    /// Skews the current coordinate system along Y axis. Angle is specified in radians.
    pub fn skew_y(&mut self, angle: f32) {
        let mut t = Transform2D::identity();
        t.skew_y(angle);
        self.state_mut().transform.premultiply(&t);
    }

    /// Scales the current coordinate system.
    pub fn scale(&mut self, x: f32, y: f32) {
        let mut t = Transform2D::identity();
        t.scale(x, y);
        self.state_mut().transform.premultiply(&t);
    }

    /// Returns the current transformation matrix
    pub fn current_transform(&self) -> Transform2D {
        self.state().transform
    }

    // Scissoring

    /// Sets the current scissor rectangle.
    ///
    /// The scissor rectangle is transformed by the current transform.
    pub fn scissor(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let state = self.state_mut();

        let w = w.max(0.0);
        let h = h.max(0.0);

        let mut transform = Transform2D::new_translation(x + w * 0.5, y + h * 0.5);
        transform.premultiply(&state.transform);
        state.scissor.transform = transform;

        state.scissor.extent = Some([w * 0.5, h * 0.5]);
    }

    /// Intersects current scissor rectangle with the specified rectangle.
    ///
    /// The scissor rectangle is transformed by the current transform.
    /// Note: in case the rotation of previous scissor rect differs from
    /// the current one, the intersection will be done between the specified
    /// rectangle and the previous scissor rectangle transformed in the current
    /// transform space. The resulting shape is always rectangle.
    pub fn intersect_scissor(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let state = self.state_mut();

        // If no previous scissor has been set, set the scissor as current scissor.
        if state.scissor.extent.is_none() {
            self.scissor(x, y, w, h);
            return;
        }

        let extent = state.scissor.extent.unwrap();

        // Transform the current scissor rect into current transform space.
        // If there is difference in rotation, this will be approximation.

        let mut pxform = Transform2D::identity();

        let mut invxform = state.transform;
        invxform.inverse();

        pxform.multiply(&invxform);

        let ex = extent[0];
        let ey = extent[1];

        let tex = ex*pxform[0].abs() + ey*pxform[2].abs();
        let tey = ex*pxform[1].abs() + ey*pxform[3].abs();

        let rect = Rect::new(pxform[4]-tex, pxform[5]-tey, tex*2.0, tey*2.0);
        let res = rect.intersect(Rect::new(x, y, w, h));

        self.scissor(res.x, res.y, res.w, res.h);
    }

    /// Reset and disables scissoring.
    pub fn reset_scissor(&mut self) {
        self.state_mut().scissor = Scissor::default();
    }

    // Paths

    /// Starts new path
    pub fn begin_path(&mut self) {
        self.verbs.clear();
        self.path_cache.clear();
    }

    /// Starts new sub-path with specified point as first point.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.append_verbs(&mut [Verb::MoveTo(x, y)]);
    }

    /// Adds line segment from the last point in the path to the specified point.
    pub fn line_to(&mut self, x: f32, y: f32) {
        self.append_verbs(&mut [Verb::LineTo(x, y)]);
    }

    /// Adds cubic bezier segment from last point in the path via two control points to the specified point.
    pub fn bezier_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.append_verbs(&mut [Verb::BezierTo(c1x, c1y, c2x, c2y, x, y)]);
    }

    /// Adds quadratic bezier segment from last point in the path via a control point to the specified point.
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let x0 = self.lastx;
        let y0 = self.lasty;

        self.append_verbs(&mut [
            Verb::BezierTo(
                x0 + 2.0/3.0*(cx - x0), y0 + 2.0/3.0*(cy - y0),
                x + 2.0/3.0*(cx - x), y + 2.0/3.0*(cy - y),
                x, y
            )
        ]);
    }

    /// Closes current sub-path with a line segment.
    pub fn close(&mut self) {
        self.append_verbs(&mut [Verb::Close]);
    }

    /// Sets the current sub-path winding, see Winding and Solidity
    pub fn winding(&mut self, winding: Winding) {
        self.append_verbs(&mut [Verb::Winding(winding)]);
    }

    /// Creates new circle arc shaped sub-path. The arc center is at cx,cy, the arc radius is r,
    /// and the arc is drawn from angle a0 to a1, and swept in direction dir (Winding)
    /// Angles are specified in radians.
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, dir: Winding) {
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
                let first_move = if !self.verbs.is_empty() { Verb::LineTo(x, y) } else { Verb::MoveTo(x, y) };
                commands.push(first_move);
            } else {
                commands.push(Verb::BezierTo(px+ptanx, py+ptany, x-tanx, y-tany, x, y));
            }

            px = x;
            py = y;
            ptanx = tanx;
            ptany = tany;
        }

        self.append_verbs(&mut commands);
    }

    /// Adds an arc segment at the corner defined by the last path point, and two specified points.
    pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) {
        if self.verbs.is_empty() {
            return;
        }

        let (x0, y0) = self.state().transform.inversed().transform_point(self.lastx, self.lasty);

        // Handle degenerate cases.
        if geometry::pt_equals(x0, y0, x1, y1, self.dist_tol) ||
            geometry::pt_equals(x1, y1, x2, y2, self.dist_tol) ||
            geometry::dist_pt_segment(x1, y1, x0, y0, x2, y2) < self.dist_tol * self.dist_tol ||
            radius < self.dist_tol {
            self.line_to(x1, y1);
        }

        let mut dx0 = x0 - x1;
        let mut dy0 = y0 - y1;
        let mut dx1 = x2 - x1;
        let mut dy1 = y2 - y1;

        geometry::normalize(&mut dx0, &mut dy0);
        geometry::normalize(&mut dx1, &mut dy1);

        let a = (dx0*dx1 + dy0*dy1).acos();
        let d = radius / (a/2.0).tan();

        if d > 10000.0 {
            return self.line_to(x1, y1);
        }

        let (cx, cy, a0, a1, dir);

        if geometry::cross(dx0, dy0, dx1, dy1) > 0.0 {
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
    }

    /// Creates new rectangle shaped sub-path.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.append_verbs(&mut [
            Verb::MoveTo(x, y),
            Verb::LineTo(x, y + h),
            Verb::LineTo(x + w, y + h),
            Verb::LineTo(x + w, y),
            Verb::Close
        ]);
    }

    /// Creates new rounded rectangle shaped sub-path.
    pub fn rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        self.rounded_rect_varying(x, y, w, h, r, r, r, r);
    }

    // TODO: Test this with tiny angles < 0.001
    /// Creates new rounded rectangle shaped sub-path with varying radii for each corner.
    pub fn rounded_rect_varying(&mut self, x: f32, y: f32, w: f32, h: f32, rad_top_left: f32, rad_top_right: f32, rad_bottom_right: f32, rad_bottom_left: f32) {
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

            self.append_verbs(&mut [
                Verb::MoveTo(x, y + ry_tl),
                Verb::LineTo(x, y + h - ry_bl),
                Verb::BezierTo(x, y + h - ry_bl*(1.0 - KAPPA90), x + rx_bl*(1.0 - KAPPA90), y + h, x + rx_bl, y + h),
                Verb::LineTo(x + w - rx_br, y + h),
                Verb::BezierTo(x + w - rx_br*(1.0 - KAPPA90), y + h, x + w, y + h - ry_br*(1.0 - KAPPA90), x + w, y + h - ry_br),
                Verb::LineTo(x + w, y + ry_tr),
                Verb::BezierTo(x + w, y + ry_tr*(1.0 - KAPPA90), x + w - rx_tr*(1.0 - KAPPA90), y, x + w - rx_tr, y),
                Verb::LineTo(x + rx_tl, y),
                Verb::BezierTo(x + rx_tl*(1.0 - KAPPA90), y, x, y + ry_tl*(1.0 - KAPPA90), x, y + ry_tl),
                Verb::Close
            ]);
        }
    }

    /// Creates new ellipse shaped sub-path.
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32) {
        self.append_verbs(&mut [
            Verb::MoveTo(cx-rx, cy),
            Verb::BezierTo(cx-rx, cy+ry*KAPPA90, cx-rx*KAPPA90, cy+ry, cx, cy+ry),
            Verb::BezierTo(cx+rx*KAPPA90, cy+ry, cx+rx, cy+ry*KAPPA90, cx+rx, cy),
            Verb::BezierTo(cx+rx, cy-ry*KAPPA90, cx+rx*KAPPA90, cy-ry, cx, cy-ry),
            Verb::BezierTo(cx-rx*KAPPA90, cy-ry, cx-rx, cy-ry*KAPPA90, cx-rx, cy),
            Verb::Close
        ]);
    }

    /// Creates new circle shaped sub-path.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32) {
        self.ellipse(cx, cy, r, r);
    }

    /// Fills the current path with current fill style.
    pub fn fill_path(&mut self, mut paint: Paint) {
        self.path_cache.set(&self.verbs, self.tess_tol, self.dist_tol);

        let transform = self.state().transform;

        // Transform paint
        paint.transform = transform;

        // Apply global alpha
        paint.mul_alpha(self.state().alpha);

        let scissor = self.state().scissor;

        if paint.shape_anti_alias() {
            self.path_cache.expand_fill(self.fringe_width, LineJoin::Miter, 2.4, self.fringe_width);
        } else {
            self.path_cache.expand_fill(0.0, LineJoin::Miter, 2.4, self.fringe_width);
        }

        let flavor = if self.path_cache.contours.len() == 1 && self.path_cache.contours[0].convexity == Convexity::Convex {
            let params = Params::new(&self.renderer, &paint, &scissor, self.fringe_width, self.fringe_width, -1.0);

            CommandType::ConvexFill { params }
        } else {
            let mut stencil_params = Params::default();
            stencil_params.stroke_thr = -1.0;
            stencil_params.shader_type = ShaderType::Stencil.to_f32();

            let fill_params = Params::new(&self.renderer, &paint, &scissor, self.fringe_width, self.fringe_width, -1.0);

            CommandType::ConcaveFill { stencil_params, fill_params }
        };

        let mut cmd = Command::new(flavor);
        cmd.transform = transform;
        cmd.fill_rule = paint.fill_rule;

        if let PaintFlavor::Image { id, .. } = paint.flavor {
            cmd.image = Some(id);
        }

        let mut offset = self.verts.len();

        for contour in &self.path_cache.contours {
            let mut drawable = Drawable::default();

            if !contour.fill.is_empty() {
                drawable.fill_verts = Some((offset, contour.fill.len()));
                self.verts.extend_from_slice(&contour.fill);
                offset += contour.fill.len();
            }

            if !contour.stroke.is_empty() {
                drawable.stroke_verts = Some((offset, contour.stroke.len()));
                self.verts.extend_from_slice(&contour.stroke);
                offset += contour.stroke.len();
            }

            cmd.drawables.push(drawable);
        }

        if let CommandType::ConcaveFill {..} = cmd.cmd_type {
            // Quad
            self.verts.push(Vertex::new(self.path_cache.bounds.maxx, self.path_cache.bounds.maxy, 0.5, 1.0));
            self.verts.push(Vertex::new(self.path_cache.bounds.maxx, self.path_cache.bounds.miny, 0.5, 1.0));
            self.verts.push(Vertex::new(self.path_cache.bounds.minx, self.path_cache.bounds.maxy, 0.5, 1.0));
            self.verts.push(Vertex::new(self.path_cache.bounds.minx, self.path_cache.bounds.miny, 0.5, 1.0));

            cmd.triangles_verts = Some((offset, 4));
        }

        self.cmds.push(cmd);
    }

    /// Fills the current path with current stroke style.
    pub fn stroke_path(&mut self, mut paint: Paint) {
        self.path_cache.set(&self.verbs, self.tess_tol, self.dist_tol);

        let scissor = self.state().scissor;
        let transform = self.state().transform;
        let scale = transform.average_scale();

        // Transform paint
        paint.transform = transform;

        // Scale stroke width by current transform scale
        paint.set_stroke_width((paint.stroke_width() * scale).max(0.0).min(200.0));

        if paint.stroke_width() < self.fringe_width {
            // If the stroke width is less than pixel size, use alpha to emulate coverage.
            // Since coverage is area, scale by alpha*alpha.
            let alpha = (paint.stroke_width() / self.fringe_width).max(0.0).min(1.0);

            paint.mul_alpha(alpha*alpha);
            paint.set_stroke_width(self.fringe_width)
        }

        // Apply global alpha
        paint.mul_alpha(self.state().alpha);

        if paint.shape_anti_alias() {
            self.path_cache.expand_stroke(paint.stroke_width() * 0.5, self.fringe_width, paint.line_cap(), paint.line_join(), paint.miter_limit(), self.tess_tol);
        } else {
            self.path_cache.expand_stroke(paint.stroke_width() * 0.5, 0.0, paint.line_cap(), paint.line_join(), paint.miter_limit(), self.tess_tol);
        }

        let params = Params::new(&self.renderer, &paint, &scissor, paint.stroke_width(), self.fringe_width, -1.0);

        let flavor = if paint.stencil_strokes() {
            let params2 = Params::new(&self.renderer, &paint, &scissor, paint.stroke_width(), self.fringe_width, 1.0 - 0.5/255.0);

            CommandType::StencilStroke { params1: params, params2 }
        } else {
            CommandType::Stroke { params }
        };

        let mut cmd = Command::new(flavor);
        cmd.transform = transform;

        if let PaintFlavor::Image { id, .. } = paint.flavor {
            cmd.image = Some(id);
        }

        let mut offset = self.verts.len();

        for contour in &self.path_cache.contours {
            let mut drawable = Drawable::default();

            if !contour.stroke.is_empty() {
                drawable.stroke_verts = Some((offset, contour.stroke.len()));
                self.verts.extend_from_slice(&contour.stroke);
                offset += contour.stroke.len();
            }

            cmd.drawables.push(drawable);
        }

        self.cmds.push(cmd);
    }

    // Text

    pub fn add_font<P: AsRef<FilePath>>(&mut self, file_path: P) {
        self.font_cache.add_font_file(file_path).expect("cannot add font");
    }

    pub fn add_font_mem(&mut self, data: Vec<u8>) {
        self.font_cache.add_font_mem(data).expect("cannot add font");
    }

    /*
    pub fn text_bounds(&mut self, x: f32, y: f32, text: &str) -> [f32; 4] {
        let scale = self.font_scale() * self.device_px_ratio;
        let invscale = 1.0 / scale;

        let mut style = FontStyle::new("NotoSans-Regular");
        style.set_size((self.state().font_size as f32 * scale) as u32);
        style.set_letter_spacing(self.state().letter_spacing * scale);
        style.set_blur(self.state().font_blur * scale);

        let layout = self.font_manager.layout_text(x, y, &mut self.renderer, style, text).unwrap();

        let mut bounds = layout.bbox;

        // Use line bounds for height.
        //let (ymin, ymax) = self.font_stash.line_bounds(y * scale);
        //bounds[1] = ymin;
        //bounds[3] = ymax;

        bounds[0] *= invscale;
        bounds[1] *= invscale;
        bounds[2] *= invscale;
        bounds[3] *= invscale;

        bounds
    }*/

    pub fn fill_text(&mut self, x: f32, y: f32, text: &str, paint: Paint) {
        self.draw_text(x, y, text, paint, GlyphRenderStyle::Fill);
    }

    pub fn stroke_text(&mut self, x: f32, y: f32, text: &str, paint: Paint) {
        self.draw_text(x, y, text, paint, GlyphRenderStyle::Stroke {
            line_width: paint.stroke_width().ceil() as u32
        });
    }

    // Private

    fn draw_text(&mut self, x: f32, y: f32, text: &str, mut paint: Paint, render_style: GlyphRenderStyle) {
        let transform = self.state().transform;
        let scissor = self.state().scissor;
        let scale = self.font_scale() * self.device_px_ratio;
        let invscale = 1.0 / scale;

        // transform paint
        paint.set_font_size((paint.font_size() as f32 * scale) as u32);
        paint.set_letter_spacing((paint.letter_spacing() as f32 * scale) as i32);
        paint.set_font_blur(paint.font_blur() * scale);

        let layout = self.font_cache.layout_text(x, y, &mut self.renderer, paint, render_style, text).unwrap();

        let text_color = if let PaintFlavor::Color(color) = paint.flavor {
            color
        } else {
            Color::black()
        };

        for cmd in &layout.cmds {
            let mut verts = Vec::with_capacity(cmd.quads.len() * 6);

            for quad in &cmd.quads {
                let (p0, p1) = transform.transform_point(quad.x0*invscale, quad.y0*invscale);
                let (p2, p3) = transform.transform_point(quad.x1*invscale, quad.y0*invscale);
                let (p4, p5) = transform.transform_point(quad.x1*invscale, quad.y1*invscale);
                let (p6, p7) = transform.transform_point(quad.x0*invscale, quad.y1*invscale);

                verts.push(Vertex::new(p0, p1, quad.s0, quad.t0));
                verts.push(Vertex::new(p4, p5, quad.s1, quad.t1));
                verts.push(Vertex::new(p2, p3, quad.s1, quad.t0));
                verts.push(Vertex::new(p0, p1, quad.s0, quad.t0));
                verts.push(Vertex::new(p6, p7, quad.s0, quad.t1));
                verts.push(Vertex::new(p4, p5, quad.s1, quad.t1));
            }

            let mut paint = Paint::image(cmd.image_id, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0);

            if let PaintFlavor::Image { tint, .. } = &mut paint.flavor {
                *tint = text_color;
            }

            // Apply global alpha
            paint.mul_alpha(self.state().alpha);

            self.render_triangles(&verts, &paint, &scissor, &transform);
        }
    }

    fn render_triangles(&mut self, verts: &[Vertex], paint: &Paint, scissor: &Scissor, transform: &Transform2D) {
        let mut params = Params::new(&self.renderer, paint, scissor, 1.0, 1.0, -1.0);
        params.shader_type = ShaderType::Img.to_f32();

        let mut cmd = Command::new(CommandType::Triangles { params });
        cmd.transform = *transform;

        if let PaintFlavor::Image { id, .. } = paint.flavor {
            cmd.image = Some(id);
        }

        cmd.triangles_verts = Some((self.verts.len(), verts.len()));
        self.cmds.push(cmd);

        self.verts.extend_from_slice(verts);
    }

    fn font_scale(&self) -> f32 {
        let avg_scale = self.state().transform.average_scale();

        geometry::quantize(avg_scale, 0.01).min(4.0)
    }

    fn state(&self) -> &State {
        self.state_stack.last().unwrap()
    }

    fn state_mut(&mut self) -> &mut State {
        self.state_stack.last_mut().unwrap()
    }

    /// Appends a slice of verbs to the path
    fn append_verbs(&mut self, verbs: &mut [Verb]) {
        let transform = self.state().transform;

        for cmd in verbs.iter_mut() {
            match cmd {
                Verb::MoveTo(x, y) => {
                    self.lastx = *x;
                    self.lasty = *y;

                    let (tx, ty) = transform.transform_point(*x, *y);

                    *x = tx;
                    *y = ty;
                }
                Verb::LineTo(x, y) => {
                    self.lastx = *x;
                    self.lasty = *y;

                    let (tx, ty) = transform.transform_point(*x, *y);

                    *x = tx;
                    *y = ty;
                }
                Verb::BezierTo(c1x, c1y, c2x, c2y, x, y) => {
                    self.lastx = *x;
                    self.lasty = *y;

                    let (tc1x, tc1y) = transform.transform_point(*c1x, *c1y);
                    let (tc2x, tc2y) = transform.transform_point(*c2x, *c2y);
                    let (tx, ty) = transform.transform_point(*x, *y);

                    *c1x = tc1x;
                    *c1y = tc1y;
                    *c2x = tc2x;
                    *c2y = tc2y;
                    *x = tx;
                    *y = ty;
                }
                _ => ()
            }
        }

        self.verbs.extend_from_slice(verbs);
    }
}

#[derive(Debug)]
pub enum CanvasError {
    GeneralError(String),
    ImageError(image::ImageError),
    FontError(FontCacheError)
}

impl fmt::Display for CanvasError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "canvas error")
    }
}

impl From<image::ImageError> for CanvasError {
    fn from(error: image::ImageError) -> Self {
        Self::ImageError(error)
    }
}

impl From<FontCacheError> for CanvasError {
    fn from(error: FontCacheError) -> Self {
        Self::FontError(error)
    }
}

impl Error for CanvasError {}

#[cfg(test)]
mod tests {

    use super::*;
    use super::renderer::*;

    #[test]
    fn path_with_one_move_to_does_not_panic() {
        let mut canvas = Canvas::new(Void).unwrap();

        canvas.begin_path();
        canvas.move_to(10.0, 10.0);
        canvas.fill_path(Paint::color(Color::rgb(100, 100, 100)));
        canvas.stroke_path(Paint::color(Color::rgb(100, 100, 100)));
    }

    #[test]
    fn path_with_two_lines_to_does_not_panic() {
        let mut canvas = Canvas::new(Void).unwrap();

        canvas.begin_path();
        canvas.line_to(10.0, 10.0);
        canvas.line_to(10.0, 10.0);
        canvas.fill_path(Paint::color(Color::rgb(100, 100, 100)));
        canvas.stroke_path(Paint::color(Color::rgb(100, 100, 100)));
    }

    #[test]
    fn self_intersecting_polygon_is_concave() {
        let mut canvas = Canvas::new(Void).unwrap();

        // star
        canvas.begin_path();
        canvas.move_to(50.0, 0.0);
        canvas.line_to(21.0, 90.0);
        canvas.line_to(98.0, 35.0);
        canvas.line_to(2.0, 35.0);
        canvas.line_to(79.0, 90.0);
        canvas.close();
        canvas.fill_path(Paint::color(Color::rgb(100, 100, 100)));

        assert_eq!(canvas.path_cache.contours[0].convexity, Convexity::Concave);
    }
}
