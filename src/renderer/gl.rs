
use std::str;
use std::ptr;
use std::mem;
use std::ffi::{NulError, CStr, c_void};
use std::{error::Error, fmt};

use fnv::FnvHashMap;
use image::DynamicImage;

// TODO: Replace all x as i32 with try_into
// TODO: After everything is finished, try to move to a struct generator for the gl bindings instead og the global generator
// TODO: Rename vert_arr to vertex_array and vert_buff to vertex_buffer. Same to frag_buff
// TODO: Rename calls to commands
// TODO: "frag" is not a good name for the fragment shader data. Rename it once finished
// Rendering dashed lines -> https://hal.inria.fr/hal-00907326/file/paper.pdf
// TODO: Remove let shader_header = "#version 100"; we only support gles2

use super::{Renderer, TextureType};
use crate::{Vertex, Paint, Scissor, ImageId, ImageFlags};
use crate::path::{Contour, Convexity};
use crate::math::Transform2D;

mod gl {
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));
}

use gl::types::*;

mod shader;
use shader::{Shader, ShaderError};

mod uniform_array;
use uniform_array::UniformArray;

// TODO: Rename those to make more sense - why do we have FillImage and Img?
#[derive(Copy, Clone)]
enum ShaderType {
    FillGradient,
    FillImage,
    Simple,
    Img
}

impl Default for ShaderType {
    fn default() -> Self { Self::Simple }
}

impl ShaderType {
    pub fn to_i32(self) -> i32 {
        match self {
            Self::FillGradient => 0,
            Self::FillImage => 1,
            Self::Simple => 2,
            Self::Img => 3,
        }
    }
}

// TODO: Use Option<CallType> instead of having None variant here.
// Also variant specific information for the call can be put here instead of the Call struct
// Also also, it's almost the same as the ShaderType enum, maybe we don't need 2 enums
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum CallType {
    None,
    Fill,
    ConvexFill,
    Stroke,
    Triangles,
}

impl Default for CallType {
    fn default() -> Self { Self::None }
}

#[derive(Default, Debug)]
struct GlRenderCall {
    call_type: CallType,
    contour_offset: usize,
    contour_count: usize,
    triangle_offset: usize,
    triangle_count: usize,
    uniform_offset: usize,
    image: Option<ImageId>
}

struct GlTexture {
    tex: GLuint,
    width: u32,
    height: u32,
    flags: ImageFlags,
    tex_type: TextureType
}

#[derive(Copy, Clone, Default, Debug)]
struct GlContour {
    fill_offset: usize,
    fill_count: usize,
    stroke_offset: usize,
    stroke_count: usize,
}

pub struct GlRenderer {
    antialias: bool,
    debug: bool,
    stencil_strokes: bool,
    is_opengles: bool,
    view: [f32; 2],
    shader: Shader,
    vert_arr: GLuint,
    vert_buff: GLuint,
    uniforms: Vec<UniformArray>,
    calls: Vec<GlRenderCall>,
    paths: Vec<GlContour>,
    verts: Vec<Vertex>,
    last_texture_id: u32,
    textures: FnvHashMap<ImageId, GlTexture>
}

impl GlRenderer {

    pub fn new<F>(load_fn: F) -> Result<Self, GlRendererError> where F: Fn(&'static str) -> *const c_void {

        // TODO: use a builder pattern or flags for these
        let antialias = true;
        let debug = true;
        let stencil_strokes = true;

        gl::load_with(load_fn);
        
        let frag_shader_src = include_str!("gl/main-fs.glsl");
        let vert_shader_src = include_str!("gl/main-vs.glsl");

        let shader = if antialias {
            Shader::new("#define EDGE_AA 1", vert_shader_src, frag_shader_src)?
        } else {
            Shader::new("", vert_shader_src, frag_shader_src)?
        };

        let mut renderer = Self {
            antialias: antialias,
            debug: debug,
            stencil_strokes: stencil_strokes,
            is_opengles: false,
            view: [0.0, 0.0],
            shader: shader,
            vert_arr: 0,
            vert_buff: 0,
            uniforms: Default::default(),
            calls: Default::default(),
            paths: Default::default(),
            verts: Default::default(),
            last_texture_id: Default::default(),
            textures: Default::default()
        };

        renderer.check_error("init");

        unsafe {
            let version = CStr::from_ptr(gl::GetString(gl::VERSION) as *mut i8);
            renderer.is_opengles = version.to_str().ok().map_or(false, |str| str.starts_with("OpenGL ES"));

            gl::GenVertexArrays(1, &mut renderer.vert_arr);
            gl::GenBuffers(1, &mut renderer.vert_buff);
        }

        renderer.check_error("vertex arrays and buffers");

        unsafe { gl::Finish(); }

        Ok(renderer)
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

}

impl Renderer for GlRenderer {

