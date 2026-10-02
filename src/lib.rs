#![deny(missing_docs)]
#![warn(missing_debug_implementations)]
#![cfg_attr(docsrs, feature(doc_cfg))]

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
    - Tests
*/

#[cfg(feature = "serde")]
#[macro_use]
extern crate serde;

#[cfg(feature = "textlayout")]
use std::ops::Range;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::Path as FilePath,
    rc::Rc,
};

use imgref::ImgVec;
use rgb::RGBA8;

mod text;

mod error;
pub use error::ErrorKind;

pub use text::{
    Align, Atlas, Baseline, DrawCommand, FontId, FontMetrics, GlyphDrawCommands, Quad, RenderMode, VariationAxisInfo,
};

pub use text::TextContext;
#[cfg(feature = "textlayout")]
pub use text::TextMetrics;

use text::{GlyphAtlas, TextContextImpl};

mod image;
use crate::image::ImageStore;

mod budgets;
mod clip;
mod filters;
mod layers;
mod shadow;
mod transient;
pub use crate::image::{
    BlendMode, ImageFilter, ImageFlags, ImageId, ImageInfo, ImageSource, PixelFormat, TurbulenceKind,
};
use crate::transient::TransientPool;
use budgets::*;
use clip::*;
use filters::*;
use layers::*;
pub use layers::{LayerEffects, MaskKind};

mod turbulence;

mod color;
pub use color::Color;

pub mod renderer;
pub use renderer::{RenderTarget, Renderer};

use renderer::{Command, CommandType, Drawable, Params, ShaderType, SurfacelessRenderer, Vertex};

pub(crate) mod geometry;
pub use geometry::Transform2D;
use geometry::*;

mod paint;
pub use paint::Paint;
pub use paint::TextDecoration;
use paint::{GlyphTexture, PaintFlavor, StrokeSettings};
use renderer::BlendPass;

mod path;
use path::Convexity;
pub use path::{Path, PathIter, Solidity, Verb};

mod gradient_store;
use gradient_store::GradientStore;

/// Determines the fill rule used when filling paths.
///
/// The fill rule defines how the interior of a shape is determined.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FillRule {
    /// The interior is determined using the even-odd rule.
    /// A point is considered inside the shape if it intersects the shape's outline an odd number of times.
    EvenOdd,
    /// The interior is determined using the non-zero winding rule (default).
    /// A point is considered inside the shape if it intersects the shape's outline a non-zero number of times,
    /// considering the direction of each intersection.
    #[default]
    NonZero,
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
    /// Creates a new `CompositeOperationState` from the provided `CompositeOperation`
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

    /// Creates a new `CompositeOperationState` with source and destination blend factors.
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
    radius: f32,
}

impl Scissor {
    /// Returns the bounding rect if the scissor clip if it's an untransformed rectangular clip
    fn as_rect(&self, canvas_width: f32, canvas_height: f32) -> Option<Rect> {
        let Some(extent) = self.extent else {
            return Some(Rect::new(0., 0., canvas_width, canvas_height));
        };

        // Abort if the clip has rounded corners: only the fragment shader's
        // scissor mask applies the corner radius, and fast paths that treat the
        // scissor as this plain rect bypass that mask. Returning None routes
        // those draws through the normal path, which clips them correctly.
        if self.radius > 0.0 {
            return None;
        }

        let Transform2D([a, b, c, d, x, y]) = self.transform;

        // Abort if we're skewing (usually doesn't happen)
        if b != 0.0 || c != 0.0 {
            return None;
        }

        // Abort if we're scaling
        if a != 1.0 || d != 1.0 {
            return None;
        }

        let half_width = extent[0];
        let half_height = extent[1];
        Some(Rect::new(
            x - half_width,
            y - half_height,
            half_width * 2.0,
            half_height * 2.0,
        ))
    }

    /// The scissor's device-space rect for sizing a layer's store: like
    /// [`as_rect`](Self::as_rect) but tolerating an axis-aligned scale, which
    /// every canvas drawn under a device-pixel ratio or a zoom carries. A
    /// rotated, skewed or rounded scissor still has no rect (`None`).
    fn device_bounds(&self, canvas_width: f32, canvas_height: f32) -> Option<Rect> {
        let Some(extent) = self.extent else {
            return Some(Rect::new(0., 0., canvas_width, canvas_height));
        };
        if self.radius > 0.0 {
            return None;
        }
        let Transform2D([a, b, c, d, x, y]) = self.transform;
        if b != 0.0 || c != 0.0 {
            return None;
        }
        let half_width = extent[0] * a.abs();
        let half_height = extent[1] * d.abs();
        Some(Rect::new(
            x - half_width,
            y - half_height,
            half_width * 2.0,
            half_height * 2.0,
        ))
    }
}

/// Determines the shape used to draw the end points of lines.
///
/// The default value is `Butt`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LineCap {
    /// The ends of lines are squared off at the endpoints.
    #[default]
    Butt,
    /// The ends of lines are rounded.
    Round,
    /// The ends of lines are squared off by adding a box with an equal
    /// width and half the height of the line's thickness.
    Square,
}

/// Determines the shape used to join two line segments where they meet.
///
/// The default value is `Miter`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LineJoin {
    /// Connected segments are joined by extending their outside edges to
    /// connect at a single point, with the effect of filling an additional
    /// lozenge-shaped area. This setting is affected by the miterLimit property.
    #[default]
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

#[derive(Copy, Clone, Debug)]
struct State {
    composite_operation: CompositeOperationState,
    transform: Transform2D,
    scissor: Scissor,
    alpha: f32,
    // How many clip_path() entries this state level owns; restore() pops the
    // clip stack back to the saved depth and marks affected planes for replay.
    clip_depth: usize,
    // Canvas 2D drop-shadow attributes. Defaults match the HTML spec: a fully
    // transparent shadow color (which disables shadows entirely), zero blur and
    // zero offset. See `Canvas::set_shadow_color` and friends.
    shadow_color: Color,
    shadow_blur: f32,
    shadow_offset: [f32; 2],
}

impl Default for State {
    fn default() -> Self {
        Self {
            composite_operation: CompositeOperationState::default(),
            transform: Transform2D::identity(),
            scissor: Scissor::default(),
            alpha: 1.0,
            // rgba(0, 0, 0, 0): the spec default. A transparent shadow color
            // means no shadow is painted, so the default path adds zero work.
            shadow_color: Color::rgbaf(0.0, 0.0, 0.0, 0.0),
            shadow_blur: 0.0,
            shadow_offset: [0.0, 0.0],
            clip_depth: 0,
        }
    }
}

/// Main 2D drawing context.
#[derive(Debug)]
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
    // Saves (false) and layers (true) taken past MAX_STATE_DEPTH, innermost
    // last, so their restores and end_layers still pair; while any exist the
    // canvas is saturated: nothing draws, and the changes made to the state
    // are discarded when the last one pops.
    overflow: Vec<bool>,
    // The deepest real state as it was when saturation began, written back
    // when the last entry past the limit pops.
    overflow_state: State,
    warned_overflow: bool,
    commands: Vec<Command>,
    verts: Vec<Vertex>,
    images: ImageStore<T::Image>,
    pending_image_deletions: HashSet<ImageId>,
    fringe_width: f32,
    device_px_ratio: f32,
    tess_tol: f32,
    dist_tol: f32,
    gradients: GradientStore,
    // Layer backing stores, filter scratches and shadow coverage, reused
    // within the frame and deleted at the flush; see `transient.rs`.
    transients: TransientPool,
    filter_work: u64,
    filter_work_budget: u64,
    // Open layers from begin_layer(), innermost last.
    layers: Vec<LayerRecord>,
    // Effect passes in progress (a backdrop placed, a mask normalised): their
    // draws are not suppressed by the depth limit or a pass-through layer.
    offscreen_passes: usize,
    // Turbulence lattice textures by seed, most recently used last. Bounded by
    // `turbulence::LATTICE_CACHE_CAPACITY`; an evicted one is deleted after
    // the next flush so a command already recorded against it still runs.
    turbulence_lattices: Vec<(i32, ImageId)>,
    // The active clip_path() stack, innermost last. Each entry lives on the
    // stencil plane of the render target it was taken on and gates only
    // draws into that target.
    clip_stack: Vec<ClipEntry>,
    clip_planes: HashMap<RenderTarget, ClipPlaneState>,
}

/// Returns the enabled text-decoration lines as `(offset, thickness)` pairs,
/// where `offset` is the line center relative to the run baseline in +y-down
/// user space. This is the single source of decoration geometry: the painter
/// builds its rects from it and the shadow pass sizes its coverage box with it,
/// so the two can never disagree.
///
/// OpenType position values measure from the baseline with +y pointing up,
/// while canvas y grows downward, so a line's center is the negated position.
/// The overline has no dedicated metric; it sits at the ascent with the
/// underline's thickness, nudged up by half that thickness so it clears the
/// glyphs. Thicknesses are clamped to one user-space unit so lines stay
/// visible for tiny fonts.
#[cfg(feature = "textlayout")]
fn decoration_lines(decoration: TextDecoration, metrics: &FontMetrics) -> impl Iterator<Item = (f32, f32)> {
    let underline_thickness = metrics.underline_thickness().max(1.0);
    [
        decoration
            .underline
            .then(|| (-metrics.underline_position(), underline_thickness)),
        decoration
            .strikethrough
            .then(|| (-metrics.strikeout_position(), metrics.strikeout_thickness().max(1.0))),
        decoration
            .overline
            .then(|| (-metrics.ascender() - underline_thickness / 2.0, underline_thickness)),
    ]
    .into_iter()
    .flatten()
}

