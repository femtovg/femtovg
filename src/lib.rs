
use std::f32::consts::PI;
use std::path::Path as FilePath;
use std::{error::Error, fmt};

use image::{DynamicImage, GenericImageView};
use bitflags::bitflags;

mod color;
pub use color::Color;

mod atlas;
pub use atlas::Atlas;

pub mod renderer;
use renderer::{Renderer, TextureType};

pub mod font_manager;
pub use font_manager::{FontManager, FontStyle, FontManagerError};

pub mod math;
use crate::math::*;

mod paint;
pub use paint::Paint;

// TODO: Use Convexity enum to describe path concave/convex
// TODO: Replace pt_equals with method on point
// TODO: Rename tess_tol and dist_tol to tesselation_tolerance and distance_tolerance
// TODO: Drawing works before the call to begin frame for some reason
// TODO: rethink image creation and resource creation in general, it's currently blocking, 
//		 it would be awesome if its non-blocking and maybe async. Or maybe resource creation
//		 should be a functionality provided by the current renderer implementation, not by the canvas itself.
// TODO: A lot of the render styles can be moved to the Paint object - stroke width, line join and cap, basically a lot of the state object
// TODO: Instead of path cache filles with paths, use path filled with contours -> https://skia.org/user/api/SkPath_Overview

const KAPPA90: f32 = 0.5522847493; // Length proportional to radius of a cubic bezier handle for 90deg arcs.

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VAlign {
	Top,
	Middle,
	Bottom,
	Baseline
}

impl Default for VAlign {
	fn default() -> Self {
		Self::Baseline
	}
}

// Point flags
const POINT_CORNER: u8 = 0x01;
const POINT_LEFT: u8 = 0x02;
const POINT_BEVEL: u8 = 0x04;
const POINT_INNERBEVEL: u8 = 0x08;

