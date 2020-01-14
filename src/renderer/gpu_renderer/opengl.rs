
use std::ptr;
use std::mem;
use std::ffi::{CStr, NulError, c_void};
use std::{error::Error, fmt};

use fnv::FnvHashMap;
use image::DynamicImage;

use super::{Command, GpuRendererBackend, Flavor, Params};
use crate::{Color, ImageFlags, Vertex};
use crate::renderer::{ImageId, TextureType};

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

    fn convex_fill(&self, cmd: &Command, params: Params) {
        self.set_uniforms(params, cmd.image);

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

    fn concave_fill(&self, cmd: &Command, fill_params: Params, stroke_params: Params) {
        unsafe {
            gl::Enable(gl::STENCIL_TEST);
            gl::StencilMask(0xff);
            gl::StencilFunc(gl::ALWAYS, 0, 0xff);
            gl::ColorMask(gl::FALSE, gl::FALSE, gl::FALSE, gl::FALSE);
        }

        self.set_uniforms(fill_params, None);

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
        }

        self.set_uniforms(stroke_params, cmd.image);

        if self.antialias {
            unsafe {
                gl::StencilFunc(gl::EQUAL, 0x00, 0xff);
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
            gl::StencilFunc(gl::NOTEQUAL, 0x0, 0xff);
            gl::StencilOp(gl::ZERO, gl::ZERO, gl::ZERO);

            if let Some((start, count)) = cmd.triangles_verts {
                gl::DrawArrays(gl::TRIANGLE_STRIP, start as i32, count as i32);
            }

            gl::Disable(gl::STENCIL_TEST);
        }

        self.check_error("concave_fill");
    }

    fn stroke(&self, cmd: &Command, params: Params) {
        self.set_uniforms(params, cmd.image);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe { gl::DrawArrays(gl::TRIANGLE_STRIP, start as i32, count as i32); }
            }
        }

        self.check_error("stroke");
    }

    fn stencil_stroke(&self, cmd: &Command, params1: Params, params2: Params) {
        unsafe {
            gl::Enable(gl::STENCIL_TEST);
            gl::StencilMask(0xff);

            // Fill the stroke base without overlap
            gl::StencilFunc(gl::EQUAL, 0x0, 0xff);
            gl::StencilOp(gl::KEEP, gl::KEEP, gl::INCR);
        }

        self.set_uniforms(params2, cmd.image);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe { gl::DrawArrays(gl::TRIANGLE_STRIP, start as i32, count as i32); }
            }
        }

        // Draw anti-aliased pixels.
        self.set_uniforms(params1, cmd.image);

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

    fn triangles(&self, cmd: &Command, params: Params) {
        self.set_uniforms(params, cmd.image);

        if let Some((start, count)) = cmd.triangles_verts {
            unsafe { gl::DrawArrays(gl::TRIANGLES, start as i32, count as i32); }
        }

        self.check_error("triangles");
    }

    fn set_uniforms(&self, params: Params, image_id: Option<ImageId>) {
        let arr = UniformArray::from(params);
        self.shader.set_config(UniformArray::size() as i32, arr.as_ptr());
        self.check_error("set_uniforms uniforms");

        let tex = image_id.and_then(|id| self.textures.get(&id)).map_or(0, |texture| texture.tex);

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, tex);
        }

        self.check_error("set_uniforms texture");
    }
}

impl GpuRendererBackend for OpenGl {
    fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        unsafe {
            gl::Viewport(x as i32, y as i32, width as i32, height as i32);
            gl::ClearColor(color.r, color.g, color.b, color.a);
            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT | gl::STENCIL_BUFFER_BIT);
        }
    }

    fn set_size(&mut self, width: u32, height: u32, _dpi: f32) {
        self.view[0] = width as f32;
        self.view[1] = height as f32;
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
            unsafe { gl::BlendFuncSeparate(gl::ONE, gl::ONE_MINUS_SRC_ALPHA, gl::ONE, gl::ONE_MINUS_SRC_ALPHA); }

            match cmd.flavor {
                Flavor::ConvexFill { params } => self.convex_fill(cmd, params),
                Flavor::ConcaveFill { fill_params, stroke_params } => self.concave_fill(cmd, fill_params, stroke_params),
                Flavor::Stroke { params } => self.stroke(cmd, params),
                Flavor::StencilStroke { pass1, pass2 } => self.stencil_stroke(cmd, pass1, pass2),
                Flavor::Triangles { params } => self.triangles(cmd, params),
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

    fn create_texture(&mut self, texture_type: TextureType, width: u32, height: u32, flags: ImageFlags) -> ImageId {
        let mut texture = Texture {
            tex: 0,
            width: width,
            height: height,
            flags: flags,
            tex_type: texture_type
        };

        unsafe {
            gl::GenTextures(1, &mut texture.tex);
            gl::BindTexture(gl::TEXTURE_2D, texture.tex);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, texture.width as i32);
            gl::PixelStorei(gl::UNPACK_SKIP_PIXELS, 0);
            gl::PixelStorei(gl::UNPACK_SKIP_ROWS, 0);
        }

        match texture.tex_type {
            TextureType::Rgba => unsafe {
                gl::TexImage2D(gl::TEXTURE_2D, 0, gl::RGBA as i32, width as i32, height as i32, 0, gl::RGBA, gl::UNSIGNED_BYTE, ptr::null());
            },
            TextureType::Alpha => unsafe {
                let format = if self.is_opengles { gl::LUMINANCE } else { gl::RED };

                gl::TexImage2D(gl::TEXTURE_2D, 0, format as i32, width as i32, height as i32, 0, format, gl::UNSIGNED_BYTE, ptr::null());
            }
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

    fn update_texture(&mut self, id: ImageId, image: &DynamicImage, x: u32, y: u32, w: u32, h: u32) {
        let texture = match self.textures.get(&id) {
            Some(texture) => texture,
            None => return
        };

        if x + w > texture.width {
            panic!();// TODO: error handling
        }

        if y + h > texture.height {
            panic!();// TODO: error handling
        }

        // TODO: the comments bellow had to me made for font support (partial texture update)
        // So now this function expects that the image provided is the entire update data,
        // before it expected the full image and only updated a region from it
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, texture.tex);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            //gl::PixelStorei(gl::UNPACK_ROW_LENGTH, texture.width as i32);

            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, w as i32);///////

            //gl::PixelStorei(gl::UNPACK_SKIP_PIXELS, x as i32);
            //gl::PixelStorei(gl::UNPACK_SKIP_ROWS, y as i32);
        }

        match texture.tex_type {
            TextureType::Rgba => unsafe {
                let image = image.to_rgba();
                gl::TexSubImage2D(gl::TEXTURE_2D, 0, x as i32, y as i32, w as i32, h as i32, gl::RGBA, gl::UNSIGNED_BYTE, image.into_raw().as_ptr() as *const GLvoid);
            },
            TextureType::Alpha => unsafe {
                let image = image.to_luma();
                let format = if self.is_opengles { gl::LUMINANCE } else { gl::RED };

                gl::TexSubImage2D(gl::TEXTURE_2D, 0, x as i32, y as i32, w as i32, h as i32, format, gl::UNSIGNED_BYTE, image.into_raw().as_ptr() as *const GLvoid);
            }
        }

        unsafe {
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 4);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, 0);
            //gl::PixelStorei(gl::UNPACK_SKIP_PIXELS, 0);
            //gl::PixelStorei(gl::UNPACK_SKIP_ROWS, 0);
            gl::BindTexture(gl::TEXTURE_2D, 0);
        }
    }

    fn delete_texture(&mut self, id: ImageId) {
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
