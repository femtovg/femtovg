
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
use geometry::*;
pub use geometry::Transform2D;

mod paint;
pub use paint::Paint;
use paint::PaintFlavor;

mod path;
pub use path::{Path, Winding, Verb};

mod path_cache;
use path_cache::{PathCache, Convexity};

// TODO: path_contains_point method

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

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub enum BlendFactor {
    Zero,
    One,
    SrcColor,
    OneMinusSrcColor,
    DstColor,
    OneMinusDstColor,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstAlpha,
    OneMinusDstAlpha,
    SrcAlphaSaturate
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub enum CompositeOperation {
    SourceOver,
    SourceIn,
    SourceOut,
    Atop,
    DestinationOver,
    DestinationIn,
    DestinationOut,
    DestinationAtop,
    Lighter,
    Copy,
    Xor
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub struct CompositeOperationState {
    src_rgb: BlendFactor,
    src_alpha: BlendFactor,
    dst_rgb: BlendFactor,
    dst_alpha: BlendFactor,
}

impl CompositeOperationState {
    pub fn new(op: CompositeOperation) -> Self {
        let (sfactor, dfactor) = match op {
            CompositeOperation::SourceOver => (BlendFactor::One, BlendFactor::OneMinusSrcAlpha),
            CompositeOperation::SourceIn => (BlendFactor::DstAlpha, BlendFactor::Zero),
            CompositeOperation::SourceOut => (BlendFactor::OneMinusDstAlpha, BlendFactor::Zero),
            CompositeOperation::Atop => (BlendFactor::DstAlpha, BlendFactor::OneMinusSrcAlpha),
            CompositeOperation::DestinationOver => (BlendFactor::OneMinusDstAlpha, BlendFactor::One),
            CompositeOperation::DestinationIn => (BlendFactor::Zero, BlendFactor::SrcAlpha),
            CompositeOperation::DestinationOut => (BlendFactor::Zero, BlendFactor::OneMinusSrcAlpha),
            CompositeOperation::DestinationAtop => (BlendFactor::OneMinusDstAlpha, BlendFactor::SrcAlpha),
            CompositeOperation::Lighter => (BlendFactor::One, BlendFactor::One),
            CompositeOperation::Copy => (BlendFactor::One, BlendFactor::Zero),
            CompositeOperation::Xor => (BlendFactor::OneMinusDstAlpha, BlendFactor::OneMinusSrcAlpha)
        };

        Self {
            src_rgb: sfactor,
            src_alpha: sfactor,
            dst_rgb: dfactor,
            dst_alpha: dfactor,
        }
    }
}

impl Default for CompositeOperationState {
    fn default() -> Self {
        Self::new(CompositeOperation::SourceOver)
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
    composite_operation: CompositeOperationState,
    transform: Transform2D,
    scissor: Scissor,
    alpha: f32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            composite_operation: Default::default(),
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
    cmds: Vec<Command>,
    verts: Vec<Vertex>,
    fringe_width: f32,
    device_px_ratio: f32,
    tess_tol: f32,
    dist_tol: f32
}

impl<T> Canvas<T> where T: Renderer {

    pub fn new(renderer: T) -> Result<Self, CanvasError> {
        let font_manager = FontCache::new()?;

        let mut canvas = Self {
            width: Default::default(),
            height: Default::default(),
            renderer: renderer,
            font_cache: font_manager,
            state_stack: Default::default(),
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
        //self.state_stack.clear();
        //self.save();
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

    /// Sets the composite operation.
    pub fn global_composite_operation(&mut self, op: CompositeOperation) {
        self.state_mut().composite_operation = CompositeOperationState::new(op);
    }

    /// Sets the composite operation with custom pixel arithmetic.
    pub fn global_composite_blend_func(&mut self, src_factor: BlendFactor, dst_factor: BlendFactor) {
        self.global_composite_blend_func_separate(src_factor, dst_factor, src_factor, dst_factor);
    }

    /// Sets the composite operation with custom pixel arithmetic for RGB and alpha components separately.
    pub fn global_composite_blend_func_separate(&mut self, src_rgb: BlendFactor, dst_rgb: BlendFactor, src_alpha: BlendFactor, dst_alpha: BlendFactor) {
        self.state_mut().composite_operation = CompositeOperationState { src_rgb, src_alpha, dst_rgb, dst_alpha }
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
        self.renderer.create_image(image, flags).unwrap()
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

    pub fn set_transform(&mut self, t: Transform2D) {
        //let transform = Transform2D([a, b, c, d, e, f]);
        self.state_mut().transform = t;
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

    pub fn transformed_point(&self, x: f32, y: f32) -> (f32, f32) {
        self.state().transform.transform_point(x, y)
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

    /// Fills the current path with current fill style.
    pub fn fill_path(&mut self, path: &mut Path, mut paint: Paint) {
        //self.path_cache.set(&self.verbs, self.tess_tol, self.dist_tol);
        let transform = self.state().transform;

        let mut path_cache = PathCache::new(path, &transform, self.tess_tol, self.dist_tol);

        // Transform paint
        paint.transform = transform;

        // Apply global alpha
        paint.mul_alpha(self.state().alpha);

        let scissor = self.state().scissor;

        if paint.shape_anti_alias() {
            path_cache.expand_fill(self.fringe_width, LineJoin::Miter, 2.4, self.fringe_width);
        } else {
            path_cache.expand_fill(0.0, LineJoin::Miter, 2.4, self.fringe_width);
        }

        let flavor = if path_cache.contours.len() == 1 && path_cache.contours[0].convexity == Convexity::Convex {
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
        cmd.composite_operation = self.state().composite_operation;

        if let PaintFlavor::Image { id, .. } = paint.flavor {
            cmd.image = Some(id);
        }

        let mut offset = self.verts.len();

        for contour in &path_cache.contours {
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
            self.verts.push(Vertex::new(path_cache.bounds.maxx, path_cache.bounds.maxy, 0.5, 1.0));
            self.verts.push(Vertex::new(path_cache.bounds.maxx, path_cache.bounds.miny, 0.5, 1.0));
            self.verts.push(Vertex::new(path_cache.bounds.minx, path_cache.bounds.maxy, 0.5, 1.0));
            self.verts.push(Vertex::new(path_cache.bounds.minx, path_cache.bounds.miny, 0.5, 1.0));

            cmd.triangles_verts = Some((offset, 4));
        }

        self.cmds.push(cmd);
    }

    /// Fills the current path with current stroke style.
    pub fn stroke_path(&mut self, path: &mut Path, mut paint: Paint) {
        let scissor = self.state().scissor;
        let transform = self.state().transform;
        let scale = transform.average_scale();

        let mut path_cache = PathCache::new(path, &transform, self.tess_tol, self.dist_tol);

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
            path_cache.expand_stroke(paint.stroke_width() * 0.5, self.fringe_width, paint.line_cap(), paint.line_join(), paint.miter_limit(), self.tess_tol);
        } else {
            path_cache.expand_stroke(paint.stroke_width() * 0.5, 0.0, paint.line_cap(), paint.line_join(), paint.miter_limit(), self.tess_tol);
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
        cmd.composite_operation = self.state().composite_operation;

        if let PaintFlavor::Image { id, .. } = paint.flavor {
            cmd.image = Some(id);
        }

        let mut offset = self.verts.len();

        for contour in &path_cache.contours {
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

    pub fn text_bounds(&mut self, x: f32, y: f32, text: &str, mut paint: Paint) -> [f32; 4] {
        let scale = self.font_scale() * self.device_px_ratio;
        let invscale = 1.0 / scale;

        // transform paint
        paint.set_font_size((paint.font_size() as f32 * scale) as u32);
        paint.set_letter_spacing((paint.letter_spacing() as f32 * scale) as i32);
        paint.set_font_blur(paint.font_blur() * scale);

        let layout = self.font_cache.layout_text(x, y, &mut self.renderer, paint, GlyphRenderStyle::Fill, text).unwrap();

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
    }

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
        cmd.composite_operation = self.state().composite_operation;
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
}

/*
impl<T: Renderer> ttf_parser::OutlineBuilder for Canvas<T> {
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
        self.close_path();
    }
}*/

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

        let mut path = Path::new();
        path.move_to(10.0, 10.0);
        canvas.fill_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
        canvas.stroke_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
    }

    #[test]
    fn path_with_two_lines_to_does_not_panic() {
        let mut canvas = Canvas::new(Void).unwrap();

        let mut path = Path::new();
        path.line_to(10.0, 10.0);
        path.line_to(10.0, 10.0);
        canvas.fill_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
        canvas.stroke_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
    }

    #[test]
    fn self_intersecting_polygon_is_concave() {
        // star
        let mut path = Path::new();
        path.move_to(50.0, 0.0);
        path.line_to(21.0, 90.0);
        path.line_to(98.0, 35.0);
        path.line_to(2.0, 35.0);
        path.line_to(79.0, 90.0);
        path.close();

        let transform = Transform2D::identity();

        let mut path_cache = PathCache::new(&path, &transform, 0.25, 0.01);
        path_cache.expand_fill(1.0, LineJoin::Miter, 10.0, 1.0);

        assert_eq!(path_cache.contours[0].convexity, Convexity::Concave);
    }
}
