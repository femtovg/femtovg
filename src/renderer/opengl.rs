use std::mem;
use std::rc::Rc;

#[cfg(not(target_arch = "wasm32"))]
use std::ffi::c_void;

use fnv::FnvHashMap;
use imgref::ImgVec;
use rgb::RGBA8;

use crate::{
    renderer::{
        GlyphTexture,
        ImageId,
        Vertex,
    },
    BlendFactor,
    Color,
    CompositeOperationState,
    ErrorKind,
    FillRule,
    ImageFilter,
    ImageInfo,
    ImageSource,
    ImageStore,
    Scissor,
};

use glow::HasContext;

use super::{
    Command,
    CommandType,
    Params,
    RenderTarget,
    Renderer,
    ShaderType,
};

mod program;
use program::MainProgram;

mod gl_texture;
use gl_texture::GlTexture;

mod framebuffer;
use framebuffer::Framebuffer;

mod uniform_array;
use uniform_array::UniformArray;

pub struct OpenGl {
    debug: bool,
    antialias: bool,
    is_opengles_2_0: bool,
    view: [f32; 2],
    screen_view: [f32; 2],
    main_program: MainProgram,
    vert_arr: Option<<glow::Context as glow::HasContext>::VertexArray>,
    vert_buff: Option<<glow::Context as glow::HasContext>::Buffer>,
    framebuffers: FnvHashMap<ImageId, Result<Framebuffer, ErrorKind>>,
    context: Rc<glow::Context>,
    screen_target: Option<Framebuffer>,
    current_render_target: RenderTarget,
}

impl OpenGl {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new<F>(load_fn: F) -> Result<Self, ErrorKind>
    where
        F: FnMut(&str) -> *const c_void,
    {
        Self::new_from_context(unsafe { glow::Context::from_loader_function(load_fn) }, false)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn new_from_html_canvas(canvas: &web_sys::HtmlCanvasElement) -> Result<Self, ErrorKind> {
        let mut attrs = web_sys::WebGlContextAttributes::new();
        attrs.stencil(true);
        attrs.antialias(false);

        use wasm_bindgen::JsCast;
        let webgl1_context = canvas
            .get_context_with_context_options("webgl", attrs.as_ref())
            .map_err(|_| ErrorKind::GeneralError("Canvas::getContext failed to retrieve WebGL 1 context".to_owned()))?
            .unwrap()
            .dyn_into::<web_sys::WebGlRenderingContext>()
            .unwrap();

        let context = glow::Context::from_webgl1_context(webgl1_context);
        Self::new_from_context(context, true)
    }

    fn new_from_context(context: glow::Context, is_opengles_2_0: bool) -> Result<Self, ErrorKind> {
        let debug = cfg!(debug_assertions);
        let antialias = true;

        let context = Rc::new(context);

        let main_program = MainProgram::new(&context, antialias)?;

        let mut opengl = OpenGl {
            debug: debug,
            antialias: antialias,
            is_opengles_2_0: false,
            view: [0.0, 0.0],
            screen_view: [0.0, 0.0],
            main_program: main_program,
            vert_arr: Default::default(),
            vert_buff: Default::default(),
            framebuffers: Default::default(),
            context: context.clone(),
            screen_target: None,
            current_render_target: RenderTarget::Screen,
        };

        unsafe {
            opengl.is_opengles_2_0 = is_opengles_2_0;

            opengl.vert_arr = opengl.context.create_vertex_array().ok();
            opengl.vert_buff = opengl.context.create_buffer().ok();
        }

        Ok(opengl)
    }

    pub fn is_opengles(&self) -> bool {
        self.is_opengles_2_0
    }

    fn check_error(&self, label: &str) {
        if !self.debug {
            return;
        }

        let err = unsafe { self.context.get_error() };

        if err == glow::NO_ERROR {
            return;
        }

        let message = match err {
            glow::INVALID_ENUM => "Invalid enum",
            glow::INVALID_VALUE => "Invalid value",
            glow::INVALID_OPERATION => "Invalid operation",
            glow::OUT_OF_MEMORY => "Out of memory",
            glow::INVALID_FRAMEBUFFER_OPERATION => "Invalid framebuffer operation",
            _ => "Unknown error",
        };

        eprintln!("({}) Error on {} - {}", err, label, message);
    }

    fn gl_factor(factor: BlendFactor) -> u32 {
        match factor {
            BlendFactor::Zero => glow::ZERO,
            BlendFactor::One => glow::ONE,
            BlendFactor::SrcColor => glow::SRC_COLOR,
            BlendFactor::OneMinusSrcColor => glow::ONE_MINUS_SRC_COLOR,
            BlendFactor::DstColor => glow::DST_COLOR,
            BlendFactor::OneMinusDstColor => glow::ONE_MINUS_DST_COLOR,
            BlendFactor::SrcAlpha => glow::SRC_ALPHA,
            BlendFactor::OneMinusSrcAlpha => glow::ONE_MINUS_SRC_ALPHA,
            BlendFactor::DstAlpha => glow::DST_ALPHA,
            BlendFactor::OneMinusDstAlpha => glow::ONE_MINUS_DST_ALPHA,
            BlendFactor::SrcAlphaSaturate => glow::SRC_ALPHA_SATURATE,
        }
    }

    fn set_composite_operation(&self, blend_state: CompositeOperationState) {
        unsafe {
            self.context.blend_func_separate(
                Self::gl_factor(blend_state.src_rgb),
                Self::gl_factor(blend_state.dst_rgb),
                Self::gl_factor(blend_state.src_alpha),
                Self::gl_factor(blend_state.dst_alpha),
            );
        }
    }

    fn convex_fill(&self, images: &ImageStore<GlTexture>, cmd: &Command, gpu_paint: &Params) {
        self.set_uniforms(images, gpu_paint, cmd.image, cmd.glyph_texture);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.fill_verts {
                unsafe {
                    self.context.draw_arrays(glow::TRIANGLE_FAN, start as i32, count as i32);
                }
            }

            if let Some((start, count)) = drawable.stroke_verts {
                unsafe {
                    self.context
                        .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                }
            }
        }

        self.check_error("convex_fill");
    }

