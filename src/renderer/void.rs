#![allow(unused_variables)]

use image::DynamicImage;

use super::{
    Renderer,
    TextureType,
    Command,
    ImageFlags,
    Vertex,
    ImageId
};

use crate::Result;

// TODO: Void renderer should behave correctly when dealing with images.

/// Void renderer used for testing
pub struct Void;

impl Renderer for Void {
    fn set_size(&mut self, width: u32, height: u32, dpi: f32) {}

    fn render(&mut self, verts: &[Vertex], commands: &[Command]) {}

    fn create_image(&mut self, image: &DynamicImage, flags: ImageFlags) -> Result<ImageId> {
        Ok(ImageId(0))
    }

    fn update_image(&mut self, id: ImageId, image: &DynamicImage, x: u32, y: u32) -> Result<()> {
        Ok(())
    }

    fn delete_image(&mut self, id: ImageId) {}

    fn texture_flags(&self, id: ImageId) -> ImageFlags { ImageFlags::empty() }
    fn texture_size(&self, id: ImageId) -> (u32, u32) { (0,0) }
    fn texture_type(&self, id: ImageId) -> Option<TextureType> { None }

    fn screenshot(&mut self) -> Option<DynamicImage> { None }
}
