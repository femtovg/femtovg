
use std::time::Instant;

use glutin::event::{Event, WindowEvent, DeviceEvent, ElementState, KeyboardInput, VirtualKeyCode, MouseButton};
use glutin::event_loop::{ControlFlow, EventLoop};
use glutin::window::WindowBuilder;
use glutin::ContextBuilder;

use gpucanvas::{
    //Renderer,
    //Canvas,
    Color,
    Paint,
    LineCap,
    LineJoin,
    FillRule,
    Winding,
    ImageFlags,
    Align,
    //CompositeOperation,
    renderer::OpenGl
};

type Canvas = gpucanvas::Canvas<OpenGl>;
type Point = euclid::default::Point2D<f32>;
type Vector = euclid::default::Vector2D<f32>;
type Size = euclid::default::Size2D<f32>;
type Rect = euclid::default::Rect<f32>;

struct Brick {
    id: u8,
    destroyed: bool,
    rect: Rect,
}

impl Brick {
    fn new(id: u8, rect: Rect) -> Self {
        Self {
            id: id,
            destroyed: false,
            rect: rect
        }
    }

    fn draw(&self, canvas: &mut Canvas) {
        if self.destroyed { return }

        canvas.begin_path();
        canvas.rect(self.rect.origin.x, self.rect.origin.y, self.rect.size.width, self.rect.size.height);

        let paint = Paint::color(match self.id {
            0 => return,
            1 => Color::rgb(220, 100, 0),
            2 => Color::rgb(255, 255, 0),
            3 => Color::rgb(0, 255, 0),
            _ => return
        });

        canvas.fill_path(paint);
        canvas.stroke_path(Paint::color(Color::black()));
    }
}