    fn concave_fill(&self, images: &ImageStore<GlTexture>, cmd: &Command, stencil_paint: &Params, fill_paint: &Params) {
        unsafe {
            self.context.enable(glow::STENCIL_TEST);
            self.context.stencil_mask(0xff);
            self.context.stencil_func(glow::ALWAYS, 0, 0xff);
            self.context.color_mask(false, false, false, false);
            //glow::DepthMask(glow::FALSE);
        }

        self.set_uniforms(images, stencil_paint, None, GlyphTexture::None);

        unsafe {
            self.context
                .stencil_op_separate(glow::FRONT, glow::KEEP, glow::KEEP, glow::INCR_WRAP);
            self.context
                .stencil_op_separate(glow::BACK, glow::KEEP, glow::KEEP, glow::DECR_WRAP);
            self.context.disable(glow::CULL_FACE);
        }

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.fill_verts {
                unsafe {
                    self.context.draw_arrays(glow::TRIANGLE_FAN, start as i32, count as i32);
                }
            }
        }

        unsafe {
            self.context.enable(glow::CULL_FACE);
            // Draw anti-aliased pixels
            self.context.color_mask(true, true, true, true);
            //glow::DepthMask(glow::TRUE);
        }

        self.set_uniforms(images, fill_paint, cmd.image, cmd.glyph_texture);

        if self.antialias {
            unsafe {
                match cmd.fill_rule {
                    FillRule::NonZero => self.context.stencil_func(glow::EQUAL, 0x0, 0xff),
                    FillRule::EvenOdd => self.context.stencil_func(glow::EQUAL, 0x0, 0x1),
                }

                self.context.stencil_op(glow::KEEP, glow::KEEP, glow::KEEP);
            }

            // draw fringes
            for drawable in &cmd.drawables {
                if let Some((start, count)) = drawable.stroke_verts {
                    unsafe {
                        self.context
                            .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                    }
                }
            }
        }

        unsafe {
            match cmd.fill_rule {
                FillRule::NonZero => self.context.stencil_func(glow::NOTEQUAL, 0x0, 0xff),
                FillRule::EvenOdd => self.context.stencil_func(glow::NOTEQUAL, 0x0, 0x1),
            }

            self.context.stencil_op(glow::ZERO, glow::ZERO, glow::ZERO);

            if let Some((start, count)) = cmd.triangles_verts {
                self.context
                    .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
            }

            self.context.disable(glow::STENCIL_TEST);
        }