// Image flags
bitflags! {
    pub struct ImageFlags: u32 {
        const GENERATE_MIPMAPS = 1 << 0;// Generate mipmaps during creation of the image.
        const REPEAT_X = 1 << 1;		// Repeat image in X direction.
        const REPEAT_Y = 1 << 2;		// Repeat image in Y direction.
        const FLIP_Y = 1 << 3;			// Flips (inverses) image in Y direction when rendered.
        const PREMULTIPLIED = 1 << 4;	// Image data has premultiplied alpha.
        const NEAREST = 1 << 5;			// Image interpolation is Nearest instead Linear
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub struct Scissor {
	transform: Transform2D,
    extent: [f32; 2],
}

impl Default for Scissor {
    fn default() -> Self {
        Self {
            transform: Default::default(),
            extent: [-1.0, -1.0]// TODO: Use Option instead of relying on -1s
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub enum Winding {
    CCW = 1, 
    CW = 2
}

impl Default for Winding {
	fn default() -> Self {
		Winding::CCW
	}
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

impl Default for LineCap {
    fn default() -> Self {
        Self::Butt
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel
}

impl Default for LineJoin {
    fn default() -> Self {
        Self::Miter
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ImageId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
enum Command {
	MoveTo(f32, f32),
	LineTo(f32, f32),
	BezierTo(f32, f32, f32, f32, f32, f32),
	Close,
	Winding(Winding)
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Default)]
#[repr(C)]
pub struct Vertex {
	x: f32,
	y: f32,
	u: f32,
	v: f32
}

impl Vertex {
	pub fn new(x: f32, y: f32, u: f32, v: f32) -> Self {
		Self { x, y, u, v }
	}
	
	pub fn set(&mut self, x: f32, y: f32, u: f32, v: f32) {
		*self = Self { x, y, u, v };
	}
}

#[derive(Copy, Clone, Debug, Default)]
struct Point {
	x: f32,
	y: f32,
	dx: f32,
	dy: f32,
	len: f32,
	dmx: f32,
	dmy: f32,
	flags: u8// TODO: Use bitflags crate for this
}

// TODO: We need an iterator for the path points that loops by chunks of 2

#[derive(Default, Debug)]
pub struct Path {
	first: usize,
	count: usize,
	closed: bool,
	bevel: usize,
	fill: Vec<Vertex>,
	stroke: Vec<Vertex>,
	winding: Winding,
	convex: bool
}

#[derive(Default)]
struct PathCache {
	points: Vec<Point>,
	paths: Vec<Path>,
	bounds: [f32; 4],
}

impl PathCache {
	pub fn clear(&mut self) {
		self.points.clear();
		self.paths.clear();
	}
	
	fn add_path(&mut self) {
		let mut path = Path::default();
		
		path.first = self.points.len();
		
		self.paths.push(path);
	}
	
	fn last_path(&mut self) -> Option<&mut Path> {
		self.paths.last_mut()
	}
	
	// TODO: Revise if this needs to return &mut or just Point
	fn last_point(&mut self) -> Option<Point> {
		self.points.last_mut().copied()
	}
	
	fn add_point(&mut self, x: f32, y: f32, flags: u8, dist_tol: f32) {
		if self.paths.len() == 0 { return }
		
		let count = &mut self.paths.last_mut().unwrap().count;
		
		if *count > 0 {
			if let Some(point) = self.points.last_mut() {
				if pt_equals(point.x, point.y, x, y, dist_tol) {
					point.flags |= flags;
					return;
				}
			}
		}
		
		let mut point = Point::default();
		point.x = x;
		point.y = y;
		point.flags = flags;
		
		self.points.push(point);
		*count += 1;
	}
	
	fn tesselate_bezier(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32, x4: f32, y4: f32, level: usize, atype: u8, tess_tol: f32, dist_tol: f32) {
		if level > 10 { return; }

		let x12 = (x1+x2)*0.5;
		let y12 = (y1+y2)*0.5;
		let x23 = (x2+x3)*0.5;
		let y23 = (y2+y3)*0.5;
		let x34 = (x3+x4)*0.5;
		let y34 = (y3+y4)*0.5;
		let x123 = (x12+x23)*0.5;
		let y123 = (y12+y23)*0.5;

		let dx = x4 - x1;
		let dy = y4 - y1;
		let d2 = (((x2 - x4) * dy - (y2 - y4) * dx)).abs();
		let d3 = (((x3 - x4) * dy - (y3 - y4) * dx)).abs();

		if (d2 + d3)*(d2 + d3) < tess_tol * (dx*dx + dy*dy) {
			self.add_point(x4, y4, atype, dist_tol);
			return;
		}

		let x234 = (x23+x34)*0.5;
		let y234 = (y23+y34)*0.5;
		let x1234 = (x123+x234)*0.5;
		let y1234 = (y123+y234)*0.5;

		self.tesselate_bezier(x1,y1, x12,y12, x123,y123, x1234,y1234, level+1, 0, tess_tol, dist_tol);
		self.tesselate_bezier(x1234,y1234, x234,y234, x34,y34, x4,y4, level+1, atype, tess_tol, dist_tol);
	}
}

#[derive(Copy, Clone)]
struct State {
	transform: Transform2D,
    scissor: Scissor,
	miter_limit: f32,
	alpha: f32,
}

impl Default for State {
	fn default() -> Self {
		Self {
			transform: Transform2D::identity(),
            scissor: Default::default(),
			miter_limit: 10.0,
			alpha: 1.0,
		}
	}
}

pub struct Canvas {
    renderer: Box<dyn Renderer>,
    font_manager: FontManager,
    state_stack: Vec<State>,
    tess_tol: f32,
	dist_tol: f32,
	fringe_width: f32,
	device_px_ratio: f32,
	commands: Vec<Command>,
	cache: PathCache,
	commandx: f32,
    commandy: f32,
}

impl Canvas {
    
    pub fn new<R: Renderer + 'static>(renderer: R) -> Self {
		
		// TODO: Return result from this method instead of unwrapping
		let font_manager = FontManager::new().unwrap();
		
        let mut canvas = Self {
            renderer: Box::new(renderer),
            font_manager: font_manager,
            state_stack: Default::default(),
            tess_tol: Default::default(),
			dist_tol: Default::default(),
			fringe_width: Default::default(),
			device_px_ratio: Default::default(),
			commands: Default::default(),
			cache: PathCache::default(),
			commandx: Default::default(),
			commandy: Default::default(),
        };
        
        canvas.save();
        canvas.reset();
        
        canvas.set_device_pixel_ratio(1.0);
        
        canvas
    }
    
    pub fn begin_frame(&mut self, window_width: f32, window_height: f32, device_px_ratio: f32) {
		self.state_stack.clear();
		self.save();
		
		self.commands.clear();
        self.cache.clear();
		
		self.set_device_pixel_ratio(device_px_ratio);
		
		self.renderer.render_viewport(window_width, window_height);
	}
	
	pub fn end_frame(&mut self) {
		self.renderer.render_flush();
	}
	
	// State Handling
    
    /// Pushes and saves the current render state into a state stack.
    /// 
    /// A matching restore() must be used to restore the state.
    pub fn save(&mut self) {
		let state = self.state_stack.last().map_or_else(State::default, |state| *state);
		
		self.state_stack.push(state);
	}
	
	/// Restores the previous render state
    pub fn restore(&mut self) {
		if self.state_stack.len() > 1 {
			self.state_stack.pop();
		}
    }
	
	/// Resets current render state to default values. Does not affect the render state stack.
	pub fn reset(&mut self) {
		*self.state_mut() = Default::default();
	}
	
	// Render styles
	
    /// Sets the miter limit of the stroke style.
    ///
    /// Miter limit controls when a sharp corner is beveled.
    pub fn set_miter_limit(&mut self, limit: f32) {
        self.state_mut().miter_limit = limit;
    }
    
    /// Sets the transparency applied to all rendered shapes.
    ///
    /// Already transparent paths will get proportionally more transparent as well.
    pub fn set_global_alpha(&mut self, alpha: f32) {
        self.state_mut().alpha = alpha;
    }
    
    // Images
    
    /// Creates image by loading it from the disk from specified file name.
    pub fn create_image<P: AsRef<FilePath>>(&mut self, filename: P, flags: ImageFlags) -> Result<ImageId, CanvasError> {
		let image = image::open(filename)?;
		
		Ok(self.create_image_rgba(flags, &image))
	}
	
	/// Creates image by loading it from the specified chunk of memory.
	pub fn create_image_mem(&mut self, flags: ImageFlags, data: &[u8]) -> Result<ImageId, CanvasError> {
		let image = image::load_from_memory(data)?;
		
		Ok(self.create_image_rgba(flags, &image))
	}
	
	/// Creates image by loading it from the specified chunk of memory.
	pub fn create_image_rgba(&mut self, flags: ImageFlags, image: &DynamicImage) -> ImageId {
		let w = image.width();
		let h = image.height();
		
		let image_id = self.renderer.create_texture(TextureType::Rgba, w, h, flags);
        
        self.renderer.update_texture(image_id, image, 0, 0, w, h);
        
        image_id
	}
	
	/// Updates image data specified by image handle.
	pub fn update_image(&mut self, id: ImageId, image: &DynamicImage) {
		let w = image.width();
		let h = image.height();
        
        self.renderer.update_texture(id, image, 0, 0, w, h);
	}
	
	/// Deletes created image.
	pub fn delete_image(&mut self, id: ImageId) {
		self.renderer.delete_texture(id);
	}
    
    // Transforms
    
    /// Resets current transform to a identity matrix.
    pub fn reset_transform(&mut self) {
        self.state_mut().transform = Transform2D::identity();
    }
    
    /// Premultiplies current coordinate system by specified matrix.
    /// The parameters are interpreted as matrix as follows:
    ///   [a c e]
    ///   [b d f]
    ///   [0 0 1]
    pub fn transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        let transform = Transform2D([a, b, c, d, e, f]);
        self.state_mut().transform.premultiply(&transform);
    }
    
    /// Translates the current coordinate system.
    pub fn translate(&mut self, x: f32, y: f32) {
        let mut t = Transform2D::identity();
        t.translate(x, y);
        self.state_mut().transform.premultiply(&t);
    }
    
    /// Rotates the current coordinate system. Angle is specified in radians.
    pub fn rotate<R: Into<Rad>>(&mut self, angle: R) {
        let mut t = Transform2D::identity();
        t.rotate(angle);
        self.state_mut().transform.premultiply(&t);
    }
    
    /// Skews the current coordinate system along X axis. Angle is specified in radians.
    pub fn skew_x<R: Into<Rad>>(&mut self, angle: R) {
        let mut t = Transform2D::identity();
        t.skew_x(angle);
        self.state_mut().transform.premultiply(&t);
    }
    
    /// Skews the current coordinate system along Y axis. Angle is specified in radians.
    pub fn skew_y<R: Into<Rad>>(&mut self, angle: R) {
        let mut t = Transform2D::identity();
        t.skew_y(angle);
        self.state_mut().transform.premultiply(&t);
    }
    
    /// Scales the current coordinate system.
    pub fn scale(&mut self, x: f32, y: f32) {
        let mut t = Transform2D::identity();
        t.scale(x, y);
        self.state_mut().transform.premultiply(&t);
    }
    
    /// Returns the current transformation matrix
    pub fn current_transform(&self) -> Transform2D {
        self.state().transform
    }
    
    // Scissoring
    
    /// Sets the current scissor rectangle.
    ///
    /// The scissor rectangle is transformed by the current transform.
    pub fn scissor(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let state = self.state_mut();
        
        let w = w.max(0.0);
        let h = h.max(0.0);
        
        state.scissor.transform = Transform2D::identity();
        state.scissor.transform[4] = x + w * 0.5;
        state.scissor.transform[5] = y + h * 0.5;
        state.scissor.transform.premultiply(&state.transform);
        
        state.scissor.extent[0] = w * 0.5;
        state.scissor.extent[1] = h * 0.5;
    }
    
    /// Intersects current scissor rectangle with the specified rectangle.
    ///
    /// The scissor rectangle is transformed by the current transform.
    /// Note: in case the rotation of previous scissor rect differs from
    /// the current one, the intersection will be done between the specified
    /// rectangle and the previous scissor rectangle transformed in the current
    /// transform space. The resulting shape is always rectangle.
    pub fn intersect_scissor(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let state = self.state_mut();
        
        // If no previous scissor has been set, set the scissor as current scissor.
        // TODO: Make state.scissor an Option instead of relying on extent being less than 0
        if state.scissor.extent[0] < 0.0 {
            self.scissor(x, y, w, h);
            return;
        }
        
        // Transform the current scissor rect into current transform space.
        // If there is difference in rotation, this will be approximation.
        
        let mut pxform = Transform2D::identity();
        
        let mut invxform = state.transform;
        invxform.inverse();
        
        pxform.multiply(&invxform);
        
        let ex = state.scissor.extent[0];
        let ey = state.scissor.extent[1];
        
        let tex = ex*pxform[0].abs() + ey*pxform[2].abs();
        let tey = ex*pxform[1].abs() + ey*pxform[3].abs();
        
        let a = Rect::new(pxform[4]-tex, pxform[5]-tey, tex*2.0, tey*2.0);
        let res = a.intersect(Rect::new(x, y, w, h));
        
        self.scissor(res.x, res.y, res.w, res.h);
    }
    
    /// Reset and disables scissoring.
    pub fn reset_scissor(&mut self) {
        self.state_mut().scissor = Scissor::default();
    }
    
    // Paths
	
	/// Clears the current path and sub-paths.
    pub fn begin_path(&mut self) {
        self.commands.clear();
        self.cache.clear();
    }
    
    /// Starts new sub-path with specified point as first point.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.append_commands(&mut [Command::MoveTo(x, y)]);
    }
    
    /// Adds line segment from the last point in the path to the specified point.
    pub fn line_to(&mut self, x: f32, y: f32) {
        self.append_commands(&mut [Command::LineTo(x, y)]);
    }
    
    /// Adds cubic bezier segment from last point in the path via two control points to the specified point.
    pub fn bezier_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.append_commands(&mut [Command::BezierTo(c1x, c1y, c2x, c2y, x, y)]);
    }
    
    /// Adds quadratic bezier segment from last point in the path via a control point to the specified point.
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
		let x0 = self.commandx;
		let y0 = self.commandy;
		
        self.append_commands(&mut [
			Command::BezierTo(
				x0 + 2.0/3.0*(cx - x0), y0 + 2.0/3.0*(cy - y0),
				x + 2.0/3.0*(cx - x), y + 2.0/3.0*(cy - y),
				x, y
			)
		]);
    }
    
    /// Closes current sub-path with a line segment.
    pub fn close_path(&mut self) {
        self.append_commands(&mut [Command::Close]);
    }
    
    /// Sets the current sub-path winding, see Winding and Solidity
    pub fn set_path_winding(&mut self, winding: Winding) {
		self.append_commands(&mut [Command::Winding(winding)]);
	}
	
	/// Creates new circle arc shaped sub-path. The arc center is at cx,cy, the arc radius is r,
	/// and the arc is drawn from angle a0 to a1, and swept in direction dir (Winding)
	/// Angles are specified in radians.
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, dir: Winding) {
		// TODO: use small stack vec here
		let mut commands = Vec::new();
		
		let mut da = a1 - a0;
		
		if dir == Winding::CW {
			if da.abs() >= PI * 2.0 {
				da = PI * 2.0;
			} else {
				while da < 0.0 { da += PI * 2.0 }
			}
		} else {
			if da.abs() >= PI * 2.0 {
				da = -PI * 2.0;
			} else {
				while da > 0.0 { da -= PI * 2.0 }
			}
		}
		
		// Split arc into max 90 degree segments.
		let ndivs = ((da.abs() / (PI * 0.5) + 0.5) as i32).min(5).max(1);
		let hda = (da / ndivs as f32) / 2.0;
		let mut kappa = (4.0 / 3.0 * (1.0 - hda.cos()) / hda.sin()).abs();
		
		if dir == Winding::CCW {
			kappa = -kappa;
		}
		
		let (mut px, mut py, mut ptanx, mut ptany) = (0f32, 0f32, 0f32, 0f32);
		
		for i in 0..=ndivs {
			let a = a0 + da * (i as f32 / ndivs as f32);
			let dx = a.cos();
			let dy = a.sin();
			let x = cx + dx*r;
			let y = cy + dy*r;
			let tanx = -dy*r*kappa;
			let tany = dx*r*kappa;
			
			if i == 0 {
				let first_move = if self.commands.len() > 0 { Command::LineTo(x, y) } else { Command::MoveTo(x, y) };
				commands.push(first_move);
			} else {
				commands.push(Command::BezierTo(px+ptanx, py+ptany, x-tanx, y-tany, x, y));
			}
			
			px = x;
			py = y;
			ptanx = tanx;
			ptany = tany;
		}
		
		self.append_commands(&mut commands);
	}
	
	/// Adds an arc segment at the corner defined by the last path point, and two specified points.
    pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) {
        if self.commands.len() == 0 {
			return;
		}
		
		let mut x0 = 0.0;
        let mut y0 = 0.0;
		
		self.state().transform.inversed().transform_point(&mut x0, &mut y0, self.commandx, self.commandy);
		
		// Handle degenerate cases.
		if pt_equals(x0, y0, x1, y1, self.dist_tol) || pt_equals(x1, y1, x2, y2, self.dist_tol) || dist_pt_segment(x1, y1, x0, y0, x2, y2) < self.dist_tol * self.dist_tol || radius < self.dist_tol {
			self.line_to(x1, y1);
			return;
		}
		
		let mut dx0 = x0 - x1;
		let mut dy0 = y0 - y1;
		let mut dx1 = x2 - x1;
		let mut dy1 = y2 - y1;
		
		normalize(&mut dx0, &mut dy0);
		normalize(&mut dx1, &mut dy1);
		
		let a = (dx0*dx1 + dy0*dy1).acos();
		let d = radius / (a/2.0).tan();
		
		if d > 10000.0 {
			self.line_to(x1, y1);
			return;
		}
		
		let (cx, cy, a0, a1, dir);
		
		if cross(dx0, dy0, dx1, dy1) > 0.0 {
			cx = x1 + dx0*d + dy0*radius;
			cy = y1 + dy0*d + -dx0*radius;
			a0 = dx0.atan2(-dy0);
			a1 = -dx1.atan2(dy1);
			dir = Winding::CW;
		} else {
			cx = x1 + dx0*d + -dy0*radius;
			cy = y1 + dy0*d + dx0*radius;
			a0 = -dx0.atan2(dy0);
			a1 = dx1.atan2(-dy1);
			dir = Winding::CCW;
		}
		
		self.arc(cx, cy, radius, a0, a1, dir);
    }
    
    /// Creates new rectangle shaped sub-path.
	pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
		self.append_commands(&mut [
			Command::MoveTo(x, y),
			Command::LineTo(x, y + h),
			Command::LineTo(x + w, y + h),
			Command::LineTo(x + w, y),
			Command::Close
		]);
	}
    
    /// Creates new rounded rectangle shaped sub-path.
	pub fn rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
		self.rounded_rect_varying(x, y, w, h, r, r, r, r);
	}
	
	/// Creates new rounded rectangle shaped sub-path with varying radii for each corner.
	pub fn rounded_rect_varying(&mut self, x: f32, y: f32, w: f32, h: f32, rad_top_left: f32, rad_top_right: f32, rad_bottom_right: f32, rad_bottom_left: f32) {
		if rad_top_left < 0.1 && rad_top_right < 0.1 && rad_bottom_right < 0.1 && rad_bottom_left < 0.1 {
			self.rect(x, y, w, h);
		} else {
			let halfw = w.abs()*0.5;
            let halfh = h.abs()*0.5;
            
            let rx_bl = rad_bottom_left.min(halfw) * w.signum();
            let ry_bl = rad_bottom_left.min(halfh) * h.signum();
            
            let rx_br = rad_bottom_right.min(halfw) * w.signum();
            let ry_br = rad_bottom_right.min(halfh) * h.signum();
            
            let rx_tr = rad_top_right.min(halfw) * w.signum();
            let ry_tr = rad_top_right.min(halfh) * h.signum();
            
            let rx_tl = rad_top_left.min(halfw) * w.signum();
            let ry_tl = rad_top_left.min(halfh) * h.signum();
            
            self.append_commands(&mut [
                Command::MoveTo(x, y + ry_tl),
                Command::LineTo(x, y + h - ry_bl),
                Command::BezierTo(x, y + h - ry_bl*(1.0 - KAPPA90), x + rx_bl*(1.0 - KAPPA90), y + h, x + rx_bl, y + h),
                Command::LineTo(x + w - rx_br, y + h),
                Command::BezierTo(x + w - rx_br*(1.0 - KAPPA90), y + h, x + w, y + h - ry_br*(1.0 - KAPPA90), x + w, y + h - ry_br),
                Command::LineTo(x + w, y + ry_tr),
                Command::BezierTo(x + w, y + ry_tr*(1.0 - KAPPA90), x + w - rx_tr*(1.0 - KAPPA90), y, x + w - rx_tr, y),
                Command::LineTo(x + rx_tl, y),
                Command::BezierTo(x + rx_tl*(1.0 - KAPPA90), y, x, y + ry_tl*(1.0 - KAPPA90), x, y + ry_tl),
                Command::Close
            ]);
		}
	}
	
	/// Creates new ellipse shaped sub-path.
	pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32) {
		self.append_commands(&mut [
			Command::MoveTo(cx-rx, cy),
			Command::BezierTo(cx-rx, cy+ry*KAPPA90, cx-rx*KAPPA90, cy+ry, cx, cy+ry),
			Command::BezierTo(cx+rx*KAPPA90, cy+ry, cx+rx, cy+ry*KAPPA90, cx+rx, cy),
			Command::BezierTo(cx+rx, cy-ry*KAPPA90, cx+rx*KAPPA90, cy-ry, cx, cy-ry),
			Command::BezierTo(cx-rx*KAPPA90, cy-ry, cx-rx, cy-ry*KAPPA90, cx-rx, cy),
			Command::Close
		]);
	}

	/// Creates new circle shaped sub-path.
	pub fn circle(&mut self, cx: f32, cy: f32, r: f32) {
		self.ellipse(cx, cy, r, r);
	}
	
	/// Fills the current path with current fill style.
	pub fn fill(&mut self, paint: &Paint) {
        self.flatten_paths();
        
        let mut paint = paint.clone();
        
		if self.renderer.edge_antialiasing() && paint.shape_anti_alias() {
			self.expand_fill(self.fringe_width, LineJoin::Miter, 2.4);
		} else {
			self.expand_fill(0.0, LineJoin::Miter, 2.4);
		}
		
        let mut transform = paint.transform();
        transform.multiply(&self.state().transform);
		paint.set_transform(transform);
        
		// Apply global alpha
        let mut inner_color = paint.inner_color();
        inner_color.a *= self.state().alpha;
        paint.set_inner_color(inner_color);
        
        let mut outer_color = paint.outer_color();
        outer_color.a *= self.state().alpha;
        paint.set_outer_color(outer_color);
		
        let scissor = &self.state_stack.last().unwrap().scissor;
        
		self.renderer.render_fill(&paint, scissor, self.fringe_width, self.cache.bounds, &self.cache.paths);
	}
	
	/// Fills the current path with current stroke style.
	pub fn stroke(&mut self, paint: &Paint) {
		let scale = self.state().transform.average_scale();
		let mut stroke_width = (paint.stroke_width() * scale).max(0.0).min(200.0);
        
        let mut paint = paint.clone();
		
		if stroke_width < self.fringe_width {
			// If the stroke width is less than pixel size, use alpha to emulate coverage.
			// Since coverage is area, scale by alpha*alpha.
			let alpha = (stroke_width / self.fringe_width).max(0.0).min(1.0);
			
            let mut inner_color = paint.inner_color();
            inner_color.a *= alpha*alpha;
            paint.set_inner_color(inner_color);
            
            let mut outer_color = paint.outer_color();
            outer_color.a *= alpha*alpha;
            paint.set_outer_color(outer_color);
			
			stroke_width = self.fringe_width;
		}
		
        let mut transform = paint.transform();
        transform.multiply(&self.state().transform);
		paint.set_transform(transform);
		
		// Apply global alpha
        let mut inner_color = paint.inner_color();
        inner_color.a *= self.state().alpha;
        paint.set_inner_color(inner_color);
        
        let mut outer_color = paint.outer_color();
        outer_color.a *= self.state().alpha;
        paint.set_outer_color(outer_color);
		
		self.flatten_paths();
		
		if self.renderer.edge_antialiasing() && paint.shape_anti_alias() {
			self.expand_stroke(stroke_width * 0.5, self.fringe_width, paint.line_cap(), paint.line_join(), self.state().miter_limit);
		} else {
			self.expand_stroke(stroke_width * 0.5, 0.0, paint.line_cap(), paint.line_join(), self.state().miter_limit);
		}
		
		let scissor = &self.state_stack.last().unwrap().scissor;
		
		self.renderer.render_stroke(&paint, scissor, self.fringe_width, stroke_width, &self.cache.paths);
	}
	
	// Text
	
	pub fn add_font<P: AsRef<FilePath>>(&mut self, file_path: P) {
		self.font_manager.add_font_file(file_path).expect("cannot add font");
	}
	
    /*
	pub fn text_bounds(&mut self, x: f32, y: f32, text: &str) -> [f32; 4] {
		let scale = self.font_scale() * self.device_px_ratio;
		let invscale = 1.0 / scale;
		
		let mut style = FontStyle::new("NotoSans-Regular");
		style.set_size((self.state().font_size as f32 * scale) as u32);
		style.set_letter_spacing(self.state().letter_spacing * scale);
		style.set_blur(self.state().font_blur * scale);
		
		let layout = self.font_manager.layout_text(x, y, &mut self.renderer, style, text).unwrap();
		
		let mut bounds = layout.bbox;
		
		// Use line bounds for height.
		//let (ymin, ymax) = self.font_stash.line_bounds(y * scale);
		//bounds[1] = ymin;
		//bounds[3] = ymax;
		
		bounds[0] *= invscale;
		bounds[1] *= invscale;
		bounds[2] *= invscale;
		bounds[3] *= invscale;
		
		bounds
	}*/
	
	pub fn text(&mut self, x: f32, y: f32, text: &str, paint: &Paint) {
		let transform = self.state().transform;
		let scale = self.font_scale() * self.device_px_ratio;
		let invscale = 1.0 / scale;
        
        let mut paint = paint.clone();
		
		//let mut style = FontStyle::new("DroidSerif");
		//let mut style = FontStyle::new("Roboto-Regular");
		//let mut style = FontStyle::new("Amiri-Regular");
		//let mut style = FontStyle::new("NotoSansDevanagari-Regular");
		let mut style = FontStyle::new(paint.font_name());
		
		style.set_size((paint.font_size() as f32 * scale) as u32);
		style.set_letter_spacing(paint.letter_spacing() * scale);
		style.set_blur(paint.font_blur() * scale);
		
		let layout = self.font_manager.layout_text(x, y, &mut self.renderer, style, text).unwrap();
		
		for cmd in &layout.cmds {
			let mut verts = Vec::new();
			
			for quad in &cmd.quads {
				let (mut p0, mut p1, mut p2, mut p3, mut p4, mut p5, mut p6, mut p7) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
			
				transform.transform_point(&mut p0, &mut p1, quad.x0*invscale, quad.y0*invscale);
				transform.transform_point(&mut p2, &mut p3, quad.x1*invscale, quad.y0*invscale);
				transform.transform_point(&mut p4, &mut p5, quad.x1*invscale, quad.y1*invscale);
				transform.transform_point(&mut p6, &mut p7, quad.x0*invscale, quad.y1*invscale);
				
				verts.push(Vertex::new(p0, p1, quad.s0, quad.t0));
				verts.push(Vertex::new(p4, p5, quad.s1, quad.t1));
				verts.push(Vertex::new(p2, p3, quad.s1, quad.t0));
				verts.push(Vertex::new(p0, p1, quad.s0, quad.t0));
				verts.push(Vertex::new(p6, p7, quad.s0, quad.t1));
				verts.push(Vertex::new(p4, p5, quad.s1, quad.t1));
			}
			
			paint.set_image(Some(cmd.image_id));
			
			// Apply global alpha
			//paint.inner_color.a *= self.state().alpha;
			//paint.outer_color.a *= self.state().alpha;
			
			let scissor = &self.state_stack.last().unwrap().scissor;
			
			self.renderer.render_triangles(&paint, scissor, &verts);
		}
	}
	
	// Private
	
	fn font_scale(&self) -> f32 {
		let avg_scale = self.state().transform.average_scale();
		
		quantize(avg_scale, 0.01).min(4.0)
	}
	
	fn state(&self) -> &State {
		self.state_stack.last().unwrap()
	}
	
    fn state_mut(&mut self) -> &mut State {
		self.state_stack.last_mut().unwrap()
	}
    
	fn set_device_pixel_ratio(&mut self, ratio: f32) {
		self.tess_tol = 0.25 / ratio;
		self.dist_tol = 0.01 / ratio;
		self.fringe_width = 1.0 / ratio;
		self.device_px_ratio = ratio;
	}
	
	fn append_commands(&mut self, commands: &mut [Command]) {
		let transform = self.state().transform;
		
		// transform
		for cmd in commands.iter_mut() {
			match cmd {
				Command::MoveTo(x, y) => {
					transform.transform_point(x, y, *x, *y);
					self.commandx = *x;
					self.commandy = *y;
				}
				Command::LineTo(x, y) => {
					transform.transform_point(x, y, *x, *y);
					self.commandx = *x;
					self.commandy = *y;
				}
				Command::BezierTo(c1x, c1y, c2x, c2y, x, y) => {
					transform.transform_point(c1x, c1y, *c1x, *c1y);
					transform.transform_point(c2x, c2y, *c2x, *c2y);
					transform.transform_point(x, y, *x, *y);
					self.commandx = *x;
					self.commandy = *y;
				}
				_ => ()
			}
		}
		
		self.commands.extend_from_slice(commands);
	}
	
	fn flatten_paths(&mut self) {
		if self.cache.paths.len() > 0 {
			return;
		}
		
		for cmd in &self.commands {
			match cmd {
				Command::MoveTo(x, y) => {
					self.cache.add_path();
					self.cache.add_point(*x, *y, POINT_CORNER, self.dist_tol);
				}
				Command::LineTo(x, y) => {
					self.cache.add_point(*x, *y, POINT_CORNER, self.dist_tol);
				}
				Command::BezierTo(c1x, c1y, c2x, c2y, x, y) => {
					if let Some(last) = self.cache.last_point() {
						self.cache.tesselate_bezier(last.x, last.y, *c1x, *c1y, *c2x, *c2y, *x, *y, 0, POINT_CORNER, self.tess_tol, self.dist_tol);
					}
				}
				Command::Close => {
					self.cache.last_path().map(|path| path.closed = true);
				}
				Command::Winding(winding) => {
					self.cache.last_path().map(|path| path.winding = *winding);
				}
			}
		}
		
		self.cache.bounds[0] = 1e6;
		self.cache.bounds[1] = 1e6;
		self.cache.bounds[2] = -1e6;
		self.cache.bounds[3] = -1e6;
		
		for path in &mut self.cache.paths {
			let mut points = &mut self.cache.points[path.first..(path.first + path.count)];
			
			let p0 = points.last().copied().unwrap();
			let p1 = points.first().copied().unwrap();
			
			// If the first and last points are the same, remove the last, mark as closed path.
			if pt_equals(p0.x, p0.y, p1.x, p1.y, self.dist_tol) {
				path.count -= 1;
				//p0 = points[path.count-1];
				path.closed = true;
				points = &mut self.cache.points[path.first..(path.first + path.count)];
			}
			
			// Enforce winding.
			if path.count > 2 {
				let area = poly_area(points);
				
				if path.winding == Winding::CCW && area < 0.0 {
                    points.reverse();
				}
				
				if path.winding == Winding::CW && area > 0.0 {
					points.reverse();
				}
			}
			
			// TODO: this is doggy and fishy.
			for i in 0..path.count {
				let p1 = points[i];
				
				let p0 = if i == 0 {
					points.last_mut().unwrap()
				} else {
					points.get_mut(i-1).unwrap()
				};
				
				p0.dx = p1.x - p0.x;
				p0.dy = p1.y - p0.y;
				p0.len = normalize(&mut p0.dx, &mut p0.dy);
				
				self.cache.bounds[0] = self.cache.bounds[0].min(p0.x);
				self.cache.bounds[1] = self.cache.bounds[1].min(p0.y);
				self.cache.bounds[2] = self.cache.bounds[2].max(p0.x);
				self.cache.bounds[3] = self.cache.bounds[3].max(p0.y);
			}
		}
	}
	
	fn expand_fill(&mut self, w: f32, line_join: LineJoin, miter_limit: f32) {
		
		let fringe = w > 0.0;
		let aa = self.fringe_width;
		
		self.calculate_joins(w, line_join, miter_limit);
		
		// Calculate max vertex usage.
        /*
		let mut vertex_count = 0;
		
		for path in &self.cache.paths {
			vertex_count += path.count + path.bevel + 1;
			
			if fringe {
				vertex_count += (path.count + path.bevel*5 + 1) * 2;// plus one for loop
			}
		}*/
		
		//self.cache.verts.clear();
		//self.cache.verts.reserve(vertex_count);
		
		let convex = self.cache.paths.len() == 1 && self.cache.paths[0].convex;
		
		for path in &mut self.cache.paths {
            let points = &self.cache.points[path.first..(path.first + path.count)];
            
            path.stroke.clear();
			
            // TODO: test this when aa is working. Currently 0.5 * aa doesn't seem to product the correct
            // result on some edges, the pixels land behind the fill shape. but if woff = aa then it looks correct.
            // The issue may be in how aa is calculated before we reach this point and not in the 0.5 * aa calculation.
            let woff = 0.5 * aa;
			//let woff = aa;
			
            if fringe {
				for i in 0..path.count {
                    let p1 = points[i];
                    
                    let p0 = if i == 0 {
                        points.last().unwrap()
                    } else {
                        points.get(i-1).unwrap()
                    };
                    
                    if p1.flags & POINT_BEVEL > 0 {
                        // TODO: why do we need these variables.. just use p0.. and p1 directly down there
                        let dlx0 = p0.dy;
                        let dly0 = -p0.dx;
                        let dlx1 = p1.dy;
                        let dly1 = -p1.dx;
                        
                        if p1.flags & POINT_LEFT > 0 {
                            let lx = p1.x + p1.dmx * woff;
                            let ly = p1.y + p1.dmy * woff;
                            path.fill.push(Vertex::new(lx, ly, 0.5, 1.0));
                        } else {
                            let lx0 = p1.x + dlx0 * woff;
                            let ly0 = p1.y + dly0 * woff;
                            let lx1 = p1.x + dlx1 * woff;
                            let ly1 = p1.y + dly1 * woff;
                            path.fill.push(Vertex::new(lx0, ly0, 0.5, 1.0));
                            path.fill.push(Vertex::new(lx1, ly1, 0.5, 1.0));
                        }
                    } else {
                        path.fill.push(Vertex::new(p1.x + (p1.dmx * woff), p1.y + (p1.dmy * woff), 0.5, 1.0));
                    }
                }
			} else {
				for i in 0..path.count {
					path.fill.push(Vertex::new(points[i].x, points[i].y, 0.5, 1.0));
				}
			}
            
            if fringe {
                let mut lw = w + woff;
                let rw = w - woff;
                let mut lu = 0.0;
                let ru = 1.0;
                
                // Create only half a fringe for convex shapes so that
                // the shape can be rendered without stenciling.
                if convex {
                    lw = woff;	// This should generate the same vertex as fill inset above.
                    lu = 0.5;	// Set outline fade at middle.
                }
                
                for i in 0..path.count {
                    let p1 = points[i];
                    
                    let p0 = if i == 0 {
                        points.last().unwrap()
                    } else {
                        points.get(i-1).unwrap()
                    };
                    
                    if p1.flags & (POINT_BEVEL | POINT_INNERBEVEL) != 0 {
                        bevel_join(&mut path.stroke, p0, &p1, lw, rw, lu, rw, self.fringe_width);
                    } else {
                        path.stroke.push(Vertex::new(p1.x + (p1.dmx * lw), p1.y + (p1.dmy * lw), lu, 1.0));
                        path.stroke.push(Vertex::new(p1.x - (p1.dmx * rw), p1.y - (p1.dmy * rw), ru, 1.0));
                    }
                }
                
                // Loop it
                let p0 = path.stroke[0];
                let p1 = path.stroke[1];
                path.stroke.push(Vertex::new(p0.x, p0.y, lu, 1.0));
                path.stroke.push(Vertex::new(p1.x, p1.y, ru, 1.0));
            }
		}
	}
	
	fn expand_stroke(&mut self, w: f32, fringe: f32, line_cap: LineCap, line_join: LineJoin, miter_limit: f32) {
		let aa = fringe;
		let mut u0 = 0.0;
		let mut u1 = 1.0;
		let ncap = curve_divisions(w, PI, self.tess_tol);
		
		let w = w + (aa * 0.5);
		
		// Disable the gradient used for antialiasing when antialiasing is not used.
		if aa == 0.0 {
			u0 = 0.5;
			u1 = 0.5;
		}
		
		self.calculate_joins(w, line_join, miter_limit);
		
		for path in &mut self.cache.paths {
			let points = &self.cache.points[path.first..(path.first + path.count)];
			
			path.stroke.clear();
			
			// TODO: this is horrible - make a pretty configurable iterator that takes into account if the path is closed or not and gives correct p0 p1
			
			if path.closed {
				
				for i in 0..path.count {
					let p1 = points[i];
					
					let p0 = if i == 0 {
						points[path.count-1]
					} else {
						points[i-1]
					};
					
					if (p1.flags & (POINT_BEVEL | POINT_INNERBEVEL)) != 0 {
						if line_join == LineJoin::Round {
							round_join(&mut path.stroke, &p0, &p1, w, w, u0, u1, ncap as usize, aa);
						} else {
							bevel_join(&mut path.stroke, &p0, &p1, w, w, u0, u1, aa);
						}
					} else {
						path.stroke.push(Vertex::new(p1.x + (p1.dmx * w), p1.y + (p1.dmy * w), u0, 1.0));
						path.stroke.push(Vertex::new(p1.x - (p1.dmx * w), p1.y - (p1.dmy * w), u1, 1.0));
					}
				}
				
				path.stroke.push(Vertex::new(path.stroke[0].x, path.stroke[0].y, u0, 1.0));
				path.stroke.push(Vertex::new(path.stroke[1].x, path.stroke[1].y, u1, 1.0));
				
			} else {
				let mut p0 = points[0];
				let mut p1 = points[1];
				
				// Add cap
				let mut dx = p1.x - p0.x;
				let mut dy = p1.y - p0.y;
				
				normalize(&mut dx, &mut dy);
				
				match line_cap {
					LineCap::Butt => butt_cap_start(&mut path.stroke, &p0, dx, dy, w, -aa*0.5, aa, u0, u1),
					LineCap::Square => butt_cap_start(&mut path.stroke, &p0, dx, dy, w, w-aa, aa, u0, u1),
					LineCap::Round => round_cap_start(&mut path.stroke, &p0, dx, dy, w, ncap as usize, aa, u0, u1),
				}
				
				// loop
				for i in 1..(path.count - 1) {
					p1 = points[i];
					p0 = points[i-1];
					
					if (p1.flags & (POINT_BEVEL | POINT_INNERBEVEL)) != 0 {
						if line_join == LineJoin::Round {
							round_join(&mut path.stroke, &p0, &p1, w, w, u0, u1, ncap as usize, aa);
						} else {
							bevel_join(&mut path.stroke, &p0, &p1, w, w, u0, u1, aa);
						}
					} else {
						path.stroke.push(Vertex::new(p1.x + (p1.dmx * w), p1.y + (p1.dmy * w), u0, 1.0));
						path.stroke.push(Vertex::new(p1.x - (p1.dmx * w), p1.y - (p1.dmy * w), u1, 1.0));
					}
				}
				
				// Add cap
				p0 = points[path.count - 2];
				p1 = points[path.count - 1];
				
				let mut dx = p1.x - p0.x;
				let mut dy = p1.y - p0.y;
				
				normalize(&mut dx, &mut dy);
				
				match line_cap {
					LineCap::Butt => butt_cap_end(&mut path.stroke, &p1, dx, dy, w, -aa*0.5, aa, u0, u1),
					LineCap::Square => butt_cap_end(&mut path.stroke, &p1, dx, dy, w, w-aa, aa, u0, u1),
					LineCap::Round => round_cap_end(&mut path.stroke, &p1, dx, dy, w, ncap as usize, aa, u0, u1),
				}
			}
		}
	}
	
	fn calculate_joins(&mut self, w: f32, line_join: LineJoin, miter_limit: f32) {
		let iw = if w > 0.0 { 1.0 / w } else { 0.0 };
		
		for path in &mut self.cache.paths {
			let points = &mut self.cache.points[path.first..(path.first+path.count)];
			let mut nleft = 0;
			
			path.bevel = 0;
			
			for i in 0..path.count {
				
				let p0 = if i == 0 {
					points.get(path.count-1).cloned().unwrap()
				} else {
					points.get(i-1).cloned().unwrap()
				};
				
				let p1 = &mut points[i];
				
				let dlx0 = p0.dy;
				let dly0 = -p0.dx;
				let dlx1 = p1.dy;
				let dly1 = -p1.dx;
				
				// Calculate extrusions
				p1.dmx = (dlx0 + dlx1) * 0.5;
				p1.dmy = (dly0 + dly1) * 0.5;
				let dmr2 = p1.dmx * p1.dmx + p1.dmy * p1.dmy;
				
				if dmr2 > 0.000001 {
					let scale = (1.0 / dmr2).min(600.0);

					p1.dmx *= scale;
					p1.dmy *= scale;
				}
				
				// Clear flags, but keep the corner.
				p1.flags = if (p1.flags & POINT_CORNER) > 0 { POINT_CORNER } else { 0 };
				
				// Keep track of left turns.
				let cross = p1.dx * p0.dy - p0.dx * p1.dy;
				
				if cross > 0.0 {
					nleft += 1;
					p1.flags |= POINT_LEFT;
				}
				
				// Calculate if we should use bevel or miter for inner join.
				let limit = (p0.len.min(p1.len) * iw).max(1.01);
				
				if (dmr2 * limit * limit) < 1.0 {
					p1.flags |= POINT_INNERBEVEL;
				}
				
				// Check to see if the corner needs to be beveled.
				if (p1.flags & POINT_CORNER) > 0 {
					if (dmr2 * miter_limit * miter_limit) < 1.0 || line_join == LineJoin::Bevel || line_join == LineJoin::Round {
						p1.flags |= POINT_BEVEL;
					}
				}
				
				if (p1.flags & (POINT_BEVEL | POINT_INNERBEVEL)) != 0 {
					path.bevel += 1;
				}
			}
			
			path.convex = nleft == path.count;
		}
	}
}