    fn edge_antialiasing(&self) -> bool {
        self.antialias
    }

    fn render_viewport(&mut self, window_width: f32, window_height: f32) {
        unsafe {
            // TODO: this is not the correct place for this clearing. What if the renderer is called after game objects are drawn - it will clear everything.
            gl::Viewport(0, 0, window_width as i32, window_height as i32);
            gl::ClearColor(1.0, 1.0, 1.0, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT | gl::STENCIL_BUFFER_BIT);
        }

        self.view[0] = window_width;
        self.view[1] = window_height;
    }

    fn render_flush(&mut self) {

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
            let size = self.verts.len() * vertex_size;
            gl::BufferData(gl::ARRAY_BUFFER, size as isize, self.verts.as_ptr() as *const GLvoid, gl::STREAM_DRAW);

            gl::EnableVertexAttribArray(0);
            gl::EnableVertexAttribArray(1);

            gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, vertex_size as i32, 0 as *const c_void);
            gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, vertex_size as i32, (2 * mem::size_of::<f32>()) as *const c_void);
        }
        
        // Set view and texture just once per frame.
        self.shader.set_tex(0);
        self.shader.set_view(self.view);

        self.check_error("render_flush prepare");

        for call in &self.calls {

            // Blend func
            unsafe { gl::BlendFuncSeparate(gl::ONE, gl::ONE_MINUS_SRC_ALPHA, gl::ONE, gl::ONE_MINUS_SRC_ALPHA); }

            match call.call_type {
                CallType::Fill => self.fill(call),
                CallType::ConvexFill => self.convex_fill(call),
                CallType::Stroke => self.stroke(call),
                CallType::Triangles => self.triangles(call),
                _ => ()
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

        self.check_error("render_flush done");

        self.calls.clear();
        self.verts.clear();
        self.uniforms.clear();
        // paths
    }

    fn render_fill(&mut self, paint: &Paint, scissor: &Scissor, fringe_width: f32, bounds: [f32; 4], contours: &[Contour]) {

        let mut call = GlRenderCall::default();

        call.call_type = CallType::Fill;
        call.triangle_count = 4; // I think this is 4 since this renders bounding box only, rest must come from path itself idk
        call.contour_offset = self.paths.len();
        call.contour_count = contours.len();
        call.image = paint.image();

        if contours.len() == 1 && contours[0].convexity == Convexity::Convex {
            call.call_type = CallType::ConvexFill;
            call.triangle_count = 0; // Bounding box fill quad not needed for convex fill
        }

        //let max_vertex_count = paths.iter().fold(0, |acc, path| acc + path.fill.len() + path.stroke.len()) + call.triangle_count;

        let mut offset = self.verts.len();

        for contour in contours {
            let mut glcontour = GlContour::default();

            if !contour.fill.is_empty() {
                glcontour.fill_offset = offset;
                glcontour.fill_count = contour.fill.len();

                self.verts.extend_from_slice(&contour.fill);

                offset += contour.fill.len();
            }

            if !contour.stroke.is_empty() {
                glcontour.stroke_offset = offset;
                glcontour.stroke_count = contour.stroke.len();

                self.verts.extend_from_slice(&contour.stroke);

                offset += contour.stroke.len();
            }

            self.paths.push(glcontour);
        }

        // Setup uniforms for draw calls
        if call.call_type == CallType::Fill {
            // Quad
            call.triangle_offset = offset;
            self.verts.push(Vertex::new(bounds[2], bounds[3], 0.5, 1.0));
            self.verts.push(Vertex::new(bounds[2], bounds[1], 0.5, 1.0));
            self.verts.push(Vertex::new(bounds[0], bounds[3], 0.5, 1.0));
            self.verts.push(Vertex::new(bounds[0], bounds[1], 0.5, 1.0));

            call.uniform_offset = self.uniforms.len();

            let mut uniform = UniformArray::default();
            uniform.set_stroke_thr(-1.0);
            uniform.set_shader_type(ShaderType::Simple.to_i32() as f32);//TODO: create ShaderType::to_f32()
            self.uniforms.push(uniform);

            let mut uniform = UniformArray::default();
            self.convert_paint(&mut uniform, paint, scissor, fringe_width, fringe_width, -1.0);
            self.uniforms.push(uniform);
        } else {
            call.uniform_offset = self.uniforms.len();

            let mut uniform = UniformArray::default();
            self.convert_paint(&mut uniform, paint, scissor, fringe_width, fringe_width, -1.0);
            self.uniforms.push(uniform);
        }

        self.calls.push(call);
    }

    fn render_stroke(&mut self, paint: &Paint, scissor: &Scissor, fringe_width: f32, stroke_width: f32, contours: &[Contour]) {
        let mut call = GlRenderCall::default();

        call.call_type = CallType::Stroke;
        call.contour_offset = self.paths.len();
        call.contour_count = contours.len();
        call.image = paint.image();

        // TODO: blend func

        let mut offset = self.verts.len();

        for contour in contours {
            let mut glcontour = GlContour::default();

            if !contour.stroke.is_empty() {
                glcontour.stroke_offset = offset;
                glcontour.stroke_count = contour.stroke.len();

                self.verts.extend_from_slice(&contour.stroke);

                offset += contour.stroke.len();
            }

            self.paths.push(glcontour);
        }

        call.uniform_offset = self.uniforms.len();

        let mut uniform = UniformArray::default();
        self.convert_paint(&mut uniform, paint, scissor, stroke_width, fringe_width, -1.0);
        self.uniforms.push(uniform);

        if self.stencil_strokes {
            let mut uniform = UniformArray::default();
            self.convert_paint(&mut uniform, paint, scissor, stroke_width, fringe_width, 1.0 - 0.5/255.0);
            self.uniforms.push(uniform);
        }

        self.calls.push(call);
    }

    fn render_triangles(&mut self, paint: &Paint, scissor: &Scissor, verts: &[Vertex]) {
        let mut call = GlRenderCall::default();

        call.call_type = CallType::Triangles;
        // TODO: blendFunc
        call.image = paint.image();
        call.triangle_offset = self.verts.len();
        call.triangle_count = verts.len();

        self.verts.extend_from_slice(verts);

        call.uniform_offset = self.uniforms.len();

        let mut uniform = UniformArray::default();
        self.convert_paint(&mut uniform, paint, scissor, 1.0, 1.0, -1.0);
        uniform.set_shader_type(ShaderType::Img.to_i32() as f32);
        self.uniforms.push(uniform);

        self.calls.push(call);
    }

    fn create_texture(&mut self, texture_type: TextureType, width: u32, height: u32, flags: ImageFlags) -> ImageId {
        let mut texture = GlTexture {
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
}

impl GlRenderer {

    fn convert_paint(&self, uniforms: &mut UniformArray, paint: &Paint, scissor: &Scissor, width: f32, fringe: f32, stroke_thr: f32) {

        uniforms.set_inner_col(paint.inner_color().premultiplied().to_array());
        uniforms.set_outer_col(paint.outer_color().premultiplied().to_array());

        let (scissor_ext, scissor_scale) = if scissor.extent[0] < -0.5 || scissor.extent[1] < -0.5 {
            ([1.0, 1.0], [1.0, 1.0])
        } else {
            uniforms.set_scissor_mat(scissor.transform.inversed().to_mat3x4());

            let scissor_scale = [
                (scissor.transform[0]*scissor.transform[0] + scissor.transform[2]*scissor.transform[2]).sqrt() / fringe,
                (scissor.transform[1]*scissor.transform[1] + scissor.transform[3]*scissor.transform[3]).sqrt() / fringe
            ];
            
            (scissor.extent, scissor_scale)
        };

        uniforms.set_scissor_ext(scissor_ext);
        uniforms.set_scissor_scale(scissor_scale);

        let extent = paint.extent();

        uniforms.set_extent(extent);
        uniforms.set_stroke_mult((width*0.5 + fringe*0.5) / fringe);
        uniforms.set_stroke_thr(stroke_thr);

        let inv_transform;

        if let Some(image_id) = paint.image() {
            let texture = self.textures.get(&image_id);

            if texture.is_none() {
                return;
            }

            let texture = texture.unwrap();

            if texture.flags.contains(ImageFlags::FLIP_Y) {
                let mut m1 = Transform2D::identity();
                m1.translate(0.0, extent[1] * 0.5);
                m1.multiply(&paint.transform());

                let mut m2 = Transform2D::identity();
                m2.scale(1.0, -1.0);
                m2.multiply(&m1);

                m1.translate(0.0, -extent[1] * 0.5);
                m1.multiply(&m2);

                inv_transform = m1.inversed();
            } else {
                inv_transform = paint.transform().inversed();
            }

            uniforms.set_shader_type(ShaderType::FillImage.to_i32() as f32);

            uniforms.set_tex_type(match texture.tex_type {
                TextureType::Rgba => if texture.flags.contains(ImageFlags::PREMULTIPLIED) { 0.0 } else { 1.0 }
                TextureType::Alpha => 2.0
            });
        } else {
            uniforms.set_shader_type(ShaderType::FillGradient.to_i32() as f32);
            uniforms.set_radius(paint.radius());
            uniforms.set_feather(paint.feather());

            inv_transform = paint.transform().inversed();
        }

        uniforms.set_paint_mat(inv_transform.to_mat3x4());
    }

    fn fill(&self, call: &GlRenderCall) {
        let paths = &self.paths[call.contour_offset..(call.contour_offset + call.contour_count)];

        unsafe {
            gl::Enable(gl::STENCIL_TEST);
            gl::StencilMask(0xff);
            gl::StencilFunc(gl::ALWAYS, 0, 0xff);
            gl::ColorMask(gl::FALSE, gl::FALSE, gl::FALSE, gl::FALSE);

            self.set_uniforms(call.uniform_offset, None);

            gl::StencilOpSeparate(gl::FRONT, gl::KEEP, gl::KEEP, gl::INCR_WRAP);
            gl::StencilOpSeparate(gl::BACK, gl::KEEP, gl::KEEP, gl::DECR_WRAP);
            gl::Disable(gl::CULL_FACE);

            for path in paths {
                gl::DrawArrays(gl::TRIANGLE_FAN, path.fill_offset as i32, path.fill_count as i32);
            }

            gl::Enable(gl::CULL_FACE);

            // Draw anti-aliased pixels
            gl::ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);

            // Set uniforms
            self.set_uniforms(call.uniform_offset + 1, call.image);

            if self.antialias {
                gl::StencilFunc(gl::EQUAL, 0x00, 0xff);
                gl::StencilOp(gl::KEEP, gl::KEEP, gl::KEEP);

                // draw fringes
                for path in paths {
                    gl::DrawArrays(gl::TRIANGLE_STRIP, path.stroke_offset as i32, path.stroke_count as i32);
                }
            }

            gl::StencilFunc(gl::NOTEQUAL, 0x0, 0xff);
            gl::StencilOp(gl::ZERO, gl::ZERO, gl::ZERO);
            gl::DrawArrays(gl::TRIANGLE_STRIP, call.triangle_offset as i32, call.triangle_count as i32);
            gl::Disable(gl::STENCIL_TEST);
        }

        self.check_error("fill");
    }

    fn convex_fill(&self, call: &GlRenderCall) {
        let paths = &self.paths[call.contour_offset..(call.contour_offset+call.contour_count)];

        self.set_uniforms(call.uniform_offset, call.image);

        for path in paths {
            unsafe {
                gl::DrawArrays(gl::TRIANGLE_FAN, path.fill_offset as i32, path.fill_count as i32);

                // Draw fringes - fringes are a thing strip of triangles with a gradient inside them surrounding the shape which simulate anti-aliasing
                if path.stroke_count > 0 {
                    gl::DrawArrays(gl::TRIANGLE_STRIP, path.stroke_offset as i32, path.stroke_count as i32);
                }
            }
        }

        self.check_error("Convex fill");
    }

    fn stroke(&self, call: &GlRenderCall) {
        let paths = &self.paths[call.contour_offset..(call.contour_offset+call.contour_count)];

        if self.stencil_strokes {
            unsafe {
                gl::Enable(gl::STENCIL_TEST);
                gl::StencilMask(0xff);

                // Fill the stroke base without overlap
                gl::StencilFunc(gl::EQUAL, 0x0, 0xff);
                gl::StencilOp(gl::KEEP, gl::KEEP, gl::INCR);

                self.set_uniforms(call.uniform_offset + 1, call.image);

                for path in paths {
                    gl::DrawArrays(gl::TRIANGLE_STRIP, path.stroke_offset as i32, path.stroke_count as i32);
                }

                // Draw anti-aliased pixels.
                self.set_uniforms(call.uniform_offset, call.image);
                gl::StencilFunc(gl::EQUAL, 0x0, 0xff);
                gl::StencilOp(gl::KEEP, gl::KEEP, gl::KEEP);

                for path in paths {
                    gl::DrawArrays(gl::TRIANGLE_STRIP, path.stroke_offset as i32, path.stroke_count as i32);
                }

                // Clear stencil buffer.
                gl::ColorMask(gl::FALSE, gl::FALSE, gl::FALSE, gl::FALSE);
                gl::StencilFunc(gl::ALWAYS, 0x0, 0xff);
                gl::StencilOp(gl::ZERO, gl::ZERO, gl::ZERO);

                for path in paths {
                    gl::DrawArrays(gl::TRIANGLE_STRIP, path.stroke_offset as i32, path.stroke_count as i32);
                }

                gl::ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);
                gl::Disable(gl::STENCIL_TEST);
            }
        } else {
            self.set_uniforms(call.uniform_offset, call.image);

            for path in paths {
                unsafe {
                    gl::DrawArrays(gl::TRIANGLE_STRIP, path.stroke_offset as i32, path.stroke_count as i32);
                }
            }

            self.check_error("stroke");
        }
    }

    fn triangles(&self, call: &GlRenderCall) {
        self.set_uniforms(call.uniform_offset, call.image);

        unsafe {
            gl::DrawArrays(gl::TRIANGLES, call.triangle_offset as i32, call.triangle_count as i32);
        }

        self.check_error("triangles");
    }

    fn set_uniforms(&self, offset: usize, image_id: Option<ImageId>) {
        self.shader.set_config(UniformArray::size() as i32, self.uniforms[offset].as_ptr());
        self.check_error("set_uniforms uniforms");

        let tex = image_id.and_then(|id| self.textures.get(&id)).map_or(0, |texture| texture.tex);

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, tex);
        }

        self.check_error("set_uniforms texture");
    }
}

impl Drop for GlRenderer {
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
pub enum GlRendererError {
    ShaderCompileError(String),
    ShaderError(ShaderError),
    GeneralError(String)
}

impl fmt::Display for GlRendererError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid first item to double")
    }
}

impl From<NulError> for GlRendererError {
    fn from(error: NulError) -> Self {
        GlRendererError::GeneralError(error.description().to_string())
    }
}

impl From<ShaderError> for GlRendererError {
    fn from(error: ShaderError) -> Self {
        GlRendererError::ShaderError(error)
    }
}

impl Error for GlRendererError {}
