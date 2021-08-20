use imgref::ImgVec;
use rgb::RGBA8;

use miniquad as mq;
type GlTexture = mq::Texture;

use crate::{
    renderer::{ImageId, Vertex},
    BlendFactor, Color, CompositeOperationState, ErrorKind, FillRule, ImageFilter, ImageFlags, ImageInfo, ImageSource,
    ImageStore, PixelFormat, Scissor,
};

use super::{Command, CommandType, Params, RenderTarget, Renderer, ShaderType};

pub struct Miniquad {
    debug: bool,
    antialias: bool,
    is_opengles_2_0: bool,
    view: [f32; 2],
    screen_view: [f32; 2],
    pipeline: mq::Pipeline,
    bindings: mq::Bindings,
    empty_texture: mq::Texture,
    // vert_arr: Option<<glow::Context as glow::HasContext>::VertexArray>,
    // vert_buff: Option<<glow::Context as glow::HasContext>::Buffer>,
    // framebuffers: FnvHashMap<ImageId, Result<Framebuffer, ErrorKind>>,
    ctx: mq::Context,
    // screen_target: Option<Framebuffer>,
    current_render_target: RenderTarget,
}

mod shader {
    use miniquad::*;

    pub const GLSL_VERSION: &str = "#version 100";
    pub const VERTEX: &str = include_str!("opengl/main-vs.glsl");
    pub const FRAGMENT: &str = include_str!("opengl/main-fs.glsl");

    pub const MAX_VERTICES: usize = 21845; // u16.max / 3 due to index buffer limitations
    pub const MAX_INDICES: usize = u16::max_value() as usize;

    pub const ATTRIBUTES: &[VertexAttribute] = &[
        VertexAttribute::new("vertex", VertexFormat::Float2),
        VertexAttribute::new("tcoord", VertexFormat::Float2),
    ];
    pub fn meta() -> ShaderMeta {
        ShaderMeta {
            images: vec!["tex".to_string(), "masktex".to_string()],
            uniforms: UniformBlockLayout {
                uniforms: vec![
                    UniformDesc::new("viewSize", UniformType::Float2),
                    UniformDesc::new("scissorMat", UniformType::Mat4),
                    UniformDesc::new("paintMat", UniformType::Mat4),
                    UniformDesc::new("innerCol", UniformType::Float4),
                    UniformDesc::new("outerCol", UniformType::Float4),
                    UniformDesc::new("scissorExt", UniformType::Float2),
                    UniformDesc::new("scissorScale", UniformType::Float2),
                    UniformDesc::new("extent", UniformType::Float2),
                    UniformDesc::new("radius", UniformType::Float1),
                    UniformDesc::new("feather", UniformType::Float1),
                    UniformDesc::new("strokeMult", UniformType::Float1),
                    UniformDesc::new("strokeThr", UniformType::Float1),
                    UniformDesc::new("texType", UniformType::Int1),
                    UniformDesc::new("shaderType", UniformType::Int1),
                    UniformDesc::new("hasMask", UniformType::Int1),
                    UniformDesc::new("imageBlurFilterDirection", UniformType::Float2),
                    UniformDesc::new("imageBlurFilterSigma", UniformType::Float1),
                    UniformDesc::new("imageBlurFilterCoeff", UniformType::Float3),
                ],
            },
        }
    }

    #[derive(Default)]
    #[repr(C)]
    pub struct Uniforms {
        pub view_size: [f32; 2],
        pub scissor_mat: glam::Mat4,
        pub paint_mat: glam::Mat4,
        pub inner_col: [f32; 4],
        pub outer_col: [f32; 4],
        pub scissor_ext: [f32; 2],
        pub scissor_scale: [f32; 2],
        pub extent: [f32; 2],
        pub radius: f32,
        pub feather: f32,
        pub stroke_mult: f32,
        pub stroke_thr: f32,
        pub tex_type: i32,
        pub shader_type: i32,
        pub has_mask: i32,
        pub image_blur_filter_direction: [f32; 2],
        pub image_blur_filter_sigma: f32,
        pub image_blur_filter_coeff: [f32; 3],
    }
}

