use std::ptr;

use crate::{ErrorKind, ImageFlags, ImageInfo, ImageSource, PixelFormat};

use super::gl;
use super::gl::types::*;

pub struct Texture {
    id: GLuint,
    info: ImageInfo,
}

impl Texture {
    pub fn new(info: ImageInfo, opengles: bool) -> Result<Self, ErrorKind> {
        //let size = src.dimensions();

        let mut texture = Texture { id: 0, info: info };

        unsafe {
            gl::GenTextures(1, &mut texture.id);
            gl::BindTexture(gl::TEXTURE_2D, texture.id);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, texture.info.width() as i32);
            gl::PixelStorei(gl::UNPACK_SKIP_PIXELS, 0);
            gl::PixelStorei(gl::UNPACK_SKIP_ROWS, 0);
        }

        match info.format() {
            PixelFormat::Gray8 => unsafe {
                //let format = if opengles { gl::RED } else { gl::RED };
                let internal_format = if opengles { gl::LUMINANCE } else { gl::R8 };

                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    internal_format as i32,
                    texture.info.width() as i32,
                    texture.info.height() as i32,
                    0,
                    gl::RED,
                    gl::UNSIGNED_BYTE,
                    ptr::null(), //data.buf().as_ptr() as *const GLvoid
                );
            },
            PixelFormat::Rgb8 => unsafe {
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGB as i32,
                    texture.info.width() as i32,
                    texture.info.height() as i32,
                    0,
                    gl::RGB,
                    gl::UNSIGNED_BYTE,
                    ptr::null(),
                    //data.buf().as_ptr() as *const GLvoid
                );
            },
            PixelFormat::Rgba8 => unsafe {
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGBA as i32,
                    texture.info.width() as i32,
                    texture.info.height() as i32,
                    0,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    ptr::null(),
                    //data.buf().as_ptr() as *const GLvoid
                );
            },
        }

        let flags = texture.info.flags();

        if flags.contains(ImageFlags::GENERATE_MIPMAPS) {
            if flags.contains(ImageFlags::NEAREST) {
                unsafe {
                    gl::TexParameteri(
                        gl::TEXTURE_2D,
                        gl::TEXTURE_MIN_FILTER,
                        gl::NEAREST_MIPMAP_NEAREST as i32,
                    );
                }
            } else {
                unsafe {
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR_MIPMAP_LINEAR as i32);
                }
            }
        } else {
            if flags.contains(ImageFlags::NEAREST) {
                unsafe {
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
                }
            } else {
                unsafe {
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
                }
            }
        }

        if flags.contains(ImageFlags::NEAREST) {
            unsafe {
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
            }
        } else {
            unsafe {
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            }
        }

        if flags.contains(ImageFlags::REPEAT_X) {
            unsafe {
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT as i32);
            }
        } else {
            unsafe {
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            }
        }

        if flags.contains(ImageFlags::REPEAT_Y) {
            unsafe {
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT as i32);
            }
        } else {
            unsafe {
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            }
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

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, 0);
        }

        Ok(texture)
    }

    pub fn id(&self) -> GLuint {
        self.id
    }

    pub fn update(&mut self, src: ImageSource, x: usize, y: usize, opengles: bool) -> Result<(), ErrorKind> {
        let size = src.dimensions();

        if x + size.0 > self.info.width() {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        if y + size.1 > self.info.height() {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        if self.info.format() != src.format() {
            return Err(ErrorKind::ImageUpdateWithDifferentFormat);
        }

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.id);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, size.0 as i32);
        }

        match src {
            ImageSource::Gray(data) => unsafe {
                //let format = if opengles { gl::LUMINANCE } else { gl::RED };

                gl::TexSubImage2D(
                    gl::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    size.0 as i32,
                    size.1 as i32,
                    gl::R8,
                    gl::UNSIGNED_BYTE,
                    data.buf().as_ptr() as *const GLvoid,
                );
            },
            ImageSource::Rgb(data) => unsafe {
                gl::TexSubImage2D(
                    gl::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    size.0 as i32,
                    size.1 as i32,
                    gl::RGB,
                    gl::UNSIGNED_BYTE,
                    data.buf().as_ptr() as *const GLvoid,
                );
            },
            ImageSource::Rgba(data) => unsafe {
                gl::TexSubImage2D(
                    gl::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    size.0 as i32,
                    size.1 as i32,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    data.buf().as_ptr() as *const GLvoid,
                );
            },
        }

        if self.info.flags().contains(ImageFlags::GENERATE_MIPMAPS) {
            unsafe {
                gl::GenerateMipmap(gl::TEXTURE_2D);
                //gl::TexParameteri(gl::TEXTURE_2D, gl::GENERATE_MIPMAP, gl::TRUE);
            }
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

    pub fn info(&self) -> ImageInfo {
        self.info
    }
}