fn triarea2(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
	let abx = bx - ax;
	let aby = by - ay;
	let acx = cx - ax;
	let acy = cy - ay;
	
	acx*aby - abx*acy
}

fn pt_equals(x1: f32, y1: f32, x2: f32, y2: f32, tol: f32) -> bool {
	let dx = x2 - x1;
	let dy = y2 - y1;
	
	dx*dx + dy*dy < tol*tol
}

fn poly_area(points: &[Point]) -> f32 {
	let mut area = 0.0;
	
	for i in 2..points.len() {
		let p0 = points[0];
		let p1 = points[i-1];
		let p2 = points[i];
		
		area += triarea2(p0.x, p0.y, p1.x, p1.y, p2.x, p2.y);
	}
	
	area * 0.5
}

fn cross(dx0: f32, dy0: f32, dx1: f32, dy1: f32) -> f32 {
	dx1*dy0 - dx0*dy1
}

fn dist_pt_segment(x: f32, y: f32, px: f32, py: f32, qx: f32, qy: f32) -> f32 {
	let pqx = qx-px;
	let pqy = qy-py;
	let dx = x-px;
	let dy = y-py;
	let d = pqx*pqx + pqy*pqy;
	let mut t = pqx*dx + pqy*dy;
	
	if d > 0.0 { t /= d; }
	
	if t < 0.0 { t = 0.0; }
	else if t > 1.0 { t = 1.0; }
	
	let dx = px + t*pqx - x;
	let dy = py + t*pqy - y;
	
	dx*dx + dy*dy
}

