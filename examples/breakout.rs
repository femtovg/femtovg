
use std::time::Instant;

use glutin::event::{Event, WindowEvent, DeviceEvent, ElementState, KeyboardInput, VirtualKeyCode, MouseButton};
use glutin::event_loop::{ControlFlow, EventLoop};
use glutin::window::{Window, WindowBuilder};
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
    ImageId,
    //CompositeOperation,
    renderer::OpenGl
};

type Canvas = gpucanvas::Canvas<OpenGl>;
type Point = euclid::default::Point2D<f32>;
type Vector = euclid::default::Vector2D<f32>;
type Size = euclid::default::Size2D<f32>;
type Rect = euclid::default::Rect<f32>;

//const BLUE: Color = Color::rgb(49, 136, 143);
//const GRAY: Color = Color::rgb(79, 80, 75);
//const BROWN: Color = Color::rgb(143, 80, 49);
//const LIGHTBROWN: Color = Color::rgb(185, 155, 117);
//const DARKBLUE: Color = Color::rgb(2, 49, 55);

#[derive(Copy, Clone)]
enum Direction {
	Up,
	Right,
	Down,
	Left
}

struct Ball {
    position: Point,
    velocity: Vector,
    radius: f32,
    on_paddle: bool
}

impl Ball {
    fn collides(&self, aabb: Rect) -> Option<(Direction, Vector)> {
        let half_extents = aabb.size / 2.0;
        let aabb_center = aabb.center();
        let diff = self.position - aabb_center;
        let clamped = diff.clamp(-half_extents.to_vector(), half_extents.to_vector());
        let closest = aabb_center + clamped;
        let difference = closest - self.position;

        if difference.length() < self.radius {
            return Some((vector_direction(difference), difference));
        }

        None
    }
}

#[derive(Copy, Clone, PartialEq)]
enum State {
    TitleScreen,
    InGame,
    Paused,
    End {
        time: f32
    },
    Win
}

struct Game {
    state: State,
    ball: Ball,
    logo_image_id: ImageId,
    paddle_rect: Rect,
    size: Size,
    bricks: Vec<Brick>,
    levels: Vec<Vec<Vec<u8>>>,
    current_level: usize,
    lives: u8,
    score: u32
}

impl Game {
    fn new(canvas: &mut Canvas, levels: Vec<Vec<Vec<u8>>>) -> Self {
        let image_id = canvas.create_image_file("examples/assets/rust-logo.png", ImageFlags::GENERATE_MIPMAPS).expect("Cannot create image");

        let paddle_rect = Rect::new(Point::new(0.0, 0.0), Size::new(100.0, 20.0));

        let mut game = Self {
            state: State::TitleScreen,
            ball: Ball {
                position: Point::new(-100.0, -100.0),
                velocity: Vector::new(0.0, 0.0),
                radius: 10.0,
                on_paddle: true
            },
            logo_image_id: image_id,
            paddle_rect: paddle_rect,
            size: Size::new(canvas.width(), canvas.height()),
            bricks: Vec::new(),
            levels: levels,
            current_level: 0,
            lives: 3,
            score: 0
        };

        game.load_level();

        game
    }

    fn load_level(&mut self) {
        // Bricks
        let brick_size = Size::new(self.size.width as f32 / self.levels[self.current_level][0].len() as f32, 30.0);
        let mut brick_loc = Point::new(0.0, 0.0);

        for row in &self.levels[self.current_level] {
            for id in row {
                if *id > 0 {
                    let rect = Rect::new(brick_loc, brick_size);
                    self.bricks.push(Brick::new(*id, rect));
                }

                brick_loc.x += brick_size.width;
            }

            brick_loc.x = 0.0;
            brick_loc.y += brick_size.height;
        }
    }

