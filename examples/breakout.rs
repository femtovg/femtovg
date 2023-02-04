use femtovg::{renderer::OpenGl, Align, Baseline, Color, FontId, ImageFlags, ImageId, Paint, Path};
use instant::Instant;
use rand::{
    distributions::{Distribution, Standard},
    prelude::*,
};
use resource::resource;
use winit::{
    event::{DeviceEvent, ElementState, Event, KeyboardInput, MouseButton, VirtualKeyCode, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

mod helpers;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    helpers::start(800, 600, "Breakout demo", false);
    #[cfg(target_arch = "wasm32")]
    helpers::start();
}

#[cfg(not(target_arch = "wasm32"))]
use glutin::prelude::*;

type Canvas = femtovg::Canvas<OpenGl>;
type Point = euclid::default::Point2D<f32>;
type Vector = euclid::default::Vector2D<f32>;
type Size = euclid::default::Size2D<f32>;
type Rect = euclid::default::Rect<f32>;

#[derive(Copy, Clone)]
enum Direction {
    Up,
    Right,
    Down,
    Left,
}

struct Ball {
    position: Point,
    velocity: Vector,
    radius: f32,
    on_paddle: bool,
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
    RoundInfo { time: f32 },
    InGame,
    Paused,
    GameOver { time: f32 },
    Win { time: f32 },
}

struct Fonts {
    regular: FontId,
    bold: FontId,
    light: FontId,
}

struct Game {
    state: State,
    balls: Vec<Ball>,
    logo_image_id: ImageId,
    fonts: Fonts,
    paddle_rect: Rect,
    size: Size,
    bricks: Vec<Brick>,
    levels: Vec<Vec<Vec<Cmd>>>,
    powerups: Vec<Powerup>,
    current_level: usize,
    lives: u8,
    score: u32,
}

impl Game {
    fn new(canvas: &mut Canvas, levels: Vec<Vec<Vec<Cmd>>>) -> Self {
        let logo_image_id = canvas
            .load_image_mem(
                &resource!("examples/assets/rust-logo.png"),
                ImageFlags::GENERATE_MIPMAPS,
            )
            .expect("Cannot create image");

        let paddle_rect = Rect::new(Point::new(0.0, 0.0), Size::new(100.0, 20.0));

        let fonts = Fonts {
            regular: canvas
                .add_font_mem(&resource!("examples/assets/Roboto-Regular.ttf"))
                .expect("Cannot add font"),
            bold: canvas
                .add_font_mem(&resource!("examples/assets/Roboto-Bold.ttf"))
                .expect("Cannot add font"),
            light: canvas
                .add_font_mem(&resource!("examples/assets/Roboto-Light.ttf"))
                .expect("Cannot add font"),
        };

        let mut game = Self {
            state: State::TitleScreen,
            balls: vec![Ball {
                position: Point::new(-100.0, -100.0),
                velocity: Vector::new(0.0, 0.0),
                radius: 10.0,
                on_paddle: true,
            }],
            logo_image_id,
            fonts,
            paddle_rect,
            size: Size::new(canvas.width(), canvas.height()),
            bricks: Vec::new(),
            levels,
            powerups: Vec::new(),
            current_level: 0,
            lives: 3,
            score: 0,
        };

        game.load_level();

        game
    }

    fn load_level(&mut self) {
        // Bricks
        let brick_padding = 5.0;

        let brick_size = Size::new(
            self.size.width / self.levels[self.current_level][0].len() as f32,
            30.0,
        );
        let mut brick_loc = Point::new(0.0, 0.0);

        for row in &self.levels[self.current_level] {
            for cmd in row {
                match cmd {
                    Cmd::Spac => {
                        brick_loc.x += brick_size.width;
                    }
                    Cmd::B(id) => {
                        let rect = Rect::new(brick_loc, brick_size - Size::new(brick_padding, brick_padding));
                        self.bricks.push(Brick::new(*id, rect));
                        brick_loc.x += brick_size.width;
                    }
                }
            }

            brick_loc.x = 0.0;
            brick_loc.y += brick_size.height;
        }
    }

    fn handle_events(&mut self, window: &Window, event: &Event<()>, control_flow: &mut ControlFlow) {
        if self.state != State::InGame {
            let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
            window.set_cursor_visible(true);
        } else {
            let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
            window.set_cursor_visible(false);
        }

        match event {
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::KeyboardInput {
                    input:
                        KeyboardInput {
                            virtual_keycode: Some(VirtualKeyCode::Escape),
                            state: ElementState::Pressed,
                            ..
                        },
                    ..
                } => match self.state {
                    State::TitleScreen => *control_flow = ControlFlow::Exit,
                    State::InGame => self.state = State::Paused,
                    State::Paused => self.state = State::TitleScreen,
                    State::Win { .. } | State::GameOver { .. } => self.state = State::TitleScreen,
                    _ => (),
                },
                WindowEvent::MouseInput {
                    button: MouseButton::Left,
                    state: ElementState::Pressed,
                    ..
                } => match self.state {
                    State::TitleScreen => {
                        self.state = State::RoundInfo { time: 3.0 };
                    }
                    State::RoundInfo { .. } => {
                        self.state = State::InGame;

                        self.paddle_rect.origin.x = self.size.width / 2.0 - self.paddle_rect.size.width / 2.0;
                        self.paddle_rect.origin.y = self.size.height - self.paddle_rect.size.height - 10.0;
                        self.balls[0].on_paddle = true;
                    }
                    State::Paused => self.state = State::InGame,
                    State::InGame => {
                        if self.balls[0].on_paddle {
                            self.balls[0].velocity = Vector::new(100.0, -350.0);
                            self.balls[0].on_paddle = false;
                        }
                    }
                    _ => (),
                },
                _ => (),
            },
            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta },
                ..
            } => {
                if self.state == State::InGame {
                    // Move the paddle
                    self.paddle_rect.origin.x += delta.0 as f32;

                    // Clamp it to the window
                    self.paddle_rect.origin.y = self.size.height - self.paddle_rect.size.height - 10.0;
                    self.paddle_rect.origin = self.paddle_rect.origin.clamp(
                        Point::new(0.0, self.paddle_rect.origin.y),
                        Point::new(
                            self.size.width - self.paddle_rect.size.width,
                            self.paddle_rect.origin.y,
                        ),
                    );
                }
            }
            _ => (),
        }
    }

    fn update(&mut self, dt: f32) {
        if let State::Win { time } = &mut self.state {
            *time -= dt;

            if *time <= 0.0 {
                self.state = State::TitleScreen;
                self.lives = 3;
                self.score = 0;
                self.current_level = 0;
                self.load_level();
            }

            return;
        }

        if let State::RoundInfo { time } = &mut self.state {
            *time -= dt;

            if *time <= 0.0 {
                self.state = State::InGame;
                self.paddle_rect.origin.x = self.size.width / 2.0 - self.paddle_rect.size.width / 2.0;
                self.paddle_rect.origin.y = self.size.height - self.paddle_rect.size.height - 10.0;
                self.balls[0].on_paddle = true;
            }

            return;
        }

        if let State::GameOver { time } = &mut self.state {
            *time -= dt;

            if *time <= 0.0 {
                self.state = State::TitleScreen;
                self.lives = 3;
                self.score = 0;
                self.current_level = 0;
                self.load_level();
            }

            return;
        }

        if self.state != State::InGame {
            return;
        }

        if !self.balls.is_empty() && self.balls[0].on_paddle {
            self.balls[0].position = (self.paddle_rect.center()
                - Point::new(0.0, self.paddle_rect.height() / 2.0 + self.balls[0].radius))
            .to_point();
        } else {
            let num_balls = self.balls.len();

            for ball in &mut self.balls {
                ball.position += ball.velocity * dt;

                // Collision with left and right walls
                if ball.position.x <= ball.radius {
                    ball.velocity.x = -ball.velocity.x;
                    ball.position.x = ball.radius;
                } else if ball.position.x + ball.radius >= self.size.width {
                    ball.velocity.x = -ball.velocity.x;
                    ball.position.x = self.size.width - ball.radius;
                }

                // Collision with ceiling
                if ball.position.y <= ball.radius {
                    ball.velocity.y = -ball.velocity.y;
                    ball.position.y = ball.radius;
                }

                if ball.position.y > self.size.height && num_balls == 1 {
                    self.lives -= 1;

                    ball.position.y += 1000.0;

                    if self.lives == 0 {
                        self.state = State::GameOver { time: 5.0 };
                    } else {
                        self.state = State::RoundInfo { time: 3.0 };
                    }
                }
            }

            if let State::RoundInfo { .. } = self.state {
                self.balls.truncate(1);
                self.paddle_rect.size = Size::new(100.0, 20.0);
            } else {
                let height = self.size.height;
                self.balls.retain(|b| b.position.y < height);
            }
        }

        let mut has_hit = false;

        // Collision with bricks
        for brick in &mut self.bricks {
            if brick.destroyed {
                continue;
            }

            for ball in &mut self.balls {
                if let Some((dir, diff_vector)) = ball.collides(brick.rect) {
                    if let BrickType::Multihit(hits) = &mut brick.brick_type {
                        *hits -= 1;

                        if *hits == 0 {
                            brick.destroyed = true;
                            has_hit = true;
                            self.score += brick.score();
                        }
                    } else if brick.brick_type != BrickType::Invincible {
                        brick.destroyed = true;
                        has_hit = true;
                        self.score += brick.score();

                        // drop powerup
                        let x: u8 = rand::random();
                        if x < 100 {
                            self.powerups.push(Powerup {
                                ty: rand::random(),
                                rect: brick.rect,
                            });
                        }
                    }

                    // velocity upper bound
                    if ball.velocity.length() < 700.0 {
                        // increase velocity
                        ball.velocity += ball.velocity * 0.025;
                    }

                    // Bricks Collision
                    match dir {
                        Direction::Left => {
                            ball.velocity.x = -ball.velocity.x;
                            ball.position.x += ball.radius - diff_vector.x.abs();
                        }
                        Direction::Right => {
                            ball.velocity.x = -ball.velocity.x;
                            ball.position.x -= ball.radius - diff_vector.x.abs();
                        }
                        Direction::Up => {
                            ball.velocity.y = -ball.velocity.y;
                            ball.position.y -= ball.radius - diff_vector.y.abs();
                        }
                        Direction::Down => {
                            ball.velocity.y = -ball.velocity.y;
                            ball.position.y += ball.radius - diff_vector.y.abs();
                        }
                    }
                }
            }
        }

        // update powerups
        for powerup in &mut self.powerups {
            powerup.rect.origin.y += 170.0 * dt;

            if self.paddle_rect.intersects(&powerup.rect) {
                powerup.rect.origin.y += 10000.0;

                let mut rng = thread_rng();

                let x: f32 = rng.gen_range(150.0..250.0);
                let y: f32 = rng.gen_range(-350.0..-250.0);

                match powerup.ty {
                    PowerupType::Multiply => {
                        self.balls.push(Ball {
                            position: self.balls[0].position,
                            velocity: Vector::new(x, y),
                            radius: 10.0,
                            on_paddle: false,
                        });

                        self.balls.push(Ball {
                            position: self.balls[0].position,
                            velocity: Vector::new(-x, y),
                            radius: 10.0,
                            on_paddle: false,
                        });
                    }
                    PowerupType::Slow => {
                        for ball in &mut self.balls {
                            if ball.velocity.length() > 100.0 {
                                ball.velocity *= 0.5;
                            }
                        }
                    }
                    PowerupType::Fast => {
                        for ball in &mut self.balls {
                            if ball.velocity.length() < 1000.0 {
                                ball.velocity *= 1.5;
                            }
                        }
                    }
                    PowerupType::Enlarge => {
                        if self.paddle_rect.size.width < 101.0 {
                            self.paddle_rect.origin.x -= 25.0;
                            self.paddle_rect.size.width += 50.0;
                        }
                    }
                    PowerupType::Shrink => {
                        if self.paddle_rect.size.width > 51.0 {
                            self.paddle_rect.origin.x += 25.0;
                            self.paddle_rect.size.width -= 50.0;
                        }
                    }
                    PowerupType::Live => {
                        self.lives += 1;
                    }
                }
            }
        }

        // remove out of bounds powerups
        let height = self.size.height;
        self.powerups.retain(|powerup| powerup.rect.origin.y < height);

        // check if all bricks are cleared
        if has_hit
            && self
                .bricks
                .iter()
                .all(|b| b.destroyed && b.brick_type != BrickType::Invincible)
        {
            // next level or win
            if self.current_level == self.levels.len() - 1 {
                // win
                self.state = State::Win { time: 5.0 };
            } else {
                // next level
                self.balls.truncate(1);
                self.current_level += 1;
                self.load_level();
                self.powerups.clear();
                self.state = State::RoundInfo { time: 3.0 };
                self.paddle_rect.size = Size::new(100.0, 20.0);
            }
        }

        // Player-Ball collision
        for ball in &mut self.balls {
            if let Some((_dir, _diff_vector)) = ball.collides(self.paddle_rect) {
                // Check where it hit the board, and change velocity based on where it hit the board
                let paddle_center = self.paddle_rect.center().x;
                let distance = ball.position.x - paddle_center;
                let percentage = distance / (self.paddle_rect.size.width / 2.0);
                // Then move accordingly
                let strength = 4.0;
                let old_velocity = ball.velocity;
                ball.velocity.x = 100.0 * percentage * strength;
                ball.velocity.y = -1.0 * ball.velocity.y.abs();
                ball.velocity = ball.velocity.normalize() * old_velocity.length();
            }
        }
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        // draw background
        let step_size_x = canvas.width() / 50.0;

        let mut path = Path::new();

        for i in 0..50 {
            path.move_to(i as f32 * step_size_x, 0.0);
            path.line_to(i as f32 * step_size_x, canvas.height());
        }

        let paint = Paint::radial_gradient(
            canvas.width() / 2.0,
            canvas.height() / 2.0,
            10.0,
            canvas.height() / 2.0,
            Color::rgb(90, 90, 90),
            Color::rgb(30, 30, 30),
        );
        canvas.stroke_path(&mut path, &paint);

        match self.state {
            State::TitleScreen => self.draw_title_screen(canvas),
            State::RoundInfo { .. } => self.draw_round_info(canvas),
            State::InGame => self.draw_game(canvas),
            State::Paused => self.draw_paused(canvas),
            State::GameOver { .. } => self.draw_game_over(canvas),
            State::Win { .. } => self.draw_win(canvas),
        }
    }

    fn draw_title_screen(&self, canvas: &mut Canvas) {
        // curtain
        let mut path = Path::new();
        path.rect(0.0, 0.0, canvas.width(), canvas.height());
        canvas.fill_path(&mut path, &Paint::color(Color::rgba(0, 0, 0, 180)));

        // rust logo
        let logo_pos = Point::new((canvas.width() / 2.0) - 50.0, (canvas.height() / 2.0) - 180.0);
        let logo_paint = Paint::image(self.logo_image_id, logo_pos.x, logo_pos.y, 100.0, 100.0, 0.0, 1.0);
        let mut path = Path::new();
        path.circle(logo_pos.x + 50.0, logo_pos.y + 50.0, 60.0);
        //canvas.fill_path(&mut path, &Paint::color(Color::rgba(200, 200, 200, 200)));
        let mut path = Path::new();
        path.rect(logo_pos.x, logo_pos.y, 100.0, 100.0);
        canvas.fill_path(&mut path, &logo_paint);

        // title
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_text_align(Align::Center);
        paint.set_font(&[self.fonts.bold]);
        paint.set_font_size(80.0);

        paint.set_line_width(4.0);
        let _ = canvas.stroke_text(canvas.width() / 2.0, canvas.height() / 2.0, "rsBREAKOUT", &paint);

        paint.set_color(Color::rgb(143, 80, 49));
        let _ = canvas.fill_text(canvas.width() / 2.0, canvas.height() / 2.0, "rsBREAKOUT", &paint);

        // Info
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_text_align(Align::Center);
        paint.set_font(&[self.fonts.regular]);
        paint.set_font_size(16.0);
        let text = "Click anywhere to START.";
        let _ = canvas.fill_text(canvas.width() / 2.0, (canvas.height() / 2.0) + 40.0, text, &paint);
    }

    fn draw_game(&self, canvas: &mut Canvas) {
        // Paddle
        let side_size = 15.0;

        let highlight = Paint::linear_gradient(
            self.paddle_rect.origin.x,
            self.paddle_rect.origin.y,
            self.paddle_rect.origin.x,
            self.paddle_rect.origin.y + 10.0,
            Color::rgba(255, 255, 255, 100),
            Color::rgba(255, 255, 255, 40),
        );

        let mut path = Path::new();
        path.rounded_rect_varying(
            self.paddle_rect.origin.x,
            self.paddle_rect.origin.y,
            side_size,
            self.paddle_rect.size.height,
            self.paddle_rect.size.height / 2.0,
            0.0,
            0.0,
            self.paddle_rect.size.height / 2.0,
        );
        path.rounded_rect_varying(
            self.paddle_rect.origin.x + self.paddle_rect.size.width - side_size,
            self.paddle_rect.origin.y,
            side_size,
            self.paddle_rect.size.height,
            0.0,
            self.paddle_rect.size.height / 2.0,
            self.paddle_rect.size.height / 2.0,
            0.0,
        );
        canvas.fill_path(&mut path, &Paint::color(Color::rgb(119, 123, 126)));
        canvas.stroke_path(&mut path, &highlight);

        let mut path = Path::new();
        path.rect(
            self.paddle_rect.origin.x + side_size + 3.0,
            self.paddle_rect.origin.y,
            self.paddle_rect.size.width - (side_size * 2.0) - 6.0,
            self.paddle_rect.size.height,
        );
        canvas.fill_path(&mut path, &Paint::color(Color::rgb(119, 123, 126)));
        canvas.stroke_path(&mut path, &highlight);

        let mut path = Path::new();
        path.rounded_rect_varying(
            self.paddle_rect.origin.x,
            self.paddle_rect.origin.y,
            self.paddle_rect.size.width,
            self.paddle_rect.size.height - 10.0,
            self.paddle_rect.size.height / 2.0,
            self.paddle_rect.size.height / 2.0,
            25.0,
            25.0,
        );

        canvas.fill_path(&mut path, &highlight);

        // Ball
        for ball in &self.balls {
            let mut path = Path::new();
            path.circle(ball.position.x, ball.position.y, ball.radius);
            canvas.fill_path(&mut path, &Paint::color(Color::rgb(183, 65, 14)));

            let bg = Paint::linear_gradient(
                ball.position.x,
                ball.position.y - ball.radius,
                ball.position.x,
                ball.position.y,
                Color::rgba(255, 255, 255, 60),
                Color::rgba(255, 255, 255, 16),
            );

            let mut path = Path::new();
            path.circle(ball.position.x, ball.position.y - ball.radius / 2.0, ball.radius / 2.0);
            canvas.fill_path(&mut path, &bg);
        }

        // powerups
        for powerup in &self.powerups {
            powerup.draw(canvas, &self.fonts);
        }

        self.draw_bricks(canvas);

        // lives
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_text_align(Align::Right);
        paint.set_font(&[self.fonts.bold]);
        paint.set_font_size(22.0);
        let _ = canvas.fill_text(canvas.width() - 20.0, 25.0, &format!("Lives: {}", self.lives), &paint);

        // score
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_font(&[self.fonts.bold]);
        paint.set_font_size(22.0);
        let _ = canvas.fill_text(20.0, 25.0, &format!("Score: {}", self.score), &paint);
    }

    fn draw_round_info(&self, canvas: &mut Canvas) {
        let heading = format!("ROUND {}", self.current_level + 1);

        self.draw_generic_info(canvas, &heading, "");
    }

    fn draw_paused(&self, canvas: &mut Canvas) {
        self.draw_generic_info(canvas, "PAUSE", "Click anywhere to resume. Press ESC to exit");
    }

    fn draw_game_over(&self, canvas: &mut Canvas) {
        let score = format!("Score: {}", self.score);

        self.draw_generic_info(canvas, "Game Over", &score);
    }

    fn draw_win(&self, canvas: &mut Canvas) {
        let score = format!("Final score: {}", self.score);

        self.draw_generic_info(canvas, "All cleared!", &score);
    }

    fn draw_bricks(&self, canvas: &mut Canvas) {
        // Bricks
        for brick in &self.bricks {
            if brick.destroyed {
                continue;
            }

            brick.draw(canvas);
        }
    }

    fn draw_generic_info(&self, canvas: &mut Canvas, heading: &str, subtext: &str) {
        self.draw_bricks(canvas);

        // curtain
        let mut path = Path::new();
        path.rect(0.0, 0.0, canvas.width(), canvas.height());
        canvas.fill_path(&mut path, &Paint::color(Color::rgba(0, 0, 0, 32)));

        // title
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_text_align(Align::Center);
        paint.set_font(&[self.fonts.bold]);
        paint.set_font_size(80.0);

        let offset = 30.0;

        paint.set_line_width(4.0);
        let _ = canvas.stroke_text(canvas.width() / 2.0, (canvas.height() / 2.0) + offset, heading, &paint);

        paint.set_color(Color::rgb(143, 80, 49));
        let _ = canvas.fill_text(canvas.width() / 2.0, (canvas.height() / 2.0) + offset, heading, &paint);

        // Info
        let mut paint = Paint::color(Color::rgb(240, 240, 240));
        paint.set_text_align(Align::Center);
        paint.set_font(&[self.fonts.regular]);
        paint.set_font_size(16.0);
        let _ = canvas.fill_text(
            canvas.width() / 2.0,
            (canvas.height() / 2.0) + offset * 2.0,
            subtext,
            &paint,
        );
    }
}

