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

mod transient;
pub use crate::image::{ImageFilter, ImageFlags, ImageId, ImageInfo, ImageSource, PixelFormat, TurbulenceKind};
use crate::transient::TransientPool;

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

#[derive(Debug)]
struct ClipGeometry {
    vertices: Box<[Vertex]>,
}

#[derive(Debug)]
struct ClipEntry {
    geometry: Rc<ClipGeometry>,
    fill_rule: FillRule,
    target: RenderTarget,
    bounds: Bounds,
    prior_armed: Rect,
    armed: Rect,
}

#[derive(Copy, Clone, Debug)]
struct ClipPlaneState {
    count: usize,
    dirty: bool,
    armed: Rect,
}

/// Effects applied to a layer when [`Canvas::end_layer`] composites it back.
///
/// Declared up front at [`Canvas::begin_layer`] - like Canvas 2D's
/// `beginLayer(filter)` proposal - so the layer's backing store can be sized
/// for the effects (a blur needs kernel-reach padding). Construct with
/// [`LayerEffects::new`] (what `Default` gives too) and the builder methods;
/// more effect kinds can be added without breaking callers.
#[derive(Clone, Debug)]
pub struct LayerEffects {
    opacity: f32,
    // Shared, so a layer record clones a pointer rather than the list:
    // a scene of thousands of filtered groups costs one copy per group
    // declaration, not one per open layer.
    filters: Rc<[ImageFilter]>,
    mask: Option<LayerMask>,
}

/// How a layer mask's coverage is derived from its image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskKind {
    /// Coverage is the mask content's luminance (Rec. 709 weights on its
    /// sRGB values) times its alpha - SVG `mask`'s default `mask-type`.
    Luminance,
    /// Coverage is the mask content's own alpha channel.
    Alpha,
}

/// The transients a mask draws its coverage through: the mask normalized
/// into layer space and, for a luminance mask, the alpha it converts to.
#[derive(Clone, Copy, Debug)]
struct MaskImages {
    normalized: ImageId,
    converted: Option<ImageId>,
}

#[derive(Clone, Copy, Debug, Default)]
struct FilterScratchImages {
    chain: [Option<ImageId>; 2],
    blur: Option<ImageId>,
}

impl FilterScratchImages {
    fn images(self) -> impl Iterator<Item = ImageId> {
        self.chain.into_iter().flatten().chain(self.blur)
    }
}

/// The transients a filter chain draws through: its result, the pair its
/// passes ping-pong between, and one horizontal Gaussian-blur scratch.
#[derive(Clone, Copy, Debug)]
struct FilterImages {
    target: ImageId,
    scratch: FilterScratchImages,
}

#[derive(Clone, Copy, Debug)]
struct LayerMask {
    image: ImageId,
    kind: MaskKind,
    // Device-space placement of the mask image.
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl LayerEffects {
    /// No-op effects: full opacity, no filters, no mask.
    pub fn new() -> Self {
        Self {
            opacity: 1.0,
            filters: Rc::default(),
            mask: None,
        }
    }

    /// Sets the group opacity the composite applies to the layer as a whole.
    ///
    /// This is SVG group-opacity / Canvas layer semantics: overlapping
    /// children inside the layer do NOT double-blend; the finished layer is
    /// faded as one image. Non-finite values are ignored; the value clamps
    /// to [0, 1].
    #[must_use]
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        if opacity.is_finite() {
            self.opacity = opacity.clamp(0.0, 1.0);
        }
        self
    }

    /// Sets an image-filter chain applied to the captured layer before it is
    /// composited, executing through
    /// [`filter_image_chain`](Canvas::filter_image_chain) - runs of color
    /// matrices still fold to one pass. The chain's result and scratches are
    /// reserved at [`begin_layer`](Canvas::begin_layer) with the layer's store.
    /// Under resource pressure a source-reading chain is omitted while group
    /// opacity is preserved; a source-replacing turbulence chain fails closed.
    #[must_use]
    pub fn with_filters(mut self, filters: &[ImageFilter]) -> Self {
        self.filters = filters.iter().take(MAX_FILTER_PASSES + 1).copied().collect();
        self
    }

    /// Masks the layer by `image`, placed at the device-space rect
    /// `(x, y, width, height)` - device space because a mask is a raster
    /// captured the way the layer is - with coverage derived per `kind`:
    /// SVG `mask` semantics, [`MaskKind::Luminance`] being SVG's default
    /// mask-type, computed on the mask's sRGB values as SVG's default
    /// `color-interpolation` has it. Applied after the filter chain, SVG's
    /// order for a group carrying both `filter` and `mask`. Pixels the mask
    /// rect does not cover are fully masked out. The rect stays root device
    /// space - the space of the target the outermost open layer draws on -
    /// however deep the masked layer nests: an enclosing layer's capture
    /// origin does not shift it.
    ///
    /// The mask image is borrowed, not owned: render mask content into your
    /// own image (upload or render target - its `ImageFlags` orientation is
    /// respected). A deletion requested while the layer is open is deferred
    /// through its composite flush. If coverage storage cannot be reserved
    /// after the layer is captured, the capture is discarded rather than
    /// composited unmasked.
    #[must_use]
    pub fn with_mask(mut self, image: ImageId, kind: MaskKind, x: f32, y: f32, width: f32, height: f32) -> Self {
        self.mask = Some(LayerMask {
            image,
            kind,
            x,
            y,
            width,
            height,
        });
        self
    }
}

