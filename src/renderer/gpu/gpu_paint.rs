
use crate::{ImageFlags, Scissor};
use crate::paint::{Paint, PaintFlavor};
use crate::geometry::Transform2D;
use super::{ShaderType, TextureType, GpuBackend};

#[derive(Copy, Clone, Debug, Default)]
pub struct GpuPaint {
    pub(super) scissor_mat: [f32; 12],
    pub(super) paint_mat: [f32; 12],
    pub(super) inner_col: [f32; 4],
    pub(super) outer_col: [f32; 4],
    pub(super) scissor_ext: [f32; 2],
    pub(super) scissor_scale: [f32; 2],
    pub(super) extent: [f32; 2],
    pub(super) radius: f32,
    pub(super) feather: f32,
    pub(super) stroke_mult: f32,
    pub(super) stroke_thr: f32,
    pub(super) shader_type: f32,
    pub(super) tex_type: f32
}

impl GpuPaint {

    pub fn new<T: GpuBackend>(backend: &T, paint: &Paint, scissor: &Scissor, width: f32, fringe: f32, stroke_thr: f32) -> Self {
        let mut gpu_paint = GpuPaint::default();

        // Scissor
        let (scissor_ext, scissor_scale) = if let Some(ext) = scissor.extent {
            if ext[0] < -0.5 || ext[1] < -0.5 {
                ([1.0, 1.0], [1.0, 1.0])
            } else {
                gpu_paint.scissor_mat = scissor.transform.inversed().to_mat3x4();

                let scissor_scale = [
                    (scissor.transform[0]*scissor.transform[0] + scissor.transform[2]*scissor.transform[2]).sqrt() / fringe,
                    (scissor.transform[1]*scissor.transform[1] + scissor.transform[3]*scissor.transform[3]).sqrt() / fringe
                ];

                (ext, scissor_scale)
            }
        } else {
            ([1.0, 1.0], [1.0, 1.0])
        };

        gpu_paint.scissor_ext = scissor_ext;
        gpu_paint.scissor_scale = scissor_scale;

        gpu_paint.stroke_mult = (width*0.5 + fringe*0.5) / fringe;
        gpu_paint.stroke_thr = stroke_thr;

        // Paint flavor
        let inv_transform;

        match paint.flavor {
            PaintFlavor::Color(color) => {
                let color = color.premultiplied().to_array();
                gpu_paint.inner_col = color;
                gpu_paint.outer_col = color;
                gpu_paint.shader_type = ShaderType::FillGradient.to_f32();
                inv_transform = paint.transform.inversed();
            },
            PaintFlavor::Image { id, cx, cy, width, height, angle, alpha, tint } => {
                let texture_flags = backend.texture_flags(id);

                gpu_paint.extent[0] = width;
                gpu_paint.extent[1] = height;

                gpu_paint.inner_col = tint.premultiplied().to_array();
                gpu_paint.outer_col = tint.premultiplied().to_array();

                gpu_paint.inner_col[3] = alpha;
                gpu_paint.outer_col[3] = alpha;

                let mut transform = Transform2D::identity();
                transform.rotate(angle);
                transform.translate(cx, cy);
                transform.multiply(&paint.transform);

                if texture_flags.contains(ImageFlags::FLIP_Y) {
                    let mut m1 = Transform2D::identity();
                    m1.translate(0.0, height * 0.5);
                    m1.multiply(&transform);

                    let mut m2 = Transform2D::identity();
                    m2.scale(1.0, -1.0);
                    m2.multiply(&m1);

                    m1.translate(0.0, -height * 0.5);
                    m1.multiply(&m2);

                    inv_transform = m1.inversed();
                } else {
                    inv_transform = transform.inversed();
                }

                gpu_paint.shader_type = ShaderType::FillImage.to_f32();

                gpu_paint.tex_type = match backend.texture_type(id) {
                    Some(TextureType::Rgba) => if texture_flags.contains(ImageFlags::PREMULTIPLIED) { 0.0 } else { 1.0 },
                    Some(TextureType::Alpha) => 2.0,
                    _ => 0.0
                };
            },
            PaintFlavor::LinearGradient { start_x, start_y, end_x, end_y, start_color, end_color } => {
                let large = 1e5f32;
                let mut dx = end_x - start_x;
                let mut dy = end_y - start_y;
                let d = (dx*dx + dy*dy).sqrt();

                if d > 0.0001 {
                    dx /= d;
                    dy /= d;
                } else {
                    dx = 0.0;
                    dy = 1.0;
                }

                let mut transform = Transform2D([
                    dy, -dx,
                    dx, dy,
                    start_x - dx*large, start_y - dy*large
                ]);

                transform.multiply(&paint.transform);

                inv_transform = transform.inversed();

                gpu_paint.extent[0] = large;
                gpu_paint.extent[1] = large + d*0.5;
                gpu_paint.feather = 1.0f32.max(d);

                gpu_paint.inner_col = start_color.premultiplied().to_array();
                gpu_paint.outer_col = end_color.premultiplied().to_array();
                gpu_paint.shader_type = ShaderType::FillGradient.to_f32();
            }
            PaintFlavor::BoxGradient { x, y, width, height, radius, feather, inner_color, outer_color } => {
                let mut transform = Transform2D::new_translation(x + width * 0.5, y + height * 0.5);
                transform.multiply(&paint.transform);
                inv_transform = transform.inversed();

                gpu_paint.extent[0] = width * 0.5;
                gpu_paint.extent[1] = height * 0.5;
                gpu_paint.radius = radius;
                gpu_paint.feather = feather;
                gpu_paint.inner_col = inner_color.premultiplied().to_array();
                gpu_paint.outer_col = outer_color.premultiplied().to_array();
                gpu_paint.shader_type = ShaderType::FillGradient.to_f32();
            }
            PaintFlavor::RadialGradient { cx, cy, in_radius, out_radius, inner_color, outer_color } => {
                let r = (in_radius + out_radius) * 0.5;
                let f = out_radius - in_radius;

                let mut transform = Transform2D::new_translation(cx, cy);
                transform.multiply(&paint.transform);
                inv_transform = transform.inversed();

                gpu_paint.extent[0] = r;
                gpu_paint.extent[1] = r;
                gpu_paint.radius = r;
                gpu_paint.feather = 1.0f32.max(f);
                gpu_paint.inner_col = inner_color.premultiplied().to_array();
                gpu_paint.outer_col = outer_color.premultiplied().to_array();
                gpu_paint.shader_type = ShaderType::FillGradient.to_f32();
            }
        }

        gpu_paint.paint_mat = inv_transform.to_mat3x4();

        gpu_paint
    }

}
