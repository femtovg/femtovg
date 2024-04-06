/*!
 * The femtovg API is (like [NanoVG](https://github.com/memononen/nanovg))
 * loosely modeled on the
 * [HTML5 Canvas API](https://bucephalus.org/text/CanvasHandbook/CanvasHandbook.html).
 *
 * The coordinate system’s origin is the top-left corner,
 * with positive X rightwards, positive Y downwards.
 */

/*
TODO:
    - Documentation
    - Tests
*/

#[cfg(feature = "serde")]
#[macro_use]
extern crate serde;

use std::{cell::RefCell, ops::Range, path::Path as FilePath, rc::Rc};

use imgref::ImgVec;
use rgb::RGBA8;

mod utils;

mod text;

mod error;
pub use error::ErrorKind;

pub use text::{
    Align, Atlas, Baseline, DrawCommand, FontId, FontMetrics, GlyphDrawCommands, Quad, RenderMode, TextContext,
    TextMetrics,
};

use text::{GlyphAtlas, TextContextImpl};

mod image;
use crate::image::ImageStore;
pub use crate::image::{ImageFilter, ImageFlags, ImageId, ImageInfo, ImageSource, PixelFormat};

mod color;
pub use color::Color;

pub mod renderer;
pub use renderer::{RenderTarget, Renderer};

use renderer::{Command, CommandType, Drawable, Params, ShaderType, Vertex};

pub(crate) mod geometry;
pub use geometry::Transform2D;
use geometry::*;

mod paint;
pub use paint::Paint;
use paint::{GlyphTexture, PaintFlavor, StrokeSettings};

mod path;
use path::Convexity;
pub use path::{Path, PathIter, Solidity, Verb};

mod gradient_store;
use gradient_store::GradientStore;

/// The fill rule used when filling paths: `EvenOdd`, `NonZero` (default).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FillRule {
    EvenOdd,
    NonZero,
}

impl Default for FillRule {
    fn default() -> Self {
        Self::NonZero
    }
}

/// Blend factors.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Hash)]
pub enum BlendFactor {
    /// Not all
    Zero,
    /// All use
    One,
    /// Using the source color
    SrcColor,
    /// Minus the source color
    OneMinusSrcColor,
    /// Using the target color
    DstColor,
    /// Minus the target color
    OneMinusDstColor,
    /// Using the source alpha
    SrcAlpha,
    /// Minus the source alpha
    OneMinusSrcAlpha,
    /// Using the target alpha
    DstAlpha,
    /// Minus the target alpha
    OneMinusDstAlpha,
    /// Scale color by minimum of source alpha and destination alpha
    SrcAlphaSaturate,
}

/// Predefined composite oprations.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Hash)]
pub enum CompositeOperation {
    /// Displays the source over the destination.
    SourceOver,
    /// Displays the source in the destination, i.e. only the part of the source inside the destination is shown and the destination is transparent.
    SourceIn,
    /// Only displays the part of the source that is outside the destination, which is made transparent.
    SourceOut,
    /// Displays the source on top of the destination. The part of the source outside the destination is not shown.
    Atop,
    /// Displays the destination over the source.
    DestinationOver,
    /// Only displays the part of the destination that is inside the source, which is made transparent.
    DestinationIn,
    /// Only displays the part of the destination that is outside the source, which is made transparent.
    DestinationOut,
    /// Displays the destination on top of the source. The part of the destination that is outside the source is not shown.
    DestinationAtop,
    /// Displays the source together with the destination, the overlapping area is rendered lighter.
    Lighter,
    /// Ignores the destination and just displays the source.
    Copy,
    /// Only the areas that exclusively belong either to the destination or the source are displayed. Overlapping parts are ignored.
    Xor,
}

/// Determines how a new ("source") data is displayed against an existing ("destination") data.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Hash)]
pub struct CompositeOperationState {
    src_rgb: BlendFactor,
    src_alpha: BlendFactor,
    dst_rgb: BlendFactor,
    dst_alpha: BlendFactor,
}

impl CompositeOperationState {
    /// Creates a new CompositeOperationState from the provided CompositeOperation
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
            CompositeOperation::Xor => (BlendFactor::OneMinusDstAlpha, BlendFactor::OneMinusSrcAlpha),
        };

        Self {
            src_rgb: sfactor,
            src_alpha: sfactor,
            dst_rgb: dfactor,
            dst_alpha: dfactor,
        }
    }

    /// Creates a new CompositeOperationState with source and destination blend factors.
    pub fn with_blend_factors(src_factor: BlendFactor, dst_factor: BlendFactor) -> Self {
        Self {
            src_rgb: src_factor,
            src_alpha: src_factor,
            dst_rgb: dst_factor,
            dst_alpha: dst_factor,
        }
    }
}

impl Default for CompositeOperationState {
    fn default() -> Self {
        Self::new(CompositeOperation::SourceOver)
    }
}

#[derive(Copy, Clone, Debug, Default)]
struct Scissor {
    transform: Transform2D,
    extent: Option<[f32; 2]>,
}

