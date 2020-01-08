
use std::path::Path as FilePath;
use std::{error::Error, fmt};

use image::{DynamicImage, GenericImageView};
use bitflags::bitflags;

mod color;
pub use color::Color;

mod atlas;
pub use atlas::Atlas;

pub mod renderer;
use renderer::{Renderer, TextureType};

pub mod font_manager;
pub use font_manager::{FontManager, FontStyle, FontManagerError};

pub mod math;
use crate::math::*;

mod paint;
pub use paint::Paint;

mod path;
pub use path::{CachedPath, Path};

// TODO: Use Convexity enum to describe path concave/convex
// TODO: Replace pt_equals with method on point
// TODO: Rename tess_tol and dist_tol to tesselation_tolerance and distance_tolerance
// TODO: Drawing works before the call to begin frame for some reason
// TODO: rethink image creation and resource creation in general, it's currently blocking,
//         it would be awesome if its non-blocking and maybe async. Or maybe resource creation
//         should be a functionality provided by the current renderer implementation, not by the canvas itself.
// TODO: A lot of the render styles can be moved to the Paint object - stroke width, line join and cap, basically a lot of the state object
// TODO: Instead of path cache filles with paths, use path filled with contours -> https://skia.org/user/api/SkPath_Overview

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
    Baseline
}

impl Default for VAlign {
    fn default() -> Self {
        Self::Baseline
    }
}

