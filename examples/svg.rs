use std::sync::Arc;

use femtovg::{Canvas, Color, FillRule, ImageFlags, Paint, Path};
use instant::Instant;
use resource::resource;
use usvg::TreeParsing;
use winit::{
    event::{ElementState, Event, MouseButton, WindowEvent},
    event_loop::EventLoop,
    window::Window,
};

mod helpers;
use helpers::{PerfGraph, WindowSurface};

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    helpers::start(1000, 600, "femtovg demo", true);
    #[cfg(target_arch = "wasm32")]
    helpers::start();
}

#[cfg(target_arch = "wasm32")]
use winit::window::Window;

fn run<W: WindowSurface>(mut canvas: Canvas<W::Renderer>, el: EventLoop<()>, mut surface: W, window: Arc<Window>) {
    canvas
        .add_font_mem(&resource!("examples/assets/Roboto-Light.ttf"))
        .expect("Cannot add font");

    canvas
        .add_font_mem(&resource!("examples/assets/Roboto-Regular.ttf"))
        .expect("Cannot add font");

    let mut screenshot_image_id = None;

    let start = Instant::now();
    let mut prevt = start;

    let mut mousex = 0.0;
    let mut mousey = 0.0;
    let mut dragging = false;

    let mut perf = PerfGraph::new();

    let svg_data = include_bytes!("assets/Ghostscript_Tiger.svg");
    let tree = usvg::Tree::from_data(svg_data, &usvg::Options::default()).unwrap();

    let paths = render_svg(tree);

    // print memory usage
    let mut total_sisze_bytes = 0;

    for path in &paths {
        total_sisze_bytes += path.0.size();
    }

    log::info!("Path mem usage: {}kb", total_sisze_bytes / 1024);

    el.run(move |event, event_loop_window_target| {
        event_loop_window_target.set_control_flow(winit::event_loop::ControlFlow::Poll);

        match event {
            Event::LoopExiting => event_loop_window_target.exit(),
            Event::WindowEvent { ref event, .. } => match event {
                #[cfg(not(target_arch = "wasm32"))]
                WindowEvent::Resized(physical_size) => {
                    surface.resize(physical_size.width, physical_size.height);
                }
                WindowEvent::MouseInput {
                    button: MouseButton::Left,
                    state,
                    ..
                } => match state {
                    ElementState::Pressed => dragging = true,
                    ElementState::Released => dragging = false,
                },
                WindowEvent::CursorMoved {
                    device_id: _, position, ..
                } => {
                    if dragging {
                        let p0 = canvas.transform().inverse().transform_point(mousex, mousey);
                        let p1 = canvas
                            .transform()
                            .inverse()
                            .transform_point(position.x as f32, position.y as f32);

                        canvas.translate(p1.0 - p0.0, p1.1 - p0.1);
                    }

                    mousex = position.x as f32;
                    mousey = position.y as f32;
                }
                WindowEvent::MouseWheel {
                    device_id: _,
                    delta: winit::event::MouseScrollDelta::LineDelta(_, y),
                    ..
                } => {
                    let pt = canvas.transform().inverse().transform_point(mousex, mousey);
                    canvas.translate(pt.0, pt.1);
                    canvas.scale(1.0 + (y / 10.0), 1.0 + (y / 10.0));
                    canvas.translate(-pt.0, -pt.1);
                }
                WindowEvent::KeyboardInput {
                    event:
                        winit::event::KeyEvent {
                            physical_key: winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::KeyS),
                            state: ElementState::Pressed,
                            ..
                        },
                    ..
                } => {
                    if let Some(screenshot_image_id) = screenshot_image_id {
                        canvas.delete_image(screenshot_image_id);
                    }

                    if let Ok(image) = canvas.screenshot() {
                        screenshot_image_id = Some(canvas.create_image(image.as_ref(), ImageFlags::empty()).unwrap());
                    }
                }
                WindowEvent::CloseRequested => event_loop_window_target.exit(),
                WindowEvent::RedrawRequested { .. } => {
                    let now = Instant::now();
                    let dt = (now - prevt).as_secs_f32();
                    prevt = now;

                    perf.update(dt);

                    let dpi_factor = window.scale_factor();
                    let size = window.inner_size();

                    canvas.set_size(size.width, size.height, dpi_factor as f32);
                    canvas.clear_rect(0, 0, size.width, size.height, Color::rgbf(0.3, 0.3, 0.32));

                    canvas.save();
                    canvas.translate(200.0, 200.0);

                    for (path, fill, stroke) in &paths {
                        if let Some(fill) = fill {
                            canvas.fill_path(path, fill);
                        }

                        if let Some(stroke) = stroke {
                            canvas.stroke_path(path, stroke);
                        }

                        if canvas.contains_point(path, mousex, mousey, FillRule::NonZero) {
                            let paint = Paint::color(Color::rgb(32, 240, 32)).with_line_width(1.0);
                            canvas.stroke_path(path, &paint);
                        }
                    }

                    canvas.restore();

                    canvas.save();
                    canvas.reset();
                    perf.render(&mut canvas, 5.0, 5.0);
                    canvas.restore();

                    surface.present(&mut canvas);
                }
                _ => (),
            },

            Event::AboutToWait => window.request_redraw(),
            _ => (),
        }
    })
    .unwrap();
}

fn render_svg(svg: usvg::Tree) -> Vec<(Path, Option<Paint>, Option<Paint>)> {
    use usvg::NodeKind;
    use usvg::PathSegment;

    let mut paths = Vec::new();

    for node in svg.root.descendants() {
        if let NodeKind::Path(svg_path) = &*node.borrow() {
            let mut path = Path::new();

            for command in svg_path.data.segments() {
                match command {
                    PathSegment::MoveTo { x, y } => path.move_to(x as f32, y as f32),
                    PathSegment::LineTo { x, y } => path.line_to(x as f32, y as f32),
                    PathSegment::CurveTo { x1, y1, x2, y2, x, y } => {
                        path.bezier_to(x1 as f32, y1 as f32, x2 as f32, y2 as f32, x as f32, y as f32)
                    }
                    PathSegment::ClosePath => path.close(),
                }
            }

            let to_femto_color = |usvg_paint: &usvg::Paint| match usvg_paint {
                usvg::Paint::Color(usvg::Color { red, green, blue }) => Some(Color::rgb(*red, *green, *blue)),
                _ => None,
            };

            let fill = svg_path
                .fill
                .as_ref()
                .and_then(|fill| to_femto_color(&fill.paint))
                .map(|col| Paint::color(col).with_anti_alias(true));

            let stroke = svg_path.stroke.as_ref().and_then(|stroke| {
                to_femto_color(&stroke.paint).map(|paint| {
                    Paint::color(paint)
                        .with_line_width(stroke.width.get() as f32)
                        .with_anti_alias(true)
                })
            });

            paths.push((path, fill, stroke))
        }
    }

    paths
}