impl Scissor {
    /// Returns the bounding rect if the scissor clip if it's an untransformed rectangular clip
    fn as_rect(&self, canvas_width: f32, canvas_height: f32) -> Option<Rect> {
        let extent = match self.extent {
            Some(extent) => extent,
            None => return Some(Rect::new(0., 0., canvas_width, canvas_height)),
        };

        // Abort if we're skewing (usually doesn't happen)
        if self.transform[1] != 0.0 || self.transform[2] != 0.0 {
            return None;
        }

        // Abort if we're scaling
        if self.transform[0] != 1.0 || self.transform[3] != 1.0 {
            return None;
        }

        let half_width = extent[0];
        let half_height = extent[1];
        Some(Rect::new(
            self.transform[4] - half_width,
            self.transform[5] - half_height,
            half_width * 2.0,
            half_height * 2.0,
        ))
    }
}

/// Determines the shape used to draw the end points of lines:
/// `Butt` (default), `Round`, `Square`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LineCap {
    /// The ends of lines are squared off at the endpoints. Default value.
    Butt,
    /// The ends of lines are rounded.
    Round,
    /// The ends of lines are squared off by adding a box with an equal
    /// width and half the height of the line's thickness.
    Square,
}

impl Default for LineCap {
    fn default() -> Self {
        Self::Butt
    }
}

/// Determines the shape used to join two line segments where they meet.
/// `Miter` (default), `Round`, `Bevel`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LineJoin {
    /// Connected segments are joined by extending their outside edges to
    /// connect at a single point, with the effect of filling an additional
    /// lozenge-shaped area. This setting is affected by the miterLimit property.
    /// Default value.
    Miter,
    /// Rounds off the corners of a shape by filling an additional sector
    /// of disc centered at the common endpoint of connected segments.
    /// The radius for these rounded corners is equal to the line width.
    Round,
    /// Fills an additional triangular area between the common endpoint
    /// of connected segments, and the separate outside rectangular
    /// corners of each segment.
    Bevel,
}

impl Default for LineJoin {
    fn default() -> Self {
        Self::Miter
    }
}

#[derive(Copy, Clone, Debug)]
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

/// Main 2D drawing context.
pub struct Canvas<T: Renderer> {
    width: u32,
    height: u32,
    renderer: T,
    text_context: Rc<RefCell<TextContextImpl>>,
    glyph_atlas: Rc<GlyphAtlas>,
    // Glyph atlas used for direct rendering of color glyphs, dropped after flush()
    ephemeral_glyph_atlas: Option<Rc<GlyphAtlas>>,
    current_render_target: RenderTarget,
    state_stack: Vec<State>,
    commands: Vec<Command>,
    verts: Vec<Vertex>,
    images: ImageStore<T::Image>,
    fringe_width: f32,
    device_px_ratio: f32,
    tess_tol: f32,
    dist_tol: f32,
    gradients: GradientStore,
}

