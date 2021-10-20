use crate::{
    paint::{GlyphTexture, GradientColors},
    Color, ImageFlags, ImageStore, Paint, PaintFlavor, PixelFormat, Scissor, Transform2D,
};

use super::ShaderType;

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
    pub(crate) glyph_texture_type: f32, // 0 -> no glyph rendering, 1 -> alpha mask, 2 -> color texture
    pub(crate) image_blur_filter_direction: [f32; 2],
    pub(crate) image_blur_filter_sigma: f32,
    pub(crate) image_blur_filter_coeff: [f32; 3],
}

impl Params {
    pub(crate) fn new<T>(
        images: &ImageStore<T>,
        paint: &Paint,
        scissor: &Scissor,
        stroke_width: f32,
        fringe_width: f32,
        stroke_thr: f32,
    ) -> Self {
        let mut params = Params::default();

        // Scissor
        let (scissor_ext, scissor_scale) = if let Some(ext) = scissor.extent {
            if ext[0] < -0.5 || ext[1] < -0.5 {
                ([1.0, 1.0], [1.0, 1.0])
            } else {
                params.scissor_mat = scissor.transform.inversed().to_mat3x4();

                let scissor_scale = [
                    (scissor.transform[0] * scissor.transform[0] + scissor.transform[2] * scissor.transform[2]).sqrt()
                        / fringe_width,
                    (scissor.transform[1] * scissor.transform[1] + scissor.transform[3] * scissor.transform[3]).sqrt()
                        / fringe_width,
                ];

                (ext, scissor_scale)
            }
        } else {
            ([1.0, 1.0], [1.0, 1.0])
        };

        params.scissor_ext = scissor_ext;
        params.scissor_scale = scissor_scale;

        params.stroke_mult = (stroke_width * 0.5 + fringe_width * 0.5) / fringe_width;
        params.stroke_thr = stroke_thr;

        params.glyph_texture_type = match paint.glyph_texture() {
            GlyphTexture::None => 0.0,
            GlyphTexture::AlphaMask(_) => 1.0,
            GlyphTexture::ColorTexture(_) => 2.0,
        };

        let inv_transform;

        match paint.flavor {
            PaintFlavor::Color(color) => {
                let color = color.premultiplied().to_array();
                params.inner_col = color;
                params.outer_col = color;
                params.shader_type = ShaderType::FillGradient.to_f32();
                inv_transform = paint.transform.inversed();
            }
            PaintFlavor::Image {
                id,
                cx,
                cy,
                width,
                height,
                angle,
                alpha,
            } => {
                let image_info = match images.info(id) {
                    Some(info) => info,
                    None => return params,
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

                if image_info.flags().contains(ImageFlags::FLIP_Y) {
                    let mut m1 = Transform2D::identity();
                    m1.translate(0.0, height * 0.5);
                    m1.multiply(&transform);

                    let mut m2 = Transform2D::identity();
                    m2.scale(1.0, -1.0);
                    m2.multiply(&m1);

                    let mut m1 = Transform2D::identity();
                    m1.translate(0.0, -height * 0.5);
                    m1.multiply(&m2);

                    inv_transform = m1.inversed();
                } else {
                    inv_transform = transform.inversed();
                }

                params.shader_type = ShaderType::FillImage.to_f32();

                params.tex_type = match image_info.format() {
                    PixelFormat::Rgba8 => {
                        if image_info.flags().contains(ImageFlags::PREMULTIPLIED) {
                            0.0
                        } else {
                            1.0
                        }
                    }
                    PixelFormat::Gray8 => 2.0,
                    _ => 0.0,
                };
            }
            PaintFlavor::LinearGradient {
                start_x,
                start_y,
                end_x,
                end_y,
                colors,
            } => {
                let large = 1e5f32;
                let mut dx = end_x - start_x;
                let mut dy = end_y - start_y;
                let d = (dx * dx + dy * dy).sqrt();

                if d > 0.0001 {
                    dx /= d;
                    dy /= d;
                } else {
                    dx = 0.0;
                    dy = 1.0;
                }

                let mut transform = Transform2D([dy, -dx, dx, dy, start_x - dx * large, start_y - dy * large]);

                transform.multiply(&paint.transform);

                inv_transform = transform.inversed();

                params.extent[0] = large;
                params.extent[1] = large + d * 0.5;
                params.feather = 1.0f32.max(d);

                match colors {
                    GradientColors::TwoStop { start_color, end_color } => {
                        params.inner_col = start_color.premultiplied().to_array();
                        params.outer_col = end_color.premultiplied().to_array();
                        params.shader_type = ShaderType::FillGradient.to_f32();
                    }
                    GradientColors::MultiStop { .. } => {
                        params.shader_type = ShaderType::FillImageGradient.to_f32();
                    }
                }
            }
            PaintFlavor::BoxGradient {
                x,
                y,
                width,
                height,
                radius,
                feather,
                colors,
            } => {
                let mut transform = Transform2D::new_translation(x + width * 0.5, y + height * 0.5);
                transform.multiply(&paint.transform);
                inv_transform = transform.inversed();

                params.extent[0] = width * 0.5;
                params.extent[1] = height * 0.5;
                params.radius = radius;
                params.feather = feather;
                match colors {
                    GradientColors::TwoStop { start_color, end_color } => {
                        params.inner_col = start_color.premultiplied().to_array();
                        params.outer_col = end_color.premultiplied().to_array();
                        params.shader_type = ShaderType::FillGradient.to_f32();
                    }
                    GradientColors::MultiStop { .. } => {
                        params.shader_type = ShaderType::FillImageGradient.to_f32();
                    }
                }
            }
            PaintFlavor::RadialGradient {
                cx,
                cy,
                in_radius,
                out_radius,
                colors,
            } => {
                let r = (in_radius + out_radius) * 0.5;
                let f = out_radius - in_radius;

                let mut transform = Transform2D::new_translation(cx, cy);
                transform.multiply(&paint.transform);
                inv_transform = transform.inversed();

                params.extent[0] = r;
                params.extent[1] = r;
                params.radius = r;
                params.feather = 1.0f32.max(f);
                match colors {
                    GradientColors::TwoStop { start_color, end_color } => {
                        params.inner_col = start_color.premultiplied().to_array();
                        params.outer_col = end_color.premultiplied().to_array();
                        params.shader_type = ShaderType::FillGradient.to_f32();
                    }
                    GradientColors::MultiStop { .. } => {
                        params.shader_type = ShaderType::FillImageGradient.to_f32();
                    }
                }
            }
        }

        params.paint_mat = inv_transform.to_mat3x4();

        params
    }
}
