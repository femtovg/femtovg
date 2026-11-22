use std::{borrow::Cow, rc::Rc};

use glow::HasContext;

use crate::{ErrorKind, ImageFlags, ImageInfo, ImageSource, PixelFormat};

#[derive(Debug)]
pub struct GlTexture {
    id: <glow::Context as glow::HasContext>::Texture,
    info: ImageInfo,
    owned: bool,
    pub(super) external: bool,
}

impl GlTexture {
    pub fn new_from_native_texture(
        texture: <glow::Context as glow::HasContext>::Texture,
        info: ImageInfo,
        external: bool,
    ) -> Self {
        Self {
            id: texture,
            info,
            owned: false,
            external,
        }
    }
    pub fn new(context: &Rc<glow::Context>, info: ImageInfo, opengles_2_0: bool) -> Result<Self, ErrorKind> {
        Self::new_with(context, info, opengles_2_0, None)
    }

    /// [`Self::new`] holding `pixels`, the whole image in its format, instead of zeros.
    pub fn new_with(
        context: &Rc<glow::Context>,
        info: ImageInfo,
        opengles_2_0: bool,
        pixels: Option<&[u8]>,
    ) -> Result<Self, ErrorKind> {
        //let size = src.dimensions();

        let id = unsafe {
            let id = context.create_texture().unwrap();
            context.bind_texture(glow::TEXTURE_2D, Some(id));
            context.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            if !opengles_2_0 {
                context.pixel_store_i32(glow::UNPACK_ROW_LENGTH, info.width() as i32);
                context.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, 0);
                context.pixel_store_i32(glow::UNPACK_SKIP_ROWS, 0);
            }
            id
        };

        let texture = Self {
            id,
            info,
            owned: true,
            external: false,
        };

        // Upload a zeroed buffer instead of NULL so the texture's contents are
        // defined from creation: glTexImage2D with NULL leaves them undefined,
        // and embedded GL drivers hand back recycled, unscrubbed memory that
        // edge-of-quad NEAREST lookups can pick up as stray pixels (#310).
        // Desktop drivers zero pages in practice, which is why this went
        // unnoticed; the wgpu backend guarantees zero-initialized textures, so
        // this also aligns the two backends.
        let bytes_per_pixel = match info.format() {
            PixelFormat::Gray8 => 1,
            PixelFormat::Rgb8 => 3,
            PixelFormat::Rgba8 => 4,
        };
        let pixels: Cow<[u8]> = match pixels {
            Some(pixels) => pixels.into(),
            None => vec![0u8; info.width() * info.height() * bytes_per_pixel].into(),
        };

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
                    glow::PixelUnpackData::Slice(Some(&pixels)),
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
                    glow::PixelUnpackData::Slice(Some(&pixels)),
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
                    glow::PixelUnpackData::Slice(Some(&pixels)),
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
        } else if flags.contains(ImageFlags::NEAREST) {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            }
        } else {
            unsafe {
                context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
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

    pub fn update(
        &mut self,
        context: &Rc<glow::Context>,
        src: ImageSource,
        x: usize,
        y: usize,
        opengles_2_0: bool,
    ) -> Result<(), ErrorKind> {
        src.check_update(&self.info, x, y)?;

        unsafe {
            context.bind_texture(glow::TEXTURE_2D, Some(self.id));
            context.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        }

        match src {
            ImageSource::Gray(data) => {
                let format = if opengles_2_0 { glow::LUMINANCE } else { glow::RED };
                upload_rows(context, data, x, y, format, opengles_2_0);
            }
            ImageSource::Rgb(data) => upload_rows(context, data, x, y, glow::RGB, opengles_2_0),
            ImageSource::Rgba(data) => upload_rows(context, data, x, y, glow::RGBA, opengles_2_0),
            #[cfg(wasm_unknown)]
            ImageSource::HtmlImageElement(image_element) => unsafe {
                context.tex_sub_image_2d_with_html_image(
                    glow::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    image_element,
                )
            },
            #[cfg(wasm_unknown)]
            ImageSource::HtmlCanvasElement(canvas_element) => unsafe {
                context.tex_sub_image_2d_with_html_canvas(
                    glow::TEXTURE_2D,
                    0,
                    x as i32,
                    y as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    canvas_element,
                )
            },
        }

        if self.info.flags().contains(ImageFlags::GENERATE_MIPMAPS) {
            unsafe {
                context.generate_mipmap(glow::TEXTURE_2D);
                //glow::TexParameteri(glow::TEXTURE_2D, glow::GENERATE_MIPMAP, glow::TRUE);
            }
        }

        unsafe {
            context.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            if !opengles_2_0 {
                context.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
            }
            //glow::PixelStorei(glow::UNPACK_SKIP_PIXELS, 0);
            //glow::PixelStorei(glow::UNPACK_SKIP_ROWS, 0);
            context.bind_texture(glow::TEXTURE_2D, None);
        }

        Ok(())
    }

    pub fn delete(self, context: &Rc<glow::Context>) {
        if self.owned {
            unsafe {
                context.delete_texture(self.id);
            }
        }
        // See `OpenGl::create_image_from_native_texture`.
        #[cfg(wasm_unknown)]
        if !self.owned && !self.external {
            context.unregister_external_texture(self.id);
        }
    }

    pub fn info(&self) -> ImageInfo {
        self.info
    }
}

/// Uploads `data` into the bound texture at `x`, `y`, reading rows `data.stride()` pixels apart. The
/// GLES 2.0 path (also taken on WebGL) sets no `UNPACK_ROW_LENGTH`, so there a strided source is
/// packed first.
fn upload_rows<T: Copy>(
    context: &glow::Context,
    data: imgref::ImgRef<T>,
    x: usize,
    y: usize,
    format: u32,
    opengles_2_0: bool,
) {
    let pixels = if opengles_2_0 && data.stride() != data.width() {
        Cow::Owned(data.pixels().collect())
    } else {
        Cow::Borrowed(*data.buf())
    };
    unsafe {
        if !opengles_2_0 {
            context.pixel_store_i32(glow::UNPACK_ROW_LENGTH, data.stride() as i32);
        }
        context.tex_sub_image_2d(
            glow::TEXTURE_2D,
            0,
            x as i32,
            y as i32,
            data.width() as i32,
            data.height() as i32,
            format,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(pixels.align_to().1)),
        );
    }
}
