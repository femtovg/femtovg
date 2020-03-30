
use crate::{
    ImageFlags,
    ImageStore,
    Scissor,
    Color,
    Paint,
    PaintFlavor,
    Transform2D,
};

use super::{
    Image,
    ShaderType,
    ImageFormat,
};

#[derive(Copy, Clone, Debug, Default)]
pub struct Params {
    pub(crate) scissor_mat: [f32; 12],
    pub(crate) paint_mat: [f32; 12],
    pub(crate) inner_col: [f32; 4],
    pub(crate) outer_col: [f32; 4],
    pub(crate) scissor_ext: [f32; 2],
    pub(crate) scissor_scale: [f32; 2],
    pub(crate) extent: [f32; 2],
    pub(crate) radius: f32,
    pub(crate) feather: f32,
    pub(crate) stroke_mult: f32,
    pub(crate) stroke_thr: f32,
    pub(crate) tex_type: f32,
    pub(crate) shader_type: f32,
    pub(crate) has_mask: f32,
}

impl Params {

    pub(crate) fn new<T: Image>(images: &ImageStore<T>, paint: &Paint, scissor: &Scissor, stroke_width: f32, fringe_width: f32, stroke_thr: f32) -> Self {
        let mut params = Params::default();

        // Scissor
        let (scissor_ext, scissor_scale) = if let Some(ext) = scissor.extent {
            if ext[0] < -0.5 || ext[1] < -0.5 {
                ([1.0, 1.0], [1.0, 1.0])
            } else {
                params.scissor_mat = scissor.transform.inversed().to_mat3x4();

                let scissor_scale = [
                    (scissor.transform[0]*scissor.transform[0] + scissor.transform[2]*scissor.transform[2]).sqrt() / fringe_width,
                    (scissor.transform[1]*scissor.transform[1] + scissor.transform[3]*scissor.transform[3]).sqrt() / fringe_width
                ];

                (ext, scissor_scale)
            }
        } else {
            ([1.0, 1.0], [1.0, 1.0])
        };

        params.scissor_ext = scissor_ext;
        params.scissor_scale = scissor_scale;

        params.stroke_mult = (stroke_width*0.5 + fringe_width*0.5) / fringe_width;
        params.stroke_thr = stroke_thr;

        params.has_mask = if paint.alpha_mask().is_some() { 1.0 } else { 0.0 };

        let inv_transform;

        match paint.flavor {
            PaintFlavor::Color(color) => {
                let color = color.premultiplied().to_array();
                params.inner_col = color;
                params.outer_col = color;
                params.shader_type = ShaderType::FillGradient.to_f32();
                inv_transform = paint.transform.inversed();
            },
            PaintFlavor::Image { id, cx, cy, width, height, angle, alpha } => {
                let image_info = match images.get(id) {
                    Some(image) => image.info(),
                    None => return params
                };

                params.extent[0] = width;
                params.extent[1] = height;

                let color = Color::rgbaf(1.0, 1.0, 1.0, alpha);

                params.inner_col = color.premultiplied().to_array();
                params.outer_col = color.premultiplied().to_array();

                let mut transform = Transform2D::identity();
                transform.rotate(angle);
                transform.translate(cx, cy);
                transform.multiply(&paint.transform);

                if image_info.flags.contains(ImageFlags::FLIP_Y) {
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

                params.shader_type = ShaderType::FillImage.to_f32();

                params.tex_type = match image_info.format {
                    ImageFormat::Rgba => if image_info.flags.contains(ImageFlags::PREMULTIPLIED) { 0.0 } else { 1.0 },
                    ImageFormat::Alpha => 2.0,
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

                params.extent[0] = large;
                params.extent[1] = large + d*0.5;
                params.feather = 1.0f32.max(d);

                params.inner_col = start_color.premultiplied().to_array();
                params.outer_col = end_color.premultiplied().to_array();
                params.shader_type = ShaderType::FillGradient.to_f32();
            }
            PaintFlavor::BoxGradient { x, y, width, height, radius, feather, inner_color, outer_color } => {
                let mut transform = Transform2D::new_translation(x + width * 0.5, y + height * 0.5);
                transform.multiply(&paint.transform);
                inv_transform = transform.inversed();

                params.extent[0] = width * 0.5;
                params.extent[1] = height * 0.5;
                params.radius = radius;
                params.feather = feather;
                params.inner_col = inner_color.premultiplied().to_array();
                params.outer_col = outer_color.premultiplied().to_array();
                params.shader_type = ShaderType::FillGradient.to_f32();
            }
            PaintFlavor::RadialGradient { cx, cy, in_radius, out_radius, inner_color, outer_color } => {
                let r = (in_radius + out_radius) * 0.5;
                let f = out_radius - in_radius;

                let mut transform = Transform2D::new_translation(cx, cy);
                transform.multiply(&paint.transform);
                inv_transform = transform.inversed();

                params.extent[0] = r;
                params.extent[1] = r;
                params.radius = r;
                params.feather = 1.0f32.max(f);
                params.inner_col = inner_color.premultiplied().to_array();
                params.outer_col = outer_color.premultiplied().to_array();
                params.shader_type = ShaderType::FillGradient.to_f32();
            }
        }

        params.paint_mat = inv_transform.to_mat3x4();

        params
    }

}