fn main() {
    let level = vec![
        vec![1, 1, 1, 1, 1, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1],
    ];

    let window_size = glutin::dpi::PhysicalSize::new(800, 600);
    let el = EventLoop::new();
    let wb = WindowBuilder::new()
        .with_inner_size(window_size)
        .with_resizable(false)
        .with_title("gpucanvas Bricks demo");

    let windowed_context = ContextBuilder::new().build_windowed(wb, &el).unwrap();
    //let windowed_context = ContextBuilder::new().with_gl(GlRequest::Specific(Api::OpenGl, (4, 4))).with_vsync(false).build_windowed(wb, &el).unwrap();
    let windowed_context = unsafe { windowed_context.make_current().unwrap() };

    let renderer = OpenGl::new(|s| windowed_context.get_proc_address(s) as *const _).expect("Cannot create renderer");
    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");

    canvas.add_font("examples/assets/Roboto-Bold.ttf");
    canvas.add_font("examples/assets/Roboto-Light.ttf");
    canvas.add_font("examples/assets/Roboto-Regular.ttf");

    let mut paused = true;
    let mut is_shooting = true;

    // Bricks
    let brick_size = Size::new(window_size.width as f32 / level[0].len() as f32, 30.0);
    let mut brick_loc = Point::new(0.0, 0.0);
    let mut bricks = Vec::new();

    for row in level {
        for id in row {
            let rect = Rect::new(brick_loc, brick_size);
            bricks.push(Brick::new(id, rect));
            brick_loc.x += brick_size.width;
        }

        brick_loc.x = 0.0;
        brick_loc.y += brick_size.height;
    }

    // Paddle
    let paddle_size = Size::new(100.0, 20.0);
    let paddle_pos = Point::new(
        (window_size.width as f32 - paddle_size.width) / 2.0,
        window_size.height as f32 - paddle_size.height - 10.0
    );

    let mut paddle_rect = Rect::new(paddle_pos, paddle_size);

    // Ball
    let ball_r = 10.0;
    let mut ball_pos = (paddle_rect.center() - Point::new(0.0, paddle_rect.height() / 2.0 + ball_r)).to_point();
    let mut ball_velocity = Vector::new(0.0, 0.0);

    let start = Instant::now();
    let mut prevt = start;

    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::LoopDestroyed => return,
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::Resized(physical_size) => {
                    windowed_context.resize(*physical_size);
                }
                WindowEvent::KeyboardInput { input: KeyboardInput { virtual_keycode: Some(VirtualKeyCode::Escape), state: ElementState::Pressed, .. }, .. } => {
                    if paused {
                        *control_flow = ControlFlow::Exit;
                    } else {
                        paused = true;
                        let _ = windowed_context.window().set_cursor_grab(false);
                        windowed_context.window().set_cursor_visible(true);
                    }
                }
                WindowEvent::MouseInput { button: MouseButton::Left, state: ElementState::Pressed, ..} => {
                    if paused {
                        paused = false;
                        let _ = windowed_context.window().set_cursor_grab(true);
                        windowed_context.window().set_cursor_visible(false);
                    } else if is_shooting {
                        ball_velocity = Vector::new(100.0, -350.0);
                        is_shooting = false;
                    }
                }
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit
                }
                _ => (),
            }
            Event::DeviceEvent { ref event, .. } => match event {
                DeviceEvent::MouseMotion { delta } => {
                    if !paused {
                        paddle_rect.origin.x += delta.0 as f32;
                        paddle_rect.origin = paddle_rect.origin.clamp(
                            Point::new(0.0, paddle_rect.origin.y),
                            Point::new(canvas.width() - paddle_rect.size.width, paddle_rect.origin.y)
                        );
                    }
                }
                _ => ()
            }
            Event::RedrawRequested(_) => {
                let dpi_factor = windowed_context.window().scale_factor();
                let size = windowed_context.window().inner_size();
                canvas.set_size(size.width as u32, size.height as u32, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width as u32, size.height as u32, Color::rgbf(0.3, 0.3, 0.32));

                let now = Instant::now();
                let dt = (now - prevt).as_secs_f32();
                prevt = now;

                if !paused {
                    if is_shooting {
                        ball_pos = (paddle_rect.center() - Point::new(0.0, paddle_rect.height() / 2.0 + ball_r)).to_point();
                    } else {
                        ball_pos += ball_velocity * dt;

                        if ball_pos.x <= ball_r {
                            ball_velocity.x = -ball_velocity.x;
                            ball_pos.x = ball_r;
                        } else if ball_pos.x + ball_r >= canvas.width() {
                            ball_velocity.x = -ball_velocity.x;
                            ball_pos.x = canvas.width() - ball_r;
                        }

                        if ball_pos.y <= ball_r {
                            ball_velocity.y = -ball_velocity.y;
                            ball_pos.y = ball_r;
                        }
                    }
                }

                // Paddle
                canvas.begin_path();
                canvas.rect(paddle_rect.origin.x, paddle_rect.origin.y, paddle_rect.size.width, paddle_rect.size.height);
                canvas.fill_path(Paint::color(Color::rgb(200, 200, 200)));

                // Ball
                canvas.begin_path();
                canvas.circle(ball_pos.x, ball_pos.y, ball_r);
                canvas.fill_path(Paint::color(Color::rgb(240, 240, 240)));

                // Bricks
                for brick in &mut bricks {
                    if brick.destroyed { continue; }

                    brick.draw(&mut canvas);

                    if let Some((dir, diff_vector)) = collides(ball_pos, ball_r, brick.rect) {
                        brick.destroyed = true;

                        // Bricks Collision
                        match dir {
                            Direction::Left => {
                                ball_velocity.x = -ball_velocity.x;
                                ball_pos.x += ball_r - diff_vector.x.abs();
                            },
                            Direction::Right => {
                                ball_velocity.x = -ball_velocity.x;
                                ball_pos.x -= ball_r - diff_vector.x.abs();
                            },
                            Direction::Up => {
                                ball_velocity.y = -ball_velocity.y;
                                ball_pos.y -= ball_r - diff_vector.y.abs();
                            },
                            Direction::Down => {
                                ball_velocity.y = -ball_velocity.y;
                                ball_pos.y += ball_r - diff_vector.y.abs();
                            },
                        }
                    }
                }

                // Player-Ball collision
                if let Some((_dir, _diff_vector)) = collides(ball_pos, ball_r, paddle_rect) {
                    // Check where it hit the board, and change velocity based on where it hit the board
                    let paddle_center = paddle_rect.center().x;
                    let distance = ball_pos.x - paddle_center;
                    let percentage = distance / (paddle_rect.size.width / 2.0);
                    // Then move accordingly
                    let strength = 2.0;
                    let old_velocity = ball_velocity;
                    ball_velocity.x = 100.0 * percentage * strength;
                    ball_velocity.y = -1.0 * ball_velocity.y.abs();
                    ball_velocity = ball_velocity.normalize() * old_velocity.length();
                }

                canvas.flush();
                windowed_context.swap_buffers().unwrap();
            }
            Event::MainEventsCleared => {
                windowed_context.window().request_redraw()
            }
            _ => (),
        }
    });
}

#[derive(Copy, Clone)]
enum Direction {
	Up,
	Right,
	Down,
	Left
}

fn collides(center: Point, r: f32, aabb: Rect) -> Option<(Direction, Vector)> {
    let half_extents = aabb.size / 2.0;
    let aabb_center = aabb.center();
    let diff = center - aabb_center;
    let clamped = diff.clamp(-half_extents.to_vector(), half_extents.to_vector());
    let closest = aabb_center + clamped;
    let difference = closest - center;

    if difference.length() < r {
        return Some((vector_direction(difference), difference));
    }

    None
}

fn vector_direction(target: Vector) -> Direction {
    let compass = [
        (Direction::Up, Vector::new(0.0, 1.0)),
        (Direction::Right, Vector::new(1.0, 0.0)),
        (Direction::Down, Vector::new(0.0, -1.0)),
        (Direction::Left, Vector::new(-1.0, 0.0))
    ];

    let mut max = 0.0;
    let mut best_match = Direction::Up;
    let target = target.normalize();

    for (dir, dir_vec) in compass.iter() {
        let dot = target.dot(*dir_vec);

        if dot > max {
            max = dot;
            best_match = *dir;
        }
    }

    best_match
}