fn quantize(a: f32, d: f32) -> f32 {
	(a / d + 0.5).trunc() * d
}

fn curve_divisions(radius: f32, arc: f32, tol: f32) -> u32 {
	let da = (radius / (radius + tol)).acos() * 2.0;
	
	((arc / da).ceil() as u32).max(2)
}

// TODO: fix this.. move it to point
fn normalize(x: &mut f32, y: &mut f32) -> f32 {
	let d = ((*x)*(*x) + (*y)*(*y)).sqrt();
	
	if d > 1e-6 {
		let id = 1.0 / d;
		*x *= id;
		*y *= id;
	}
	
	d
}

fn butt_cap_start(verts: &mut Vec<Vertex>, point: &Point, dx: f32, dy: f32, w: f32, d: f32, aa: f32, u0: f32, u1: f32) {
	let px = point.x - dx*d;
	let py = point.y - dy*d;
	let dlx = dy;
	let dly = -dx;
	
	verts.push(Vertex::new(px + dlx*w - dx*aa, py + dly*w - dy*aa, u0, 0.0));
	verts.push(Vertex::new(px - dlx*w - dx*aa, py - dly*w - dy*aa, u1, 0.0));
	verts.push(Vertex::new(px + dlx*w, py + dly*w, u0, 1.0));
	verts.push(Vertex::new(px - dlx*w, py - dly*w, u1, 1.0));
}

