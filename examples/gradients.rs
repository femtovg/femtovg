use femtovg::{renderer::OpenGl, Canvas, Color, Paint, Path, Renderer};
use instant::Instant;
use resource::resource;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

mod helpers;
use helpers::PerfGraph;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    helpers::start(1000, 670, "Gradient test", false);
    #[cfg(target_arch = "wasm32")]
    helpers::start();
}

#[cfg(not(target_arch = "wasm32"))]
use glutin::prelude::*;

#[cfg(target_arch = "wasm32")]
use winit::window::Window;

fn run(
    mut canvas: Canvas<OpenGl>,
    el: EventLoop<()>,
    #[cfg(not(target_arch = "wasm32"))] context: glutin::context::PossiblyCurrentContext,
    #[cfg(not(target_arch = "wasm32"))] surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
    window: Window,
) {
    canvas
        .add_font_mem(&resource!("examples/assets/Roboto-Regular.ttf"))
        .expect("Cannot add font");

    let start = Instant::now();
    let mut prevt = start;

    let mut perf = PerfGraph::new();

    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::LoopDestroyed => *control_flow = ControlFlow::Exit,
            Event::WindowEvent { ref event, .. } => match event {
                #[cfg(not(target_arch = "wasm32"))]
                WindowEvent::Resized(physical_size) => {
                    surface.resize(
                        &context,
                        physical_size.width.try_into().unwrap(),
                        physical_size.height.try_into().unwrap(),
                    );
                }
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                _ => (),
            },
            Event::RedrawRequested(_) => {
                let dpi_factor = window.scale_factor();
                let size = window.inner_size();
                canvas.set_size(size.width, size.height, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width, size.height, Color::rgbf(0.9, 0.9, 0.9));

                let now = Instant::now();
                let dt = (now - prevt).as_secs_f32();
                prevt = now;

                perf.update(dt);

                draw_gradients(&mut canvas);

                canvas.save();
                canvas.reset();
                perf.render(&mut canvas, 5.0, 5.0);
                canvas.restore();

                canvas.flush();
                #[cfg(not(target_arch = "wasm32"))]
                surface.swap_buffers(&context).unwrap();
            }
            Event::MainEventsCleared => window.request_redraw(),
            _ => (),
        }
    });
}

