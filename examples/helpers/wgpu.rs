use std::sync::Arc;

use femtovg::{renderer::WGPURenderer, Canvas};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::Window;

use super::{run, Callbacks, WindowSurface};

pub struct DemoSurface {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    surface: wgpu::Surface<'static>,
}

impl WindowSurface for DemoSurface {
    type Renderer = femtovg::renderer::WGPURenderer;

    fn resize(&mut self, width: u32, height: u32) {
        self.surface_config.width = width.max(1);
        self.surface_config.height = height.max(1);
        self.surface.configure(&self.device, &self.surface_config);
    }

    fn present(&self, canvas: &mut femtovg::Canvas<Self::Renderer>) {
        let frame = self
            .surface
            .get_current_texture()
            .expect("unable to get next texture from swapchain");

        let commands = canvas.flush_to_surface(&frame.texture);

        self.queue.submit(Some(commands));

        frame.present();
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct WgpuApp {
    width: u32,
    height: u32,
    title: &'static str,
    resizeable: bool,
    callbacks: Option<Callbacks>,
    window: Option<Arc<Window>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ApplicationHandler for WgpuApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.callbacks.is_some() {
            return;
        }

        let window_attrs = Window::default_attributes()
            .with_inner_size(winit::dpi::PhysicalSize::new(self.width, self.height))
            .with_resizable(self.resizeable)
            .with_title(self.title);

        let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
        self.window = Some(window.clone());

        let (width, height): (u32, u32) = window.inner_size().into();

        let backends = wgpu::Backends::from_env().unwrap_or_default();
        let dx12_shader_compiler = wgpu::Dx12Compiler::from_env().unwrap_or_default();
        let dx12_presentation_system = wgpu::wgt::Dx12SwapchainKind::from_env().unwrap_or_default();
        let dx12_latency_waitable_object = wgpu::wgt::Dx12UseFrameLatencyWaitableObject::from_env().unwrap_or_default();
        let gles_minor_version = wgpu::Gles3MinorVersion::from_env().unwrap_or_default();

        let instance = pollster::block_on(wgpu::util::new_instance_with_webgpu_detection(
            &wgpu::InstanceDescriptor {
                backends,
                flags: wgpu::InstanceFlags::from_build_config().with_env(),
                backend_options: wgpu::BackendOptions {
                    dx12: wgpu::Dx12BackendOptions {
                        shader_compiler: dx12_shader_compiler,
                        presentation_system: dx12_presentation_system,
                        latency_waitable_object: dx12_latency_waitable_object,
                    },
                    gl: wgpu::GlBackendOptions {
                        gles_minor_version,
                        fence_behavior: wgpu::GlFenceBehavior::default(),
                    },
                    noop: wgpu::NoopBackendOptions::default(),
                },
                memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            },
        ));

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = pollster::block_on(wgpu::util::initialize_adapter_from_env_or_default(
            &instance,
            Some(&surface),
        ))
        .expect("Failed to find an appropriate adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::default(),
        }))
        .expect("Failed to create device");

        let mut surface_config = surface.get_default_config(&adapter, width, height).unwrap();

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let swapchain_format = swapchain_capabilities
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or_else(|| swapchain_capabilities.formats[0]);
        surface_config.format = swapchain_format;
        surface.configure(&device, &surface_config);

        let demo_surface = DemoSurface {
            device: device.clone(),
            queue: queue.clone(),
            surface_config,
            surface,
        };

        let renderer = WGPURenderer::new(device, queue);

        let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
        canvas.set_size(width, height, window.scale_factor() as f32);

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
pub fn start_wgpu(width: u32, height: u32, title: &'static str, resizeable: bool) {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut app = WgpuApp {
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
struct WgpuWasmApp {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    callbacks: Option<Callbacks>,
    window: Option<Arc<Window>>,
}

#[cfg(target_arch = "wasm32")]
impl ApplicationHandler for WgpuWasmApp {
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

        let window_attrs = Window::default_attributes().with_canvas(Some(html_canvas));
        let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
        let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(width, height));
        self.window = Some(window.clone());

        let surface = self.instance.create_surface(window.clone()).unwrap();

        let mut surface_config = surface.get_default_config(&self.adapter, width, height).unwrap();
        let swapchain_capabilities = surface.get_capabilities(&self.adapter);
        let swapchain_format = swapchain_capabilities
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or_else(|| swapchain_capabilities.formats[0]);
        surface_config.format = swapchain_format;
        surface.configure(&self.device, &surface_config);

        let demo_surface = DemoSurface {
            device: self.device.clone(),
            queue: self.queue.clone(),
            surface_config,
            surface,
        };

        let renderer = WGPURenderer::new(self.device.clone(), self.queue.clone());

        let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
        canvas.set_size(width, height, window.scale_factor() as f32);

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
pub async fn start_wgpu_wasm() {
    #[cfg(debug_assertions)]
    console_error_panic_hook::set_once();

    let instance = wgpu::util::new_instance_with_webgpu_detection(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or_default(),
        flags: wgpu::InstanceFlags::from_build_config().with_env(),
        backend_options: wgpu::BackendOptions::default(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
    })
    .await;

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: None,
        })
        .await
        .expect("Failed to find an appropriate adapter");

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::default(),
        })
        .await
        .expect("Failed to create device");

    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    use winit::platform::web::EventLoopExtWebSys;
    event_loop.spawn_app(WgpuWasmApp {
        instance,
        adapter,
        device,
        queue,
        callbacks: None,
        window: None,
    });
}
