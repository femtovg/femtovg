
use std::ffi::c_void;

use image::DynamicImage;

use crate::geometry::*;
use crate::{ImageFlags, Vertex, Paint, Scissor, Verb, Color, LineJoin};
use crate::renderer::{ImageId, Renderer};
use crate::paint::PaintFlavor;

mod gpu_path;
use gpu_path::{Convexity, GpuPath};

mod gpu_paint;
use gpu_paint::GpuPaint;

mod opengl;
pub use opengl::OpenGl;

pub trait GpuRendererBackend {
    fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color);
    fn set_size(&mut self, width: u32, height: u32, dpi: f32);

    fn render(&mut self, verts: &[Vertex], commands: &[Command]);

    fn create_image(&mut self, image: &DynamicImage, flags: ImageFlags) -> ImageId;
    fn update_image(&mut self, id: ImageId, image: &DynamicImage, x: u32, y: u32);
    fn delete_image(&mut self, id: ImageId);

    fn texture_flags(&self, id: ImageId) -> ImageFlags;
    fn texture_size(&self, id: ImageId) -> (u32, u32);
    fn texture_type(&self, id: ImageId) -> Option<TextureType>;

    fn screenshot(&mut self) -> DynamicImage;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TextureType {
    Rgb,
    Rgba,
    Alpha
}

#[derive(Debug)]
pub enum Flavor {
    ConvexFill {
        gpu_paint: GpuPaint
    },
    ConcaveFill {
        fill_paint: GpuPaint,
        stroke_paint: GpuPaint,
    },
    Stroke {
        gpu_paint: GpuPaint
    },
    StencilStroke {
        paint1: GpuPaint,
        paint2: GpuPaint
    },
    Triangles {
        gpu_paint: GpuPaint
    },
}

#[derive(Copy, Clone, Default)]
pub struct Drawable {
    fill_verts: Option<(usize, usize)>,
    stroke_verts: Option<(usize, usize)>,
}

pub struct Command {
    flavor: Flavor,
    drawables: Vec<Drawable>,
    triangles_verts: Option<(usize, usize)>,
    image: Option<ImageId>,
}

impl Command {
    pub fn new(flavor: Flavor) -> Self {
        Self {
            flavor: flavor,
            drawables: Default::default(),
            triangles_verts: Default::default(),
            image: Default::default(),
        }
    }
}

pub struct GpuRenderer<T> {
    backend: T,
    cmds: Vec<Command>,
    verts: Vec<Vertex>,
    current_path: Option<GpuPath>,
    tess_tol: f32,
    dist_tol: f32,
    fringe_width: f32,
}

impl<T: GpuRendererBackend> GpuRenderer<T> {
    pub fn new(backend: T) -> Self {
        Self {
            backend: backend,
            cmds: Default::default(),
            verts: Default::default(),
            current_path: None,
            tess_tol: 0.25,
            dist_tol: 0.01,
            fringe_width: 1.0,
        }
    }
}

impl GpuRenderer<OpenGl> {
    pub fn with_gl<F>(load_fn: F) -> Self where F: Fn(&'static str) -> *const c_void {
        Self::new(OpenGl::new(load_fn).expect("Cannot create opengl backend"))
    }
}

impl<T: GpuRendererBackend> Renderer for GpuRenderer<T> {
    fn flush(&mut self) {
        self.backend.render(&self.verts, &self.cmds);
        self.cmds.clear();
        self.verts.clear();
    }

    fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        self.backend.clear_rect(x, y, width, height, color);
    }

    fn set_size(&mut self, width: u32, height: u32, dpi: f32) {
        self.tess_tol = 0.25 / dpi;
        self.dist_tol = 0.01 / dpi;
        self.fringe_width = 1.0 / dpi;

        self.backend.set_size(width, height, dpi);
    }

    fn set_current_path(&mut self, verbs: &[Verb]) {
        if self.current_path.is_none() {
            self.current_path = Some(GpuPath::new(verbs, self.tess_tol, self.dist_tol));
        }
    }

    fn clear_current_path(&mut self) {
        self.current_path = None;
    }

    fn fill(&mut self, paint: &Paint, scissor: &Scissor) {
        let gpu_path = if let Some(gpu_path) = self.current_path.as_mut() {
            gpu_path
        } else {
            return;
        };

        if paint.shape_anti_alias() {
            gpu_path.expand_fill(self.fringe_width, LineJoin::Miter, 2.4, self.fringe_width);
        } else {
            gpu_path.expand_fill(0.0, LineJoin::Miter, 2.4, self.fringe_width);
        }

        let flavor = if gpu_path.contours.len() == 1 && gpu_path.contours[0].convexity == Convexity::Convex {
            let gpu_paint = GpuPaint::new(&self.backend, paint, scissor, self.fringe_width, self.fringe_width, -1.0);

            Flavor::ConvexFill { gpu_paint }
        } else {
            let mut fill_paint = GpuPaint::default();
            fill_paint.stroke_thr = -1.0;
            fill_paint.shader_type = ShaderType::Simple.to_i32() as f32;//TODO to_f32 method

            let stroke_paint = GpuPaint::new(&self.backend, paint, scissor, self.fringe_width, self.fringe_width, -1.0);

            Flavor::ConcaveFill { fill_paint, stroke_paint }
        };

        let mut cmd = Command::new(flavor);

        if let PaintFlavor::Image { id, .. } = paint.flavor {
            cmd.image = Some(id);
        }

        let mut offset = self.verts.len();

        for contour in &gpu_path.contours {
            let mut drawable = Drawable::default();

            if !contour.fill.is_empty() {
                drawable.fill_verts = Some((offset, contour.fill.len()));
                self.verts.extend_from_slice(&contour.fill);
                offset += contour.fill.len();
            }

            if !contour.stroke.is_empty() {
                drawable.stroke_verts = Some((offset, contour.stroke.len()));
                self.verts.extend_from_slice(&contour.stroke);
                offset += contour.stroke.len();
            }

            cmd.drawables.push(drawable);
        }

        if let Flavor::ConcaveFill {..} = cmd.flavor {
            // Quad
            self.verts.push(Vertex::new(gpu_path.bounds.maxx, gpu_path.bounds.maxy, 0.5, 1.0));
            self.verts.push(Vertex::new(gpu_path.bounds.maxx, gpu_path.bounds.miny, 0.5, 1.0));
            self.verts.push(Vertex::new(gpu_path.bounds.minx, gpu_path.bounds.maxy, 0.5, 1.0));
            self.verts.push(Vertex::new(gpu_path.bounds.minx, gpu_path.bounds.miny, 0.5, 1.0));

            cmd.triangles_verts = Some((offset, 4));
        }

        self.cmds.push(cmd);
    }

    fn stroke(&mut self, paint: &Paint, scissor: &Scissor) {
        let tess_tol = 0.25;

        let gpu_path = if let Some(gpu_path) = self.current_path.as_mut() {
            gpu_path
        } else {
            return;
        };

        if paint.shape_anti_alias() {
            gpu_path.expand_stroke(paint.stroke_width() * 0.5, self.fringe_width, paint.line_cap(), paint.line_join(), paint.miter_limit(), tess_tol);
        } else {
            gpu_path.expand_stroke(paint.stroke_width() * 0.5, 0.0, paint.line_cap(), paint.line_join(), paint.miter_limit(), tess_tol);
        }

        let gpu_paint = GpuPaint::new(&self.backend, paint, scissor, paint.stroke_width(), self.fringe_width, -1.0);

        let flavor = if paint.stencil_strokes() {
            let paint2 = GpuPaint::new(&self.backend, paint, scissor, paint.stroke_width(), self.fringe_width, 1.0 - 0.5/255.0);

            Flavor::StencilStroke { paint1: gpu_paint, paint2 }
        } else {
            Flavor::Stroke { gpu_paint }
        };

        let mut cmd = Command::new(flavor);

        if let PaintFlavor::Image { id, .. } = paint.flavor {
            cmd.image = Some(id);
        }

        let mut offset = self.verts.len();

        for contour in &gpu_path.contours {
            let mut drawable = Drawable::default();

            if !contour.stroke.is_empty() {
                drawable.stroke_verts = Some((offset, contour.stroke.len()));
                self.verts.extend_from_slice(&contour.stroke);
                offset += contour.stroke.len();
            }

            cmd.drawables.push(drawable);
        }

        self.cmds.push(cmd);
    }

    fn triangles(&mut self, paint: &Paint, scissor: &Scissor, verts: &[Vertex]) {
        let mut gpu_paint = GpuPaint::new(&self.backend, paint, scissor, 1.0, 1.0, -1.0);
        gpu_paint.shader_type = ShaderType::Img.to_f32();

        let mut cmd = Command::new(Flavor::Triangles { gpu_paint });

        if let PaintFlavor::Image { id, .. } = paint.flavor {
            cmd.image = Some(id);
        }

        cmd.triangles_verts = Some((self.verts.len(), verts.len()));
        self.cmds.push(cmd);

        self.verts.extend_from_slice(verts);
    }

    fn create_image(&mut self, image: &DynamicImage, flags: ImageFlags) -> ImageId {
        self.backend.create_image(image, flags)
    }

    fn update_image(&mut self, id: ImageId, image: &DynamicImage, x: u32, y: u32) {
        self.backend.update_image(id, image, x, y);
    }

    fn delete_image(&mut self, id: ImageId) {
        self.backend.delete_image(id);
    }

    fn screenshot(&mut self) -> Option<DynamicImage> {
        Some(self.backend.screenshot())
    }
}

