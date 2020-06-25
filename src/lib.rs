
use std::path::Path as FilePath;

use rgb::RGBA8;
use imgref::ImgVec;

/*
HTML5 Canvas API:
https://bucephalus.org/text/CanvasHandbook/CanvasHandbook.html

TODO:
    - Rename text helper to renderer or something
    - Review text positioning mess
        - evaluate all the maths that is happening from shaping to drawing
        - arabic scripts need to get fixed
    - Text todos:
        - Review Font api and move shared functionality from the shaper & renderer to it
        - Laying out paragraphs - iterator design + correct breaking
        - Review if TextStyle struct is even needed - it's best to use paint itself
        - Measuring text - text_bounds?
        - Computing bounding boxes - text_bounds?
        - Mapping from coordinates to character indices
        - Mapping from character index to coordinates
        - Review font db design - do we need it in it's current form - a huge simplification would be for a paint to just accept an array of font ids
    - Fix blurring
    - Finish demo text
    - Integrate euclid
    - Canvas push state with callback auto pop
    - Documentation
    - Rename crate to femtovg
    - Tests
    - Publish to crates.io
    - Emoji support
*/

mod utils;

mod text;

mod error;
pub use error::ErrorKind;

pub use text::{
    Weight,
    WidthClass,
    FontStyle,
    Baseline,
    Align,
    TextLayout
};

use text::{
    FontDb,
    Shaper,
    TextStyle,
    RenderStyle,
    TextHelperContext
};

mod image;
pub use crate::image::{
    ImageId,
    ImageInfo,
    ImageFlags,
    ImageStore,
    PixelFormat,
    ImageSource,
};

mod color;
pub use color::Color;

pub mod renderer;
pub use renderer::{
    Renderer,
    RenderTarget
};

use renderer::{
    Vertex,
    Params,
    Command,
    CommandType,
    ShaderType,
    Drawable,
};

pub(crate) mod geometry;
use geometry::*;
pub use geometry::Transform2D;

mod paint;
pub use paint::Paint;
use paint::PaintFlavor;

mod path;
use path::Convexity;
pub use path::{
    Path,
    Solidity
};

type Result<T> = std::result::Result<T, ErrorKind>;

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

pub struct Canvas<T: Renderer> {
    width: u32,
    height: u32,
    renderer: T,
    fontdb: FontDb,
    shaper: Shaper,
    text_helper_context: TextHelperContext,// TODO: rename this
    current_render_target: RenderTarget,
    state_stack: Vec<State>,
    commands: Vec<Command>,
    verts: Vec<Vertex>,
    images: ImageStore<T::Image>,
    fringe_width: f32,
    device_px_ratio: f32,
    tess_tol: f32,
    dist_tol: f32
}

impl<T> Canvas<T> where T: Renderer {

    pub fn new(renderer: T) -> Result<Self> {
        let fontdb = FontDb::new()?;

        let mut canvas = Self {
            width: 0,
            height: 0,
            renderer: renderer,
            fontdb: fontdb,
            shaper: Default::default(),
            text_helper_context: Default::default(),
            current_render_target: RenderTarget::Screen,
            state_stack: Default::default(),
            commands: Default::default(),
            verts: Default::default(),
            images: ImageStore::new(),
            fringe_width: 1.0,
            device_px_ratio: 1.0,
            tess_tol: 0.25,
            dist_tol: 0.01
        };

        canvas.save();

        Ok(canvas)
    }

    pub fn set_size(&mut self, width: u32, height: u32, dpi: f32) {
        self.width = width;
        self.height = height;
        self.fringe_width = 1.0 / dpi;
        self.tess_tol = 0.25 / dpi;
        self.dist_tol = 0.01 / dpi;
        self.device_px_ratio = dpi;
        
        self.renderer.set_size(width, height, dpi);
        
        self.append_cmd(Command::new(CommandType::SetRenderTarget(RenderTarget::Screen)));
    }

    pub fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        let cmd = Command::new(CommandType::ClearRect {
            x, y, width, height, color
        });

