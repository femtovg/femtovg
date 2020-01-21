
use std::ptr;
use std::mem;
use std::ops::DerefMut;
use std::ffi::{CStr, NulError, c_void};
use std::{error::Error, fmt};

use fnv::FnvHashMap;
use image::{DynamicImage, GenericImageView};

use super::{Command, Renderer, CommandType, Params, TextureType};
use crate::{Color, ImageFlags, FillRule, CompositeOperationState, BlendFactor};
use crate::renderer::{Vertex, ImageId};

mod shader;
use shader::{Shader, ShaderError};

mod uniform_array;
use uniform_array::UniformArray;

#[allow(clippy::all)]
mod gl {
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));
}

use gl::types::*;

struct Texture {
    tex: GLuint,
    width: u32,
    height: u32,
    flags: ImageFlags,
    tex_type: TextureType
}

pub struct OpenGl {
    debug: bool,
    antialias: bool,
    is_opengles: bool,
    view: [f32; 2],
    shader: Shader,
    vert_arr: GLuint,
    vert_buff: GLuint,
    last_texture_id: u32,
    textures: FnvHashMap<ImageId, Texture>
}

impl OpenGl {

    pub fn new<F>(load_fn: F) -> Result<Self, OpenGlError> where F: Fn(&'static str) -> *const c_void {
        let debug = true;
        let antialias = true;

        gl::load_with(load_fn);

        let frag_shader_src = include_str!("opengl/main-fs.glsl");
        let vert_shader_src = include_str!("opengl/main-vs.glsl");

        let shader = if antialias {
            Shader::new("#define EDGE_AA 1", vert_shader_src, frag_shader_src)?
        } else {
            Shader::new("", vert_shader_src, frag_shader_src)?
        };

        let mut opengl = OpenGl {
            debug: debug,
            antialias: antialias,
            is_opengles: false,
            view: [0.0, 0.0],
            shader: shader,
            vert_arr: 0,
            vert_buff: 0,
            last_texture_id: 0,
            textures: Default::default()
        };

        unsafe {
            let version = CStr::from_ptr(gl::GetString(gl::VERSION) as *mut i8);
            opengl.is_opengles = version.to_str().ok().map_or(false, |str| str.starts_with("OpenGL ES"));

            gl::GenVertexArrays(1, &mut opengl.vert_arr);
            gl::GenBuffers(1, &mut opengl.vert_buff);
        }

        Ok(opengl)
    }

    fn check_error(&self, label: &str) {
        if !self.debug { return }

        let err = unsafe { gl::GetError() };

        if err == gl::NO_ERROR { return; }

        let message = match err {
            gl::INVALID_ENUM => "Invalid enum",
            gl::INVALID_VALUE => "Invalid value",
            gl::INVALID_OPERATION => "Invalid operation",
            gl::OUT_OF_MEMORY => "Out of memory",
            gl::INVALID_FRAMEBUFFER_OPERATION => "Invalid framebuffer operation",
            _ => "Unknown error"
        };

        eprintln!("({}) Error on {} - {}", err, label, message);
    }

    fn gl_factor(factor: BlendFactor) -> GLenum {
        match factor {
            BlendFactor::Zero => gl::ZERO,
            BlendFactor::One => gl::ONE,
            BlendFactor::SrcColor => gl::SRC_COLOR,
            BlendFactor::OneMinusSrcColor => gl::ONE_MINUS_SRC_COLOR,
            BlendFactor::DstColor => gl::DST_COLOR,
            BlendFactor::OneMinusDstColor => gl::ONE_MINUS_DST_COLOR,
            BlendFactor::SrcAlpha => gl::SRC_ALPHA,
            BlendFactor::OneMinusSrcAlpha => gl::ONE_MINUS_SRC_ALPHA,
            BlendFactor::DstAlpha => gl::DST_ALPHA,
            BlendFactor::OneMinusDstAlpha => gl::ONE_MINUS_DST_ALPHA,
            BlendFactor::SrcAlphaSaturate => gl::SRC_ALPHA_SATURATE,
        }
    }

    fn set_composite_operation(&self, blend_state: CompositeOperationState) {
        unsafe {
            gl::BlendFuncSeparate(
                Self::gl_factor(blend_state.src_rgb),
                Self::gl_factor(blend_state.dst_rgb),
                Self::gl_factor(blend_state.src_alpha),
                Self::gl_factor(blend_state.dst_alpha)
            );
        }
    }

