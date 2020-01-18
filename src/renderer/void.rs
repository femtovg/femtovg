#![allow(unused_variables)]

use image::DynamicImage;

use crate::geometry::Transform2D;
use crate::{Path, Color, Paint, Scissor};
use super::{Renderer, ImageId, Vertex, ImageFlags};

/// Void renderer. Intended for testing and documentation.
#[derive(Default)]
pub struct Void;

impl Renderer for Void {
    fn flush(&mut self) {}
    fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {}
    fn set_size(&mut self, width: u32, height: u32, dpi: f32) {}

    fn fill(&mut self, path: &Path, paint: &Paint, scissor: &Scissor, transform: &Transform2D) {}
    fn stroke(&mut self, path: &Path, paint: &Paint, scissor: &Scissor, transform: &Transform2D) {}
    fn triangles(&mut self, verts: &[Vertex], paint: &Paint, scissor: &Scissor, transform: &Transform2D) {}

    fn create_image(&mut self, image: &DynamicImage, flags: ImageFlags) -> ImageId {
        ImageId(0)
    }

    fn update_image(&mut self, id: ImageId, image: &DynamicImage, x: u32, y: u32) {}
    fn delete_image(&mut self, id: ImageId) {}
}