// Hand-written so the default is `new()`'s no-op effects: a derived Default
// would zero the opacity and make `LayerEffects::default()` a layer that
// composites nothing.
impl Default for LayerEffects {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct LayerRecord {
    // None marks a layer without a capture. It either passes through or, when
    // `discard` is set, suppresses draws that would be unsafe to expose.
    image: Option<ImageId>,
    // Optional effect storage reserved with the capture.
    mask_images: Option<MaskImages>,
    filter_images: Option<FilterImages>,
    reserved_filter_work: u64,
    discard: bool,
    previous_target: RenderTarget,
    // Where the store lands on the previous target: the composite's origin,
    // in that target's device space.
    origin: (f32, f32),
    // Root device coordinates of the store's (0, 0): `origin` plus the shift
    // of every enclosing capture. A pass-through layer draws on the enclosing
    // target unchanged and carries that target's root origin along. A mask
    // rect is root device space, so it is placed against this, not `origin`.
    root_origin: (f32, f32),
    width: usize,
    height: usize,
    effects: LayerEffects,
    outer_alpha: f32,
    // The state stack's length while the layer's own entry - the save
    // `begin_layer` pushes - is on top. That entry is the layer's boundary:
    // `restore()` at it closes the layer, `end_layer()` restores to it, and
    // the saves above it belong to the layer's content.
    state_depth: usize,
}

impl LayerRecord {
    /// Every transient the layer holds - its capture, the mask's coverage
    /// images and the filter chain's - which is what a flush keeps live and
    /// what `end_layer` or a discard returns to the pool.
    fn images(&self) -> impl Iterator<Item = ImageId> {
        let mask = self.mask_images;
        let filter = self.filter_images;
        self.image
            .into_iter()
            .chain(mask.map(|images| images.normalized))
            .chain(mask.and_then(|images| images.converted))
            .chain(filter.map(|images| images.target))
            .chain(filter.into_iter().flat_map(|images| images.scratch.images()))
    }
}

/// The largest standard deviation a blur chain, a layer filter or a shadow
/// renders; above it the sigma is clamped. A cost guard: the split below
/// runs `(sigma / 8)^2` passes of two full-size draws each, so this is 256
/// passes - sigma 128 device pixels is a CSS `blur(40px)` at a 3x device
/// pixel ratio, and reaches 386 px - and an absurd or non-finite sigma
/// cannot plan an unbounded pass count. Browsers stop at Skia's `kMaxSigma`
/// of 532 (SkBlurImageFilter.cpp, a 1000 px box kernel), which they reach by
/// downscaling or running-sum box blurs, not by pass count; that path is
/// femtovg/femtovg#325's.
const MAX_CHAIN_BLUR_SIGMA: f32 = 128.0;

/// The standard deviation a chain renders for a requested `sigma`: `None`
/// for a degenerate one (zero, negative, NaN), which the coefficient
/// sanitization renders as a copy, else the value clamped to
/// [`MAX_CHAIN_BLUR_SIGMA`]. The one place the pass split and the store
/// padding read a blur's sigma, so the passes a chain runs and the reach a
/// layer or shadow pads for cannot disagree.
fn chain_blur_sigma(sigma: f32) -> Option<f32> {
    (sigma > 0.0).then(|| sigma.min(MAX_CHAIN_BLUR_SIGMA))
}

/// How a Gaussian blur of `sigma` runs within the shader's per-pass bound
/// ([`renderer::MAX_BLUR_SIGMA`]): `(passes, sigma per pass)`. Gaussians
/// compose in quadrature - k passes of sigma s blur like one pass of
/// s * sqrt(k) - so a sigma above the bound B is exactly k = ceil((sigma / B)^2)
/// passes of sigma / sqrt(k), each at most B: sigma 16 is four passes of 8,
/// sigma 23 nine of 23/3. A sigma within the bound, or a degenerate one, is
/// one pass with the value untouched, so small blurs render exactly as they
/// did before the split existed. The cost is quadratic in sigma (each pass is
/// two full-size draws), which is what the ceiling above bounds.
/// The blur padding a store of `extent` px can afford under the backend's
/// texture `limit`, given that stores round up to `granularity`: the full
/// `pad` when it fits, else what leaves the rounded store within the limit,
/// never negative. A layer or shadow at the limit then captures with its
/// reach truncated at the store edge rather than passing through.
fn bounded_pad(pad: f32, extent: f32, limit: usize, granularity: usize) -> f32 {
    if pad <= 0.0 {
        return 0.0;
    }
    let room = limit as f32 - extent.ceil() - granularity as f32;
    pad.min((room * 0.5).floor().max(0.0))
}

fn blur_passes(sigma: f32) -> (usize, f32) {
    let bound = renderer::MAX_BLUR_SIGMA;
    match chain_blur_sigma(sigma) {
        Some(sigma) if sigma > bound => {
            let ratio = sigma / bound;
            let passes = (ratio * ratio).ceil() as usize;
            (passes, sigma / (passes as f32).sqrt())
        }
        _ => (1, sigma),
    }
}

const MAX_FILTER_PASSES: usize = 257;

/// The deepest the state stack goes, WebKit's canvas limit. A level costs
/// about a hundred bytes plus whatever it clips, so this bounds the stack
/// near 1.6 MiB however deeply an untrusted scene nests. Past it the canvas
/// saturates: a `save()` or `begin_layer()` becomes one entry that its
/// `restore()` or `end_layer()` pairs with, and until the last of those
/// entries pops nothing draws and the changes made to the state are
/// discarded - the conservative reading of every effect a layer past the
/// limit could have declared.
const MAX_STATE_DEPTH: usize = 16 * 1024;

const DEFAULT_FILTER_WORK_BUDGET: u64 = 4 * 1024 * 1024 * 1024;

fn filter_work(filters: &[ImageFilter], width: usize, height: usize) -> u64 {
    let samples = filters.iter().fold(0u64, |total, filter| {
        let per_pixel = match filter {
            ImageFilter::GaussianBlur { sigma } => {
                let sigma = if *sigma > 0.0 {
                    sigma.min(renderer::MAX_BLUR_SIGMA)
                } else {
                    1e-3
                };
                let radius = (3.0 * sigma).ceil() as u64;
                2 * (1 + 2 * radius.saturating_sub(1))
            }
            ImageFilter::Turbulence { num_octaves, .. } => (8 * u64::from((*num_octaves).min(10))).max(1),
            _ => 1,
        };
        total.saturating_add(per_pixel)
    });
    (width as u64).saturating_mul(height as u64).saturating_mul(samples)
}

/// The passes a filter list runs as: runs of adjacent color matrices folded
/// where that is exact ([`ImageFilter::fold_with`]), each Gaussian blur
/// above the shader's per-pass bound split into the passes that compose to
/// it ([`blur_passes`]), plus an identity pass when the flip count comes out
/// even, so every chain shape leaves storage flipped once - which makes the
/// empty list a copy. The result is never empty; `None` rejects a plan above
/// [`MAX_FILTER_PASSES`]. What [`Canvas::filter_image_chain`] executes and
/// what a layer's scratch reservation is sized from, so the two cannot disagree.
fn filter_passes(filters: &[ImageFilter]) -> Option<Vec<ImageFilter>> {
    if filters.len() > MAX_FILTER_PASSES {
        return None;
    }
    // Fold first, split second: a blur never folds today, but the split
    // passes are adjacent blurs, and expanding after the fold keeps them
    // from being folded back should a fold of blurs ever exist. ImageFilter
    // is Copy, so neither list deep-copies anything.
    let mut folded: Vec<ImageFilter> = Vec::with_capacity(filters.len().min(MAX_FILTER_PASSES));
    for filter in filters {
        if let Some(prev) = folded.last_mut() {
            if let Some(merged) = prev.fold_with(*filter) {
                *prev = merged;
                continue;
            }
        }
        folded.push(*filter);
    }
    // The capacity covers every split pass plus the parity pass, so the list
    // never reallocates.
    let count = folded.iter().try_fold(0usize, |count, filter| {
        let passes = match filter {
            ImageFilter::GaussianBlur { sigma } => blur_passes(*sigma).0,
            _ => 1,
        };
        count.checked_add(passes).filter(|count| *count <= MAX_FILTER_PASSES)
    })?;
    let mut passes: Vec<ImageFilter> = Vec::with_capacity(count + 1);
    for filter in folded {
        match filter {
            ImageFilter::GaussianBlur { sigma } => {
                let (count, sigma) = blur_passes(sigma);
                passes.extend(std::iter::repeat_n(ImageFilter::GaussianBlur { sigma }, count));
            }
            other => passes.push(other),
        }
    }
    // A color-matrix pass flips the image (the render-target convention),
    // the two-pass Gaussian blur preserves it - however many of them a
    // split adds, the parity is the unsplit chain's.
    let flips = passes.iter().filter(|f| f.flips_output()).count();
    if flips % 2 == 0 {
        if passes.len() == MAX_FILTER_PASSES {
            return None;
        }
        passes.push(ImageFilter::identity());
    }
    Some(passes)
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

    /// Drops every open layer without compositing it: the state stack
    /// rebalances, the layers' images return to the pool and drawing
    /// continues on the target that was current before the outermost layer.
    fn discard_open_layers(&mut self) {
        let mut outermost_target = None;
        while let Some(record) = self.layers.pop() {
            self.restore_to_layer_boundary(&record);
            self.refund_filter_work(record.reserved_filter_work);
            for image in record.images() {
                self.release_transient_image(image);
            }
            outermost_target = Some(record.previous_target);
        }
        if let Some(target) = outermost_target {
            self.set_render_target(target);
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

    /// Whether the stack is past [`MAX_STATE_DEPTH`]: nothing draws until
    /// the entries past it are popped.
    fn saturated(&self) -> bool {
        !self.overflow.is_empty()
    }

    /// Records a save or layer past [`MAX_STATE_DEPTH`] and says so; below
    /// the limit it records nothing and the caller pushes a real level. The
    /// first entry snapshots the deepest real state, which setters keep
    /// writing into meanwhile; popping the last entry restores the snapshot.
    fn push_past_depth_limit(&mut self, layer: bool) -> bool {
        if !self.saturated() && self.state_stack.len() < MAX_STATE_DEPTH {
            return false;
        }
        if self.overflow.is_empty() {
            self.overflow_state = *self.state_stack.last().unwrap();
        }
        if !self.warned_overflow {
            self.warned_overflow = true;
            log::warn!("the state stack is {MAX_STATE_DEPTH} deep: nothing draws until it unwinds");
        }
        self.overflow.push(layer);
        true
    }

    /// Pops the innermost entry past the limit, reporting whether it was a
    /// layer; the last one restores the state saved when saturation began.
    fn pop_past_depth_limit(&mut self) -> Option<bool> {
        let layer = self.overflow.pop()?;
        self.restore_after_saturation();
        Some(layer)
    }

    /// Pops through the innermost layer entry past the limit; false, popping
    /// nothing, when none of the entries is a layer.
    fn pop_layer_past_depth_limit(&mut self) -> bool {
        let Some(boundary) = self.overflow.iter().rposition(|&layer| layer) else {
            return false;
        };
        self.overflow.truncate(boundary);
        self.restore_after_saturation();
        true
    }

    fn restore_after_saturation(&mut self) {
        if self.overflow.is_empty() {
            *self.state_stack.last_mut().unwrap() = self.overflow_state;
        }
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

    /// Restores to `record`'s boundary: the saves left open inside the layer
    /// go with it, then the layer's own entry. The stack cannot be below the
    /// boundary while the layer is open - `restore()` closes the layer
    /// rather than cross it - so this pops at least the layer's entry.
    fn restore_to_layer_boundary(&mut self, record: &LayerRecord) {
        debug_assert!(self.state_stack.len() >= record.state_depth);
        // Entries past the limit sit above every real boundary.
        self.overflow.clear();
        self.state_stack.truncate(record.state_depth);
        self.pop_state();
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

    /// Drops the clips above `depth`. Their target planes are reconciled only
    /// when drawing resumes on them, so consecutive restores coalesce.
    fn pop_clips_to(&mut self, depth: usize) {
        if self.clip_stack.len() <= depth {
            return;
        }

        let mut removed = HashMap::<RenderTarget, (usize, Rect)>::new();
        for entry in self.clip_stack.drain(depth..) {
            removed
                .entry(entry.target)
                .and_modify(|(count, _)| *count += 1)
                .or_insert((1, entry.prior_armed));
        }
        for (target, (count, armed)) in removed {
            let plane = self
                .clip_planes
                .get_mut(&target)
                .expect("a clip entry has a target plane");
            plane.count -= count;
            plane.dirty = true;
            plane.armed = armed;
        }
    }

    fn reconcile_current_clip_plane(&mut self) {
        // A suppressed layer records no current-target draws, so defer stencil
        // repair instead of marking an unrecorded replay clean.
        if self.commands_suppressed() {
            return;
        }
        let target = self.current_render_target;
        let dirty = self.clip_planes.get(&target).is_some_and(|plane| plane.dirty);
        if !dirty {
            return;
        }
        self.clip_planes.get_mut(&target).unwrap().dirty = false;
        self.replay_clip_stack();
        if self.clip_planes.get(&target).is_some_and(|plane| plane.count == 0) {
            self.clip_planes.remove(&target);
        }
    }

    /// Whether the stencil clip gates draws into the current render target:
    /// some clip on the stack was taken on it.
    fn clip_active(&self) -> bool {
        self.clip_planes
            .get(&self.current_render_target)
            .is_some_and(|plane| plane.count != 0)
    }

    fn forget_clip_target(&mut self, target: RenderTarget) {
        if !self.clip_planes.contains_key(&target) {
            return;
        }
        let removed: Vec<usize> = self
            .clip_stack
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| (entry.target == target).then_some(index))
            .collect();
        if !removed.is_empty() {
            for state in &mut self.state_stack {
                state.clip_depth -= removed.partition_point(|&index| index < state.clip_depth);
            }
            self.clip_stack.retain(|entry| entry.target != target);
        }
        self.clip_planes.remove(&target);
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

    /// Sets the color of drop shadows drawn behind subsequent fills, strokes and text.
    ///
    /// This mirrors the Canvas 2D `shadowColor` attribute. The default is a fully
    /// transparent color (`rgba(0, 0, 0, 0)`), which disables shadows: when the
    /// shadow color is fully transparent no offscreen shadow pass is performed and
    /// drawing adds zero overhead. The shadow color's own alpha multiplies the
    /// shadow's coverage.
    ///
    /// # Performance
    ///
    /// Shadows are not cheap: every shadowed fill, stroke or text draw renders the
    /// shape's coverage into a transient offscreen image sized to its padded
    /// bounds, runs a two-pass Gaussian blur over it (when `shadowBlur` is
    /// non-zero), and composites the result — per draw, every frame. Prefer
    /// shadowing a few composed shapes over many small primitives, and if the
    /// same shadowed shape is drawn every frame, consider rendering it once into
    /// an [image](Self::create_image_empty) via
    /// [`set_render_target`](Self::set_render_target) and re-drawing that cached
    /// image instead. Setting a fully transparent shadow color restores the
    /// zero-overhead path.
    pub fn set_shadow_color(&mut self, color: Color) {
        self.state_mut().shadow_color = color;
    }

    /// Sets the blur radius applied to drop shadows.
    ///
    /// This mirrors the Canvas 2D `shadowBlur` attribute. Following the HTML
    /// drawing model, the shadow image is blurred with a Gaussian whose standard
    /// deviation is `shadowBlur / 2`, expressed in output (device) pixels. The
    /// default is `0` (no blur). Negative or non-finite values are ignored.
    ///
    /// One blur shader pass covers a standard deviation of 8 device pixels
    /// (`shadowBlur` 16; its kernel reach is bounded at +/-24 px, a GLES 2.0
    /// constraint on loop bounds). A larger blur runs as several passes that
    /// compose to the requested sigma, the way a filter chain's blur does
    /// (see [`filter_image_chain`](Self::filter_image_chain)), with the
    /// shadow's offscreen padded by the full reach, so `shadowBlur` 40 spreads
    /// like the browsers' sigma 20 rather than a sigma-8 one; the pass count
    /// grows with the square of the sigma, up to a sigma of 128 (`shadowBlur`
    /// 256), and each pass's kernel stops at 2.875 sigma, so the composed
    /// blur lands within 2 % of the requested sigma.
    pub fn set_shadow_blur(&mut self, blur: f32) {
        if blur.is_finite() && blur >= 0.0 {
            self.state_mut().shadow_blur = blur;
        }
    }

    /// Sets the drop shadow offset, in output (device) pixels.
    ///
    /// This mirrors the Canvas 2D `shadowOffsetX`/`shadowOffsetY` attributes.
    /// Positive `x` shifts the shadow right and positive `y` shifts it down. Per
    /// the spec the offset is *not* affected by the current transformation
    /// matrix: it keeps the same magnitude and direction relative to the shape
    /// regardless of scale or rotation. Non-finite values are ignored (the
    /// previous offset is preserved), matching the Canvas setter semantics used
    /// by `set_shadow_blur`. The default is `(0, 0)`.
    pub fn set_shadow_offset(&mut self, x: f32, y: f32) {
        if x.is_finite() && y.is_finite() {
            self.state_mut().shadow_offset = [x, y];
        }
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
        self.saturated() || self.layers.iter().any(|layer| layer.discard && layer.image.is_none())
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
        if self.pending_image_deletions.contains(&id) {
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
        if self.pending_image_deletions.contains(&id) {
            return Err(ErrorKind::ImageIdNotFound);
        }
        self.images.update(&mut self.renderer, id, src.into(), x, y)
    }

    /// Deletes an image at the next flush, after earlier commands are encoded.
    /// An open layer borrowing it as a mask keeps it through that layer's
    /// composite and the following flush.
    pub fn delete_image(&mut self, id: ImageId) {
        self.defer_image_deletion(id);
    }

    fn defer_image_deletion(&mut self, id: ImageId) {
        if self.images.info(id).is_none() || !self.pending_image_deletions.insert(id) {
            return;
        }
        if self.current_render_target == RenderTarget::Image(id) {
            self.set_render_target(RenderTarget::Screen);
        }
        for layer in &mut self.layers {
            if layer.previous_target == RenderTarget::Image(id) {
                layer.previous_target = RenderTarget::Screen;
            }
        }
        self.forget_clip_target(RenderTarget::Image(id));
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

    /// Renders the given `source_image` into `target_image` while applying a filter effect.
    ///
    /// The target image must have the same size as the source image. The filtering is recorded
    /// as a drawing command and run by the renderer when [`Self::flush()`] is called.
    ///
    /// The filtering does not take any transformation set on the Canvas into account nor does it
    /// change the current rendering target.
    ///
    /// This is one shader pass, and a Gaussian blur pass renders a standard
    /// deviation of at most 8 device pixels (the shader's kernel is bounded
    /// at 24 taps per side, a GLES 2.0 loop constraint): a larger `sigma` is
    /// clamped to 8 here. For a blur above that use
    /// [`filter_image_chain`](Self::filter_image_chain), which splits it
    /// into passes that compose to the requested sigma.
    ///
    /// [`ImageFilter::Turbulence`] reads nothing from `source_image` - it only takes the output
    /// size from it - and keeps a small per-seed cache of lattice textures (512 KB each, the
    /// last four seeds used) alive across flushes.
    /// Unsafe in-place sampling filters, over-budget work and a blur that
    /// cannot reserve its transient scratch leave the target unchanged.
    pub fn filter_image(&mut self, target_image: ImageId, filter: ImageFilter, source_image: ImageId) {
        let Ok((image_width, image_height)) = self.image_size(source_image) else {
            return;
        };
        if self.image_info(target_image).is_err() {
            return;
        }
        if target_image == source_image
            && !matches!(
                filter,
                ImageFilter::GaussianBlur { .. } | ImageFilter::Turbulence { .. }
            )
        {
            return;
        }
        let work = filter_work(std::slice::from_ref(&filter), image_width, image_height);
        if !self.reserve_filter_work(work) {
            return;
        }
        let blur_scratch = if matches!(filter, ImageFilter::GaussianBlur { .. }) {
            match self.acquire_transient_image(image_width, image_height, ImageFlags::PREMULTIPLIED) {
                Ok(image) => Some(image),
                Err(_) => {
                    self.refund_filter_work(work);
                    return;
                }
            }
        } else {
            None
        };
        let recorded = self.filter_image_with_scratch(target_image, filter, source_image, blur_scratch);
        if let Some(image) = blur_scratch {
            self.release_transient_image(image);
        }
        if !recorded {
            self.refund_filter_work(work);
        }
    }

    fn filter_image_with_scratch(
        &mut self,
        target_image: ImageId,
        filter: ImageFilter,
        source_image: ImageId,
        blur_scratch: Option<ImageId>,
    ) -> bool {
        debug_assert_eq!(
            matches!(filter, ImageFilter::GaussianBlur { .. }),
            blur_scratch.is_some()
        );
        debug_assert!(
            target_image != source_image
                || matches!(
                    filter,
                    ImageFilter::GaussianBlur { .. } | ImageFilter::Turbulence { .. }
                )
        );
        if let Some(scratch) = blur_scratch {
            debug_assert!(scratch != source_image && scratch != target_image);
        }
        let Ok((image_width, image_height)) = self.image_size(source_image) else {
            return false;
        };

        // The renderer will receive a RenderFilteredImage command with two triangles attached that
        // cover the image and the source image. A turbulence pass generates rather than samples,
        // so it binds its noise lattice where the source would go; the source still sizes the quad.
        let sampled = match filter {
            ImageFilter::Turbulence { seed, .. } => match self.turbulence_lattice(seed) {
                Ok(lattice) => lattice,
                Err(_) => return false,
            },
            _ => source_image,
        };
        let mut cmd = Command::new(CommandType::RenderFilteredImage { target_image, filter });
        cmd.image = Some(sampled);
        cmd.filter_scratch = blur_scratch;

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

        self.append_cmd(cmd);
        if let Some(plane) = self.clip_planes.get_mut(&RenderTarget::Image(target_image)) {
            plane.dirty = true;
        }
        true
    }

    /// The lattice texture for a turbulence seed, built on first use and kept
    /// for the [`turbulence::LATTICE_CACHE_CAPACITY`] most recently used seeds.
    /// An evicted lattice is deleted at the next flush, once any command
    /// already recorded against it has run.
    fn turbulence_lattice(&mut self, seed: i32) -> Result<ImageId, ErrorKind> {
        if let Some(at) = self.turbulence_lattices.iter().position(|(s, _)| *s == seed) {
            let entry = self.turbulence_lattices.remove(at);
            self.turbulence_lattices.push(entry);
            return Ok(entry.1);
        }
        let texels = turbulence::lattice_texels(seed);
        let id = self.create_image(turbulence::lattice_source(&texels), turbulence::lattice_flags())?;
        self.turbulence_lattices.push((seed, id));
        if self.turbulence_lattices.len() > turbulence::LATTICE_CACHE_CAPACITY {
            let (_, evicted) = self.turbulence_lattices.remove(0);
            self.defer_image_deletion(evicted);
        }
        Ok(id)
    }

    fn prepare_turbulence_lattices(&mut self, filters: &[ImageFilter]) -> Result<(), ErrorKind> {
        for filter in filters {
            if let ImageFilter::Turbulence { seed, .. } = filter {
                self.turbulence_lattice(*seed)?;
            }
        }
        Ok(())
    }

    /// Acquires a transient offscreen image from the pool; see `transient.rs`.
    fn acquire_transient_image(
        &mut self,
        width: usize,
        height: usize,
        flags: ImageFlags,
    ) -> Result<ImageId, ErrorKind> {
        self.transients
            .acquire(&mut self.images, &mut self.renderer, width, height, flags)
    }

    fn acquire_transient_image_reserving(
        &mut self,
        width: usize,
        height: usize,
        flags: ImageFlags,
        headroom: usize,
    ) -> Result<ImageId, ErrorKind> {
        self.transients
            .acquire_reserving(&mut self.images, &mut self.renderer, width, height, flags, headroom)
    }

    /// Returns a transient image to the pool once every command that reads it
    /// has been recorded.
    fn release_transient_image(&mut self, id: ImageId) {
        // The pool may hand the image to the next layer; clip state from a
        // discarded layer's store must not follow it there.
        self.forget_clip_target(RenderTarget::Image(id));
        self.transients.release(&self.images, id);
    }

    /// Cancels a reservation before it records commands. Fresh images are
    /// freed immediately; reused images remain alive for earlier commands.
    fn rollback_transient_image(&mut self, id: ImageId) {
        self.forget_clip_target(RenderTarget::Image(id));
        self.transients.rollback(&mut self.images, &mut self.renderer, id);
    }

    /// Returns the backend-estimated bytes charged to the transient budget,
    /// including conservative attachment and blur-scratch reservations. This
    /// is an admission estimate, not a renderer-wide live-allocation counter.
    pub fn transient_image_bytes(&self) -> usize {
        self.transients.bytes()
    }

    /// Caps the estimated cost admitted for transient layer, filter, mask, and
    /// shadow images. The default is 128 MiB. Work that does not fit degrades
    /// as documented by the operation. Smaller targets should set a
    /// platform-appropriate value.
    pub fn set_transient_image_budget(&mut self, bytes: usize) {
        self.transients.set_budget(bytes);
    }

    /// Caps texture-sampling work recorded between flushes by filter chains,
    /// turbulence and shadow blurs. The default is roughly 4 billion samples.
    /// Work that does not fit degrades as documented by the operation.
    pub fn set_filter_work_budget(&mut self, samples: u64) {
        self.filter_work_budget = samples;
    }

    fn reserve_filter_work(&mut self, work: u64) -> bool {
        if self.filter_work.saturating_add(work) > self.filter_work_budget {
            return false;
        }
        self.filter_work = self.filter_work.saturating_add(work);
        true
    }

    fn refund_filter_work(&mut self, work: u64) {
        self.filter_work = self.filter_work.saturating_sub(work);
    }

    /// Applies a list of image filters as one chain, `filters[0]` first —
    /// the execution model behind a Canvas `ctx.filter` list
    /// (`"blur(5px) brightness(1.2)"`) and SVG filter chains.
    ///
    /// Runs of adjacent color-matrix filters fold into a single matrix on the
    /// CPU (see [`ImageFilter::fold_with`]), so a run of color operations costs
    /// one GPU pass - as long as each matrix but the last stays within [0, 1];
    /// one that can overflow (`brightness(>1)`, `contrast`, `sepia`) keeps its
    /// own pass so its clamp still happens, matching how browsers clamp per
    /// filter function. A Gaussian blur whose standard deviation is above the
    /// 8 device pixels one shader pass covers runs as `ceil((sigma / 8)^2)`
    /// passes of `sigma / sqrt(passes)`: Gaussians compose in quadrature, so
    /// four passes of sigma 8 are exactly one blur of sigma 16, and nine of
    /// 23/3 one of 23 - the full reach, where the single-pass
    /// [`filter_image`](Self::filter_image) would clamp to 8. The pass count
    /// grows with the square of the sigma, so sigma is capped at 128 and one
    /// operation is capped at 257 total planned passes. Passes that do not fold ping-pong between at most two
    /// transient scratch images sized like the source; a blur plan reserves
    /// one more full-size horizontal scratch, so peak transient
    /// memory is twice the source image, or three times across a blur - bounded
    /// regardless of chain length or pass count either way. The scratches are
    /// freed at the next flush. All color work is in unpremultiplied sRGB with
    /// output clamped to [0, 1] per pass, so an alpha-amplifying matrix feeding
    /// a blur cannot blow out later passes.
    ///
    /// The target ends up in the same orientation convention as a single
    /// color-matrix [`filter_image`](Self::filter_image) call: content stored
    /// vertically flipped, sampled upright via [`ImageFlags::FLIP_Y`], and
    /// carrying premultiplied alpha - create chain targets with
    /// `ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y` so semi-transparent
    /// results composite once, not twice. An empty list degrades to a plain
    /// copy under that same convention. A chain whose flip parity comes out
    /// even (for example a lone blur) pays one extra identity pass for that
    /// uniformity; blur-only callers who want the single-pass form can call
    /// `filter_image` directly.
    ///
    /// Returns [`ErrorKind::ImageIdNotFound`] when either image is missing,
    /// [`ErrorKind::RenderTargetError`] when a single sampling pass would read
    /// and write the same image, [`ErrorKind::FilterPassLimitExceeded`] when
    /// the chain exceeds the per-operation pass cap,
    /// [`ErrorKind::FilterWorkBudgetExceeded`] when it exceeds the command
    /// stream's work budget, and [`ErrorKind::TransientImageBudgetExceeded`]
    /// when its scratches do not fit; either way no pass runs and
    /// `target_image` is left as it was. A layer's chain never
    /// stops here - its scratches are reserved at
    /// [`begin_layer`](Self::begin_layer) with the layer's store.
    ///
    /// The chain borrows `source_image` and `target_image` without taking
    /// ownership - both may be caller-managed or acquired transients (a layer
    /// capture pass, say, can feed its layer in as `source_image` and keep
    /// releasing it on its own schedule). Only the internal scratches are
    /// tied to the flush lifecycle, so composing this under group effects
    /// adds no copies and no extra retained images.
    pub fn filter_image_chain(
        &mut self,
        target_image: ImageId,
        filters: &[ImageFilter],
        source_image: ImageId,
    ) -> Result<(), ErrorKind> {
        let passes = filter_passes(filters).ok_or(ErrorKind::FilterPassLimitExceeded)?;
        let (width, height) = self.image_size(source_image)?;
        self.image_info(target_image)?;
        if target_image == source_image && filters.is_empty() {
            return Ok(());
        }
        if target_image == source_image && passes.len() == 1 && !matches!(passes[0], ImageFilter::Turbulence { .. }) {
            return Err(ErrorKind::RenderTargetError(
                "a single-pass filter cannot read and write the same image".into(),
            ));
        }
        let work = filter_work(&passes, width, height);
        if !self.reserve_filter_work(work) {
            return Err(ErrorKind::FilterWorkBudgetExceeded);
        }
        if let Err(err) = self.prepare_turbulence_lattices(&passes) {
            self.refund_filter_work(work);
            return Err(err);
        }
        let scratch = self
            .acquire_filter_scratches(
                width,
                height,
                passes.len(),
                passes
                    .iter()
                    .any(|filter| matches!(filter, ImageFilter::GaussianBlur { .. })),
                2,
                0,
            )
            .inspect_err(|_| self.refund_filter_work(work))?;
        self.run_filter_passes(target_image, &passes, source_image, scratch, false);
        Ok(())
    }

    /// Acquires the scratches a chain of `passes` ping-pongs between: none
    /// for a single pass, which writes its target directly, one for two, two
    /// beyond, whatever the chain's length. Scratches hold premultiplied
    /// filter output; the flag keeps every consumer (filter passes and
    /// composites) reading them under the same alpha convention - without it,
    /// semi-transparent content is premultiplied a second time at each read
    /// and darkens per pass. Holds nothing on failure.
    fn acquire_filter_scratches(
        &mut self,
        width: usize,
        height: usize,
        passes: usize,
        needs_blur: bool,
        chain_limit: usize,
        headroom: usize,
    ) -> Result<FilterScratchImages, ErrorKind> {
        let mut scratch = FilterScratchImages::default();
        for i in 0..passes.saturating_sub(1).min(chain_limit) {
            match self.acquire_transient_image_reserving(width, height, ImageFlags::PREMULTIPLIED, headroom) {
                Ok(id) => scratch.chain[i] = Some(id),
                Err(err) => {
                    for id in scratch.images() {
                        self.rollback_transient_image(id);
                    }
                    return Err(err);
                }
            }
        }
        if needs_blur {
            match self.acquire_transient_image_reserving(width, height, ImageFlags::PREMULTIPLIED, headroom) {
                Ok(id) => scratch.blur = Some(id),
                Err(err) => {
                    for id in scratch.images() {
                        self.rollback_transient_image(id);
                    }
                    return Err(err);
                }
            }
        }
        Ok(scratch)
    }

    /// Runs `passes` (a [`filter_passes`] plan) from `source_image` into
    /// `target_image`, ping-ponging through the `scratch` images acquired for
    /// that plan, and releases them once the chain is recorded: they are free
    /// for the next chain (or layer) of this size.
    fn run_filter_passes(
        &mut self,
        target_image: ImageId,
        passes: &[ImageFilter],
        source_image: ImageId,
        scratch: FilterScratchImages,
        target_as_scratch: bool,
    ) {
        debug_assert!(!target_as_scratch || target_image != source_image);
        let mut src = source_image;
        let last = passes.len() - 1;
        for (i, filter) in passes.iter().enumerate() {
            let dst = if i == last || (target_as_scratch && (last - i).is_multiple_of(2)) {
                target_image
            } else {
                let index = if target_as_scratch { 0 } else { i % 2 };
                scratch.chain[index].expect("a scratch was acquired for every pass but the last")
            };
            let blur_scratch = matches!(filter, ImageFilter::GaussianBlur { .. })
                .then(|| scratch.blur.expect("a blur scratch was reserved"));
            let _ = self.filter_image_with_scratch(dst, *filter, src, blur_scratch);
            src = dst;
        }
        for id in scratch.images() {
            self.release_transient_image(id);
        }
    }

    /// Opens a layer that [`end_layer`](Self::end_layer) composites with the
    /// declared opacity, filters, and mask. The current scissor bounds the
    /// capture when it is an axis-aligned rectangle; blur reach expands it.
    ///
    /// Returns `false` only when no capture fits and ordinary content passes
    /// through. Its current alpha is scaled by the requested opacity as an
    /// approximation; overlapping draws need a capture for true group opacity.
    /// A `true` layer is isolated or safely suppressed. A captured layer keeps
    /// group opacity, omits an ordinary filter if needed, and suppresses content
    /// whose mask or source-replacing filter cannot be applied.
    #[must_use = "false means ordinary content is using the pass-through fallback"]
    pub fn begin_layer(&mut self, effects: &LayerEffects) -> bool {
        if self.push_past_depth_limit(true) {
            // Past the depth limit the layer is one entry that end_layer()
            // pairs with, and its content is suppressed: the safe reading
            // of every effect it could have declared, so `true`.
            return true;
        }
        let state = *self.state();
        // The store spans what is being drawn into: the canvas, or an image
        // render target of another size (a layer opened while rendering to
        // an offscreen larger than the canvas must capture all of it).
        let (canvas_w, canvas_h) = self.render_target_size();
        // Root device coordinates of that target's (0, 0): its own when no
        // layer is open, else the enclosing layer's store origin in root
        // space - which a pass-through enclosing layer inherits unchanged.
        let root = self.layers.last().map_or((0.0, 0.0), |layer| layer.root_origin);

        // A layer opened under a non-invertible transform is not rasterizable
        // at all - not even for draws that set a valid transform inside it
        // (WPT 2d.layer.non-invertible-matrix). Capture into a small void
        // store that end_layer never composites (outer alpha 0).
        let [a, b, c, d, _, _] = state.transform.0;
        if (a * d - b * c).abs() < 1e-6 {
            let void = transient::LAYER_GRANULARITY;
            let image = self
                .acquire_transient_image(void, void, ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y)
                .ok();
            self.layers.push(LayerRecord {
                image,
                mask_images: None,
                filter_images: None,
                reserved_filter_work: 0,
                discard: image.is_none(),
                previous_target: self.current_render_target,
                origin: (0.0, 0.0),
                root_origin: root,
                width: void,
                height: void,
                effects: effects.clone(),
                outer_alpha: 0.0,
                state_depth: self.state_stack.len() + 1,
            });
            self.save();
            if let Some(image) = image {
                self.set_render_target(RenderTarget::Image(image));
                self.clear_rect(0, 0, void as u32, void as u32, Color::rgbaf(0.0, 0.0, 0.0, 0.0));
            }
            return true;
        }

        // Blur reach padding: 3 sigma covers >99.7% of the kernel. The sigma
        // is each blur's true one (the chain runs a blur above the shader's
        // per-pass bound as passes that compose to it, so its reach is real),
        // clamped only at the chain ceiling. Successive Gaussians compound in
        // quadrature - n blurs of sigma reach like one of sigma * sqrt(n) - so
        // the reach of a chain is the root of the sum of squares.
        let sigma_sq: f32 = effects
            .filters
            .iter()
            .filter_map(|f| match f {
                ImageFilter::GaussianBlur { sigma } => chain_blur_sigma(*sigma),
                _ => None,
            })
            .map(|sigma| sigma * sigma)
            .sum();
        let pad = if sigma_sq > 0.0 {
            (sigma_sq.sqrt() * 3.0).ceil() + 2.0
        } else {
            0.0
        };

        // A rounded or rotated scissor has no device rect: the store spans
        // the canvas and the scissor applies once, at the composite, after
        // the filters - SVG's clip-path over a filtered group and Canvas 2D's
        // clip under `ctx.filter` both clip the result, not the input.
        let rect = state
            .scissor
            .device_bounds(canvas_w, canvas_h)
            .unwrap_or_else(|| Rect::new(0.0, 0.0, canvas_w, canvas_h));
        // The true reach can push a full-width store past the backend's
        // texture limit (2048 px on a VideoCore IV); bound the pad so the
        // layer still captures with its reach truncated at the store edge,
        // instead of passing through with every effect dropped.
        let pad = bounded_pad(
            pad,
            rect.w.max(rect.h),
            self.renderer.max_texture_size(),
            transient::LAYER_GRANULARITY,
        );
        let minx = (rect.x - pad).floor().max(-pad);
        let miny = (rect.y - pad).floor().max(-pad);
        let maxx = (rect.x + rect.w + pad).ceil().min(canvas_w + pad);
        let maxy = (rect.y + rect.h + pad).ceil().min(canvas_h + pad);
        let width = transient::round_up((maxx - minx) as usize, transient::LAYER_GRANULARITY);
        let height = transient::round_up((maxy - miny) as usize, transient::LAYER_GRANULARITY);

        // Past the backend's texture limit (2048 px on a VideoCore IV), an
        // ordinary layer passes through; effects that cannot safely expose
        // their source are suppressed below.
        let limit = self.renderer.max_texture_size();
        let image = if width == 0 || height == 0 || width > limit || height > limit {
            None
        } else {
            // Render-target storage is premultiplied and vertically flipped;
            // FLIP_Y makes the unfiltered composite sample it upright.
            self.acquire_transient_image(width, height, ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y)
                .ok()
        };

        let fail_closed = image.is_none()
            && (effects.mask.is_some()
                || effects
                    .filters
                    .iter()
                    .any(|filter| matches!(filter, ImageFilter::Turbulence { .. }))
                || state.alpha * effects.opacity <= 0.0);
        let mut record = LayerRecord {
            image,
            mask_images: None,
            filter_images: None,
            reserved_filter_work: 0,
            discard: fail_closed,
            previous_target: self.current_render_target,
            origin: (minx, miny),
            root_origin: root,
            width,
            height,
            effects: effects.clone(),
            outer_alpha: state.alpha,
            state_depth: self.state_stack.len() + 1,
        };
        if let Some(image) = image {
            record.root_origin = (root.0 + minx, root.1 + miny);
            let headroom = self
                .images
                .info(image)
                .map(|info| self.renderer.transient_image_cost(info))
                .unwrap_or(0);

            if let Some(mask) = effects.mask {
                let work = match mask.kind {
                    MaskKind::Alpha => 0,
                    MaskKind::Luminance => {
                        filter_work(std::slice::from_ref(&ImageFilter::luminance_to_alpha()), width, height)
                    }
                };
                if self.reserve_filter_work(work) {
                    if let Some(images) = self.reserve_mask_images(width, height, mask.kind, headroom) {
                        record.mask_images = Some(images);
                        record.reserved_filter_work = work;
                    } else {
                        self.refund_filter_work(work);
                        record.discard = true;
                    }
                } else {
                    record.discard = true;
                }
            }

            if !record.discard && !effects.filters.is_empty() {
                let replacing = effects
                    .filters
                    .iter()
                    .any(|filter| matches!(filter, ImageFilter::Turbulence { .. }));
                if let Some(passes) = filter_passes(&effects.filters) {
                    let work = filter_work(&passes, width, height);
                    if self.reserve_filter_work(work) {
                        if let Some(images) = self.reserve_filter_images(width, height, &effects.filters, headroom) {
                            if self.prepare_turbulence_lattices(&passes).is_ok() {
                                record.filter_images = Some(images);
                                record.reserved_filter_work = record.reserved_filter_work.saturating_add(work);
                            } else {
                                for image in std::iter::once(images.target).chain(images.scratch.images()) {
                                    self.rollback_transient_image(image);
                                }
                                self.refund_filter_work(work);
                                record.discard = replacing;
                            }
                        } else {
                            self.refund_filter_work(work);
                            record.discard = replacing;
                        }
                    } else {
                        record.discard = replacing;
                    }
                } else {
                    record.discard = replacing;
                }
            }

            if record.discard {
                let effect_images: Vec<_> = record
                    .mask_images
                    .into_iter()
                    .flat_map(|images| [Some(images.normalized), images.converted])
                    .flatten()
                    .chain(
                        record
                            .filter_images
                            .into_iter()
                            .flat_map(|images| std::iter::once(images.target).chain(images.scratch.images())),
                    )
                    .collect();
                for held in effect_images {
                    self.rollback_transient_image(held);
                }
                self.refund_filter_work(record.reserved_filter_work);
                record.mask_images = None;
                record.filter_images = None;
                record.reserved_filter_work = 0;
            }
        }
        let image = record.image;
        let discard = record.discard;
        self.layers.push(record);

        self.save();
        let Some(image) = image else {
            if !discard {
                self.state_mut().alpha *= effects.opacity;
            }
            return discard;
        };
        self.set_render_target(RenderTarget::Image(image));
        self.clear_rect(0, 0, width as u32, height as u32, Color::rgbaf(0.0, 0.0, 0.0, 0.0));

        // Shift device space so the captured rect's origin lands on (0, 0),
        // exactly like the shadow pass maps its padded bbox. Shadow state is
        // a layer rendering attribute too: it applies to the layer's result
        // at end_layer, so it must not also apply to every draw inside, or
        // the layer's children would each cast their own shadow and then the
        // layer would cast one more over the lot.
        let mut layer_transform = Transform2D::translation(-minx, -miny);
        layer_transform.premultiply(&state.transform);
        self.enter_offscreen_state(layer_transform);
        true
    }

    /// Closes the innermost [`begin_layer`](Self::begin_layer) and composites
    /// the captured layer onto the previous target with the layer's declared
    /// effects, honoring the outer scissor and composite operation.
    ///
    /// The state is restored to what it was at `begin_layer`: a `save()`
    /// left open inside the layer is discarded with it, like Skia's
    /// `restoreToCount()`. With no layer open this does nothing - a
    /// `restore()` that reached the layer's entry has already closed it.
    pub fn end_layer(&mut self) {
        if self.pop_layer_past_depth_limit() {
            // A layer past the limit closes with the saves left open inside
            // it; nothing was drawn to composite.
            return;
        }
        let Some(mut record) = self.layers.pop() else {
            return;
        };
        self.restore_to_layer_boundary(&record);
        self.set_render_target(record.previous_target);
        let Some(image) = record.image else {
            return; // pass-through layer: nothing captured
        };

        if record.discard {
            self.release_layer_images(&record, image);
            return;
        }

        let alpha = record.outer_alpha * record.effects.opacity;
        if alpha <= 0.0 {
            self.refund_filter_work(record.reserved_filter_work);
            self.release_layer_images(&record, image);
            return;
        }

        // Run the filter chain, if any, through the images reserved for it at
        // begin_layer: the chain releases the scratches, the result goes back
        // with the composite. Orientation bookkeeping per the chain contract:
        // the capture holds flipped storage; the chain flips storage-parity
        // exactly once, so the filtered result is stored upright and must be
        // sampled WITHOUT the FLIP_Y flag the raw capture needs.
        let source = match record.filter_images.take() {
            Some(FilterImages { target, scratch }) => {
                let passes =
                    filter_passes(&record.effects.filters).expect("an admitted layer has a bounded filter plan");
                self.run_filter_passes(target, &passes, image, scratch, true);
                target
            }
            None => image,
        };

        let (minx, miny) = record.origin;

        // The mask applies after the filter chain - SVG's order for a group
        // carrying both - and multiplies the layer's alpha in place.
        if let (Some(mask), Some(images)) = (record.effects.mask, record.mask_images) {
            self.apply_layer_mask(source, &record, mask, images, source != image);
        }
        let tint = Color::rgbaf(1.0, 1.0, 1.0, alpha);
        let mut layer_paint =
            Paint::image_tint(source, minx, miny, record.width as f32, record.height as f32, 0.0, tint);
        layer_paint.set_anti_alias(false);

        // Composite in plain device space at the captured origin; the state
        // restored above supplies the outer scissor and composite operation.
        // The restore above put back the shadow state that was current at
        // begin_layer, and this composite deliberately runs under it: the
        // shadow pass builds coverage from the paint's real alpha, so the
        // layer image casts one shadow for the whole group, the way Canvas
        // 2D's beginLayer applies the shadow to the layer's result and SVG's
        // feDropShadow applies to a filtered group. Inside the layer the
        // shadow state was reset, so nothing has been shadowed twice.
        self.fill_device_rect(
            minx,
            miny,
            record.width as f32,
            record.height as f32,
            &layer_paint.flavor,
        );

        // The composite that reads the layer is recorded; its images can back
        // the next layer of this size.
        self.release_layer_images(&record, source);
    }

    /// Returns a finished layer's images to the transient pool: everything
    /// the record holds and, when different from the capture, its filtered
    /// result `source`. Every command that reads them has been recorded.
    fn release_layer_images(&mut self, record: &LayerRecord, source: ImageId) {
        for image in record.images() {
            self.release_transient_image(image);
        }
        if record.image != Some(source) {
            self.release_transient_image(source);
        }
    }

    /// Puts the current state into the shape every offscreen pass draws
    /// under: `transform` as the pass's device space, full alpha, no scissor,
    /// source-over and no shadow. The caller's `save()` holds the state this
    /// replaces.
    fn enter_offscreen_state(&mut self, transform: Transform2D) {
        let state = self.state_mut();
        state.transform = transform;
        state.alpha = 1.0;
        state.scissor = Scissor::default();
        state.composite_operation = CompositeOperationState::default();
        state.shadow_color = Color::rgbaf(0.0, 0.0, 0.0, 0.0);
        state.shadow_blur = 0.0;
        state.shadow_offset = [0.0, 0.0];
    }

    /// Fills a device-space rect with `paint` under the identity transform,
    /// at full alpha and without antialiasing: the blit an offscreen pass
    /// ends with. The scissor, composite operation and shadow state stay the
    /// caller's - that is how a layer's composite honors the outer scissor
    /// and casts the group's shadow.
    fn fill_device_rect(&mut self, x: f32, y: f32, width: f32, height: f32, paint: &PaintFlavor) {
        let saved_transform = self.state().transform;
        let saved_alpha = self.state().alpha;
        self.state_mut().transform = Transform2D::identity();
        self.state_mut().alpha = 1.0;
        let mut rect = Path::new();
        rect.rect(x, y, width, height);
        self.fill_path_internal(&rect, paint, false, FillRule::NonZero);
        self.state_mut().transform = saved_transform;
        self.state_mut().alpha = saved_alpha;
    }

    /// Acquires a mask's coverage transients for a layer store of
    /// `width` x `height`: the mask normalized into layer space, sampled
    /// upright through FLIP_Y like a capture, and for luminance masks the
    /// conversion target, whose color-matrix pass leaves storage upright.
    /// `None`, holding nothing, when the budget cannot fit them.
    fn reserve_mask_images(
        &mut self,
        width: usize,
        height: usize,
        kind: MaskKind,
        headroom: usize,
    ) -> Option<MaskImages> {
        let normalized = self
            .acquire_transient_image_reserving(width, height, ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y, headroom)
            .ok()?;
        let converted = match kind {
            MaskKind::Alpha => None,
            MaskKind::Luminance => {
                match self.acquire_transient_image_reserving(width, height, ImageFlags::PREMULTIPLIED, headroom) {
                    Ok(converted) => Some(converted),
                    Err(_) => {
                        self.rollback_transient_image(normalized);
                        return None;
                    }
                }
            }
        };
        Some(MaskImages { normalized, converted })
    }

    /// Acquires a filter chain's transients for a layer store of
    /// `width` x `height`: the result, which the chain stores upright and the
    /// composite samples without FLIP_Y, and the scratches its pass plan
    /// needs, sized like the capture the chain reads. `None`, holding
    /// nothing, when the budget cannot fit them.
    fn reserve_filter_images(
        &mut self,
        width: usize,
        height: usize,
        filters: &[ImageFilter],
        headroom: usize,
    ) -> Option<FilterImages> {
        let passes = filter_passes(filters)?;
        let target = self
            .acquire_transient_image_reserving(width, height, ImageFlags::PREMULTIPLIED, headroom)
            .ok()?;
        let needs_blur = passes
            .iter()
            .any(|filter| matches!(filter, ImageFilter::GaussianBlur { .. }));
        // The result and chain scratches share the same storage convention,
        // so a layer can alternate through its result and one scratch.
        match self.acquire_filter_scratches(width, height, passes.len(), needs_blur, 1, headroom) {
            Ok(scratch) => Some(FilterImages { target, scratch }),
            Err(_) => {
                self.rollback_transient_image(target);
                None
            }
        }
    }

    /// Multiplies `layer`'s alpha by the mask's coverage, in layer space,
    /// drawing through the coverage images reserved at `begin_layer`.
    ///
    /// Orientation: a draw into an image target lands in flipped storage, so
    /// draw-space row 0 writes the storage row holding a raw capture's top
    /// but a filtered result's bottom (the chain flipped storage parity
    /// once). Both coverage images sample upright - `normalized` through
    /// FLIP_Y like a capture, `converted` without it like a filtered result -
    /// so the coverage draw runs under the identity for a raw capture and
    /// under a vertical flip for a filtered one.
    fn apply_layer_mask(
        &mut self,
        layer: ImageId,
        record: &LayerRecord,
        mask: LayerMask,
        images: MaskImages,
        layer_is_filtered: bool,
    ) {
        let (width, height) = (record.width as f32, record.height as f32);
        // The mask rect is root device space and the store's (0, 0) sits at
        // the record's root origin - every enclosing capture's shift
        // included - not at the local origin the composite lands on.
        let (minx, miny) = record.root_origin;
        let previous_target = self.current_render_target;
        self.save();

        // Normalize the mask into layer space with an ordinary draw, so the
        // caller's storage convention (an upload or a render target, whatever
        // ImageFlags it carries) never enters the parity rule above. The
        // backdrop sets what uncovered pixels mean. A luminance mask lands
        // over opaque black: every pixel is then opaque, the color-matrix
        // pass's unpremultiply is the identity, and the luminance it writes
        // from the premultiplied color is luminance x alpha - SVG's mask
        // value - in one pass, with the uncovered rest reading black,
        // coverage 0. An alpha mask lands over transparent and its own alpha
        // is the coverage.
        let backdrop = match mask.kind {
            MaskKind::Luminance => Color::black(),
            MaskKind::Alpha => Color::rgbaf(0.0, 0.0, 0.0, 0.0),
        };
        self.set_render_target(RenderTarget::Image(images.normalized));
        self.clear_rect(0, 0, record.width as u32, record.height as u32, backdrop);
        self.enter_offscreen_state(Transform2D::identity());
        let mask_paint = Paint::image(
            mask.image,
            mask.x - minx,
            mask.y - miny,
            mask.width,
            mask.height,
            0.0,
            1.0,
        );
        self.fill_device_rect(
            mask.x - minx,
            mask.y - miny,
            mask.width,
            mask.height,
            &mask_paint.flavor,
        );

        let coverage = match images.converted {
            Some(converted) => {
                let _ = self.filter_image_with_scratch(
                    converted,
                    ImageFilter::luminance_to_alpha(),
                    images.normalized,
                    None,
                );
                converted
            }
            None => images.normalized,
        };

        // layer.alpha *= coverage.alpha over the whole store.
        self.set_render_target(RenderTarget::Image(layer));
        let transform = if layer_is_filtered {
            Transform2D::new(1.0, 0.0, 0.0, -1.0, 0.0, height)
        } else {
            Transform2D::identity()
        };
        self.enter_offscreen_state(transform);
        self.state_mut().composite_operation = CompositeOperationState::new(CompositeOperation::DestinationIn);
        let coverage_paint = Paint::image(coverage, 0.0, 0.0, width, height, 0.0, 1.0);
        let mut store = Path::new();
        store.rect(0.0, 0.0, width, height);
        self.fill_path_internal(&store, &coverage_paint.flavor, false, FillRule::NonZero);

        self.restore();
        self.set_render_target(previous_target);
    }

    /// Intersects the clip region with `path` under the current transform,
    /// with `fill_rule` as the clip-rule: Canvas 2D `clip()`, SVG `clip-path`
    /// and `clip-rule`. Drawing after this call is limited to the
    /// intersection of every clip taken on the current render target.
    ///
    /// Clip edges are not antialiased: unlike a fill or stroke edge, a pixel
    /// is either inside the clip or outside it.
    ///
    /// Clips are part of the saved state - [`restore`](Self::restore) drops
    /// the clips taken since the matching [`save`](Self::save) - and belong
    /// to the render target they were taken on: a clip on the canvas still
    /// applies when a layer opened on it is composited back, and a clip taken
    /// inside a layer or while rendering into an image gates only that layer
    /// or image.
    ///
    /// [`clear_rect`](Self::clear_rect) is not clipped; a Canvas 2D
    /// `clearRect` under a clip is a fill of the rect with an opaque paint
    /// under [`CompositeOperation::DestinationOut`].
    pub fn clip_path(&mut self, path: &Path, fill_rule: FillRule) {
        if self.saturated() {
            // Nothing draws past the depth limit, so nothing to gate; the
            // clip would only cost geometry.
            return;
        }
        self.reconcile_current_clip_plane();
        let target = self.current_render_target;
        let transform = self.state().transform;
        let (vertices, bounds) = {
            let path_cache = path.cache(&transform, self.tess_tol, self.dist_tol);
            (path_cache.winding_triangles(), path_cache.bounds)
        };
        let geometry = Rc::new(ClipGeometry {
            vertices: vertices.into_boxed_slice(),
        });
        let target_rect = self.render_target_rect();
        let path_rect = Self::clip_bounds(bounds, target_rect);
        let previous_armed = self.clip_planes.get(&target).map_or(target_rect, |plane| plane.armed);

        if !self.clip_active() {
            self.emit_clip_reset(true);
        }
        self.emit_clip_fill(&geometry, fill_rule, previous_armed);
        let armed = previous_armed.intersect(path_rect);
        self.clip_stack.push(ClipEntry {
            geometry,
            fill_rule,
            target,
            bounds,
            prior_armed: previous_armed,
            armed,
        });
        let plane = self.clip_planes.entry(target).or_insert(ClipPlaneState {
            count: 0,
            dirty: false,
            armed: target_rect,
        });
        plane.count += 1;
        plane.armed = armed;
        self.state_mut().clip_depth = self.clip_stack.len();
    }

    /// Re-establishes the current render target's stencil clip plane from
    /// the stack: disarmed when no clip on it survives, otherwise reset to
    /// visible and re-intersected with the survivors (a few stencil-only
    /// draws, no color work).
    fn replay_clip_stack(&mut self) {
        let target = self.current_render_target;
        let entries: Vec<_> = self
            .clip_stack
            .iter()
            .filter(|entry| entry.target == target)
            .map(|entry| (entry.geometry.clone(), entry.fill_rule, entry.bounds))
            .collect();
        if entries.is_empty() {
            self.emit_clip_reset(false);
            return;
        }
        self.emit_clip_reset(true);
        let mut previous_armed = self.render_target_rect();
        let target_rect = previous_armed;
        let mut armed_values = Vec::with_capacity(entries.len());
        for (geometry, fill_rule, bounds) in entries {
            self.emit_clip_fill(&geometry, fill_rule, previous_armed);
            let armed = previous_armed.intersect(Self::clip_bounds(bounds, target_rect));
            armed_values.push((previous_armed, armed));
            previous_armed = armed;
        }
        for (entry, (prior_armed, armed)) in self
            .clip_stack
            .iter_mut()
            .filter(|entry| entry.target == target)
            .zip(armed_values)
        {
            entry.prior_armed = prior_armed;
            entry.armed = armed;
        }
        if let Some(plane) = self.clip_planes.get_mut(&target) {
            plane.armed = previous_armed;
        }
    }

    fn render_target_rect(&self) -> Rect {
        let (width, height) = self.render_target_size();
        Rect::new(0.0, 0.0, width, height)
    }

    fn clip_bounds(bounds: Bounds, target: Rect) -> Rect {
        if ![bounds.minx, bounds.miny, bounds.maxx, bounds.maxy]
            .into_iter()
            .all(f32::is_finite)
            || bounds.minx > bounds.maxx
            || bounds.miny > bounds.maxy
        {
            return Rect::default();
        }
        Rect::new(
            bounds.minx - 1.0,
            bounds.miny - 1.0,
            bounds.maxx - bounds.minx + 2.0,
            bounds.maxy - bounds.miny + 2.0,
        )
        .intersect(target)
    }

    /// Pushes a triangle strip over the whole current render target and
    /// returns its vertex range: the stencil quads that arm, disarm and
    /// resolve the clip plane must reach every pixel of the target, which
    /// for a layer store is the store, not the canvas.
    fn push_target_quad(&mut self) -> (usize, usize) {
        self.push_clip_quad(self.render_target_rect())
    }

    fn push_clip_quad(&mut self, rect: Rect) -> (usize, usize) {
        let offset = self.verts.len();
        let x1 = rect.x + rect.w;
        let y1 = rect.y + rect.h;
        self.verts.push(Vertex::new(rect.x, y1, 0.5, 1.0));
        self.verts.push(Vertex::new(x1, y1, 0.5, 1.0));
        self.verts.push(Vertex::new(rect.x, rect.y, 0.5, 1.0));
        self.verts.push(Vertex::new(x1, rect.y, 0.5, 1.0));
        (offset, 4)
    }

    fn emit_clip_reset(&mut self, visible: bool) {
        let mut cmd = Command::new(CommandType::ClipReset { visible });
        cmd.triangles_verts = Some(self.push_target_quad());
        self.append_cmd(cmd);
    }

    fn emit_clip_fill(&mut self, geometry: &ClipGeometry, fill_rule: FillRule, resolve: Rect) {
        let mut cmd = Command::new(CommandType::ClipFill);
        cmd.fill_rule = fill_rule;

        let offset = self.verts.len();
        self.verts.extend_from_slice(&geometry.vertices);
        if !geometry.vertices.is_empty() {
            cmd.drawables.push(Drawable {
                fill_verts: Some((offset, geometry.vertices.len())),
                ..Drawable::default()
            });
        }

        cmd.triangles_verts = Some(self.push_clip_quad(resolve));
        self.append_cmd(cmd);
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

    /// Returns `true` when the current state would paint a visible drop shadow.
    ///
    /// Matching the Canvas spec, a shadow is only drawn when the shadow color is
    /// not fully transparent *and* at least one of the blur or offset components
    /// is non-zero. (An opaque shadow color with zero blur and zero offset would
    /// land exactly under the shape and contribute nothing, so the spec treats it
    /// as no shadow.) When this returns `false` the drawing entry points skip the
    /// whole offscreen shadow pass, so the common (no-shadow) case has zero added
    /// cost.
    fn shadow_enabled(&self) -> bool {
        let state = self.state();
        state.alpha > 0.0
            && state.shadow_color.a > 0.0
            && (state.shadow_blur != 0.0 || state.shadow_offset[0] != 0.0 || state.shadow_offset[1] != 0.0)
    }

    /// Returns `true` when the drop shadow for a shape with the given device-space
    /// `shape_bounds` could land on the render target once the (device-space)
    /// shadow offset and blur spread are taken into account.
    ///
    /// Drawing entry points must not cull a shadow on the shape's own bounds
    /// alone: a shape entirely off-screen can still cast a visible shadow when the
    /// offset and/or blur pull the shadow back onto the target. The blur spread is
    /// `ceil(3 * sigma)` (sigma = `shadowBlur / 2`), which covers >99.7% of the
    /// Gaussian — the same reach `render_shadow` uses to pad its offscreen image.
    fn shadow_could_be_visible(&self, shape_bounds: Bounds) -> bool {
        let state = self.state();
        let [ox, oy] = state.shadow_offset;
        let spread = (state.shadow_blur / 2.0 * 3.0).ceil();
        let minx = shape_bounds.minx + ox - spread;
        let miny = shape_bounds.miny + oy - spread;
        let maxx = shape_bounds.maxx + ox + spread;
        let maxy = shape_bounds.maxy + oy + spread;
        maxx >= 0.0 && minx <= self.width() as f32 && maxy >= 0.0 && miny <= self.height() as f32
    }

    /// Renders a drop shadow for a shape whose device-space bounding box is
    /// `shape_bounds`, using the supplied closure to draw the shape's coverage.
    ///
    /// Per the Canvas drawing model the shadow is built from the *alpha of the
    /// actually-rendered source*, not from a forced-opaque tint: a semi-transparent
    /// fill, or a gradient/image with transparent texels, must cast a
    /// correspondingly weaker shadow. To achieve this the closure draws the real
    /// source (its actual paint and per-pixel alpha) into a transient offscreen
    /// image, then a solid `shadowColor` is composited over it with
    /// `CompositeOperation::SourceIn`, which masks the shadow color by the source's
    /// alpha. The result carries `shadowColor.rgb` with alpha
    /// `source.alpha * shadowColor.a` per pixel. That image is then
    /// Gaussian-blurred (standard deviation `shadowBlur / 2`) through the
    /// chain planner's split ([`blur_passes`]): a sigma above the shader's
    /// per-pass bound runs as the passes that compose to it, ping-ponging
    /// between the coverage image and the blurred one, and finally composited
    /// back into the current render target, translated by the device-space
    /// shadow offset and drawn *under* the actual shape. The current scissor,
    /// global alpha and composite operation are honored when compositing.
    ///
    /// `draw_coverage` is expected to issue the shape's normal draw command(s) with
    /// its real paint; the canvas transform in effect during the call already maps
    /// the shape's device-space coordinates into the offscreen image.
    fn render_shadow(&mut self, shape_bounds: Bounds, draw_coverage: impl FnOnce(&mut Self)) {
        // Degenerate / off-screen bounds: nothing to cast a shadow from.
        if shape_bounds.maxx <= shape_bounds.minx || shape_bounds.maxy <= shape_bounds.miny {
            return;
        }

        let state = *self.state();
        let shadow_color = state.shadow_color;

        // Standard deviation in device pixels (HTML drawing model: sigma = blur/2).
        let sigma = state.shadow_blur / 2.0;

        // The shadow offset is expressed in output (device) pixels and, per the
        // Canvas spec, is NOT affected by the current transformation matrix: it
        // keeps the same magnitude and direction relative to the shape under any
        // scale or rotation. Apply the raw components directly when positioning
        // the blurred shadow (matching WebKit: "canvas shadows must not be
        // affected by any transformation and keep the same offset relative to the
        // shape").
        let [txx, txy] = state.shadow_offset;

        // Pad the offscreen image for the blur kernel reach (~3 sigma covers
        // >99.7% of the Gaussian) plus a fringe pixel for antialiased edges.
        // The reach is the true sigma's: the blur below runs as as many
        // passes as it takes to compose to it.
        let reach = chain_blur_sigma(sigma).unwrap_or(0.0);
        let pad = (reach * 3.0).ceil() + 2.0;
        // Bounded like a layer's: a shadow whose padded coverage would pass
        // the texture limit keeps its coverage and loses reach at the edge.
        let pad = bounded_pad(
            pad,
            (shape_bounds.maxx - shape_bounds.minx).max(shape_bounds.maxy - shape_bounds.miny),
            self.renderer.max_texture_size(),
            transient::SHADOW_GRANULARITY,
        );

        // Coverage is rendered at the shape's own location; the offset is applied
        // later when compositing, so the offscreen only needs to bound the shape.
        let minx = (shape_bounds.minx - pad).floor();
        let miny = (shape_bounds.miny - pad).floor();
        let maxx = (shape_bounds.maxx + pad).ceil();
        let maxy = (shape_bounds.maxy + pad).ceil();

        let width = transient::round_up((maxx - minx) as usize, transient::SHADOW_GRANULARITY);
        let height = transient::round_up((maxy - miny) as usize, transient::SHADOW_GRANULARITY);

        // Guard against absurd allocations (e.g. enormous blur on a huge shape)
        // and the backend's texture limit.
        let limit = self.renderer.max_texture_size();
        if width == 0 || height == 0 || width > limit || height > limit {
            return;
        }

        let blur_plan = (sigma >= 0.01).then(|| blur_passes(sigma));
        let work = blur_plan.map_or(0, |(passes, pass_sigma)| {
            filter_work(
                std::slice::from_ref(&ImageFilter::GaussianBlur { sigma: pass_sigma }),
                width,
                height,
            )
            .saturating_mul(passes as u64)
        });
        if !self.reserve_filter_work(work) {
            return;
        }

        // Offscreen render targets store premultiplied-alpha results, so flag the
        // images as PREMULTIPLIED. Otherwise the image-sampling shader would
        // re-premultiply on composite (multiplying rgb by alpha a second time),
        // darkening partially-transparent shadow texels — which the source-alpha
        // shadow now produces wherever the source is semi-transparent or
        // antialiased.
        //
        // Image render targets store their content vertically flipped in texture
        // space: both backends keep the GL FBO convention where canvas y = 0 lands
        // on the *last* texture row (the wgpu backend's texture-target vertex
        // stage reproduces it deliberately, and the glyph atlas pre-flips its
        // rasterization coordinates to compensate). FLIP_Y declares that
        // orientation so the composite below samples the coverage upright;
        // without it the shadow is mirrored about its rect's horizontal midline.
        // The Gaussian blur is unaffected: each of its two passes flips once, so
        // the blurred image keeps the coverage image's orientation.
        let image_flags = ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y;
        // Both come from the transient pool: past the budget the shadow is
        // skipped rather than allocated, like a layer degrading.
        let Ok(coverage_image) = self.acquire_transient_image(width, height, image_flags) else {
            self.refund_filter_work(work);
            return;
        };
        // The blur kernel divides by sigma, so a zero (or sub-pixel) blur skips
        // the filter pass entirely — and with it the second offscreen image.
        let (blurred_image, blur_scratch) = if sigma >= 0.01 {
            match self.acquire_transient_image(width, height, image_flags) {
                Ok(image) => match self.acquire_transient_image(width, height, ImageFlags::PREMULTIPLIED) {
                    Ok(scratch) => (Some(image), Some(scratch)),
                    Err(_) => {
                        self.rollback_transient_image(image);
                        self.rollback_transient_image(coverage_image);
                        self.refund_filter_work(work);
                        return;
                    }
                },
                Err(_) => {
                    self.rollback_transient_image(coverage_image);
                    self.refund_filter_work(work);
                    return;
                }
            }
        } else {
            (None, None)
        };

        let previous_target = self.current_render_target;

        // Draw the *real* source (its actual paint and per-pixel alpha) into the
        // offscreen image, then recolor it by the shadow color while preserving the
        // source alpha. The image space is the device space translated so the
        // padded bbox origin maps to (0, 0): pre-translate the CTM by (-minx, -miny).
        self.save();
        self.set_render_target(RenderTarget::Image(coverage_image));
        self.clear_rect(0, 0, width as u32, height as u32, Color::rgbaf(0.0, 0.0, 0.0, 0.0));

        // Build the offset transform for coverage rendering: original CTM with an
        // extra device-space translation that shifts the shape into the offscreen.
        // Render the source at full strength: the shadow color's alpha and the
        // global alpha are applied later (the former via the SourceIn mask below,
        // the latter when compositing the finished shadow under the shape).
        let mut coverage_transform = Transform2D::translation(-minx, -miny);
        coverage_transform.premultiply(&state.transform);
        self.enter_offscreen_state(coverage_transform);

        // 1. Rasterize the source with its real paint so the offscreen holds the
        //    source's true per-pixel alpha (semi-transparent fills, gradient/image
        //    transparency, antialiased edges, ...).
        draw_coverage(self);

        // 2. Recolor by the shadow color, masked by the source alpha. SourceIn
        //    keeps `shadowColor * dst.alpha`, so the offscreen ends up carrying
        //    shadowColor.rgb with per-pixel alpha = source.alpha * shadowColor.a.
        //    Where the source was transparent the shadow stays transparent, so a
        //    fully transparent source casts no shadow and a 50%-alpha source casts
        //    a half-strength shadow. The mask must cover the whole offscreen in its
        //    own pixel space, so draw it with the identity transform (not the
        //    shape's coverage transform, which is scaled/translated).
        self.state_mut().composite_operation = CompositeOperationState::new(CompositeOperation::SourceIn);
        self.fill_device_rect(0.0, 0.0, width as f32, height as f32, &PaintFlavor::Color(shadow_color));

        self.restore();

        // Blur the coverage into the second offscreen image; a sharp shadow (no
        // blur image allocated) composites the coverage directly. A sigma above
        // the shader's per-pass bound is the planner's k passes of sigma /
        // sqrt(k), ping-ponging between the two images (each pass reads one
        // and writes the other through the reserved horizontal scratch),
        // so the result sits in the blurred image after an odd count and back
        // in the coverage image after an even one.
        let source_image = if let Some(blurred_image) = blurred_image {
            let (passes, pass_sigma) = blur_plan.expect("a blurred image has a blur plan");
            let mut src = coverage_image;
            let mut dst = blurred_image;
            for _ in 0..passes {
                let _ = self.filter_image_with_scratch(
                    dst,
                    ImageFilter::GaussianBlur { sigma: pass_sigma },
                    src,
                    blur_scratch,
                );
                std::mem::swap(&mut src, &mut dst);
            }
            src
        } else {
            coverage_image
        };

        // Composite the shadow back into the original target, offset by the
        // device-space shadow offset and drawn under the shape. The shadow color's
        // alpha is already baked into the image (via the SourceIn mask); only the
        // global alpha is folded into the image tint here.
        self.set_render_target(previous_target);

        let dst_x = minx + txx;
        let dst_y = miny + txy;

        // The shadow image already carries shadowColor.rgb and per-pixel alpha
        // `source.alpha * shadowColor.a` (baked in by the SourceIn mask above), so
        // here we only fold in the current global alpha.
        let tint = Color::rgbaf(1.0, 1.0, 1.0, state.alpha);
        let mut shadow_paint = Paint::image_tint(source_image, dst_x, dst_y, width as f32, height as f32, 0.0, tint);
        shadow_paint.set_anti_alias(false);

        // Composite in plain device space (identity transform) at the offset
        // position, honoring the caller's scissor and composite operation.
        // A shadow casts no shadow of its own: mute the shadow state around
        // the blit.
        self.state_mut().shadow_color = Color::rgbaf(0.0, 0.0, 0.0, 0.0);
        self.fill_device_rect(dst_x, dst_y, width as f32, height as f32, &shadow_paint.flavor);
        self.state_mut().shadow_color = shadow_color;

        // The composite that reads them is recorded; the next shadow of this
        // size draws into the same images.
        self.release_transient_image(coverage_image);
        if let Some(blurred_image) = blurred_image {
            self.release_transient_image(blurred_image);
        }
        if let Some(blur_scratch) = blur_scratch {
            self.release_transient_image(blur_scratch);
        }
    }

    /// Deletes the frame's transient images, except those of layers still
    /// open across the flush: a layer's draws so far already live in its
    /// capture and the ones still to come must land in the same image, and
    /// its effects draw at `end_layer` through the images reserved for them
    /// with it.
    fn release_transient_images(&mut self) {
        let held: Vec<ImageId> = self.layers.iter().flat_map(LayerRecord::images).collect();
        self.transients.release_all(&mut self.images, &mut self.renderer, &held);
    }

    fn release_pending_images(&mut self) {
        let held_masks: HashSet<ImageId> = self
            .layers
            .iter()
            .filter_map(|layer| layer.effects.mask.map(|mask| mask.image))
            .collect();
        let releasable: Vec<ImageId> = self
            .pending_image_deletions
            .iter()
            .filter(|id| !held_masks.contains(id))
            .copied()
            .collect();
        for id in releasable {
            self.pending_image_deletions.remove(&id);
            self.images.remove(&mut self.renderer, id);
        }
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

/// `clear_rect` clears the whole stencil unless a clip is armed on the
/// target it clears, when only the winding bits may go: the command carries
/// that decision so a tiler takes its tile clear whenever it can.
#[test]
fn clear_rect_keeps_the_clip_plane_only_while_a_clip_is_armed() {
    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let keep_clips = |canvas: &mut Canvas<RecordingRenderer>| -> Vec<bool> {
        canvas.flush_to_output(());
        let commands = recorded_commands.borrow();
        commands
            .iter()
            .filter_map(|cmd| match cmd.cmd_type {
                CommandType::ClearRect { keep_clip, .. } => Some(keep_clip),
                _ => None,
            })
            .collect()
    };

    canvas.clear_rect(0, 0, 100, 100, Color::white());
    assert_eq!(
        keep_clips(&mut canvas),
        vec![false],
        "no clip: the whole stencil is cleared"
    );

    let mut clip = Path::new();
    clip.rect(10.0, 10.0, 50.0, 50.0);
    canvas.save();
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.clear_rect(0, 0, 100, 100, Color::white());
    assert_eq!(
        keep_clips(&mut canvas),
        vec![true],
        "a clip on the screen survives the clear"
    );

    let image = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(image));
    canvas.clear_rect(0, 0, 64, 64, Color::white());
    assert_eq!(
        keep_clips(&mut canvas),
        vec![false],
        "the screen's clip does not gate an image target"
    );
    canvas.set_render_target(RenderTarget::Screen);
    canvas.restore();

    canvas.clear_rect(0, 0, 100, 100, Color::white());
    assert_eq!(
        keep_clips(&mut canvas),
        vec![false],
        "the clip is popped: the whole stencil is cleared again"
    );
}

#[test]
fn consecutive_clip_restores_replay_once_before_the_next_draw() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    let mut clip = Path::new();
    clip.rect(10.0, 10.0, 80.0, 80.0);

    for _ in 0..64 {
        canvas.save();
        canvas.clip_path(&clip, FillRule::NonZero);
    }
    canvas.flush_to_output(());
    for _ in 0..64 {
        canvas.restore();
    }
    assert!(canvas.commands.is_empty());

    canvas.fill_path(&clip, &Paint::color(Color::white()));
    assert_eq!(
        canvas
            .commands
            .iter()
            .filter(|command| matches!(command.cmd_type, CommandType::ClipReset { visible: false }))
            .count(),
        1
    );
    assert!(!canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::ClipFill)));
}

#[test]
fn same_size_set_size_does_not_replay_a_clip() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    let mut clip = Path::new();
    clip.rect(10.0, 10.0, 80.0, 80.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.flush_to_output(());

    canvas.set_size(100, 100, 1.0);
    assert!(!canvas
        .commands
        .iter()
        .any(|command| { matches!(command.cmd_type, CommandType::ClipFill | CommandType::ClipReset { .. }) }));

    canvas.set_size(120, 120, 1.0);
    assert!(!canvas
        .commands
        .iter()
        .any(|command| { matches!(command.cmd_type, CommandType::ClipFill | CommandType::ClipReset { .. }) }));
    canvas.fill_path(&clip, &Paint::color(Color::white()));
    assert_eq!(
        canvas
            .commands
            .iter()
            .filter(|command| matches!(command.cmd_type, CommandType::ClipFill))
            .count(),
        1
    );
}

#[test]
fn nested_clip_resolve_is_bounded_by_the_outer_clip() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    let mut outer = Path::new();
    outer.rect(10.0, 20.0, 30.0, 40.0);
    let mut inner = Path::new();
    inner.rect(15.0, 25.0, 10.0, 10.0);
    canvas.clip_path(&outer, FillRule::NonZero);
    canvas.clip_path(&inner, FillRule::NonZero);

    let (resolve_start, resolve_len) = canvas
        .commands
        .iter()
        .filter(|command| matches!(command.cmd_type, CommandType::ClipFill))
        .nth(1)
        .and_then(|command| command.triangles_verts)
        .unwrap();
    assert_eq!(resolve_len, 4);
    let resolve = &canvas.verts[resolve_start..resolve_start + resolve_len];
    assert!(resolve.iter().all(|vertex| (9.0..=41.0).contains(&vertex.x)));
    assert!(resolve.iter().all(|vertex| (19.0..=61.0).contains(&vertex.y)));
}

#[test]
fn many_contour_clip_uses_one_winding_drawable() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    let mut clip = Path::new();
    for inset in 0..256 {
        let inset = inset as f32 * 0.01;
        clip.rect(inset, inset, 10.0, 10.0);
    }

    canvas.clip_path(&clip, FillRule::NonZero);

    let command = canvas
        .commands
        .iter()
        .find(|command| matches!(command.cmd_type, CommandType::ClipFill))
        .unwrap();
    assert_eq!(command.drawables.len(), 1);
    assert_eq!(command.drawables[0].fill_verts.map(|(_, len)| len), Some(256 * 6));
}

#[test]
fn a_deleted_image_cannot_be_reselected_after_flush() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let image = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(image));
    canvas.delete_image(image);
    canvas.flush_to_output(());

    canvas.set_render_target(RenderTarget::Image(image));
    assert_eq!(canvas.current_render_target, RenderTarget::Screen);
    assert!(canvas.image_info(image).is_err());
}

#[test]
fn an_open_layer_keeps_a_pending_mask_alive_through_its_composite() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let effects = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&effects));
    canvas.delete_image(mask);
    canvas.flush_to_output(());
    assert!(canvas.images.info(mask).is_some());
    assert!(canvas.pending_image_deletions.contains(&mask));

    canvas.end_layer();
    assert!(canvas.images.info(mask).is_some());
    canvas.flush_to_output(());
    assert!(canvas.images.info(mask).is_none());
    assert!(!canvas.pending_image_deletions.contains(&mask));
}

