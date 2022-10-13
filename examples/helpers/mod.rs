use super::run;

use femtovg::{renderer::OpenGl, Canvas};
use winit::{event_loop::EventLoop, window::WindowBuilder};

mod perf_graph;
pub use perf_graph::PerfGraph;

pub fn start(
    #[cfg(not(target_arch = "wasm32"))] width: u32,
    #[cfg(not(target_arch = "wasm32"))] height: u32,
    #[cfg(not(target_arch = "wasm32"))] title: &'static str,
    #[cfg(not(target_arch = "wasm32"))] resizeable: bool,
) {
    // This provides better error messages in debug mode.
    // It's disabled in release mode so it doesn't bloat up the file size.
    #[cfg(all(debug_assertions, target_arch = "wasm32"))]
    console_error_panic_hook::set_once();

    let event_loop = EventLoop::new();

    #[cfg(not(target_arch = "wasm32"))]
    let (canvas, windowed_context) = {
        use glutin::ContextBuilder;

        let window_builder = WindowBuilder::new()
            .with_inner_size(winit::dpi::PhysicalSize::new(width, height))
            .with_resizable(resizeable)
            .with_title(title);

        let windowed_context = ContextBuilder::new()
            .with_vsync(false)
            .build_windowed(window_builder, &event_loop)
            .unwrap();
        let windowed_context = unsafe { windowed_context.make_current().unwrap() };

        let renderer = OpenGl::new_from_glutin_context(&windowed_context).expect("Cannot create renderer");

        let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
        canvas.set_size(width, height, windowed_context.window().scale_factor() as f32);

        (canvas, windowed_context)
    };

    #[cfg(target_arch = "wasm32")]
    let (canvas, window) = {
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

        let renderer = OpenGl::new_from_html_canvas(&canvas).expect("Cannot create renderer");

        let window = WindowBuilder::new()
            .with_canvas(Some(canvas))
            .build(&event_loop)
            .unwrap();

        let canvas = Canvas::new(renderer).expect("Cannot create canvas");

        (canvas, window)
    };

    run(
        canvas,
        event_loop,
        #[cfg(not(target_arch = "wasm32"))]
        windowed_context,
        #[cfg(target_arch = "wasm32")]
        window,
    );
}
