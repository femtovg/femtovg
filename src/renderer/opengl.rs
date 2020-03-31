
use std::ptr;
use std::mem;
use std::ffi::{CString, CStr, c_void};

use rgb::RGBA8;
use imgref::ImgVec;

use crate::{
    Color,
    Result,
    Image,
    ImageStore,
    ImageFormat,
    ImageSource,
    FillRule,
    CompositeOperationState,
    BlendFactor,
    renderer::{Vertex, ImageId}
};

use super::{
    Params,
    Renderer,
    Command,
    CommandType,
    ImageFlags,
    RenderTarget
};

mod program;
use program::{
    Shader,
    Program,
    MainProgram,
    BlurProgram
};

mod texture;
use texture::Texture;

mod uniform_array;
use uniform_array::UniformArray;

#[allow(clippy::all)]
mod gl {
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));
}

use gl::types::*;

pub struct OpenGl {
    debug: bool,
    antialias: bool,
    is_opengles: bool,
    view: [f32; 2],
    main_program: MainProgram,
    blur_program: BlurProgram,
    vert_arr: GLuint,
    vert_buff: GLuint,
    quad_vao: GLuint,
    quad_vbo: GLuint
}

impl OpenGl {

    pub fn new<F>(load_fn: F) -> Result<Self> where F: Fn(&'static str) -> *const c_void {
        let debug = true;
        let antialias = true;

        gl::load_with(load_fn);

        let main_program = MainProgram::new(antialias)?;
        let blur_program = BlurProgram::new()?;

        let mut opengl = OpenGl {
            debug: debug,
            antialias: antialias,
            is_opengles: false,
            view: [0.0, 0.0],
            main_program: main_program,
            blur_program: blur_program,
            vert_arr: Default::default(),
            vert_buff: Default::default(),
            quad_vao: Default::default(),
            quad_vbo: Default::default(),
        };

        unsafe {
            let version = CStr::from_ptr(gl::GetString(gl::VERSION) as *mut i8);
            opengl.is_opengles = version.to_str().ok().map_or(false, |str| str.starts_with("OpenGL ES"));

            gl::GenVertexArrays(1, &mut opengl.vert_arr);
            gl::GenBuffers(1, &mut opengl.vert_buff);
        }

        opengl.create_quad();

        Ok(opengl)
    }

    pub fn is_opengles(&self) -> bool {
        self.is_opengles
    }

    fn create_quad(&mut self) {
        let verts: [f32; 16] = [
            -1.0,  1.0, 0.0, 1.0,
            -1.0, -1.0, 0.0, 0.0,
             1.0,  1.0, 1.0, 1.0,
             1.0, -1.0, 1.0, 0.0,
        ];

        unsafe {
            gl::GenVertexArrays(1, &mut self.quad_vao);
            gl::GenBuffers(1, &mut self.quad_vbo);
            gl::BindVertexArray(self.quad_vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.quad_vbo);

            let size = verts.len() * mem::size_of::<f32>();
            gl::BufferData(gl::ARRAY_BUFFER, size as isize, verts.as_ptr() as *const GLvoid, gl::STREAM_DRAW);

            gl::EnableVertexAttribArray(0);
            gl::EnableVertexAttribArray(1);

            gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 4 * mem::size_of::<f32>() as i32, ptr::null::<c_void>());
            gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, 4 * mem::size_of::<f32>() as i32, (2 * mem::size_of::<f32>()) as *const c_void);
        }
    }

    fn render_quad(&self) {
        unsafe {
            gl::BindVertexArray(self.quad_vao);
            gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
            gl::BindVertexArray(0);
        }
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

    fn convex_fill(&self, images: &ImageStore<Texture>, cmd: &Command, gpu_paint: Params) {
        self.set_uniforms(images, gpu_paint, cmd.image, cmd.alpha_mask);

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

    fn concave_fill(&self, images: &ImageStore<Texture>, cmd: &Command, stencil_paint: Params, fill_paint: Params) {
        unsafe {
            gl::Enable(gl::STENCIL_TEST);
            gl::StencilMask(0xff);
            gl::StencilFunc(gl::ALWAYS, 0, 0xff);
            gl::ColorMask(gl::FALSE, gl::FALSE, gl::FALSE, gl::FALSE);
            //gl::DepthMask(gl::FALSE);
        }

        self.set_uniforms(images, stencil_paint, None, None);

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

        self.set_uniforms(images, fill_paint, cmd.image, cmd.alpha_mask);

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

    fn stroke(&self, images: &ImageStore<Texture>, cmd: &Command, paint: Params) {
        self.set_uniforms(images, paint, cmd.image, cmd.alpha_mask);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe { gl::DrawArrays(gl::TRIANGLE_STRIP, start as i32, count as i32); }
            }
        }

        self.check_error("stroke");
    }

    fn stencil_stroke(&self, images: &ImageStore<Texture>, cmd: &Command, paint1: Params, paint2: Params) {
        unsafe {
            gl::Enable(gl::STENCIL_TEST);
            gl::StencilMask(0xff);

            // Fill the stroke base without overlap
            gl::StencilFunc(gl::EQUAL, 0x0, 0xff);
            gl::StencilOp(gl::KEEP, gl::KEEP, gl::INCR);
        }

        self.set_uniforms(images, paint2, cmd.image, cmd.alpha_mask);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe { gl::DrawArrays(gl::TRIANGLE_STRIP, start as i32, count as i32); }
            }
        }

        // Draw anti-aliased pixels.
        self.set_uniforms(images, paint1, cmd.image, cmd.alpha_mask);

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

    fn triangles(&self, images: &ImageStore<Texture>, cmd: &Command, paint: Params) {
        self.set_uniforms(images, paint, cmd.image, cmd.alpha_mask);

        if let Some((start, count)) = cmd.triangles_verts {
            unsafe { gl::DrawArrays(gl::TRIANGLES, start as i32, count as i32); }
        }

        self.check_error("triangles");
    }

    fn set_uniforms(&self, images: &ImageStore<Texture>, paint: Params, image_tex: Option<ImageId>, alpha_tex: Option<ImageId>) {
        let arr = UniformArray::from(paint);
        self.main_program.set_config(UniformArray::size() as i32, arr.as_ptr());
        self.check_error("set_uniforms uniforms");

        let tex = image_tex.and_then(|id| images.get(id)).map_or(0, |tex| tex.id());

        unsafe {
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, tex);
        }

        let masktex = alpha_tex.and_then(|id| images.get(id)).map_or(0, |tex| tex.id());

        unsafe {
            gl::ActiveTexture(gl::TEXTURE0 + 1);
            gl::BindTexture(gl::TEXTURE_2D, masktex);
        }

        self.check_error("set_uniforms texture");
    }

    fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        unsafe {
            gl::Enable(gl::SCISSOR_TEST);
            gl::Scissor(x as i32, self.view[1] as i32 - (height as i32 + y as i32), width as i32, height as i32);
            gl::ClearColor(color.r, color.g, color.b, color.a);
            gl::Clear(gl::COLOR_BUFFER_BIT | gl::STENCIL_BUFFER_BIT);
            gl::Disable(gl::SCISSOR_TEST);
        }
    }
}

