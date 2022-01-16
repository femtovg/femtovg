//! Module containing renderer implementations.

use imgref::ImgVec;
use rgb::RGBA8;

use crate::{
    paint::GlyphTexture, Color, CompositeOperationState, ErrorKind, FillRule, ImageFilter, ImageId, ImageInfo,
    ImageSource, ImageStore,
};

mod opengl;
pub use opengl::OpenGl;

mod void;
pub use void::Void;

mod params;
pub(crate) use params::Params;

#[derive(Copy, Clone, Default, Debug)]
pub struct Drawable {
    pub(crate) fill_verts: Option<(usize, usize)>,
    pub(crate) stroke_verts: Option<(usize, usize)>,
}

#[derive(Debug)]
pub enum CommandType {
    SetRenderTarget(RenderTarget),
    ClearRect {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        color: Color,
    },
    ConvexFill {
        params: Params,
    },
    ConcaveFill {
        stencil_params: Params,
        fill_params: Params,
    },
    Stroke {
        params: Params,
    },
    StencilStroke {
        params1: Params,
        params2: Params,
    },
    Triangles {
        params: Params,
    },
    RenderFilteredImage {
        target_image: ImageId,
        filter: ImageFilter,
    },
}

pub struct Command {
    pub(crate) cmd_type: CommandType,
    pub(crate) drawables: Vec<Drawable>,
    pub(crate) triangles_verts: Option<(usize, usize)>,
    pub(crate) image: Option<ImageId>,
    pub(crate) glyph_texture: GlyphTexture,
    pub(crate) fill_rule: FillRule,
    pub(crate) composite_operation: CompositeOperationState,
}

impl Command {
    pub fn new(flavor: CommandType) -> Self {
        Self {
            cmd_type: flavor,
            drawables: Default::default(),
            triangles_verts: Default::default(),
            image: Default::default(),
            glyph_texture: Default::default(),
            fill_rule: Default::default(),
            composite_operation: Default::default(),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum RenderTarget {
    Screen,
    Image(ImageId),
}

/// This is the main renderer trait that the [Canvas](../struct.Canvas.html) draws to.
pub trait Renderer {
    type Image;

    fn set_size(&mut self, width: u32, height: u32);

    fn render(&mut self, images: &mut ImageStore<Self::Image>, verts: &[Vertex], commands: Vec<Command>);

    fn alloc_image(&mut self, info: ImageInfo) -> Result<Self::Image, ErrorKind>;
    fn update_image(&mut self, image: &mut Self::Image, data: ImageSource, x: usize, y: usize)
        -> Result<(), ErrorKind>;
    fn delete_image(&mut self, image: Self::Image, image_id: ImageId);

    fn screenshot(&mut self) -> Result<ImgVec<RGBA8>, ErrorKind>;
}

/// Vertex struct for specifying triangle geometry
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Default)]
#[repr(C)]
pub struct Vertex {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
}

impl Vertex {
    pub fn new(x: f32, y: f32, u: f32, v: f32) -> Self {
        Self { x, y, u, v }
    }

    pub fn set(&mut self, x: f32, y: f32, u: f32, v: f32) {
        *self = Self { x, y, u, v };
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ShaderType {
    FillGradient,
    FillImage,
    Stencil,
    FillImageGradient,
    FilterImage,
}

impl Default for ShaderType {
    fn default() -> Self {
        Self::FillGradient
    }
}

impl ShaderType {
    pub fn to_f32(self) -> f32 {
        match self {
            Self::FillGradient => 0.0,
            Self::FillImage => 1.0,
            Self::Stencil => 2.0,
            Self::FillImageGradient => 3.0,
            Self::FilterImage => 4.0,
        }
    }
}