        self.check_error("concave_fill");
    }

    fn stroke(&self, images: &ImageStore<GlTexture>, cmd: &Command, paint: &Params) {
        self.set_uniforms(images, paint, cmd.image, cmd.glyph_texture);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe {
                    self.context
                        .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                }
            }
        }

        self.check_error("stroke");
    }

    fn stencil_stroke(&self, images: &ImageStore<GlTexture>, cmd: &Command, paint1: &Params, paint2: &Params) {
        unsafe {
            self.context.enable(glow::STENCIL_TEST);
            self.context.stencil_mask(0xff);

            // Fill the stroke base without overlap
            self.context.stencil_func(glow::EQUAL, 0x0, 0xff);
            self.context.stencil_op(glow::KEEP, glow::KEEP, glow::INCR);
        }

        self.set_uniforms(images, paint2, cmd.image, cmd.glyph_texture);

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe {
                    self.context
                        .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                }
            }
        }

        // Draw anti-aliased pixels.
        self.set_uniforms(images, paint1, cmd.image, cmd.glyph_texture);

        unsafe {
            self.context.stencil_func(glow::EQUAL, 0x0, 0xff);
            self.context.stencil_op(glow::KEEP, glow::KEEP, glow::KEEP);
        }

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe {
                    self.context
                        .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                }
            }
        }

        unsafe {
            // Clear stencil buffer.
            self.context.color_mask(false, false, false, false);
            self.context.stencil_func(glow::ALWAYS, 0x0, 0xff);
            self.context.stencil_op(glow::ZERO, glow::ZERO, glow::ZERO);
        }

        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                unsafe {
                    self.context
                        .draw_arrays(glow::TRIANGLE_STRIP, start as i32, count as i32);
                }
            }
        }

        unsafe {
            self.context.color_mask(true, true, true, true);
            self.context.disable(glow::STENCIL_TEST);
        }

        self.check_error("stencil_stroke");
    }

    fn triangles(&self, images: &ImageStore<GlTexture>, cmd: &Command, paint: &Params) {
        self.set_uniforms(images, paint, cmd.image, cmd.glyph_texture);

        if let Some((start, count)) = cmd.triangles_verts {
            unsafe {
                self.context.draw_arrays(glow::TRIANGLES, start as i32, count as i32);
            }
        }

        self.check_error("triangles");
    }

    fn set_uniforms(
        &self,
        images: &ImageStore<GlTexture>,
        paint: &Params,
        image_tex: Option<ImageId>,
        glyph_tex: GlyphTexture,
    ) {
        let arr = UniformArray::from(paint);
        self.main_program.set_config(arr.as_slice());
        self.check_error("set_uniforms uniforms");

        let tex = image_tex
            .and_then(|id| images.get(id))
            .map_or(None, |tex| Some(tex.id()));

        unsafe {
            self.context.active_texture(glow::TEXTURE0);
            self.context.bind_texture(glow::TEXTURE_2D, tex);
        }

        let glyphtex = match glyph_tex {
            GlyphTexture::None => None,
            GlyphTexture::AlphaMask(id) | GlyphTexture::ColorTexture(id) => images.get(id).map(|tex| tex.id()),
        };

        unsafe {
            self.context.active_texture(glow::TEXTURE0 + 1);
            self.context.bind_texture(glow::TEXTURE_2D, glyphtex);
        }

        self.check_error("set_uniforms texture");
    }

    fn clear_rect(&self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        unsafe {
            self.context.enable(glow::SCISSOR_TEST);
            self.context.scissor(
                x as i32,
                self.view[1] as i32 - (height as i32 + y as i32),
                width as i32,
                height as i32,
            );
            self.context.clear_color(color.r, color.g, color.b, color.a);
            self.context.clear(glow::COLOR_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);
            self.context.disable(glow::SCISSOR_TEST);
        }
    }

    fn set_target(&mut self, images: &ImageStore<GlTexture>, target: RenderTarget) {
        self.current_render_target = target;
        match (target, &self.screen_target) {
            (RenderTarget::Screen, None) => unsafe {
                Framebuffer::unbind(&self.context);
                self.view = self.screen_view;
                self.context.viewport(0, 0, self.view[0] as i32, self.view[1] as i32);
            },
            (RenderTarget::Screen, Some(framebuffer)) => {
                framebuffer.bind();
                self.view = self.screen_view;
                unsafe {
                    self.context.viewport(0, 0, self.view[0] as i32, self.view[1] as i32);
                }
            }
            (RenderTarget::Image(id), _) => {
                let context = self.context.clone();
                if let Some(texture) = images.get(id) {
                    if let Ok(fb) = self
                        .framebuffers
                        .entry(id)
                        .or_insert_with(|| Framebuffer::new(&context, texture))
                    {
                        fb.bind();

                        self.view[0] = texture.info().width() as f32;
                        self.view[1] = texture.info().height() as f32;

                        unsafe {
                            self.context
                                .viewport(0, 0, texture.info().width() as i32, texture.info().height() as i32);
                        }
                    }
                }
            }
        }
    }

    /// Make the "Screen" RenderTarget actually render to a framebuffer object. This is useful when
    /// embedding femtovg into another program where final composition is handled by an external task.
    /// The given `framebuffer_object` must refer to a Framebuffer Object created on the current OpenGL
    /// Context, and must have a depth & stencil attachment.
    ///
    /// Pass `None` to clear any previous Framebuffer Object ID that was passed and target rendering to
    /// the default target (normally the window).
    pub fn set_screen_target(&mut self, framebuffer_object: Option<<glow::Context as glow::HasContext>::Framebuffer>) {
        match framebuffer_object {
            Some(fbo_id) => self.screen_target = Some(Framebuffer::from_external(&self.context, fbo_id)),
            None => self.screen_target = None,
        }
    }

    fn render_filtered_image(
        &mut self,
        images: &mut ImageStore<GlTexture>,
        cmd: Command,
        target_image: ImageId,
        filter: ImageFilter,
    ) {
        match filter {
            ImageFilter::GaussianBlur { sigma } => self.render_gaussian_blur(images, cmd, target_image, sigma),
        }
    }

    fn render_gaussian_blur(
        &mut self,
        images: &mut ImageStore<GlTexture>,
        mut cmd: Command,
        target_image: ImageId,
        sigma: f32,
    ) {
        let original_render_target = self.current_render_target;

        // The filtering happens in two passes, first a horizontal blur and then the vertial blur. The
        // first pass therefore renders into an intermediate, temporarily allocated texture.

        let source_image_info = images.get(cmd.image.unwrap()).unwrap().info();

        let image_paint = crate::Paint::image(
            cmd.image.unwrap(),
            0.,
            0.,
            source_image_info.width() as _,
            source_image_info.height() as _,
            0.,
            1.,
        );
        let mut blur_params = Params::new(images, &image_paint, &Scissor::default(), 0., 0., 0.);
        blur_params.shader_type = ShaderType::FilterImage.to_f32();

        let gauss_coeff_x = 1. / ((2. * std::f32::consts::PI).sqrt() * sigma);
        let gauss_coeff_y = f32::exp(-0.5 / (sigma * sigma));
        let gauss_coeff_z = gauss_coeff_y * gauss_coeff_y;

        blur_params.image_blur_filter_coeff[0] = gauss_coeff_x;
        blur_params.image_blur_filter_coeff[1] = gauss_coeff_y;
        blur_params.image_blur_filter_coeff[2] = gauss_coeff_z;

        blur_params.image_blur_filter_direction = [1.0, 0.0];

        // GLES 2.0 does not allow non-constant loop indices, so limit the standard devitation to allow for a upper fixed limit
        // on the number of iterations in the fragment shader.
        blur_params.image_blur_filter_sigma = sigma.min(8.);

        let horizontal_blur_buffer = images.alloc(self, source_image_info).unwrap();
        self.set_target(images, RenderTarget::Image(horizontal_blur_buffer));
        self.main_program.set_view(self.view);

        self.clear_rect(
            0,
            0,
            source_image_info.width() as _,
            source_image_info.height() as _,
            Color::rgbaf(0., 0., 0., 0.),
        );

        self.triangles(images, &cmd, &blur_params);

        self.set_target(images, RenderTarget::Image(target_image));
        self.main_program.set_view(self.view);

        self.clear_rect(
            0,
            0,
            source_image_info.width() as _,
            source_image_info.height() as _,
            Color::rgbaf(0., 0., 0., 0.),
        );

        blur_params.image_blur_filter_direction = [0.0, 1.0];

        cmd.image = Some(horizontal_blur_buffer);

        self.triangles(images, &cmd, &blur_params);

        images.remove(self, horizontal_blur_buffer);

        // restore previous render target and view
        self.set_target(images, original_render_target);
        self.main_program.set_view(self.view);
    }
}