impl Renderer for OpenGl {
    type Image = Texture;

    fn set_size(&mut self, width: u32, height: u32, _dpi: f32) {
        self.view[0] = width as f32;
        self.view[1] = height as f32;

        unsafe {
            gl::Viewport(0, 0, width as i32, height as i32);
        }
    }

    fn render(&mut self, images: &ImageStore<Texture>, verts: &[Vertex], commands: &[Command]) {
        self.main_program.bind();

        unsafe {
            gl::Enable(gl::CULL_FACE);

            gl::CullFace(gl::BACK);
            gl::FrontFace(gl::CCW);
            gl::Enable(gl::BLEND);
            gl::Disable(gl::DEPTH_TEST);
            gl::Disable(gl::SCISSOR_TEST);
            gl::ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);
            gl::StencilMask(0xffff_ffff);
            gl::StencilOp(gl::KEEP, gl::KEEP, gl::KEEP);
            gl::StencilFunc(gl::ALWAYS, 0, 0xffff_ffff);
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

        // Bind the two uniform samplers to texture units
        self.main_program.set_tex(0);
        self.main_program.set_masktex(1);
        // Set uniforms
        self.main_program.set_view(self.view);

        self.check_error("render prepare");

        for cmd in commands {
            self.set_composite_operation(cmd.composite_operation);

            match cmd.cmd_type {
                CommandType::ConvexFill { params } => self.convex_fill(images, cmd, params),
                CommandType::ConcaveFill { stencil_params, fill_params } => self.concave_fill(images, cmd, stencil_params, fill_params),
                CommandType::Stroke { params } => self.stroke(images, cmd, params),
                CommandType::StencilStroke { params1, params2 } => self.stencil_stroke(images, cmd, params1, params2),
                CommandType::Triangles { params } => self.triangles(images, cmd, params),
                CommandType::ClearRect { x, y, width, height, color } => {
                    self.clear_rect(x, y, width, height, color);
                }
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

        self.main_program.unbind();

        self.check_error("render done");
    }

    fn create_image(&mut self, data: ImageSource, flags: ImageFlags) -> Result<Self::Image> {
        Texture::new(data, flags, self.is_opengles)
    }

    fn update_image(&mut self, image: &mut Self::Image, data: ImageSource, x: usize, y: usize) -> Result<()> {
        image.update(data, x, y, self.is_opengles)
    }

    fn delete_image(&mut self, image: Self::Image) {
        image.delete();
    }

    fn set_target(&mut self, images: &ImageStore<Texture>, target: RenderTarget) {
        match target {
            RenderTarget::Screen => {
                //glBindFramebuffer(GL_FRAMEBUFFER, 0);
            },
            RenderTarget::Image(id) => {
                if let Some(texture) = images.get(id) {

                }
            }
        }
    }

    fn blur(&mut self, texture: &mut Texture, amount: f32, x: usize, y: usize, width: usize, height: usize) {
        let pingpong_fbo = [0; 2];
        let pingpong_tex = [0; 2];

        unsafe {
            gl::GenFramebuffers(2, pingpong_fbo.as_ptr() as *mut GLuint);
            gl::GenTextures(2, pingpong_tex.as_ptr() as *mut GLuint);

            gl::Viewport(0, 0, texture.info().width() as i32, texture.info().height() as i32);
            gl::Enable(gl::SCISSOR_TEST);

            let padding = amount as i32 * 2;

            gl::Scissor(
                x as i32 - padding,
                y as i32 - padding,
                width as i32 + padding * 2,
                height as i32 + padding * 2
            );
        }

        let gl_format = match texture.info().format() {
            ImageFormat::Rgb => gl::RGB,
            ImageFormat::Rgba => gl::RGBA,
            ImageFormat::Gray => gl::RED,
        };

        for (fbo, tex) in pingpong_fbo.iter().zip(pingpong_tex.iter()) {
            unsafe {
                gl::BindFramebuffer(gl::FRAMEBUFFER, *fbo);
                gl::BindTexture(gl::TEXTURE_2D, *tex);
                gl::TexImage2D(gl::TEXTURE_2D, 0, gl_format as i32, texture.info().width() as i32, texture.info().height() as i32, 0, gl_format, gl::UNSIGNED_BYTE, ptr::null());
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);

                gl::FramebufferTexture2D(
                    gl::FRAMEBUFFER, gl::COLOR_ATTACHMENT0, gl::TEXTURE_2D, *tex, 0
                );

                if gl::CheckFramebufferStatus(gl::FRAMEBUFFER) != gl::FRAMEBUFFER_COMPLETE {
                    panic!("Framebuffer not complete!");
                }
            }
        }

        self.check_error("blur setup");

        let mut horizontal = true;
        let amount = (amount * 2.0) as usize;

        self.blur_program.bind();
        self.blur_program.set_image(0);
        self.blur_program.set_image_size([
            texture.info().width() as f32,
            texture.info().height() as f32
        ]);

        for i in 0..amount {
            unsafe {
                gl::BindFramebuffer(gl::FRAMEBUFFER, pingpong_fbo[horizontal as usize]);
                self.blur_program.set_horizontal(horizontal);
                gl::BindTexture(gl::TEXTURE_2D, if i == 0 { texture.id() } else { pingpong_tex[!horizontal as usize] });
            }

            self.render_quad();

            horizontal = !horizontal;
        }

        self.check_error("blur render");

        self.blur_program.unbind();

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, texture.id());
            gl::CopyTexSubImage2D(
                gl::TEXTURE_2D,
                0,
                x as i32,
                y as i32,
                x as i32,
                y as i32,
                width as i32,
                height as i32
            );

            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);

