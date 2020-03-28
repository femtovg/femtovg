
use image::{DynamicImage, GenericImageView};

use crate::{
    Result,
    ErrorKind,
    ImageFlags,
    renderer::{
        Image,
        ImageInfo,
        ImageFormat
    }
};

use super::gl;
use super::gl::types::*;

pub struct Texture {
    id: GLuint,
    info: ImageInfo
}

impl Texture {
    pub fn new(image: &DynamicImage, flags: ImageFlags, opengles: bool) -> Result<Self> {
        let size = image.dimensions();

        let mut texture = Texture {
            id: 0,
            info: ImageInfo {
                width: size.0 as usize,
                height: size.1 as usize,
                flags: flags,
                format: ImageFormat::Rgba
            }
        };

        unsafe {
            gl::GenTextures(1, &mut texture.id);
            gl::BindTexture(gl::TEXTURE_2D, texture.id);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, texture.info.width as i32);
            gl::PixelStorei(gl::UNPACK_SKIP_PIXELS, 0);
            gl::PixelStorei(gl::UNPACK_SKIP_ROWS, 0);
        }

        match image {
            DynamicImage::ImageLuma8(gray_image) => unsafe {
                let format = if opengles { gl::LUMINANCE } else { gl::RED };

                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    format as i32,
                    texture.info.width as i32,
                    texture.info.height as i32,
                    0,
                    format,
                    gl::UNSIGNED_BYTE,
                    gray_image.as_ref().as_ptr() as *const GLvoid
                );

                texture.info.format = ImageFormat::Alpha;
            },
            DynamicImage::ImageRgb8(rgb_image) => unsafe {
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGB as i32,
                    texture.info.width as i32,
                    texture.info.height as i32,
                    0,
                    gl::RGB,
                    gl::UNSIGNED_BYTE,
                    rgb_image.as_ref().as_ptr() as *const GLvoid
                );

                texture.info.format = ImageFormat::Rgb;
            },
            DynamicImage::ImageRgba8(rgba_image) => unsafe {
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGBA as i32,
                    texture.info.width as i32,
                    texture.info.height as i32,
                    0,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    rgba_image.as_ref().as_ptr() as *const GLvoid
                );

                texture.info.format = ImageFormat::Rgba;
            },
            DynamicImage::ImageLumaA8(_) =>
                return Err(ErrorKind::UnsuportedImageFromat(String::from("ImageLumaA8"))),
            DynamicImage::ImageBgr8(_) =>
                return Err(ErrorKind::UnsuportedImageFromat(String::from("ImageBgr8"))),
            DynamicImage::ImageBgra8(_) =>
                return Err(ErrorKind::UnsuportedImageFromat(String::from("ImageBgra8"))),
            _ => return Err(ErrorKind::UnsuportedImageFromat(String::from("Unknown image format"))),
        }

        if flags.contains(ImageFlags::GENERATE_MIPMAPS) {
            if flags.contains(ImageFlags::NEAREST) {
                unsafe { gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST_MIPMAP_NEAREST as i32); }
            } else {
                unsafe { gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR_MIPMAP_LINEAR as i32); }
            }
        } else {
            if flags.contains(ImageFlags::NEAREST) {
                unsafe { gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32); }
            } else {
                unsafe { gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32); }
            }
        }

        if flags.contains(ImageFlags::NEAREST) {
            unsafe { gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32); }
        } else {
            unsafe { gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32); }
        }

        if flags.contains(ImageFlags::REPEAT_X) {
            unsafe { gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT as i32); }
        } else {
            unsafe { gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32); }
        }

        if flags.contains(ImageFlags::REPEAT_Y) {
            unsafe { gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT as i32); }
        } else {
            unsafe { gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32); }
        }

        unsafe {
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 4);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, 0);
            gl::PixelStorei(gl::UNPACK_SKIP_PIXELS, 0);
            gl::PixelStorei(gl::UNPACK_SKIP_ROWS, 0);
        }

        if flags.contains(ImageFlags::GENERATE_MIPMAPS) {
            unsafe {
                gl::GenerateMipmap(gl::TEXTURE_2D);
                //gl::TexParameteri(gl::TEXTURE_2D, gl::GENERATE_MIPMAP, gl::TRUE);
            }
        }

        unsafe { gl::BindTexture(gl::TEXTURE_2D, 0); }

        Ok(texture)
    }

    pub fn id(&self) -> GLuint {
        self.id
    }

    pub fn update(&mut self, image: &DynamicImage, x: usize, y: usize, opengles: bool) -> Result<()> {
        let size = image.dimensions();

        if x + size.0 as usize > self.info.width {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        if y + size.1 as usize > self.info.height {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.id);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, size.0 as i32);
        }

        match image {
            DynamicImage::ImageLuma8(gray_image) => unsafe {
                let format = if opengles { gl::LUMINANCE } else { gl::RED };

                if self.info.format != ImageFormat::Alpha {
                    return Err(ErrorKind::ImageUpdateWithDifferentFormat);
                }

                gl::TexSubImage2D(
                    gl::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    size.0 as i32,
                    size.1 as i32,
                    format,
                    gl::UNSIGNED_BYTE,
                    gray_image.as_ref().as_ptr() as *const GLvoid
                );
            }
            DynamicImage::ImageRgb8(rgb_image) => unsafe {
                if self.info.format != ImageFormat::Rgb {
                    return Err(ErrorKind::ImageUpdateWithDifferentFormat);
                }

                gl::TexSubImage2D(
                    gl::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    size.0 as i32,
                    size.1 as i32,
                    gl::RGB,
                    gl::UNSIGNED_BYTE,
                    rgb_image.as_ref().as_ptr() as *const GLvoid
                );
            }
            DynamicImage::ImageRgba8(rgba_image) => unsafe {
                if self.info.format != ImageFormat::Rgba {
                    return Err(ErrorKind::ImageUpdateWithDifferentFormat);
                }

                gl::TexSubImage2D(
                    gl::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    size.0 as i32,
                    size.1 as i32,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    rgba_image.as_ref().as_ptr() as *const GLvoid
                );
            }
            DynamicImage::ImageLumaA8(_) =>
                return Err(ErrorKind::UnsuportedImageFromat(String::from("ImageLumaA8"))),
            DynamicImage::ImageBgr8(_) =>
                return Err(ErrorKind::UnsuportedImageFromat(String::from("ImageBgr8"))),
            DynamicImage::ImageBgra8(_) =>
                return Err(ErrorKind::UnsuportedImageFromat(String::from("ImageBgra8"))),
            _ => return Err(ErrorKind::UnsuportedImageFromat(String::from("Unknown image format"))),
        }

        unsafe {
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 4);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, 0);
            //gl::PixelStorei(gl::UNPACK_SKIP_PIXELS, 0);
            //gl::PixelStorei(gl::UNPACK_SKIP_ROWS, 0);
            gl::BindTexture(gl::TEXTURE_2D, 0);
        }

        Ok(())
    }

    pub fn delete(self) {
        unsafe {
            gl::DeleteTextures(1, &self.id);
        }
    }
}

impl Image for Texture {
    fn info(&self) -> ImageInfo {
        self.info
    }
}