#[test]
fn reallocating_an_image_marks_its_clip_plane_for_replay() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let image = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(image));
    let mut clip = Path::new();
    clip.rect(0.0, 0.0, 16.0, 32.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.flush_to_output(());

    canvas
        .realloc_image(image, 48, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    assert_eq!(canvas.renderer.image_deletion_count, 1);
    assert!(canvas.clip_planes[&RenderTarget::Image(image)].dirty);
    canvas.fill_path(&clip, &Paint::color(Color::white()));
    assert!(!canvas.clip_planes[&RenderTarget::Image(image)].dirty);
    assert!(canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::ClipFill)));
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
fn filtering_an_image_marks_its_clip_plane_for_replay() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let source = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let target = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(target));
    let mut clip = Path::new();
    clip.rect(0.0, 0.0, 16.0, 32.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.flush_to_output(());

    canvas.filter_image(target, ImageFilter::identity(), source);
    assert!(canvas.clip_planes[&RenderTarget::Image(target)].dirty);
    canvas.fill_path(&clip, &Paint::color(Color::white()));

    let filter = canvas
        .commands
        .iter()
        .position(|command| matches!(command.cmd_type, CommandType::RenderFilteredImage { .. }))
        .unwrap();
    let replay = canvas
        .commands
        .iter()
        .rposition(|command| matches!(command.cmd_type, CommandType::ClipFill))
        .unwrap();
    assert!(replay > filter);
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

/// A rounded scissor set before `begin_layer` is not applied to the draws
/// inside the layer - padded store or not - and clips the composite once,
/// where it was set: the draw inside records no scissor, and the composite
/// carries the scissor centered at the root center, extent and radius as set.
#[test]
fn a_rounded_scissor_clips_a_layer_once_at_its_composite() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);

    // Sigma 2 pads the store by ceil(3 * 2) + 2 = 8 px per side; no filter
    // leaves the store at the root origin. Neither may clip the inside.
    let blur = LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 2.0 }]);
    let plain = LayerEffects::new();
    for (effects, origin) in [(&blur, (-8.0, -8.0)), (&plain, (0.0, 0.0))] {
        canvas.save();
        canvas.rounded_scissor(10.0, 10.0, 40.0, 20.0, 5.0); // centered on root (30, 20)
        assert!(canvas.begin_layer(effects));
        assert_eq!(canvas.layers.last().unwrap().origin, origin);
        fill_rect_with_current_scissor(&mut canvas);
        {
            let commands = recorded_commands.borrow();
            let params = first_draw_params(&commands);
            assert_eq!(
                params.scissor_ext,
                [1.0, 1.0],
                "no scissor inside the layer, store origin {origin:?}"
            );
        }
        canvas.end_layer();
        canvas.flush_to_output(());
        {
            let commands = recorded_commands.borrow();
            let params = first_draw_params(&commands);
            let expected = Transform2D::translation(30.0, 20.0).inverse().to_mat3x4();
            assert_eq!(
                params.scissor_mat, expected,
                "the composite is clipped where the scissor was set"
            );
            assert_eq!(params.scissor_ext, [20.0, 10.0]);
            assert_approx_eq(params.scissor_radius, 5.0);
        }
        canvas.restore();
    }
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

