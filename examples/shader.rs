use std::num::NonZeroU32;

use femtovg::{renderer::OpenGl, Canvas, Color, Paint, Path};
use glow::{Context, HasContext, NativeFramebuffer, NativeProgram, NativeTexture, Program};
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder},
    display::GetGlDisplay,
    prelude::*,
    surface::{SurfaceAttributesBuilder, WindowSurface},
};
use glutin_winit::DisplayBuilder;
use raw_window_handle::HasWindowHandle;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::Window,
};

const WINDOW_WIDTH: f32 = 640.0;
const WINDOW_HEIGHT: f32 = 480.0;

struct App {
    // State after resumed
    state: Option<AppState>,
}

struct AppState {
    window: Window,
    gl_context: glutin::context::PossiblyCurrentContext,
    surface: glutin::surface::Surface<WindowSurface>,
    context: Context,
    framebuffer: NativeFramebuffer,
    texture_colorbuffer: NativeTexture,
    shader_program: Program,
    canvas: Canvas<OpenGl>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window_attrs = Window::default_attributes()
            .with_inner_size(winit::dpi::PhysicalSize::<f32>::new(WINDOW_WIDTH, WINDOW_HEIGHT))
            .with_resizable(false)
            .with_decorations(true)
            .with_title("SHADER");

        let template = ConfigTemplateBuilder::new().with_alpha_size(8);