#[derive(Copy, Clone, Debug)]
enum PowerupType {
    Enlarge,
    Shrink,
    Slow,
    Multiply,
    Fast,
    Live,
}

impl Distribution<PowerupType> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> PowerupType {
        match rng.gen_range(0..7) {
            0 => PowerupType::Enlarge,
            1 => PowerupType::Shrink,
            3 => PowerupType::Slow,
            4 => PowerupType::Multiply,
            5 => PowerupType::Fast,
            6 => PowerupType::Live,
            _ => PowerupType::Multiply,
        }
    }
}

struct Powerup {
    ty: PowerupType,
    rect: Rect,
}

impl Powerup {
    fn draw(&self, canvas: &mut Canvas, fonts: &Fonts) {
        let mut path = Path::new();
        path.rounded_rect(
            self.rect.origin.x,
            self.rect.origin.y,
            self.rect.size.width,
            self.rect.size.height,
            5.0,
        );

        canvas.stroke_path(&mut path, &Paint::color(Color::rgb(240, 240, 240)));

        let mut text_paint = Paint::color(Color::rgb(240, 240, 240));
        text_paint.set_text_align(Align::Center);
        text_paint.set_text_baseline(Baseline::Middle);
        text_paint.set_font(&[fonts.light]);
        text_paint.set_font_size(16.0);
        let _ = canvas.fill_text(
            self.rect.center().x,
            self.rect.center().y,
            &format!("{:?}", self.ty),
            &text_paint,
        );
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum BrickType {
    Variant0,
    Variant1,
    Variant2,
    Variant3,
    Invincible,
    Multihit(u8),
}

struct Brick {
    brick_type: BrickType,
    destroyed: bool,
    rect: Rect,
}

impl Brick {
    fn new(id: u8, rect: Rect) -> Self {
        let brick_type = match id {
            0 => BrickType::Variant0,
            1 => BrickType::Variant1,
            2 => BrickType::Variant2,
            3 => BrickType::Variant3,
            4 => BrickType::Invincible,
            5 => BrickType::Multihit(2),
            _ => BrickType::Variant0,
        };

        Self {
            brick_type,
            destroyed: false,
            rect,
        }
    }

    fn score(&self) -> u32 {
        match self.brick_type {
            BrickType::Variant0 => 40,
            BrickType::Variant1 => 50,
            BrickType::Variant2 => 60,
            BrickType::Variant3 => 70,
            BrickType::Invincible => 0,
            BrickType::Multihit(_) => 20,
        }
    }

    fn draw(&self, canvas: &mut Canvas) {
        if self.destroyed {
            return;
        }

        let mut path = Path::new();
        path.rounded_rect(
            self.rect.origin.x,
            self.rect.origin.y,
            self.rect.size.width,
            self.rect.size.height,
            3.0,
        );

        let paint = Paint::color(match self.brick_type {
            BrickType::Variant0 => Color::rgb(49, 136, 143),
            BrickType::Variant1 => Color::rgb(143, 80, 49),
            BrickType::Variant2 => Color::rgb(185, 155, 117),
            BrickType::Variant3 => Color::rgb(211, 211, 211),
            BrickType::Invincible => Color::rgb(79, 80, 75),
            BrickType::Multihit(hits) => match hits {
                2 => Color::rgb(152, 152, 152),
                _ => Color::rgb(175, 175, 175),
            },
        });

        canvas.fill_path(&mut path, &paint);
        canvas.stroke_path(&mut path, &Paint::color(Color::rgb(240, 240, 240)));

        let mut path = Path::new();
        path.rounded_rect_varying(
            self.rect.origin.x,
            self.rect.origin.y,
            self.rect.size.width,
            self.rect.size.height / 2.0,
            3.0,
            3.0,
            15.0,
            15.0,
        );
        canvas.fill_path(&mut path, &Paint::color(Color::rgba(255, 255, 255, 50)));
    }
}

// Level commands
enum Cmd {
    Spac,  // 1 brick space
    B(u8), // Brick Id
}

fn run(
    mut canvas: Canvas,
    el: EventLoop<()>,
    #[cfg(not(target_arch = "wasm32"))] context: glutin::context::PossiblyCurrentContext,
    #[cfg(not(target_arch = "wasm32"))] surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
    window: Window,
) {
    let mut levels = Vec::new();

    levels.push(vec![
        vec![
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
        ],
        vec![
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
        ],
        vec![
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
        ],
        vec![
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
        ],
        vec![
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
        ],
    ]);

    levels.push(vec![
        vec![
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(0),
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(0),
            Cmd::B(0),
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::Spac,
        ],
        vec![
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
        ],
    ]);

    levels.push(vec![
        vec![
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
            Cmd::B(5),
        ],
        vec![
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
            Cmd::B(3),
        ],
        vec![
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
            Cmd::B(2),
        ],
        vec![
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
            Cmd::B(1),
        ],
        vec![
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
            Cmd::Spac,
        ],
        vec![
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
            Cmd::B(0),
        ],
    ]);

    let mut game = Game::new(&mut canvas, levels);
    game.size = Size::new(window.inner_size().width as f32, window.inner_size().height as f32);

    let start = Instant::now();
    let mut prevt = start;

    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        game.handle_events(&window, &event, control_flow);

        match event {
            Event::LoopDestroyed => *control_flow = ControlFlow::Exit,
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::Resized(physical_size) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    surface.resize(
                        &context,
                        physical_size.width.try_into().unwrap(),
                        physical_size.height.try_into().unwrap(),
                    );
                    game.size = Size::new(physical_size.width as f32, physical_size.height as f32);
                }
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                _ => (),
            },
            Event::RedrawRequested(_) => {
                let dpi_factor = window.scale_factor();
                let size = window.inner_size();
                canvas.set_size(size.width, size.height, dpi_factor as f32);
                canvas.clear_rect(
                    0,
                    0,
                    size.width,
                    size.height,
                    Color::rgbf(0.15, 0.15, 0.12),
                );

                let now = Instant::now();
                let dt = (now - prevt).as_secs_f32();
                prevt = now;

                game.update(dt);
                game.draw(&mut canvas);

                canvas.flush();
                #[cfg(not(target_arch = "wasm32"))]
                surface.swap_buffers(&context).unwrap();
            }
            Event::MainEventsCleared => window.request_redraw(),
            _ => (),
        }
    });
}

fn vector_direction(target: Vector) -> Direction {
    let compass = [
        (Direction::Up, Vector::new(0.0, 1.0)),
        (Direction::Right, Vector::new(1.0, 0.0)),
        (Direction::Down, Vector::new(0.0, -1.0)),
        (Direction::Left, Vector::new(-1.0, 0.0)),
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