/// The Canvas 2D shadow attributes must start at their spec-mandated defaults:
/// a fully transparent shadow color, zero blur and zero offset.
#[test]
fn shadow_attribute_defaults_match_spec() {
    let canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    let state = canvas.state();

    assert_eq!(state.shadow_color, Color::rgbaf(0.0, 0.0, 0.0, 0.0));
    assert_eq!(state.shadow_blur, 0.0);
    assert_eq!(state.shadow_offset, [0.0, 0.0]);
    // Transparent shadow color disables shadows entirely.
    assert!(!canvas.shadow_enabled());

    // Per the enable rule, even an opaque shadow color stays disabled while blur
    // and offset are both zero (the shadow would land exactly under the shape).
    let mut canvas = canvas;
    canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
    assert!(
        !canvas.shadow_enabled(),
        "opaque color alone (zero blur, zero offset) must not enable a shadow"
    );
    canvas.set_shadow_offset(1.0, 0.0);
    assert!(
        canvas.shadow_enabled(),
        "a non-zero offset with an opaque color must enable the shadow"
    );
}

/// `set_shadow_blur` ignores negative and non-finite values, matching the Canvas
/// spec ("on setting, if the value is negative, infinite, or NaN, it must be
/// ignored").
#[test]
fn shadow_blur_rejects_invalid_values() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();

    canvas.set_shadow_blur(4.0);
    assert_eq!(canvas.state().shadow_blur, 4.0);

    canvas.set_shadow_blur(-1.0);
    assert_eq!(canvas.state().shadow_blur, 4.0, "negative blur must be ignored");

    canvas.set_shadow_blur(f32::NAN);
    assert_eq!(canvas.state().shadow_blur, 4.0, "NaN blur must be ignored");

    canvas.set_shadow_blur(f32::INFINITY);
    assert_eq!(canvas.state().shadow_blur, 4.0, "infinite blur must be ignored");
}