impl<T> Canvas<T>
where
    T: Renderer,
{
    /// Creates a new canvas.
    pub fn new(renderer: T) -> Result<Self, ErrorKind> {
        let text_context = Rc::new(RefCell::new(TextContextImpl::default()));
        let glyph_atlas = Rc::new(GlyphAtlas::new(&text_context));
        let mut canvas = Self {
            width: 0,
            height: 0,
            renderer,
            text_context,
            glyph_atlas,
            ephemeral_glyph_atlas: None,
            current_render_target: RenderTarget::Screen,
            state_stack: Vec::new(),
            overflow: Vec::new(),
            overflow_state: State::default(),
            warned_overflow: false,
            commands: Vec::new(),
            verts: Vec::new(),
            images: ImageStore::new(),
            pending_image_deletions: HashSet::new(),
            fringe_width: 1.0,
            device_px_ratio: 1.0,
            tess_tol: 0.25,
            dist_tol: 0.01,
            gradients: GradientStore::new(),
            transients: TransientPool::new(transient::DEFAULT_BUDGET),
            filter_work: 0,
            filter_work_budget: DEFAULT_FILTER_WORK_BUDGET,
            layers: Vec::new(),
            offscreen_passes: 0,
            turbulence_lattices: Vec::new(),
            clip_stack: Vec::new(),
            clip_planes: HashMap::new(),
        };

        canvas.save();

        Ok(canvas)
    }

    /// Creates a new canvas with the specified renderer and using the fonts registered with the
    /// provided [`TextContext`]. Note that the context is explicitly shared, so that any fonts
    /// registered with a clone of this context will also be visible to this canvas.
    pub fn new_with_text_context(renderer: T, text_context: TextContext) -> Result<Self, ErrorKind> {
        let glyph_atlas = Rc::new(GlyphAtlas::new(&text_context.0));
        let mut canvas = Self {
            width: 0,
            height: 0,
            renderer,
            text_context: text_context.0,
            glyph_atlas,
            ephemeral_glyph_atlas: None,
            current_render_target: RenderTarget::Screen,
            state_stack: Vec::new(),
            overflow: Vec::new(),
            overflow_state: State::default(),
            warned_overflow: false,
            commands: Vec::new(),
            verts: Vec::new(),
            images: ImageStore::new(),
            pending_image_deletions: HashSet::new(),
            fringe_width: 1.0,
            device_px_ratio: 1.0,
            tess_tol: 0.25,
            dist_tol: 0.01,
            gradients: GradientStore::new(),
            transients: TransientPool::new(transient::DEFAULT_BUDGET),
            filter_work: 0,
            filter_work_budget: DEFAULT_FILTER_WORK_BUDGET,
            layers: Vec::new(),
            offscreen_passes: 0,
            turbulence_lattices: Vec::new(),
            clip_stack: Vec::new(),
            clip_planes: HashMap::new(),
        };

        canvas.save();

        Ok(canvas)
    }

    /// Sets the size of the default framebuffer (screen size)
    pub fn set_size(&mut self, width: u32, height: u32, dpi: f32) {
        let resized = width != self.width || height != self.height || dpi != self.device_px_ratio;
        self.width = width;
        self.height = height;
        self.fringe_width = 1.0 / dpi;
        self.tess_tol = 0.25 / dpi;
        self.dist_tol = 0.01 / dpi;
        self.device_px_ratio = dpi;

        self.renderer.set_size(width, height, dpi);

        if resized {
            // A resize invalidates the device-space bounds of every open
            // layer: discard them, as a Canvas 2D reset discards pending layers
            // (WPT 2d.layer.reset). Their draws so far are dropped and their
            // images return to the pool.
            self.discard_open_layers();
        }
        // The renderer starts the stream on the screen; the tracked target
        // follows, or a later set_render_target(Image) is skipped as a no-op.
        self.append_cmd(Command::new(CommandType::SetRenderTarget(RenderTarget::Screen)));
        self.current_render_target = RenderTarget::Screen;

        if resized {
            if let Some(plane) = self.clip_planes.get_mut(&RenderTarget::Screen) {
                plane.dirty = true;
            }
        }
        if let Some(image) = self.layers.last().and_then(|layer| layer.image) {
            // Same size at a frame boundary: the open layer keeps capturing
            // (WPT 2d.layer.flush-on-frame-presentation).
            self.set_render_target(RenderTarget::Image(image));
        }
    }

    /// The size of the current render target in device pixels.
    fn render_target_size(&self) -> (f32, f32) {
        match self.current_render_target {
            RenderTarget::Image(id) => match self.images.info(id) {
                Some(info) => (info.width() as f32, info.height() as f32),
                None => (self.width as f32, self.height as f32),
            },
            RenderTarget::Screen => (self.width as f32, self.height as f32),
        }
    }

    /// Clears the rectangle area defined by left upper corner (x,y), width and height with the provided color.
    ///
    /// This is a raw clear of device pixels: the transform, the scissor and
    /// any [`clip_path`](Self::clip_path) do not apply. A Canvas 2D
    /// `clearRect`, which the transform and clip do affect, is a fill of the
    /// rect with an opaque paint under
    /// [`CompositeOperation::DestinationOut`]. The stencil under the rect is
    /// cleared with the color so nothing a fill left behind reaches the next
    /// frame; a clip armed on the target survives it (only the winding bits
    /// are cleared then, at the cost of a full-target quad on a tiler).
    pub fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        self.reconcile_current_clip_plane();
        // A clip armed on this target must survive the clear; without one
        // the whole stencil can go, which is a plain tile clear on a tiler
        // where a masked stencil clear is a full-target quad.
        let keep_clip = self.clip_active();
        let mut cmd = Command::new(CommandType::ClearRect { color, keep_clip });
        cmd.composite_operation = self.state().composite_operation;

        let x0 = x as f32;
        let y0 = y as f32;
        let x1 = x0 + width as f32;
        let y1 = y0 + height as f32;

        let (p0, p1) = (x0, y0);
        let (p2, p3) = (x1, y0);
        let (p4, p5) = (x1, y1);
        let (p6, p7) = (x0, y1);

        let verts = [
            Vertex::new(p0, p1, 0.0, 0.0),
            Vertex::new(p4, p5, 0.0, 0.0),
            Vertex::new(p2, p3, 0.0, 0.0),
            Vertex::new(p0, p1, 0.0, 0.0),
            Vertex::new(p6, p7, 0.0, 0.0),
            Vertex::new(p4, p5, 0.0, 0.0),
        ];

        cmd.triangles_verts = Some((self.verts.len(), verts.len()));
        self.append_cmd(cmd);

        self.verts.extend_from_slice(&verts);
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
    pub fn flush_to_output(&mut self, output: impl Into<T::RenderOutput>) -> T::CommandBuffer {
        let command_buffer = self.renderer.render(
            output,
            &mut self.images,
            &self.verts,
            std::mem::take(&mut self.commands),
        );
        self.verts.clear();
        self.release_pending_images();
        self.gradients
            .release_old_gradients(&mut self.images, &mut self.renderer);
        self.release_transient_images();
        self.filter_work = self.layers.iter().map(|layer| layer.reserved_filter_work).sum();
        if let Some(atlas) = self.ephemeral_glyph_atlas.take() {
            atlas.clear(self);
        }
        self.reissue_render_target_after_flush();
        command_buffer
    }

    /// Returns a screenshot of the current canvas.
    pub fn screenshot(&mut self) -> Result<ImgVec<RGBA8>, ErrorKind> {
        self.renderer.screenshot()
    }

    // State Handling

    /// Pushes and saves the current render state into a state stack.
    ///
    /// A matching [`restore`](Self::restore) pops it. Layers share this
    /// stack: [`begin_layer`](Self::begin_layer) pushes an entry of its own
    /// that [`end_layer`](Self::end_layer), or a `restore()` reaching it,
    /// pops. The stack is bounded at 16,384 levels; a save past that is
    /// still paired with its restore, but nothing draws at that level and
    /// the changes made to the state there are discarded when it unwinds.
    pub fn save(&mut self) {
        if self.push_past_depth_limit(false) {
            return;
        }
        let state = self.state_stack.last().map_or_else(State::default, |state| *state);

        self.state_stack.push(state);
    }

    /// Restores the previous render state.
    ///
    /// A layer opened by [`begin_layer`](Self::begin_layer) is an entry on
    /// the same stack, so a `restore()` with the layer's own entry on top
    /// closes the layer exactly as [`end_layer`](Self::end_layer) would
    /// (Skia's `saveLayer()` / `restore()` pairing). An unmatched restore at
    /// the base state is ignored.
    pub fn restore(&mut self) {
        if self.pop_past_depth_limit().is_some() {
            // A save or layer past the limit: nothing was drawn.
            return;
        }
        if self
            .layers
            .last()
            .is_some_and(|layer| layer.state_depth == self.state_stack.len())
        {
            self.end_layer();
            return;
        }
        if self.state_stack.len() == 1 {
            return;
        }
        self.pop_state();
    }

    /// Pops the state on top and the clips it owned.
    fn pop_state(&mut self) {
        self.state_stack.pop();
        let depth = self.state().clip_depth;
        self.pop_clips_to(depth);
    }

    /// Resets current state to default values. Does not affect the state stack.
    ///
    /// Clips added at this state level are dropped with the rest of the
    /// level's state; clips established by outer levels stay in force, as
    /// they would after a `restore()`.
    pub fn reset(&mut self) {
        // A reset discards pending layers along with the state (WPT
        // 2d.layer.reset); drawing continues where the outermost one began.
        // Entries past the depth limit go with them.
        self.overflow.clear();
        self.discard_open_layers();
        // The clip stack is shared across state levels: this level owns the
        // entries past the depth it inherited from its parent (none at the
        // base level). Resetting the recorded depth without dropping those
        // entries would leave the stencil plane clipping draws the state
        // says are unclipped.
        let inherited_depth = self
            .state_stack
            .len()
            .checked_sub(2)
            .map_or(0, |parent| self.state_stack[parent].clip_depth);
        *self.state_mut() = State {
            clip_depth: inherited_depth,
            ..State::default()
        };
        self.pop_clips_to(inherited_depth);
    }

    /// Saves the current state before calling the callback and restores it afterwards
    ///
    /// This is less error prone than remembering to match `save()` -> `restore()` calls
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
        if let RenderTarget::Image(id) = target {
            if self.pending_image_deletions.contains(&id) || self.images.info(id).is_none() {
                return;
            }
        }
        if self.current_render_target != target {
            self.append_cmd(Command::new(CommandType::SetRenderTarget(target)));
            self.current_render_target = target;
        }
    }

    fn append_cmd(&mut self, cmd: Command) {
        if self.commands_suppressed()
            && !matches!(
                &cmd.cmd_type,
                CommandType::SetRenderTarget(_) | CommandType::RenderFilteredImage { .. }
            )
        {
            return;
        }
        let mut cmd = cmd;
        // Stencil bookkeeping commands and target switches carry no fragments
        // to gate; clear_rect is a raw clear that neither backend clips; a
        // filter pass draws into its own target image, not the clipped one.
        if !matches!(
            cmd.cmd_type,
            CommandType::ClipFill
                | CommandType::ClipReset { .. }
                | CommandType::SetRenderTarget(_)
                | CommandType::ClearRect { .. }
                | CommandType::RenderFilteredImage { .. }
        ) {
            cmd.clip_active = self.clip_active();
        }
        self.commands.push(cmd);
    }

    fn commands_suppressed(&self) -> bool {
        self.offscreen_passes == 0
            && (self.saturated() || self.layers.iter().any(|layer| layer.discard && layer.image.is_none()))
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
    /// Use this function to import native textures into the rendering of a scene
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

    /// Allocates an image that wraps the given backend-specific texture.
    /// Use this function to import native textures marked as external into the
    /// rendering of a scene with femtovg.
    ///
    /// It is necessary to call `[Self::delete_image`] to free femtovg specific
    /// book-keeping data structures, the underlying backend-specific texture memory
    /// will not be freed. It is the caller's responsible to delete it.
    pub fn create_image_from_external_texture(
        &mut self,
        texture: T::ExternalTexture,
        info: ImageInfo,
    ) -> Result<ImageId, ErrorKind> {
        self.images.register_external_texture(&mut self.renderer, texture, info)
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

    /// Retrieves a reference to the image with the specified ID.
    pub fn get_image(&self, id: ImageId) -> Option<&T::Image> {
        if self.pending_image_deletions.contains(&id) {
            return None;
        }
        self.images.get(id)
    }

    /// Retrieves a mutable reference to the image with the specified ID.
    pub fn get_image_mut(&mut self, id: ImageId) -> Option<&mut T::Image> {
        if self.pending_image_deletions.contains(&id) {
            return None;
        }
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
        if self.pending_image_deletions.contains(&id) || self.transients.owns(id) {
            return Err(ErrorKind::ImageIdNotFound);
        }
        let info = ImageInfo::new(flags, width, height, format);
        self.images.realloc(&mut self.renderer, id, info)?;
        if let Some(plane) = self.clip_planes.get_mut(&RenderTarget::Image(id)) {
            plane.dirty = true;
        }
        Ok(())
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
        if self.pending_image_deletions.contains(&id) || self.transients.owns(id) {
            return Err(ErrorKind::ImageIdNotFound);
        }
        self.images.update(&mut self.renderer, id, src.into(), x, y)
    }

    /// Deletes an image at the next flush, after earlier commands are encoded.
    /// An open layer borrowing it as a mask or a blend backdrop keeps it
    /// through that layer's composite and the following flush. A layer's own
    /// store is the canvas's, not the caller's: it is left alone here and by
    /// [`realloc_image`](Self::realloc_image) and
    /// [`update_image`](Self::update_image).
    pub fn delete_image(&mut self, id: ImageId) {
        self.defer_image_deletion(id);
    }

    /// Returns image info
    pub fn image_info(&self, id: ImageId) -> Result<ImageInfo, ErrorKind> {
        if self.pending_image_deletions.contains(&id) {
            return Err(ErrorKind::ImageIdNotFound);
        }
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
        let t = Transform2D::translation(x, y);
        self.state_mut().transform.premultiply(&t);
    }

    /// Rotates the current coordinate system. Angle is specified in radians.
    pub fn rotate(&mut self, angle: f32) {
        let t = Transform2D::rotation(angle);
        self.state_mut().transform.premultiply(&t);
    }

    /// Scales the current coordinate system.
    pub fn scale(&mut self, x: f32, y: f32) {
        let t = Transform2D::scaling(x, y);
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

    /// Returns the current transformation matrix
    pub fn transform(&self) -> Transform2D {
        self.state().transform
    }

    // Scissoring

    /// Sets the current scissor rectangle.
    ///
    /// The scissor rectangle is transformed by the current transform.
    pub fn scissor(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.rounded_scissor(x, y, w, h, 0.0);
    }

    /// Sets the current rounded scissor rectangle.
    ///
    /// The scissor rectangle is transformed by the current transform.
    pub fn rounded_scissor(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let state = self.state_mut();

        let w = w.max(0.0);
        let h = h.max(0.0);

        let mut transform = Transform2D::translation(x + w * 0.5, y + h * 0.5);
        transform *= state.transform;
        state.scissor.transform = transform;

        state.scissor.extent = Some([w * 0.5, h * 0.5]);
        state.scissor.radius = r.max(0.0).min(w * 0.5).min(h * 0.5);
    }

    /// Intersects current scissor rectangle with the specified rectangle.
    ///
    /// The scissor rectangle is transformed by the current transform.
    /// Note: in case the rotation of previous scissor rect differs from
    /// the current one, the intersection will be done between the specified
    /// rectangle and the previous scissor rectangle transformed in the current
    /// transform space. The resulting shape is always rectangle.
    pub fn intersect_scissor(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.intersect_rounded_scissor(x, y, w, h, 0.0);
    }

    /// Intersects current scissor rectangle with the specified rounded rectangle.
    ///
    /// The resulting rounded corners are exact when this is the first active
    /// scissor or when the previous clip is a containing rectangle with the same
    /// transform. Other intersections fall back to rectangular scissoring.
    pub fn intersect_rounded_scissor(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let tolerance = self.dist_tol;
        let state = self.state_mut();

        // If no previous scissor has been set, set the scissor as current scissor.
        if state.scissor.extent.is_none() {
            self.rounded_scissor(x, y, w, h, r);
            return;
        }

        let extent = state.scissor.extent.unwrap();

        // Transform the current scissor rect into current transform space.
        // If there is difference in rotation, this will be approximation.

        let Transform2D([a, b, c, d, tx, ty]) = state.scissor.transform / state.transform;

        let ex = extent[0];
        let ey = extent[1];

        let tex = ex * a.abs() + ey * c.abs();
        let tey = ex * b.abs() + ey * d.abs();

        let rect = Rect::new(tx - tex, ty - tey, tex * 2.0, tey * 2.0);
        let res = rect.intersect(Rect::new(x, y, w, h));

        let requested = Rect::new(x, y, w, h);
        let requested_contains_existing = requested.x <= rect.x + tolerance
            && requested.y <= rect.y + tolerance
            && rect.x + rect.w <= requested.x + requested.w + tolerance
            && rect.y + rect.h <= requested.y + requested.h + tolerance;
        if r <= 0.0 && state.scissor.radius > 0.0 && requested_contains_existing {
            return;
        }

        if r <= 0.0 && state.scissor.radius > 0.0 {
            let radius = state.scissor.radius;
            let contains_point = |x: f32, y: f32| {
                let left = rect.x + radius;
                let right = rect.x + rect.w - radius;
                let top = rect.y + radius;
                let bottom = rect.y + rect.h - radius;
                let dx = if x < left {
                    left - x
                } else if x > right {
                    x - right
                } else {
                    0.0
                };
                let dy = if y < top {
                    top - y
                } else if y > bottom {
                    y - bottom
                } else {
                    0.0
                };
                dx * dx + dy * dy <= (radius + tolerance) * (radius + tolerance)
            };
            let rounded_contains_requested = contains_point(requested.x, requested.y)
                && contains_point(requested.x + requested.w, requested.y)
                && contains_point(requested.x, requested.y + requested.h)
                && contains_point(requested.x + requested.w, requested.y + requested.h);
            if rounded_contains_requested {
                self.scissor(res.x, res.y, res.w, res.h);
            } else {
                self.rounded_scissor(res.x, res.y, res.w, res.h, radius);
            }
            return;
        }

        let contains_requested = rect.x <= requested.x + tolerance
            && rect.y <= requested.y + tolerance
            && requested.x + requested.w <= rect.x + rect.w + tolerance
            && requested.y + requested.h <= rect.y + rect.h + tolerance;
        if contains_requested {
            self.rounded_scissor(requested.x, requested.y, requested.w, requested.h, r);
        } else {
            self.scissor(res.x, res.y, res.w, res.h);
        }
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
        self.reconcile_current_clip_plane();
        let mut paint_flavor = paint_flavor.clone();
        let transform = self.state().transform;

        let canvas_width = self.width();
        let canvas_height = self.height();

        // Draw the drop shadow (if any) under the fill. The closure re-enters
        // fill_path_internal with the *real* paint so render_shadow can build the
        // shadow from the source's true per-pixel alpha; render_shadow temporarily
        // disables shadows in the state so this does not recurse. This runs in its
        // own scope so the path cache's RefMut borrow is released before the path
        // is cloned (cloning a Path while its cache is borrowed would panic).
        if self.shadow_enabled() {
            let bounds = {
                let cache = path.cache(&transform, self.tess_tol, self.dist_tol);
                cache.bounds
            };
            // Only skip when even the offset+blurred shadow cannot reach the
            // target; an off-screen shape may still cast an on-screen shadow.
            if self.shadow_could_be_visible(bounds) {
                let path = path.clone();
                let shadow_flavor = paint_flavor.clone();
                self.render_shadow(bounds, move |canvas| {
                    canvas.fill_path_internal(&path, &shadow_flavor, anti_alias, fill_rule);
                });
            }
        }

        // The path cache saves a flattened and transformed version of the path.
        let mut path_cache = path.cache(&transform, self.tess_tol, self.dist_tol);

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
        path_cache.expand_fill(fringe_width, LineJoin::Miter, 2.4, fill_rule);

        // Detect if this path fill is in fact just an unclipped image copy

        if let (Some(path_rect), Some(scissor_rect), true, true) = (
            path_cache.path_fill_is_rect(),
            scissor.as_rect(canvas_width as f32, canvas_height as f32),
            paint_flavor.is_straight_tinted_image(anti_alias),
            // The unclipped blit bypasses the stencil clip plane (the #292
            // rounded-scissor precedent): route clipped blits through the
            // normal masked path.
            !self.clip_active(),
        ) {
            if scissor_rect.contains_rect(&path_rect) {
                self.render_unclipped_image_blit(&path_rect, &transform, &paint_flavor);
            } else if let Some(intersection) = path_rect.intersection(&scissor_rect) {
                self.render_unclipped_image_blit(&intersection, &transform, &paint_flavor);
            }

            return;
        }

        // GPU uniforms
        let flavor = if path_cache.contours.len() == 1 && path_cache.contours[0].convexity == Convexity::Convex {
            let params = Params::new(
                &self.images,
                &transform,
                &paint_flavor,
                &GlyphTexture::default(),
                &scissor,
                self.fringe_width,
                self.fringe_width,
                -1.0,
            );

            CommandType::ConvexFill { params }
        } else {
            let stencil_params = Params::stencil();

            let fill_params = Params::new(
                &self.images,
                &transform,
                &paint_flavor,
                &GlyphTexture::default(),
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
        self.reconcile_current_clip_plane();
        let mut paint_flavor = paint_flavor.clone();
        let transform = self.state().transform;

        if !stroke.line_dash.is_empty() {
            let dashed_path = path.dashed_with_tolerance(&stroke.line_dash, stroke.line_dash_offset, self.tess_tol);
            if dashed_path.is_empty() {
                return;
            }

            let mut solid_stroke = stroke.clone();
            solid_stroke.line_dash.clear();
            solid_stroke.line_dash_offset = 0.0;
            self.stroke_path_internal(&dashed_path, &paint_flavor, anti_alias, &solid_stroke);
            return;
        }

        // Draw the drop shadow (if any) under the stroke. The path-cache bounds
        // only cover the centerline, so expand them by the device-space stroke
        // half-width before handing them to render_shadow. This runs in its own
        // scope so the cache's RefMut borrow is released before the path is cloned
        // (cloning a Path while its cache is borrowed would panic). render_shadow
        // disables shadows in the state, so re-entering stroke does not recurse.
        if self.shadow_enabled() {
            let centerline = {
                let cache = path.cache(&transform, self.tess_tol, self.dist_tol);
                cache.bounds
            };
            let half = (stroke.line_width * transform.average_scale()).max(self.fringe_width) * 0.5;
            let mut bounds = centerline;
            bounds.minx -= half;
            bounds.miny -= half;
            bounds.maxx += half;
            bounds.maxy += half;
            // Skip only when even the offset+blurred shadow cannot reach the
            // render target. The offset and blur spread can pull a shadow back
            // on-screen for a shape whose own bounds are off-screen, so we must
            // not cull on the shape's bounds alone.
            if self.shadow_could_be_visible(bounds) {
                let path = path.clone();
                let stroke = stroke.clone();
                let shadow_flavor = paint_flavor.clone();
                self.render_shadow(bounds, move |canvas| {
                    canvas.stroke_path_internal(&path, &shadow_flavor, anti_alias, &stroke);
                });
            }
        }

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
            // A stroke thinner than the fringe is drawn at fringe width with its
            // alpha scaled by the ratio, so it puts down the ink its area calls
            // for: a fringe-wide stroke integrates to one pixel per unit length,
            // so a w-pixel line carries w. That is the linear coverage Skia's
            // hairline path applies (SkDrawTreatAAStrokeAsHairline scales the
            // paint alpha by the device width); nanovg squared the ratio, which
            // left a 0.5 px line at a quarter of its coverage.
            let alpha = (line_width / self.fringe_width).clamp(0.0, 1.0);

            paint_flavor.mul_alpha(alpha);
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
            &GlyphTexture::default(),
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
                &GlyphTexture::default(),
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

    /// After a flush the renderer starts the next command stream on the
    /// screen target; if drawing is currently redirected (an open layer, or a
    /// caller-selected image target), the redirect must be re-issued so the
    /// next frame's commands keep landing where the canvas state says.
    fn reissue_render_target_after_flush(&mut self) {
        if self.current_render_target != RenderTarget::Screen {
            self.append_cmd(Command::new(CommandType::SetRenderTarget(self.current_render_target)));
        }
    }

    fn render_unclipped_image_blit(&mut self, target_rect: &Rect, transform: &Transform2D, paint_flavor: &PaintFlavor) {
        self.reconcile_current_clip_plane();
        let scissor = self.state().scissor;

        let mut params = Params::new(
            &self.images,
            transform,
            paint_flavor,
            &GlyphTexture::default(),
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
        let mut to_texture_space_transform = Transform2D::scaling(1. / params.extent[0], 1. / params.extent[1]);
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
    #[cfg(feature = "textlayout")]
    pub fn add_font<P: AsRef<FilePath>>(&mut self, file_path: P) -> Result<FontId, ErrorKind> {
        self.text_context.borrow_mut().add_font_file(file_path)
    }

    /// Adds a font to the canvas by reading it from the specified chunk of memory.
    #[cfg(feature = "textlayout")]
    pub fn add_font_mem(&mut self, data: &[u8]) -> Result<FontId, ErrorKind> {
        self.text_context.borrow_mut().add_font_mem(data)
    }

    /// Adds all .ttf files from a directory
    #[cfg(feature = "textlayout")]
    pub fn add_font_dir<P: AsRef<FilePath>>(&mut self, dir_path: P) -> Result<Vec<FontId>, ErrorKind> {
        self.text_context.borrow_mut().add_font_dir(dir_path)
    }

    /// Returns the variation axes available for the specified font.
    ///
    /// For variable fonts, this returns information about each axis (e.g. weight, width).
    /// For static fonts, this returns an empty vector.
    ///
    /// Axes are returned in the order they appear in the font's OpenType
    /// `fvar` table. This is the same order that [`Canvas::fill_glyph_run`]
    /// and [`Canvas::stroke_glyph_run`] expect for their normalized
    /// coordinate slices: the i-th coordinate corresponds to the i-th axis.
    pub fn font_variation_axes(&self, font_id: FontId) -> Result<Vec<VariationAxisInfo>, ErrorKind> {
        let ctx = self.text_context.borrow();
        let font = ctx.font(font_id).ok_or(ErrorKind::NoFontFound)?;
        Ok(font.variation_axes())
    }

    /// Returns information on how the provided text will be drawn with the specified paint.
    #[cfg(feature = "textlayout")]
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

    /// Returns font metrics for a particular Paint, in user-space units.
    ///
    /// The values scale with the paint's font size only — the canvas
    /// transform and DPI factor do not affect them, matching the space
    /// [`measure_text`](Self::measure_text) reports and
    /// [`fill_text`](Self::fill_text) consumes.
    #[cfg(feature = "textlayout")]
    pub fn measure_font(&self, paint: &Paint) -> Result<FontMetrics, ErrorKind> {
        // User-space units, independent of the canvas transform and DPI
        // factor: the same space `measure_text()` reports, `fill_text()`
        // consumes and `TextContext::measure_font()` already returns. The
        // result was previously multiplied by the canvas's internal
        // (quantized) glyph-rasterization scale, so metrics read from a
        // zoomed canvas came back inflated — sub/superscript runs sized from
        // `subscript_size()` grew with the zoom instead of staying anchored
        // to the run's font size.
        self.text_context.borrow_mut().measure_font(
            paint.text.font_size,
            &paint.text.font_ids,
            &paint.text.font_variations,
        )
    }

    /// Returns the maximum index-th byte of text that will fit inside `max_width`.
    ///
    /// The retuned index will always lie at the start and/or end of a UTF-8 code point sequence or at the start or end of the text
    #[cfg(feature = "textlayout")]
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

    /// Returnes a list of ranges representing each line of text that will fit inside `max_width`
    #[cfg(feature = "textlayout")]
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
    #[cfg(feature = "textlayout")]
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
    #[cfg(feature = "textlayout")]
    pub fn stroke_text<S: AsRef<str>>(
        &mut self,
        x: f32,
        y: f32,
        text: S,
        paint: &Paint,
    ) -> Result<TextMetrics, ErrorKind> {
        self.draw_text(x, y, text.as_ref(), paint, RenderMode::Stroke)
    }

    /// Fills the provided glyphs with the specified Paint.
    ///
    /// `normalized_coords` specifies variation axis positions for variable
    /// fonts as `i16` values in F2DOT14 format (the OpenType normalized
    /// coordinate representation, range \[-1.0, 1.0\] mapped to
    /// \[-16384, 16384\]), one per axis in `fvar` order. Pass an empty slice
    /// for the font's default instance. These coordinates are typically
    /// obtained from a text shaper (e.g. rustybuzz, harfbuzz, parley).
    /// See [`Canvas::font_variation_axes`] to query the available axes.
    pub fn fill_glyph_run(
        &mut self,
        font_id: FontId,
        normalized_coords: &[i16],
        glyphs: impl IntoIterator<Item = PositionedGlyph>,
        paint: &Paint,
    ) -> Result<(), ErrorKind> {
        self.draw_glyph_run(glyphs, paint, font_id, normalized_coords, RenderMode::Fill)
    }

    /// Strokes the provided glyphs with the specified Paint.
    ///
    /// `normalized_coords` specifies variation axis positions for variable
    /// fonts as `i16` values in F2DOT14 format (the OpenType normalized
    /// coordinate representation, range \[-1.0, 1.0\] mapped to
    /// \[-16384, 16384\]), one per axis in `fvar` order. Pass an empty slice
    /// for the font's default instance. These coordinates are typically
    /// obtained from a text shaper (e.g. rustybuzz, harfbuzz, parley).
    /// See [`Canvas::font_variation_axes`] to query the available axes.
    pub fn stroke_glyph_run(
        &mut self,
        font_id: FontId,
        normalized_coords: &[i16],
        glyphs: impl IntoIterator<Item = PositionedGlyph>,
        paint: &Paint,
    ) -> Result<(), ErrorKind> {
        self.draw_glyph_run(glyphs, paint, font_id, normalized_coords, RenderMode::Stroke)
    }

    /// Dispatch an explicit set of `GlyphDrawCommands` to the renderer. Use this only if you are
    /// using a custom font rasterizer/layout.
    pub fn draw_glyph_commands(&mut self, draw_commands: GlyphDrawCommands, paint: &Paint) {
        let transform = self.state().transform;
        let create_vertices = |quads: &Vec<text::Quad>| {
            let mut verts = Vec::with_capacity(quads.len() * 6);

            for quad in quads {
                let left = quad.x0;
                let right = quad.x1;
                let top = quad.y0;
                let bottom = quad.y1;

                let (p0, p1) = transform.transform_point(left, top);
                let (p2, p3) = transform.transform_point(right, top);
                let (p4, p5) = transform.transform_point(right, bottom);
                let (p6, p7) = transform.transform_point(left, bottom);

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

    #[cfg(feature = "textlayout")]
    fn draw_text(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        paint: &Paint,
        render_mode: RenderMode,
    ) -> Result<TextMetrics, ErrorKind> {
        use itertools::Itertools;

        let scale = self.font_scale() * self.device_px_ratio;
        let invscale = 1.0 / scale;

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

        let normalized_coords = {
            let text_context = self.text_context.borrow();
            text::normalize_variations(&text_context, &paint.text.font_ids, &paint.text.font_variations)
        };

        // Whether anything will actually be painted; decorations and shadows
        // hang off the run only when it has at least one drawable glyph.
        let has_drawable_glyphs = layout.glyphs.iter().any(|shaped_glyph| !shaped_glyph.c.is_control());

        // Font metrics for the run in user-space units, matching the user-space
        // baseline. Fetched once (with the same variations the run was shaped
        // with) and shared by the shadow-extent computation and the decoration
        // painter below.
        let run_font_metrics = {
            let text_context = self.text_context.borrow();
            text_context
                .measure_font(paint.text.font_size, &paint.text.font_ids, &paint.text.font_variations)
                .ok()
        };

        // Draw the drop shadow (if any) under the text. The shaped layout gives a
        // user-space box; transform its corners by the CTM to obtain device-space
        // bounds and let render_shadow re-enter draw_text with the shadow tint.
        // The re-entered draw_text draws glyphs *and* decoration lines (with
        // shadows suppressed), so a single shadow covers the whole painting
        // operation, matching the drawing model — decorations are not shadowed
        // separately. (render_shadow disables shadows in the state so this does
        // not recurse.)
        if self.shadow_enabled() {
            // Layout metrics are in the scaled shaping space; bring them back to
            // user space. The horizontal extent unions the glyph ink boxes with
            // the run's advance box (layout.x .. layout.x + width): negative letter
            // spacing can collapse the advance width while ink is still painted,
            // yet the decoration lines are drawn across that advance box, so the
            // shadow must cover both.
            let (mut gx0, mut gx1) = (f32::INFINITY, f32::NEG_INFINITY);
            for glyph in &layout.glyphs {
                gx0 = gx0.min(glyph.x);
                gx1 = gx1.max(glyph.x + glyph.width);
            }
            let (ux0, uy0, ux1, uy1) = if gx0 <= gx1 {
                let rx0 = layout.x.min(layout.x + layout.width());
                let rx1 = layout.x.max(layout.x + layout.width());
                let x0 = (gx0 * invscale).min(rx0 * invscale);
                let x1 = (gx1 * invscale).max(rx1 * invscale);
                // The vertical extent starts from the font's line box (baseline -
                // ascent .. baseline - descent), not the glyph ink box: an ink-box
                // extent would clip the shadows of ascenders, descenders and
                // diacritics on short (x-height-only) runs like "www". Any enabled
                // decoration lines are unioned in through the same geometry the
                // painter uses, so a line outside the line box (the nudged
                // overline, a font with an unusual underline position) grows the
                // box with it.
                let baseline = layout.baseline() * invscale;
                let (ascent, descent) = run_font_metrics
                    .as_ref()
                    .map_or((0.0, 0.0), |m| (m.ascender(), m.descender()));
                let (mut top, mut bottom) = (baseline - ascent, baseline - descent);
                if let Some(metrics) = &run_font_metrics {
                    for (offset, thickness) in decoration_lines(paint.text.text_decoration, metrics) {
                        top = top.min(baseline + offset - thickness / 2.0);
                        bottom = bottom.max(baseline + offset + thickness / 2.0);
                    }
                }
                // A little slack for antialiased edges and blur reach.
                let margin = paint.text.font_size * 0.2;
                (x0 - margin, top - margin, x1 + margin, bottom + margin)
            } else {
                // No drawable glyphs: nothing painted, nothing to shadow.
                (0.0, 0.0, 0.0, 0.0)
            };

            let transform = self.state().transform;
            let mut device = Bounds::default();
            for (cx, cy) in [(ux0, uy0), (ux1, uy0), (ux1, uy1), (ux0, uy1)] {
                let (dx, dy) = transform.transform_point(cx, cy);
                device.minx = device.minx.min(dx);
                device.miny = device.miny.min(dy);
                device.maxx = device.maxx.max(dx);
                device.maxy = device.maxy.max(dy);
            }

            // Skip only when the offset+blurred shadow cannot reach the target;
            // text just off-screen may still cast an on-screen shadow.
            if self.shadow_could_be_visible(device) {
                let text = text.to_owned();
                // Draw the text with its *real* paint so the shadow is built from
                // the glyphs' true coverage/alpha; render_shadow recolors it by the
                // shadow color while preserving that alpha.
                let shadow_paint = paint.clone();
                self.render_shadow(device, move |canvas| {
                    let _ = canvas.draw_text(x, y, &text, &shadow_paint, render_mode);
                });
            }
        }

        // The run-level shadow above is the only shadow this text should cast.
        // Glyph runs that fall back to outline rendering are drawn through
        // fill/stroke_path_internal, whose own shadow hooks would otherwise add a
        // second shadow per glyph on top of it — so suppress shadows while the
        // actual glyphs are drawn, restoring afterwards (also on error).
        let saved_shadow_color = self.state().shadow_color;
        self.state_mut().shadow_color = Color::rgbaf(0.0, 0.0, 0.0, 0.0);

        let mut glyph_run_result = Ok(());
        for (font_id, glyph_run) in &layout
            .glyphs
            .iter()
            .filter(|shaped_glyph| !shaped_glyph.c.is_control())
            .chunk_by(|g| g.font_id)
        {
            glyph_run_result = self.draw_glyph_run(
                glyph_run.map(|shaped_glyph| PositionedGlyph {
                    x: shaped_glyph.x * invscale,
                    y: shaped_glyph.y * invscale,
                    glyph_id: shaped_glyph.glyph_id,
                }),
                paint,
                font_id,
                &normalized_coords,
                render_mode,
            );
            if glyph_run_result.is_err() {
                break;
            }
        }

        layout.scale(invscale);

        // Text decorations are an SVG/CSS extension (Canvas 2D has none). They are
        // emitted as plain filled rectangles in user space — the same coordinate
        // space as the `* invscale` glyph positions handed to `draw_glyph_run` —
        // so `fill_path` runs them through the identical canvas transform the
        // glyph runs use. That keeps the lines aligned with the glyphs across the
        // direct-outline, atlas, and scale-baked-atlas paths alike.
        //
        // They are drawn while shadows are still suppressed: the run-level shadow
        // pass above already rendered the decorations into its coverage (it
        // re-enters draw_text, which draws them), so glyphs and decorations share
        // a single shadow and the lines must not cast a second one.
        if glyph_run_result.is_ok() && !paint.text.text_decoration.is_none() && has_drawable_glyphs {
            if let Some(metrics) = &run_font_metrics {
                self.draw_text_decorations(paint, metrics, layout.baseline(), layout.x, layout.width());
            }
        }

        // Restore the shadow state on the success and error paths alike.
        self.state_mut().shadow_color = saved_shadow_color;
        glyph_run_result?;

        Ok(layout)
    }

    /// Emits the enabled text-decoration lines for a run as filled rectangles in
    /// user space. `baseline` is the run baseline (user space, +y down), `x` the
    /// run's left edge, and `width` its advance width. `metrics` is the run's
    /// font metrics in the same user-space units as `baseline`.
    #[cfg(feature = "textlayout")]
    fn draw_text_decorations(&mut self, paint: &Paint, metrics: &FontMetrics, baseline: f32, x: f32, width: f32) {
        // NOTE: this assumes a horizontal writing mode. The lines run along the
        // advance direction (x) and are offset perpendicular to it (y), spanning
        // `[x, x + width]` at a baseline-relative y. That is correct for both LTR
        // and RTL runs, since a horizontal decoration is direction-independent.
        // A vertical writing mode (top-to-bottom) would need the lines to run
        // along y and offset along x, driven by vertical metrics; the geometry
        // below would have to be generalized to the advance axis rather than
        // hardcoding x as the run axis.
        //
        // All enabled lines share one path (a single verbs/coords allocation) and
        // one fill_path call, so a run costs one draw no matter how many lines
        // are on. The rects are disjoint horizontal bands, and the paint below
        // forces NonZero filling, so any degenerate overlap (e.g. thickness
        // clamping on tiny fonts) still paints solid.
        let mut path = Path::new();
        for (offset, thickness) in decoration_lines(paint.text.text_decoration, metrics) {
            path.rect(x, baseline + offset - thickness / 2.0, width, thickness);
        }

        if path.is_empty() {
            return;
        }

        // The decoration takes the text paint's color, matching SVG where the
        // decoration uses the text fill. The full paint flavor (gradient/image)
        // is reused as-is so a gradient-filled run gets a gradient-filled line.
        let line_paint = paint.clone().with_fill_rule(FillRule::NonZero);
        self.fill_path(&path, &line_paint);
    }

    fn draw_glyph_run(
        &mut self,
        glyphs: impl IntoIterator<Item = PositionedGlyph>,
        paint: &Paint,
        font_id: FontId,
        normalized_coords: &[i16],
        render_mode: RenderMode,
    ) -> Result<(), ErrorKind> {
        // TODO: Early out if text is outside the canvas bounds, or maybe even check for each character in layout.

        let text_context = self.text_context.clone();
        let mut text_context = text_context.borrow_mut();

        // How this glyph run is rasterized for the current canvas transform.
        #[derive(Clone, Copy)]
        enum Rasterization {
            Path,
            Atlas,
            ScaledAtlas {
                scale: f32,
                true_scale: f32,
                translation: (f32, f32),
            },
        }

        // Classify the canvas transform. 1e-3 epsilon: tight enough to catch any
        // intentional transform, loose enough to tolerate matrix-op drift.
        let rasterization = match self.state().transform.as_uniform_scale_translation(1e-3) {
            // Rotation / skew / non-uniform / negative scale: outline rendering.
            None => Rasterization::Path,
            Some((true_scale, tx, ty)) => {
                // Quantize the baked scale so small animation steps don't churn the
                // atlas; 1/16 steps (≈6%) are imperceptible at typical zoom levels.
                let scale = geometry::quantize(true_scale, 1.0 / 16.0).max(1.0 / 16.0);
                if paint.text.font_size * scale > 92.0 {
                    // Cached bitmap would be too large.
                    Rasterization::Path
                } else if scale == 1.0 {
                    // Pure translation (within a quantization step): nothing to bake.
                    Rasterization::Atlas
                } else if matches!(paint.flavor, PaintFlavor::Color(_)) {
                    Rasterization::ScaledAtlas {
                        scale,
                        true_scale,
                        translation: (tx, ty),
                    }
                } else {
                    // Gradients/images map their coordinates through the canvas
                    // transform that the atlas path swaps out for a translation,
                    // which would shift them; keep those direct.
                    Rasterization::Path
                }
            }
        };

        let need_direct_rendering = matches!(rasterization, Rasterization::Path);
        let effective_scale = match rasterization {
            Rasterization::ScaledAtlas { scale, .. } => scale,
            _ => 1.0,
        };
        let effective_font_size = paint.text.font_size * effective_scale;

        // Everything measured in user text space crosses into the rasterizer's
        // space through `effective_scale`, exactly once, right here: font size,
        // glyph positions (below), and the stroke width. Bake-space consumers
        // (the atlas) receive the scaled values; the live-transform consumer
        // (`render_direct`) has `effective_scale == 1` and receives them
        // untouched, letting the canvas transform apply the scale instead.
        // Splitting these -- scaling some quantities and not others, or scaling
        // one of them twice -- is what made stroke widths change with the zoom
        // regime rather than the zoom.
        let mut stroke = paint.stroke.clone();
        stroke.line_width *= effective_scale;

        let Some(font) = text_context.font_mut(font_id) else {
            return Err(ErrorKind::NoFontFound);
        };

        let font_face = font.face_ref_with_normalized_coords(normalized_coords);

        // TODO: create on demand

        let mut color_glyphs = Vec::new();

        let glyphs_it = glyphs.into_iter();
        let non_color_glyphs = glyphs_it
            .filter(|glyph| {
                if font
                    .glyph(&font_face, glyph.glyph_id, normalized_coords)
                    .is_some_and(|glyph| glyph.path.is_none())
                {
                    color_glyphs.push(glyph.clone());

                    false
                } else {
                    true
                }
            })
            .collect::<Vec<_>>();

        // When baking scale into the rasterization, pre-multiply glyph positions
        // by `effective_scale` so that under a translation-only canvas transform
        // they still land at the original screen position.
        let scaled = |g: &PositionedGlyph| PositionedGlyph {
            x: g.x * effective_scale,
            y: g.y * effective_scale,
            glyph_id: g.glyph_id,
        };

        let mut draw_commands = if need_direct_rendering {
            text::render_direct(
                self,
                font,
                non_color_glyphs.into_iter(),
                &paint.flavor,
                paint.shape_anti_alias,
                &stroke,
                paint.text.font_size,
                render_mode,
                normalized_coords,
            )?;
            GlyphDrawCommands::default()
        } else {
            self.glyph_atlas.clone().render_atlas(
                self,
                font_id,
                font,
                &font_face,
                non_color_glyphs.iter().map(scaled),
                effective_font_size,
                stroke.line_width,
                render_mode,
                normalized_coords,
            )?
        };

        if !color_glyphs.is_empty() {
            let color_commands = {
                let atlas = if need_direct_rendering {
                    self.ephemeral_glyph_atlas
                        .get_or_insert_with(|| Rc::new(GlyphAtlas::new(&self.text_context)))
                        .clone()
                } else {
                    self.glyph_atlas.clone()
                };

                // Color glyphs on the atlas path follow the same scale baking.
                // On the direct path we leave them at the original font_size —
                // that already matches today's behavior.
                if need_direct_rendering {
                    atlas.render_atlas(
                        self,
                        font_id,
                        font,
                        &font_face,
                        color_glyphs.into_iter(),
                        paint.text.font_size,
                        stroke.line_width,
                        render_mode,
                        normalized_coords,
                    )?
                } else {
                    atlas.render_atlas(
                        self,
                        font_id,
                        font,
                        &font_face,
                        color_glyphs.iter().map(scaled),
                        effective_font_size,
                        stroke.line_width,
                        render_mode,
                        normalized_coords,
                    )?
                }
            };

            draw_commands.alpha_glyphs.extend(color_commands.alpha_glyphs);
            draw_commands.color_glyphs.extend(color_commands.color_glyphs);
        }

        // For the scaled-atlas path, present the pre-scaled glyph quads with a
        // translation-only transform so the bitmap shows at its on-screen pixel
        // size. render_atlas already emitted quads in the scaled glyph space, so
        // only draw_glyph_commands (which applies the canvas transform) needs the
        // swap — and since it is infallible, the transform is always restored even
        // though the fallible rendering above used `?`.
        match rasterization {
            Rasterization::ScaledAtlas {
                scale,
                true_scale,
                translation: (tx, ty),
            } => {
                // Rasterize at the quantized scale (cache-stable) but position with
                // the TRUE scale: present the pre-scaled quads under the residual
                // scale true/quantized (within one 1/16 step of 1.0) plus the true
                // translation. This keeps glyphs locked to the same on-screen point
                // as vector geometry under any zoom, instead of snapping by the
                // quantization error times the glyph's distance from the origin.
                let residual = true_scale / scale;
                let saved = self.state().transform;
                self.state_mut().transform = Transform2D::new(residual, 0.0, 0.0, residual, tx, ty);
                self.draw_glyph_commands(draw_commands, paint);
                self.state_mut().transform = saved;
            }
            _ => self.draw_glyph_commands(draw_commands, paint),
        }

        Ok(())
    }

    fn render_triangles(
        &mut self,
        verts: &[Vertex],
        transform: &Transform2D,
        paint_flavor: &PaintFlavor,
        glyph_texture: GlyphTexture,
    ) {
        self.reconcile_current_clip_plane();
        let scissor = self.state().scissor;

        let params = Params::new(
            &self.images,
            transform,
            paint_flavor,
            &glyph_texture,
            &scissor,
            1.0,
            self.fringe_width,
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

    /// Get a list of all font textures.
    #[cfg(feature = "debug_inspector")]
    pub fn debug_inspector_get_font_textures(&self) -> Vec<ImageId> {
        self.glyph_atlas
            .glyph_textures
            .borrow()
            .iter()
            .map(|t| t.image_id)
            .collect()
    }

    /// Draws an image with the specified `id` on the whole canvas.
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

impl<T> Canvas<T>
where
    T: SurfacelessRenderer,
{
    /// Tells the renderer to execute all drawing commands and clears the current internal state
    ///
    /// Call this at the end of each frame.
    pub fn flush(&mut self) {
        self.renderer
            .render_surfaceless(&mut self.images, &self.verts, std::mem::take(&mut self.commands));
        self.verts.clear();
        self.release_pending_images();
        self.gradients
            .release_old_gradients(&mut self.images, &mut self.renderer);
        self.release_transient_images();
        self.filter_work = self.layers.iter().map(|layer| layer.reserved_filter_work).sum();
        if let Some(atlas) = self.ephemeral_glyph_atlas.take() {
            atlas.clear(self);
        }
        self.reissue_render_target_after_flush();
    }
}

impl<T: Renderer> Drop for Canvas<T> {
    fn drop(&mut self) {
        self.images.clear(&mut self.renderer);
    }
}

/// This struct holds the parameter needs to draw a single glyph using the low-level `fill_glyphs`
/// and `stroke_glyphs` API.
#[derive(Clone, Debug)]
pub struct PositionedGlyph {
    /// The glyph will be drawn at the specified x position.
    pub x: f32,
    /// The glyph will be drawn at the specified x position.
    pub y: f32,
    /// The TrueType glyph id to use when rendering the glyph. This is specific
    /// to the font registered under the `font_id` field.
    pub glyph_id: u16,
}

// re-exports
#[cfg(feature = "image-loading")]
pub use ::image as img;

pub use imgref;
pub use rgb;

/// Internal structure that implements the Renderer trait for unit testing.
#[cfg(test)]
#[derive(Default, Debug)]
pub struct RecordingRenderer {
    /// Vector of the last commands submitted to the renderer.
    pub last_commands: Rc<RefCell<Vec<renderer::Command>>>,
    /// Vertex buffer submitted with the last render call.
    pub last_verts: Rc<RefCell<Vec<renderer::Vertex>>>,
    /// Texture limit to report; 0 means the trait default.
    pub max_texture_size: usize,
    /// Makes image allocation fail for resource-pressure tests.
    pub fail_image_allocations: bool,
    /// Number of image allocation attempts.
    pub image_allocation_attempts: usize,
    /// Number of backend images released.
    pub image_deletion_count: usize,
}

#[cfg(test)]
impl Renderer for RecordingRenderer {
    type Image = DummyImage;
    type NativeTexture = ();
    type ExternalTexture = ();
    type RenderOutput = ();
    type CommandBuffer = ();

    fn set_size(&mut self, _width: u32, _height: u32, _dpi: f32) {}

    fn render(
        &mut self,
        _output: impl Into<Self::RenderOutput>,
        _images: &mut ImageStore<Self::Image>,
        verts: &[renderer::Vertex],
        commands: Vec<renderer::Command>,
    ) {
        *self.last_commands.borrow_mut() = commands;
        *self.last_verts.borrow_mut() = verts.to_vec();
    }

    fn alloc_image(&mut self, info: crate::ImageInfo) -> Result<Self::Image, ErrorKind> {
        self.image_allocation_attempts += 1;
        if self.fail_image_allocations {
            return Err(ErrorKind::UnknownError);
        }
        Ok(Self::Image { info })
    }

    fn create_image_from_native_texture(
        &mut self,
        _native_texture: Self::NativeTexture,
        _info: crate::ImageInfo,
    ) -> Result<Self::Image, ErrorKind> {
        Err(ErrorKind::UnsupportedImageFormat)
    }

    fn create_image_from_external_texture(
        &mut self,
        _external_texture: Self::ExternalTexture,
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
        data.check_update(&image.info, x, y)
    }

    fn delete_image(&mut self, _image: Self::Image, _image_id: crate::ImageId) {
        self.image_deletion_count += 1;
    }

    fn screenshot(&mut self) -> Result<imgref::ImgVec<rgb::RGBA8>, ErrorKind> {
        Ok(imgref::ImgVec::new(Vec::new(), 0, 0))
    }

    fn max_texture_size(&self) -> usize {
        if self.max_texture_size == 0 {
            8192
        } else {
            self.max_texture_size
        }
    }
}

/// Dummy image type used for tests.
#[cfg(test)]
#[derive(Debug)]
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
    canvas.flush_to_output(());

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

#[cfg(test)]
fn first_draw_params(commands: &[renderer::Command]) -> &Params {
    use renderer::CommandType;

    commands
        .iter()
        .find_map(|command| match &command.cmd_type {
            CommandType::ConvexFill { params } | CommandType::Stroke { params } | CommandType::Triangles { params } => {
                Some(params)
            }
            CommandType::ConcaveFill { fill_params, .. } => Some(fill_params),
            CommandType::StencilStroke { params1, .. } => Some(params1),
            _ => None,
        })
        .expect("expected a draw command")
}

#[cfg(all(test, feature = "textlayout"))]
fn first_glyph_draw_params(commands: &[renderer::Command]) -> &Params {
    use renderer::CommandType;

    commands
        .iter()
        .find_map(|command| match &command.cmd_type {
            CommandType::Triangles { params } if params.glyph_texture_type != 0 => Some(params),
            _ => None,
        })
        .expect("expected a glyph draw command")
}

#[cfg(test)]
fn assert_approx_eq(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.001,
        "expected {actual} to be approximately {expected}"
    );
}

#[cfg(test)]
fn fill_rect_with_current_scissor(canvas: &mut Canvas<RecordingRenderer>) {
    let mut path = Path::new();
    path.rect(0.0, 0.0, 100.0, 100.0);
    canvas.fill_path(&path, &Paint::color(Color::white()));
    canvas.flush_to_output(());
}

#[test]
fn failed_image_reallocation_preserves_the_existing_image() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    let image = canvas
        .create_image_empty(32, 24, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.renderer.fail_image_allocations = true;

    assert!(canvas
        .realloc_image(image, 64, 48, PixelFormat::Rgba8, ImageFlags::empty())
        .is_err());
    let info = canvas.image_info(image).unwrap();
    assert_eq!((info.width(), info.height()), (32, 24));
    assert_eq!(canvas.renderer.image_deletion_count, 0);
}

#[test]
fn rounded_scissor_radius_is_clamped_into_render_params() {
    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);

    canvas.rounded_scissor(10.0, 10.0, 40.0, 20.0, 100.0);
    fill_rect_with_current_scissor(&mut canvas);

    let commands = recorded_commands.borrow();
    let params = first_draw_params(&commands);
    assert_eq!(params.glyph_texture_type, 0);
    assert_approx_eq(params.scissor_radius, 10.0);
}

#[cfg(feature = "textlayout")]
#[test]
fn glyph_scissor_ramp_matches_fill_at_high_dpi() {
    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 2.0);

    let font = canvas
        .add_font("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");
    let paint = Paint::color(Color::white()).with_font(&[font]).with_font_size(16.0);

    canvas.rounded_scissor(10.0, 10.0, 56.0, 28.0, 14.0);

    let mut rect = Path::new();
    rect.rect(0.0, 0.0, 100.0, 100.0);
    canvas.fill_path(&rect, &Paint::color(Color::white()));
    canvas.fill_text(12.0, 30.0, "Click", &paint).unwrap();
    canvas.flush_to_output(());

    let commands = recorded_commands.borrow();
    let fill = first_draw_params(&commands);
    let glyph = first_glyph_draw_params(&commands);

    assert_approx_eq(glyph.scissor_scale[0], 2.0);
    assert_approx_eq(glyph.scissor_scale[1], 2.0);
    assert_approx_eq(glyph.scissor_scale[0], fill.scissor_scale[0]);
    assert_approx_eq(glyph.scissor_scale[1], fill.scissor_scale[1]);
}

#[test]
fn intersect_scissor_preserves_contained_rounded_clip() {
    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);

    canvas.rounded_scissor(10.0, 10.0, 40.0, 20.0, 8.0);
    canvas.intersect_scissor(0.0, 0.0, 100.0, 100.0);
    fill_rect_with_current_scissor(&mut canvas);

    let commands = recorded_commands.borrow();
    let params = first_draw_params(&commands);
    assert_eq!(params.glyph_texture_type, 0);
    assert_approx_eq(params.scissor_radius, 8.0);
}

#[test]
fn intersect_scissor_inside_rounded_clip_uses_rectangular_inner_clip() {
    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);

    canvas.rounded_scissor(10.0, 10.0, 80.0, 80.0, 20.0);
    canvas.intersect_scissor(35.0, 35.0, 20.0, 20.0);
    fill_rect_with_current_scissor(&mut canvas);

    let commands = recorded_commands.borrow();
    let params = first_draw_params(&commands);
    assert_eq!(params.glyph_texture_type, 0);
    assert_approx_eq(params.scissor_radius, 0.0);
    assert_approx_eq(params.scissor_ext[0], 10.0);
    assert_approx_eq(params.scissor_ext[1], 10.0);
}

#[test]
fn intersect_rounded_scissor_partial_overlap_falls_back_to_rectangular_intersection() {
    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);

    canvas.rounded_scissor(10.0, 10.0, 40.0, 40.0, 12.0);
    canvas.intersect_rounded_scissor(35.0, 35.0, 40.0, 40.0, 12.0);
    fill_rect_with_current_scissor(&mut canvas);

    let commands = recorded_commands.borrow();
    let params = first_draw_params(&commands);
    assert_eq!(params.glyph_texture_type, 0);
    assert_approx_eq(params.scissor_radius, 0.0);
    assert_approx_eq(params.scissor_ext[0], 7.5);
    assert_approx_eq(params.scissor_ext[1], 7.5);
}

#[test]
fn rounded_scissor_captures_transform_at_clip_time() {
    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);

    canvas.scale(2.0, 3.0);
    canvas.rounded_scissor(10.0, 10.0, 20.0, 10.0, 4.0);
    canvas.reset_transform();
    fill_rect_with_current_scissor(&mut canvas);

    let commands = recorded_commands.borrow();
    let params = first_draw_params(&commands);
    assert_eq!(params.glyph_texture_type, 0);
    assert_approx_eq(params.scissor_radius, 4.0);
    assert_approx_eq(params.scissor_ext[0], 10.0);
    assert_approx_eq(params.scissor_ext[1], 5.0);
    assert_approx_eq(params.scissor_scale[0], 2.0);
    assert_approx_eq(params.scissor_scale[1], 3.0);
}

#[test]
fn intersect_rounded_scissor_uses_inner_radius_when_contained() {
    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);

    canvas.scissor(0.0, 0.0, 100.0, 100.0);
    canvas.intersect_rounded_scissor(10.0, 10.0, 40.0, 20.0, 100.0);
    fill_rect_with_current_scissor(&mut canvas);

    let commands = recorded_commands.borrow();
    let params = first_draw_params(&commands);
    assert_eq!(params.glyph_texture_type, 0);
    assert_approx_eq(params.scissor_radius, 10.0);
}

/// The conic gradient `start_angle` must be plumbed all the way into the
/// rendering `Params` so the shaders can apply it. This is a CI-friendly check
/// that does not require a GPU: it records the commands emitted for a conic fill
/// and inspects the resulting `Params`.
#[test]
fn conic_gradient_start_angle_is_plumbed_into_params() {
    use renderer::{CommandType, ShaderType};

    fn conic_params(paint: &Paint) -> Params {
        let renderer = RecordingRenderer::default();
        let recorded = renderer.last_commands.clone();
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(100, 100, 1.);

        let mut path = Path::new();
        path.circle(50., 50., 40.);
        canvas.fill_path(&path, paint);
        canvas.flush_to_output(());

        let params = recorded
            .borrow()
            .iter()
            .find_map(|cmd| match &cmd.cmd_type {
                CommandType::ConvexFill { params } | CommandType::Stroke { params } => Some(*params),
                CommandType::ConcaveFill { fill_params, .. } => Some(*fill_params),
                _ => None,
            })
            .expect("expected a fill command for the conic gradient");
        params
    }

    // Default (angle-less) constructor must keep start_angle at 0.
    let stops = [(0.0, Color::rgb(255, 0, 0)), (0.5, Color::rgb(0, 0, 255))];
    let params = conic_params(&Paint::conic_gradient_stops(50., 50., stops));
    assert_eq!(params.shader_type, ShaderType::FillImageGradientConic);
    assert_eq!(params.conic_start_angle, 0.0);

    // Explicit-angle multi-stop constructor must carry the angle through.
    let angle = std::f32::consts::FRAC_PI_2;
    let params = conic_params(&Paint::conic_gradient_stops_with_angle(50., 50., angle, stops));
    assert_eq!(params.shader_type, ShaderType::FillImageGradientConic);
    assert_eq!(params.conic_start_angle, angle);

    // Two-color constructor variants.
    let params = conic_params(&Paint::conic_gradient(
        50.,
        50.,
        Color::rgb(255, 0, 0),
        Color::rgb(0, 0, 255),
    ));
    assert_eq!(params.shader_type, ShaderType::FillGradientConic);
    assert_eq!(params.conic_start_angle, 0.0);

    let angle = -std::f32::consts::PI;
    let params = conic_params(&Paint::conic_gradient_with_angle(
        50.,
        50.,
        angle,
        Color::rgb(255, 0, 0),
        Color::rgb(0, 0, 255),
    ));
    assert_eq!(params.shader_type, ShaderType::FillGradientConic);
    assert_eq!(params.conic_start_angle, angle);
}

/// The scissor radius (frag[12].w) and the conic start angle (frag[13].x) live
/// in adjacent uniform slots. A draw that uses both a rounded scissor and a
/// conic gradient with a start angle must carry each value independently in the
/// same `Params`, so neither feature can clobber the other's slot.
#[test]
fn rounded_scissor_and_conic_start_angle_coexist_in_params() {
    use renderer::ShaderType;

    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);

    canvas.rounded_scissor(10.0, 10.0, 80.0, 80.0, 12.0);

    let angle = std::f32::consts::FRAC_PI_2;
    let paint = Paint::conic_gradient_with_angle(50.0, 50.0, angle, Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
    let mut path = Path::new();
    path.rect(0.0, 0.0, 100.0, 100.0);
    canvas.fill_path(&path, &paint);
    canvas.flush_to_output(());

    let commands = recorded_commands.borrow();
    let params = first_draw_params(&commands);
    assert_eq!(params.shader_type, ShaderType::FillGradientConic);
    assert_approx_eq(params.scissor_radius, 12.0);
    assert_approx_eq(params.conic_start_angle, angle);
}

/// Text rendering picks one of two strategies depending on the canvas transform
/// and paint: cached atlas bitmaps (emitting a `Triangles` command that samples a
/// glyph texture) or direct outline rendering (emitting plain path fills with no
/// glyph texture). Verify each canvas use is routed to the expected strategy.
#[cfg(feature = "textlayout")]
#[test]
fn fill_text_selects_atlas_or_path_rendering() {
    use crate::paint::GlyphTexture;
    use renderer::CommandType;

    #[derive(Clone, Copy)]
    enum PaintKind {
        Solid,
        BigSolid,
        Gradient,
    }

    #[derive(Debug, PartialEq)]
    enum Expect {
        Atlas,
        Path,
    }

    // A fresh canvas per case so the persistent glyph atlas (or any other state)
    // built by one case can't influence another. A large viewport plus a
    // near-origin draw position keeps even the heavily scaled cases on-screen —
    // off-screen geometry is culled, which would hide the commands we inspect.
    let make_canvas = || {
        let renderer = RecordingRenderer::default();
        let recorded = renderer.last_commands.clone();
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(4000, 4000, 1.0);
        let font = canvas
            .add_font_mem(include_bytes!("../examples/assets/amiri-regular.ttf"))
            .expect("failed to load test font");
        (canvas, recorded, font)
    };

    // (description, canvas transform, paint, expected strategy)
    let cases = [
        // Pure translation: cached atlas bitmaps, nothing baked.
        (
            "pure translation",
            Transform2D::translation(10.0, 20.0),
            PaintKind::Solid,
            Expect::Atlas,
        ),
        // Uniform scale + solid color: the scale is baked into the atlas bitmap.
        (
            "uniform scale, solid",
            Transform2D::scaling(2.0, 2.0),
            PaintKind::Solid,
            Expect::Atlas,
        ),
        // A scale that quantizes back to 1.0 still uses the atlas.
        (
            "near-unit scale, solid",
            Transform2D::scaling(1.02, 1.02),
            PaintKind::Solid,
            Expect::Atlas,
        ),
        // Gradients can't bake scale (their coords map through the swapped-out
        // transform), so a scaled gradient falls back to outlines.
        (
            "uniform scale, gradient",
            Transform2D::scaling(2.0, 2.0),
            PaintKind::Gradient,
            Expect::Path,
        ),
        // Rotation isn't a uniform scale + translation: outlines.
        (
            "rotation",
            Transform2D::rotation(std::f32::consts::FRAC_PI_4),
            PaintKind::Solid,
            Expect::Path,
        ),
        // Effective size over the 92px atlas cap: outlines.
        (
            "oversized scale",
            Transform2D::scaling(20.0, 20.0),
            PaintKind::Solid,
            Expect::Path,
        ),
        (
            "oversized font",
            Transform2D::identity(),
            PaintKind::BigSolid,
            Expect::Path,
        ),
    ];

    for (description, transform, paint_kind, expect) in cases {
        let (mut canvas, recorded, font) = make_canvas();
        let paint = match paint_kind {
            PaintKind::Solid => Paint::color(Color::black()).with_font(&[font]),
            PaintKind::BigSolid => Paint::color(Color::black()).with_font(&[font]).with_font_size(100.0),
            PaintKind::Gradient => {
                Paint::linear_gradient(0.0, 0.0, 100.0, 0.0, Color::black(), Color::white()).with_font(&[font])
            }
        };

        // A fresh canvas starts at the identity transform.
        canvas.set_transform(&transform);
        canvas.fill_text(10.0, 40.0, "Hello", &paint).unwrap();
        canvas.flush_to_output(());

        let commands = recorded.borrow();
        // Atlas rendering blits glyphs from a glyph texture; outline rendering only
        // ever emits plain path fills (note: atlas cache misses also emit path fills
        // while rasterizing into the atlas, so the glyph texture is the reliable
        // discriminator, not the absence of fills).
        let used_atlas = commands.iter().any(|c| !matches!(c.glyph_texture, GlyphTexture::None));
        let filled_outlines = commands.iter().any(|c| {
            matches!(c.glyph_texture, GlyphTexture::None)
                && matches!(
                    c.cmd_type,
                    CommandType::ConvexFill { .. } | CommandType::ConcaveFill { .. }
                )
        });

        match expect {
            Expect::Atlas => assert!(used_atlas, "expected atlas rendering for case: {description}"),
            Expect::Path => assert!(
                filled_outlines && !used_atlas,
                "expected outline rendering for case: {description} (used_atlas={used_atlas}, filled_outlines={filled_outlines})"
            ),
        }
    }
}

/// A shadow blur above what one shader pass covers (sigma 8, the 24-tap
/// GLES 2.0 loop) runs as the planner's quadrature passes: `shadowBlur` 40 is
/// sigma 20, seven passes of 20 / sqrt(7) whose squares sum back to 400,
/// ping-ponging between the coverage and blurred images, and the composite
/// reads the image the odd count leaves the result in. The offscreen pads by
/// the true reach, 62 px per side, not the 26 of the per-pass bound.
#[test]
fn a_large_shadow_blur_runs_as_quadrature_passes() {
    use renderer::CommandType;

    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(300, 300, 1.0);

    canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
    canvas.set_shadow_blur(40.0);

    let mut path = Path::new();
    path.rect(100.0, 100.0, 20.0, 20.0);
    let mut paint = Paint::color(Color::rgb(255, 0, 0));
    paint.set_anti_alias(false);
    canvas.fill_path(&path, &paint);
    // The coverage, blurred and horizontal scratch images: 20 + 2 * 62 =
    // 144 px square (shadow images round to 8), where the per-pass bound
    // padded 72.
    assert_eq!(canvas.transients.images.len(), 3);
    for &id in &canvas.transients.images {
        assert_eq!(canvas.image_size(id).unwrap(), (144, 144));
    }
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    let passes: Vec<(ImageId, ImageId, f32)> = commands
        .iter()
        .filter_map(|c| match c.cmd_type {
            CommandType::RenderFilteredImage {
                target_image,
                filter: ImageFilter::GaussianBlur { sigma },
            } => Some((c.image.expect("a blur reads an image"), target_image, sigma)),
            _ => None,
        })
        .collect();
    assert_eq!(passes.len(), 7);
    for (_, _, sigma) in &passes {
        assert!((sigma - 20.0 / 7f32.sqrt()).abs() < 1e-5, "{sigma}");
    }
    let composed: f32 = passes.iter().map(|(_, _, s)| s * s).sum::<f32>().sqrt();
    assert!((composed - 20.0).abs() < 1e-3, "{composed}");
    for window in passes.windows(2) {
        let ((src, dst, _), (next_src, next_dst, _)) = (window[0], window[1]);
        assert_eq!((next_src, next_dst), (dst, src), "passes ping-pong");
    }
    let (_, last_target, _) = passes[6];
    let composite = commands
        .iter()
        .rev()
        .find(|c| c.image.is_some() && !matches!(c.cmd_type, CommandType::RenderFilteredImage { .. }))
        .expect("the shadow composite");
    assert_eq!(
        composite.image,
        Some(last_target),
        "the composite reads the seventh pass's target"
    );
}

/// A shadowed text run must perform exactly one run-level shadow pass, no matter
/// how the glyphs are rasterized. Outline-rendered glyphs (large font sizes) are
/// drawn through `fill_path_internal`, whose own shadow hook would otherwise add
/// a per-glyph shadow on top of the run shadow — double-darkening the result and
/// multiplying the offscreen cost by the glyph count. Compare the number of
/// offscreen target switches for a 1-glyph and a many-glyph string: it must not
/// scale with glyph count.
#[cfg(feature = "textlayout")]
#[test]
fn outline_text_shadow_pass_count_is_glyph_count_invariant() {
    use renderer::CommandType;

    let shadow_target_switches = |text: &str| -> usize {
        let renderer = RecordingRenderer::default();
        let recorded = renderer.last_commands.clone();
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(600, 300, 1.0);

        let font = canvas
            .add_font("examples/assets/RobotoFlex-VariableFont.ttf")
            .expect("Font not found");
        // Font size above the atlas cap forces the outline (path) rasterization.
        let paint = Paint::color(Color::white()).with_font(&[font]).with_font_size(100.0);

        canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
        canvas.set_shadow_offset(10.0, 10.0);
        canvas.fill_text(20.0, 150.0, text, &paint).unwrap();
        canvas.flush_to_output(());

        let commands = recorded.borrow();
        commands
            .iter()
            .filter(|c| matches!(c.cmd_type, CommandType::SetRenderTarget(RenderTarget::Image(_))))
            .count()
    };

    let single = shadow_target_switches("I");
    let many = shadow_target_switches("Illuminate");

    assert!(single > 0, "shadowed text must run an offscreen shadow pass");
    assert_eq!(
        single, many,
        "shadow passes must not scale with glyph count: outline glyphs would each cast \
         their own shadow on top of the run-level one"
    );
}

/// Fills `path` once on `canvas`, flushes, and returns the raw bytes of the
/// vertex buffer handed to the renderer for that single fill.
#[cfg(test)]
fn record_fill_bytes(
    canvas: &mut Canvas<RecordingRenderer>,
    verts: &Rc<RefCell<Vec<renderer::Vertex>>>,
    path: &Path,
) -> Vec<u8> {
    canvas.fill_path(path, &Paint::color(Color::white()));
    canvas.flush_to_output(());
    bytemuck::cast_slice(verts.borrow().as_slice()).to_vec()
}

/// `Path` keeps a single-slot interior-mutable tessellation cache keyed by the
/// canvas transform, so a `Path` shared by two canvases with different
/// transforms rebuilds that cache on every hand-off. Thrashing is a
/// performance matter, but leakage would be a correctness bug: one canvas
/// must never observe geometry flattened under the other canvas's transform.
#[test]
fn shared_arc_path_across_canvases_keeps_tessellation_isolated() {
    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    path.svg_arc_to(40.0, 25.0, 0.4, false, true, 120.0, 80.0);

    let make_canvas = || {
        let renderer = RecordingRenderer::default();
        let verts = renderer.last_verts.clone();
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(800, 800, 1.0);
        (canvas, verts)
    };

    // Canvas A stays at the identity; canvas B translates and scales.
    let (mut canvas_a, verts_a) = make_canvas();
    let (mut canvas_b, verts_b) = make_canvas();
    canvas_b.translate(100.0, 0.0);
    canvas_b.scale(2.0, 1.0);

    let a1 = record_fill_bytes(&mut canvas_a, &verts_a, &path);
    let b1 = record_fill_bytes(&mut canvas_b, &verts_b, &path);
    let a2 = record_fill_bytes(&mut canvas_a, &verts_a, &path);
    let b2 = record_fill_bytes(&mut canvas_b, &verts_b, &path);

    assert!(!a1.is_empty() && !b1.is_empty());
    assert_eq!(a1, a2, "canvas A geometry changed after canvas B used the shared path");
    assert_eq!(b1, b2, "canvas B geometry changed after canvas A used the shared path");
    assert_ne!(a1, b1, "the two transforms must produce different geometry");
}

/// Switching the render target between an image and the screen must not
/// perturb the tessellation of a path filled on both: the emitted fill
/// vertices are a function of the path and canvas transform only.
#[test]
fn arc_tessellation_is_stable_across_render_target_switches() {
    let renderer = RecordingRenderer::default();
    let verts = renderer.last_verts.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(400, 400, 1.0);
    let image = canvas
        .create_image_empty(400, 400, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();

    let mut path = Path::new();
    path.move_to(20.0, 200.0);
    path.svg_arc_to(90.0, 60.0, 0.3, true, false, 260.0, 210.0);

    let targets = [
        RenderTarget::Image(image),
        RenderTarget::Screen,
        RenderTarget::Image(image),
        RenderTarget::Screen,
    ];
    let outputs: Vec<Vec<u8>> = targets
        .into_iter()
        .map(|target| {
            canvas.set_render_target(target);
            record_fill_bytes(&mut canvas, &verts, &path)
        })
        .collect();

    assert!(!outputs[0].is_empty());
    for (index, output) in outputs.iter().enumerate().skip(1) {
        assert_eq!(
            &outputs[0], output,
            "render {index} diverged from the first despite identical path and transform"
        );
    }
}

/// `Path` and `Canvas` deliberately contain non-`Sync` interior state (the
/// `RefCell` tessellation cache; `Rc` handles in the canvas), so sharing one
/// `Path` or `Canvas` across threads is rejected at compile time — see the
/// `compile_fail` doctest on [`Path`]. What must hold is that fully
/// independent per-thread instances tessellate deterministically while other
/// threads do the same concurrently.
#[test]
fn arc_tessellation_is_deterministic_across_threads() {
    let workers: Vec<_> = (0..4)
        .map(|index| {
            std::thread::spawn(move || {
                let renderer = RecordingRenderer::default();
                let verts = renderer.last_verts.clone();
                let mut canvas = Canvas::new(renderer).unwrap();
                canvas.set_size(600, 600, 1.0);

                // Give each thread its own arc so a cross-thread mix-up could
                // not hide behind identical inputs.
                let mut path = Path::new();
                path.move_to(10.0 + index as f32, 20.0);
                path.svg_arc_to(80.0 + index as f32, 50.0, 0.2, index % 2 == 0, true, 300.0, 240.0);

                let baseline = record_fill_bytes(&mut canvas, &verts, &path);
                assert!(!baseline.is_empty());
                for iteration in 1..100 {
                    let bytes = record_fill_bytes(&mut canvas, &verts, &path);
                    assert_eq!(
                        baseline, bytes,
                        "thread {index} produced unstable geometry at iteration {iteration}"
                    );
                }
            })
        })
        .collect();

    for worker in workers {
        worker.join().unwrap();
    }
}

/// Collects the screen-space filled rectangles that text decoration emits,
/// grouped by fill command.
///
/// A run's decoration lines are batched into one solid path fill drawn to the
/// screen with no glyph texture: a single enabled line arrives as a
/// one-contour `ConvexFill`, several enabled lines as one multi-contour
/// (concave) fill command. (Atlas glyph rasterization also emits
/// `ConvexFill`s, but those run while the render target is the atlas image,
/// so tracking the active target discriminates them.) Returns one entry per
/// such fill command, listing the vertical `[min_y, max_y]` span of each
/// contour in that command, in screen space.
#[cfg(all(test, feature = "textlayout"))]
fn recorded_decoration_fills(
    commands: &[renderer::Command],
    verts: &[renderer::Vertex],
) -> Vec<Vec<(f32, f32, f32, f32)>> {
    use crate::paint::GlyphTexture;
    use renderer::{CommandType, RenderTarget};

    let mut target = RenderTarget::Screen;
    let mut fills = Vec::new();

    for cmd in commands {
        match &cmd.cmd_type {
            CommandType::SetRenderTarget(new_target) => target = *new_target,
            CommandType::ConvexFill { .. } | CommandType::ConcaveFill { .. }
                if target == RenderTarget::Screen && matches!(cmd.glyph_texture, GlyphTexture::None) =>
            {
                let mut spans = Vec::new();
                for drawable in &cmd.drawables {
                    if let Some((offset, len)) = drawable.fill_verts {
                        let mut min_x = f32::INFINITY;
                        let mut max_x = f32::NEG_INFINITY;
                        let mut min_y = f32::INFINITY;
                        let mut max_y = f32::NEG_INFINITY;
                        for v in &verts[offset..offset + len] {
                            min_x = min_x.min(v.x);
                            max_x = max_x.max(v.x);
                            min_y = min_y.min(v.y);
                            max_y = max_y.max(v.y);
                        }
                        if min_y.is_finite() {
                            spans.push((min_x, min_y, max_x, max_y));
                        }
                    }
                }
                if !spans.is_empty() {
                    fills.push(spans);
                }
            }
            _ => {}
        }
    }

    fills
}

/// With any decoration enabled, `fill_text` emits exactly ONE extra solid fill
/// command for the run, carrying one rect per enabled line positioned from the
/// font's own metrics: underline below the baseline, strikethrough above it
/// (through the text), overline near the ascent. With no decoration, no such
/// fill is emitted.
#[cfg(feature = "textlayout")]
#[test]
fn fill_text_emits_decoration_rects() {
    let make_canvas = || {
        let renderer = RecordingRenderer::default();
        let commands = renderer.last_commands.clone();
        let verts = renderer.last_verts.clone();
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(1000, 1000, 1.0);
        let font = canvas
            .add_font_mem(include_bytes!("../examples/assets/RobotoFlex-VariableFont.ttf"))
            .expect("failed to load test font");
        (canvas, commands, verts, font)
    };

    // Baseline::Alphabetic places the baseline exactly at the draw y, so the
    // metric offsets are easy to reason about. A pure-translation transform keeps
    // text on the cached-atlas path.
    let baseline_y = 200.0_f32;

    // Reference metrics for this font/size, in user space.
    let metrics = {
        let (canvas, _, _, font) = make_canvas();
        let paint = Paint::color(Color::black()).with_font(&[font]).with_font_size(40.0);
        canvas.measure_font(&paint).expect("metrics")
    };
    assert!(metrics.underline_thickness() > 0.0);
    assert!(metrics.strikeout_thickness() > 0.0);

    let base_paint = || {
        Paint::color(Color::black())
            .with_font_size(40.0)
            .with_text_baseline(Baseline::Alphabetic)
    };

    // No decoration: no screen-space solid fills at all.
    {
        let (mut canvas, commands, verts, font) = make_canvas();
        canvas
            .fill_text(50.0, baseline_y, "Hello", &base_paint().with_font(&[font]))
            .unwrap();
        canvas.flush_to_output(());
        let fills = recorded_decoration_fills(&commands.borrow(), &verts.borrow());
        assert!(fills.is_empty(), "expected no decoration fill, got {fills:?}");
    }

    // Underline: one fill command with one rect, centered below the baseline at
    // -underline_position.
    {
        let (mut canvas, commands, verts, font) = make_canvas();
        let paint = base_paint().with_font(&[font]).with_text_decoration(TextDecoration {
            underline: true,
            strikethrough: false,
            overline: false,
        });
        canvas.fill_text(50.0, baseline_y, "Hello", &paint).unwrap();
        canvas.flush_to_output(());
        let fills = recorded_decoration_fills(&commands.borrow(), &verts.borrow());
        assert_eq!(fills.len(), 1, "expected exactly one decoration fill, got {fills:?}");
        assert_eq!(fills[0].len(), 1, "expected exactly one underline rect, got {fills:?}");
        let (_, min_y, _, max_y) = fills[0][0];
        let center = (min_y + max_y) / 2.0;
        let expected = baseline_y - metrics.underline_position();
        assert!(center > baseline_y, "underline should sit below the baseline");
        assert!(
            (center - expected).abs() <= 1.0,
            "underline center {center} should be near {expected}"
        );
        assert!(
            (max_y - min_y - metrics.underline_thickness()).abs() <= 1.0,
            "underline thickness {} should be near {}",
            max_y - min_y,
            metrics.underline_thickness()
        );
    }

    // Strikethrough: one fill command with one rect, above the baseline at
    // -strikeout_position.
    {
        let (mut canvas, commands, verts, font) = make_canvas();
        let paint = base_paint().with_font(&[font]).with_text_decoration(TextDecoration {
            underline: false,
            strikethrough: true,
            overline: false,
        });
        canvas.fill_text(50.0, baseline_y, "Hello", &paint).unwrap();
        canvas.flush_to_output(());
        let fills = recorded_decoration_fills(&commands.borrow(), &verts.borrow());
        assert_eq!(fills.len(), 1, "expected exactly one decoration fill, got {fills:?}");
        assert_eq!(
            fills[0].len(),
            1,
            "expected exactly one strikethrough rect, got {fills:?}"
        );
        let (_, min_y, _, max_y) = fills[0][0];
        let center = (min_y + max_y) / 2.0;
        let expected = baseline_y - metrics.strikeout_position();
        assert!(center < baseline_y, "strikethrough should sit above the baseline");
        assert!(
            (center - expected).abs() <= 1.0,
            "strikethrough center {center} should be near {expected}"
        );
    }

    // Overline: one fill command with one rect, above the ascent.
    {
        let (mut canvas, commands, verts, font) = make_canvas();
        let paint = base_paint().with_font(&[font]).with_text_decoration(TextDecoration {
            underline: false,
            strikethrough: false,
            overline: true,
        });
        canvas.fill_text(50.0, baseline_y, "Hello", &paint).unwrap();
        canvas.flush_to_output(());
        let fills = recorded_decoration_fills(&commands.borrow(), &verts.borrow());
        assert_eq!(fills.len(), 1, "expected exactly one decoration fill, got {fills:?}");
        assert_eq!(fills[0].len(), 1, "expected exactly one overline rect, got {fills:?}");
        let (_, _, _, max_y) = fills[0][0];
        assert!(
            max_y <= baseline_y - metrics.ascender() + 1.0,
            "overline (bottom {max_y}) should sit at/above the ascent {}",
            baseline_y - metrics.ascender()
        );
    }

    // All three at once: still exactly ONE fill command — the lines are batched
    // into a single path and draw — carrying three disjoint rects, one at each
    // metric-derived position.
    {
        let (mut canvas, commands, verts, font) = make_canvas();
        let paint = base_paint().with_font(&[font]).with_text_decoration(TextDecoration {
            underline: true,
            strikethrough: true,
            overline: true,
        });
        canvas.fill_text(50.0, baseline_y, "Hello", &paint).unwrap();
        canvas.flush_to_output(());
        let fills = recorded_decoration_fills(&commands.borrow(), &verts.borrow());
        assert_eq!(
            fills.len(),
            1,
            "all lines must share one decoration fill, got {fills:?}"
        );
        let mut spans = fills[0].clone();
        assert_eq!(spans.len(), 3, "expected three decoration rects, got {spans:?}");

        // Top to bottom: overline above the ascent, strikethrough above the
        // baseline, underline below it — three non-overlapping bands.
        spans.sort_by(|a, b| a.1.total_cmp(&b.1));
        assert!(
            spans[0].3 <= baseline_y - metrics.ascender() + 1.0,
            "overline (bottom {}) should sit at/above the ascent {}",
            spans[0].3,
            baseline_y - metrics.ascender()
        );
        let strike_center = (spans[1].1 + spans[1].3) / 2.0;
        let strike_expected = baseline_y - metrics.strikeout_position();
        assert!(
            (strike_center - strike_expected).abs() <= 1.0,
            "strikethrough center {strike_center} should be near {strike_expected}"
        );
        let under_center = (spans[2].1 + spans[2].3) / 2.0;
        let under_expected = baseline_y - metrics.underline_position();
        assert!(
            (under_center - under_expected).abs() <= 1.0,
            "underline center {under_center} should be near {under_expected}"
        );
        assert!(
            spans[0].3 <= spans[1].1 && spans[1].3 <= spans[2].1,
            "decoration bands should not overlap: {spans:?}"
        );
    }
}

/// A decoration line spans exactly the run's advance box `[layout.x, layout.x
/// + width]`. Alignment moves `layout.x` (Center/Right shift the run left of
/// the requested x), and an RTL run lays its glyphs out right-to-left — in
/// every case the line must track the box the glyphs actually occupy, not the
/// requested draw position.
#[cfg(feature = "textlayout")]
#[test]
fn decoration_rect_spans_the_run_advance_box() {
    let anchor_x = 300.0_f32;
    for (case, text, align) in [
        ("ltr left", "Deco", Align::Left),
        ("ltr center", "Deco", Align::Center),
        ("ltr right", "Deco", Align::Right),
        // Arabic shapes right-to-left; Amiri provides the glyphs.
        ("rtl left", "سلام", Align::Left),
        ("rtl right", "سلام", Align::Right),
    ] {
        let renderer = RecordingRenderer::default();
        let commands = renderer.last_commands.clone();
        let verts = renderer.last_verts.clone();
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(1000, 1000, 1.0);
        let latin = canvas
            .add_font_mem(include_bytes!("../examples/assets/RobotoFlex-VariableFont.ttf"))
            .expect("failed to load test font");
        let arabic = canvas
            .add_font_mem(include_bytes!("../examples/assets/amiri-regular.ttf"))
            .expect("failed to load test font");

        let paint = Paint::color(Color::black())
            .with_font(&[latin, arabic])
            .with_font_size(40.0)
            .with_text_baseline(Baseline::Alphabetic)
            .with_text_align(align)
            .with_text_decoration(TextDecoration {
                underline: true,
                strikethrough: false,
                overline: false,
            });
        let layout = canvas.fill_text(anchor_x, 200.0, text, &paint).unwrap();
        canvas.flush_to_output(());

        assert!(layout.width() > 0.0, "{case}: run should have advance width");
        match align {
            Align::Left => assert!((layout.x - anchor_x).abs() < 1e-3, "{case}: run starts at the anchor"),
            Align::Center | Align::Right => {
                assert!(layout.x < anchor_x, "{case}: aligned run starts left of the anchor")
            }
        }

        let fills = recorded_decoration_fills(&commands.borrow(), &verts.borrow());
        assert_eq!(
            fills.len(),
            1,
            "{case}: expected exactly one decoration fill, got {fills:?}"
        );
        assert_eq!(
            fills[0].len(),
            1,
            "{case}: expected exactly one underline rect, got {fills:?}"
        );
        let (min_x, _, max_x, _) = fills[0][0];
        assert!(
            (min_x - layout.x).abs() <= 0.5,
            "{case}: underline left edge {min_x} should be the run's left edge {}",
            layout.x
        );
        assert!(
            (max_x - (layout.x + layout.width())).abs() <= 0.5,
            "{case}: underline right edge {max_x} should be the run's right edge {}",
            layout.x + layout.width()
        );

        // Independent of the advance-box arithmetic: the line must actually run
        // under every glyph the shaper placed, wherever alignment or RTL
        // ordering put them.
        for glyph in layout.glyphs.iter().filter(|shaped_glyph| !shaped_glyph.c.is_control()) {
            let glyph_center = glyph.x + glyph.width / 2.0;
            assert!(
                min_x <= glyph_center && glyph_center <= max_x,
                "{case}: underline [{min_x}, {max_x}] must run under the glyph at {glyph_center}"
            );
        }
    }
}

/// Under a uniform-scale (scale-baked atlas) transform, the decoration rect must
/// still line up with the glyphs: it is emitted in user space and run through the
/// same canvas transform, so its screen-space center scales with the baseline.
#[cfg(feature = "textlayout")]
#[test]
fn decoration_rect_tracks_scaled_atlas_transform() {
    let renderer = RecordingRenderer::default();
    let commands = renderer.last_commands.clone();
    let verts = renderer.last_verts.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(2000, 2000, 1.0);
    let font = canvas
        .add_font_mem(include_bytes!("../examples/assets/RobotoFlex-VariableFont.ttf"))
        .expect("failed to load test font");

    let baseline_y = 150.0_f32;
    let scale = 2.0_f32;

    let metrics = {
        let paint = Paint::color(Color::black()).with_font(&[font]).with_font_size(24.0);
        canvas.measure_font(&paint).expect("metrics")
    };

    let paint = Paint::color(Color::black())
        .with_font(&[font])
        .with_font_size(24.0)
        .with_text_baseline(Baseline::Alphabetic)
        .with_text_decoration(TextDecoration {
            underline: true,
            strikethrough: false,
            overline: false,
        });

    canvas.scale(scale, scale);
    canvas.fill_text(40.0, baseline_y, "Scaled", &paint).unwrap();
    canvas.flush_to_output(());

    let fills = recorded_decoration_fills(&commands.borrow(), &verts.borrow());
    assert_eq!(fills.len(), 1, "expected exactly one decoration fill, got {fills:?}");
    assert_eq!(fills[0].len(), 1, "expected exactly one underline rect, got {fills:?}");
    let (_, min_y, _, max_y) = fills[0][0];
    let center = (min_y + max_y) / 2.0;
    // User-space underline center is baseline - underline_position; on screen the
    // whole thing is multiplied by the canvas scale.
    let expected = (baseline_y - metrics.underline_position()) * scale;
    assert!(
        (center - expected).abs() <= 2.0,
        "scaled underline center {center} should be near {expected}"
    );
}

/// `TextMetrics::y` and `height` must bracket the ink the glyphs actually
/// draw. `layout()` places each glyph on the alphabetic baseline, so the run
/// box has to back out the glyph's `bearing_y` to reach the ink top. Measuring
/// from the baseline instead put the reported box a cap height below the drawn
/// text, which made the demo's gutter bubble - sized from `measure_text` -
/// render below its line number.
#[cfg(feature = "textlayout")]
#[test]
fn text_metrics_box_brackets_the_drawn_ink() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(200, 100, 1.0);
    let font_id = canvas
        .add_font_mem(include_bytes!("../examples/assets/RobotoFlex-VariableFont.ttf"))
        .expect("failed to load test font");
    let paint = Paint::color(Color::black())
        .with_font(&[font_id])
        .with_font_size(12.0)
        .with_text_baseline(Baseline::Middle);

    let requested_y = 50.0;
    let layout = canvas.measure_text(100.0, requested_y, "1", &paint).unwrap();
    let glyph = layout.glyphs.first().expect("expected a glyph for the digit");

    let ink_top = glyph.y - glyph.bearing_y;
    let ink_bottom = ink_top + glyph.height;
    assert!(
        layout.y <= ink_top + 0.001 && layout.y + layout.height() >= ink_bottom - 0.001,
        "run box {}..{} must contain the glyph ink {ink_top}..{ink_bottom}",
        layout.y,
        layout.y + layout.height()
    );

    // A digit draws entirely above the alphabetic baseline, so the whole box
    // must sit above it too.
    assert!(
        layout.y + layout.height() <= layout.baseline() + 0.001,
        "run box bottom {} must not fall below the baseline {}",
        layout.y + layout.height(),
        layout.baseline()
    );

    // `Baseline::Middle` centers the digit on the requested y, so the box has
    // to start above it. Measuring from the baseline pinned the top at exactly
    // the requested y and pushed the rest below the glyph.
    assert!(
        layout.y < requested_y,
        "with Baseline::Middle the run box must start above the requested y \
         (box top {}, requested {requested_y})",
        layout.y
    );

    // `measure_text` reports user-space units, so a DPI ratio must not skew
    // the relationship between the box, the baseline and the per-glyph
    // bearing: shaping happens in device space and is scaled back down.
    canvas.set_size(200, 100, 2.0);
    let hidpi = canvas.measure_text(100.0, requested_y, "1", &paint).unwrap();
    let hidpi_glyph = hidpi.glyphs.first().expect("expected a glyph for the digit");
    let hidpi_ink_top = hidpi_glyph.y - hidpi_glyph.bearing_y;
    assert!(
        hidpi.y <= hidpi_ink_top + 0.001 && hidpi.y + hidpi.height() >= hidpi_ink_top + hidpi_glyph.height - 0.001,
        "run box {}..{} must contain the glyph ink at a 2x DPI ratio",
        hidpi.y,
        hidpi.y + hidpi.height()
    );
    assert!(
        (hidpi.y - layout.y).abs() < 0.5,
        "the user-space box top must not depend on the DPI ratio ({} at 1x, {} at 2x)",
        layout.y,
        hidpi.y
    );
}

/// The decoration baseline must be the shared run baseline, independent of the
/// first drawable glyph's GPOS y-offset. `layout` bakes that offset into
/// `glyph.y`, so a run beginning with a combining mark (non-zero `offset_y`)
/// would otherwise drag every decoration line up or down with the mark.
///
/// The paragraph base direction follows the first strong character (UAX #9
/// P2/P3). For an RTL paragraph that puts trailing neutral punctuation at the
/// visual LEFT end and orders an embedded LTR word between its Arabic
/// neighbors right-to-left; a pinned-LTR base got both wrong. An LTR-first
/// paragraph with an embedded Arabic word must keep its old ordering.
#[cfg(feature = "textlayout")]
#[test]
fn paragraph_base_direction_follows_first_strong_character() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(1000, 300, 1.0);
    let font_id = canvas
        .add_font_mem(include_bytes!("../examples/assets/amiri-regular.ttf"))
        .expect("failed to load test font");
    let paint = Paint::color(Color::black()).with_font(&[font_id]).with_font_size(30.0);

    // Mean x position of the glyphs whose byte range covers `needle`.
    let cluster_x = |layout: &TextMetrics, text: &str, needle: &str| -> f32 {
        let start = text.find(needle).unwrap();
        let end = start + needle.len();
        let xs: Vec<f32> = layout
            .glyphs
            .iter()
            .filter(|g| g.byte_index >= start && g.byte_index < end)
            .map(|g| g.x)
            .collect();
        assert!(!xs.is_empty(), "no glyphs for {needle:?}");
        xs.iter().sum::<f32>() / xs.len() as f32
    };

    // RTL paragraph, embedded LTR word: visual order is right-to-left, so the
    // logically-later Arabic word sits LEFT of "Alice", which sits LEFT of the
    // logically-first Arabic word.
    let text = "\u{0642}\u{0631}\u{0623} Alice \u{0643}\u{062A}\u{0627}\u{0628}";
    let layout = canvas.measure_text(300.0, 100.0, text, &paint).unwrap();
    let first_word = cluster_x(&layout, text, "\u{0642}\u{0631}\u{0623}");
    let alice = cluster_x(&layout, text, "Alice");
    let last_word = cluster_x(&layout, text, "\u{0643}\u{062A}\u{0627}\u{0628}");
    assert!(
        last_word < alice && alice < first_word,
        "RTL paragraph should order runs right-to-left: got first-word x {first_word}, \
         Alice x {alice}, last-word x {last_word}"
    );

    // Trailing punctuation of an RTL paragraph resolves to the paragraph level
    // and lands at the visual LEFT end.
    let text = "\u{0633}\u{0644}\u{0627}\u{0645}!";
    let layout = canvas.measure_text(300.0, 100.0, text, &paint).unwrap();
    let bang = cluster_x(&layout, text, "!");
    let word = cluster_x(&layout, text, "\u{0633}\u{0644}\u{0627}\u{0645}");
    assert!(
        bang < word,
        "the ! of an RTL sentence belongs at its visual left end (bang x {bang}, word x {word})"
    );

    // European digits inside an RTL paragraph stay an LTR sequence, placed to
    // the left of the word that logically precedes them.
    let text = "\u{0635}\u{0641}\u{062D}\u{0629} 42";
    let layout = canvas.measure_text(300.0, 100.0, text, &paint).unwrap();
    let four = cluster_x(&layout, text, "4");
    let two = cluster_x(&layout, text, "2");
    let page = cluster_x(&layout, text, "\u{0635}\u{0641}\u{062D}\u{0629}");
    assert!(four < two, "digits stay left-to-right: 4 at {four}, 2 at {two}");
    assert!(two < page, "the number sits left of the RTL word that precedes it");

    // An LTR-first paragraph with an embedded RTL word keeps LTR ordering.
    let text = "The word \u{0633}\u{0644}\u{0627}\u{0645} means peace";
    let layout = canvas.measure_text(20.0, 100.0, text, &paint).unwrap();
    let the = cluster_x(&layout, text, "The");
    let salam = cluster_x(&layout, text, "\u{0633}\u{0644}\u{0627}\u{0645}");
    let peace = cluster_x(&layout, text, "peace");
    assert!(
        the < salam && salam < peace,
        "LTR-first paragraph keeps left-to-right run order ({the}, {salam}, {peace})"
    );
}

/// Regression cases from shipped browser bidi bugs (Firefox 726420/459035/
/// 721821, WebKit bug 3435 class, Firefox 1177350's bracket pairing):
/// strong-less text stays LTR, an Arabic-Indic date is one number run in
/// logical order, bracket pairs around an embedded LTR word resolve to the
/// paragraph direction together, and combining marks travel with their base
/// through RTL reversal.
#[cfg(feature = "textlayout")]
#[test]
fn bidi_browser_regression_cases() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(1200, 300, 1.0);
    let font_id = canvas
        .add_font_mem(include_bytes!("../examples/assets/amiri-regular.ttf"))
        .expect("failed to load test font");
    let paint = Paint::color(Color::black()).with_font(&[font_id]).with_font_size(30.0);

    let cluster_x = |layout: &TextMetrics, text: &str, needle: &str| -> f32 {
        let start = text.find(needle).unwrap();
        let end = start + needle.len();
        let xs: Vec<f32> = layout
            .glyphs
            .iter()
            .filter(|g| g.byte_index >= start && g.byte_index < end)
            .map(|g| g.x)
            .collect();
        assert!(!xs.is_empty(), "no glyphs for {needle:?}");
        xs.iter().sum::<f32>() / xs.len() as f32
    };

    // UAX #9 P3: text with no strong character keeps the LTR default, in
    // logical order (Firefox bug 726420's class).
    let text = "123 + 456!";
    let layout = canvas.measure_text(20.0, 100.0, text, &paint).unwrap();
    assert!(
        cluster_x(&layout, text, "123") < cluster_x(&layout, text, "456"),
        "strong-less text must stay left-to-right"
    );
    let bang_x = layout
        .glyphs
        .iter()
        .find(|g| g.byte_index == text.find('!').unwrap())
        .unwrap()
        .x;
    assert!(
        bang_x > cluster_x(&layout, text, "456"),
        "trailing ! of an LTR-defaulted line stays at the right end"
    );

    // UAX #9 W4: a separator between numbers of the same kind joins them into
    // one number run, displayed in logical order - the day stays leftmost in
    // an Arabic-Indic date (Firefox bug 459035).
    let text = "\u{0663}\u{0660}/\u{0661}\u{0662}/\u{0662}\u{0660}\u{0660}\u{0668}";
    let layout = canvas.measure_text(400.0, 100.0, text, &paint).unwrap();
    let day = cluster_x(&layout, text, "\u{0663}\u{0660}");
    let month = cluster_x(&layout, text, "\u{0661}\u{0662}");
    let year = cluster_x(&layout, text, "\u{0662}\u{0660}\u{0660}\u{0668}");
    assert!(
        day < month && month < year,
        "the Arabic-Indic date must stay in logical order left-to-right (day {day}, month {month}, year {year})"
    );

    // UAX #9 N0 (bracket pairs): parens wrapping an embedded LTR word in an
    // RTL paragraph resolve to the paragraph level TOGETHER - the pair
    // surrounds the word, open on the visual right, close on the visual left,
    // both mirrored (the Firefox bug 1177350 / "(GMT+0800 (CST))" class).
    let text = "\u{0627}\u{062E}\u{062A}\u{0628}\u{0627}\u{0631} (test) \u{0646}\u{0635}";
    let layout = canvas.measure_text(400.0, 100.0, text, &paint).unwrap();
    let open_x = layout
        .glyphs
        .iter()
        .find(|g| g.byte_index == text.find('(').unwrap())
        .unwrap()
        .x;
    let close_x = layout
        .glyphs
        .iter()
        .find(|g| g.byte_index == text.find(')').unwrap())
        .unwrap()
        .x;
    let word_x = cluster_x(&layout, text, "test");
    assert!(
        close_x < word_x && word_x < open_x,
        "the bracket pair must surround its LTR content in RTL order (close {close_x}, test {word_x}, open {open_x})"
    );

    // Explicit directional overrides (UAX #9 X1-X8): RLO forces even strong
    // LTR characters to RTL, reversing them visually - the WPT
    // 2d.text.draw.fill.rtl case Chromium's fast glyph path broke (issue
    // 389726691's class). The override mark itself must add no advance.
    let text = "\u{202E}abc\u{202C}";
    let layout = canvas.measure_text(400.0, 100.0, text, &paint).unwrap();
    let gx = |ch: char| {
        layout
            .glyphs
            .iter()
            .find(|g| g.byte_index == text.find(ch).unwrap())
            .unwrap_or_else(|| panic!("no glyph for {ch:?}"))
            .x
    };
    assert!(
        gx('a') > gx('b') && gx('b') > gx('c'),
        "RLO must reverse Latin text visually: a at {}, b at {}, c at {}",
        gx('a'),
        gx('b'),
        gx('c')
    );
    let plain = canvas.measure_text(400.0, 100.0, "abc", &paint).unwrap();
    assert!(
        (layout.width() - plain.width()).abs() < 0.5,
        "RLO/PDF marks must not add advance: {} vs {}",
        layout.width(),
        plain.width()
    );

    // Combining marks travel with their base through RTL reversal (the
    // Firefox bug 721821 class): every Hebrew niqqud mark must sit at the
    // same pen position as its base consonant, not detached at a reversed
    // per-character position.
    let text = "\u{05E9}\u{05B8}\u{05C1}\u{05DC}\u{05D5}\u{05B9}\u{05DD}";
    let layout = canvas.measure_text(400.0, 100.0, text, &paint).unwrap();
    for (mark, base) in [
        ('\u{05B8}', '\u{05E9}'), // qamats under shin
        ('\u{05B9}', '\u{05D5}'), // holam on vav
    ] {
        let mark_i = text.find(mark).unwrap();
        let base_i = text.find(base).unwrap();
        let mark_g = layout.glyphs.iter().find(|g| g.byte_index == mark_i);
        let Some(mark_g) = mark_g else {
            // Some fonts substitute base+mark into one glyph; that also keeps
            // the mark attached, which is what this guards.
            continue;
        };
        let base_g = layout.glyphs.iter().find(|g| g.byte_index == base_i).unwrap();
        assert!(
            (mark_g.x - base_g.x).abs() < 30.0 * 0.8,
            "mark {mark:?} at x {} should ride its base {base:?} at x {}",
            mark_g.x,
            base_g.x
        );
    }
}

/// The shaped-word cache keys on letter spacing: `shape_word` bakes the
/// spacing into the cached advances, so the same word measured at two
/// spacings must not share an entry - previously the second measurement
/// replayed the first's advances.
#[cfg(feature = "textlayout")]
#[test]
fn letter_spacing_does_not_collide_in_the_shaping_cache() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(1000, 300, 1.0);
    let font_id = canvas
        .add_font_mem(include_bytes!("../examples/assets/RobotoFlex-VariableFont.ttf"))
        .expect("failed to load test font");
    let base = Paint::color(Color::black()).with_font(&[font_id]).with_font_size(30.0);

    let plain = canvas.measure_text(20.0, 100.0, "cache", &base).unwrap().width();
    let spaced = canvas
        .measure_text(20.0, 100.0, "cache", &base.clone().with_letter_spacing(6.0))
        .unwrap()
        .width();
    assert!(
        spaced > plain + 4.0 * 6.0 - 1.0,
        "letter spacing must widen the cached word: plain {plain}, spaced {spaced}"
    );
}

/// Bidi control characters are default-ignorable: isolate marks (U+2066..
/// U+2069) and direction marks (U+200E/U+200F) must add no advance and no
/// visible glyph, whether or not the font has real coverage for them - the
/// class behind Firefox bugs 1439018/1440470 (isolates rendered as tofu).
#[cfg(feature = "textlayout")]
#[test]
fn bidi_controls_are_invisible_and_zero_width() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(1000, 300, 1.0);
    let font_id = canvas
        .add_font_mem(include_bytes!("../examples/assets/amiri-regular.ttf"))
        .expect("failed to load test font");
    let paint = Paint::color(Color::black()).with_font(&[font_id]).with_font_size(30.0);

    let bare = "\u{0633}\u{0644}\u{0627}\u{0645}";
    let isolated = "\u{2068}\u{0633}\u{0644}\u{0627}\u{0645}\u{2069}";
    let marked = "\u{200F}\u{0633}\u{0644}\u{0627}\u{0645}\u{200E}";

    let w_bare = canvas.measure_text(300.0, 100.0, bare, &paint).unwrap().width();
    let w_isolated = canvas.measure_text(300.0, 100.0, isolated, &paint).unwrap().width();
    let w_marked = canvas.measure_text(300.0, 100.0, marked, &paint).unwrap().width();

    assert!(
        (w_isolated - w_bare).abs() < 0.5,
        "FSI/PDI must not add advance: bare {w_bare}, isolated {w_isolated}"
    );
    assert!(
        (w_marked - w_bare).abs() < 0.5,
        "LRM/RLM must not add advance: bare {w_bare}, marked {w_marked}"
    );
}

/// Paired brackets mirror in RTL runs (UAX #9 L4): the '(' of an Arabic
/// sentence renders with the ')' glyph. The shaped-word cache must key on the
/// run direction for this to survive cache hits — shaping the same bracket
/// text in an LTR sentence first used to poison the cache with the unmirrored
/// shaping, which the RTL sentence then reused.
#[cfg(feature = "textlayout")]
#[test]
fn brackets_mirror_in_rtl_runs_even_after_ltr_caching() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(1000, 300, 1.0);
    let font_id = canvas
        .add_font_mem(include_bytes!("../examples/assets/amiri-regular.ttf"))
        .expect("failed to load test font");
    let paint = Paint::color(Color::black()).with_font(&[font_id]).with_font_size(30.0);

    let glyph_at = |layout: &TextMetrics, text: &str, needle: char| -> u16 {
        let at = text.find(needle).unwrap();
        layout
            .glyphs
            .iter()
            .find(|g| g.byte_index == at)
            .unwrap_or_else(|| panic!("no glyph for {needle:?}"))
            .glyph_id
    };

    // Prime the shaped-word cache with LTR shapings of the same bracket words.
    let ltr = "he said (yes) loudly";
    let ltr_layout = canvas.measure_text(20.0, 100.0, ltr, &paint).unwrap();
    let ltr_open = glyph_at(&ltr_layout, ltr, '(');
    let ltr_close = glyph_at(&ltr_layout, ltr, ')');
    assert_ne!(ltr_open, ltr_close, "font distinguishes the paren glyphs");

    // The same words inside an Arabic sentence shape as an RTL run: each paren
    // must come out mirrored, not replayed from the LTR cache entry.
    let rtl = "\u{0642}\u{0627}\u{0644} (\u{0646}\u{0639}\u{0645}) \u{0628}\u{0635}\u{0648}\u{062A}";
    let rtl_layout = canvas.measure_text(300.0, 100.0, rtl, &paint).unwrap();
    let rtl_open = glyph_at(&rtl_layout, rtl, '(');
    let rtl_close = glyph_at(&rtl_layout, rtl, ')');
    assert_eq!(
        rtl_open, ltr_close,
        "'(' in an RTL run should render with the ')' glyph (UAX #9 L4 mirroring)"
    );
    assert_eq!(
        rtl_close, ltr_open,
        "')' in an RTL run should render with the '(' glyph (UAX #9 L4 mirroring)"
    );
}

