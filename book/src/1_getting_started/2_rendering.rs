use femtovg::renderer::OpenGl;
use femtovg::{Canvas, Color, Renderer};
use glutin::dpi::PhysicalSize;
use glutin::event_loop::EventLoop;
use glutin::window::{Window, WindowBuilder};
use glutin::{ContextBuilder, ContextWrapper, PossiblyCurrent};

fn main() {
    let event_loop = EventLoop::new();
    let context = create_window(&event_loop);

    let renderer = OpenGl::new_from_glutin_context(&context).expect("Cannot create renderer");
    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");

    render(&context, &mut canvas);

    loop {}
}

fn create_window(event_loop: &EventLoop<()>) -> ContextWrapper<PossiblyCurrent, Window> {
    let window_builder = WindowBuilder::new()
        .with_inner_size(PhysicalSize::new(1000., 600.))
        .with_title("Femtovg");
    let context = ContextBuilder::new()
        .with_vsync(false)
        .build_windowed(window_builder, &event_loop)
        .unwrap();
    let current_context = unsafe { context.make_current().expect("Could not make the context current") };

    current_context
}

fn render<T: Renderer>(context: &ContextWrapper<PossiblyCurrent, Window>, canvas: &mut Canvas<T>) {
    // Make sure the canvas has the right size:
    let window = context.window();
    let size = window.inner_size();
    canvas.set_size(size.width, size.height, window.scale_factor() as f32);

    // Make smol red rectangle
    canvas.clear_rect(30, 30, 30, 30, Color::rgbf(1., 0., 0.));

    // Tell renderer to execute all drawing commands
    canvas.flush();
    // Display what we've just rendered
    context.swap_buffers().expect("Could not swap buffers");
}