    fn handle_events(&mut self, window: &Window, event: &Event<()>, control_flow: &mut ControlFlow) {
        if self.state != State::InGame {
            let _ = window.set_cursor_grab(false);
            window.set_cursor_visible(true);
        }
        
        match event {
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::KeyboardInput { input: KeyboardInput { virtual_keycode: Some(VirtualKeyCode::Escape), state: ElementState::Pressed, .. }, .. } => {
                    match self.state {
                        State::TitleScreen => *control_flow = ControlFlow::Exit,
                        State::InGame => self.state = State::Paused,
                        State::Paused => self.state = State::TitleScreen,
                        State::Win | State::End { .. } => self.state = State::TitleScreen
                    }
                }
                WindowEvent::MouseInput { button: MouseButton::Left, state: ElementState::Pressed, ..} => {
                    match self.state {
                        State::TitleScreen => {
                            self.state = State::InGame;

                            self.paddle_rect.origin.x = self.size.width as f32 / 2.0 - self.paddle_rect.size.width / 2.0;
                            self.paddle_rect.origin.y = self.size.height as f32 - self.paddle_rect.size.height - 10.0;
                            self.ball.on_paddle = true;
                        }
                        State::Paused => self.state = State::InGame,
                        State::InGame => {
                            let _ = window.set_cursor_grab(true);
                            window.set_cursor_visible(false);

                            if self.ball.on_paddle {
                                self.ball.velocity = Vector::new(100.0, -350.0);
                                self.ball.on_paddle = false;
                            }
                        }
                        _ => ()
                    }
                }
                _ => (),
            }
            Event::DeviceEvent { ref event, .. } => match event {
                DeviceEvent::MouseMotion { delta } => {
                    if self.state == State::InGame {
                        // Move the paddle
                        self.paddle_rect.origin.x += delta.0 as f32;

                        // Clamp it to the window
                        self.paddle_rect.origin.y = self.size.height as f32 - self.paddle_rect.size.height - 10.0;
                        self.paddle_rect.origin = self.paddle_rect.origin.clamp(
                            Point::new(0.0, self.paddle_rect.origin.y),
                            Point::new(self.size.width as f32 - self.paddle_rect.size.width, self.paddle_rect.origin.y)
                        );
                    }
                }
                _ => ()
            }
            _ => (),
        }
    }

    fn update(&mut self, dt: f32) {
        if let State::End { time } = &mut self.state {
            *time -= dt;

            if *time <= 0.0 {
                self.state = State::TitleScreen;
                self.lives = 3;
                self.score = 0;
            }
        }

        if self.state != State::InGame { return }

        if self.ball.on_paddle {
            self.ball.position = (
                self.paddle_rect.center() - Point::new(0.0, self.paddle_rect.height() / 2.0 + self.ball.radius)
            ).to_point();
        } else {
            self.ball.position += self.ball.velocity * dt;

            // Collision with left and right walls
            if self.ball.position.x <= self.ball.radius {
                self.ball.velocity.x = -self.ball.velocity.x;
                self.ball.position.x = self.ball.radius;
            } else if self.ball.position.x + self.ball.radius >= self.size.width {
                self.ball.velocity.x = -self.ball.velocity.x;
                self.ball.position.x = self.size.width - self.ball.radius;
            }

            // Collision with ceiling
            if self.ball.position.y <= self.ball.radius {
                self.ball.velocity.y = -self.ball.velocity.y;
                self.ball.position.y = self.ball.radius;
            }

            if self.ball.position.y > self.size.height {
                self.lives -= 1;

                if self.lives == 0 {
                    self.state = State::End { time: 5.0 };
                } else {
                    self.ball.on_paddle = true;
                }
            }
        }

        // Collision with bricks
        for brick in &mut self.bricks {
            if brick.destroyed { continue; }

            if let Some((dir, diff_vector)) = self.ball.collides(brick.rect) {
                brick.destroyed = true;

                self.score += 10;

                // Bricks Collision
                match dir {
                    Direction::Left => {
                        self.ball.velocity.x = -self.ball.velocity.x;
                        self.ball.position.x += self.ball.radius - diff_vector.x.abs();
                    },
                    Direction::Right => {
                        self.ball.velocity.x = -self.ball.velocity.x;
                        self.ball.position.x -= self.ball.radius - diff_vector.x.abs();
                    },
                    Direction::Up => {
                        self.ball.velocity.y = -self.ball.velocity.y;
                        self.ball.position.y -= self.ball.radius - diff_vector.y.abs();
                    },
                    Direction::Down => {
                        self.ball.velocity.y = -self.ball.velocity.y;
                        self.ball.position.y += self.ball.radius - diff_vector.y.abs();
                    },
                }
            }
        }

        // Player-Ball collision
        if let Some((_dir, _diff_vector)) = self.ball.collides(self.paddle_rect) {
            // Check where it hit the board, and change velocity based on where it hit the board
            let paddle_center = self.paddle_rect.center().x;
            let distance = self.ball.position.x - paddle_center;
            let percentage = distance / (self.paddle_rect.size.width / 2.0);
            // Then move accordingly
            let strength = 2.0;
            let old_velocity = self.ball.velocity;
            self.ball.velocity.x = 100.0 * percentage * strength;
            self.ball.velocity.y = -1.0 * self.ball.velocity.y.abs();
            self.ball.velocity = self.ball.velocity.normalize() * old_velocity.length();
        }
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        match self.state {
            State::TitleScreen => self.draw_title_screen(canvas),
            State::InGame => self.draw_game(canvas),
            State::Paused => self.draw_paused(canvas),
            State::End { .. } => self.draw_end(canvas),
            _ => ()
        }
    }

    fn draw_title_screen(&self, canvas: &mut Canvas) {
        // curtain
        canvas.begin_path();
        canvas.rect(0.0, 0.0, canvas.width(), canvas.height());
        canvas.fill_path(Paint::color(Color::rgba(0, 0, 0, 180)));

        // rust logo
        let logo_pos = Point::new((canvas.width() / 2.0) - 50.0, (canvas.height() / 2.0) - 180.0);
        let logo_paint = Paint::image(self.logo_image_id, logo_pos.x, logo_pos.y, 100.0, 100.0, 0.0, 1.0);
        canvas.begin_path();
        canvas.circle(logo_pos.x + 50.0, logo_pos.y + 50.0, 60.0);
        //canvas.fill_path(Paint::color(Color::rgba(200, 200, 200, 200)));
        canvas.begin_path();
        canvas.rect(logo_pos.x, logo_pos.y, 100.0, 100.0);
        canvas.fill_path(logo_paint);

        // title
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_text_align(Align::Center);
        paint.set_font_name("Roboto-Bold");
        paint.set_font_size(80);

        paint.set_stroke_width(4.0);
        canvas.stroke_text((canvas.width() / 2.0) - 2.0, (canvas.height() / 2.0) - 1.0, "rsBREAKOUT", paint);

        paint.set_color(Color::rgb(143, 80, 49));
        canvas.fill_text(canvas.width() / 2.0, canvas.height() / 2.0, "rsBREAKOUT", paint);

        // Info
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_text_align(Align::Center);
        paint.set_font_name("Roboto-Regular");
        paint.set_font_size(16);
        let text = "Click anywhere to START.";
        canvas.fill_text(canvas.width() / 2.0, (canvas.height() / 2.0) + 40.0, text, paint);
    }

    fn draw_game(&self, canvas: &mut Canvas) {
        // Paddle
        canvas.begin_path();
        canvas.rounded_rect(self.paddle_rect.origin.x, self.paddle_rect.origin.y, self.paddle_rect.size.width, self.paddle_rect.size.height, self.paddle_rect.size.height / 2.0);
        canvas.fill_path(Paint::color(Color::rgb(200, 200, 200)));

        // Ball
        canvas.begin_path();
        canvas.circle(self.ball.position.x, self.ball.position.y, self.ball.radius);
        canvas.fill_path(Paint::color(Color::rgb(2, 49, 55)));

        self.draw_bricks(canvas);

        // lives
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_text_align(Align::Right);
        paint.set_font_name("Roboto-Bold");
        paint.set_font_size(22);
        canvas.fill_text(canvas.width() - 20.0, 25.0, &format!("Lives: {}", self.lives), paint);

        // score
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_font_name("Roboto-Bold");
        paint.set_font_size(22);
        canvas.fill_text(20.0, 25.0, &format!("Score: {}", self.score), paint);
    }

    fn draw_paused(&self, canvas: &mut Canvas) {
        self.draw_bricks(canvas);

        // curtain
        canvas.begin_path();
        canvas.rect(0.0, 0.0, canvas.width(), canvas.height());
        canvas.fill_path(Paint::color(Color::rgba(0, 0, 0, 180)));

        // title
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_text_align(Align::Center);
        paint.set_font_name("Roboto-Bold");
        paint.set_font_size(80);

        paint.set_stroke_width(4.0);
        canvas.stroke_text((canvas.width() / 2.0) - 2.0, (canvas.height() / 2.0) - 1.0, "PAUSE", paint);

        paint.set_color(Color::rgb(143, 80, 49));
        canvas.fill_text(canvas.width() / 2.0, canvas.height() / 2.0, "PAUSE", paint);

        // Info
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_text_align(Align::Center);
        paint.set_font_name("Roboto-Regular");
        paint.set_font_size(16);
        let text = "Click anywhere to resume. Press ESC to exit";
        canvas.fill_text(canvas.width() / 2.0, (canvas.height() / 2.0) + 40.0, text, paint);
    }

    fn draw_end(&self, canvas: &mut Canvas) {
        self.draw_bricks(canvas);

        // curtain
        canvas.begin_path();
        canvas.rect(0.0, 0.0, canvas.width(), canvas.height());
        canvas.fill_path(Paint::color(Color::rgba(0, 0, 0, 180)));

        // title
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_text_align(Align::Center);
        paint.set_font_name("Roboto-Bold");
        paint.set_font_size(80);

        paint.set_stroke_width(4.0);
        canvas.stroke_text((canvas.width() / 2.0) - 2.0, (canvas.height() / 2.0) - 1.0, "NICE RUN!!!", paint);

        paint.set_color(Color::rgb(143, 80, 49));
        canvas.fill_text(canvas.width() / 2.0, canvas.height() / 2.0, "NICE RUN!!!", paint);

        // Info
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_text_align(Align::Center);
        paint.set_font_name("Roboto-Regular");
        paint.set_font_size(16);
        let text = format!("Total score: {}", self.score);
        canvas.fill_text(canvas.width() / 2.0, (canvas.height() / 2.0) + 40.0, &text, paint);
    }

    fn draw_bricks(&self, canvas: &mut Canvas) {
        // Bricks
        for brick in &self.bricks {
            if brick.destroyed { continue; }

            brick.draw(canvas);
        }
    }
}

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
    let mut levels = Vec::new();

    levels.push(vec![
        vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    ]);

    levels.push(vec![
        vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        vec![1, 1, 0, 0, 0, 0, 0, 0, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    ]);

    let window_size = glutin::dpi::PhysicalSize::new(800, 600);
    let el = EventLoop::new();
    let wb = WindowBuilder::new()
        .with_inner_size(window_size)
        .with_resizable(false)
        .with_title("Breakout demo");

    let windowed_context = ContextBuilder::new().build_windowed(wb, &el).unwrap();
    let windowed_context = unsafe { windowed_context.make_current().unwrap() };

    let renderer = OpenGl::new(|s| windowed_context.get_proc_address(s) as *const _).expect("Cannot create renderer");
    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
    canvas.set_size(window_size.width as u32, window_size.height as u32, windowed_context.window().scale_factor() as f32);

    canvas.add_font("examples/assets/Roboto-Bold.ttf");
    canvas.add_font("examples/assets/Roboto-Light.ttf");
    canvas.add_font("examples/assets/Roboto-Regular.ttf");

    let mut game = Game::new(&mut canvas, levels);
    game.size = Size::new(window_size.width as f32, window_size.height as f32);

    let start = Instant::now();
    let mut prevt = start;

    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        game.handle_events(windowed_context.window(), &event, control_flow);

        match event {
            Event::LoopDestroyed => return,
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::Resized(physical_size) => {
                    windowed_context.resize(*physical_size);
                    game.size = Size::new(physical_size.width as f32, physical_size.height as f32);
                }
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit
                }
                _ => (),
            }
            Event::RedrawRequested(_) => {
                let dpi_factor = windowed_context.window().scale_factor();
                let size = windowed_context.window().inner_size();
                canvas.set_size(size.width as u32, size.height as u32, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width as u32, size.height as u32, Color::rgbf(0.9, 0.9, 0.92));

                let now = Instant::now();
                let dt = (now - prevt).as_secs_f32();
                prevt = now;

                game.update(dt);
                game.draw(&mut canvas);

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