/// `Canvas::measure_font` reports user-space metrics: the same values under
/// any canvas transform or device pixel ratio, in the space `fill_text()`
/// consumes — matching `measure_text()` and `TextContext::measure_font()`.
/// A zoomed canvas must not inflate the metrics consumers use to size
/// sub/superscript runs or position decoration lines; the old behavior
/// multiplied them by the internal quantized glyph-rasterization scale
/// (e.g. 2.3 at a 2.35x zoom).
#[cfg(feature = "textlayout")]
#[test]
fn measure_font_is_transform_independent() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(460, 260, 1.0);
    let font_id = canvas
        .add_font_mem(include_bytes!("../examples/assets/amiri-regular.ttf"))
        .expect("failed to load test font");
    let paint = Paint::color(Color::black()).with_font(&[font_id]).with_font_size(34.0);

    let reference = canvas.measure_font(&paint).expect("metrics at identity");
    // The paint's font size is the only scale input: spot values follow the
    // font's tables (Amiri: OS/2 script sizes of 1433/2048 em).
    assert!((reference.subscript_size().1 - 34.0 * 1433.0 / 2048.0).abs() < 1e-3);

    let assert_matches = |metrics: &FontMetrics, what: &str| {
        for (name, got, want) in [
            ("ascender", metrics.ascender(), reference.ascender()),
            (
                "underline_position",
                metrics.underline_position(),
                reference.underline_position(),
            ),
            (
                "underline_thickness",
                metrics.underline_thickness(),
                reference.underline_thickness(),
            ),
            (
                "subscript_size",
                metrics.subscript_size().1,
                reference.subscript_size().1,
            ),
            (
                "subscript_offset",
                metrics.subscript_offset().1,
                reference.subscript_offset().1,
            ),
            (
                "superscript_offset",
                metrics.superscript_offset().1,
                reference.superscript_offset().1,
            ),
        ] {
            assert!(
                (got - want).abs() < 1e-3,
                "{what}: {name} {got} should equal identity-transform value {want}"
            );
        }
    };

    // A camera-style zoom (translate * scale * translate) must not leak into
    // the metrics. 2.35 quantizes to a 2.3 internal rasterization scale,
    // which the old behavior multiplied in.
    canvas.save();
    canvas.translate(230.0, 130.0);
    canvas.scale(2.35, 2.35);
    canvas.translate(-230.0, -130.0);
    let zoomed = canvas.measure_font(&paint).expect("metrics under zoom");
    canvas.restore();
    assert_matches(&zoomed, "zoomed canvas");

    // Neither must the device pixel ratio.
    canvas.set_size(460, 260, 2.0);
    let hidpi = canvas.measure_font(&paint).expect("metrics under hidpi");
    assert_matches(&hidpi, "hidpi canvas");

    // And the TextContext-level API agrees.
    let context_level = canvas.text_context.borrow_mut().measure_font(
        paint.text.font_size,
        &paint.text.font_ids,
        &paint.text.font_variations,
    );
    assert_matches(
        &context_level.expect("context-level metrics"),
        "TextContext::measure_font",
    );
}