// Image flags
bitflags! {
    pub struct ImageFlags: u32 {
        const GENERATE_MIPMAPS = 1 << 0;// Generate mipmaps during creation of the image.
        const REPEAT_X = 1 << 1;        // Repeat image in X direction.
        const REPEAT_Y = 1 << 2;        // Repeat image in Y direction.
        const FLIP_Y = 1 << 3;            // Flips (inverses) image in Y direction when rendered.
        const PREMULTIPLIED = 1 << 4;    // Image data has premultiplied alpha.
        const NEAREST = 1 << 5;            // Image interpolation is Nearest instead Linear
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub struct Scissor {
    transform: Transform2D,
    extent: [f32; 2],
}

impl Default for Scissor {
    fn default() -> Self {
        Self {
            transform: Default::default(),
            extent: [-1.0, -1.0]// TODO: Use Option instead of relying on -1s
        }
    }
}

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

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
enum Command {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    BezierTo(f32, f32, f32, f32, f32, f32),
    Close,
    Winding(Winding)
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Default)]
#[repr(C)]
pub struct Vertex {
    x: f32,
    y: f32,
    u: f32,
    v: f32
}

impl Vertex {
    pub fn new(x: f32, y: f32, u: f32, v: f32) -> Self {
        Self { x, y, u, v }
    }

    pub fn set(&mut self, x: f32, y: f32, u: f32, v: f32) {
        *self = Self { x, y, u, v };
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
    flags: u8// TODO: Use bitflags crate for this
}

// TODO: We need an iterator for the contour points that loops by chunks of 2

#[derive(Clone, Default, Debug)]
pub struct Contour {
    first: usize,
    count: usize,
    closed: bool,
    bevel: usize,
    fill: Vec<Vertex>,
    stroke: Vec<Vertex>,
    winding: Winding,
    convex: bool
}

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

pub struct Canvas {
    renderer: Box<dyn Renderer>,
    font_manager: FontManager,
    state_stack: Vec<State>,
    tess_tol: f32,
    dist_tol: f32,
    fringe_width: f32,
    device_px_ratio: f32,
}

impl Canvas {

    pub fn new<R: Renderer + 'static>(renderer: R) -> Self {

        // TODO: Return result from this method instead of unwrapping
        let font_manager = FontManager::new().unwrap();

        let mut canvas = Self {
            renderer: Box::new(renderer),
            font_manager: font_manager,
            state_stack: Default::default(),
            tess_tol: Default::default(),
            dist_tol: Default::default(),
            fringe_width: Default::default(),
            device_px_ratio: Default::default(),
        };

        canvas.save();
        canvas.reset();

        canvas.set_device_pixel_ratio(1.0);

        canvas
    }

    pub fn begin_frame(&mut self, window_width: f32, window_height: f32, device_px_ratio: f32) {
        self.state_stack.clear();
        self.save();

        self.set_device_pixel_ratio(device_px_ratio);

        self.renderer.render_viewport(window_width, window_height);
    }

    pub fn end_frame(&mut self) {
        self.renderer.render_flush();
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
    pub fn create_image<P: AsRef<FilePath>>(&mut self, filename: P, flags: ImageFlags) -> Result<ImageId, CanvasError> {
        let image = image::open(filename)?;

        Ok(self.create_image_rgba(flags, &image))
    }

    /// Creates image by loading it from the specified chunk of memory.
    pub fn create_image_mem(&mut self, flags: ImageFlags, data: &[u8]) -> Result<ImageId, CanvasError> {
        let image = image::load_from_memory(data)?;

        Ok(self.create_image_rgba(flags, &image))
    }

    /// Creates image by loading it from the specified chunk of memory.
    pub fn create_image_rgba(&mut self, flags: ImageFlags, image: &DynamicImage) -> ImageId {
        let w = image.width();
        let h = image.height();

        let image_id = self.renderer.create_texture(TextureType::Rgba, w, h, flags);

        self.renderer.update_texture(image_id, image, 0, 0, w, h);

        image_id
    }

    /// Updates image data specified by image handle.
    pub fn update_image(&mut self, id: ImageId, image: &DynamicImage) {
        let w = image.width();
        let h = image.height();

        self.renderer.update_texture(id, image, 0, 0, w, h);
    }

    /// Deletes created image.
    pub fn delete_image(&mut self, id: ImageId) {
        self.renderer.delete_texture(id);
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
    pub fn rotate<R: Into<Rad>>(&mut self, angle: R) {
        let mut t = Transform2D::identity();
        t.rotate(angle);
        self.state_mut().transform.premultiply(&t);
    }

    /// Skews the current coordinate system along X axis. Angle is specified in radians.
    pub fn skew_x<R: Into<Rad>>(&mut self, angle: R) {
        let mut t = Transform2D::identity();
        t.skew_x(angle);
        self.state_mut().transform.premultiply(&t);
    }

    /// Skews the current coordinate system along Y axis. Angle is specified in radians.
    pub fn skew_y<R: Into<Rad>>(&mut self, angle: R) {
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

        state.scissor.transform = Transform2D::identity();
        state.scissor.transform[4] = x + w * 0.5;
        state.scissor.transform[5] = y + h * 0.5;
        state.scissor.transform.premultiply(&state.transform);

        state.scissor.extent[0] = w * 0.5;
        state.scissor.extent[1] = h * 0.5;
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
        // TODO: Make state.scissor an Option instead of relying on extent being less than 0
        if state.scissor.extent[0] < 0.0 {
            self.scissor(x, y, w, h);
            return;
        }

        // Transform the current scissor rect into current transform space.
        // If there is difference in rotation, this will be approximation.

        let mut pxform = Transform2D::identity();

        let mut invxform = state.transform;
        invxform.inverse();

        pxform.multiply(&invxform);

        let ex = state.scissor.extent[0];
        let ey = state.scissor.extent[1];

        let tex = ex*pxform[0].abs() + ey*pxform[2].abs();
        let tey = ex*pxform[1].abs() + ey*pxform[3].abs();

        let a = Rect::new(pxform[4]-tex, pxform[5]-tey, tex*2.0, tey*2.0);
        let res = a.intersect(Rect::new(x, y, w, h));

        self.scissor(res.x, res.y, res.w, res.h);
    }

    /// Reset and disables scissoring.
    pub fn reset_scissor(&mut self) {
        self.state_mut().scissor = Scissor::default();
    }

    /// Fills the current path with current fill style.
    pub fn fill(&mut self, path: &Path, paint: &Paint) {
        //self.flatten_paths(&path.commands);

        let mut cache = CachedPath::new(path, self.state().transform, self.tess_tol, self.dist_tol);

        let mut paint = paint.clone();

        if self.renderer.edge_antialiasing() && paint.shape_anti_alias() {
            //self.expand_fill(&mut cache, self.fringe_width, LineJoin::Miter, 2.4);
            cache.expand_fill(self.fringe_width, LineJoin::Miter, 2.4, self.fringe_width);
        } else {
            cache.expand_fill(0.0, LineJoin::Miter, 2.4, self.fringe_width);
        }

        let mut transform = paint.transform();
        transform.multiply(&self.state().transform);
        paint.set_transform(transform);

        // Apply global alpha
        let mut inner_color = paint.inner_color();
        inner_color.a *= self.state().alpha;
        paint.set_inner_color(inner_color);

        let mut outer_color = paint.outer_color();
        outer_color.a *= self.state().alpha;
        paint.set_outer_color(outer_color);

        let scissor = &self.state_stack.last().unwrap().scissor;

        self.renderer.render_fill(&paint, scissor, self.fringe_width, cache.bounds, &cache.contours);
    }

    /// Fills the current path with current stroke style.
    pub fn stroke(&mut self, path: &Path, paint: &Paint) {
        let scale = self.state().transform.average_scale();
        let mut stroke_width = (paint.stroke_width() * scale).max(0.0).min(200.0);

        let mut cache = CachedPath::new(path, self.state().transform, self.tess_tol, self.dist_tol);

        let mut paint = paint.clone();

        if stroke_width < self.fringe_width {
            // If the stroke width is less than pixel size, use alpha to emulate coverage.
            // Since coverage is area, scale by alpha*alpha.
            let alpha = (stroke_width / self.fringe_width).max(0.0).min(1.0);

            let mut inner_color = paint.inner_color();
            inner_color.a *= alpha*alpha;
            paint.set_inner_color(inner_color);

            let mut outer_color = paint.outer_color();
            outer_color.a *= alpha*alpha;
            paint.set_outer_color(outer_color);

            stroke_width = self.fringe_width;
        }

        let mut transform = paint.transform();
        transform.multiply(&self.state().transform);
        paint.set_transform(transform);

        // Apply global alpha
        let mut inner_color = paint.inner_color();
        inner_color.a *= self.state().alpha;
        paint.set_inner_color(inner_color);

        let mut outer_color = paint.outer_color();
        outer_color.a *= self.state().alpha;
        paint.set_outer_color(outer_color);

        if self.renderer.edge_antialiasing() && paint.shape_anti_alias() {
            cache.expand_stroke(stroke_width * 0.5, self.fringe_width, paint.line_cap(), paint.line_join(), paint.miter_limit(), self.tess_tol);
        } else {
            cache.expand_stroke(stroke_width * 0.5, 0.0, paint.line_cap(), paint.line_join(), paint.miter_limit(), self.tess_tol);
        }

        let scissor = &self.state_stack.last().unwrap().scissor;

        self.renderer.render_stroke(&paint, scissor, self.fringe_width, stroke_width, &cache.contours);
    }

    // Text

    pub fn add_font<P: AsRef<FilePath>>(&mut self, file_path: P) {
        self.font_manager.add_font_file(file_path).expect("cannot add font");
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

    pub fn text(&mut self, x: f32, y: f32, text: &str, paint: &Paint) {
        let transform = self.state().transform;
        let scale = self.font_scale() * self.device_px_ratio;
        let invscale = 1.0 / scale;

        let mut paint = paint.clone();

        //let mut style = FontStyle::new("DroidSerif");
        //let mut style = FontStyle::new("Roboto-Regular");
        //let mut style = FontStyle::new("Amiri-Regular");
        //let mut style = FontStyle::new("NotoSansDevanagari-Regular");
        let mut style = FontStyle::new(paint.font_name());

        style.set_size((paint.font_size() as f32 * scale) as u32);
        style.set_letter_spacing(paint.letter_spacing() * scale);
        style.set_blur(paint.font_blur() * scale);

        let layout = self.font_manager.layout_text(x, y, &mut self.renderer, style, text).unwrap();

        for cmd in &layout.cmds {
            let mut verts = Vec::new();

            for quad in &cmd.quads {
                let (mut p0, mut p1, mut p2, mut p3, mut p4, mut p5, mut p6, mut p7) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

                transform.transform_point(&mut p0, &mut p1, quad.x0*invscale, quad.y0*invscale);
                transform.transform_point(&mut p2, &mut p3, quad.x1*invscale, quad.y0*invscale);
                transform.transform_point(&mut p4, &mut p5, quad.x1*invscale, quad.y1*invscale);
                transform.transform_point(&mut p6, &mut p7, quad.x0*invscale, quad.y1*invscale);

                verts.push(Vertex::new(p0, p1, quad.s0, quad.t0));
                verts.push(Vertex::new(p4, p5, quad.s1, quad.t1));
                verts.push(Vertex::new(p2, p3, quad.s1, quad.t0));
                verts.push(Vertex::new(p0, p1, quad.s0, quad.t0));
                verts.push(Vertex::new(p6, p7, quad.s0, quad.t1));
                verts.push(Vertex::new(p4, p5, quad.s1, quad.t1));
            }

            paint.set_image(Some(cmd.image_id));

            // Apply global alpha
            let mut inner_color = paint.inner_color();
            inner_color.a *= self.state().alpha;
            paint.set_inner_color(inner_color);

            let mut outer_color = paint.outer_color();
            outer_color.a *= self.state().alpha;
            paint.set_outer_color(outer_color);

            let scissor = &self.state_stack.last().unwrap().scissor;

            self.renderer.render_triangles(&paint, scissor, &verts);
        }
    }

    // Private

    fn font_scale(&self) -> f32 {
        let avg_scale = self.state().transform.average_scale();

        quantize(avg_scale, 0.01).min(4.0)
    }

    fn state(&self) -> &State {
        self.state_stack.last().unwrap()
    }

    fn state_mut(&mut self) -> &mut State {
        self.state_stack.last_mut().unwrap()
    }

    fn set_device_pixel_ratio(&mut self, ratio: f32) {
        self.tess_tol = 0.25 / ratio;
        self.dist_tol = 0.01 / ratio;
        self.fringe_width = 1.0 / ratio;
        self.device_px_ratio = ratio;
    }
}

fn triarea2(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let acx = cx - ax;
    let acy = cy - ay;

    acx*aby - abx*acy
}

fn pt_equals(x1: f32, y1: f32, x2: f32, y2: f32, tol: f32) -> bool {
    let dx = x2 - x1;
    let dy = y2 - y1;

    dx*dx + dy*dy < tol*tol
}

fn poly_area(points: &[Point]) -> f32 {
    let mut area = 0.0;

    for i in 2..points.len() {
        let p0 = points[0];
        let p1 = points[i-1];
        let p2 = points[i];

        area += triarea2(p0.x, p0.y, p1.x, p1.y, p2.x, p2.y);
    }

    area * 0.5
}

fn cross(dx0: f32, dy0: f32, dx1: f32, dy1: f32) -> f32 {
    dx1*dy0 - dx0*dy1
}

fn dist_pt_segment(x: f32, y: f32, px: f32, py: f32, qx: f32, qy: f32) -> f32 {
    let pqx = qx-px;
    let pqy = qy-py;
    let dx = x-px;
    let dy = y-py;
    let d = pqx*pqx + pqy*pqy;
    let mut t = pqx*dx + pqy*dy;

    if d > 0.0 { t /= d; }

    if t < 0.0 { t = 0.0; }
    else if t > 1.0 { t = 1.0; }

    let dx = px + t*pqx - x;
    let dy = py + t*pqy - y;

    dx*dx + dy*dy
}

fn quantize(a: f32, d: f32) -> f32 {
    (a / d + 0.5).trunc() * d
}

// TODO: fix this.. move it to point
fn normalize(x: &mut f32, y: &mut f32) -> f32 {
    let d = ((*x)*(*x) + (*y)*(*y)).sqrt();

    if d > 1e-6 {
        let id = 1.0 / d;
        *x *= id;
        *y *= id;
    }

    d
}

#[derive(Debug)]
pub enum CanvasError {
    GeneralError(String),
    ImageError(image::ImageError),
    FontError(FontManagerError)
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

impl From<FontManagerError> for CanvasError {
    fn from(error: FontManagerError) -> Self {
        Self::FontError(error)
    }
}

impl Error for CanvasError {}
