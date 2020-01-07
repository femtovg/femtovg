
use image::DynamicImage;

use super::{Paint, Contour, Scissor, ImageId, Vertex, ImageFlags};

mod gl;
pub use gl::GlRenderer;

pub enum TextureType {
	Rgba,
	Alpha
}

pub trait Renderer {
	fn edge_antialiasing(&self) -> bool;
    fn render_viewport(&mut self, window_width: f32, window_height: f32);
    fn render_flush(&mut self);
    
    fn render_fill(&mut self, paint: &Paint, scissor: &Scissor, fringe_width: f32, bounds: [f32; 4], contours: &[Contour]);
    fn render_stroke(&mut self, paint: &Paint, scissor: &Scissor, fringe_width: f32, stroke_width: f32, contours: &[Contour]);
    fn render_triangles(&mut self, paint: &Paint, scissor: &Scissor, verts: &[Vertex]);
    
    fn create_texture(&mut self, texture_type: TextureType, width: u32, height: u32, flags: ImageFlags) -> ImageId;
    fn update_texture(&mut self, id: ImageId, image: &DynamicImage, x: u32, y: u32, w: u32, h: u32);
    fn delete_texture(&mut self, id: ImageId);
}