impl From<&Params> for shader::Uniforms {
    fn from(params: &Params) -> Self {
        let mut scissor_vec = params.scissor_mat.to_vec();
        scissor_vec.extend([0., 0., 0., 1.].to_vec());
        let mut paint_vec = params.paint_mat.to_vec();
        paint_vec.extend([0., 0., 0., 1.].to_vec());

        Self {
            scissor_mat: glam::Mat4::from_cols_slice(scissor_vec.as_slice()),
            paint_mat: glam::Mat4::from_cols_slice(paint_vec.as_slice()),
            inner_col: params.inner_col,
            outer_col: params.outer_col,
            scissor_ext: params.scissor_ext,
            scissor_scale: params.scissor_scale,
            extent: params.extent,
            radius: params.radius,
            feather: params.feather,
            stroke_mult: params.stroke_mult,
            stroke_thr: params.stroke_thr,
            tex_type: params.tex_type as i32,
            shader_type: params.shader_type as i32,
            has_mask: params.has_mask as i32,
            image_blur_filter_direction: params.image_blur_filter_direction,
            image_blur_filter_sigma: params.image_blur_filter_sigma,
            image_blur_filter_coeff: params.image_blur_filter_coeff,
            ..Default::default()
        }
    }
}

impl Miniquad {
    pub fn new(mut ctx: mq::Context) -> Result<Self, ErrorKind> {
        let debug = cfg!(debug_assertions);
        let antialias = true;

        let shader_defs = if antialias { "#define EDGE_AA 1" } else { "" };
        let vert_shader_src = format!("{}\n{}\n{}", shader::GLSL_VERSION, shader_defs, shader::VERTEX);
        let frag_shader_src = format!("{}\n{}\n{}", shader::GLSL_VERSION, shader_defs, shader::FRAGMENT);

        let shader = mq::Shader::new(
            &mut ctx,
            vert_shader_src.as_str(),
            frag_shader_src.as_str(),
            shader::meta(),
        )
        .map_err(|error| ErrorKind::ShaderCompileError(error.to_string()))?;
        let pipeline = mq::Pipeline::with_params(
            &mut ctx,
            &[mq::BufferLayout::default()],
            shader::ATTRIBUTES,
            shader,
            mq::PipelineParams {
                depth_write: false,
                color_blend: None,
                color_write: (true, true, true, true),
                front_face_order: mq::FrontFaceOrder::CounterClockwise,
                ..Default::default()
            },
        );

        let vertex_buffer = mq::Buffer::stream(
            &mut ctx,
            mq::BufferType::VertexBuffer,
            shader::MAX_VERTICES * std::mem::size_of::<Vertex>(),
        );
        let index_buffer = mq::Buffer::stream(
            &mut ctx,
            mq::BufferType::IndexBuffer,
            shader::MAX_INDICES * std::mem::size_of::<u16>(),
        );

        let empty_texture = mq::Texture::new_render_texture(&mut ctx, mq::TextureParams::default());

        let bindings = mq::Bindings {
            vertex_buffers: vec![vertex_buffer],
            index_buffer,
            images: vec![empty_texture, empty_texture],
        };

        let mq = Miniquad {
            debug,
            antialias,
            is_opengles_2_0: false,
            view: [0.0, 0.0],
            screen_view: [0.0, 0.0],
            pipeline,
            bindings,
            empty_texture,
            // vert_arr: Default::default(),
            // vert_buff: Default::default(),
            // framebuffers: Default::default(),
            ctx,
            // screen_target: None,
            current_render_target: RenderTarget::Screen,
        };

        Ok(mq)
    }

    pub fn is_opengles(&self) -> bool {
        self.is_opengles_2_0
    }

    fn check_error(&self, label: &str) {
        if !self.debug {
            return;
        }

        // let err = unsafe { self.context.get_error() };

        // if err == glow::NO_ERROR {
        //     return;
        // }

        // let message = match err {
        //     glow::INVALID_ENUM => "Invalid enum",
        //     glow::INVALID_VALUE => "Invalid value",
        //     glow::INVALID_OPERATION => "Invalid operation",
        //     glow::OUT_OF_MEMORY => "Out of memory",
        //     glow::INVALID_FRAMEBUFFER_OPERATION => "Invalid framebuffer operation",
        //     _ => "Unknown error",
        // };

        // eprintln!("({}) Error on {} - {}", err, label, message);
    }

