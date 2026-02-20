#[cfg(not(target_arch = "wasm32"))]
use std::num::NonZeroU32;
use std::sync::Arc;

use super::{run, Callbacks, WindowSurface};

use femtovg::{renderer::OpenGl, Canvas};
#[cfg(not(target_arch = "wasm32"))]
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder},
    display::GetGlDisplay,
    prelude::*,
    surface::SurfaceAttributesBuilder,
};
#[cfg(not(target_arch = "wasm32"))]
use glutin_winit::DisplayBuilder;
#[cfg(not(target_arch = "wasm32"))]
use raw_window_handle::HasWindowHandle;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::Window;

pub struct DemoSurface {
    #[cfg(not(target_arch = "wasm32"))]
    context: glutin::context::PossiblyCurrentContext,
    #[cfg(not(target_arch = "wasm32"))]
    surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
}

impl WindowSurface for DemoSurface {
    type Renderer = OpenGl;

    fn resize(&mut self, width: u32, height: u32) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.surface
                .resize(&self.context, width.try_into().unwrap(), height.try_into().unwrap());
        }
    }
    fn present(&self, canvas: &mut femtovg::Canvas<Self::Renderer>) {
        canvas.flush_to_surface(&());
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.surface.swap_buffers(&self.context).unwrap();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct GlApp {
    width: u32,
    height: u32,
    title: &'static str,
    resizeable: bool,
    callbacks: Option<Callbacks>,
    window: Option<Arc<Window>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ApplicationHandler for GlApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.callbacks.is_some() {
            return;
        }

        let window_attrs = Window::default_attributes()
            .with_inner_size(winit::dpi::PhysicalSize::new(self.width, self.height))
            .with_resizable(self.resizeable)
            .with_title(self.title);

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
        let attrs = SurfaceAttributesBuilder::<glutin::surface::WindowSurface>::new().build(
            raw_window_handle,
            NonZeroU32::new(width).unwrap(),
            NonZeroU32::new(height).unwrap(),
        );

        let surface = unsafe { gl_config.display().create_window_surface(&gl_config, &attrs).unwrap() };

        let gl_context = not_current_gl_context.take().unwrap().make_current(&surface).unwrap();

        let renderer = unsafe { OpenGl::new_from_function_cstr(|s| gl_display.get_proc_address(s).cast()) }
            .expect("Cannot create renderer");

        let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
        canvas.set_size(width, height, window.scale_factor() as f32);

        let window = Arc::new(window);
        self.window = Some(window.clone());

        let demo_surface = DemoSurface {
            context: gl_context,
            surface,
        };

        self.callbacks = Some(run(canvas, demo_surface, window));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: winit::window::WindowId, event: WindowEvent) {
        if let Some(ref mut callbacks) = self.callbacks {
            (callbacks.window_event)(event, event_loop);
        }
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, device_id: DeviceId, event: DeviceEvent) {
        if let Some(ref mut callbacks) = self.callbacks {
            if let Some(ref mut device_cb) = callbacks.device_event {
                device_cb(device_id, event, event_loop);
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn start_opengl(width: u32, height: u32, title: &'static str, resizeable: bool) {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut app = GlApp {
        width,
        height,
        title,
        resizeable,
        callbacks: None,
        window: None,
    };

    event_loop.run_app(&mut app).unwrap();
}

#[cfg(target_arch = "wasm32")]
struct GlWasmApp {
    callbacks: Option<Callbacks>,
    window: Option<Arc<Window>>,
}

#[cfg(target_arch = "wasm32")]
impl ApplicationHandler for GlWasmApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.callbacks.is_some() {
            return;
        }

        use wasm_bindgen::JsCast;
        use winit::platform::web::WindowAttributesExtWebSys;

        let html_canvas = web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .get_element_by_id("canvas")
            .unwrap()
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .unwrap();

        let width = html_canvas.width();
        let height = html_canvas.height();

        let renderer = OpenGl::new_from_html_canvas(&html_canvas).expect("Cannot create renderer");

        let window_attrs = Window::default_attributes().with_canvas(Some(html_canvas));
        let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
        let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(width, height));
        self.window = Some(window.clone());

        let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
        canvas.set_size(width, height, window.scale_factor() as f32);

        let demo_surface = DemoSurface {};

        self.callbacks = Some(run(canvas, demo_surface, window));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: winit::window::WindowId, event: WindowEvent) {
        if let Some(ref mut callbacks) = self.callbacks {
            (callbacks.window_event)(event, event_loop);
        }
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, device_id: DeviceId, event: DeviceEvent) {
        if let Some(ref mut callbacks) = self.callbacks {
            if let Some(ref mut device_cb) = callbacks.device_event {
                device_cb(device_id, event, event_loop);
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub async fn start_opengl_wasm() {
    #[cfg(debug_assertions)]
    console_error_panic_hook::set_once();

    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    use winit::platform::web::EventLoopExtWebSys;
    event_loop.spawn_app(GlWasmApp {
        callbacks: None,
        window: None,
    });
}