    fn convex_fill(&self, cmd: &Command, gpu_paint: Params) {
        self.set_uniforms(gpu_paint, cmd.image);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.fill_verts {
                unsafe { gl::DrawArrays(gl::TRIANGLE_FAN, start as i32, count as i32); }
            }

            if let Some((start, count)) = drawable.stroke_verts {
                unsafe { gl::DrawArrays(gl::TRIANGLE_STRIP, start as i32, count as i32); }
            }
        }

        self.check_error("convex_fill");
    }

    fn concave_fill(&self, cmd: &Command, stencil_paint: Params, fill_paint: Params) {
        unsafe {
            gl::Enable(gl::STENCIL_TEST);
            gl::StencilMask(0xff);
            gl::StencilFunc(gl::ALWAYS, 0, 0xff);
            gl::ColorMask(gl::FALSE, gl::FALSE, gl::FALSE, gl::FALSE);
            //gl::DepthMask(gl::FALSE);
        }

        self.set_uniforms(stencil_paint, None);

        unsafe {
            gl::StencilOpSeparate(gl::FRONT, gl::KEEP, gl::KEEP, gl::INCR_WRAP);
            gl::StencilOpSeparate(gl::BACK, gl::KEEP, gl::KEEP, gl::DECR_WRAP);
            gl::Disable(gl::CULL_FACE);
        }

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.fill_verts {
                unsafe { gl::DrawArrays(gl::TRIANGLE_FAN, start as i32, count as i32); }
            }
        }

        unsafe {
            gl::Enable(gl::CULL_FACE);
            // Draw anti-aliased pixels
            gl::ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);
            //gl::DepthMask(gl::TRUE);
        }

        self.set_uniforms(fill_paint, cmd.image);

        if self.antialias {
            unsafe {
                match cmd.fill_rule {
                    FillRule::NonZero => gl::StencilFunc(gl::EQUAL, 0x0, 0xff),
                    FillRule::EvenOdd => gl::StencilFunc(gl::EQUAL, 0x0, 0x1)
                }

                gl::StencilOp(gl::KEEP, gl::KEEP, gl::KEEP);
            }

            // draw fringes
            for drawable in &cmd.drawables {
                if let Some((start, count)) = drawable.stroke_verts {
                    unsafe { gl::DrawArrays(gl::TRIANGLE_STRIP, start as i32, count as i32); }
                }
            }
        }

        unsafe {
            match cmd.fill_rule {
                FillRule::NonZero => gl::StencilFunc(gl::NOTEQUAL, 0x0, 0xff),
                FillRule::EvenOdd => gl::StencilFunc(gl::NOTEQUAL, 0x0, 0x1)
            }

            gl::StencilOp(gl::ZERO, gl::ZERO, gl::ZERO);

            if let Some((start, count)) = cmd.triangles_verts {
                gl::DrawArrays(gl::TRIANGLE_STRIP, start as i32, count as i32);
            }

            gl::Disable(gl::STENCIL_TEST);
        }

        self.check_error("concave_fill");
    }

    fn stroke(&self, cmd: &Command, paint: Params) {
        self.set_uniforms(paint, cmd.image);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe { gl::DrawArrays(gl::TRIANGLE_STRIP, start as i32, count as i32); }
            }
        }

        self.check_error("stroke");
    }

    fn stencil_stroke(&self, cmd: &Command, paint1: Params, paint2: Params) {
        unsafe {
            gl::Enable(gl::STENCIL_TEST);
            gl::StencilMask(0xff);

            // Fill the stroke base without overlap
            gl::StencilFunc(gl::EQUAL, 0x0, 0xff);
            gl::StencilOp(gl::KEEP, gl::KEEP, gl::INCR);
        }

        self.set_uniforms(paint2, cmd.image);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe { gl::DrawArrays(gl::TRIANGLE_STRIP, start as i32, count as i32); }
            }
        }

        // Draw anti-aliased pixels.
        self.set_uniforms(paint1, cmd.image);

        unsafe {
            gl::StencilFunc(gl::EQUAL, 0x0, 0xff);
            gl::StencilOp(gl::KEEP, gl::KEEP, gl::KEEP);
        }

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe { gl::DrawArrays(gl::TRIANGLE_STRIP, start as i32, count as i32); }
            }
        }

        unsafe {
            // Clear stencil buffer.
            gl::ColorMask(gl::FALSE, gl::FALSE, gl::FALSE, gl::FALSE);
            gl::StencilFunc(gl::ALWAYS, 0x0, 0xff);
            gl::StencilOp(gl::ZERO, gl::ZERO, gl::ZERO);
        }

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe { gl::DrawArrays(gl::TRIANGLE_STRIP, start as i32, count as i32); }
            }
        }

        unsafe {
            gl::ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);
            gl::Disable(gl::STENCIL_TEST);
        }

        self.check_error("stencil_stroke");
    }

    fn triangles(&self, cmd: &Command, paint: Params) {
        self.set_uniforms(paint, cmd.image);

        if let Some((start, count)) = cmd.triangles_verts {
            unsafe { gl::DrawArrays(gl::TRIANGLES, start as i32, count as i32); }
        }

        self.check_error("triangles");
    }

    fn set_uniforms(&self, paint: Params, image_id: Option<ImageId>) {
        let arr = UniformArray::from(paint);
        self.shader.set_config(UniformArray::size() as i32, arr.as_ptr());
        self.check_error("set_uniforms uniforms");

        let tex = image_id.and_then(|id| self.textures.get(&id)).map_or(0, |texture| texture.tex);

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, tex);
        }

        self.check_error("set_uniforms texture");
    }
}

