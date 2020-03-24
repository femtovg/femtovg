#![allow(unused_variables)]

use image::{
    DynamicImage,
    GenericImageView
};

use super::{
    Renderer,
    TextureType,
    Command,
    ImageFlags,
    Vertex,
    ImageStore,
    ImageInfo,
    Image
};

use crate::Result;

// TODO: Void renderer should behave correctly when dealing with images.

/// Void renderer used for testing
pub struct Void;

impl Renderer for Void {
    type Image = VoidImage;

    fn set_size(&mut self, width: u32, height: u32, dpi: f32) {}

    fn render(&mut self, images: &ImageStore<Void>, verts: &[Vertex], commands: &[Command]) {}

    fn screenshot(&mut self) -> Option<DynamicImage> { None }
}

pub struct VoidImage {
    info: ImageInfo
}

impl Image<Void> for VoidImage {
    fn create(renderer: &mut Void, image: &DynamicImage, flags: ImageFlags) -> Result<VoidImage> {
        let size = image.dimensions();

        Ok(VoidImage {
            info: ImageInfo {
                width: size.0 as usize,
                height: size.1 as usize,
                flags: flags,
                format: TextureType::Rgba
            }
        })
    }

    fn update(&mut self, renderer: &mut Void, data: &DynamicImage, x: usize, y: usize) -> Result<()> {
        Ok(())
    }

    fn delete(self, renderer: &mut Void) {

    }

    fn info(&self) -> ImageInfo {
        self.info
    }
}
