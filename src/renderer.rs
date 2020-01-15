
use image::DynamicImage;

use crate::{Color, Paint, Path, Scissor, ImageId, Vertex, ImageFlags};

mod void;
pub use void::Void;

pub mod gpu_renderer;

pub trait Renderer {
    fn flush(&mut self);
    fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color);
    fn set_size(&mut self, width: u32, height: u32, dpi: f32);

    fn fill(&mut self, paint: &Paint, scissor: &Scissor, path: &Path);
    fn stroke(&mut self, paint: &Paint, scissor: &Scissor, path: &Path);
    fn triangles(&mut self, paint: &Paint, scissor: &Scissor, verts: &[Vertex]);

    fn create_image(&mut self, image: &DynamicImage, flags: ImageFlags) -> ImageId;
    fn update_image(&mut self, id: ImageId, image: &DynamicImage, x: u32, y: u32);
    fn delete_image(&mut self, id: ImageId);

    fn screenshot(&mut self) -> Option<DynamicImage> {
        None
    }
}