fn draw_gradients<T: Renderer>(canvas: &mut Canvas<T>) {
    let mut r = |x, y, name, paint| {
        let mut p = Path::new();
        p.rect(0.0, 0.0, 100.0, 100.0);
        canvas.translate(x, y);
        canvas.fill_path(&p, &paint);
        canvas.translate(-x, -y);
        let mut text_paint = Paint::color(Color::black());
        text_paint.set_font_size(14.0);
        let _ = canvas.fill_text(x, y + 114.0, name, &text_paint);
    };
    // Various two stop gradients
    let mut x = 10.0;
    let mut y = 10.0;

    // OPAQUE LINEAR
    r(
        x,
        y,
        "x linear opaque",
        Paint::linear_gradient(
            0.0,
            0.0,
            100.0,
            0.0,
            Color::rgba(255, 0, 0, 255),
            Color::rgba(0, 0, 255, 255),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "y linear opaque",
        Paint::linear_gradient(
            0.0,
            0.0,
            0.0,
            100.0,
            Color::rgba(255, 0, 0, 255),
            Color::rgba(0, 0, 255, 255),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "xy linear opaque",
        Paint::linear_gradient(
            0.0,
            0.0,
            100.0,
            100.0,
            Color::rgba(255, 0, 0, 255),
            Color::rgba(0, 0, 255, 255),
        ),
    );

    // 50% LINEAR
    x += 110.0;
    r(
        x,
        y,
        "x linear 50%",
        Paint::linear_gradient(
            0.0,
            0.0,
            100.0,
            0.0,
            Color::rgba(255, 0, 0, 128),
            Color::rgba(0, 0, 255, 128),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "y linear 50%",
        Paint::linear_gradient(
            0.0,
            0.0,
            0.0,
            100.0,
            Color::rgba(255, 0, 0, 128),
            Color::rgba(0, 0, 255, 128),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "xy linear 50%",
        Paint::linear_gradient(
            0.0,
            0.0,
            100.0,
            100.0,
            Color::rgba(255, 0, 0, 128),
            Color::rgba(0, 0, 255, 128),
        ),
    );

    // TRANSPARENT TO OPAQUE LINEAR
    x += 110.0;
    r(
        x,
        y,
        "x linear 0-100",
        Paint::linear_gradient(
            0.0,
            0.0,
            100.0,
            0.0,
            Color::rgba(255, 0, 0, 0),
            Color::rgba(0, 0, 255, 255),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "y linear 0-100",
        Paint::linear_gradient(
            0.0,
            0.0,
            0.0,
            100.0,
            Color::rgba(255, 0, 0, 0),
            Color::rgba(0, 0, 255, 255),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "xy linear 0-100",
        Paint::linear_gradient(
            0.0,
            0.0,
            100.0,
            100.0,
            Color::rgba(255, 0, 0, 0),
            Color::rgba(0, 0, 255, 255),
        ),
    );

    y += 130.0;
    x = 10.0;
    // OPAQUE RADIAL
    r(
        x,
        y,
        "radial opaque",
        Paint::radial_gradient(
            50.0,
            50.0,
            0.0,
            50.0,
            Color::rgba(255, 0, 0, 255),
            Color::rgba(0, 0, 255, 255),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "0,0 rad opaque",
        Paint::radial_gradient(
            0.0,
            0.0,
            0.0,
            100.0,
            Color::rgba(255, 0, 0, 255),
            Color::rgba(0, 0, 255, 255),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "fill rad opaque",
        Paint::radial_gradient(
            50.0,
            50.0,
            25.0,
            75.0,
            Color::rgba(255, 0, 0, 255),
            Color::rgba(0, 0, 255, 255),
        ),
    );

    // 50% LINEAR
    x += 110.0;
    r(
        x,
        y,
        "radial 50%",
        Paint::radial_gradient(
            50.0,
            50.0,
            0.0,
            50.0,
            Color::rgba(255, 0, 0, 128),
            Color::rgba(0, 0, 255, 128),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "0,0 rad 50%",
        Paint::radial_gradient(
            0.0,
            0.0,
            0.0,
            100.0,
            Color::rgba(255, 0, 0, 128),
            Color::rgba(0, 0, 255, 128),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "fill rad 50%",
        Paint::radial_gradient(
            50.0,
            50.0,
            25.0,
            75.0,
            Color::rgba(255, 0, 0, 128),
            Color::rgba(0, 0, 255, 128),
        ),
    );

    // TRANSPARENT TO OPAQUE LINEAR
    x += 110.0;
    r(
        x,
        y,
        "radial 0-100",
        Paint::radial_gradient(
            50.0,
            50.0,
            0.0,
            50.0,
            Color::rgba(255, 0, 0, 0),
            Color::rgba(0, 0, 255, 128),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "0,0 rad 0-100",
        Paint::radial_gradient(
            0.0,
            0.0,
            0.0,
            100.0,
            Color::rgba(255, 0, 0, 0),
            Color::rgba(0, 0, 255, 128),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "fill rad 0-100",
        Paint::radial_gradient(
            50.0,
            50.0,
            25.0,
            75.0,
            Color::rgba(255, 0, 0, 0),
            Color::rgba(0, 0, 255, 128),
        ),
    );

    // Multistop!
    y += 130.0;
    x = 10.0;
    r(
        x,
        y,
        "ms x linear op",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            [
                (0.0, Color::rgba(255, 0, 0, 255)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms y linear op",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            0.0,
            100.0,
            [
                (0.0, Color::rgba(255, 0, 0, 255)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms xy linear op",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            100.0,
            [
                (0.0, Color::rgba(255, 0, 0, 255)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
    // Multistop linear 50%
    x += 110.0;
    r(
        x,
        y,
        "ms x linear 50%",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            [
                (0.0, Color::rgba(255, 0, 0, 128)),
                (0.5, Color::rgba(0, 255, 0, 128)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms y linear 50%",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            0.0,
            100.0,
            [
                (0.0, Color::rgba(255, 0, 0, 128)),
                (0.5, Color::rgba(0, 255, 0, 128)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms xy linear 50%",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            100.0,
            [
                (0.0, Color::rgba(255, 0, 0, 128)),
                (0.5, Color::rgba(0, 255, 0, 128)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    // Multistop linear transparent & opaque
    x += 110.0;
    r(
        x,
        y,
        "ms x linear 0-100",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            [
                (0.0, Color::rgba(255, 0, 0, 0)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms y linear 0-100",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            0.0,
            100.0,
            [
                (0.0, Color::rgba(255, 0, 0, 0)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms xy linear 0-100%",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            100.0,
            [
                (0.0, Color::rgba(255, 0, 0, 0)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    // Multistop radial
    y += 130.0;
    x = 10.0;
    r(
        x,
        y,
        "ms radial opq",
        Paint::radial_gradient_stops(
            50.0,
            50.0,
            0.0,
            50.0,
            [
                (0.0, Color::rgba(255, 0, 0, 255)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms 0,0 rad opq",
        Paint::radial_gradient_stops(
            0.0,
            0.0,
            0.0,
            100.0,
            [
                (0.0, Color::rgba(255, 0, 0, 255)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms rad opq",
        Paint::radial_gradient_stops(
            50.0,
            50.0,
            25.0,
            75.0,
            [
                (0.0, Color::rgba(255, 0, 0, 255)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
    // Multistop radial 50%
    x += 110.0;
    r(
        x,
        y,
        "ms radial 50%",
        Paint::radial_gradient_stops(
            50.0,
            50.0,
            0.0,
            50.0,
            [
                (0.0, Color::rgba(255, 0, 0, 128)),
                (0.5, Color::rgba(0, 255, 0, 128)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms 0,0 rad 50%",
        Paint::radial_gradient_stops(
            0.0,
            0.0,
            0.0,
            100.0,
            [
                (0.0, Color::rgba(255, 0, 0, 128)),
                (0.5, Color::rgba(0, 255, 0, 128)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms rad 50%",
        Paint::radial_gradient_stops(
            50.0,
            50.0,
            25.0,
            75.0,
            [
                (0.0, Color::rgba(255, 0, 0, 128)),
                (0.5, Color::rgba(0, 255, 0, 128)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    // Multistop radial transparent
    x += 110.0;
    r(
        x,
        y,
        "ms radial 0-100",
        Paint::radial_gradient_stops(
            50.0,
            50.0,
            0.0,
            50.0,
            [
                (0.0, Color::rgba(255, 0, 0, 0)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms 0,0 rad 0-100",
        Paint::radial_gradient_stops(
            0.0,
            0.0,
            0.0,
            100.0,
            [
                (0.0, Color::rgba(255, 0, 0, 0)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms rad 0-100",
        Paint::radial_gradient_stops(
            50.0,
            50.0,
            25.0,
            75.0,
            [
                (0.0, Color::rgba(255, 0, 0, 0)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );

    // Multistop padding cases
    x = 10.0;
    y += 130.0;
    r(
        x,
        y,
        "ms pad start",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            [(0.5, Color::rgba(255, 0, 0, 255)), (1.0, Color::rgba(0, 0, 255, 255))],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms pad both",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            [(0.4, Color::rgba(255, 0, 0, 255)), (0.6, Color::rgba(0, 0, 255, 255))],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms pad end",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            [(0.0, Color::rgba(255, 0, 0, 255)), (0.5, Color::rgba(0, 0, 255, 255))],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms same stop",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            [(0.5, Color::rgba(255, 0, 0, 255)), (0.5, Color::rgba(0, 0, 255, 255))],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms one stop",
        Paint::linear_gradient_stops(0.0, 0.0, 100.0, 0.0, [(0.5, Color::rgba(255, 0, 0, 255))]),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms zero stops",
        Paint::linear_gradient_stops(0.0, 0.0, 100.0, 0.0, []),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms non-seq 1",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            [(0.5, Color::rgba(255, 0, 0, 255)), (0.0, Color::rgba(0, 0, 255, 255))],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms non-seq 2",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            [(0.5, Color::rgba(255, 0, 0, 255)), (0.3, Color::rgba(0, 0, 255, 255))],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms non-seq 3",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            [
                (0.5, Color::rgba(255, 0, 0, 255)),
                (0.6, Color::rgba(0, 255, 0, 255)),
                (0.3, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
}