/// `set_shadow_offset` ignores non-finite values, preserving the previous offset.
/// This matches the Canvas setter semantics already used by `set_shadow_blur` and
/// keeps NaN/inf out of the offscreen geometry.
#[test]
fn shadow_offset_rejects_non_finite_values() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();

    canvas.set_shadow_offset(10.0, -5.0);
    assert_eq!(canvas.state().shadow_offset, [10.0, -5.0]);

    canvas.set_shadow_offset(f32::NAN, 7.0);
    assert_eq!(
        canvas.state().shadow_offset,
        [10.0, -5.0],
        "NaN x must be ignored, previous offset preserved"
    );

    canvas.set_shadow_offset(3.0, f32::INFINITY);
    assert_eq!(
        canvas.state().shadow_offset,
        [10.0, -5.0],
        "infinite y must be ignored, previous offset preserved"
    );

    canvas.set_shadow_offset(f32::NEG_INFINITY, f32::NAN);
    assert_eq!(
        canvas.state().shadow_offset,
        [10.0, -5.0],
        "non-finite components must be ignored, previous offset preserved"
    );

    // A subsequent finite update still applies.
    canvas.set_shadow_offset(2.0, 4.0);
    assert_eq!(canvas.state().shadow_offset, [2.0, 4.0]);
}

