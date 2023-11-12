#![allow(unused_variables)]

use imgref::ImgVec;
use rgb::RGBA8;

use crate::{ErrorKind, ImageInfo, ImageSource, ImageStore};

use super::{Command, ImageId, Renderer, Vertex};

/// Void renderer used for testing
pub struct Void;

impl Renderer for Void {
    type Image = VoidImage;
    type NativeTexture = ();

    fn set_size(&mut self, width: u32, height: u32, dpi: f32) {}

    fn render(&mut self, images: &mut ImageStore<VoidImage>, verts: &[Vertex], commands: Vec<Command>) {}

    fn alloc_image(&mut self, info: ImageInfo) -> Result<Self::Image, ErrorKind> {
        Ok(VoidImage { info })
    }

    fn create_image_from_native_texture(
        &mut self,
        _native_texture: Self::NativeTexture,
        _info: ImageInfo,
    ) -> Result<Self::Image, ErrorKind> {
        Err(ErrorKind::UnsupportedImageFormat)
    }

    fn update_image(
        &mut self,
        image: &mut Self::Image,
        data: ImageSource,
        x: usize,
        y: usize,
    ) -> Result<(), ErrorKind> {
        let size = data.dimensions();

        if x + size.width > image.info.width() {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        if y + size.height > image.info.height() {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        Ok(())
    }

    fn delete_image(&mut self, image: Self::Image, _image_id: ImageId) {}

    fn screenshot(&mut self) -> Result<ImgVec<RGBA8>, ErrorKind> {
        Ok(ImgVec::new(Vec::new(), 0, 0))
    }
}

pub struct VoidImage {
    info: ImageInfo,
}