/// The baseline `layout()` stores in `TextMetrics` is the anchor decorations
/// hang from. It must be the run baseline the `Baseline` setting places, not
/// something re-derived from glyph positions, which per-glyph y-offsets
/// (combining marks) would skew.
#[cfg(feature = "textlayout")]
#[test]
fn layout_stores_the_run_baseline() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(400, 200, 1.0);
    let font_id = canvas
        .add_font_mem(include_bytes!("../examples/assets/RobotoFlex-VariableFont.ttf"))
        .expect("failed to load test font");

    let y = 80.0;
    let base_paint = Paint::color(Color::black()).with_font(&[font_id]).with_font_size(24.0);
    let metrics = canvas.measure_font(&base_paint).expect("font metrics");

    // The alphabetic baseline is the requested y itself; the other settings
    // shift it by the run's ascent/descent exactly as layout() aligns glyphs.
    for (setting, expected) in [
        (Baseline::Alphabetic, y),
        (Baseline::Top, y + metrics.ascender()),
        (Baseline::Middle, y + (metrics.ascender() + metrics.descender()) / 2.0),
        (Baseline::Bottom, y + metrics.descender()),
    ] {
        let paint = base_paint.clone().with_text_baseline(setting);
        let layout = canvas.measure_text(15.0, y, "Ay", &paint).expect("shaping succeeds");
        assert!(
            (layout.baseline() - expected).abs() < 1e-3,
            "{setting:?}: baseline {} should be {expected}",
            layout.baseline()
        );
    }
}