/// Per the Canvas spec a shadow is painted only when the shadow color is
/// non-transparent AND at least one of blur, offsetX or offsetY is non-zero. An
/// opaque shadow color with zero blur and zero offset must therefore emit no
/// offscreen shadow pass; flipping on a non-zero offset *or* a non-zero blur must
/// re-enable it.
#[test]
fn shadow_enable_rule_requires_blur_or_offset() {
    use renderer::CommandType;

    let run = |configure: &dyn Fn(&mut Canvas<RecordingRenderer>)| -> bool {
        let renderer = RecordingRenderer::default();
        let recorded = renderer.last_commands.clone();
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(100, 100, 1.0);
        configure(&mut canvas);

        let mut path = Path::new();
        path.rect(10.0, 10.0, 30.0, 30.0);
        canvas.fill_path(&path, &Paint::color(Color::rgb(255, 0, 0)));
        canvas.flush_to_output(());

        let commands = recorded.borrow();
        commands
            .iter()
            .any(|c| matches!(c.cmd_type, CommandType::SetRenderTarget(RenderTarget::Image(_))))
    };

    // Opaque color, zero blur, zero offset: no shadow.
    assert!(
        !run(&|canvas| {
            canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
            canvas.set_shadow_blur(0.0);
            canvas.set_shadow_offset(0.0, 0.0);
        }),
        "opaque color with zero blur and zero offset must not emit a shadow pass"
    );

    // A non-zero offsetX re-enables the shadow.
    assert!(
        run(&|canvas| {
            canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
            canvas.set_shadow_offset(5.0, 0.0);
        }),
        "a non-zero offset must re-enable the shadow"
    );

    // A non-zero offsetY re-enables the shadow.
    assert!(
        run(&|canvas| {
            canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
            canvas.set_shadow_offset(0.0, 5.0);
        }),
        "a non-zero offsetY must re-enable the shadow"
    );

    // A non-zero blur re-enables the shadow.
    assert!(
        run(&|canvas| {
            canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
            canvas.set_shadow_blur(4.0);
        }),
        "a non-zero blur must re-enable the shadow"
    );

    // Non-zero blur/offset but transparent color stays disabled.
    assert!(
        !run(&|canvas| {
            canvas.set_shadow_color(Color::rgba(0, 0, 0, 0));
            canvas.set_shadow_blur(4.0);
            canvas.set_shadow_offset(5.0, 5.0);
        }),
        "transparent shadow color must keep the shadow disabled"
    );
}

/// Shadow attributes are part of the drawing state and must be stacked by
/// save()/restore() like every other state member.
#[test]
fn shadow_state_is_saved_and_restored() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();

    canvas.set_shadow_color(Color::rgba(10, 20, 30, 40));
    canvas.set_shadow_blur(5.0);
    canvas.set_shadow_offset(3.0, 7.0);

    canvas.save();
    canvas.set_shadow_color(Color::rgba(99, 99, 99, 99));
    canvas.set_shadow_blur(11.0);
    canvas.set_shadow_offset(-1.0, -2.0);
    assert_eq!(canvas.state().shadow_blur, 11.0);
    canvas.restore();

    assert_eq!(canvas.state().shadow_color, Color::rgba(10, 20, 30, 40));
    assert_eq!(canvas.state().shadow_blur, 5.0);
    assert_eq!(canvas.state().shadow_offset, [3.0, 7.0]);
}

/// With a transparent shadow color (the default), filling a path must NOT emit
/// any offscreen shadow work: no SetRenderTarget and no RenderFilteredImage
/// commands, just the plain fill. This guards the "zero added overhead" rule.
#[test]
fn transparent_shadow_emits_no_offscreen_work() {
    use renderer::CommandType;

    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);

    // Shadow color left at its transparent default.
    canvas.set_shadow_blur(10.0);
    canvas.set_shadow_offset(5.0, 5.0);

    let mut path = Path::new();
    path.rect(10.0, 10.0, 30.0, 30.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(255, 0, 0)));
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c.cmd_type, CommandType::RenderFilteredImage { .. })),
        "transparent shadow must not run the blur filter"
    );
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c.cmd_type, CommandType::SetRenderTarget(RenderTarget::Image(_)))),
        "transparent shadow must not allocate an offscreen render target"
    );
}

/// A filter pass draws into its own target image, never into the target a
/// clip gates, so it must be recorded ungated under an active clip: a backend
/// that stencil-tests it (OpenGL) would otherwise test the filter quad against
/// the target image's blank plane and drop the whole pass - every layer
/// filter and every blurred shadow drawn under a clip.
#[test]
fn filter_passes_are_not_gated_by_the_active_clip() {
    use renderer::CommandType;

    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let source = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let target = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();

    let mut clip = Path::new();
    clip.rect(0.0, 0.0, 50.0, 50.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.filter_image(target, ImageFilter::GaussianBlur { sigma: 2.0 }, source);
    let mut path = Path::new();
    path.rect(10.0, 10.0, 30.0, 30.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(255, 0, 0)));
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    let filter = commands
        .iter()
        .find(|c| matches!(c.cmd_type, CommandType::RenderFilteredImage { .. }))
        .expect("the filter pass is recorded");
    assert!(!filter.clip_active, "a filter pass is not gated by the clip");
    let fill = commands
        .iter()
        .find(|c| matches!(c.cmd_type, CommandType::ConvexFill { .. }))
        .expect("the fill is recorded");
    assert!(fill.clip_active, "the fill under the clip is gated");
}

/// With an opaque shadow color and a non-zero blur, filling a path must emit the
/// offscreen shadow pass: render the coverage into an image target and run the
/// Gaussian blur filter before the final fill.
#[test]
fn opaque_shadow_emits_offscreen_blur_pass() {
    use renderer::CommandType;

    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);

    canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
    canvas.set_shadow_blur(6.0);
    canvas.set_shadow_offset(4.0, 4.0);

    let mut path = Path::new();
    path.rect(10.0, 10.0, 30.0, 30.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(255, 0, 0)));
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    let filtered = commands.iter().find_map(|c| match c.cmd_type {
        CommandType::RenderFilteredImage { filter, .. } => Some(filter),
        _ => None,
    });

    match filtered {
        Some(ImageFilter::GaussianBlur { sigma }) => {
            // HTML drawing model: sigma == shadowBlur / 2.
            assert!(
                (sigma - 3.0).abs() < 1e-4,
                "expected sigma 3.0 for blur 6.0, got {sigma}"
            );
        }
        Some(other) => panic!("opaque shadow must run the Gaussian blur filter, got {other:?}"),
        None => panic!("opaque shadow must run the Gaussian blur filter"),
    }

    assert!(
        commands
            .iter()
            .any(|c| matches!(c.cmd_type, CommandType::SetRenderTarget(RenderTarget::Image(_)))),
        "opaque shadow must render coverage into an offscreen image target"
    );
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

#[test]
fn an_unrepresentably_large_shadow_is_skipped_without_overflow() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    canvas.set_shadow_color(Color::black());
    canvas.set_shadow_blur(1.0);
    let extent = f32::MAX / 2.0;
    canvas.render_shadow(
        Bounds {
            minx: -extent,
            miny: 0.0,
            maxx: extent,
            maxy: 1.0,
        },
        |_| panic!("an oversized shadow must be rejected before drawing"),
    );
    assert!(canvas.transients.images.is_empty());
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

/// Turbulence lattices are cached per seed with a fixed capacity: a repeated
/// seed reuses its texture, the least recently used seed is queued for
/// deletion (so a command recorded against it still runs before the delete),
/// and a flush deletes only the evicted ones.
#[test]
fn turbulence_lattice_cache_is_bounded() {
    use crate::{ImageFilter, TurbulenceKind};
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let src = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let dst = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::FLIP_Y)
        .unwrap();
    let noise = |seed: i32| ImageFilter::Turbulence {
        base_frequency: [0.1, 0.1],
        num_octaves: 2,
        seed,
        stitch_tiles: false,
        kind: TurbulenceKind::FractalNoise,
        transform: Transform2D::identity(),
    };

    for seed in 1..=turbulence::LATTICE_CACHE_CAPACITY as i32 {
        canvas.filter_image(dst, noise(seed), src);
    }
    assert_eq!(canvas.turbulence_lattices.len(), turbulence::LATTICE_CACHE_CAPACITY);
    assert!(canvas.pending_image_deletions.is_empty());

    // A cache hit reorders, allocating nothing.
    canvas.filter_image(dst, noise(1), src);
    assert_eq!(canvas.turbulence_lattices.len(), turbulence::LATTICE_CACHE_CAPACITY);
    assert_eq!(canvas.turbulence_lattices.last().unwrap().0, 1);
    assert!(canvas.pending_image_deletions.is_empty());

    // One past capacity evicts the least recently used seed (2, since 1 was
    // just touched) after the next flush, not straight from the image store.
    let evicted = canvas.turbulence_lattices[0].1;
    canvas.filter_image(dst, noise(99), src);
    assert_eq!(canvas.turbulence_lattices.len(), turbulence::LATTICE_CACHE_CAPACITY);
    assert!(canvas.turbulence_lattices.iter().all(|(s, _)| *s != 2));
    assert!(canvas.pending_image_deletions.contains(&evicted));
    assert!(
        canvas.images.info(evicted).is_some(),
        "evicted lattice stays alive until flush"
    );

    canvas.flush_to_output(());
    assert!(canvas.pending_image_deletions.is_empty());
    assert!(canvas.images.info(evicted).is_none());
    assert_eq!(canvas.turbulence_lattices.len(), turbulence::LATTICE_CACHE_CAPACITY);

    for seed in 100..109 {
        canvas.filter_image(dst, noise(seed), src);
    }
    let lattice_ids: HashSet<_> = canvas
        .commands
        .iter()
        .filter(|command| matches!(command.cmd_type, CommandType::RenderFilteredImage { .. }))
        .filter_map(|command| command.image)
        .collect();
    assert_eq!(lattice_ids.len(), 9);
}

/// Layer backing stores are bounded by the scissor rect plus declared blur
/// reach - the memory rule that keeps nested layers from allocating
/// full-canvas images. Pass-through degradation keeps begin/end balanced.
#[test]
fn layer_bounds_follow_the_scissor() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(800, 600, 1.0);

    // Unscissored: the layer spans the canvas.
    assert!(canvas.begin_layer(&LayerEffects::new()));
    let record = canvas.layers.last().unwrap();
    // Stores round up to 64 px per axis so siblings share them (see TRANSIENT_GRANULARITY).
    assert_eq!((record.width, record.height), (832, 640));
    canvas.end_layer();

    // A rect scissor bounds the layer to its size.
    canvas.save();
    canvas.scissor(100.0, 50.0, 120.0, 80.0);
    assert!(canvas.begin_layer(&LayerEffects::new()));
    let record = canvas.layers.last().unwrap();
    assert_eq!((record.width, record.height), (128, 128));
    canvas.end_layer();

    // Declaring a blur pads the store by the kernel reach (3*sigma + 2).
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 4.0 }])));
    let record = canvas.layers.last().unwrap();
    assert_eq!((record.width, record.height), (192, 128)); // 148 x 108 before rounding
    canvas.end_layer();
    canvas.restore();

    // Two plain layers cost one transient each (their sizes differ, so no
    // reuse); the blurred layer costs its capture, the filtered target, and
    // the chain's single ping-pong scratch and blur scratch. All six are free again once
    // their layers have ended, and the flush deletes them.
    assert_eq!(canvas.transients.images.len(), 6);
    assert_eq!(canvas.transients.free.len(), 6);
    canvas.flush_to_output(());
    assert_eq!(canvas.transients.images.len(), 0);
    assert_eq!(canvas.transients.free.len(), 0);
    assert_eq!(canvas.transient_image_bytes(), 0);

    // Unbalanced end_layer is ignored.
    canvas.end_layer();
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

/// Sibling layers share backing stores: a layer's images return to the pool
/// at end_layer and the next layer of the same size takes them, so a frame's
/// transient memory is its deepest nesting, not its layer count. This is what
/// lets a 1080p frame of a few hundred layers - every BuseyBench portrait -
/// stay within tens of MB instead of the gigabytes their sum would be.
#[test]
fn sibling_layers_reuse_backing_stores() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(320, 200, 1.0);
    let bytes = 320 * 256 * 4; // 320 x 200 rounds up to 320 x 256

    // Three plain siblings: one allocation.
    for _ in 0..3 {
        assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
        assert!(canvas.layers.last().unwrap().image.is_some());
        canvas.end_layer();
    }
    assert_eq!(canvas.transients.images.len(), 1);
    assert_eq!(canvas.transient_image_bytes(), bytes);

    // Nesting needs one store per open level: a child cannot take its parent's.
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    canvas.end_layer();
    canvas.end_layer();
    assert_eq!(canvas.transients.images.len(), 2);

    // Blurred siblings: capture, filtered target, one chain scratch and one
    // horizontal blur scratch, once.
    let blur = LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 2.0 }]);
    for _ in 0..4 {
        assert!(canvas.begin_layer(&blur));
        canvas.end_layer();
    }
    let padded = 384 * 256 * 4; // 336 x 216 padded, rounded
    assert_eq!(canvas.transients.images.len(), 2 + 4);
    assert_eq!(canvas.transient_image_bytes(), 2 * bytes + 4 * padded);

    // Everything is free between layers, nothing after the flush.
    assert_eq!(canvas.transients.free.len(), 6);
    canvas.flush_to_output(());
    assert_eq!(canvas.transients.images.len(), 0);
    assert_eq!(canvas.transient_image_bytes(), 0);
}

/// One capture-sized slice stays available after a layer reserves its effect
/// images, so a later group can still preserve group opacity.
#[test]
fn a_budget_for_one_layer_fits_a_frame_of_them() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(256, 256, 1.0);
    let padded = 320 * 320 * 4; // 272 x 272 padded, rounded
    canvas.set_transient_image_budget(5 * padded);
    let blur = LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 2.0 }]);
    for i in 0..200 {
        assert!(canvas.begin_layer(&blur));
        assert!(
            canvas.layers.last().unwrap().image.is_some(),
            "layer {i} degraded to pass-through"
        );
        canvas.end_layer();
    }
    assert_eq!(canvas.transients.images.len(), 4);
    assert_eq!(canvas.transient_image_bytes(), 4 * padded);
}