fn butt_cap_end(verts: &mut Vec<Vertex>, point: &Point, dx: f32, dy: f32, w: f32, d: f32, aa: f32, u0: f32, u1: f32) {
	let px = point.x + dx*d;
	let py = point.y + dy*d;
	let dlx = dy;
	let dly = -dx;
	
	verts.push(Vertex::new(px + dlx*w, py + dly*w, u0, 1.0));
	verts.push(Vertex::new(px - dlx*w, py - dly*w, u1, 1.0));
	verts.push(Vertex::new(px + dlx*w + dx*aa, py + dly*w + dy*aa, u0, 0.0));
	verts.push(Vertex::new(px - dlx*w + dx*aa, py - dly*w + dy*aa, u1, 0.0));
}

fn round_cap_start(verts: &mut Vec<Vertex>, point: &Point, dx: f32, dy: f32, w: f32, ncap: usize, _aa: f32, u0: f32, u1: f32) {
	let px = point.x;
	let py = point.y;
	let dlx = dy;
	let dly = -dx;
	
	for i in 0..ncap {
		let a = i as f32/(ncap as f32 - 1.0)*PI;
		let ax = a.cos() * w;
		let ay = a.sin() * w;
		
		verts.push(Vertex::new(px - dlx*ax - dx*ay, py - dly*ax - dy*ay, u0, 1.0));
		verts.push(Vertex::new(px, py, 0.5, 1.0));
	}
	
	verts.push(Vertex::new(px + dlx*w, py + dly*w, u0, 1.0));
	verts.push(Vertex::new(px - dlx*w, py - dly*w, u1, 1.0));
}