impl Renderer for OpenGl {
    type Image = GlTexture;

    fn set_size(&mut self, width: u32, height: u32, _dpi: f32) {
        self.view[0] = width as f32;
        self.view[1] = height as f32;

        self.screen_view = self.view;

        unsafe {
            self.context.viewport(0, 0, width as i32, height as i32);
        }
    }

    fn render(&mut self, images: &mut ImageStore<Self::Image>, verts: &[Vertex], commands: Vec<Command>) {
        self.main_program.bind();

        unsafe {
            self.context.enable(glow::CULL_FACE);

            self.context.cull_face(glow::BACK);
            self.context.front_face(glow::CCW);
            self.context.enable(glow::BLEND);
            self.context.disable(glow::DEPTH_TEST);
            self.context.disable(glow::SCISSOR_TEST);
            self.context.color_mask(true, true, true, true);
            self.context.stencil_mask(0xffff_ffff);
            self.context.stencil_op(glow::KEEP, glow::KEEP, glow::KEEP);
            self.context.stencil_func(glow::ALWAYS, 0, 0xffff_ffff);
            self.context.active_texture(glow::TEXTURE0);
            self.context.bind_texture(glow::TEXTURE_2D, None);
            self.context.active_texture(glow::TEXTURE0 + 1);
            self.context.bind_texture(glow::TEXTURE_2D, None);

            self.context.bind_vertex_array(self.vert_arr);

            let vertex_size = mem::size_of::<Vertex>();

            self.context.bind_buffer(glow::ARRAY_BUFFER, self.vert_buff);
            self.context
                .buffer_data_u8_slice(glow::ARRAY_BUFFER, verts.align_to().1, glow::STREAM_DRAW);

            self.context.enable_vertex_attrib_array(0);
            self.context.enable_vertex_attrib_array(1);

            self.context
                .vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, vertex_size as i32, 0);
            self.context.vertex_attrib_pointer_f32(
                1,
                2,
                glow::FLOAT,
                false,
                vertex_size as i32,
                2 * mem::size_of::<f32>() as i32,
            );
        }