/// A layer keeps its capture and group opacity when optional filter storage
/// does not fit. With enough room plus capture headroom, it applies the full
/// chain.
#[test]
fn a_layer_short_of_its_chain_scratch_budget_keeps_its_capture() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(128, 128, 1.0);
    let padded = 192 * 192 * 4; // 144 x 144 padded, rounded
    let blur = LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 2.0 }]);
    canvas.set_transient_image_budget(3 * padded);
    assert!(canvas.begin_layer(&blur));
    assert!(canvas.layers.last().unwrap().image.is_some());
    assert!(canvas.layers.last().unwrap().filter_images.is_none());
    assert_eq!(canvas.transients.images.len(), 1);
    canvas.end_layer();

    canvas.flush_to_output(());
    canvas.set_transient_image_budget(5 * padded);
    assert!(canvas.begin_layer(&blur));
    let target = canvas.layers.last().unwrap().filter_images.unwrap().target;
    assert_eq!(canvas.transients.images.len(), 4);
    assert_eq!(
        canvas.transients.free.len(),
        0,
        "the chain's images are held from begin_layer"
    );
    canvas.end_layer();
    // The composite samples the filtered result, not the capture.
    let composite = canvas
        .commands
        .iter()
        .rev()
        .find(|c| c.image.is_some())
        .expect("a composite was recorded");
    assert_eq!(composite.image, Some(target));
    assert_eq!(canvas.transients.free.len(), 4);
}

/// A filtered layer's reservation is sized by its pass plan: the result, one
/// chain scratch when needed, and a blur scratch when needed. The layer's
/// result has the same storage convention as its scratch, so longer chains
/// can alternate through the two.
#[test]
fn a_filtered_layer_reserves_its_chain_images_by_pass_plan() {
    use crate::ImageFilter;
    let cases: [(&[ImageFilter], usize, usize); 4] = [
        // One color pass: the result only. No blur, so the store is the canvas.
        (&[ImageFilter::brightness(0.0)], 2, 64),
        // Blur plus its parity pass: one scratch. Sigma 1 pads 5 px, rounding to 128.
        (&[ImageFilter::GaussianBlur { sigma: 1.0 }], 4, 128),
        // A blur above the per-pass bound is four passes plus parity, and the
        // store pads by the true reach, 50 px, to 192.
        (&[ImageFilter::GaussianBlur { sigma: 16.0 }], 4, 192),
        // A blur never folds with a color matrix, so brightness, blur and
        // invert are three passes plus the parity identity: four, two scratches.
        (
            &[
                ImageFilter::brightness(2.0),
                ImageFilter::GaussianBlur { sigma: 1.0 },
                ImageFilter::invert(1.0),
            ],
            4,
            128,
        ),
    ];
    for (filters, images, store) in cases {
        let renderer = RecordingRenderer::default();
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(64, 64, 1.0);
        let effects = LayerEffects::new().with_filters(filters);
        let bytes = store * store * 4;

        canvas.set_transient_image_budget((images + 1) * bytes);
        assert!(canvas.begin_layer(&effects), "{filters:?}: {images} images fit");
        assert_eq!(canvas.transients.images.len(), images, "{filters:?}");
        assert_eq!(
            canvas.transients.free.len(),
            0,
            "{filters:?}: all held from begin_layer"
        );
        canvas.end_layer();
        assert_eq!(
            canvas.transients.free.len(),
            images,
            "{filters:?}: all returned at end_layer"
        );

        canvas.flush_to_output(());
        canvas.set_transient_image_budget(images * bytes);
        assert!(canvas.begin_layer(&effects), "{filters:?}: the capture still fits");
        assert!(canvas.layers.last().unwrap().filter_images.is_none(), "{filters:?}");
        canvas.end_layer();
    }
}

/// A frame boundary at the same size keeps an open layer capturing (WPT
/// 2d.layer.flush-on-frame-presentation): set_size re-issues the layer's
/// target and the tracked target agrees with the command stream - it used to
/// say the layer while the stream said the screen, and the next frame's
/// draws went to the screen unfaded.
#[test]
fn set_size_keeps_an_open_layer_across_a_frame() {
    use crate::renderer::CommandType;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(200, 200, 1.0);
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    let layer = canvas.layers.last().unwrap().image.unwrap();
    canvas.flush_to_output(());
    canvas.set_size(200, 200, 1.0);
    let targets: Vec<RenderTarget> = canvas
        .commands
        .iter()
        .filter_map(|c| match c.cmd_type {
            CommandType::SetRenderTarget(target) => Some(target),
            _ => None,
        })
        .collect();
    assert_eq!(targets.last(), Some(&RenderTarget::Image(layer)));
    assert_eq!(canvas.current_render_target, RenderTarget::Image(layer));
    canvas.end_layer();
    assert_eq!(canvas.current_render_target, RenderTarget::Screen);
}

/// A resize discards open layers, as a Canvas 2D reset discards pending
/// layers (WPT 2d.layer.reset): the state stack rebalances, the stores return
/// to the pool and drawing continues on the screen.
#[test]
fn set_size_resize_discards_open_layers() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(200, 200, 1.0);
    let depth = canvas.state_stack.len();
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    canvas.set_size(300, 200, 1.0);
    assert!(canvas.layers.is_empty());
    assert_eq!(canvas.state_stack.len(), depth);
    assert_eq!(canvas.current_render_target, RenderTarget::Screen);
    assert_eq!(canvas.transients.free.len(), canvas.transients.images.len());
    canvas.end_layer(); // unbalanced now, ignored
    assert_eq!(canvas.current_render_target, RenderTarget::Screen);
}

/// Successive Gaussians compound in quadrature: three sigma-8 blurs reach
/// like one of sigma 13.9, so the store pads by 44 px, not the 26 of the
/// largest alone.
#[test]
fn chained_blurs_pad_in_quadrature() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(800, 600, 1.0);
    canvas.save();
    canvas.scissor(100.0, 50.0, 200.0, 200.0);
    let blur = ImageFilter::GaussianBlur { sigma: 8.0 };
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[blur, blur, blur])));
    let record = canvas.layers.last().unwrap();
    // 200 + 2 * (ceil(3 * sqrt(3 * 64)) + 2) = 288, rounded to 320.
    assert_eq!((record.width, record.height), (320, 320));
    canvas.end_layer();
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[blur])));
    let record = canvas.layers.last().unwrap();
    // 200 + 2 * 26 = 252, rounded to 256.
    assert_eq!((record.width, record.height), (256, 256));
    canvas.end_layer();
    canvas.restore();
}

/// A blur within the shader's per-pass bound is one pass with its sigma
/// untouched, plus the parity identity - the plan it always had, so small
/// blurs render exactly as before the split existed. A degenerate sigma is
/// one pass too, for the coefficient sanitization to copy through.
#[test]
fn a_blur_within_the_shader_bound_stays_one_pass() {
    use crate::ImageFilter;
    for sigma in [0.5, 3.0, 8.0] {
        let passes = filter_passes(&[ImageFilter::GaussianBlur { sigma }]).unwrap();
        assert_eq!(passes.len(), 2, "sigma {sigma}: one blur pass and the parity identity");
        assert!(
            matches!(passes[0], ImageFilter::GaussianBlur { sigma: s } if s == sigma),
            "{:?}",
            passes[0]
        );
        assert!(matches!(passes[1], ImageFilter::ColorMatrix { matrix } if matrix == ImageFilter::IDENTITY_MATRIX));
    }
    assert_eq!(blur_passes(8.0), (1, 8.0));
    assert_eq!(blur_passes(0.0), (1, 0.0));
    assert_eq!(blur_passes(-1.0).0, 1);
    assert_eq!(blur_passes(f32::NAN).0, 1);
}

/// Gaussians compose in quadrature, so a blur above the bound B = 8 is
/// exactly k = ceil((sigma / B)^2) passes of sigma / sqrt(k): 16 is four of
/// 8, 23 is nine of 23/3, every pass within the bound and their squares
/// summing back to the requested sigma's. The ceiling keeps the plan finite
/// for an absurd sigma.
#[test]
fn a_blur_above_the_bound_splits_into_quadrature_passes() {
    use crate::ImageFilter;
    let blur_sigmas = |filters: &[ImageFilter]| -> Vec<f32> {
        filter_passes(filters)
            .unwrap()
            .iter()
            .filter_map(|f| match f {
                ImageFilter::GaussianBlur { sigma } => Some(*sigma),
                _ => None,
            })
            .collect()
    };
    assert_eq!(blur_sigmas(&[ImageFilter::GaussianBlur { sigma: 16.0 }]), vec![8.0; 4]);
    let nine = blur_sigmas(&[ImageFilter::GaussianBlur { sigma: 23.0 }]);
    assert_eq!(nine.len(), 9);
    for sigma in &nine {
        assert!((sigma - 23.0 / 3.0).abs() < 1e-5, "{sigma}");
        assert!(*sigma <= renderer::MAX_BLUR_SIGMA);
    }
    let composed: f32 = nine.iter().map(|s| s * s).sum::<f32>().sqrt();
    assert!((composed - 23.0).abs() < 1e-4, "{composed}");
    // Just past the bound: two passes, neither above it.
    let (passes, sigma) = blur_passes(8.5);
    assert_eq!(passes, 2);
    assert!((sigma - 8.5 / 2f32.sqrt()).abs() < 1e-5);
    // The ceiling bounds the plan: an infinite sigma is 128's 256 passes,
    // not 2^56 of them.
    assert_eq!(blur_passes(f32::INFINITY), blur_passes(128.0));
    assert_eq!(blur_passes(128.0), (256, 8.0));
    assert_eq!(blur_passes(1e9), blur_passes(128.0));
}

/// The split changes the pass count, not the chain's shape: a blur pass
/// preserves the flip, so [blur 16] ends with the parity identity like
/// [blur 8] does and [blur 16, brightness] does not, like [blur 8,
/// brightness]; and every chain ping-pongs through the same scratch pair -
/// min(2, passes - 1) of them, however many passes a split adds.
#[test]
fn a_split_blur_keeps_the_chain_parity_and_scratch_count() {
    use crate::ImageFilter;
    let ends_with_identity = |filters: &[ImageFilter]| {
        matches!(
            filter_passes(filters).unwrap().last(),
            Some(ImageFilter::ColorMatrix { matrix }) if *matrix == ImageFilter::IDENTITY_MATRIX
        )
    };
    let small = ImageFilter::GaussianBlur { sigma: 8.0 };
    let big = ImageFilter::GaussianBlur { sigma: 16.0 };
    let bright = ImageFilter::brightness(1.2);
    assert!(ends_with_identity(&[small]) && ends_with_identity(&[big]));
    assert!(!ends_with_identity(&[small, bright]) && !ends_with_identity(&[big, bright]));
    assert_eq!(filter_passes(&[big, bright]).unwrap().len(), 5);

    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    for filters in [&[big][..], &[big, bright], &[big, bright, big]] {
        let scratch = canvas
            .acquire_filter_scratches(64, 64, filter_passes(filters).unwrap().len(), true, 2, 0)
            .unwrap();
        assert_eq!(
            scratch.chain.iter().flatten().count(),
            2,
            "{filters:?}: one scratch pair"
        );
        assert!(scratch.blur.is_some());
        for id in scratch.images() {
            canvas.release_transient_image(id);
        }
    }
    assert_eq!(
        canvas.transients.images.len(),
        3,
        "the scratches are reused across plans"
    );
}

#[test]
fn filter_plans_bound_total_work_and_fail_layers_atomically() {
    let max_blur = ImageFilter::GaussianBlur { sigma: 128.0 };
    assert_eq!(filter_passes(&[max_blur]).unwrap().len(), MAX_FILTER_PASSES);
    assert!(filter_passes(&[max_blur, max_blur]).is_none());

    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let source = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let target = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    assert!(matches!(
        canvas.filter_image_chain(target, &[max_blur, max_blur], source),
        Err(ErrorKind::FilterPassLimitExceeded)
    ));
    assert!(!canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::RenderFilteredImage { .. })));

    let effects = LayerEffects::new().with_filters(&[max_blur, max_blur]);
    assert!(canvas.begin_layer(&effects));
    assert!(canvas.layers.last().unwrap().image.is_some());
    assert!(canvas.layers.last().unwrap().filter_images.is_none());
    canvas.end_layer();

    canvas.flush_to_output(());
    let too_many = vec![ImageFilter::identity(); MAX_FILTER_PASSES + 1];
    let effects = LayerEffects::new().with_filters(&too_many);
    assert_eq!(effects.filters.len(), MAX_FILTER_PASSES + 1);
    assert!(canvas.begin_layer(&effects));
    assert!(canvas.layers.last().unwrap().filter_images.is_none());
    canvas.end_layer();
}

#[test]
fn filter_work_matches_shader_sampling_and_resets_at_flush() {
    let blur = ImageFilter::GaussianBlur { sigma: 8.0 };
    assert_eq!(filter_work(&[blur], 10, 10), 9_400);
    let turbulence = |num_octaves| ImageFilter::Turbulence {
        base_frequency: [0.1, 0.1],
        num_octaves,
        seed: 1,
        stitch_tiles: false,
        kind: TurbulenceKind::Turbulence,
        transform: Transform2D::identity(),
    };
    assert_eq!(filter_work(&[turbulence(0)], 10, 10), 100);
    assert_eq!(filter_work(&[turbulence(10)], 10, 10), 8_000);

    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(16, 16, 1.0);
    let source = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let target = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_filter_work_budget(512);
    let color = ImageFilter::brightness(0.5);
    canvas.filter_image_chain(target, &[color], source).unwrap();
    canvas.filter_image_chain(target, &[color], source).unwrap();
    assert!(matches!(
        canvas.filter_image_chain(target, &[color], source),
        Err(ErrorKind::FilterWorkBudgetExceeded)
    ));
    assert_eq!(canvas.filter_work, 512);
    canvas.flush_to_output(());
    assert_eq!(canvas.filter_work, 0);
    canvas.filter_image_chain(target, &[color], source).unwrap();
}

#[test]
fn over_budget_layers_preserve_safe_fallbacks() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    canvas.set_filter_work_budget(0);

    let color = LayerEffects::new()
        .with_opacity(0.5)
        .with_filters(&[ImageFilter::brightness(0.5)]);
    assert!(canvas.begin_layer(&color));
    assert!(canvas.layers.last().unwrap().filter_images.is_none());
    assert!(!canvas.layers.last().unwrap().discard);
    canvas.end_layer();

    let turbulence = ImageFilter::Turbulence {
        base_frequency: [0.1, 0.1],
        num_octaves: 1,
        seed: 1,
        stitch_tiles: false,
        kind: TurbulenceKind::Turbulence,
        transform: Transform2D::identity(),
    };
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[turbulence])));
    assert!(canvas.layers.last().unwrap().discard);
    canvas.end_layer();
}

#[test]
fn capture_failure_suppresses_masks_but_keeps_an_opacity_fallback() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(1, 1, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_transient_image_budget(0);

    let masked = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    let commands = canvas.commands.len();
    assert!(canvas.begin_layer(&masked));
    assert!(canvas.layers.last().unwrap().image.is_none());
    assert!(canvas.layers.last().unwrap().discard);
    let mut rect = Path::new();
    rect.rect(0.0, 0.0, 64.0, 64.0);
    canvas.fill_path(&rect, &Paint::color(Color::white()));
    assert_eq!(canvas.commands.len(), commands);
    canvas.end_layer();

    let faded = LayerEffects::new().with_opacity(0.5);
    assert!(!canvas.begin_layer(&faded));
    assert_eq!(canvas.state().alpha, 0.5);
    canvas.end_layer();
    assert_eq!(canvas.state().alpha, 1.0);
}

#[test]
fn a_suppressed_layer_reissues_an_image_target_across_flush() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let target = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let mask = canvas
        .create_image_empty(1, 1, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(target));
    canvas.set_transient_image_budget(0);

    let masked = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&masked));
    assert!(canvas.layers.last().unwrap().discard);
    canvas.flush_to_output(());
    canvas.end_layer();

    let mut rect = Path::new();
    rect.rect(0.0, 0.0, 8.0, 8.0);
    canvas.fill_path(&rect, &Paint::color(Color::white()));
    assert!(matches!(
        canvas.commands.first().map(|command| &command.cmd_type),
        Some(CommandType::SetRenderTarget(RenderTarget::Image(id))) if *id == target
    ));
}

#[test]
fn a_suppressed_draw_does_not_consume_a_dirty_clip_replay() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(1, 1, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();

    let mut outer = Path::new();
    outer.rect(4.0, 4.0, 56.0, 56.0);
    canvas.clip_path(&outer, FillRule::NonZero);
    canvas.save();
    let mut inner = Path::new();
    inner.rect(8.0, 8.0, 32.0, 32.0);
    canvas.clip_path(&inner, FillRule::NonZero);
    canvas.flush_to_output(());
    canvas.restore();
    assert!(canvas.clip_planes[&RenderTarget::Screen].dirty);

    canvas.set_transient_image_budget(0);
    let masked = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&masked));
    let mut rect = Path::new();
    rect.rect(0.0, 0.0, 64.0, 64.0);
    canvas.fill_path(&rect, &Paint::color(Color::white()));
    assert!(canvas.clip_planes[&RenderTarget::Screen].dirty);
    canvas.end_layer();

    canvas.fill_path(&rect, &Paint::color(Color::white()));
    assert!(!canvas.clip_planes[&RenderTarget::Screen].dirty);
    assert!(canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::ClipReset { visible: true })));
    assert!(canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::ClipFill)));
}

