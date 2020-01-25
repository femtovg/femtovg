#![allow(unused_variables)]

use std::fmt;
use std::error;

use image::DynamicImage;

use super::{
    Renderer,
    TextureType,
    Command,
    ImageFlags,
    Vertex,
    ImageId
};

/// Void renderer used for testing
pub struct Void;

impl Renderer for Void {
    type Error = VoidError;

    fn set_size(&mut self, width: u32, height: u32, dpi: f32) {}

    fn render(&mut self, verts: &[Vertex], commands: &[Command]) {}

    fn create_image(&mut self, image: &DynamicImage, flags: ImageFlags) -> Result<ImageId, VoidError> {
        Ok(ImageId(0))
    }

    fn update_image(&mut self, id: ImageId, image: &DynamicImage, x: u32, y: u32) {}
    fn delete_image(&mut self, id: ImageId) {}

    fn texture_flags(&self, id: ImageId) -> ImageFlags { ImageFlags::empty() }
    fn texture_size(&self, id: ImageId) -> (u32, u32) { (0,0) }
    fn texture_type(&self, id: ImageId) -> Option<TextureType> { None }

    fn screenshot(&mut self) -> Option<DynamicImage> { None }
}

#[derive(Debug)]
pub struct VoidError;

impl fmt::Display for VoidError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Void error")
    }
}

impl error::Error for VoidError {}
