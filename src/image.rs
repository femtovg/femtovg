
use ::image::DynamicImage;
use bitflags::bitflags;
use generational_arena::{Arena, Index};

use crate::{
    Result,
    ErrorKind,
    Renderer
};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ImageId(pub Index);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ImageFormat {
    Rgb,
    Rgba,
    Alpha
}

bitflags! {
    pub struct ImageFlags: u32 {
        const GENERATE_MIPMAPS = 1;     // Generate mipmaps during creation of the image.
        const REPEAT_X = 1 << 1;        // Repeat image in X direction.
        const REPEAT_Y = 1 << 2;        // Repeat image in Y direction.
        const FLIP_Y = 1 << 3;          // Flips (inverses) image in Y direction when rendered.
        const PREMULTIPLIED = 1 << 4;   // Image data has premultiplied alpha.
        const NEAREST = 1 << 5;         // Image interpolation is Nearest instead Linear
    }
}

#[derive(Copy, Clone)]
pub struct ImageInfo {
    flags: ImageFlags,
    width: usize,
    height: usize,
    format: ImageFormat
}

impl ImageInfo {
    pub fn new(flags: ImageFlags, width: usize, height: usize, format: ImageFormat) -> Self {
        Self { flags, width, height, format }
    }

    pub fn flags(&self) -> ImageFlags {
        self.flags
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn format(&self) -> ImageFormat {
        self.format
    }

    pub fn set_format(&mut self, format: ImageFormat) {
        self.format = format;
    }
}

pub trait Image {
    fn info(&self) -> ImageInfo;
}

pub struct ImageStore<T>(Arena<T>);

impl<T: Image> Default for ImageStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Image> ImageStore<T> {
    pub fn new() -> Self {
        Self(Arena::new())
    }

    pub fn add<R: Renderer<Image = T>>(&mut self, renderer: &mut R, data: &DynamicImage, flags: ImageFlags) -> Result<ImageId> {
        let image = renderer.create_image(data, flags)?;

        Ok(ImageId(self.0.insert(image)))
    }

    pub fn get(&self, id: ImageId) -> Option<&T> {
        self.0.get(id.0)
    }

    pub fn update<R: Renderer<Image = T>>(&mut self, renderer: &mut R, id: ImageId, image_src: &DynamicImage, x: usize, y: usize) -> Result<()> {
        if let Some(image) = self.0.get_mut(id.0) {
            renderer.update_image(image, image_src, x, y)?;
        } else {
            return Err(ErrorKind::ImageIdNotFound);
        }

        Ok(())
    }

    pub fn remove<R: Renderer<Image = T>>(&mut self, renderer: &mut R, id: ImageId) {
        if let Some(image) = self.0.remove(id.0) {
            renderer.delete_image(image);
        }
    }

    pub fn clear<R: Renderer<Image = T>>(&mut self, renderer: &mut R) {
        for (_idx, image) in self.0.drain() {
            renderer.delete_image(image);
        }
    }
}
