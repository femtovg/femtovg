//! Module containing renderer implementations.

use imgref::ImgVec;
use rgb::RGBA8;

use crate::{
    geometry::Position, paint::GlyphTexture, Color, CompositeOperationState, ErrorKind, FillRule, ImageFilter, ImageId,
    ImageInfo, ImageSource, ImageStore,
};

mod opengl;
pub use opengl::OpenGl;

#[cfg(feature = "wgpu")]
mod wgpu;
#[cfg(feature = "wgpu")]
pub use wgpu::{WGPURenderOutput, WGPURenderer};

mod void;
pub use void::Void;

mod params;
pub(crate) use params::Params;

/// Represents a drawable object.
#[derive(Copy, Clone, Default, Debug)]
pub struct Drawable {
    pub(crate) fill_verts: Option<(usize, usize)>,
    pub(crate) stroke_verts: Option<(usize, usize)>,
}

/// Defines different types of commands that can be executed by the renderer.
#[derive(Debug)]
pub enum CommandType {
    /// Intersects the persistent stencil clip with a path: the drawables carry
    /// the winding fan triangles and `triangles_verts` the resolve quad over
    /// the previously visible clip bounds. `fill_rule` is the clip-rule.
    ClipFill,
    /// Rewrites the persistent stencil clip with a full-canvas quad
    /// (`triangles_verts`): `visible: true` arms the clip plane (everything
    /// visible, ambient value 0x80), `false` disarms it back to the zero
    /// ambient so clip-free rendering pays nothing.
    ClipReset {
        /// Whether the plane resets to "everything visible" (armed) or to the
        /// disarmed zero state.
        visible: bool,
    },
    /// Set the render target (screen or image).
    SetRenderTarget(RenderTarget),
    /// Clear a rectangle with the specified color.
    ClearRect {
        /// Color to fill the rectangle with.
        color: Color,
        /// A clip is armed on the target: clear only the stencil's winding
        /// bits so the clip plane (bit 7) survives. Otherwise the whole
        /// stencil is cleared, the tile clear a tiler does for free.
        keep_clip: bool,
    },
    /// Fill a convex shape.
    ConvexFill {
        /// Rendering parameters for the fill operation.
        params: Params,
    },
    /// Accumulates a fill's exact per-pixel coverage into the coverage atlas:
    /// one instance per edge (a [`Vertex`] holding both endpoints), swept to
    /// the right edge of the fill's region (`params.extent`). `clear` starts
    /// a batch of fills whose regions do not overlap.
    AccumulateCoverage {
        /// Rendering parameters: the shader and the region's far edge.
        params: Params,
        /// Whether the atlas is cleared first.
        clear: bool,
    },
    /// Draws a fill's paint through its accumulated coverage: a quad over
    /// the region, sampling the atlas as its glyph texture.
    CoverageFill {
        /// Rendering parameters for the fill operation.
        params: Params,
    },
    /// Fill a concave shape.
    ConcaveFill {
        /// Rendering parameters for the stencil operation.
        stencil_params: Params,
        /// Rendering parameters for the fill operation.
        fill_params: Params,
    },
    /// Stroke a shape.
    Stroke {
        /// Rendering parameters for the stroke operation.
        params: Params,
    },
    /// Stroke a shape using stencil.
    StencilStroke {
        /// Rendering parameters for the first stroke operation.
        params1: Params,
        /// Rendering parameters for the second stroke operation.
        params2: Params,
    },
    /// Render triangles.
    Triangles {
        /// Rendering parameters for the triangle operation.
        params: Params,
    },
    /// Render a filtered image.
    RenderFilteredImage {
        /// ID of the target image.
        target_image: ImageId,
        /// Image filter to apply.
        filter: ImageFilter,
    },
}

/// A blend pass's inputs beyond its mode: whether the backdrop (the glyph
/// texture) is stored the other way up from the image, the alpha the image
/// is scaled by first, and whether to write the image's contribution over
/// the backdrop - what source-over onto it adds - instead of the result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BlendPass {
    pub(crate) backdrop_flipped: bool,
    pub(crate) source_alpha: f32,
    pub(crate) contribution: bool,
}

impl Default for BlendPass {
    fn default() -> Self {
        Self {
            backdrop_flipped: false,
            source_alpha: 1.0,
            contribution: false,
        }
    }
}

/// Represents a command that can be executed by the renderer.
#[derive(Debug)]
pub struct Command {
    pub(crate) cmd_type: CommandType,
    // Whether the persistent stencil clip (Canvas::clip_path) applies to this
    // command's fragments. Set centrally when the command is appended.
    pub(crate) clip_active: bool,
    pub(crate) drawables: Vec<Drawable>,
    pub(crate) triangles_verts: Option<(usize, usize)>,
    pub(crate) image: Option<ImageId>,
    pub(crate) filter_scratch: Option<ImageId>,
    pub(crate) glyph_texture: GlyphTexture,
    // A blend pass's inputs beyond its mode; the backdrop is the glyph texture.
    pub(crate) blend_pass: BlendPass,
    pub(crate) fill_rule: FillRule,
    pub(crate) composite_operation: CompositeOperationState,
}