    fn gl_factor(factor: BlendFactor) -> mq::BlendFactor {
        match factor {
            BlendFactor::Zero => mq::BlendFactor::Zero,
            BlendFactor::One => mq::BlendFactor::One,
            BlendFactor::SrcColor => mq::BlendFactor::Value(mq::BlendValue::SourceColor),
            BlendFactor::OneMinusSrcColor => mq::BlendFactor::OneMinusValue(mq::BlendValue::SourceColor),
            BlendFactor::DstColor => mq::BlendFactor::Value(mq::BlendValue::DestinationColor),
            BlendFactor::OneMinusDstColor => mq::BlendFactor::OneMinusValue(mq::BlendValue::DestinationColor),
            BlendFactor::SrcAlpha => mq::BlendFactor::Value(mq::BlendValue::SourceAlpha),
            BlendFactor::OneMinusSrcAlpha => mq::BlendFactor::OneMinusValue(mq::BlendValue::SourceAlpha),
            BlendFactor::DstAlpha => mq::BlendFactor::Value(mq::BlendValue::DestinationAlpha),
            BlendFactor::OneMinusDstAlpha => mq::BlendFactor::OneMinusValue(mq::BlendValue::DestinationAlpha),
            BlendFactor::SrcAlphaSaturate => mq::BlendFactor::SourceAlphaSaturate,
        }
    }

    fn set_composite_operation(&mut self, blend_state: CompositeOperationState) {
        self.ctx.set_blend(
            Some(mq::BlendState::new(
                mq::Equation::Add,
                Self::gl_factor(blend_state.src_rgb),
                Self::gl_factor(blend_state.dst_rgb),
            )),
            Some(mq::BlendState::new(
                mq::Equation::Add,
                Self::gl_factor(blend_state.src_alpha),
                Self::gl_factor(blend_state.dst_alpha),
            )),
        );
    }

    // from https://www.khronos.org/opengl/wiki/Primitive:
    // GL_TRIANGLE_FAN:
    // Indices:     0 1 2 3 4 5 ... (6 total indices)
    // Triangles:  {0 1 2}
    //             {0} {2 3}
    //             {0}   {3 4}
    //             {0}     {4 5}    (4 total triangles)
    //
    // GL_TRIANGLES:
    // Indices:     0 1 2 3 4 5 ...
    // Triangles:  {0 1 2}
    //                   {3 4 5}
    /// Adds indices to convert from GL_TRIANGLE_FAN to GL_TRIANGLES
    #[inline]
    fn add_triangle_fan(indices: &mut Vec<u16>, first_vertex_index: u16, index_count: u16) {
        let start_index = first_vertex_index;
        for i in first_vertex_index..first_vertex_index + index_count - 2 {
            indices.push(start_index);
            indices.push(i + 1);
            indices.push(i + 2);
        }
    }

    // from https://www.khronos.org/opengl/wiki/Primitive:
    // GL_TRIANGLES:
    // Indices:     0 1 2 3 4 5 ... (6 total indices)
    // Triangles:  {0 1 2}
    //                   {3 4 5}    (2 total indices)
    /// Adds indices to draw GL_TRIANGLES
    #[inline]
    fn add_triangles(indices: &mut Vec<u16>, first_vertex_index: u16, index_count: u16) {
        // TODO: test!
        for i in (first_vertex_index..first_vertex_index + index_count).step_by(3) {
            indices.push(i);
            indices.push(i + 1);
            indices.push(i + 2);
        }
    }

    // from https://www.khronos.org/opengl/wiki/Primitive:
    // GL_TRIANGLE_STRIP:
    // Indices:     0 1 2 3 4 5 ... (6 total indices)
    // Triangles:  {0 1 2}
    //               {1 2 3}  drawing order is (2 1 3) to maintain proper winding
    //                 {2 3 4}
    //                   {3 4 5}  drawing order is (4 3 5) to maintain proper winding (4 total triangles)
    //
    // GL_TRIANGLES:
    // Indices:     0 1 2 3 4 5 ...
    // Triangles:  {0 1 2}
    //                   {3 4 5}
    /// Adds indices to convert from GL_TRIANGLE_STRIP to GL_TRIANGLES
    #[inline]
    fn add_triangle_strip(indices: &mut Vec<u16>, first_vertex_index: u16, index_count: u16) {
        let mut draw_order_winding = true; // true to draw in straight (0 1 2) order; false to draw in (1 0 2) order to maintain proper winding
        for i in first_vertex_index..first_vertex_index + index_count - 2 {
            if draw_order_winding {
                indices.push(i);
                indices.push(i + 1);
            } else {
                indices.push(i + 1);
                indices.push(i);
            }
            draw_order_winding = !draw_order_winding;
            indices.push(i + 2);
        }
    }

