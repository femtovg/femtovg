//! Module containing renderer implementations

use image::DynamicImage;

use crate::{
    Color,
    Result,
    FillRule,
    ImageId,
    ImageFlags,
    ImageStore,
    CompositeOperationState
};

mod opengl;
pub use opengl::OpenGl;

mod void;
pub use void::Void;

mod params;
pub(crate) use params::Params;

// TODO: Rename this to ImageFormat

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TextureType {
    Rgb,
    Rgba,
    Alpha
}

#[derive(Copy, Clone, Default)]
pub struct Drawable {
    pub(crate) fill_verts: Option<(usize, usize)>,
    pub(crate) stroke_verts: Option<(usize, usize)>,
}

#[derive(Debug)]
pub enum CommandType {
    ClearRect {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        color: Color
    },
    ConvexFill {
        params: Params
    },
    ConcaveFill {
        stencil_params: Params,
        fill_params: Params,
    },
    Stroke {
        params: Params
    },
    StencilStroke {
        params1: Params,
        params2: Params
    },
    Triangles {
        params: Params
    },
}

pub struct Command {
    pub(crate) cmd_type: CommandType,
    pub(crate) drawables: Vec<Drawable>,
    pub(crate) triangles_verts: Option<(usize, usize)>,
    pub(crate) image: Option<ImageId>,
    pub(crate) alpha_mask: Option<ImageId>,
    pub(crate) fill_rule: FillRule,
    pub(crate) composite_operation: CompositeOperationState
}

impl Command {
    pub fn new(flavor: CommandType) -> Self {
        Self {
            cmd_type: flavor,
            drawables: Default::default(),
            triangles_verts: Default::default(),
            image: Default::default(),
            alpha_mask: Default::default(),
            fill_rule: Default::default(),
            composite_operation: Default::default()
        }
    }
}

#[derive(Copy, Clone)]
pub struct ImageInfo {
    flags: ImageFlags,
    width: usize,
    height: usize,
    format: TextureType
}

pub trait Image<T: Renderer> {
    fn create(renderer: &mut T, data: &DynamicImage, flags: ImageFlags) -> Result<Self> where Self: Sized;
    fn update(&mut self, renderer: &mut T, data: &DynamicImage, x: usize, y: usize) -> Result<()>;
    fn delete(self, renderer: &mut T);

    fn info(&self) -> ImageInfo;
}

/// This is the main renderer trait that the [Canvas](../struct.Canvas.html) draws to.
pub trait Renderer: Sized {
    type Image: Image<Self>;

    fn set_size(&mut self, width: u32, height: u32, dpi: f32);

    fn render(&mut self, images: &ImageStore<Self>, verts: &[Vertex], commands: &[Command]);

    fn screenshot(&mut self) -> Option<DynamicImage>;
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

#[derive(Copy, Clone)]
pub enum ShaderType {
    FillGradient,
    FillImage,
    Stencil,
}

impl Default for ShaderType {
    fn default() -> Self { Self::FillGradient }
}

impl ShaderType {
    pub fn to_f32(self) -> f32 {
        match self {
            Self::FillGradient => 0.0,
            Self::FillImage => 1.0,
            Self::Stencil => 2.0,
        }
    }
}
