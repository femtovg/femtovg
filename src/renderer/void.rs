#![allow(unused_variables)]

use image::{
    DynamicImage,
    GenericImageView
};

use crate::{
    Result,
    ErrorKind
};

use super::{
    Renderer,
    ImageFormat,
    Command,
    ImageFlags,
    Vertex,
    ImageStore,
    ImageInfo,
    Image
};

/// Void renderer used for testing
pub struct Void;

impl Renderer for Void {
    type Image = VoidImage;

    fn set_size(&mut self, width: u32, height: u32, dpi: f32) {}

    fn render(&mut self, images: &ImageStore<VoidImage>, verts: &[Vertex], commands: &[Command]) {}

    fn create_image(&mut self, data: &DynamicImage, flags: ImageFlags) -> Result<Self::Image> {
        let size = data.dimensions();

        Ok(VoidImage {
            info: ImageInfo {
                width: size.0 as usize,
                height: size.1 as usize,
                flags: flags,
                format: ImageFormat::Rgba
            }
        })
    }

    fn update_image(&mut self, image: &mut Self::Image, data: &DynamicImage, x: usize, y: usize) -> Result<()> {
        let size = data.dimensions();

        if x + size.0 as usize > image.info.width {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        if y + size.1 as usize > image.info.height {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        Ok(())
    }

    fn delete_image(&mut self, image: Self::Image) {}

    fn screenshot(&mut self) -> Option<DynamicImage> { None }
}

pub struct VoidImage {
    info: ImageInfo
}

impl Image for VoidImage {
    fn info(&self) -> ImageInfo {
        self.info
    }
}
