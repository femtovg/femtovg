use std::sync::Arc;

use femtovg::{renderer::WGPURenderer, Canvas};
use winit::{event_loop::EventLoop, window::WindowBuilder};

use super::{run, WindowSurface};

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

pub async fn start_wgpu(
    #[cfg(not(target_arch = "wasm32"))] width: u32,
    #[cfg(not(target_arch = "wasm32"))] height: u32,
    #[cfg(not(target_arch = "wasm32"))] title: &'static str,
    #[cfg(not(target_arch = "wasm32"))] resizeable: bool,
) {
    // This provides better error messages in debug mode.
    // It's disabled in release mode so it doesn't bloat up the file size.
    #[cfg(all(debug_assertions, target_arch = "wasm32"))]
    console_error_panic_hook::set_once();

    let event_loop = EventLoop::new().unwrap();

    #[cfg(not(target_arch = "wasm32"))]
    let window = {
        let window_builder = WindowBuilder::new()
            .with_inner_size(winit::dpi::PhysicalSize::new(width, height))
            .with_resizable(resizeable)
            .with_title(title);
        window_builder.build(&event_loop).unwrap()
    };

    #[cfg(target_arch = "wasm32")]
    let (window, width, height) = {
        use wasm_bindgen::JsCast;

        let canvas = web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .get_element_by_id("canvas")
            .unwrap()
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .unwrap();

        use winit::platform::web::WindowBuilderExtWebSys;

        let width = canvas.width();
        let height = canvas.height();

        let window = WindowBuilder::new()
            .with_canvas(Some(canvas.clone()))
            .build(&event_loop)
            .unwrap();

        let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(width, height));

        (window, width, height)
    };

    let window = Arc::new(window);

    let backends = wgpu::Backends::from_env().unwrap_or_default();
    let dx12_shader_compiler = wgpu::Dx12Compiler::from_env().unwrap_or_default();
    let dx12_presentation_system = wgpu::wgt::Dx12SwapchainKind::from_env().unwrap_or_default();
    let dx12_latency_waitable_object = wgpu::wgt::Dx12UseFrameLatencyWaitableObject::from_env().unwrap_or_default();
    let gles_minor_version = wgpu::Gles3MinorVersion::from_env().unwrap_or_default();

    let instance = wgpu::util::new_instance_with_webgpu_detection(&wgpu::InstanceDescriptor {
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
    })
    .await;

    let surface = instance.create_surface(window.clone()).unwrap();

    let adapter = wgpu::util::initialize_adapter_from_env_or_default(&instance, Some(&surface))
        .await
        .expect("Failed to find an appropriate adapter");

    // Create the logical device and command queue
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            // Make sure we use the texture resolution limits from the adapter, so we can support images the size of the swapchain.
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::default(),
        })
        .await
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

    run(canvas, event_loop, demo_surface, window);
}
