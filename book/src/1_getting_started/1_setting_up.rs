use glutin::dpi::PhysicalSize;
use glutin::event_loop::EventLoop;
use glutin::window::{Window, WindowBuilder};
use glutin::{ContextBuilder, ContextWrapper, PossiblyCurrent};

fn main() {
    let event_loop = EventLoop::new();
    let _context = create_window(&event_loop);
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