fn round_cap_end(verts: &mut Vec<Vertex>, point: &Point, dx: f32, dy: f32, w: f32, ncap: usize, _aa: f32, u0: f32, u1: f32) {
	let px = point.x;
	let py = point.y;
	let dlx = dy;
	let dly = -dx;
	
	verts.push(Vertex::new(px + dlx*w, py + dly*w, u0, 1.0));
	verts.push(Vertex::new(px - dlx*w, py - dly*w, u1, 1.0));
	
	for i in 0..ncap {
		let a = i as f32/(ncap as f32 - 1.0)*PI;
		let ax = a.cos() * w;
		let ay = a.sin() * w;
		
		verts.push(Vertex::new(px, py, 0.5, 1.0));
		verts.push(Vertex::new(px - dlx*ax + dx*ay, py - dly*ax + dy*ay, u0, 1.0));
	}
}

fn choose_bevel(bevel: bool, p0: &Point, p1: &Point, w: f32) -> (f32, f32, f32, f32) {
	if bevel {
		(p1.x + p0.dy * w, p1.y - p0.dx * w, p1.x + p1.dy * w, p1.y - p1.dx * w)
	} else {
		(p1.x + p1.dmx * w, p1.y + p1.dmy * w, p1.x + p1.dmx * w, p1.y + p1.dmy * w)
	}
}

