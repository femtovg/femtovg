use femtovg::renderer::OpenGl;
use femtovg::{Canvas, Color, Renderer};
use glutin::dpi::{PhysicalPosition, PhysicalSize};
use glutin::event::{Event, WindowEvent};
use glutin::event_loop::{ControlFlow, EventLoop};
use glutin::window::{Window, WindowBuilder};
use glutin::{ContextBuilder, ContextWrapper, PossiblyCurrent};

fn main() {
    let event_loop = EventLoop::new();
    let context = create_window(&event_loop);

    let renderer = OpenGl::new_from_glutin_context(&context).expect("Cannot create renderer");
    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");

    let mut mouse_position = PhysicalPosition::new(0., 0.);

    event_loop.run(move |event, _target, control_flow| match event {
        Event::WindowEvent { event, .. } => match event {
            WindowEvent::CursorMoved { position, .. } => {
                mouse_position = position;
                context.window().request_redraw();
            }
            WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
            _ => {}
        },
        Event::RedrawRequested(_) => {
            render(&context, &mut canvas, mouse_position);
        }
        _ => {}
    })
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

fn render<T: Renderer>(
    context: &ContextWrapper<PossiblyCurrent, Window>,
    canvas: &mut Canvas<T>,
    square_position: PhysicalPosition<f64>,
) {
    // Make sure the canvas has the right size:
    let window = context.window();
    let size = window.inner_size();
    canvas.set_size(size.width, size.height, window.scale_factor() as f32);

    canvas.clear_rect(0, 0, size.width, size.height, Color::black());

    // Make smol red rectangle
    canvas.clear_rect(
        square_position.x as u32,
        square_position.y as u32,
        30,
        30,
        Color::rgbf(1., 0., 0.),
    );

    // Tell renderer to execute all drawing commands
    canvas.flush();
    // Display what we've just rendered
    context.swap_buffers().expect("Could not swap buffers");
}
