#![allow(unused_variables)]

use rgb::RGBA8;
use imgref::ImgVec;

use crate::{
    Result,
    ErrorKind,
    ImageInfo,
    ImageStore,
    ImageSource,
};

use super::{
    Renderer,
    Command,
    Vertex,
    RenderTarget
};

/// Void renderer used for testing
pub struct Void;

impl Renderer for Void {
    type Image = VoidImage;

    fn set_size(&mut self, width: u32, height: u32, dpi: f32) {}

    fn render(&mut self, images: &ImageStore<VoidImage>, verts: &[Vertex], commands: &[Command]) {}

    fn alloc_image(&mut self, info: ImageInfo) -> Result<Self::Image> {
        Ok(VoidImage {
            info
        })
    }

    fn update_image(&mut self, image: &mut Self::Image, data: ImageSource, x: usize, y: usize) -> Result<()> {
        let size = data.dimensions();

        if x + size.0 > image.info.width() {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        if y + size.1 > image.info.height() {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        Ok(())
    }

    fn delete_image(&mut self, image: Self::Image) {}

    fn set_target(&mut self, images: &ImageStore<Self::Image>, target: RenderTarget) {}

    fn blur(&mut self, image: &mut Self::Image, passes: u8, x: usize, y: usize, width: usize, height: usize) {

    }

    fn screenshot(&mut self) -> Result<ImgVec<RGBA8>> {
        Ok(ImgVec::new(Vec::new(), 0, 0))
    }
}

pub struct VoidImage {
    info: ImageInfo
}
