use bitflags::bitflags;
use generational_arena::{
    Arena,
    Index,
};
use imgref::*;
use rgb::alt::GRAY8;
use rgb::*;

#[cfg(feature = "image-loading")]
use ::image::DynamicImage;

#[cfg(feature = "image-loading")]
use std::convert::TryFrom;

use crate::{
    ErrorKind,
    Renderer,
};

/// An image handle.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ImageId(pub Index);

/// Image format: `Rgb8`, `Rgba8`, `Gray8`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum PixelFormat {
    Rgb8,
    Rgba8,
    Gray8,
}

bitflags! {
    /// Image flags (eg. repeat, flip, mipmaps, etc.)
    pub struct ImageFlags: u32 {
        const GENERATE_MIPMAPS = 1;     // Generate mipmaps during creation of the image.
        const REPEAT_X = 1 << 1;        // Repeat image in X direction.
        const REPEAT_Y = 1 << 2;        // Repeat image in Y direction.
        const FLIP_Y = 1 << 3;          // Flips (inverses) image in Y direction when rendered.
        const PREMULTIPLIED = 1 << 4;   // Image data has premultiplied alpha.
        const NEAREST = 1 << 5;         // Image interpolation is Nearest instead Linear
    }
}

/// Image source
#[derive(Copy, Clone, Debug)]
#[non_exhaustive]
pub enum ImageSource<'a> {
    Rgb(ImgRef<'a, RGB8>),
    Rgba(ImgRef<'a, RGBA8>),
    Gray(ImgRef<'a, GRAY8>),
    #[cfg(target_arch = "wasm32")]
    HtmlImageElement(&'a web_sys::HtmlImageElement),
}

impl ImageSource<'_> {
    /// Source format
    pub fn format(&self) -> PixelFormat {
        match self {
            Self::Rgb(_) => PixelFormat::Rgb8,
            Self::Rgba(_) => PixelFormat::Rgba8,
            Self::Gray(_) => PixelFormat::Gray8,
            #[cfg(target_arch = "wasm32")]
            Self::HtmlImageElement(_) => PixelFormat::Rgba8,
        }
    }

    /// Source dimensions
    pub fn dimensions(&self) -> (usize, usize) {
        // TODO: Create size struct and use it here and in ImageInfo.

        match self {
            Self::Rgb(imgref) => (imgref.width(), imgref.height()),
            Self::Rgba(imgref) => (imgref.width(), imgref.height()),
            Self::Gray(imgref) => (imgref.width(), imgref.height()),
            #[cfg(target_arch = "wasm32")]
            Self::HtmlImageElement(element) => (element.width() as usize, element.height() as usize),
        }
    }
}

impl<'a> From<ImgRef<'a, RGB8>> for ImageSource<'a> {
    fn from(src: ImgRef<'a, RGB8>) -> Self {
        Self::Rgb(src)
    }
}

impl<'a> From<ImgRef<'a, RGBA8>> for ImageSource<'a> {
    fn from(src: ImgRef<'a, RGBA8>) -> Self {
        Self::Rgba(src)
    }
}

impl<'a> From<ImgRef<'a, GRAY8>> for ImageSource<'a> {
    fn from(src: ImgRef<'a, GRAY8>) -> Self {
        Self::Gray(src)
    }
}

#[cfg(target_arch = "wasm32")]
impl<'a> From<&'a web_sys::HtmlImageElement> for ImageSource<'a> {
    fn from(src: &'a web_sys::HtmlImageElement) -> Self {
        Self::HtmlImageElement(src)
    }
}