        let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attrs));

        let (window, gl_config) = display_builder
            .build(event_loop, template, |configs| {
                configs
                    .reduce(|accum, config| {
                        let transparency_check = config.supports_transparency().unwrap_or(false)
                            & !accum.supports_transparency().unwrap_or(false);

                        if transparency_check || config.num_samples() < accum.num_samples() {
                            config
                        } else {
                            accum
                        }
                    })
                    .unwrap()
            })
            .unwrap();

        let window = window.unwrap();

        let raw_window_handle = window.window_handle().unwrap().as_raw();

        let gl_display = gl_config.display();

        let context_attributes = ContextAttributesBuilder::new().build(Some(raw_window_handle));
        let fallback_context_attributes = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(None))
            .build(Some(raw_window_handle));
        let mut not_current_gl_context = Some(unsafe {
            gl_display
                .create_context(&gl_config, &context_attributes)
                .unwrap_or_else(|_| {
                    gl_display
                        .create_context(&gl_config, &fallback_context_attributes)
                        .expect("failed to create context")
                })
        });

        let (width, height): (u32, u32) = window.inner_size().into();
        let raw_window_handle = window.window_handle().unwrap().as_raw();
        let attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw_window_handle,
            NonZeroU32::new(width).unwrap(),
            NonZeroU32::new(height).unwrap(),
        );

        let surface = unsafe { gl_config.display().create_window_surface(&gl_config, &attrs).unwrap() };

        let gl_context = not_current_gl_context.take().unwrap().make_current(&surface).unwrap();

        let context: Context;
        let framebuffer: NativeFramebuffer;
        let texture_colorbuffer: NativeTexture;
        let shader_program: Program;
        let mut renderer: OpenGl;

        unsafe {
            context = glow::Context::from_loader_function_cstr(|symbol| gl_display.get_proc_address(symbol).cast());
            renderer = OpenGl::new_from_function_cstr(|s| gl_display.get_proc_address(s).cast())
                .expect("Cannot create renderer");

            shader_program = create_shader_program(&context);
            (framebuffer, texture_colorbuffer) = create_framebuffer_colorbuffer(&context);

            renderer.set_screen_target(Some(framebuffer));
        }

        let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
        canvas.set_size(WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32, 1.0);

        self.state = Some(AppState {
            window,
            gl_context,
            surface,
            context,
            framebuffer,
            texture_colorbuffer,
            shader_program,
            canvas,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: winit::window::WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                prepare_framebuffer_for_render(&state.context, state.framebuffer);

                let dpi_factor = state.window.scale_factor();
                let size = state.window.inner_size();
                state.canvas.set_size(size.width, size.height, dpi_factor as f32);
                state
                    .canvas
                    .clear_rect(0, 0, size.width, size.height, Color::rgbf(1., 1., 1.));

                state.canvas.save();

                let paint = Paint::color(Color::rgbf(1., 0., 0.));
                let mut path = Path::new();
                path.rect(WINDOW_WIDTH / 2. - 25., WINDOW_HEIGHT / 2. - 25., 50., 50.);
                state.canvas.fill_path(&path, &paint);
                state.canvas.restore();

                state.canvas.flush();

                render_framebuffer_to_screen(&state.context, state.shader_program, state.texture_colorbuffer);

                state.surface.swap_buffers(&state.gl_context).unwrap();
            }
            _ => (),
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.as_ref() {
            state.window.request_redraw();
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut app = App { state: None };
    event_loop.run_app(&mut app).unwrap();
}

fn create_shader_program(context: &glow::Context) -> NativeProgram {
    unsafe {
        let v_shader = context.create_shader(glow::VERTEX_SHADER).unwrap();
        let vert_shader = include_str!("../assets/screen.vert.glsl");
        context.shader_source(v_shader, vert_shader);
        context.compile_shader(v_shader);
        if !context.get_shader_compile_status(v_shader) {
            let error_msg = context.get_shader_info_log(v_shader);
            panic!("ERROR::SHADER::VERTEX::COMPILATION_FAILED\n{error_msg:?}");
        }

        let f_shader = context.create_shader(glow::FRAGMENT_SHADER).unwrap();
        let frag_shader = include_str!("../assets/screen.frag.glsl");
        context.shader_source(f_shader, frag_shader);
        context.compile_shader(f_shader);
        if !context.get_shader_compile_status(f_shader) {
            let error_msg = context.get_shader_info_log(f_shader);
            panic!("ERROR::SHADER::FRAGMENT::COMPILATION_FAILED\n{error_msg:?}");
        }

        let shader_program = context.create_program().unwrap();
        context.attach_shader(shader_program, v_shader);
        context.attach_shader(shader_program, f_shader);
        context.link_program(shader_program);

        if !context.get_program_link_status(shader_program) {
            let error_msg = context.get_program_info_log(shader_program);
            panic!("ERROR::SHADER::PROGRAM::COMPILATION_FAILED\n{error_msg:?}");
        }

        context.use_program(Some(shader_program));
        let uni = context.get_uniform_location(shader_program, "screenTexture").unwrap();
        context.uniform_1_i32(Some(&uni), 0);

        shader_program
    }
}

fn create_framebuffer_colorbuffer(context: &Context) -> (NativeFramebuffer, NativeTexture) {
    unsafe {
        // Setup Framebuffer
        let framebuffer = context.create_framebuffer().unwrap();
        context.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));

        // generate texture
        let texture_colorbuffer = context.create_texture().unwrap();
        context.active_texture(glow::TEXTURE0);
        context.bind_texture(glow::TEXTURE_2D, Some(texture_colorbuffer));
        context.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGB as i32,
            WINDOW_WIDTH as i32,
            WINDOW_HEIGHT as i32,
            0,
            glow::RGB,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );
        context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
        context.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
        context.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture_colorbuffer),
            0,
        );
        context.bind_framebuffer(glow::FRAMEBUFFER, None);

        (framebuffer, texture_colorbuffer)
    }
}

fn prepare_framebuffer_for_render(context: &Context, framebuffer: NativeFramebuffer) {
    unsafe {
        context.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
        context.enable(glow::DEPTH_TEST);
        context.enable(glow::STENCIL_TEST);
        context.clear_color(0.0, 0.0, 0.0, 1.0);
        context.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);
    }
}

pub fn render_framebuffer_to_screen(
    context: &Context,
    shader_program: NativeProgram,
    texture_colorbuffer: NativeTexture,
) {
    unsafe {
        context.bind_framebuffer(glow::FRAMEBUFFER, None);
        context.enable(glow::STENCIL_TEST);
        context.disable(glow::DEPTH_TEST);
        context.clear_color(0.0, 0.0, 0.0, 1.0);
        context.clear(glow::COLOR_BUFFER_BIT);

        context.use_program(Some(shader_program));
        context.active_texture(glow::TEXTURE0);
        context.bind_texture(glow::TEXTURE_2D, Some(texture_colorbuffer));
        context.draw_arrays(glow::TRIANGLES, 0, 6);
    }
}
