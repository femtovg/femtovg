use std::{f32::consts::PI, sync::Arc};

use femtovg::{Canvas, Color, Paint, Path, Renderer, StrokeSettings, TextSettings};
use instant::Instant;
use winit::{event::WindowEvent, window::Window};

mod helpers;
use helpers::WindowSurface;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    helpers::start(1000, 750, "3D Path Projections", false);
    #[cfg(target_arch = "wasm32")]
    helpers::start();
}

fn run<W: WindowSurface + 'static>(
    mut canvas: Canvas<W::Renderer>,
    mut surface: W,
    window: Arc<Window>,
) -> helpers::Callbacks {
    let font = canvas
        .add_font_mem(&resource::resource!("examples/assets/Roboto-Regular.ttf"))
        .expect("Cannot add font");

    let start = Instant::now();

    helpers::Callbacks {
        window_event: Box::new(move |event, event_loop| match event {
            #[cfg(not(target_arch = "wasm32"))]
            WindowEvent::Resized(physical_size) => {
                surface.resize(physical_size.width, physical_size.height);
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                let size = window.inner_size();
                let dpi_factor = window.scale_factor();
                canvas.set_size(size.width, size.height, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width, size.height, Color::rgbf(0.1, 0.1, 0.15));

                let elapsed = start.elapsed().as_secs_f32();
                let width = size.width as f32;
                let height = size.height as f32;

                let label = TextSettings::new(&[font], 16.0);

                let half_w = width / 2.0;
                let half_h = height / 2.0;

                let qh = half_h / 2.0;
                draw_scene(
                    &mut canvas,
                    elapsed,
                    [half_w * 0.5, half_h * 0.5],
                    qh,
                    "Perspective",
                    project_perspective,
                    &label,
                );
                draw_scene(
                    &mut canvas,
                    elapsed,
                    [half_w * 1.5, half_h * 0.5],
                    qh,
                    "Orthographic",
                    project_orthographic,
                    &label,
                );
                draw_scene(
                    &mut canvas,
                    elapsed,
                    [half_w * 0.5, half_h * 1.5],
                    qh,
                    "Fisheye",
                    project_fisheye,
                    &label,
                );
                draw_scene(
                    &mut canvas,
                    elapsed,
                    [half_w * 1.5, half_h * 1.5],
                    qh,
                    "Cylindrical",
                    project_cylindrical,
                    &label,
                );

                let divider_paint = Paint::color(Color::rgba(255, 255, 255, 30));
                let divider_stroke = StrokeSettings::new(1.0);
                let mut divider = Path::new();
                divider.move_to([half_w, 0.0]);
                divider.line_to([half_w, height]);
                divider.move_to([0.0, half_h]);
                divider.line_to([width, half_h]);
                canvas.stroke_path(&divider, &divider_paint, &divider_stroke);

                surface.present(&mut canvas);
                window.request_redraw();
            }
            _ => (),
        }),
        device_event: None,
    }
}

fn rotate_y(point: [f32; 3], angle: f32) -> [f32; 3] {
    let (sin, cos) = angle.sin_cos();
    [
        point[0] * cos + point[2] * sin,
        point[1],
        -point[0] * sin + point[2] * cos,
    ]
}

fn rotate_x(point: [f32; 3], angle: f32) -> [f32; 3] {
    let (sin, cos) = angle.sin_cos();
    [
        point[0],
        point[1] * cos - point[2] * sin,
        point[1] * sin + point[2] * cos,
    ]
}

fn make_scene(time: f32) -> (Path<3>, Path<3>) {
    let size = 70.0;
    let vertices: [[f32; 3]; 8] = [
        [-size, -size, -size],
        [size, -size, -size],
        [size, size, -size],
        [-size, size, -size],
        [-size, -size, size],
        [size, -size, size],
        [size, size, size],
        [-size, size, size],
    ];

    let edges: [[usize; 2]; 12] = [
        [0, 1],
        [1, 2],
        [2, 3],
        [3, 0],
        [4, 5],
        [5, 6],
        [6, 7],
        [7, 4],
        [0, 4],
        [1, 5],
        [2, 6],
        [3, 7],
    ];

    let mut wireframe = Path::<3>::new();
    for [a, b] in &edges {
        let va = rotate_x(rotate_y(vertices[*a], time * 0.7), time * 0.5);
        let vb = rotate_x(rotate_y(vertices[*b], time * 0.7), time * 0.5);
        wireframe.move_to(va);
        wireframe.line_to(vb);
    }

    let steps = 100;
    let radius = 50.0;
    let mut helix = Path::<3>::new();
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let angle = t * PI * 6.0 + time;
        let point = rotate_x(
            rotate_y(
                [angle.cos() * radius, (t - 0.5) * 200.0, angle.sin() * radius],
                time * 0.7,
            ),
            time * 0.5,
        );
        if i == 0 {
            helix.move_to(point);
        } else {
            helix.line_to(point);
        }
    }

    (wireframe, helix)
}

fn project_perspective(point: [f32; 3]) -> [f32; 2] {
    let distance = 150.0;
    let scale = distance / (distance + point[2]);
    [point[0] * scale, point[1] * scale]
}

fn project_orthographic(point: [f32; 3]) -> [f32; 2] {
    [point[0], point[1]]
}

fn project_fisheye(point: [f32; 3]) -> [f32; 2] {
    let distance = 120.0;
    let r = (point[0] * point[0] + point[1] * point[1]).sqrt();
    let theta = r.atan2(distance + point[2]);
    let fisheye_r = distance * theta;

    if r < 0.001 {
        [0.0, 0.0]
    } else {
        let scale = fisheye_r / r;
        [point[0] * scale, point[1] * scale]
    }
}

fn project_cylindrical(point: [f32; 3]) -> [f32; 2] {
    let distance = 120.0;
    let angle = point[0].atan2(distance + point[2]);
    let x = angle * distance;
    let depth = ((distance + point[2]) * (distance + point[2]) + point[0] * point[0]).sqrt();
    let y = point[1] * distance / depth;
    [x, y]
}

fn draw_scene<T: Renderer>(
    canvas: &mut Canvas<T>,
    time: f32,
    center: [f32; 2],
    quad_half_h: f32,
    title: &str,
    project: fn([f32; 3]) -> [f32; 2],
    label_settings: &TextSettings,
) {
    let (wireframe, helix) = make_scene(time);

    let map = |p: [f32; 3]| -> [f32; 2] {
        let [x, y] = project(p);
        [center[0] + x, center[1] + y]
    };

    let wire2d = wireframe.map(map);
    let helix2d = helix.map(map);

    let stroke = StrokeSettings::new(2.0);
    canvas.stroke_path(&wire2d, &Paint::color(Color::rgba(100, 200, 255, 200)), &stroke);
    canvas.stroke_path(&helix2d, &Paint::color(Color::rgba(255, 150, 50, 200)), &stroke);

    let paint = Paint::color(Color::rgba(255, 255, 255, 200));
    let title_settings = label_settings.clone().with_align(femtovg::Align::Center);
    let _ = canvas.fill_text(
        center[0],
        center[1] - quad_half_h + 20.0,
        title,
        &paint,
        &title_settings,
    );
}