impl Renderer for OpenGl {
    fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        unsafe {
            gl::Viewport(x as i32, y as i32, width as i32, height as i32);
            gl::ClearColor(color.r, color.g, color.b, color.a);
            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT | gl::STENCIL_BUFFER_BIT);
            gl::Viewport(0, 0, self.view[0] as i32, self.view[1] as i32);
        }
    }

    fn set_size(&mut self, width: u32, height: u32, _dpi: f32) {
        self.view[0] = width as f32;
        self.view[1] = height as f32;

        unsafe {
            gl::Viewport(0, 0, width as i32, height as i32);
        }
    }

    fn render(&mut self, verts: &[Vertex], commands: &[Command]) {
        unsafe {
            self.shader.bind();

            gl::Enable(gl::CULL_FACE);

            gl::CullFace(gl::BACK);
            gl::FrontFace(gl::CCW);
            gl::Enable(gl::BLEND);
            gl::Disable(gl::DEPTH_TEST);
            gl::Disable(gl::SCISSOR_TEST);
            gl::ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);
            gl::StencilMask(0xffffffff);
            gl::StencilOp(gl::KEEP, gl::KEEP, gl::KEEP);
            gl::StencilFunc(gl::ALWAYS, 0, 0xffffffff);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, 0);

            gl::BindVertexArray(self.vert_arr);

            let vertex_size = mem::size_of::<Vertex>();

            gl::BindBuffer(gl::ARRAY_BUFFER, self.vert_buff);
            let size = verts.len() * vertex_size;
            gl::BufferData(gl::ARRAY_BUFFER, size as isize, verts.as_ptr() as *const GLvoid, gl::STREAM_DRAW);

            gl::EnableVertexAttribArray(0);
            gl::EnableVertexAttribArray(1);

            gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, vertex_size as i32, ptr::null::<c_void>());
            gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, vertex_size as i32, (2 * mem::size_of::<f32>()) as *const c_void);
        }

        // Set view and texture just once per frame.
        self.shader.set_tex(0);
        self.shader.set_view(self.view);

        self.check_error("render prepare");

        for cmd in commands {
            // TODO: Blend func
            self.set_composite_operation(cmd.composite_operation);

            match cmd.cmd_type {
                CommandType::ConvexFill { params } => self.convex_fill(cmd, params),
                CommandType::ConcaveFill { stencil_params, fill_params } => self.concave_fill(cmd, stencil_params, fill_params),
                CommandType::Stroke { params } => self.stroke(cmd, params),
                CommandType::StencilStroke { params1, params2 } => self.stencil_stroke(cmd, params1, params2),
                CommandType::Triangles { params } => self.triangles(cmd, params),
            }
        }

        unsafe {
            gl::DisableVertexAttribArray(0);
            gl::DisableVertexAttribArray(1);
            gl::BindVertexArray(0);

            gl::Disable(gl::CULL_FACE);
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::BindTexture(gl::TEXTURE_2D, 0);
        }

        self.shader.unbind();

        self.check_error("render done");
    }

    fn create_image(&mut self, image: &DynamicImage, flags: ImageFlags) -> ImageId {
        let size = image.dimensions();

        let mut texture = Texture {
            tex: 0,
            width: size.0,
            height: size.1,
            flags: flags,
            tex_type: TextureType::Rgba
        };

        unsafe {
            gl::GenTextures(1, &mut texture.tex);
            gl::BindTexture(gl::TEXTURE_2D, texture.tex);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, texture.width as i32);
            gl::PixelStorei(gl::UNPACK_SKIP_PIXELS, 0);
            gl::PixelStorei(gl::UNPACK_SKIP_ROWS, 0);
        }

        match image {
            DynamicImage::ImageLuma8(gray_image) => unsafe {
                let format = if self.is_opengles { gl::LUMINANCE } else { gl::RED };

                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    format as i32,
                    texture.width as i32,
                    texture.height as i32,
                    0,
                    format,
                    gl::UNSIGNED_BYTE,
                    gray_image.as_ref().as_ptr() as *const GLvoid
                );

                texture.tex_type = TextureType::Alpha;
            },
            DynamicImage::ImageRgb8(rgb_image) => unsafe {
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGB as i32,
                    texture.width as i32,
                    texture.height as i32,
                    0,
                    gl::RGB,
                    gl::UNSIGNED_BYTE,
                    rgb_image.as_ref().as_ptr() as *const GLvoid
                );

                texture.tex_type = TextureType::Rgb;
            },
            DynamicImage::ImageRgba8(rgba_image) => unsafe {
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGBA as i32,
                    texture.width as i32,
                    texture.height as i32,
                    0,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    rgba_image.as_ref().as_ptr() as *const GLvoid
                );

                texture.tex_type = TextureType::Rgba;
            },
            _ => panic!("Unsupported image format")
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

        let id = self.last_texture_id;
        self.last_texture_id = self.last_texture_id.wrapping_add(1);

        self.textures.insert(ImageId(id), texture);

        ImageId(id)
    }

    fn update_image(&mut self, id: ImageId, image: &DynamicImage, x: u32, y: u32) {
        let size = image.dimensions();

        let texture = match self.textures.get(&id) {
            Some(texture) => texture,
            None => return
        };

        if x + size.0 > texture.width {
            panic!();// TODO: error handling
        }

        if y + size.1 > texture.height {
            panic!();// TODO: error handling
        }

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, texture.tex);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, size.0 as i32);
        }

        match image {
            DynamicImage::ImageLuma8(gray_image) => unsafe {
                let format = if self.is_opengles { gl::LUMINANCE } else { gl::RED };

                if texture.tex_type != TextureType::Alpha {
                    panic!("Attemped to update texture with an image of a different format");
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
                if texture.tex_type != TextureType::Rgb {
                    panic!("Attemped to update texture with an image of a different format");
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
                if texture.tex_type != TextureType::Rgba {
                    panic!("Attemped to update texture with an image of a different format");
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
            _ => panic!("Unsupported image format")
        }

        unsafe {
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 4);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, 0);
            //gl::PixelStorei(gl::UNPACK_SKIP_PIXELS, 0);
            //gl::PixelStorei(gl::UNPACK_SKIP_ROWS, 0);
            gl::BindTexture(gl::TEXTURE_2D, 0);
        }
    }

    fn delete_image(&mut self, id: ImageId) {
        if let Some(texture) = self.textures.remove(&id) {
            unsafe {
                gl::DeleteTextures(1, &texture.tex);
            }
        }
    }

    fn texture_flags(&self, id: ImageId) -> ImageFlags {
        self.textures.get(&id).unwrap().flags
    }

    fn texture_size(&self, id: ImageId) -> (u32, u32) {
        let tex = self.textures.get(&id).unwrap();

        (tex.width, tex.height)
    }

    fn texture_type(&self, id: ImageId) -> Option<TextureType> {
        let tex = self.textures.get(&id).unwrap();

        Some(tex.tex_type)
    }

    fn screenshot(&mut self) -> Option<DynamicImage> {
        let mut image = image::RgbaImage::new(self.view[0] as u32, self.view[1] as u32);

        unsafe {
            gl::ReadPixels(0, 0, self.view[0] as i32, self.view[1] as i32, gl::RGBA, gl::UNSIGNED_BYTE, image.deref_mut().as_ptr() as *mut GLvoid);
        }

        image = image::imageops::flip_vertical(&image);

        Some(DynamicImage::ImageRgba8(image))
    }
}

impl Drop for OpenGl {
    fn drop(&mut self) {
        for (_, texture) in self.textures.drain() {
            unsafe { gl::DeleteTextures(1, &texture.tex); }
        }

        if self.vert_arr != 0 {
            unsafe { gl::DeleteVertexArrays(1, &self.vert_arr); }
        }

        if self.vert_buff != 0 {
            unsafe { gl::DeleteBuffers(1, &self.vert_buff); }
        }
    }
}

#[derive(Debug)]
pub enum OpenGlError {
    GeneralError(String),
    ShaderError(ShaderError),
}

impl fmt::Display for OpenGlError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid first item to double")//TODO
    }
}

impl From<NulError> for OpenGlError {
    fn from(error: NulError) -> Self {
        OpenGlError::GeneralError(error.description().to_string())
    }
}

impl From<ShaderError> for OpenGlError {
    fn from(error: ShaderError) -> Self {
        OpenGlError::ShaderError(error)
    }
}

impl Error for OpenGlError {}