    fn convex_fill(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, gpu_paint: &Params) {
        let mut indices = Vec::new();
        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.fill_verts {
                Self::add_triangle_fan(&mut indices, start as u16, count as u16);
            }

            if let Some((start, count)) = drawable.stroke_verts {
                Self::add_triangle_strip(&mut indices, start as u16, count as u16);
            }
        }

        self.set_uniforms(images, gpu_paint, cmd.image, cmd.alpha_mask, &indices);
        self.ctx.draw(0, indices.len() as i32, 1);
        self.check_error("convex_fill");
    }

    fn concave_fill(
        &mut self,
        images: &ImageStore<GlTexture>,
        cmd: &Command,
        stencil_paint: &Params,
        fill_paint: &Params,
    ) {
        let mut stencil_state = mq::StencilState {
            front: mq::StencilFaceState {
                fail_op: mq::StencilOp::Keep,
                depth_fail_op: mq::StencilOp::Keep,
                pass_op: mq::StencilOp::IncrementWrap,
                test_func: mq::CompareFunc::Always,
                test_ref: 0,
                test_mask: 0xff,
                write_mask: 0xff,
            },
            back: mq::StencilFaceState {
                fail_op: mq::StencilOp::Keep,
                depth_fail_op: mq::StencilOp::Keep,
                pass_op: mq::StencilOp::DecrementWrap,
                test_func: mq::CompareFunc::Always,
                test_ref: 0,
                test_mask: 0xff,
                write_mask: 0xff,
            },
        };

        self.ctx.set_stencil(Some(stencil_state));
        self.ctx.set_color_write((false, false, false, false));

        self.ctx.set_cull_face(mq::CullFace::Nothing);

        let mut indices = Vec::new();
        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.fill_verts {
                Self::add_triangle_fan(&mut indices, start as u16, count as u16);
            }
        }
        self.set_uniforms(images, stencil_paint, None, None, &indices);
        self.ctx.draw(0, indices.len() as i32, 1);

        self.ctx.set_color_write((true, true, true, true));

        if self.antialias {
            match cmd.fill_rule {
                FillRule::NonZero => {
                    stencil_state.front.test_func = mq::CompareFunc::Equal;
                    stencil_state.front.test_mask = 0xff;
                    stencil_state.back.test_func = mq::CompareFunc::Equal;
                    stencil_state.back.test_mask = 0xff;
                }
                FillRule::EvenOdd => {
                    stencil_state.front.test_func = mq::CompareFunc::Equal;
                    stencil_state.front.test_mask = 0x1;
                    stencil_state.back.test_func = mq::CompareFunc::Equal;
                    stencil_state.back.test_mask = 0x1;
                }
            }
            stencil_state.front.pass_op = mq::StencilOp::Keep;
            stencil_state.back.pass_op = mq::StencilOp::Keep;

            self.ctx.set_stencil(Some(stencil_state));

            // draw fringes
            indices.clear();
            for drawable in &cmd.drawables {
                if let Some((start, count)) = drawable.stroke_verts {
                    Self::add_triangle_strip(&mut indices, start as u16, count as u16);
                }
            }
            self.set_uniforms(images, fill_paint, cmd.image, cmd.alpha_mask, &indices);
            self.ctx.draw(0, indices.len() as i32, 1);
        }

        match cmd.fill_rule {
            FillRule::NonZero => {
                stencil_state.front.test_func = mq::CompareFunc::NotEqual;
                stencil_state.front.test_mask = 0xff;
                stencil_state.back.test_func = mq::CompareFunc::NotEqual;
                stencil_state.back.test_mask = 0xff;
            }
            FillRule::EvenOdd => {
                stencil_state.front.test_func = mq::CompareFunc::Equal;
                stencil_state.front.test_mask = 0x1;
                stencil_state.back.test_func = mq::CompareFunc::Equal;
                stencil_state.back.test_mask = 0x1;
            }
        }
        stencil_state.front.fail_op = mq::StencilOp::Zero;
        stencil_state.front.depth_fail_op = mq::StencilOp::Zero;
        stencil_state.front.pass_op = mq::StencilOp::Zero;
        stencil_state.back.fail_op = mq::StencilOp::Zero;
        stencil_state.back.depth_fail_op = mq::StencilOp::Zero;
        stencil_state.back.pass_op = mq::StencilOp::Zero;

        self.ctx.set_stencil(Some(stencil_state));

        indices.clear();
        if let Some((start, count)) = cmd.triangles_verts {
            Self::add_triangle_strip(&mut indices, start as u16, count as u16);
        }
        self.set_uniforms(images, fill_paint, cmd.image, cmd.alpha_mask, &indices);
        self.ctx.draw(0, indices.len() as i32, 1);

        self.ctx.set_stencil(None);

        self.check_error("concave_fill");
    }

    fn stroke(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, paint: &Params) {
        let mut indices = Vec::new();
        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                Self::add_triangle_strip(&mut indices, start as u16, count as u16);
            }
        }

        self.set_uniforms(images, paint, cmd.image, cmd.alpha_mask, &indices);
        self.ctx.draw(0, indices.len() as i32, 1);
        self.check_error("stroke");
    }

    fn stencil_stroke(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, paint1: &Params, paint2: &Params) {
        let mut stencil_state = mq::StencilState {
            front: mq::StencilFaceState {
                fail_op: mq::StencilOp::Keep,
                depth_fail_op: mq::StencilOp::Keep,
                pass_op: mq::StencilOp::IncrementClamp,
                test_func: mq::CompareFunc::Equal,
                test_ref: 0,
                test_mask: 0xff,
                write_mask: 0xff,
            },
            back: mq::StencilFaceState {
                fail_op: mq::StencilOp::Keep,
                depth_fail_op: mq::StencilOp::Keep,
                pass_op: mq::StencilOp::IncrementClamp,
                test_func: mq::CompareFunc::Equal,
                test_ref: 0,
                test_mask: 0xff,
                write_mask: 0xff,
            },
        };

        // Fill the stroke base without overlap
        self.ctx.set_stencil(Some(stencil_state));

        let mut indices = Vec::new();
        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                Self::add_triangle_strip(&mut indices, start as u16, count as u16);
            }
        }

        self.set_uniforms(images, paint2, cmd.image, cmd.alpha_mask, &indices);
        self.ctx.draw(0, indices.len() as i32, 1);

        // Draw anti-aliased pixels.
        stencil_state.front.pass_op = mq::StencilOp::Keep;
        stencil_state.back.pass_op = mq::StencilOp::Keep;
        self.ctx.set_stencil(Some(stencil_state));

        indices.clear();
        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                Self::add_triangle_strip(&mut indices, start as u16, count as u16);
            }
        }

        self.set_uniforms(images, paint1, cmd.image, cmd.alpha_mask, &indices);
        self.ctx.draw(0, indices.len() as i32, 1);

        // Clear stencil buffer.
        self.ctx.set_color_write((false, false, false, false));
        stencil_state.front.test_func = mq::CompareFunc::Always;
        stencil_state.front.fail_op = mq::StencilOp::Zero;
        stencil_state.front.depth_fail_op = mq::StencilOp::Zero;
        stencil_state.front.pass_op = mq::StencilOp::Zero;
        stencil_state.back.test_func = mq::CompareFunc::Always;
        stencil_state.back.fail_op = mq::StencilOp::Zero;
        stencil_state.back.depth_fail_op = mq::StencilOp::Zero;
        stencil_state.back.pass_op = mq::StencilOp::Zero;

        indices.clear();
        for drawable in &cmd.drawables {
            if let Some((start, count)) = drawable.stroke_verts {
                Self::add_triangle_strip(&mut indices, start as u16, count as u16);
            }
        }

        self.ctx.set_color_write((true, true, true, true));
        self.ctx.set_stencil(None);

        self.set_uniforms(images, paint1, cmd.image, cmd.alpha_mask, &indices);
        self.ctx.draw(0, indices.len() as i32, 1);

        self.check_error("stencil_stroke");
    }

    fn triangles(&mut self, images: &ImageStore<GlTexture>, cmd: &Command, paint: &Params) {
        let mut indices = Vec::new();
        if let Some((start, count)) = cmd.triangles_verts {
            Self::add_triangles(&mut indices, start as u16, count as u16);
        }

        self.set_uniforms(images, paint, cmd.image, cmd.alpha_mask, &indices);
        self.ctx.draw(0, indices.len() as i32, 1);
        self.check_error("triangles");
    }

    fn set_uniforms(
        &mut self,
        images: &ImageStore<GlTexture>,
        paint: &Params,
        image_tex: Option<ImageId>,
        alpha_tex: Option<ImageId>,
        indices: &Vec<u16>,
    ) {
        let mut uniforms = shader::Uniforms::from(paint);
        uniforms.view_size = self.view;
        self.ctx.apply_uniforms(&uniforms);
        self.check_error("set_uniforms uniforms");

        let tex = image_tex.and_then(|id| images.get(id)).unwrap_or(&self.empty_texture);
        self.bindings.images[0] = *tex;

        let masktex = alpha_tex.and_then(|id| images.get(id)).unwrap_or(&self.empty_texture);
        self.bindings.images[1] = *masktex;

        self.bindings.index_buffer.update(&mut self.ctx, indices);
        self.ctx.apply_bindings(&mut self.bindings);
        self.check_error("set_uniforms texture");
    }

    fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        self.ctx.apply_scissor_rect(
            x as i32,
            self.view[1] as i32 - (height as i32 + y as i32),
            width as i32,
            height as i32,
        );
        self.ctx.clear(Some((color.r, color.g, color.b, color.a)), None, None);
        let screen_size = self.ctx.screen_size();
        self.ctx
            .apply_scissor_rect(0, 0, screen_size.0 as i32, screen_size.1 as i32);
    }

    fn set_target(&mut self, images: &ImageStore<GlTexture>, target: RenderTarget) {
        self.current_render_target = target;
        match (target, None as Option<()> /*&self.screen_target*/) {
            (RenderTarget::Screen, None) => {
                // Framebuffer::unbind(&self.context);
                self.view = self.screen_view;
                self.ctx.apply_viewport(0, 0, self.view[0] as i32, self.view[1] as i32);
            }
            (RenderTarget::Screen, Some(framebuffer)) => {
                todo!();
                // framebuffer.bind();
                // self.view = self.screen_view;
                // unsafe {
                //     self.context.viewport(0, 0, self.view[0] as i32, self.view[1] as i32);
                // }
            }
            (RenderTarget::Image(id), _) => {
                todo!();
                // let context = self.context.clone();
                // if let Some(texture) = images.get(id) {
                //     if let Ok(fb) = self
                //         .framebuffers
                //         .entry(id)
                //         .or_insert_with(|| Framebuffer::new(&context, texture))
                //     {
                //         fb.bind();

                //         self.view[0] = texture.info().width() as f32;
                //         self.view[1] = texture.info().height() as f32;

                //         unsafe {
                //             self.context
                //                 .viewport(0, 0, texture.info().width() as i32, texture.info().height() as i32);
                //         }
                //     }
                // }
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
        todo!();
        // match framebuffer_object {
        //     Some(fbo_id) => self.screen_target = Some(Framebuffer::from_external(&self.context, fbo_id)),
        //     None => self.screen_target = None,
        // }
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
        todo!();
        // let original_render_target = self.current_render_target;

        // // The filtering happens in two passes, first a horizontal blur and then the vertial blur. The
        // // first pass therefore renders into an intermediate, temporarily allocated texture.

        // let source_image_info = images.get(cmd.image.unwrap()).unwrap().info();

        // let image_paint = crate::Paint::image(
        //     cmd.image.unwrap(),
        //     0.,
        //     0.,
        //     source_image_info.width() as _,
        //     source_image_info.height() as _,
        //     0.,
        //     1.,
        // );
        // let mut blur_params = Params::new(images, &image_paint, &Scissor::default(), 0., 0., 0.);
        // blur_params.shader_type = ShaderType::FilterImage.to_f32();

        // let gauss_coeff_x = 1. / ((2. * std::f32::consts::PI).sqrt() * sigma);
        // let gauss_coeff_y = f32::exp(-0.5 / (sigma * sigma));
        // let gauss_coeff_z = gauss_coeff_y * gauss_coeff_y;

        // blur_params.image_blur_filter_coeff[0] = gauss_coeff_x;
        // blur_params.image_blur_filter_coeff[1] = gauss_coeff_y;
        // blur_params.image_blur_filter_coeff[2] = gauss_coeff_z;

        // blur_params.image_blur_filter_direction = [1.0, 0.0];

        // // GLES 2.0 does not allow non-constant loop indices, so limit the standard devitation to allow for a upper fixed limit
        // // on the number of iterations in the fragment shader.
        // blur_params.image_blur_filter_sigma = sigma.min(8.);

        // let horizontal_blur_buffer = images.alloc(self, source_image_info).unwrap();
        // self.set_target(images, RenderTarget::Image(horizontal_blur_buffer));
        // self.main_program.set_view(self.view);

        // self.clear_rect(
        //     0,
        //     0,
        //     source_image_info.width() as _,
        //     source_image_info.height() as _,
        //     Color::rgbaf(0., 0., 0., 0.),
        // );

        // self.triangles(images, &cmd, &blur_params);

        // self.set_target(images, RenderTarget::Image(target_image));
        // self.main_program.set_view(self.view);

        // self.clear_rect(
        //     0,
        //     0,
        //     source_image_info.width() as _,
        //     source_image_info.height() as _,
        //     Color::rgbaf(0., 0., 0., 0.),
        // );

        // blur_params.image_blur_filter_direction = [0.0, 1.0];

        // cmd.image = Some(horizontal_blur_buffer);

        // self.triangles(images, &cmd, &blur_params);

        // images.remove(self, horizontal_blur_buffer);

        // // restore previous render target and view
        // self.set_target(images, original_render_target);
        // self.main_program.set_view(self.view);
    }
}

pub fn femtovg_pixel_format_to_mq(format: PixelFormat) -> mq::TextureFormat {
    match format {
        PixelFormat::Rgb8 => mq::TextureFormat::RGB8,
        PixelFormat::Rgba8 => mq::TextureFormat::RGBA8,
        PixelFormat::Gray8 => mq::TextureFormat::Alpha,
    }
}

impl Renderer for Miniquad {
    type Image = GlTexture;

    fn set_size(&mut self, width: u32, height: u32, _dpi: f32) {
        self.view[0] = width as f32;
        self.view[1] = height as f32;

        self.screen_view = self.view;

        self.ctx.apply_viewport(0, 0, width as i32, height as i32);
    }

    fn render(&mut self, images: &mut ImageStore<Self::Image>, verts: &[Vertex], commands: Vec<Command>) {
        self.ctx.begin_default_pass(mq::PassAction::Nothing);
        self.ctx.apply_pipeline(&self.pipeline);

        // unsafe {
        self.ctx.set_cull_face(mq::CullFace::Back);

        //     self.context.cull_face(glow::BACK);
        //     self.context.front_face(glow::CCW);
        //     self.context.enable(glow::BLEND);
        //     self.context.disable(glow::DEPTH_TEST);
        //     self.context.disable(glow::SCISSOR_TEST);
        //     self.context.color_mask(true, true, true, true);
        //     self.context.stencil_mask(0xffff_ffff);
        //     self.context.stencil_op(glow::KEEP, glow::KEEP, glow::KEEP);
        //     self.context.stencil_func(glow::ALWAYS, 0, 0xffff_ffff);
        //     self.context.active_texture(glow::TEXTURE0);
        //     self.context.bind_texture(glow::TEXTURE_2D, None);
        //     self.context.active_texture(glow::TEXTURE0 + 1);
        //     self.context.bind_texture(glow::TEXTURE_2D, None);

        self.ctx.apply_bindings(&self.bindings);
        self.bindings.vertex_buffers[0].update(&mut self.ctx, verts);

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
                    // self.main_program.set_view(self.view);
                }
                CommandType::RenderFilteredImage { target_image, filter } => {
                    self.render_filtered_image(images, cmd, target_image, filter)
                }
            }
        }

        // unsafe {
        //     self.context.disable_vertex_attrib_array(0);
        //     self.context.disable_vertex_attrib_array(1);
        //     self.context.bind_vertex_array(None);

        //     self.context.disable(glow::CULL_FACE);
        //     self.context.bind_buffer(glow::ARRAY_BUFFER, None);
        //     self.context.bind_texture(glow::TEXTURE_2D, None);
        // }

        self.ctx.end_render_pass();
        self.ctx.commit_frame(); // FIXME: miniquad context should not be bound in self!

        self.check_error("render done");
    }

    fn alloc_image(&mut self, info: ImageInfo) -> Result<Self::Image, ErrorKind> {
        Ok(Self::Image::new_render_texture(
            &mut self.ctx,
            mq::TextureParams {
                format: femtovg_pixel_format_to_mq(info.format()),
                wrap: if info.flags().contains(ImageFlags::REPEAT_X | ImageFlags::REPEAT_Y) {
                    mq::TextureWrap::Repeat
                } else if info.flags().contains(ImageFlags::FLIP_Y) {
                    mq::TextureWrap::Mirror
                } else {
                    mq::TextureWrap::Clamp
                },
                filter: if info.flags().contains(ImageFlags::NEAREST) {
                    mq::FilterMode::Nearest
                } else {
                    mq::FilterMode::Linear
                },
                width: info.width() as u32,
                height: info.height() as u32,
            },
        ))
    }

    fn update_image(&mut self, image: &mut Self::Image, src: ImageSource, x: usize, y: usize) -> Result<(), ErrorKind> {
        let size = src.dimensions();

        if x + size.0 > image.width as usize {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        if y + size.1 > image.height as usize {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        if image.format != femtovg_pixel_format_to_mq(src.format()) {
            return Err(ErrorKind::ImageUpdateWithDifferentFormat);
        }

        match src {
            ImageSource::Gray(data) => {
                image.update_texture_part(
                    &mut self.ctx,
                    x as i32,
                    y as i32,
                    size.0 as i32,
                    size.1 as i32,
                    data.buf().iter().map(|c| **c).collect::<Vec<u8>>().as_slice(),
                );
            }
            ImageSource::Rgb(data) => {
                image.update_texture_part(
                    &mut self.ctx,
                    x as i32,
                    y as i32,
                    size.0 as i32,
                    size.1 as i32,
                    data.buf()
                        .iter()
                        .map(|c| [c.r, c.g, c.b])
                        .flatten()
                        .collect::<Vec<u8>>()
                        .as_slice(),
                );
            }
            ImageSource::Rgba(data) => {
                image.update_texture_part(
                    &mut self.ctx,
                    x as i32,
                    y as i32,
                    size.0 as i32,
                    size.1 as i32,
                    data.buf()
                        .iter()
                        .map(|c| [c.r, c.g, c.b, c.a])
                        .flatten()
                        .collect::<Vec<u8>>()
                        .as_slice(),
                );
            }
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

        // if self.info.flags().contains(ImageFlags::GENERATE_MIPMAPS) {
        //     unsafe {
        //         self.context.generate_mipmap(glow::TEXTURE_2D);
        //         //glow::TexParameteri(glow::TEXTURE_2D, glow::GENERATE_MIPMAP, glow::TRUE);
        //     }
        // }

        // unsafe {
        //     self.context.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
        //     if !opengles_2_0 {
        //         self.context.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
        //     }
        //     //glow::PixelStorei(glow::UNPACK_SKIP_PIXELS, 0);
        //     //glow::PixelStorei(glow::UNPACK_SKIP_ROWS, 0);
        //     self.context.bind_texture(glow::TEXTURE_2D, None);
        // }

        Ok(())
    }

    fn delete_image(&mut self, image: Self::Image, image_id: ImageId) {
        // self.framebuffers.remove(&image_id);
        image.delete();
    }

    fn screenshot(&mut self) -> Result<ImgVec<RGBA8>, ErrorKind> {
        todo!();
        // //let mut image = image::RgbaImage::new(self.view[0] as u32, self.view[1] as u32);
        // let w = self.view[0] as usize;
        // let h = self.view[1] as usize;

        // let mut image = ImgVec::new(
        //     vec![
        //         RGBA8 {
        //             r: 255,
        //             g: 255,
        //             b: 255,
        //             a: 255
        //         };
        //         w * h
        //     ],
        //     w,
        //     h,
        // );

        // unsafe {
        //     self.context.read_pixels(
        //         0,
        //         0,
        //         self.view[0] as i32,
        //         self.view[1] as i32,
        //         glow::RGBA,
        //         glow::UNSIGNED_BYTE,
        //         glow::PixelPackData::Slice(image.buf_mut().align_to_mut().1),
        //     );
        // }

        // let mut flipped = Vec::with_capacity(w * h);

        // for row in image.rows().rev() {
        //     flipped.extend_from_slice(row);
        // }

        // Ok(ImgVec::new(flipped, w, h))
    }
}

impl Drop for Miniquad {
    fn drop(&mut self) {
        self.empty_texture.delete();
    }
}