#[cfg(feature = "image-loading")]
impl<'a> TryFrom<&'a DynamicImage> for ImageSource<'a> {
    type Error = ErrorKind;

    fn try_from(src: &'a DynamicImage) -> Result<Self, ErrorKind> {
        match src {
            ::image::DynamicImage::ImageLuma8(img) => {
                let src: Img<&[GRAY8]> =
                    Img::new(img.as_ref().as_pixels(), img.width() as usize, img.height() as usize);

                Ok(ImageSource::from(src))
            }
            ::image::DynamicImage::ImageRgb8(img) => {
                let src = Img::new(img.as_ref().as_rgb(), img.width() as usize, img.height() as usize);
                Ok(ImageSource::from(src))
            }
            ::image::DynamicImage::ImageRgba8(img) => {
                let src = Img::new(img.as_ref().as_rgba(), img.width() as usize, img.height() as usize);
                Ok(ImageSource::from(src))
            }
            // TODO: if format is not supported maybe we should convert it here,
            // Buut that is an expensive operation on the render thread that will remain hidden from the user
            _ => Err(ErrorKind::UnsuportedImageFromat),
        }
    }
}

/// Information about an image.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ImageInfo {
    flags: ImageFlags,
    width: usize,
    height: usize,
    format: PixelFormat,
}

impl ImageInfo {
    pub fn new(flags: ImageFlags, width: usize, height: usize, format: PixelFormat) -> Self {
        Self {
            flags,
            width,
            height,
            format,
        }
    }

    /// Image flags
    pub fn flags(&self) -> ImageFlags {
        self.flags
    }

    /// Image width in pixels
    pub fn width(&self) -> usize {
        self.width
    }

    /// Image height in pixels
    pub fn height(&self) -> usize {
        self.height
    }

    /// Image format
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    pub fn set_format(&mut self, format: PixelFormat) {
        self.format = format;
    }
}

pub struct ImageStore<T>(Arena<(ImageInfo, T)>);

impl<T> Default for ImageStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ImageStore<T> {
    pub fn new() -> Self {
        Self(Arena::new())
    }

    pub fn alloc<R: Renderer<Image = T>>(&mut self, renderer: &mut R, info: ImageInfo) -> Result<ImageId, ErrorKind> {
        let image = renderer.alloc_image(info)?;

        Ok(ImageId(self.0.insert((info, image))))
    }

    ///
    /// Reallocates the image without changing the id.
    ///
    pub fn realloc<R: Renderer<Image = T>>(
        &mut self,
        renderer: &mut R,
        id: ImageId,
        info: ImageInfo,
    ) -> Result<(), ErrorKind> {
        if let Some(old) = self.0.get_mut(id.0) {
            let new = renderer.alloc_image(info)?;
            old.0 = info;
            old.1 = new;
            Ok(())
        } else {
            Err(ErrorKind::ImageIdNotFound)
        }
    }

    pub fn get(&self, id: ImageId) -> Option<&T> {
        self.0.get(id.0).map(|inner| &inner.1)
    }

    pub fn get_mut(&mut self, id: ImageId) -> Option<&mut T> {
        self.0.get_mut(id.0).map(|inner| &mut inner.1)
    }

    pub fn update<R: Renderer<Image = T>>(
        &mut self,
        renderer: &mut R,
        id: ImageId,
        data: ImageSource,
        x: usize,
        y: usize,
    ) -> Result<(), ErrorKind> {
        if let Some(image) = self.0.get_mut(id.0) {
            renderer.update_image(&mut image.1, data, x, y)?;
            Ok(())
        } else {
            Err(ErrorKind::ImageIdNotFound)
        }
    }

    pub fn info(&self, id: ImageId) -> Option<ImageInfo> {
        self.0.get(id.0).map(|inner| inner.0)
    }

    pub fn remove<R: Renderer<Image = T>>(&mut self, renderer: &mut R, id: ImageId) {
        if let Some(image) = self.0.remove(id.0) {
            renderer.delete_image(image.1);
        }
    }

    pub fn clear<R: Renderer<Image = T>>(&mut self, renderer: &mut R) {
        for (_idx, image) in self.0.drain() {
            renderer.delete_image(image.1);
        }
    }
}