/// Chain execution stays within a bounded transient budget: a run of
/// range-safe color matrices folds to a single pass (zero transient images);
/// a matrix that can leave [0, 1] keeps its own clamped pass rather than
/// folding (see [`ImageFilter::fold_with`]); and any chain - however long,
/// whatever the mix - ping-pongs between at most two transient scratches. The
/// WebKit-600MB-intermediates class (bug 218422) made into an invariant.
#[test]
fn filter_chain_bounds_transient_images() {
    use crate::ImageFilter;
    let make = || {
        let renderer = RecordingRenderer::default();
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(64, 64, 1.0);
        let src = canvas
            .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
            .unwrap();
        let dst = canvas
            .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::FLIP_Y)
            .unwrap();
        (canvas, src, dst)
    };

    // A run of range-safe color ops folds into ONE pass: no scratches at all.
    // Each matrix but the last stays within [0, 1]; the last may overflow
    // (brightness(1.5)) since its output is clamped at the end of the pass.
    let (mut canvas, src, dst) = make();
    canvas
        .filter_image_chain(
            dst,
            &[
                ImageFilter::saturate(0.7),
                ImageFilter::grayscale(0.5),
                ImageFilter::opacity(0.8),
                ImageFilter::invert(1.0),
                ImageFilter::brightness(1.5),
            ],
            src,
        )
        .unwrap();
    assert_eq!(
        canvas.transients.images.len(),
        0,
        "folded range-safe color run must not allocate scratches"
    );

    // An overflow-capable matrix breaks the fold so its clamp is preserved,
    // but the chain still caps at two scratches. brightness(2) cannot fold
    // into the next matrix, and sepia(1)*saturate(0) is not range-safe either,
    // so this runs as three passes ping-ponging through two scratches.
    let (mut canvas, src, dst) = make();
    canvas
        .filter_image_chain(
            dst,
            &[
                ImageFilter::brightness(2.0),
                ImageFilter::saturate(0.0),
                ImageFilter::sepia(1.0),
                ImageFilter::invert(1.0),
            ],
            src,
        )
        .unwrap();
    assert_eq!(
        canvas.transients.images.len(),
        2,
        "overflow-broken folds still ping-pong between at most two scratches"
    );

    // A long mixed chain (blurs break the folds) caps at two ping-pong
    // scratches plus one horizontal blur scratch.
    let (mut canvas, src, dst) = make();
    canvas
        .filter_image_chain(
            dst,
            &[
                ImageFilter::GaussianBlur { sigma: 1.0 },
                ImageFilter::sepia(1.0),
                ImageFilter::GaussianBlur { sigma: 2.0 },
                ImageFilter::invert(1.0),
                ImageFilter::GaussianBlur { sigma: 1.5 },
                ImageFilter::brightness(1.3),
            ],
            src,
        )
        .unwrap();
    assert_eq!(
        canvas.transients.images.len(),
        3,
        "mixed chains use one scratch pair and one blur scratch"
    );
    assert_eq!(
        canvas.transients.free.len(),
        3,
        "a finished chain returns all scratches"
    );

    // The empty chain is a single identity pass - a copy, no scratches.
    let (mut canvas, src, dst) = make();
    canvas.filter_image_chain(dst, &[], src).unwrap();
    assert_eq!(canvas.transients.images.len(), 0);
}

