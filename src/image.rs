use bitflags::bitflags;
use imgref::*;
use rgb::alt::GRAY8;
use rgb::*;
use slotmap::{DefaultKey, SlotMap};

#[cfg(feature = "image-loading")]
use ::image::DynamicImage;

#[cfg(feature = "image-loading")]
use std::convert::TryFrom;

use crate::{ErrorKind, Renderer};

/// An image handle.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ImageId(DefaultKey);

/// Specifies the format of an image's pixels.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum PixelFormat {
    /// 24-bit RGB image format (8 bits per channel)
    Rgb8,
    /// 32-bit RGBA image format (8 bits per channel, including alpha)
    Rgba8,
    /// 8-bit grayscale image format
    Gray8,
}

bitflags! {
    /// Represents a set of flags that modify the behavior of an image.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ImageFlags: u32 {
        /// Generates mipmaps during the creation of the image.
        const GENERATE_MIPMAPS = 1;
        /// Repeats the image in the X direction when rendered.
        const REPEAT_X = 1 << 1;
        /// Repeats the image in the Y direction when rendered.
        const REPEAT_Y = 1 << 2;
        /// Flips (inverses) the image in the Y direction when rendered.
        const FLIP_Y = 1 << 3;
        /// Indicates that the image data has premultiplied alpha.
        const PREMULTIPLIED = 1 << 4;
        /// Uses nearest-neighbor interpolation instead of linear interpolation when rendering the image.
        const NEAREST = 1 << 5;
    }
}

/// Represents the source of an image.
#[derive(Copy, Clone, Debug)]
#[non_exhaustive]
pub enum ImageSource<'a> {
    /// Image source with RGB image format (8 bits per channel)
    Rgb(ImgRef<'a, RGB8>),
    /// Image source with RGBA image format (8 bits per channel, including alpha)
    Rgba(ImgRef<'a, RGBA8>),
    /// Image source with 8-bit grayscale image format
    Gray(ImgRef<'a, GRAY8>),
    /// Image source referencing a HTML image element (only available on `wasm32` target)
    #[cfg(target_arch = "wasm32")]
    HtmlImageElement(&'a web_sys::HtmlImageElement),
}

impl ImageSource<'_> {
    /// Returns the format of the image source.
    pub fn format(&self) -> PixelFormat {
        match self {
            Self::Rgb(_) => PixelFormat::Rgb8,
            Self::Rgba(_) => PixelFormat::Rgba8,
            Self::Gray(_) => PixelFormat::Gray8,
            #[cfg(target_arch = "wasm32")]
            Self::HtmlImageElement(_) => PixelFormat::Rgba8,
        }
    }

    /// Returns the dimensions (width and height) of the image source.
    pub fn dimensions(&self) -> Size {
        match self {
            Self::Rgb(imgref) => Size::new(imgref.width(), imgref.height()),
            Self::Rgba(imgref) => Size::new(imgref.width(), imgref.height()),
            Self::Gray(imgref) => Size::new(imgref.width(), imgref.height()),
            #[cfg(target_arch = "wasm32")]
            Self::HtmlImageElement(element) => Size::new(element.width() as usize, element.height() as usize),
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
        Ok(match src {
            ::image::DynamicImage::ImageLuma8(img) => {
                let src: Img<&[GRAY8]> = Img::new(img.as_pixels(), img.width() as usize, img.height() as usize);
                ImageSource::from(src)
            }
            ::image::DynamicImage::ImageRgb8(img) => {
                let src = Img::new(img.as_rgb(), img.width() as usize, img.height() as usize);
                ImageSource::from(src)
            }
            ::image::DynamicImage::ImageRgba8(img) => {
                let src = Img::new(img.as_rgba(), img.width() as usize, img.height() as usize);
                ImageSource::from(src)
            }
            // TODO: if format is not supported maybe we should convert it here,
            // But that is an expensive operation on the render thread that will remain hidden from the user
            _ => return Err(ErrorKind::UnsupportedImageFormat),
        })
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Size {
    pub width: usize,
    pub height: usize,
}

impl Size {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }
}

/// Information about an image.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ImageInfo {
    flags: ImageFlags,
    size: Size,
    format: PixelFormat,
}

impl ImageInfo {
    /// Creates a new `ImageInfo` with the specified flags, width, height, and format.
    pub fn new(flags: ImageFlags, width: usize, height: usize, format: PixelFormat) -> Self {
        Self {
            flags,
            size: Size { width, height },
            format,
        }
    }

    /// Returns the image flags.
    pub fn flags(&self) -> ImageFlags {
        self.flags
    }

    /// Returns the image width in pixels.
    pub fn width(&self) -> usize {
        self.size.width
    }

    /// Returns the image height in pixels.
    pub fn height(&self) -> usize {
        self.size.height
    }

    /// Returns the image size (width and height) in pixels.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Returns the image format.
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// Sets the image format.
    pub fn set_format(&mut self, format: PixelFormat) {
        self.format = format;
    }
}

pub struct ImageStore<T>(SlotMap<DefaultKey, (ImageInfo, T)>);

impl<T> Default for ImageStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ImageStore<T> {
    pub fn new() -> Self {
        Self(SlotMap::new())
    }

    pub fn alloc<R: Renderer<Image = T>>(&mut self, renderer: &mut R, info: ImageInfo) -> Result<ImageId, ErrorKind> {
        let image = renderer.alloc_image(info)?;
        Ok(ImageId(self.0.insert((info, image))))
    }

    pub fn register_native_texture<R: Renderer<Image = T>>(
        &mut self,
        renderer: &mut R,
        texture: R::NativeTexture,
        info: ImageInfo,
    ) -> Result<ImageId, ErrorKind> {
        let image = renderer.create_image_from_native_texture(texture, info)?;
        Ok(ImageId(self.0.insert((info, image))))
    }

    // Reallocates the image without changing the id.
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
            renderer.delete_image(image.1, id);
        }
    }

    pub fn clear<R: Renderer<Image = T>>(&mut self, renderer: &mut R) {
        for (idx, image) in self.0.drain() {
            renderer.delete_image(image.1, ImageId(idx));
        }
    }
}

/// Specifies the type of filter to apply to images with `crate::Canvas::filter_image`.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum ImageFilter {
    /// Applies a Gaussian blur filter with the specified standard deviation.
    GaussianBlur {
        /// The standard deviation of the Gaussian blur filter.
        sigma: f32,
    },
}