            gl::Viewport(0, 0, self.view[0] as i32, self.view[1] as i32);
            gl::Disable(gl::SCISSOR_TEST);
        }

        unsafe {
            gl::DeleteTextures(2, pingpong_tex.as_ptr() as *mut GLuint);
            gl::DeleteFramebuffers(2, pingpong_fbo.as_ptr() as *mut GLuint);
        }

        self.check_error("blur copy");
    }

    fn screenshot(&mut self) -> Result<ImgVec<RGBA8>> {
        //let mut image = image::RgbaImage::new(self.view[0] as u32, self.view[1] as u32);
        let w = self.view[0] as usize;
        let h = self.view[1] as usize;

        let mut image = ImgVec::new(vec![RGBA8 {r:255, g:255, b:255, a: 255}; w*h], w, h);

        unsafe {
            gl::ReadPixels(0, 0, self.view[0] as i32, self.view[1] as i32, gl::RGBA, gl::UNSIGNED_BYTE, image.buf_mut().as_ptr() as *mut GLvoid);
        }

        // TODO: flip image
        //image = image::imageops::flip_vertical(&image);

        Ok(image)
    }
}

impl Drop for OpenGl {
    fn drop(&mut self) {
        if self.vert_arr != 0 {
            unsafe { gl::DeleteVertexArrays(1, &self.vert_arr); }
        }

        if self.vert_buff != 0 {
            unsafe { gl::DeleteBuffers(1, &self.vert_buff); }
        }

        if self.quad_vao != 0 {
            unsafe { gl::DeleteVertexArrays(1, &self.quad_vao); }
        }

        if self.quad_vbo != 0 {
            unsafe { gl::DeleteBuffers(1, &self.quad_vbo); }
        }
    }
}