impl Command {
    /// Creates a new command with the specified command type.
    pub fn new(flavor: CommandType) -> Self {
        Self {
            cmd_type: flavor,
            clip_active: false,
            drawables: Vec::new(),
            triangles_verts: None,
            image: None,
            filter_scratch: None,
            glyph_texture: GlyphTexture::default(),
            blend_pass: BlendPass::default(),
            fill_rule: FillRule::default(),
            composite_operation: CompositeOperationState::default(),
        }
    }
}

/// Represents different render targets (screen or image).
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub enum RenderTarget {
    /// Render to the screen.
    Screen,
    /// Render to a specific image.
    Image(ImageId),
}

/// The main renderer trait that the [Canvas](../struct.Canvas.html) draws to.
pub trait Renderer {
    /// Associated image type.
    type Image;

    /// Associated native texture type.
    type NativeTexture;

    /// Associated external texture type.
    type ExternalTexture;

    /// Associated render output type.
    type RenderOutput;

    /// Associated type to hold commands created via `flush_to_output`.
    type CommandBuffer;

    /// Set the size of the renderer.
    fn set_size(&mut self, width: u32, height: u32, dpi: f32);

    /// Whether antialiased fills may be rasterized as exact per-pixel
    /// coverage ([`CommandType::AccumulateCoverage`] and
    /// [`CommandType::CoverageFill`]); otherwise they draw with the
    /// stencil and fringe path.
    fn supports_coverage_fills(&self) -> bool {
        false
    }

    /// Render the specified commands.
    fn render(
        &mut self,
        output: impl Into<Self::RenderOutput>,
        images: &mut ImageStore<Self::Image>,
        verts: &[Vertex],
        commands: Vec<Command>,
    ) -> Self::CommandBuffer;

    /// Allocate a new image with the specified image info.
    fn alloc_image(&mut self, info: ImageInfo) -> Result<Self::Image, ErrorKind>;

    /// Create a new image from a native texture.
    fn create_image_from_native_texture(
        &mut self,
        native_texture: Self::NativeTexture,
        info: ImageInfo,
    ) -> Result<Self::Image, ErrorKind>;

    /// Create a new image from a external texture.
    fn create_image_from_external_texture(
        &mut self,
        native_texture: Self::ExternalTexture,
        info: ImageInfo,
    ) -> Result<Self::Image, ErrorKind>;

    /// Update an image with new data.
    ///
    /// Implementations should start by calling [`ImageSource::check_update`], so
    /// that a copy reaching past the image, or carrying a pixel format the image
    /// was not created with, is reported as an error instead of being handed to
    /// the graphics API, where it fails validation and usually aborts.
    fn update_image(&mut self, image: &mut Self::Image, data: ImageSource, x: usize, y: usize)
        -> Result<(), ErrorKind>;

    /// Get the native texture associated with an image (default implementation returns an error).
    #[allow(unused_variables)]
    fn get_native_texture(&self, image: &Self::Image) -> Result<Self::NativeTexture, ErrorKind> {
        Err(ErrorKind::UnsupportedImageFormat)
    }

    /// Delete an image.
    fn delete_image(&mut self, image: Self::Image, image_id: ImageId);

    /// Take a screenshot of the current render target.
    fn screenshot(&mut self) -> Result<ImgVec<RGBA8>, ErrorKind>;

    /// The largest width or height this backend can allocate for an image, in
    /// pixels. Layers and shadows whose stores would exceed it degrade rather
    /// than fail. The default matches current desktop GPUs; a VideoCore IV
    /// (Raspberry Pi Zero through 3) reports 2048.
    fn max_texture_size(&self) -> usize {
        8192
    }

    /// Backend allocation charged when the transient pool creates `info`.
    /// Renderers override this for attachments or scratch reserved alongside
    /// pooled images.
    fn transient_image_cost(&self, info: ImageInfo) -> usize {
        let bytes_per_pixel = match info.format() {
            crate::PixelFormat::Gray8 => 1,
            crate::PixelFormat::Rgb8 => 3,
            crate::PixelFormat::Rgba8 => 4,
        };
        info.width()
            .saturating_mul(info.height())
            .saturating_mul(bytes_per_pixel)
    }
}

/// Marker trait for renderers that don't have a surface.
pub trait SurfacelessRenderer: Renderer {
    /// Render the specified commands.
    fn render_surfaceless(&mut self, images: &mut ImageStore<Self::Image>, verts: &[Vertex], commands: Vec<Command>);
}

use bytemuck::{Pod, Zeroable};

/// Vertex struct for specifying triangle geometry.
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Default, Pod, Zeroable)]
#[repr(C)]
pub struct Vertex {
    /// X-coordinate of the vertex.
    pub x: f32,
    /// Y-coordinate of the vertex.
    pub y: f32,
    /// U-coordinate of the vertex (for texture mapping).
    pub u: f32,
    /// V-coordinate of the vertex (for texture mapping).
    pub v: f32,
}