        // Bind the two uniform samplers to texture units
        self.main_program.set_tex(0);
        self.main_program.set_glyphtex(1);

        self.check_error("render prepare");

        for cmd in commands.into_iter() {
            self.set_composite_operation(cmd.composite_operation);

            match cmd.cmd_type {
                CommandType::ConvexFill { ref params } => self.convex_fill(images, &cmd, params),
                CommandType::ConcaveFill {
                    ref stencil_params,
                    ref fill_params,
                } => self.concave_fill(images, &cmd, stencil_params, fill_params),
                CommandType::Stroke { ref params } => self.stroke(images, &cmd, params),
                CommandType::StencilStroke {
                    ref params1,
                    ref params2,
                } => self.stencil_stroke(images, &cmd, params1, params2),
                CommandType::Triangles { ref params } => self.triangles(images, &cmd, params),
                CommandType::ClearRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                } => {
                    self.clear_rect(x, y, width, height, color);
                }
                CommandType::SetRenderTarget(target) => {
                    self.set_target(images, target);
                    self.main_program.set_view(self.view);
                }
                CommandType::RenderFilteredImage { target_image, filter } => {
                    self.render_filtered_image(images, cmd, target_image, filter)
                }
            }
        }

        unsafe {
            self.context.disable_vertex_attrib_array(0);
            self.context.disable_vertex_attrib_array(1);
            self.context.bind_vertex_array(None);

            self.context.disable(glow::CULL_FACE);
            self.context.bind_buffer(glow::ARRAY_BUFFER, None);
            self.context.bind_texture(glow::TEXTURE_2D, None);
        }

        self.main_program.unbind();

        self.check_error("render done");
    }

    fn alloc_image(&mut self, info: ImageInfo) -> Result<Self::Image, ErrorKind> {
        Self::Image::new(&self.context, info, self.is_opengles_2_0)
    }

    fn update_image(
        &mut self,
        image: &mut Self::Image,
        data: ImageSource,
        x: usize,
        y: usize,
    ) -> Result<(), ErrorKind> {
        image.update(data, x, y, self.is_opengles_2_0)
    }

    fn delete_image(&mut self, image: Self::Image, image_id: ImageId) {
        self.framebuffers.remove(&image_id);
        image.delete();
    }

    fn screenshot(&mut self) -> Result<ImgVec<RGBA8>, ErrorKind> {
        //let mut image = image::RgbaImage::new(self.view[0] as u32, self.view[1] as u32);
        let w = self.view[0] as usize;
        let h = self.view[1] as usize;

        let mut image = ImgVec::new(
            vec![
                RGBA8 {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255
                };
                w * h
            ],
            w,
            h,
        );

        unsafe {
            self.context.read_pixels(
                0,
                0,
                self.view[0] as i32,
                self.view[1] as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(image.buf_mut().align_to_mut().1),
            );
        }

        let mut flipped = Vec::with_capacity(w * h);

        for row in image.rows().rev() {
            flipped.extend_from_slice(row);
        }

        Ok(ImgVec::new(flipped, w, h))
    }
}

impl Drop for OpenGl {
    fn drop(&mut self) {
        if let Some(vert_arr) = self.vert_arr {
            unsafe {
                self.context.delete_vertex_array(vert_arr);
            }
        }

        if let Some(vert_buff) = self.vert_buff {
            unsafe {
                self.context.delete_buffer(vert_buff);
            }
        }
    }
}
