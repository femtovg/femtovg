
use std::ffi::c_void;

use image::DynamicImage;

use crate::{ImageFlags, Vertex, Paint, Scissor, Path, Color, LineJoin, FillRule};
use crate::renderer::{ImageId, Renderer};
use crate::paint::PaintFlavor;
use crate::geometry::Transform2D;

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
pub enum CommandFlavor {
    ConvexFill {
        gpu_paint: GpuPaint
    },
    ConcaveFill {
        stencil_paint: GpuPaint,
        fill_paint: GpuPaint,
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
    flavor: CommandFlavor,
    drawables: Vec<Drawable>,
    triangles_verts: Option<(usize, usize)>,
    image: Option<ImageId>,
    fill_rule: FillRule,
    transform: Transform2D,
}

impl Command {
    pub fn new(flavor: CommandFlavor) -> Self {
        Self {
            flavor: flavor,
            drawables: Default::default(),
            triangles_verts: Default::default(),
            image: Default::default(),
            fill_rule: Default::default(),
            transform: Default::default()
        }
    }
}

pub struct GpuRenderer<T> {
    backend: T,
    cmds: Vec<Command>,
    verts: Vec<Vertex>,
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

    fn fill(&mut self, path: &Path, paint: &Paint, scissor: &Scissor, transform: &Transform2D) {
        let mut gpu_path = GpuPath::new(path, transform, self.tess_tol, self.dist_tol);

        if paint.shape_anti_alias() {
            gpu_path.expand_fill(self.fringe_width, LineJoin::Miter, 2.4, self.fringe_width);
        } else {
            gpu_path.expand_fill(0.0, LineJoin::Miter, 2.4, self.fringe_width);
        }

        let flavor = if gpu_path.contours.len() == 1 && gpu_path.contours[0].convexity == Convexity::Convex {
            let gpu_paint = GpuPaint::new(&self.backend, paint, scissor, self.fringe_width, self.fringe_width, -1.0);

            CommandFlavor::ConvexFill { gpu_paint }
        } else {
            let mut stencil_paint = GpuPaint::default();
            stencil_paint.stroke_thr = -1.0;
            stencil_paint.shader_type = ShaderType::Stencil.to_f32();

            let fill_paint = GpuPaint::new(&self.backend, paint, scissor, self.fringe_width, self.fringe_width, -1.0);

            CommandFlavor::ConcaveFill { stencil_paint, fill_paint }
        };

        let mut cmd = Command::new(flavor);
        cmd.transform = *transform;
        cmd.fill_rule = paint.fill_rule;

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

        if let CommandFlavor::ConcaveFill {..} = cmd.flavor {
            // Quad
            self.verts.push(Vertex::new(gpu_path.bounds.maxx, gpu_path.bounds.maxy, 0.5, 1.0));
            self.verts.push(Vertex::new(gpu_path.bounds.maxx, gpu_path.bounds.miny, 0.5, 1.0));
            self.verts.push(Vertex::new(gpu_path.bounds.minx, gpu_path.bounds.maxy, 0.5, 1.0));
            self.verts.push(Vertex::new(gpu_path.bounds.minx, gpu_path.bounds.miny, 0.5, 1.0));

            cmd.triangles_verts = Some((offset, 4));
        }

        self.cmds.push(cmd);
    }

    fn stroke(&mut self, path: &Path, paint: &Paint, scissor: &Scissor, transform: &Transform2D) {
        let mut gpu_path = GpuPath::new(path, transform, self.tess_tol, self.dist_tol);

        if paint.shape_anti_alias() {
            gpu_path.expand_stroke(paint.stroke_width() * 0.5, self.fringe_width, paint.line_cap(), paint.line_join(), paint.miter_limit(), self.tess_tol);
        } else {
            gpu_path.expand_stroke(paint.stroke_width() * 0.5, 0.0, paint.line_cap(), paint.line_join(), paint.miter_limit(), self.tess_tol);
        }

        let gpu_paint = GpuPaint::new(&self.backend, paint, scissor, paint.stroke_width(), self.fringe_width, -1.0);

        let flavor = if paint.stencil_strokes() {
            let paint2 = GpuPaint::new(&self.backend, paint, scissor, paint.stroke_width(), self.fringe_width, 1.0 - 0.5/255.0);

            CommandFlavor::StencilStroke { paint1: gpu_paint, paint2 }
        } else {
            CommandFlavor::Stroke { gpu_paint }
        };

        let mut cmd = Command::new(flavor);
        cmd.transform = *transform;

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

    fn triangles(&mut self, verts: &[Vertex], paint: &Paint, scissor: &Scissor, transform: &Transform2D) {
        let mut gpu_paint = GpuPaint::new(&self.backend, paint, scissor, 1.0, 1.0, -1.0);
        gpu_paint.shader_type = ShaderType::Img.to_f32();

        let mut cmd = Command::new(CommandFlavor::Triangles { gpu_paint });
        cmd.transform = *transform;

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
    Stencil,
    Img
}

impl Default for ShaderType {
    fn default() -> Self { Self::FillGradient }
}

impl ShaderType {
    pub fn to_f32(self) -> f32 {
        match self {
            Self::FillGradient => 0.0,
            Self::FillImage => 1.0,
            Self::Stencil => 2.0,
            Self::Img => 3.0,
        }
    }
}