#[test]
fn explicit_image_filters_are_not_suppressed_with_layer_draws() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(1, 1, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let source = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let target = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_transient_image_budget(0);

    let masked = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&masked));
    canvas.filter_image(target, ImageFilter::brightness(0.5), source);
    assert!(canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::RenderFilteredImage { .. })));
    canvas.end_layer();
}

#[test]
fn open_layer_filter_work_survives_a_flush_until_recorded() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let effects = LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 2.0 }]);
    assert!(canvas.begin_layer(&effects));
    let reserved = canvas.layers.last().unwrap().reserved_filter_work;
    assert!(reserved > 0);
    canvas.flush_to_output(());
    assert_eq!(canvas.filter_work, reserved);
    canvas.end_layer();
    assert_eq!(canvas.filter_work, reserved);
    canvas.flush_to_output(());
    assert_eq!(canvas.filter_work, 0);
}

#[test]
fn a_transparent_layer_skips_its_filter_plan() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let effects = LayerEffects::new()
        .with_opacity(0.0)
        .with_filters(&[ImageFilter::GaussianBlur { sigma: 128.0 }]);
    assert!(canvas.begin_layer(&effects));
    canvas.end_layer();
    assert_eq!(canvas.filter_work, 0);
    assert!(!canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::RenderFilteredImage { .. })));
}

/// A layer pads its store by the blur's true reach, 3 sigma + 2 per side,
/// since the chain now renders it: sigma 16 pads 50 px where the per-pass
/// bound padded 26, and two of them compound to sqrt(512) = 22.6, 70 px.
#[test]
fn a_layer_pads_by_the_true_blur_reach() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(800, 600, 1.0);
    canvas.save();
    canvas.scissor(100.0, 50.0, 200.0, 200.0);
    for (sigma, pad, store) in [(8.0, 26.0, 256), (16.0, 50.0, 320), (23.0, 71.0, 384)] {
        let blur = ImageFilter::GaussianBlur { sigma };
        assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[blur])));
        let record = canvas.layers.last().unwrap();
        assert_eq!(record.origin, (100.0 - pad, 50.0 - pad), "sigma {sigma}");
        // 200 + 2 * pad, rounded up to 64.
        assert_eq!((record.width, record.height), (store, store), "sigma {sigma}");
        canvas.end_layer();
    }
    let blur = ImageFilter::GaussianBlur { sigma: 16.0 };
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[blur, blur])));
    let record = canvas.layers.last().unwrap();
    assert_eq!(record.origin, (30.0, -20.0));
    assert_eq!((record.width, record.height), (384, 384));
    canvas.end_layer();
    canvas.restore();
}

/// A layer whose true blur reach would pad its store past the backend's
/// texture limit still captures: the pad shrinks to what the limit leaves,
/// so the blur loses reach at the store edge instead of the layer passing
/// through with every effect dropped.
#[test]
fn a_layer_at_the_texture_limit_keeps_its_blur_with_a_bounded_pad() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer {
        max_texture_size: 2048,
        ..RecordingRenderer::default()
    };
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(1920, 1080, 1.0);
    // Sigma 40 wants 122 px of pad: 2164 px wide, past the limit.
    let blur = LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 40.0 }]);
    assert!(
        canvas.begin_layer(&blur),
        "the layer captures instead of passing through"
    );
    let record = canvas.layers.last().unwrap();
    assert!(record.image.is_some());
    assert!(
        record.width <= 2048 && record.height <= 2048,
        "store {}x{}",
        record.width,
        record.height
    );
    // Pad 32 (what a 1920 px span leaves under 2048 with a 64 px rounding
    // margin): 1984 wide, within the limit.
    assert_eq!(record.width, 1984, "the store uses what the limit leaves");
    canvas.end_layer();

    // The same blur on a small scissor keeps its full reach.
    canvas.scissor(100.0, 100.0, 200.0, 200.0);
    assert!(canvas.begin_layer(&blur));
    let record = canvas.layers.last().unwrap();
    assert_eq!(record.origin, (100.0 - 122.0, 100.0 - 122.0));
    canvas.end_layer();
}

/// The pad rule alone: the full pad when it fits, what the rounded store can
/// afford when it does not, never negative.
#[test]
fn a_bounded_pad_never_pushes_a_store_past_the_limit() {
    assert_eq!(bounded_pad(122.0, 1920.0, 2048, 64), 32.0);
    assert_eq!(bounded_pad(122.0, 200.0, 2048, 64), 122.0);
    assert_eq!(bounded_pad(122.0, 2048.0, 2048, 64), 0.0);
    assert_eq!(bounded_pad(0.0, 1920.0, 2048, 64), 0.0);
}

/// A scissor set under a scale still bounds the layer - every canvas drawn
/// under a device-pixel ratio has one - so the store follows the device-space
/// rect, not the whole canvas.
#[test]
fn layer_bounds_follow_a_scaled_scissor() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(800, 600, 1.0);
    canvas.save();
    canvas.scale(2.0, 2.0);
    canvas.scissor(50.0, 25.0, 60.0, 40.0); // device rect (100, 50) 120 x 80
    assert!(canvas.begin_layer(&LayerEffects::new()));
    let record = canvas.layers.last().unwrap();
    assert_eq!((record.width, record.height), (128, 128));
    assert_eq!(record.origin, (100.0, 50.0));
    canvas.end_layer();
    canvas.restore();
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

/// Past the backend's texture limit a layer passes through - drawing keeps
/// landing on the current target and begin/end stay balanced - instead of
/// failing to allocate. A VideoCore IV (Raspberry Pi Zero) reports 2048.
#[test]
fn layers_past_the_texture_limit_pass_through() {
    let renderer = RecordingRenderer {
        max_texture_size: 2048,
        ..RecordingRenderer::default()
    };
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(4096, 64, 1.0);
    assert!(!canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    assert!(canvas.layers.last().unwrap().image.is_none());
    canvas.end_layer();
    assert!(canvas.transients.images.is_empty());
    // A scissor within the limit captures again.
    canvas.save();
    canvas.scissor(0.0, 0.0, 1000.0, 64.0);
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    assert_eq!(canvas.layers.last().unwrap().width, 1024);
    canvas.end_layer();
    canvas.restore();
}

/// Shadow coverage rounds to 8 px, not the layers' 64: a 20 px shadowed
/// shape under a 2 px blur takes a 40 x 40 store, not 64 x 64.
#[test]
fn shadow_stores_round_to_eight_pixels() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(200, 200, 1.0);
    canvas.set_shadow_color(Color::rgba(0, 0, 0, 128));
    canvas.set_shadow_blur(4.0); // sigma 2: pad 8 each side
    let mut path = Path::new();
    path.rect(40.0, 40.0, 20.0, 20.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(200, 0, 0)));
    let info = canvas.images.info(canvas.transients.images[0]).unwrap();
    assert_eq!((info.width(), info.height()), (40, 40));
}

/// Masked sibling layers reuse the mask's transients too: a luminance mask
/// needs the layer capture, the normalized mask and its alpha conversion,
/// once for any number of siblings.
#[test]
fn masked_siblings_reuse_mask_transients() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(200, 120, 1.0);
    let mask = canvas
        .create_image_empty(200, 120, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    for _ in 0..4 {
        assert!(canvas.begin_layer(&LayerEffects::new().with_mask(mask, MaskKind::Luminance, 0.0, 0.0, 200.0, 120.0)));
        assert!(canvas.layers.last().unwrap().image.is_some());
        // Reserved with the store: nothing of the mask's is left to chance
        // at end_layer.
        assert_eq!(
            canvas.transients.images.len(),
            3,
            "capture, normalized mask, converted mask"
        );
        assert!(
            canvas.transients.free.is_empty(),
            "all three are held while the layer is open"
        );
        canvas.end_layer();
        assert_eq!(
            canvas.transients.free.len(),
            3,
            "and returned once its composite is recorded"
        );
    }
    assert_eq!(
        canvas.transients.images.len(),
        3,
        "four masked siblings, one set of images"
    );
    canvas.flush_to_output(());
    assert_eq!(canvas.transients.images.len(), 0);
}

/// A reset or a resize inside a masked layer returns everything the layer
/// held - the capture and the mask's two coverage images - not only the
/// capture: under a budget of exactly those three, the next masked layer
/// takes them back from the pool instead of being refused a fourth.
#[test]
fn a_discarded_masked_layer_returns_its_coverage_images() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_transient_image_budget(4 * 64 * 64 * 4);
    let luminance = LayerEffects::new().with_mask(mask, MaskKind::Luminance, 0.0, 0.0, 64.0, 64.0);
    for what in ["reset", "resize"] {
        assert!(canvas.begin_layer(&luminance), "{what}: the first masked layer fits");
        assert_eq!(canvas.transients.images.len(), 3);
        match what {
            "reset" => canvas.reset(),
            _ => {
                canvas.set_size(128, 64, 1.0);
                canvas.set_size(64, 64, 1.0);
            }
        }
        assert!(canvas.layers.is_empty(), "{what}: the open layer is discarded");
        assert_eq!(
            canvas.transients.free.len(),
            3,
            "{what}: capture, normalized mask and converted mask all return"
        );
        assert!(
            canvas.begin_layer(&luminance),
            "{what}: the next masked layer takes them back"
        );
        assert_eq!(canvas.transients.images.len(), 3, "{what}: no fourth image");
        assert!(canvas.transients.free.is_empty());
        canvas.end_layer();
    }
}

/// A masked layer whose coverage images do not fit captures its draws but
/// discards them rather than revealing the group unmasked.
#[test]
fn a_masked_layer_without_room_for_its_coverage_fails_closed() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    // Room for the capture and the normalized mask, not the luminance conversion.
    canvas.set_transient_image_budget(2 * 64 * 64 * 4);
    let luminance = LayerEffects::new().with_mask(mask, MaskKind::Luminance, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&luminance));
    assert!(canvas.layers.last().unwrap().image.is_some());
    assert!(canvas.layers.last().unwrap().discard);
    canvas.end_layer();
    // The same budget fits an alpha mask, which needs no conversion.
    let alpha = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&alpha));
    canvas.end_layer();
    canvas.set_transient_image_budget(4 * 64 * 64 * 4);
    assert!(
        canvas.begin_layer(&luminance),
        "three images fit with one capture held in reserve"
    );
    assert!(!canvas.layers.last().unwrap().discard);
    canvas.end_layer();
}

/// `LayerEffects::default()` is the no-op effects `new()` describes, field by
/// field: full opacity, no filters, no mask. A derived Default would zero the
/// opacity, and `end_layer` composites nothing at alpha 0.
#[test]
fn default_layer_effects_are_the_no_op_effects_new_describes() {
    let default = LayerEffects::default();
    let new = LayerEffects::new();
    assert_eq!(default.opacity, 1.0);
    assert_eq!(default.opacity, new.opacity);
    assert!(default.filters.is_empty());
    assert!(new.filters.is_empty());
    assert!(default.mask.is_none());
    assert!(new.mask.is_none());
}

/// Shadows draw through the pool too: the coverage and blurred images of one
/// shadow serve the next shadow of the same size.
#[test]
fn shadow_passes_reuse_their_images() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(256, 256, 1.0);
    canvas.set_shadow_color(Color::rgba(0, 0, 0, 128));
    canvas.set_shadow_blur(4.0);
    let mut path = Path::new();
    path.rect(40.0, 40.0, 100.0, 60.0);
    for _ in 0..5 {
        canvas.fill_path(&path, &Paint::color(Color::rgb(200, 0, 0)));
    }
    assert_eq!(
        canvas.transients.images.len(),
        3,
        "five same-sized shadows allocate coverage, blur and horizontal scratch images"
    );
    assert_eq!(canvas.transients.free.len(), 3);
    canvas.flush_to_output(());
    assert_eq!(canvas.transients.images.len(), 0);
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

/// Past MAX_STATE_DEPTH the canvas saturates: saves become one bit each,
/// every restore still pairs with its save, nothing changes the real state
/// and, once unwound, the base is exactly what it was.
#[cfg(test)]
#[test]
fn saves_past_the_depth_limit_stay_paired_and_bounded() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let total = MAX_STATE_DEPTH + 1000;
    for _ in 0..total {
        canvas.save();
        canvas.translate(1.0, 0.0);
    }
    assert_eq!(canvas.state_stack.len(), MAX_STATE_DEPTH);
    assert_eq!(canvas.overflow.len(), total - (MAX_STATE_DEPTH - 1));
    assert!(canvas.saturated());
    assert_eq!(
        canvas.overflow_state.transform.0[4],
        (MAX_STATE_DEPTH - 1) as f32,
        "the state saved when saturation began"
    );

    for _ in 0..1001 {
        canvas.restore();
    }
    assert!(!canvas.saturated());
    assert_eq!(canvas.state_stack.len(), MAX_STATE_DEPTH);
    assert_eq!(
        canvas.state().transform.0[4],
        (MAX_STATE_DEPTH - 1) as f32,
        "translates past the limit were discarded"
    );
    for _ in 0..(MAX_STATE_DEPTH - 1) {
        canvas.restore();
    }
    assert_eq!(canvas.state_stack.len(), 1);
    assert_eq!(canvas.state().transform, Transform2D::identity());
    canvas.restore();
    assert_eq!(canvas.state_stack.len(), 1, "unmatched restore at the base is ignored");
}

/// Nothing draws past the limit - the safe reading of any effect a layer
/// there could declare - and a layer past it says so with `true`, keeps its
/// boundary for end_layer(), and leaves the real layer below open.
#[cfg(test)]
#[test]
fn a_layer_past_the_depth_limit_is_suppressed_and_keeps_its_boundary() {
    use renderer::CommandType;
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mut rect = Path::new();
    rect.rect(0.0, 0.0, 10.0, 10.0);
    let fills = |recorded: &Rc<RefCell<Vec<renderer::Command>>>| {
        recorded
            .borrow()
            .iter()
            .filter(|c| matches!(c.cmd_type, CommandType::ConvexFill { .. }))
            .count()
    };

    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    while canvas.state_stack.len() < MAX_STATE_DEPTH {
        canvas.save();
    }
    let store = canvas.current_render_target;
    assert!(
        canvas.begin_layer(&LayerEffects::new().with_opacity(0.0)),
        "suppressed, hence true"
    );
    assert_eq!(canvas.layers.len(), 1);
    assert_eq!(canvas.overflow.iter().filter(|&&layer| layer).count(), 1);
    canvas.save();
    canvas.fill_path(&rect, &Paint::color(Color::black()));
    canvas.flush_to_output(());
    assert_eq!(fills(&recorded), 0, "nothing draws past the limit");
    canvas.end_layer();
    assert!(
        !canvas.saturated(),
        "end_layer closed the layer with the save left open inside it"
    );
    assert_eq!(canvas.current_render_target, store);
    assert_eq!(canvas.layers.len(), 1, "the real layer is still open");
    canvas.fill_path(&rect, &Paint::color(Color::black()));
    canvas.flush_to_output(());
    assert_eq!(fills(&recorded), 1, "drawing resumes below the limit");
    while canvas.state_stack.len() > 2 {
        canvas.restore();
    }
    canvas.end_layer();
    assert!(canvas.layers.is_empty());
    assert_eq!(canvas.current_render_target, RenderTarget::Screen);
    assert_eq!(canvas.state_stack.len(), 1);
}

/// end_layer() past the limit with no layer to close pops nothing, so a
/// later restore() still pairs with its own save and the clip that save
/// level owns stays armed until that level is popped.
#[cfg(test)]
#[test]
fn end_layer_past_the_depth_limit_without_a_layer_pops_nothing() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    canvas.save();
    let mut clip = Path::new();
    clip.rect(0.0, 0.0, 32.0, 64.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    while canvas.state_stack.len() < MAX_STATE_DEPTH {
        canvas.save();
    }
    canvas.save(); // past the limit
    canvas.end_layer(); // no layer anywhere: nothing to close
    assert_eq!(canvas.overflow.len(), 1);
    canvas.restore(); // pairs with the save past the limit
    assert!(!canvas.saturated());
    assert_eq!(canvas.clip_stack.len(), 1, "the clip is untouched");
    while canvas.state_stack.len() > 2 {
        canvas.restore();
    }
    assert_eq!(canvas.clip_stack.len(), 1, "still armed at the level that took it");
    canvas.restore();
    assert!(canvas.clip_stack.is_empty());
}

/// Balanced save / clip_path / restore past the limit records no clip and
/// no geometry, and reset() drops the entries past the limit with the
/// layers it discards.
#[cfg(test)]
#[test]
fn clips_past_the_depth_limit_cost_nothing_and_reset_clears_the_overflow() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    while canvas.state_stack.len() < MAX_STATE_DEPTH {
        canvas.save();
    }
    let mut clip = Path::new();
    clip.rect(0.0, 0.0, 32.0, 64.0);
    for _ in 0..32 {
        canvas.save();
        canvas.clip_path(&clip, FillRule::NonZero);
        canvas.restore();
    }
    assert!(canvas.clip_stack.is_empty());
    assert!(canvas.clip_planes.is_empty());
    assert!(!canvas.saturated());

    for _ in 0..3 {
        canvas.save();
    }
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    assert_eq!(canvas.overflow.len(), 4);
    canvas.reset();
    assert!(!canvas.saturated());
    assert!(canvas.layers.is_empty());
    assert!(canvas.warned_overflow, "one warning per canvas");
}