/// A layer's root origin accumulates the shift of every enclosing capture -
/// what places a root-device-space mask rect at any depth - while its local
/// origin stays the composite's: an outer capture from (16, 8) and a middle
/// one from (24, 16), which is (8, 8) of the outer store, put the inner
/// store's (0, 0) at root (24, 16). A pass-through layer draws on the
/// enclosing target unchanged, so it carries that target's root origin along
/// rather than adding the origin of the store it never got.
#[test]
fn nested_layers_accumulate_their_root_origin() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    // Three 64 x 64 stores: the deepest nesting below.
    canvas.set_transient_image_budget(4 * 64 * 64 * 4);
    let origins = |canvas: &Canvas<RecordingRenderer>| {
        let record = canvas.layers.last().unwrap();
        (record.origin, record.root_origin)
    };
    canvas.save();
    canvas.scissor(16.0, 8.0, 48.0, 56.0);
    assert!(canvas.begin_layer(&LayerEffects::new()));
    assert_eq!(origins(&canvas), ((16.0, 8.0), (16.0, 8.0)));
    // Device space inside the capture is shifted by (-16, -8).
    canvas.scissor(24.0, 16.0, 40.0, 48.0);
    assert!(canvas.begin_layer(&LayerEffects::new()));
    assert_eq!(origins(&canvas), ((8.0, 8.0), (24.0, 16.0)));
    assert!(canvas.begin_layer(&LayerEffects::new()));
    assert_eq!(origins(&canvas), ((0.0, 0.0), (24.0, 16.0)));
    canvas.end_layer();
    canvas.end_layer();

    // Over the whole outer store, a blurred middle layer wants a 128 x 128
    // store the budget refuses: it passes through at its padded origin
    // without shifting anything, and the layer inside it still captures
    // against the outer store.
    canvas.reset_scissor();
    let blur = ImageFilter::GaussianBlur { sigma: 4.0 };
    assert!(!canvas.begin_layer(&LayerEffects::new().with_filters(&[blur])));
    assert_eq!(origins(&canvas), ((-14.0, -14.0), (16.0, 8.0)));
    assert!(canvas.begin_layer(&LayerEffects::new()));
    assert_eq!(origins(&canvas), ((0.0, 0.0), (16.0, 8.0)));
    canvas.end_layer();
    canvas.end_layer();
    canvas.end_layer();
    canvas.restore();
}