        self.append_cmd(cmd);
    }

    /// Returns the with of the canvas
    pub fn width(&self) -> f32 {
        self.width as f32
    }

    /// Returns the height of the canvas
    pub fn height(&self) -> f32 {
        self.height as f32
    }

    /// Tells the renderer to execute all drawing commands and clears the current internal state
    ///
    /// Call this at the end of rach frame.
    pub fn flush(&mut self) {
        self.renderer.render(&self.images, &self.verts, &self.commands);
        self.commands.clear();
        self.verts.clear();
    }

    pub fn screenshot(&mut self) -> Result<ImgVec<RGBA8>> {
        self.flush();
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
    ///
    /// Restoring the initial/first state will just reset it to the defaults
    pub fn restore(&mut self) {
        if self.state_stack.len() > 1 {
            self.state_stack.pop();
        } else {
            self.reset();
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

    // TODO: remove self.current_render_target and check in this method if target is different than current render target
    pub fn set_render_target(&mut self, target: RenderTarget) {
        //self.flush();
        //self.renderer.set_target(&self.images, target);
        if self.current_render_target != target {
            //self.render_targets.push((target, Vec::new()));
            self.append_cmd(Command::new(CommandType::SetRenderTarget(target)));
            self.current_render_target = target;
        }
    }

    fn append_cmd(&mut self, cmd: Command) {
        self.commands.push(cmd);
    }

    // Images

    pub fn create_image_empty(&mut self, width: usize, height: usize, format: PixelFormat, flags: ImageFlags) -> Result<ImageId> {
        let info = ImageInfo::new(flags, width, height, format);

        self.images.alloc(&mut self.renderer, info)
    }

    pub fn create_image<'a, S: Into<ImageSource<'a>>>(&mut self, src: S, flags: ImageFlags) -> Result<ImageId> {
        let src = src.into();
        let size = src.dimensions();

        let id = self.create_image_empty(size.0, size.1, src.format(), flags)?;

        self.images.update(&mut self.renderer, id, src, 0, 0)?;

        Ok(id)
    }

    /// Decode an image from file
    #[cfg(feature = "image-loading")]
    pub fn load_image_file<P: AsRef<FilePath>>(&mut self, filename: P, flags: ImageFlags) -> Result<ImageId> {
        let image = ::image::open(filename)?;

        use std::convert::TryFrom;

        let src = ImageSource::try_from(&image)?;

        self.create_image(src, flags)
    }

    /// Decode an image from memory
    #[cfg(feature = "image-loading")]
    pub fn load_image_mem(&mut self, data: &[u8], flags: ImageFlags) -> Result<ImageId> {
        let image = ::image::load_from_memory(data)?;

        use std::convert::TryFrom;

        let src = ImageSource::try_from(&image)?;

        self.create_image(src, flags)
    }

    /// Updates image data specified by image handle.
    pub fn update_image<'a, S: Into<ImageSource<'a>>>(&mut self, id: ImageId, src: S, x: usize, y: usize) -> Result<()> {
        self.images.update(&mut self.renderer, id, src.into(), x, y)
    }

    /// Deletes created image.
    pub fn delete_image(&mut self, id: ImageId) {
        self.images.remove(&mut self.renderer, id);
    }

    /// Blurs the provided image inside the specified region.
    /// Amount is the number of times that the blur is ran against the image.
    pub fn blur_image(&mut self, id: ImageId, amount: u8, x: usize, y: usize, width: usize, height: usize) {
        if let Some(img) = self.images.get_mut(id) {
            self.renderer.blur(img, amount, x, y, width, height);
        }
    }

    /// Returns the size in pixels of the image for the specified id.
    pub fn image_size(&self, id: ImageId) -> Result<(usize, usize)> {
        if let Some(info) = self.images.info(id) {
            Ok((info.width(), info.height()))
        } else {
            Err(ErrorKind::ImageIdNotFound)
        }
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
    pub fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
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
    ///
    /// TODO: It's not ok that this method returns Transform2D while set_transform accepts 6 floats - make it consistant
    pub fn transform(&self) -> Transform2D {
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
        transform.multiply(&state.transform);
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

    pub fn contains_point(&mut self, path: &mut Path, x: f32, y: f32, fill_rule: FillRule) -> bool {
        let transform = self.state().transform;

        // The path cache saves a flattened and transformed version of the path.
        let path_cache = path.cache(&transform, self.tess_tol, self.dist_tol);

        // Early out if path is outside the canvas bounds
        if path_cache.bounds.maxx < 0.0 || path_cache.bounds.minx > self.width() ||
            path_cache.bounds.maxy < 0.0 || path_cache.bounds.miny > self.height() {
            return false;
        }

        path_cache.contains_point(x, y, fill_rule)
    }

    pub fn path_bbox(&self, path: &mut Path) -> Bounds {
        let transform = self.state().transform;

        // The path cache saves a flattened and transformed version of the path.
        let path_cache = path.cache(&transform, self.tess_tol, self.dist_tol);

        path_cache.bounds
    }

    /// Fills the current path with current fill style.
    pub fn fill_path(&mut self, path: &mut Path, mut paint: Paint) {
        let transform = self.state().transform;

        // The path cache saves a flattened and transformed version of the path.
        let path_cache = path.cache(&transform, self.tess_tol, self.dist_tol);

        // Early out if path is outside the canvas bounds
        if path_cache.bounds.maxx < 0.0 || path_cache.bounds.minx > self.width() ||
            path_cache.bounds.maxy < 0.0 || path_cache.bounds.miny > self.height() {
            return;
        }

        // Transform paint
        paint.transform = transform;

        // Apply global alpha
        paint.mul_alpha(self.state().alpha);

        let scissor = self.state().scissor;

        // Calculate fill vertices.
        // expand_fill will fill path_cache.contours[].{stroke, fill} with vertex data for the GPU
        // fringe_with is the size of the strip of triangles generated at the path border used for AA
        let fringe_with = if paint.anti_alias() { self.fringe_width } else { 0.0 };
        path_cache.expand_fill(fringe_with, LineJoin::Miter, 2.4);

        // GPU uniforms
        let flavor = if path_cache.contours.len() == 1 && path_cache.contours[0].convexity == Convexity::Convex {
            let params = Params::new(&self.images, &paint, &scissor, self.fringe_width, self.fringe_width, -1.0);

            CommandType::ConvexFill { params }
        } else {
            let mut stencil_params = Params::default();
            stencil_params.stroke_thr = -1.0;
            stencil_params.shader_type = ShaderType::Stencil.to_f32();

            let fill_params = Params::new(&self.images, &paint, &scissor, self.fringe_width, self.fringe_width, -1.0);

            CommandType::ConcaveFill { stencil_params, fill_params }
        };

        // GPU command
        let mut cmd = Command::new(flavor);
        cmd.fill_rule = paint.fill_rule;
        cmd.composite_operation = self.state().composite_operation;

        if let PaintFlavor::Image { id, .. } = paint.flavor {
            cmd.image = Some(id);
        }

        // All verts from all shapes are kept in a single buffer here in the canvas.
        // Drawable struct is used to describe the range of vertices each draw call will operate on
        let mut offset = self.verts.len();

        for contour in &path_cache.contours {
            let mut drawable = Drawable::default();

            // Fill commands can have both fill and stroke vertices. Fill vertices are used to fill
            // the body of the shape while stroke vertices are used to prodice antialiased edges

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
            // Concave shapes are first filled by writing to a stencil buffer and then drawing a quad
            // over the shape area with stencil test enabled to produce the final fill. These are
            // the verts needed for the covering quad
            self.verts.push(Vertex::new(path_cache.bounds.maxx, path_cache.bounds.maxy, 0.5, 1.0));
            self.verts.push(Vertex::new(path_cache.bounds.maxx, path_cache.bounds.miny, 0.5, 1.0));
            self.verts.push(Vertex::new(path_cache.bounds.minx, path_cache.bounds.maxy, 0.5, 1.0));
            self.verts.push(Vertex::new(path_cache.bounds.minx, path_cache.bounds.miny, 0.5, 1.0));

            cmd.triangles_verts = Some((offset, 4));
        }

        self.append_cmd(cmd);
    }

    /// Strokes the provided Path using Paint.
    pub fn stroke_path(&mut self, path: &mut Path, mut paint: Paint) {
        let transform = self.state().transform;

        // The path cache saves a flattened and transformed version of the path.
        let path_cache = path.cache(&transform, self.tess_tol, self.dist_tol);

        // Early out if path is outside the canvas bounds
        if path_cache.bounds.maxx < 0.0 || path_cache.bounds.minx > self.width() ||
            path_cache.bounds.maxy < 0.0 || path_cache.bounds.miny > self.height() {
            return;
        }

        let scissor = self.state().scissor;

        // Transform paint
        paint.transform = transform;

        // Scale stroke width by current transform scale.
        // Note: I don't know why the original author clamped the max stroke width to 200, but it didn'
        // look correct when zooming in. There was probably a good reson for doing so and I may have
        // introduced a bug by removing the upper bound.
        //paint.set_stroke_width((paint.stroke_width() * transform.average_scale()).max(0.0).min(200.0));
        paint.set_stroke_width((paint.stroke_width() * transform.average_scale()).max(0.0));

        if paint.stroke_width() < self.fringe_width {
            // If the stroke width is less than pixel size, use alpha to emulate coverage.
            // Since coverage is area, scale by alpha*alpha.
            let alpha = (paint.stroke_width() / self.fringe_width).max(0.0).min(1.0);

            paint.mul_alpha(alpha*alpha);
            paint.set_stroke_width(self.fringe_width)
        }

        // Apply global alpha
        paint.mul_alpha(self.state().alpha);

        // Calculate stroke vertices.
        // expand_stroke will fill path_cache.contours[].stroke with vertex data for the GPU
        let fringe_with = if paint.anti_alias() { self.fringe_width } else { 0.0 };
        path_cache.expand_stroke(
            paint.stroke_width() * 0.5,
            fringe_with,
            paint.line_cap_start,
            paint.line_cap_end,
            paint.line_join(),
            paint.miter_limit(),
            self.tess_tol
        );

        // GPU uniforms
        let params = Params::new(&self.images, &paint, &scissor, paint.stroke_width(), self.fringe_width, -1.0);

        let flavor = if paint.stencil_strokes() {
            let params2 = Params::new(&self.images, &paint, &scissor, paint.stroke_width(), self.fringe_width, 1.0 - 0.5/255.0);

            CommandType::StencilStroke { params1: params, params2 }
        } else {
            CommandType::Stroke { params }
        };

        // GPU command
        let mut cmd = Command::new(flavor);
        cmd.composite_operation = self.state().composite_operation;

        if let PaintFlavor::Image { id, .. } = paint.flavor {
            cmd.image = Some(id);
        }

        // All verts from all shapes are kept in a single buffer here in the canvas.
        // Drawable struct is used to describe the range of vertices each draw call will operate on
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

        self.append_cmd(cmd);
    }

    // Text

    pub fn add_font<P: AsRef<FilePath>>(&mut self, file_path: P) -> Result<()> {
        self.fontdb.add_font_file(file_path)?;
        self.shaper.clear_cache();
        Ok(())
    }

    pub fn scan_font_dir<P: AsRef<FilePath>>(&mut self, dir_path: P) -> Result<()> {
        self.fontdb.scan_dir(dir_path)?;
        self.shaper.clear_cache();
        Ok(())
    }

    pub fn add_font_mem(&mut self, data: Vec<u8>) -> Result<()> {
        self.fontdb.add_font_mem(data)?;
        self.shaper.clear_cache();
        Ok(())
    }

    pub fn layout_text<S: AsRef<str>>(&mut self, x: f32, y: f32, text: S, paint: Paint) -> Result<TextLayout> {
        let text = text.as_ref();
        let scale = self.font_scale() * self.device_px_ratio;
        let style = self.text_style_for_paint(&paint);
        let invscale = 1.0 / scale;

        let mut layout = self.shaper.shape(x * scale, y * scale, &mut self.fontdb, &style, text)?;
        layout.width *= invscale;
        layout.height *= invscale;

        Ok(layout)
    }

    pub fn fill_text<S: AsRef<str>>(&mut self, x: f32, y: f32, text: S, paint: Paint) -> Result<TextLayout> {
        let text = text.as_ref();
        self.draw_text(x, y, text, paint, RenderStyle::Fill)
    }

    pub fn stroke_text<S: AsRef<str>>(&mut self, x: f32, y: f32, text: S, paint: Paint) -> Result<TextLayout> {
        let text = text.as_ref();

        self.draw_text(x, y, text, paint, RenderStyle::Stroke {
            width: paint.stroke_width().ceil() as u16// TODO: this is fishy
        })
    }

    // Private

    fn text_style_for_paint<'a>(&self, paint: &'a Paint) -> TextStyle<'a> {
        let scale = self.font_scale() * self.device_px_ratio;
        TextStyle {
            family_name: paint.font_family,
            size: (paint.font_size() as f32 * scale) as u16,
            weight: paint.font_weight(),
            width_class: paint.font_width_class(),
            font_style: paint.font_style(),
            letter_spacing: paint.letter_spacing() * scale,
            baseline: paint.text_baseline(),
            align: paint.text_align(),
            blur: paint.font_blur().saturating_mul(scale.min(255.0) as u8),
            render_style: Default::default()
        }
    }

    fn draw_text(&mut self, x: f32, y: f32, text: &str, mut paint: Paint, render_style: RenderStyle) -> Result<TextLayout> {
        let transform = self.state().transform;
        let scissor = self.state().scissor;
        let scale = self.font_scale() * self.device_px_ratio;
        let invscale = 1.0 / scale;

        let mut style = self.text_style_for_paint(&paint);
        style.render_style = render_style;

        //let layout = self.shaper.shape(x * scale, y * scale, &mut self.fontdb, &style, text)?;
        let layout = self.layout_text(x, y, text, paint)?;

        // TODO: Early out if text is outside the canvas bounds, or maybe even check for each character in layout.

        if style.size > 40 {
            text::render_text_direct(self, &layout, &style, &paint, invscale)?;
        } else {
            let cmds = text::render_text(self, &layout, &style)?;
            //let cmds = self.text_renderer.render(&mut self.renderer, &mut self.images, &mut self.fontdb, &layout, &style).unwrap();

            for cmd in &cmds {
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

                paint.set_alpha_mask(Some(cmd.image_id));

                // Apply global alpha
                paint.mul_alpha(self.state().alpha);

                self.render_triangles(&verts, &paint, &scissor);
            }
        }
        

        Ok(layout)
    }

    fn render_triangles(&mut self, verts: &[Vertex], paint: &Paint, scissor: &Scissor) {
        let params = Params::new(&self.images, paint, scissor, 1.0, 1.0, -1.0);

        let mut cmd = Command::new(CommandType::Triangles { params });
        cmd.composite_operation = self.state().composite_operation;
        cmd.alpha_mask = paint.alpha_mask();

        if let PaintFlavor::Image { id, .. } = paint.flavor {
            cmd.image = Some(id);
        }

        cmd.triangles_verts = Some((self.verts.len(), verts.len()));
        self.append_cmd(cmd);

        self.verts.extend_from_slice(verts);
    }

    fn font_scale(&self) -> f32 {
        let avg_scale = self.state().transform.average_scale();

        geometry::quantize(avg_scale, 0.1).min(7.0)
    }

    //

    fn state(&self) -> &State {
        self.state_stack.last().unwrap()
    }

    fn state_mut(&mut self) -> &mut State {
        self.state_stack.last_mut().unwrap()
    }
}

impl<T: Renderer> Drop for Canvas<T> {
    fn drop(&mut self) {
        self.images.clear(&mut self.renderer);
    }
}