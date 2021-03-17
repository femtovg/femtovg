use std::rc::Rc;

use crate::{
    ErrorKind,
    ImageFlags,
    ImageInfo,
    ImageSource,
    PixelFormat,
};

use glow::HasContext;

pub struct GlTexture {
    context: Rc<glow::Context>,
    id: <glow::Context as glow::HasContext>::Texture,
    info: ImageInfo,
}

impl GlTexture {
    pub fn new(context: &Rc<glow::Context>, info: ImageInfo, opengles_2_0: bool) -> Result<Self, ErrorKind> {
        //let size = src.dimensions();

        let mut texture = Self {
            context: context.clone(),
            id: Default::default(),
            info: info,
        };

        unsafe {
            texture.id = context.create_texture().unwrap();
            context.bind_texture(glow::TEXTURE_2D, Some(texture.id));
            context.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            if !opengles_2_0 {
                context.pixel_store_i32(glow::UNPACK_ROW_LENGTH, texture.info.width() as i32);
                context.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, 0);
                context.pixel_store_i32(glow::UNPACK_SKIP_ROWS, 0);
            }
        }

        match info.format() {
            PixelFormat::Gray8 => unsafe {
                let internal_format = if opengles_2_0 { glow::LUMINANCE } else { glow::R8 };
                let format = if opengles_2_0 { internal_format } else { glow::RED };

                context.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    internal_format as i32,
                    texture.info.width() as i32,
                    texture.info.height() as i32,
                    0,
                    format,
                    glow::UNSIGNED_BYTE,
                    None, //data.buf().as_ptr() as *const GLvoid
                );
            },
            PixelFormat::Rgb8 => unsafe {
                context.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGB as i32,
                    texture.info.width() as i32,
                    texture.info.height() as i32,
                    0,
                    glow::RGB,
                    glow::UNSIGNED_BYTE,
                    None,
                    //data.buf().as_ptr() as *const GLvoid
                );
            },
            PixelFormat::Rgba8 => unsafe {
                context.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA as i32,
                    texture.info.width() as i32,
                    texture.info.height() as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    None,
                    //data.buf().as_ptr() as *const GLvoid
                );
            },
        }

        let flags = texture.info.flags();

        if flags.contains(ImageFlags::GENERATE_MIPMAPS) {
            if flags.contains(ImageFlags::NEAREST) {
                unsafe {
                    context.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_MIN_FILTER,
                        glow::NEAREST_MIPMAP_NEAREST as i32,
                    );
                }
            } else {
                unsafe {
                    context.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_MIN_FILTER,
                        glow::LINEAR_MIPMAP_LINEAR as i32,
                    );
                }
            }
        } else {
            if flags.contains(ImageFlags::NEAREST) {
                unsafe {
                    context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
                }
            } else {
                unsafe {
                    context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
                }
            }
        }

        if flags.contains(ImageFlags::NEAREST) {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
            }
        } else {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            }
        }

        if flags.contains(ImageFlags::REPEAT_X) {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
            }
        } else {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            }
        }

        if flags.contains(ImageFlags::REPEAT_Y) {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
            }
        } else {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            }
        }

        unsafe {
            context.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            if !opengles_2_0 {
                context.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
                context.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, 0);
                context.pixel_store_i32(glow::UNPACK_SKIP_ROWS, 0);
            }
        }

        if flags.contains(ImageFlags::GENERATE_MIPMAPS) {
            unsafe {
                context.generate_mipmap(glow::TEXTURE_2D);
                //glow::TexParameteri(glow::TEXTURE_2D, glow::GENERATE_MIPMAP, glow::TRUE);
            }
        }

        unsafe {
            context.bind_texture(glow::TEXTURE_2D, None);
        }

        Ok(texture)
    }

    pub fn id(&self) -> <glow::Context as glow::HasContext>::Texture {
        self.id
    }

    pub fn update(&mut self, src: ImageSource, x: usize, y: usize, opengles_2_0: bool) -> Result<(), ErrorKind> {
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
            self.context.bind_texture(glow::TEXTURE_2D, Some(self.id));
            self.context.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            if !opengles_2_0 {
                self.context.pixel_store_i32(glow::UNPACK_ROW_LENGTH, size.0 as i32);
            }
        }

        match src {
            ImageSource::Gray(data) => unsafe {
                let format = if opengles_2_0 { glow::LUMINANCE } else { glow::R8 };

                self.context.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    size.0 as i32,
                    size.1 as i32,
                    format,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(data.buf().align_to().1),
                );
            },
            ImageSource::Rgb(data) => unsafe {
                self.context.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    size.0 as i32,
                    size.1 as i32,
                    glow::RGB,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(data.buf().align_to().1),
                );
            },
            ImageSource::Rgba(data) => unsafe {
                self.context.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    size.0 as i32,
                    size.1 as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(data.buf().align_to().1),
                );
            },
            #[cfg(target_arch = "wasm32")]
            ImageSource::HtmlImageElement(image_element) => unsafe {
                self.context.tex_sub_image_2d_with_html_image(
                    glow::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    image_element,
                )
            },
        }

        if self.info.flags().contains(ImageFlags::GENERATE_MIPMAPS) {
            unsafe {
                self.context.generate_mipmap(glow::TEXTURE_2D);
                //glow::TexParameteri(glow::TEXTURE_2D, glow::GENERATE_MIPMAP, glow::TRUE);
            }
        }

        unsafe {
            self.context.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            if !opengles_2_0 {
                self.context.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
            }
            //glow::PixelStorei(glow::UNPACK_SKIP_PIXELS, 0);
            //glow::PixelStorei(glow::UNPACK_SKIP_ROWS, 0);
            self.context.bind_texture(glow::TEXTURE_2D, None);
        }

        Ok(())
    }

    pub fn delete(self) {
        unsafe {
            self.context.delete_texture(self.id);
        }
    }

    pub fn info(&self) -> ImageInfo {
        self.info
    }
}
