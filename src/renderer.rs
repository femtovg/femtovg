#![allow(dead_code)]

use image::DynamicImage;

use crate::{Color, Paint, Path, Scissor, ImageId, Vertex, ImageFlags};

mod void;
pub use void::Void;

pub mod gpu_renderer;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TextureType {
    Rgba,
    Alpha
}

pub trait Renderer {
    fn flush(&mut self);
    fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color);
    fn set_size(&mut self, width: u32, height: u32, dpi: f32);

    fn fill(&mut self, paint: &Paint, scissor: &Scissor, path: &Path);
    fn stroke(&mut self, paint: &Paint, scissor: &Scissor, path: &Path);
    fn triangles(&mut self, paint: &Paint, scissor: &Scissor, verts: &[Vertex]);

    fn create_texture(&mut self, texture_type: TextureType, width: u32, height: u32, flags: ImageFlags) -> ImageId;
    fn update_texture(&mut self, id: ImageId, image: &DynamicImage, x: u32, y: u32, w: u32, h: u32);
    fn delete_texture(&mut self, id: ImageId);
}