impl<T> Canvas<T>
where
    T: Renderer,
{
    /// Creates a new canvas.
    pub fn new(renderer: T) -> Result<Self, ErrorKind> {
        let mut canvas = Self {
            width: 0,
            height: 0,
            renderer,
            text_context: Default::default(),
            glyph_atlas: Default::default(),
            ephemeral_glyph_atlas: Default::default(),
            current_render_target: RenderTarget::Screen,
            state_stack: Default::default(),
            commands: Default::default(),
            verts: Default::default(),
            images: ImageStore::new(),
            fringe_width: 1.0,
            device_px_ratio: 1.0,
            tess_tol: 0.25,
            dist_tol: 0.01,
            gradients: GradientStore::new(),
        };

        canvas.save();

        Ok(canvas)
    }

    /// Creates a new canvas with the specified renderer and using the fonts registered with the
    /// provided [`TextContext`]. Note that the context is explicitly shared, so that any fonts
    /// registered with a clone of this context will also be visible to this canvas.
    pub fn new_with_text_context(renderer: T, text_context: TextContext) -> Result<Self, ErrorKind> {
        let mut canvas = Self {
            width: 0,
            height: 0,
            renderer,
            text_context: text_context.0,
            glyph_atlas: Default::default(),
            ephemeral_glyph_atlas: Default::default(),
            current_render_target: RenderTarget::Screen,
            state_stack: Default::default(),
            commands: Default::default(),
            verts: Default::default(),
            images: ImageStore::new(),
            fringe_width: 1.0,
            device_px_ratio: 1.0,
            tess_tol: 0.25,
            dist_tol: 0.01,
            gradients: GradientStore::new(),
        };

        canvas.save();

        Ok(canvas)
    }

    /// Sets the size of the default framebuffer (screen size)
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

    /// Clears the rectangle area defined by left upper corner (x,y), width and height with the provided color.
    pub fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        let cmd = Command::new(CommandType::ClearRect {
            x,
            y,
            width,
            height,
            color,
        });

        self.append_cmd(cmd);
    }

    /// Returns the width of the current render target.
    pub fn width(&self) -> u32 {
        match self.current_render_target {
            RenderTarget::Image(id) => self.image_info(id).map(|info| info.width() as u32).unwrap_or(0),
            RenderTarget::Screen => self.width,
        }
    }

    /// Returns the height of the current render target.
    pub fn height(&self) -> u32 {
        match self.current_render_target {
            RenderTarget::Image(id) => self.image_info(id).map(|info| info.height() as u32).unwrap_or(0),
            RenderTarget::Screen => self.height,
        }
    }

    /// Tells the renderer to execute all drawing commands and clears the current internal state
    ///
    /// Call this at the end of each frame.
    pub fn flush(&mut self) {
        self.renderer
            .render(&mut self.images, &self.verts, std::mem::take(&mut self.commands));
        self.verts.clear();
        self.gradients
            .release_old_gradients(&mut self.images, &mut self.renderer);
        if let Some(atlas) = self.ephemeral_glyph_atlas.take() {
            atlas.clear(self);
        }
    }

    pub fn screenshot(&mut self) -> Result<ImgVec<RGBA8>, ErrorKind> {
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

    /// Resets current state to default values. Does not affect the state stack.
    pub fn reset(&mut self) {
        *self.state_mut() = Default::default();
    }

    /// Saves the current state before calling the callback and restores it afterwards
    ///
    /// This is less error prone than remembering to match save() -> restore() calls
    pub fn save_with(&mut self, mut callback: impl FnMut(&mut Self)) {
        self.save();

        callback(self);

        self.restore();
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
    pub fn global_composite_blend_func_separate(
        &mut self,
        src_rgb: BlendFactor,
        dst_rgb: BlendFactor,
        src_alpha: BlendFactor,
        dst_alpha: BlendFactor,
    ) {
        self.state_mut().composite_operation = CompositeOperationState {
            src_rgb,
            src_alpha,
            dst_rgb,
            dst_alpha,
        }
    }

    /// Sets a new render target. All drawing operations after this call will happen on the provided render target
    pub fn set_render_target(&mut self, target: RenderTarget) {
        if self.current_render_target != target {
            self.append_cmd(Command::new(CommandType::SetRenderTarget(target)));
            self.current_render_target = target;
        }
    }

    fn append_cmd(&mut self, cmd: Command) {
        self.commands.push(cmd);
    }

    // Images

    /// Allocates an empty image with the provided domensions and format.
    pub fn create_image_empty(
        &mut self,
        width: usize,
        height: usize,
        format: PixelFormat,
        flags: ImageFlags,
    ) -> Result<ImageId, ErrorKind> {
        let info = ImageInfo::new(flags, width, height, format);

        self.images.alloc(&mut self.renderer, info)
    }

    /// Allocates an image that wraps the given backend-specific texture.
    /// Use this function to import external textures into the rendering of a scene
    /// with femtovg.
    ///
    /// It is necessary to call `[Self::delete_image`] to free femtovg specific
    /// book-keeping data structures, the underlying backend-specific texture memory
    /// will not be freed. It is the caller's responsible to delete it.
    pub fn create_image_from_native_texture(
        &mut self,
        texture: T::NativeTexture,
        info: ImageInfo,
    ) -> Result<ImageId, ErrorKind> {
        self.images.register_native_texture(&mut self.renderer, texture, info)
    }

    /// Creates image from specified image data.
    pub fn create_image<'a, S: Into<ImageSource<'a>>>(
        &mut self,
        src: S,
        flags: ImageFlags,
    ) -> Result<ImageId, ErrorKind> {
        let src = src.into();
        let size = src.dimensions();
        let id = self.create_image_empty(size.width, size.height, src.format(), flags)?;
        self.images.update(&mut self.renderer, id, src, 0, 0)?;
        Ok(id)
    }

    /// Returns the native texture of an image given its ID.
    pub fn get_native_texture(&self, id: ImageId) -> Result<T::NativeTexture, ErrorKind> {
        self.get_image(id)
            .ok_or(ErrorKind::ImageIdNotFound)
            .and_then(|image| self.renderer.get_native_texture(image))
    }

    pub fn get_image(&self, id: ImageId) -> Option<&T::Image> {
        self.images.get(id)
    }

    pub fn get_image_mut(&mut self, id: ImageId) -> Option<&mut T::Image> {
        self.images.get_mut(id)
    }

    /// Resizes an image to the new provided dimensions.
    pub fn realloc_image(
        &mut self,
        id: ImageId,
        width: usize,
        height: usize,
        format: PixelFormat,
        flags: ImageFlags,
    ) -> Result<(), ErrorKind> {
        let info = ImageInfo::new(flags, width, height, format);
        self.images.realloc(&mut self.renderer, id, info)
    }

    /// Decode an image from file
    #[cfg(feature = "image-loading")]
    pub fn load_image_file<P: AsRef<FilePath>>(
        &mut self,
        filename: P,
        flags: ImageFlags,
    ) -> Result<ImageId, ErrorKind> {
        let image = ::image::open(filename)?;

        let src = ImageSource::try_from(&image)?;

        self.create_image(src, flags)
    }

    /// Decode an image from memory
    #[cfg(feature = "image-loading")]
    pub fn load_image_mem(&mut self, data: &[u8], flags: ImageFlags) -> Result<ImageId, ErrorKind> {
        let image = ::image::load_from_memory(data)?;

        let src = ImageSource::try_from(&image)?;

        self.create_image(src, flags)
    }

    /// Updates image data specified by image handle.
    pub fn update_image<'a, S: Into<ImageSource<'a>>>(
        &mut self,
        id: ImageId,
        src: S,
        x: usize,
        y: usize,
    ) -> Result<(), ErrorKind> {
        self.images.update(&mut self.renderer, id, src.into(), x, y)
    }

    /// Deletes created image.
    pub fn delete_image(&mut self, id: ImageId) {
        self.images.remove(&mut self.renderer, id);
    }

    /// Returns image info
    pub fn image_info(&self, id: ImageId) -> Result<ImageInfo, ErrorKind> {
        if let Some(info) = self.images.info(id) {
            Ok(info)
        } else {
            Err(ErrorKind::ImageIdNotFound)
        }
    }

    /// Returns the size in pixels of the image for the specified id.
    pub fn image_size(&self, id: ImageId) -> Result<(usize, usize), ErrorKind> {
        let info = self.image_info(id)?;
        Ok((info.width(), info.height()))
    }

    /// Renders the given source_image into target_image while applying a filter effect.
    ///
    /// The target image must have the same size as the source image. The filtering is recorded
    /// as a drawing command and run by the renderer when [`Self::flush()`] is called.
    ///
    /// The filtering does not take any transformation set on the Canvas into account nor does it
    /// change the current rendering target.
    pub fn filter_image(&mut self, target_image: ImageId, filter: ImageFilter, source_image: ImageId) {
        let (image_width, image_height) = match self.image_size(source_image) {
            Ok((w, h)) => (w, h),
            Err(_) => return,
        };

        // The renderer will receive a RenderFilteredImage command with two triangles attached that
        // cover the image and the source image.
        let mut cmd = Command::new(CommandType::RenderFilteredImage { target_image, filter });
        cmd.image = Some(source_image);

        let vertex_offset = self.verts.len();

        let image_width = image_width as f32;
        let image_height = image_height as f32;

        let quad_x0 = 0.0;
        let quad_y0 = -image_height;
        let quad_x1 = image_width;
        let quad_y1 = image_height;

        let texture_x0 = -(image_width / 2.);
        let texture_y0 = -(image_height / 2.);
        let texture_x1 = (image_width) / 2.;
        let texture_y1 = (image_height) / 2.;

        self.verts.push(Vertex::new(quad_x0, quad_y0, texture_x0, texture_y0));
        self.verts.push(Vertex::new(quad_x1, quad_y1, texture_x1, texture_y1));
        self.verts.push(Vertex::new(quad_x1, quad_y0, texture_x1, texture_y0));
        self.verts.push(Vertex::new(quad_x0, quad_y0, texture_x0, texture_y0));
        self.verts.push(Vertex::new(quad_x0, quad_y1, texture_x0, texture_y1));
        self.verts.push(Vertex::new(quad_x1, quad_y1, texture_x1, texture_y1));

        cmd.triangles_verts = Some((vertex_offset, 6));

        self.append_cmd(cmd)
    }

    // Transforms

    /// Resets current transform to a identity matrix.
    pub fn reset_transform(&mut self) {
        self.state_mut().transform = Transform2D::identity();
    }

    #[allow(clippy::many_single_char_names)]
    /// Premultiplies current coordinate system by specified transform.
    pub fn set_transform(&mut self, transform: &Transform2D) {
        self.state_mut().transform.premultiply(transform);
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

        let mut pxform = state.scissor.transform;

        let mut invxform = state.transform;
        invxform.inverse();

        pxform.multiply(&invxform);

        let ex = extent[0];
        let ey = extent[1];

        let tex = ex * pxform[0].abs() + ey * pxform[2].abs();
        let tey = ex * pxform[1].abs() + ey * pxform[3].abs();

        let rect = Rect::new(pxform[4] - tex, pxform[5] - tey, tex * 2.0, tey * 2.0);
        let res = rect.intersect(Rect::new(x, y, w, h));

        self.scissor(res.x, res.y, res.w, res.h);
    }

    /// Reset and disables scissoring.
    pub fn reset_scissor(&mut self) {
        self.state_mut().scissor = Scissor::default();
    }

    // Paths

    /// Returns true if the specified point (x,y) is in the provided path, and false otherwise.
    pub fn contains_point(&self, path: &Path, x: f32, y: f32, fill_rule: FillRule) -> bool {
        let transform = self.state().transform;

        // The path cache saves a flattened and transformed version of the path.
        let path_cache = path.cache(&transform, self.tess_tol, self.dist_tol);

        // Early out if path is outside the canvas bounds
        if path_cache.bounds.maxx < 0.0
            || path_cache.bounds.minx > self.width() as f32
            || path_cache.bounds.maxy < 0.0
            || path_cache.bounds.miny > self.height() as f32
        {
            return false;
        }

        path_cache.contains_point(x, y, fill_rule)
    }

    /// Return the bounding box for a Path
    pub fn path_bbox(&self, path: &Path) -> Bounds {
        let transform = self.state().transform;

        // The path cache saves a flattened and transformed version of the path.
        let path_cache = path.cache(&transform, self.tess_tol, self.dist_tol);

        path_cache.bounds
    }

    /// Fills the provided Path with the specified Paint.
    pub fn fill_path(&mut self, path: &Path, paint: &Paint) {
        self.fill_path_internal(path, &paint.flavor, paint.shape_anti_alias, paint.fill_rule);
    }

    fn fill_path_internal(&mut self, path: &Path, paint_flavor: &PaintFlavor, anti_alias: bool, fill_rule: FillRule) {
        let mut paint_flavor = paint_flavor.clone();
        let transform = self.state().transform;

        // The path cache saves a flattened and transformed version of the path.
        let mut path_cache = path.cache(&transform, self.tess_tol, self.dist_tol);

        let canvas_width = self.width();
        let canvas_height = self.height();

        // Early out if path is outside the canvas bounds
        if path_cache.bounds.maxx < 0.0
            || path_cache.bounds.minx > canvas_width as f32
            || path_cache.bounds.maxy < 0.0
            || path_cache.bounds.miny > canvas_height as f32
        {
            return;
        }

        // Apply global alpha
        paint_flavor.mul_alpha(self.state().alpha);

        let scissor = self.state().scissor;

        // Calculate fill vertices.
        // expand_fill will fill path_cache.contours[].{stroke, fill} with vertex data for the GPU
        // fringe_with is the size of the strip of triangles generated at the path border used for AA
        let fringe_width = if anti_alias { self.fringe_width } else { 0.0 };
        path_cache.expand_fill(fringe_width, LineJoin::Miter, 2.4);

        // Detect if this path fill is in fact just an unclipped image copy

        if let (Some(path_rect), Some(scissor_rect), true) = (
            path_cache.path_fill_is_rect(),
            scissor.as_rect(canvas_width as f32, canvas_height as f32),
            paint_flavor.is_straight_tinted_image(anti_alias),
        ) {
            if scissor_rect.contains_rect(&path_rect) {
                self.render_unclipped_image_blit(&path_rect, &transform, &paint_flavor);
                return;
            } else if let Some(intersection) = path_rect.intersection(&scissor_rect) {
                self.render_unclipped_image_blit(&intersection, &transform, &paint_flavor);
                return;
            } else {
                return;
            }
        }

        // GPU uniforms
        let flavor = if path_cache.contours.len() == 1 && path_cache.contours[0].convexity == Convexity::Convex {
            let params = Params::new(
                &self.images,
                &transform,
                &paint_flavor,
                &Default::default(),
                &scissor,
                self.fringe_width,
                self.fringe_width,
                -1.0,
            );

            CommandType::ConvexFill { params }
        } else {
            let stencil_params = Params {
                stroke_thr: -1.0,
                shader_type: ShaderType::Stencil,
                ..Params::default()
            };

            let fill_params = Params::new(
                &self.images,
                &transform,
                &paint_flavor,
                &Default::default(),
                &scissor,
                self.fringe_width,
                self.fringe_width,
                -1.0,
            );

            CommandType::ConcaveFill {
                stencil_params,
                fill_params,
            }
        };

        // GPU command
        let mut cmd = Command::new(flavor);
        cmd.fill_rule = fill_rule;
        cmd.composite_operation = self.state().composite_operation;

        if let PaintFlavor::Image { id, .. } = paint_flavor {
            cmd.image = Some(id);
        } else if let Some(paint::GradientColors::MultiStop { stops }) = paint_flavor.gradient_colors() {
            cmd.image = self
                .gradients
                .lookup_or_add(stops, &mut self.images, &mut self.renderer)
                .ok();
        }

        // All verts from all shapes are kept in a single buffer here in the canvas.
        // Drawable struct is used to describe the range of vertices each draw call will operate on
        let mut offset = self.verts.len();

        cmd.drawables.reserve_exact(path_cache.contours.len());
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

        if let CommandType::ConcaveFill { .. } = cmd.cmd_type {
            // Concave shapes are first filled by writing to a stencil buffer and then drawing a quad
            // over the shape area with stencil test enabled to produce the final fill. These are
            // the verts needed for the covering quad
            self.verts.push(Vertex::new(
                path_cache.bounds.maxx + fringe_width,
                path_cache.bounds.maxy + fringe_width,
                0.5,
                1.0,
            ));
            self.verts.push(Vertex::new(
                path_cache.bounds.maxx + fringe_width,
                path_cache.bounds.miny - fringe_width,
                0.5,
                1.0,
            ));
            self.verts.push(Vertex::new(
                path_cache.bounds.minx - fringe_width,
                path_cache.bounds.maxy + fringe_width,
                0.5,
                1.0,
            ));
            self.verts.push(Vertex::new(
                path_cache.bounds.minx - fringe_width,
                path_cache.bounds.miny,
                0.5,
                1.0,
            ));

            cmd.triangles_verts = Some((offset, 4));
        }

        self.append_cmd(cmd);
    }

    /// Strokes the provided Path with the specified Paint.
    pub fn stroke_path(&mut self, path: &Path, paint: &Paint) {
        self.stroke_path_internal(path, &paint.flavor, paint.shape_anti_alias, &paint.stroke);
    }

    fn stroke_path_internal(
        &mut self,
        path: &Path,
        paint_flavor: &PaintFlavor,
        anti_alias: bool,
        stroke: &StrokeSettings,
    ) {
        let mut paint_flavor = paint_flavor.clone();
        let transform = self.state().transform;

        // The path cache saves a flattened and transformed version of the path.
        let mut path_cache = path.cache(&transform, self.tess_tol, self.dist_tol);

        // Early out if path is outside the canvas bounds
        if path_cache.bounds.maxx < 0.0
            || path_cache.bounds.minx > self.width() as f32
            || path_cache.bounds.maxy < 0.0
            || path_cache.bounds.miny > self.height() as f32
        {
            return;
        }

        let scissor = self.state().scissor;

        // Scale stroke width by current transform scale.
        // Note: I don't know why the original author clamped the max stroke width to 200, but it didn't
        // look correct when zooming in. There was probably a good reson for doing so and I may have
        // introduced a bug by removing the upper bound.
        //paint.set_stroke_width((paint.stroke_width() * transform.average_scale()).max(0.0).min(200.0));
        let mut line_width = (stroke.line_width * transform.average_scale()).max(0.0);

        if line_width < self.fringe_width {
            // If the stroke width is less than pixel size, use alpha to emulate coverage.
            // Since coverage is area, scale by alpha*alpha.
            let alpha = (line_width / self.fringe_width).max(0.0).min(1.0);

            paint_flavor.mul_alpha(alpha * alpha);
            line_width = self.fringe_width;
        }

        // Apply global alpha
        paint_flavor.mul_alpha(self.state().alpha);

        // Calculate stroke vertices.
        // expand_stroke will fill path_cache.contours[].stroke with vertex data for the GPU
        let fringe_with = if anti_alias { self.fringe_width } else { 0.0 };
        path_cache.expand_stroke(
            line_width * 0.5,
            fringe_with,
            stroke.line_cap_start,
            stroke.line_cap_end,
            stroke.line_join,
            stroke.miter_limit,
            self.tess_tol,
        );

        // GPU uniforms
        let params = Params::new(
            &self.images,
            &transform,
            &paint_flavor,
            &Default::default(),
            &scissor,
            line_width,
            self.fringe_width,
            -1.0,
        );

        let flavor = if stroke.stencil_strokes {
            let params2 = Params::new(
                &self.images,
                &transform,
                &paint_flavor,
                &Default::default(),
                &scissor,
                line_width,
                self.fringe_width,
                1.0 - 0.5 / 255.0,
            );

            CommandType::StencilStroke {
                params1: params,
                params2,
            }
        } else {
            CommandType::Stroke { params }
        };

        // GPU command
        let mut cmd = Command::new(flavor);
        cmd.composite_operation = self.state().composite_operation;

        if let PaintFlavor::Image { id, .. } = paint_flavor {
            cmd.image = Some(id);
        } else if let Some(paint::GradientColors::MultiStop { stops }) = paint_flavor.gradient_colors() {
            cmd.image = self
                .gradients
                .lookup_or_add(stops, &mut self.images, &mut self.renderer)
                .ok();
        }

        // All verts from all shapes are kept in a single buffer here in the canvas.
        // Drawable struct is used to describe the range of vertices each draw call will operate on
        let mut offset = self.verts.len();

        cmd.drawables.reserve_exact(path_cache.contours.len());
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

    fn render_unclipped_image_blit(&mut self, target_rect: &Rect, transform: &Transform2D, paint_flavor: &PaintFlavor) {
        let scissor = self.state().scissor;

        let mut params = Params::new(
            &self.images,
            transform,
            paint_flavor,
            &Default::default(),
            &scissor,
            0.,
            0.,
            -1.0,
        );
        params.shader_type = ShaderType::TextureCopyUnclipped;

        let mut cmd = Command::new(CommandType::Triangles { params });
        cmd.composite_operation = self.state().composite_operation;

        let x0 = target_rect.x;
        let y0 = target_rect.y;
        let x1 = x0 + target_rect.w;
        let y1 = y0 + target_rect.h;

        let (p0, p1) = (x0, y0);
        let (p2, p3) = (x1, y0);
        let (p4, p5) = (x1, y1);
        let (p6, p7) = (x0, y1);

        // Apply the same mapping from vertex coordinates to texture coordinates as in the fragment shader,
        // but now ahead of time.
        let mut to_texture_space_transform = Transform2D::identity();
        to_texture_space_transform.scale(1. / params.extent[0], 1. / params.extent[1]);
        to_texture_space_transform.premultiply(&Transform2D([
            params.paint_mat[0],
            params.paint_mat[1],
            params.paint_mat[4],
            params.paint_mat[5],
            params.paint_mat[8],
            params.paint_mat[9],
        ]));

        let (s0, t0) = to_texture_space_transform.transform_point(target_rect.x, target_rect.y);
        let (s1, t1) =
            to_texture_space_transform.transform_point(target_rect.x + target_rect.w, target_rect.y + target_rect.h);

        let verts = [
            Vertex::new(p0, p1, s0, t0),
            Vertex::new(p4, p5, s1, t1),
            Vertex::new(p2, p3, s1, t0),
            Vertex::new(p0, p1, s0, t0),
            Vertex::new(p6, p7, s0, t1),
            Vertex::new(p4, p5, s1, t1),
        ];

        if let &PaintFlavor::Image { id, .. } = paint_flavor {
            cmd.image = Some(id);
        }

        cmd.triangles_verts = Some((self.verts.len(), verts.len()));
        self.append_cmd(cmd);

        self.verts.extend_from_slice(&verts);
    }

    // Text

    /// Adds a font file to the canvas
    pub fn add_font<P: AsRef<FilePath>>(&mut self, file_path: P) -> Result<FontId, ErrorKind> {
        self.text_context.borrow_mut().add_font_file(file_path)
    }

    /// Adds a font to the canvas by reading it from the specified chunk of memory.
    pub fn add_font_mem(&mut self, data: &[u8]) -> Result<FontId, ErrorKind> {
        self.text_context.borrow_mut().add_font_mem(data)
    }

    /// Adds all .ttf files from a directory
    pub fn add_font_dir<P: AsRef<FilePath>>(&mut self, dir_path: P) -> Result<Vec<FontId>, ErrorKind> {
        self.text_context.borrow_mut().add_font_dir(dir_path)
    }

    /// Returns information on how the provided text will be drawn with the specified paint.
    pub fn measure_text<S: AsRef<str>>(
        &self,
        x: f32,
        y: f32,
        text: S,
        paint: &Paint,
    ) -> Result<TextMetrics, ErrorKind> {
        let scale = self.font_scale() * self.device_px_ratio;

        let mut text_settings = paint.text.clone();
        text_settings.font_size *= scale;
        text_settings.letter_spacing *= scale;

        let scale = self.font_scale() * self.device_px_ratio;
        let invscale = 1.0 / scale;

        self.text_context
            .borrow_mut()
            .measure_text(x * scale, y * scale, text, &text_settings)
            .map(|mut metrics| {
                metrics.scale(invscale);
                metrics
            })
    }

    /// Returns font metrics for a particular Paint.
    pub fn measure_font(&self, paint: &Paint) -> Result<FontMetrics, ErrorKind> {
        let scale = self.font_scale() * self.device_px_ratio;

        self.text_context
            .borrow_mut()
            .measure_font(paint.text.font_size * scale, &paint.text.font_ids)
    }

    /// Returns the maximum index-th byte of text that will fit inside max_width.
    ///
    /// The retuned index will always lie at the start and/or end of a UTF-8 code point sequence or at the start or end of the text
    pub fn break_text<S: AsRef<str>>(&self, max_width: f32, text: S, paint: &Paint) -> Result<usize, ErrorKind> {
        let scale = self.font_scale() * self.device_px_ratio;

        let mut text_settings = paint.text.clone();
        text_settings.font_size *= scale;
        text_settings.letter_spacing *= scale;

        let max_width = max_width * scale;

        self.text_context
            .borrow_mut()
            .break_text(max_width, text, &text_settings)
    }

    /// Returnes a list of ranges representing each line of text that will fit inside max_width
    pub fn break_text_vec<S: AsRef<str>>(
        &self,
        max_width: f32,
        text: S,
        paint: &Paint,
    ) -> Result<Vec<Range<usize>>, ErrorKind> {
        let scale = self.font_scale() * self.device_px_ratio;

        let mut text_settings = paint.text.clone();
        text_settings.font_size *= scale;
        text_settings.letter_spacing *= scale;

        let max_width = max_width * scale;

        self.text_context
            .borrow_mut()
            .break_text_vec(max_width, text, &text_settings)
    }

    /// Fills the provided string with the specified Paint.
    pub fn fill_text<S: AsRef<str>>(
        &mut self,
        x: f32,
        y: f32,
        text: S,
        paint: &Paint,
    ) -> Result<TextMetrics, ErrorKind> {
        self.draw_text(x, y, text.as_ref(), paint, RenderMode::Fill)
    }

    /// Strokes the provided string with the specified Paint.
    pub fn stroke_text<S: AsRef<str>>(
        &mut self,
        x: f32,
        y: f32,
        text: S,
        paint: &Paint,
    ) -> Result<TextMetrics, ErrorKind> {
        self.draw_text(x, y, text.as_ref(), paint, RenderMode::Stroke)
    }

    /// Dispatch an explicit set of GlyphDrawCommands to the renderer. Use this only if you are
    /// using a custom font rasterizer/layout.
    pub fn draw_glyph_commands(&mut self, draw_commands: GlyphDrawCommands, paint: &Paint, scale: f32) {
        let transform = self.state().transform;
        let invscale = 1.0 / scale;
        let create_vertices = |quads: &Vec<text::Quad>| {
            let mut verts = Vec::with_capacity(quads.len() * 6);

            for quad in quads {
                let (p0, p1) = transform.transform_point(quad.x0 * invscale, quad.y0 * invscale);
                let (p2, p3) = transform.transform_point(quad.x1 * invscale, quad.y0 * invscale);
                let (p4, p5) = transform.transform_point(quad.x1 * invscale, quad.y1 * invscale);
                let (p6, p7) = transform.transform_point(quad.x0 * invscale, quad.y1 * invscale);

                verts.push(Vertex::new(p0, p1, quad.s0, quad.t0));
                verts.push(Vertex::new(p4, p5, quad.s1, quad.t1));
                verts.push(Vertex::new(p2, p3, quad.s1, quad.t0));
                verts.push(Vertex::new(p0, p1, quad.s0, quad.t0));
                verts.push(Vertex::new(p6, p7, quad.s0, quad.t1));
                verts.push(Vertex::new(p4, p5, quad.s1, quad.t1));
            }
            verts
        };

        // Apply global alpha
        let mut paint_flavor = paint.flavor.clone();
        paint_flavor.mul_alpha(self.state().alpha);

        for cmd in draw_commands.alpha_glyphs {
            let verts = create_vertices(&cmd.quads);

            self.render_triangles(&verts, &transform, &paint_flavor, GlyphTexture::AlphaMask(cmd.image_id));
        }

        for cmd in draw_commands.color_glyphs {
            let verts = create_vertices(&cmd.quads);

            self.render_triangles(
                &verts,
                &transform,
                &paint_flavor,
                GlyphTexture::ColorTexture(cmd.image_id),
            );
        }
    }

    // Private

    fn draw_text(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        paint: &Paint,
        render_mode: RenderMode,
    ) -> Result<TextMetrics, ErrorKind> {
        let scale = self.font_scale() * self.device_px_ratio;
        let invscale = 1.0 / scale;

        let mut stroke = paint.stroke.clone();
        stroke.line_width *= scale;

        let mut text_settings = paint.text.clone();
        text_settings.font_size *= scale;
        text_settings.letter_spacing *= scale;

        let mut layout = text::shape(
            x * scale,
            y * scale,
            &mut self.text_context.borrow_mut(),
            &text_settings,
            text,
            None,
        )?;
        //let layout = self.layout_text(x, y, text, &paint)?;

        // TODO: Early out if text is outside the canvas bounds, or maybe even check for each character in layout.

        let bitmap_glyphs = layout.has_bitmap_glyphs();
        let need_direct_rendering = text_settings.font_size > 92.0;

        if need_direct_rendering && !bitmap_glyphs {
            text::render_direct(
                self,
                &layout,
                &paint.flavor,
                paint.shape_anti_alias,
                &stroke,
                text_settings.font_size,
                render_mode,
                invscale,
            )?;
        } else {
            let atlas = if bitmap_glyphs && need_direct_rendering {
                self.ephemeral_glyph_atlas.get_or_insert_with(Default::default).clone()
            } else {
                self.glyph_atlas.clone()
            };

            let draw_commands =
                atlas.render_atlas(self, &layout, text_settings.font_size, stroke.line_width, render_mode)?;
            self.draw_glyph_commands(draw_commands, paint, scale);
        }

        layout.scale(invscale);

        Ok(layout)
    }

    fn render_triangles(
        &mut self,
        verts: &[Vertex],
        transform: &Transform2D,
        paint_flavor: &PaintFlavor,
        glyph_texture: GlyphTexture,
    ) {
        let scissor = self.state().scissor;

        let params = Params::new(
            &self.images,
            transform,
            paint_flavor,
            &glyph_texture,
            &scissor,
            1.0,
            1.0,
            -1.0,
        );

        let mut cmd = Command::new(CommandType::Triangles { params });
        cmd.composite_operation = self.state().composite_operation;
        cmd.glyph_texture = glyph_texture;

        if let &PaintFlavor::Image { id, .. } = paint_flavor {
            cmd.image = Some(id);
        } else if let Some(paint::GradientColors::MultiStop { stops }) = paint_flavor.gradient_colors() {
            cmd.image = self
                .gradients
                .lookup_or_add(stops, &mut self.images, &mut self.renderer)
                .ok();
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

    #[cfg(feature = "debug_inspector")]
    pub fn debug_inspector_get_font_textures(&self) -> Vec<ImageId> {
        self.glyph_atlas
            .glyph_textures
            .borrow()
            .iter()
            .map(|t| t.image_id)
            .collect()
    }

    #[cfg(feature = "debug_inspector")]
    pub fn debug_inspector_draw_image(&mut self, id: ImageId) {
        if let Ok(size) = self.image_size(id) {
            let width = size.0 as f32;
            let height = size.1 as f32;
            let mut path = Path::new();
            path.rect(0f32, 0f32, width, height);
            self.fill_path(&path, &Paint::image(id, 0f32, 0f32, width, height, 0f32, 1f32));
        }
    }
}

impl<T: Renderer> Drop for Canvas<T> {
    fn drop(&mut self) {
        self.images.clear(&mut self.renderer);
    }
}

// re-exports
#[cfg(feature = "image-loading")]
pub use ::image as img;

pub use imgref;
pub use rgb;

#[cfg(test)]
#[derive(Default)]
pub struct RecordingRenderer {
    pub last_commands: Rc<RefCell<Vec<renderer::Command>>>,
}

#[cfg(test)]
impl Renderer for RecordingRenderer {
    type Image = DummyImage;
    type NativeTexture = ();

    fn set_size(&mut self, _width: u32, _height: u32, _dpi: f32) {}

    fn render(
        &mut self,
        _images: &mut ImageStore<Self::Image>,
        _verts: &[renderer::Vertex],
        commands: Vec<renderer::Command>,
    ) {
        *self.last_commands.borrow_mut() = commands;
    }

    fn alloc_image(&mut self, info: crate::ImageInfo) -> Result<Self::Image, ErrorKind> {
        Ok(Self::Image { info })
    }

    fn create_image_from_native_texture(
        &mut self,
        _native_texture: Self::NativeTexture,
        _info: crate::ImageInfo,
    ) -> Result<Self::Image, ErrorKind> {
        Err(ErrorKind::UnsupportedImageFormat)
    }

    fn update_image(
        &mut self,
        image: &mut Self::Image,
        data: crate::ImageSource,
        x: usize,
        y: usize,
    ) -> Result<(), ErrorKind> {
        let size = data.dimensions();

        if x + size.width > image.info.width() {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        if y + size.height > image.info.height() {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        Ok(())
    }

    fn delete_image(&mut self, _image: Self::Image, _image_id: crate::ImageId) {}

    fn screenshot(&mut self) -> Result<imgref::ImgVec<rgb::RGBA8>, ErrorKind> {
        Ok(imgref::ImgVec::new(Vec::new(), 0, 0))
    }
}

#[cfg(test)]
pub struct DummyImage {
    info: ImageInfo,
}

#[test]
fn test_image_blit_fast_path() {
    use renderer::{Command, CommandType};

    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.);
    let mut path = Path::new();
    path.rect(10., 10., 50., 50.);
    let image = canvas
        .create_image_empty(30, 30, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let paint = Paint::image(image, 0., 0., 30., 30., 0., 0.).with_anti_alias(false);
    canvas.fill_path(&path, &paint);
    canvas.flush();

    let commands = recorded_commands.borrow();
    let mut commands = commands.iter();
    assert!(matches!(
        commands.next(),
        Some(Command {
            cmd_type: CommandType::SetRenderTarget(..),
            ..
        })
    ));
    assert!(matches!(
        commands.next(),
        Some(Command {
            cmd_type: CommandType::Triangles {
                params: Params {
                    shader_type: renderer::ShaderType::TextureCopyUnclipped,
                    ..
                }
            },
            ..
        })
    ));
}
