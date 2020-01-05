// TODO: Consider changing Paint/Brush to be an enum (Color, Gradient, Image)
// TODO: Move State.shape_anti_alias to Paint/Brush.anti_alias
// TODO: Maybe we should get rid of Color and just use this paint struct

use crate::math::{Rad, Transform2D};
use super::{Color, ImageId};

#[derive(Copy, Clone, Debug)]
pub struct Paint {
    pub(crate) transform: Transform2D,
    pub(crate) extent: [f32; 2],
    pub(crate) radius: f32,
    pub(crate) feather: f32,
    pub(crate) inner_color: Color,
    pub(crate) outer_color: Color,
    pub(crate) image: Option<ImageId>
}

impl Default for Paint {
	fn default() -> Self {
		Self {
			transform: Default::default(),
			extent: Default::default(),
			radius: Default::default(),
			feather: Default::default(),
			inner_color: Color::white(),
			outer_color: Default::default(),
			image: Default::default(),
		}
	}
}

impl Paint {
	pub fn color(color: Color) -> Self {
		let mut new = Self::default();
		new.set_color(color);
		new
	}
	
	/// Creates and returns an image pattern. 
	/// 
	/// Parameters (cx,cy) specify the left-top location of the image pattern, (w,h) the size of one image,
	/// angle rotation around the top-left corner, id is handle to the image to render.
	pub fn image<A: Into<Rad>>(id: ImageId, cx: f32, cy: f32, w: f32, h: f32, angle: A, alpha: f32) -> Paint {
		let mut paint = Self::default();
		
		paint.transform.rotate(angle);
		paint.transform[4] = cx;
		paint.transform[5] = cy;
		
		paint.extent[0] = w;
        paint.extent[1] = h;
        
        paint.image = Some(id);
        
        paint.inner_color = Color::rgbaf(1.0, 1.0, 1.0, alpha);
        paint.outer_color = Color::rgbaf(1.0, 1.0, 1.0, alpha);
        
        paint
	}
	
	/// Creates and returns a linear gradient paint. 
    ///
    /// The gradient is transformed by the current transform when it is passed to fill_paint() or stroke_paint().
	pub fn linear_gradient(start_x: f32, start_y: f32, end_x: f32, end_y: f32, start_color: Color, end_color: Color) -> Self {
        let mut paint = Self::default();
        
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
        
        paint.transform = Transform2D([
            dy, -dx,
            dx, dy,
            start_x - dx*large, start_y - dy*large
        ]);
        
        paint.extent[0] = large;
        paint.extent[1] = large + d*0.5;
        paint.radius = 0.0;
        paint.feather = 1.0f32.max(d);
        
        paint.inner_color = start_color;
        paint.outer_color = end_color;
        
        paint
    }
    
    /// Creates and returns a box gradient. 
    ///
    /// Box gradient is a feathered rounded rectangle, it is useful for rendering
    /// drop shadows or highlights for boxes. Parameters (x,y) define the top-left corner of the rectangle,
    /// (w,h) define the size of the rectangle, r defines the corner radius, and f feather. Feather defines how blurry
    /// the border of the rectangle is. Parameter inner_color specifies the inner color and outer_color the outer color of the gradient.
    /// The gradient is transformed by the current transform when it is passed to fill_paint() or stroke_paint().
    pub fn box_gradient(x: f32, y: f32, w: f32, h: f32, r: f32, f: f32, inner_color: Color, outer_color: Color) -> Self {
        let mut paint = Self::default();
        
        paint.transform = Transform2D::default();
        
        paint.transform[4] = x+w*0.5;
        paint.transform[5] = y+h*0.5;
        
        paint.extent[0] = w*0.5;
        paint.extent[1] = h*0.5;
        
        paint.radius = r;
        paint.feather = 1.0f32.max(f);
        
        paint.inner_color = inner_color;
        paint.outer_color = outer_color;
        
        paint
    }
    
    /// Creates and returns a radial gradient. 
    ///
    /// Parameters (cx,cy) specify the center, inr and outr specify
    /// the inner and outer radius of the gradient, icol specifies the start color and ocol the end color.
    /// The gradient is transformed by the current transform when it is passed to fill_paint() or stroke_paint().
    pub fn radial_gradient(cx: f32, cy: f32, inr: f32, outr: f32, inner_color: Color, outer_color: Color) -> Self {
        let mut paint = Self::default();
        
        let r = (inr + outr) * 0.5;
        let f = outr - inr;
        
        paint.transform[4] = cx;
        paint.transform[5] = cy;

        paint.extent[0] = r;
        paint.extent[1] = r;

        paint.radius = r;

        paint.feather = 1.0f32.max(f);

        paint.inner_color = inner_color;
        paint.outer_color = outer_color;

        paint
    }
	
    pub fn set_color(&mut self, color: Color) {
        self.transform = Transform2D::identity();
        self.radius = 0.0;
        self.feather = 1.0;
        self.inner_color = color;
        self.outer_color = color;
    }
}