fn round_join(verts: &mut Vec<Vertex>, p0: &Point, p1: &Point, lw: f32, rw: f32, lu: f32, ru: f32, ncap: usize, _fringe: f32) {
	let dlx0 = p0.dy;
	let dly0 = -p0.dx;
	let dlx1 = p1.dy;
	let dly1 = -p1.dx;
	
	let a0;
	let mut a1;
	
	// TODO: this if else arms are almost identical, maybe they can be combined
	if p1.flags & POINT_LEFT > 0 {
		let (lx0, ly0, lx1, ly1) = choose_bevel(p1.flags & POINT_INNERBEVEL > 0, p0, p1, lw);
		a0 = (-dly0).atan2(-dlx0);
		a1 = (-dly1).atan2(-dlx1);
		
		if a1 > a0 {
			a1 -= PI * 2.0;
		}
		
		verts.push(Vertex::new(lx0, ly0, lu, 1.0));
		verts.push(Vertex::new(p1.x - dlx0*rw, p1.y - dly0*rw, ru, 1.0));
		
		let n = ((((a0 - a1) / PI) * ncap as f32).ceil() as usize).max(2).min(ncap);
		
		for i in 0..n {
			let u = i as f32 / (n-1) as f32;
			let a = a0 + u*(a1-a0);
			let rx = p1.x + a.cos() * rw;
			let ry = p1.y + a.sin() * rw;
			
			verts.push(Vertex::new(p1.x, p1.y, 0.5, 1.0));
			verts.push(Vertex::new(rx, ry, ru, 1.0));
		}
		
		verts.push(Vertex::new(lx1, ly1, lu, 1.0));
		verts.push(Vertex::new(p1.x - dlx1*rw, p1.y - dly1*rw, ru, 1.0));
	} else {
		let (rx0, ry0, rx1, ry1) = choose_bevel(p1.flags & POINT_INNERBEVEL > 0, p0, p1, -rw);
		a0 = dly0.atan2(dlx0);
		a1 = dly1.atan2(dlx1);
		
		if a1 < a0 {
			a1 += PI * 2.0;
		}
		
		verts.push(Vertex::new(p1.x + dlx0*rw, p1.y + dly0*rw, lu, 1.0));
		verts.push(Vertex::new(rx0, ry0, ru, 1.0));
		
		let n = ((((a1 - a0) / PI) * ncap as f32).ceil() as usize).max(2).min(ncap);
		
		for i in 0..n {
			let u = i as f32 / (n-1) as f32;
			let a = a0 + u*(a1-a0);
			let lx = p1.x + a.cos() * lw;
			let ly = p1.y + a.sin() * lw;
			
			verts.push(Vertex::new(lx, ly, lu, 1.0));
			verts.push(Vertex::new(p1.x, p1.y, 0.5, 1.0));
		}
		
		verts.push(Vertex::new(p1.x + dlx1*rw, p1.y + dly1*rw, lu, 1.0));
		verts.push(Vertex::new(rx1, ry1, ru, 1.0));
	}
}

