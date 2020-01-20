//! Module containing renderer implementations

use image::DynamicImage;

use crate::geometry::Transform2D;
use crate::{Color, Paint, Path, Scissor, ImageId, ImageFlags};

mod void;
pub use void::Void;

mod image_renderer;
pub use image_renderer::ImageRenderer;

pub mod gpu;

/// This is the main renderer trait that the [Canvas](../struct.Canvas.html) draws to.
pub trait Renderer {
    fn flush(&mut self);
    fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color);
    fn set_size(&mut self, width: u32, height: u32, dpi: f32);

    fn fill(&mut self, path: &Path, paint: &Paint, scissor: &Scissor, transform: &Transform2D);
    fn stroke(&mut self, path: &Path, paint: &Paint, scissor: &Scissor, transform: &Transform2D);
    fn triangles(&mut self, verts: &[Vertex], paint: &Paint, scissor: &Scissor, transform: &Transform2D);

    fn create_image(&mut self, image: &DynamicImage, flags: ImageFlags) -> ImageId;
    fn update_image(&mut self, id: ImageId, image: &DynamicImage, x: u32, y: u32);
    fn delete_image(&mut self, id: ImageId);

    fn screenshot(&mut self) -> Option<DynamicImage> {
        None
    }
}

/// Vertex struct for specifying triangle geometry
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