/// A stroke thinner than the fringe is drawn at fringe width with its alpha
/// scaled by the width ratio itself - the linear coverage Skia's hairline
/// path applies - not by its square, the nanovg heuristic that left a 0.4 px
/// line at 16% (`tests/hairline_stroke_wgpu.rs` measures the rendered
/// coverage). The scale is applied to the paint before the `Params` are
/// built, so pinning it on the recorded command holds for every backend.
#[test]
fn sub_pixel_stroke_alpha_scales_linearly_with_width() {
    /// The stroke `Params` recorded for a horizontal white line of
    /// `line_width` user units on a canvas at `dpi`, drawn as a stencilled
    /// stroke (the default) or a plain one.
    fn stroke_params(line_width: f32, dpi: f32, stencil: bool) -> Params {
        let renderer = RecordingRenderer::default();
        let recorded = renderer.last_commands.clone();
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(100, 100, dpi);

        let mut path = Path::new();
        path.move_to(10.0, 50.0);
        path.line_to(90.0, 50.0);
        let paint = Paint::color(Color::white())
            .with_line_width(line_width)
            .with_anti_alias(true)
            .with_stencil_strokes(stencil);
        canvas.stroke_path(&path, &paint);
        canvas.flush_to_output(());

        let params = recorded
            .borrow()
            .iter()
            .find_map(|cmd| match &cmd.cmd_type {
                CommandType::Stroke { params } => Some(*params),
                CommandType::StencilStroke { params1, params2 } => {
                    // Both passes of a stencilled stroke carry the same paint.
                    assert_eq!(params1.inner_col, params2.inner_col);
                    Some(*params1)
                }
                _ => None,
            })
            .expect("expected a stroke command");
        params
    }

    let thin = stroke_params(0.4, 1.0, true);
    let thick = stroke_params(0.8, 1.0, true);

    // `inner_col` is the premultiplied paint colour, so white carries the
    // scaled alpha in every channel.
    assert!(
        (thin.inner_col[3] - 0.4).abs() < 1e-6,
        "0.4 px stroke recorded alpha {}, expected 0.4 (the squared scale gave 0.16)",
        thin.inner_col[3]
    );
    assert!(
        thin.inner_col[..3].iter().all(|c| (c - 0.4).abs() < 1e-6),
        "premultiplied colour {:?} does not carry the scaled alpha",
        thin.inner_col
    );
    assert!(
        (thick.inner_col[3] - 0.8).abs() < 1e-6,
        "0.8 px stroke recorded alpha {}, expected 0.8",
        thick.inner_col[3]
    );
    let ratio = thick.inner_col[3] / thin.inner_col[3];
    assert!(
        (ratio - 2.0).abs() < 1e-6,
        "0.8 px / 0.4 px alpha ratio {ratio}, expected 2 (the squared scale gave 4)"
    );

    // Both are widened to the fringe: the geometry carries no trace of the
    // requested width, only the alpha does.
    assert_eq!(thin.stroke_mult, 1.0);
    assert_eq!(thick.stroke_mult, 1.0);

    // The ratio is against the fringe (one device pixel), not one user unit:
    // at 2x DPI the fringe is half a unit, so a 0.25-unit line is half a pixel.
    let hidpi = stroke_params(0.25, 2.0, true);
    assert!(
        (hidpi.inner_col[3] - 0.5).abs() < 1e-6,
        "0.25-unit stroke at 2x DPI recorded alpha {}, expected 0.5",
        hidpi.inner_col[3]
    );

    // A plain (non-stencilled) stroke goes through the same scale.
    assert!(
        (stroke_params(0.4, 1.0, false).inner_col[3] - 0.4).abs() < 1e-6,
        "plain 0.4 px stroke did not record alpha 0.4"
    );

    // A stroke at the fringe or wider keeps its full alpha.
    assert_eq!(stroke_params(1.0, 1.0, true).inner_col[3], 1.0);
    assert_eq!(stroke_params(3.0, 1.0, true).inner_col[3], 1.0);
}

