
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
pub use font_manager::{FontManager, FontStyle, FontManagerError, GlyphRenderStyle};

pub mod math;
use crate::math::*;

mod paint;
pub use paint::Paint;

mod path;
pub use path::{CachedPath, Path};

// TODO: Rename tess_tol and dist_tol to tesselation_tolerance and distance_tolerance
// TODO: Drawing works before the call to begin frame for some reason
// TODO: rethink image creation and resource creation in general, it's currently blocking,
//         it would be awesome if its non-blocking and maybe async. Or maybe resource creation
//         should be a functionality provided by the current renderer implementation, not by the canvas itself.

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
        const FLIP_Y = 1 << 3;          // Flips (inverses) image in Y direction when rendered.
        const PREMULTIPLIED = 1 << 4;   // Image data has premultiplied alpha.
        const NEAREST = 1 << 5;         // Image interpolation is Nearest instead Linear
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

    pub fn set_size(&mut self, width: u32, height: u32, dpi: f32) {
        self.renderer.set_size(width, height, dpi);
    }

    pub fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        self.renderer.clear_rect(x, y, width, height, color);
    }

    /*
    pub fn begin_frame(&mut self, window_width: f32, window_height: f32, device_px_ratio: f32) {

        self.set_device_pixel_ratio(device_px_ratio);

        //self.renderer.render_viewport(window_width, window_height);
    }*/

    pub fn end_frame(&mut self) {
        self.renderer.flush();

        self.state_stack.clear();
        self.save();
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
    pub fn fill_path(&mut self, path: &Path, paint: &Paint) {
        let mut paint = paint.clone();
        let mut path = path.clone();

        // Transform path
        path.transform(self.state().transform);

        // Transform paint
        let mut transform = paint.transform();
        transform.multiply(&self.state().transform);
        paint.set_transform(transform);

        // Apply global alpha
        paint.inner_color_mut().a *= self.state().alpha;
        paint.outer_color_mut().a *= self.state().alpha;

        let scissor = self.state().scissor;

        //self.renderer.fill(&paint, scissor, self.fringe_width, cache.bounds, &cache.contours);
        self.renderer.fill(&paint, &scissor, &path);
    }

    /// Fills the current path with current stroke style.
    pub fn stroke_path(&mut self, path: &Path, paint: &Paint) {
        let scale = self.state().transform.average_scale();

        let mut paint = paint.clone();
        let mut path = path.clone();

        // Transform path
        path.transform(self.state().transform);

        // Transform paint
        let mut transform = paint.transform();
        transform.multiply(&self.state().transform);
        paint.set_transform(transform);

        // Scale stroke width by current transform scale
        paint.set_stroke_width((paint.stroke_width() * scale).max(0.0).min(200.0));

        if paint.stroke_width() < self.fringe_width {
            // If the stroke width is less than pixel size, use alpha to emulate coverage.
            // Since coverage is area, scale by alpha*alpha.
            let alpha = (paint.stroke_width() / self.fringe_width).max(0.0).min(1.0);

            paint.inner_color_mut().a *= alpha*alpha;
            paint.outer_color_mut().a *= alpha*alpha;
            paint.set_stroke_width(self.fringe_width)
        }

        // Apply global alpha
        paint.inner_color_mut().a *= self.state().alpha;
        paint.outer_color_mut().a *= self.state().alpha;

        let scissor = self.state().scissor;

        self.renderer.stroke(&paint, &scissor, &path);
    }

    // Text

    pub fn add_font<P: AsRef<FilePath>>(&mut self, file_path: P) {
        self.font_manager.add_font_file(file_path).expect("cannot add font");
    }

    pub fn add_font_mem(&mut self, data: Vec<u8>) {
        self.font_manager.add_font_mem(data).expect("cannot add font");
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

    pub fn fill_text(&mut self, x: f32, y: f32, text: &str, paint: &Paint) {
        self.draw_text(x, y, text, paint, GlyphRenderStyle::Fill);
    }

    pub fn stroke_text(&mut self, x: f32, y: f32, text: &str, paint: &Paint) {
        self.draw_text(x, y, text, paint, GlyphRenderStyle::Stroke {
            line_width: paint.stroke_width().ceil() as u32
        });
    }

    // Private

    fn draw_text(&mut self, x: f32, y: f32, text: &str, paint: &Paint, render_style: GlyphRenderStyle) {
        let transform = self.state().transform;
        let scale = self.font_scale() * self.device_px_ratio;
        let invscale = 1.0 / scale;

        let mut paint = paint.clone();

        let mut style = FontStyle::new(paint.font_name());
        style.set_size((paint.font_size() as f32 * scale) as u32);
        style.set_letter_spacing((paint.letter_spacing() as f32 * scale) as i32);
        style.set_blur(paint.font_blur() * scale);
        style.set_render_style(render_style);

        let layout = self.font_manager.layout_text(x, y, self.renderer.as_mut(), style, text).unwrap();

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

            self.renderer.triangles(&paint, scissor, &verts);
        }
    }

    fn font_scale(&self) -> f32 {
        let avg_scale = self.state().transform.average_scale();

        math::quantize(avg_scale, 0.01).min(4.0)
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