fn bevel_join(verts: &mut Vec<Vertex>, p0: &Point, p1: &Point, lw: f32, rw: f32, lu: f32, ru: f32, _fringe: f32) {
	let dlx0 = p0.dy;
	let dly0 = -p0.dx;
	let dlx1 = p1.dy;
	let dly1 = -p1.dx;
	
	// TODO: this if else arms are almost identical, maybe they can be combined
	if p1.flags & POINT_LEFT > 0 {
		let (lx0, ly0, lx1, ly1) = choose_bevel(p1.flags & POINT_INNERBEVEL > 0, p0, p1, lw);
		
		verts.push(Vertex::new(lx0, ly0, lu, 1.0));
		verts.push(Vertex::new(p1.x - dlx0*rw, p1.y - dly0*rw, ru, 1.0));

		if p1.flags & POINT_BEVEL > 0 {
			verts.push(Vertex::new(lx0, ly0, lu, 1.0));
			verts.push(Vertex::new(p1.x - dlx0*rw, p1.y - dly0*rw, ru, 1.0));

			verts.push(Vertex::new(lx1, ly1, lu, 1.0));
			verts.push(Vertex::new(p1.x - dlx1*rw, p1.y - dly1*rw, ru, 1.0));
		} else {
			let rx0 = p1.x - p1.dmx * rw;
			let ry0 = p1.y - p1.dmy * rw;

			verts.push(Vertex::new(p1.x, p1.y, 0.5, 1.0));
			verts.push(Vertex::new(p1.x - dlx0*rw, p1.y - dly0*rw, ru, 1.0));

			verts.push(Vertex::new(rx0, ry0, ru, 1.0));
			verts.push(Vertex::new(rx0, ry0, ru, 1.0));

			verts.push(Vertex::new(p1.x, p1.y, 0.5, 1.0));
			verts.push(Vertex::new(p1.x - dlx1*rw, p1.y - dly1*rw, ru, 1.0));
		}

		verts.push(Vertex::new(lx1, ly1, lu, 1.0));
		verts.push(Vertex::new(p1.x - dlx1*rw, p1.y - dly1*rw, ru, 1.0));
	} else {
		let (rx0, ry0, rx1, ry1) = choose_bevel(p1.flags & POINT_INNERBEVEL > 0, p0, p1, -rw);

		verts.push(Vertex::new(p1.x + dlx0*lw, p1.y + dly0*lw, lu, 1.0));
		verts.push(Vertex::new(rx0, ry0, ru, 1.0));

		if p1.flags & POINT_BEVEL > 0 {
			verts.push(Vertex::new(p1.x + dlx0*lw, p1.y + dly0*lw, lu, 1.0));
			verts.push(Vertex::new(rx0, ry0, ru, 1.0));

			verts.push(Vertex::new(p1.x + dlx1*lw, p1.y + dly1*lw, lu, 1.0));
			verts.push(Vertex::new(rx1, ry1, ru, 1.0));
		} else {
			let lx0 = p1.x + p1.dmx * lw;
			let ly0 = p1.y + p1.dmy * lw;

			verts.push(Vertex::new(p1.x + dlx0*lw, p1.y + dly0*lw, lu, 1.0));
			verts.push(Vertex::new(p1.x, p1.y, 0.5, 1.0));

			verts.push(Vertex::new(lx0, ly0, lu, 1.0));
			verts.push(Vertex::new(lx0, ly0, lu, 1.0));

			verts.push(Vertex::new(p1.x + dlx1*lw, p1.y + dly1*lw, lu, 1.0));
			verts.push(Vertex::new(p1.x, p1.y, 0.5, 1.0));
		}

		verts.push(Vertex::new(p1.x + dlx1*lw, p1.y + dly1*lw, lu, 1.0));
		verts.push(Vertex::new(rx1, ry1, ru, 1.0));
	}
}

#[derive(Debug)]
pub enum CanvasError {
	GeneralError(String),
	ImageError(image::ImageError),
	FontError(FontManagerError)
}

impl fmt::Display for CanvasError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "canvas error")
    }
}

impl From<image::ImageError> for CanvasError {
	fn from(error: image::ImageError) -> Self {
		Self::ImageError(error)
	}
}

impl From<FontManagerError> for CanvasError {
	fn from(error: FontManagerError) -> Self {
		Self::FontError(error)
	}
}

impl Error for CanvasError {}