/// Rebuilds a sfnt/TrueType font byte buffer with the named 4-byte tables
/// removed, so the fallback metric paths can be exercised on real assets.
#[cfg(all(test, feature = "textlayout"))]
fn font_without_tables(data: &[u8], drop_tags: &[&[u8; 4]]) -> Vec<u8> {
    let read_u16 = |buf: &[u8], at: usize| u16::from_be_bytes([buf[at], buf[at + 1]]);
    let read_u32 =
        |buf: &[u8], at: usize| u32::from_be_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]) as usize;

    let num_tables = read_u16(data, 4) as usize;

    // Collect (tag, offset, length) for the tables we keep, in directory order.
    let mut kept: Vec<([u8; 4], usize, usize)> = Vec::new();
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        let tag = [data[rec], data[rec + 1], data[rec + 2], data[rec + 3]];
        if drop_tags.iter().any(|d| **d == tag) {
            continue;
        }
        let offset = read_u32(data, rec + 8);
        let length = read_u32(data, rec + 12);
        kept.push((tag, offset, length));
    }

    let new_num = kept.len();
    let mut out = Vec::new();
    // Offset table header: keep the original sfnt version, fix up the table count
    // and the binary-search hint fields for the new count.
    out.extend_from_slice(&data[0..4]);
    out.extend_from_slice(&(new_num as u16).to_be_bytes());
    let max_pow2: u16 = 1 << (15 - (new_num.max(1) as u16).leading_zeros());
    out.extend_from_slice(&(max_pow2 * 16).to_be_bytes());
    out.extend_from_slice(&(15 - max_pow2.leading_zeros() as u16).to_be_bytes());
    out.extend_from_slice(&((new_num as u16 * 16).wrapping_sub(max_pow2 * 16)).to_be_bytes());

    let mut data_offset = 12 + new_num * 16;
    let mut records = Vec::new();
    let mut blobs = Vec::new();
    for (tag, offset, length) in kept {
        let padded = (length + 3) & !3;
        let mut blob = data[offset..offset + length].to_vec();
        blob.resize(padded, 0);
        let mut rec = Vec::new();
        rec.extend_from_slice(&tag);
        rec.extend_from_slice(&0u32.to_be_bytes()); // checksum (ignored by ttf-parser)
        rec.extend_from_slice(&(data_offset as u32).to_be_bytes());
        rec.extend_from_slice(&(length as u32).to_be_bytes());
        records.push(rec);
        blobs.push(blob);
        data_offset += padded;
    }
    for rec in records {
        out.extend_from_slice(&rec);
    }
    for blob in blobs {
        out.extend_from_slice(&blob);
    }
    out
}

/// A font without an OS/2 table (so no strikeout metric) and without a post
/// table (so no underline metric) must still yield sensible, finite, positive
/// decoration metrics via the ascender/descender-derived fallbacks — and never
/// panic when drawing.
#[cfg(feature = "textlayout")]
#[test]
fn decoration_metrics_fall_back_without_os2_and_post() {
    let original = include_bytes!("../examples/assets/amiri-regular.ttf");

    // Sanity: ttf-parser sees no strikeout/underline once the tables are gone.
    let stripped = font_without_tables(original, &[b"OS/2", b"post"]);
    let face = ttf_parser::Face::parse(&stripped, 0).expect("stripped font should still parse");
    assert!(
        face.strikeout_metrics().is_none(),
        "OS/2 strikeout should be absent after stripping"
    );
    assert!(
        face.underline_metrics().is_none(),
        "post underline should be absent after stripping"
    );

    let text_context = TextContext::default();
    let font_id = text_context.add_font_mem(&stripped).expect("stripped font should load");
    let paint = Paint::default().with_font(&[font_id]).with_font_size(20.0);

    let metrics = text_context.measure_font(&paint).expect("metrics");

    assert!(
        metrics.strikeout_thickness() > 0.0 && metrics.strikeout_thickness().is_finite(),
        "fallback strikeout thickness must be positive and finite, got {}",
        metrics.strikeout_thickness()
    );
    assert!(
        metrics.strikeout_position() > 0.0 && metrics.strikeout_position().is_finite(),
        "fallback strikeout should sit above the baseline, got {}",
        metrics.strikeout_position()
    );
    assert!(
        metrics.underline_thickness() > 0.0 && metrics.underline_thickness().is_finite(),
        "fallback underline thickness must be positive and finite, got {}",
        metrics.underline_thickness()
    );
    assert!(
        metrics.underline_position() < 0.0 && metrics.underline_position().is_finite(),
        "fallback underline should sit below the baseline, got {}",
        metrics.underline_position()
    );

    // Drawing with the fallback font must not panic and must still emit both
    // rects, batched into the run's single decoration fill.
    let renderer = RecordingRenderer::default();
    let commands = renderer.last_commands.clone();
    let verts = renderer.last_verts.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(1000, 1000, 1.0);
    let font = canvas
        .add_font_mem(&font_without_tables(original, &[b"OS/2", b"post"]))
        .expect("stripped font should load into canvas");
    let paint = Paint::color(Color::black())
        .with_font(&[font])
        .with_font_size(20.0)
        .with_text_baseline(Baseline::Alphabetic)
        .with_text_decoration(TextDecoration {
            underline: true,
            strikethrough: true,
            overline: false,
        });
    canvas.fill_text(20.0, 100.0, "fallback", &paint).unwrap();
    canvas.flush_to_output(());
    let fills = recorded_decoration_fills(&commands.borrow(), &verts.borrow());
    assert_eq!(fills.len(), 1, "expected exactly one decoration fill, got {fills:?}");
    assert_eq!(
        fills[0].len(),
        2,
        "expected underline + strikethrough rects in one fill, got {fills:?}"
    );
}

/// Random interleavings of save / restore / begin_layer / end_layer /
/// translate / clip_path / fill / flush / set_render_target against a model
/// of one stack whose entries are saves or layers, each owning the clips
/// taken at its level. After every step: the state stack and the layer list
/// agree with the model, the clip stack holds exactly the levels' clips and
/// the top level records its depth, every target's plane counts its own
/// entries, the render target is the model's, and outside layers the
/// transform is the model's.
#[cfg(test)]
#[test]
fn random_api_sequences_keep_one_consistent_stack() {
    use rand::{RngExt, SeedableRng};

    #[derive(Clone, Copy)]
    struct Level {
        layer: bool,
        transform: Transform2D,
        clips: usize,
        // The render target to return to when this level is a layer.
        previous_target: RenderTarget,
    }

    for seed in 0..48u64 {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
        canvas.set_size(128, 128, 1.0);
        let image = canvas
            .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
            .unwrap();
        let mut model = vec![Level {
            layer: false,
            transform: Transform2D::identity(),
            clips: 0,
            previous_target: RenderTarget::Screen,
        }];
        let mut target = RenderTarget::Screen;
        let effects = LayerEffects::new().with_opacity(0.5);
        let mut rect = Path::new();
        rect.rect(4.0, 4.0, 20.0, 20.0);

        for step in 0..160 {
            match rng.random_range(0..9) {
                0 => {
                    canvas.save();
                    let top = *model.last().unwrap();
                    model.push(Level {
                        layer: false,
                        clips: 0,
                        previous_target: target,
                        ..top
                    });
                }
                1 => {
                    canvas.restore();
                    if model.len() > 1 {
                        let popped = model.pop().unwrap();
                        if popped.layer {
                            target = popped.previous_target;
                        }
                    }
                }
                2 => {
                    let _ = canvas.begin_layer(&effects);
                    let top = *model.last().unwrap();
                    model.push(Level {
                        layer: true,
                        clips: 0,
                        previous_target: target,
                        ..top
                    });
                    // A captured layer redirects drawing to its store; a
                    // pass-through one draws on.
                    if let Some(store) = canvas.layers.last().and_then(|layer| layer.image) {
                        target = RenderTarget::Image(store);
                    }
                }
                3 => {
                    canvas.end_layer();
                    if let Some(boundary) = model.iter().rposition(|level| level.layer) {
                        target = model[boundary].previous_target;
                        model.truncate(boundary);
                    }
                }
                4 => {
                    let mut clip = Path::new();
                    clip.rect(rng.random_range(0.0..40.0), rng.random_range(0.0..40.0), 60.0, 60.0);
                    canvas.clip_path(&clip, FillRule::NonZero);
                    model.last_mut().unwrap().clips += 1;
                }
                5 => canvas.fill_path(&rect, &Paint::color(Color::black())),
                6 => canvas.flush_to_output(()),
                7 => {
                    target = if rng.random_range(0..2) == 0 {
                        RenderTarget::Screen
                    } else {
                        RenderTarget::Image(image)
                    };
                    canvas.set_render_target(target);
                }
                _ => {
                    let (dx, dy) = (rng.random_range(-8.0..8.0), rng.random_range(-8.0..8.0));
                    canvas.translate(dx, dy);
                    if !model.iter().any(|level| level.layer) {
                        model.last_mut().unwrap().transform.translate(dx, dy);
                    }
                }
            }

            let at = format!("seed {seed} step {step}");
            assert_eq!(canvas.state_stack.len(), model.len(), "{at}: state stack");
            assert_eq!(
                canvas.layers.len(),
                model.iter().filter(|level| level.layer).count(),
                "{at}: open layers"
            );
            for layer in &canvas.layers {
                assert!(layer.state_depth <= canvas.state_stack.len(), "{at}: boundary");
            }
            let clips: usize = model.iter().map(|level| level.clips).sum();
            assert_eq!(canvas.clip_stack.len(), clips, "{at}: clip stack");
            assert_eq!(canvas.state().clip_depth, clips, "{at}: top level's clip depth");
            for (plane_target, plane) in &canvas.clip_planes {
                let entries = canvas
                    .clip_stack
                    .iter()
                    .filter(|entry| entry.target == *plane_target)
                    .count();
                assert_eq!(plane.count, entries, "{at}: plane count for {plane_target:?}");
            }
            for entry in &canvas.clip_stack {
                assert!(
                    canvas.clip_planes.contains_key(&entry.target),
                    "{at}: plane for {:?}",
                    entry.target
                );
            }
            assert_eq!(canvas.current_render_target, target, "{at}: render target");
            if !model.iter().any(|level| level.layer) {
                assert_eq!(
                    canvas.state().transform,
                    model.last().unwrap().transform,
                    "{at}: transform"
                );
            }
        }
        canvas.flush_to_output(());
    }
}