// TODO: Rename those to make more sense - why do we have FillImage and Img?
#[derive(Copy, Clone)]
enum ShaderType {
    FillGradient,
    FillImage,
    Simple,
    Img
}

impl Default for ShaderType {
    fn default() -> Self { Self::Simple }
}

impl ShaderType {
    pub fn to_i32(self) -> i32 {
        match self {
            Self::FillGradient => 0,
            Self::FillImage => 1,
            Self::Simple => 2,
            Self::Img => 3,
        }
    }

    pub fn to_f32(self) -> f32 {
        match self {
            Self::FillGradient => 0.0,
            Self::FillImage => 1.0,
            Self::Simple => 2.0,
            Self::Img => 3.0,
        }
    }
}


#[derive(Copy, Clone, Default, Debug)]
pub struct Params {
    scissor_mat: [f32; 12],
    paint_mat: [f32; 12],
    inner_col: [f32; 4],
    outer_col: [f32; 4],
    scissor_ext: [f32; 2],
    scissor_scale: [f32; 2],
    extent: [f32; 2],
    radius: f32,
    feather: f32,
    stroke_mult: f32,
    stroke_thr: f32,
    shader_type: f32,
    tex_type: f32
}
/*
impl Params {

    fn new<T: GpuRendererBackend>(backend: &T, paint: &Paint, scissor: &Scissor, width: f32, fringe: f32, stroke_thr: f32) -> Self {

        let mut params = Self::default();

        params.inner_col = paint.inner_color.premultiplied().to_array();
        params.outer_col = paint.outer_color.premultiplied().to_array();

        let (scissor_ext, scissor_scale) = if let Some(ext) = scissor.extent {
            if ext[0] < -0.5 || ext[1] < -0.5 {
                ([1.0, 1.0], [1.0, 1.0])
            } else {
                params.scissor_mat = scissor.transform.inversed().to_mat3x4();

                let scissor_scale = [
                    (scissor.transform[0]*scissor.transform[0] + scissor.transform[2]*scissor.transform[2]).sqrt() / fringe,
                    (scissor.transform[1]*scissor.transform[1] + scissor.transform[3]*scissor.transform[3]).sqrt() / fringe
                ];

                (ext, scissor_scale)
            }
        } else {
            ([1.0, 1.0], [1.0, 1.0])
        };

        params.scissor_ext = scissor_ext;
        params.scissor_scale = scissor_scale;

        let extent = paint.extent;

        params.extent = extent;
        params.stroke_mult = (width*0.5 + fringe*0.5) / fringe;
        params.stroke_thr = stroke_thr;

        let inv_transform;

        if let Some(image_id) = paint.image {
            let texture_flags = backend.texture_flags(image_id);

            if texture_flags.contains(ImageFlags::FLIP_Y) {
                let mut m1 = Transform2D::identity();
                m1.translate(0.0, extent[1] * 0.5);
                m1.multiply(&paint.transform);

                let mut m2 = Transform2D::identity();
                m2.scale(1.0, -1.0);
                m2.multiply(&m1);

                m1.translate(0.0, -extent[1] * 0.5);
                m1.multiply(&m2);

                inv_transform = m1.inversed();
            } else {
                inv_transform = paint.transform.inversed();
            }

            params.shader_type = ShaderType::FillImage.to_i32() as f32;// TODO: To f32 native method

            params.tex_type = match backend.texture_type(image_id) {
                Some(TextureType::Rgba) => if texture_flags.contains(ImageFlags::PREMULTIPLIED) { 0.0 } else { 1.0 },
                Some(TextureType::Alpha) => 2.0,
                _ => 0.0
            };
        } else {
            params.shader_type = ShaderType::FillGradient.to_i32() as f32;// TODO: To f32 native method
            params.radius = paint.radius;
            params.feather = paint.feather;

            inv_transform = paint.transform.inversed();
        }

        params.paint_mat = inv_transform.to_mat3x4();

        params
    }

}*/