impl Vertex {
    pub(crate) fn pos(position: Position, u: f32, v: f32) -> Self {
        let Position { x, y } = position;
        Self { x, y, u, v }
    }

    /// Create a new vertex with the specified coordinates.
    pub fn new(x: f32, y: f32, u: f32, v: f32) -> Self {
        Self { x, y, u, v }
    }

    /// Set the coordinates of the vertex.
    pub fn set(&mut self, x: f32, y: f32, u: f32, v: f32) {
        *self = Self { x, y, u, v };
    }
}

/// Represents different types of shaders used by the renderer.
///
/// The default value is `FillGradient`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub enum ShaderType {
    /// Fill gradient shader.
    #[default]
    FillGradient,
    /// Fill image shader.
    FillImage,
    /// Stencil shader.
    Stencil,
    /// Fill image gradient shader.
    FillImageGradient,
    /// Filter image shader.
    FilterImage,
    /// Fill color shader.
    FillColor,
    /// Texture copy unclipped shader.
    TextureCopyUnclipped,
    /// Fill color shader without clipping, used for `clear_rect()`
    FillColorUnclipped,
    /// Fill conic gradient shader.
    FillGradientConic,
    /// Fill image conic gradient shader.
    FillImageGradientConic,
    /// Color-matrix image filter shader (`feColorMatrix` / CSS color functions).
    FilterImageColorMatrix,
    /// Fill two-point (independently centered) radial gradient shader.
    ///
    /// This is the general Canvas `createRadialGradient(x0, y0, r0, x1, y1, r1)`
    /// form, where the start and end circles may have different centers. Ordinary
    /// concentric radial gradients keep using the cheaper box-gradient
    /// [`FillGradient`](Self::FillGradient) path.
    FillGradientTwoPointRadial,
    /// Fill image two-point radial gradient shader (multi-stop LUT variant).
    FillImageGradientTwoPointRadial,
    /// Turbulence generator shader (SVG `feTurbulence`); samples the noise
    /// lattice bound in place of the image.
    FilterImageTurbulence,
    /// sRGB transfer-curve shader: linearRGB to sRGB, or the reverse.
    FilterImageTransfer,
    /// Blend shader (SVG `feBlend`): the image over the backdrop bound in the
    /// glyph-texture slot, with one of the sixteen blend modes.
    FilterImageBlend,
    /// Coverage accumulation: an edge's signed area per pixel.
    CoverageAccumulate,
}

impl ShaderType {
    /// Convert the shader type to a u8 value.
    pub fn to_u8(self) -> u8 {
        match self {
            Self::FillGradient => 0,
            Self::FillImage => 1,
            Self::Stencil => 2,
            Self::FillImageGradient => 3,
            Self::FilterImage => 4,
            Self::FillColor => 5,
            Self::TextureCopyUnclipped => 6,
            Self::FillColorUnclipped => 7,
            Self::FillGradientConic => 8,
            Self::FillImageGradientConic => 9,
            Self::FilterImageColorMatrix => 10,
            Self::FillGradientTwoPointRadial => 11,
            Self::FillImageGradientTwoPointRadial => 12,
            Self::FilterImageTurbulence => 13,
            Self::FilterImageTransfer => 14,
            Self::FilterImageBlend => 15,
            Self::CoverageAccumulate => 16,
        }
    }

    /// Convert the shader type to a f32 value.
    pub fn to_f32(self) -> f32 {
        self.to_u8() as f32
    }
}

/// The largest standard deviation one Gaussian blur pass renders. The
/// fragment shader's blur loop is bounded at 24 taps per side (GLES 2.0 needs
/// a constant loop bound) and the kernel reaches 3 sigma, so a pass covers
/// sigma 8 exactly and no more. A blur above it is not clamped away: the chain
/// planner (`filter_passes` in lib.rs) runs it as several passes of at most
/// this sigma, which compose in quadrature to the requested one. The
/// coefficients below and the shader's tap count agree on this value.
pub(crate) const MAX_BLUR_SIGMA: f32 = 8.0;

/// Gaussian blur coefficients for `sigma`, sanitized the same way for every
/// backend. Sigma 0 (or negative / NaN) would divide the coefficient by zero
/// and blank the output instead of passing the image through, and a sigma
/// above [`MAX_BLUR_SIGMA`] must clamp to the bound the fragment shader's loop
/// uses so the coefficients and the iteration count agree - a single
/// `filter_image` pass renders such a sigma at the bound; a chain, a layer
/// filter or a shadow blur splits it into passes first and never sends one
/// above it. Near-zero renders as a visually exact copy. Returns the three
/// coefficients and the sanitized sigma the shader must be given.
pub(crate) fn gaussian_blur_coefficients(sigma: f32) -> ([f32; 3], f32) {
    let sigma = if sigma > 0.0 { sigma.min(MAX_BLUR_SIGMA) } else { 1e-3 };
    let x = 1. / ((2. * std::f32::consts::PI).sqrt() * sigma);
    let y = f32::exp(-0.5 / (sigma * sigma));
    ([x, y, y * y], sigma)
}
