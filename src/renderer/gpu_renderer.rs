
use std::ffi::c_void;

use image::DynamicImage;

use crate::math::*;
use crate::{ImageFlags, Vertex, Paint, Scissor, Verb, Color, LineJoin};
use crate::renderer::{ImageId, Renderer};

mod gpu_path;
use gpu_path::{Convexity, GpuPath};

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
    Rgba,
    Alpha
}

#[derive(Debug)]
pub enum Flavor {
    ConvexFill {
        params: Params
    },
    ConcaveFill {
        fill_params: Params,
        stroke_params: Params,
    },
    Stroke {
        params: Params
    },
    StencilStroke {
        pass1: Params,
        pass2: Params
    },
    Triangles {
        params: Params
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
    fringe_width: f32,
    current_path: Option<GpuPath>
}

impl<T: GpuRendererBackend> GpuRenderer<T> {
    pub fn new(backend: T) -> Self {
        Self {
            backend: backend,
            cmds: Default::default(),
            verts: Default::default(),
            fringe_width: 1.0,
            current_path: None
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
        // TODO: use dpi to calculate fringe_width, tes_tol and dist_tol
        self.backend.set_size(width, height, dpi);
    }

    fn set_current_path(&mut self, verbs: &[Verb]) {
        if self.current_path.is_none() {
            // TODO: don't hardcode tes_tol and dist_tol here
            self.current_path = Some(GpuPath::new(verbs, 0.25, 0.01));
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
            let params = Params::new(&self.backend, paint, scissor, self.fringe_width, self.fringe_width, -1.0);

            Flavor::ConvexFill { params }
        } else {
            let mut fill_params = Params::default();
            fill_params.stroke_thr = -1.0;
            fill_params.shader_type = ShaderType::Simple.to_i32() as f32;//TODO to_f32 method

            let stroke_params = Params::new(&self.backend, paint, scissor, self.fringe_width, self.fringe_width, -1.0);

            Flavor::ConcaveFill { fill_params, stroke_params }
        };

        let mut cmd = Command::new(flavor);
        cmd.image = paint.image();

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
            self.verts.push(Vertex::new(gpu_path.bounds.max.x, gpu_path.bounds.max.y, 0.5, 1.0));
            self.verts.push(Vertex::new(gpu_path.bounds.max.x, gpu_path.bounds.min.y, 0.5, 1.0));
            self.verts.push(Vertex::new(gpu_path.bounds.min.x, gpu_path.bounds.max.y, 0.5, 1.0));
            self.verts.push(Vertex::new(gpu_path.bounds.min.x, gpu_path.bounds.min.y, 0.5, 1.0));

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

        let params = Params::new(&self.backend, paint, scissor, paint.stroke_width(), self.fringe_width, -1.0);

        let flavor = if paint.stencil_strokes() {
            let pass2 = Params::new(&self.backend, paint, scissor, paint.stroke_width(), self.fringe_width, 1.0 - 0.5/255.0);

            Flavor::StencilStroke { pass1: params, pass2 }
        } else {
            Flavor::Stroke { params }
        };

        let mut cmd = Command::new(flavor);
        cmd.image = paint.image();

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
        let mut params = Params::new(&self.backend, paint, scissor, 1.0, 1.0, -1.0);
        params.shader_type = ShaderType::Img.to_i32() as f32; // TODO:

        let mut cmd = Command::new(Flavor::Triangles { params });
        cmd.image = paint.image();
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

impl Params {

    fn new<T: GpuRendererBackend>(backend: &T, paint: &Paint, scissor: &Scissor, width: f32, fringe: f32, stroke_thr: f32) -> Self {

        let mut params = Self::default();

        params.inner_col = paint.inner_color().premultiplied().to_array();
        params.outer_col = paint.outer_color().premultiplied().to_array();

        let (scissor_ext, scissor_scale) = if scissor.extent[0] < -0.5 || scissor.extent[1] < -0.5 {
            ([1.0, 1.0], [1.0, 1.0])
        } else {
            if let Some(inv) = scissor.transform.inverse() {
                params.scissor_mat = inv.to_mat3x4();
            }


            let scissor_scale = [
                (scissor.transform.m11*scissor.transform.m11 + scissor.transform.m21*scissor.transform.m21).sqrt() / fringe,
                (scissor.transform.m12*scissor.transform.m12 + scissor.transform.m22*scissor.transform.m22).sqrt() / fringe
            ];

            (scissor.extent, scissor_scale)
        };

        params.scissor_ext = scissor_ext;
        params.scissor_scale = scissor_scale;

        let extent = paint.extent();

        params.extent = extent;
        params.stroke_mult = (width*0.5 + fringe*0.5) / fringe;
        params.stroke_thr = stroke_thr;

        let inv_transform;

        if let Some(image_id) = paint.image() {

            let texture_flags = backend.texture_flags(image_id);

            if texture_flags.contains(ImageFlags::FLIP_Y) {
                // TODO: Test this
                let mut m1 = Transform2D::create_translation(0.0, extent[1] * 0.5).post_transform(&paint.transform());

                let m2 = Transform2D::create_scale(1.0, -1.0).post_transform(&m1);

                m1 = m1.post_translate(Vector2D::new(0.0, -extent[1] * 0.5)).post_transform(&m2);

                inv_transform = m1.inverse();
            } else {
                inv_transform = paint.transform().inverse();
            }

            params.shader_type = ShaderType::FillImage.to_i32() as f32;// TODO: To f32 native method

            params.tex_type = match backend.texture_type(image_id) {
                Some(TextureType::Rgba) => if texture_flags.contains(ImageFlags::PREMULTIPLIED) { 0.0 } else { 1.0 },
                Some(TextureType::Alpha) => 2.0,
                _ => 0.0
            };
        } else {
            params.shader_type = ShaderType::FillGradient.to_i32() as f32;// TODO: To f32 native method
            params.radius = paint.radius();
            params.feather = paint.feather();

            inv_transform = paint.transform().inverse();
        }

        if let Some(inv) = inv_transform {
            params.paint_mat = inv.to_mat3x4();
        }

        params
    }

}
