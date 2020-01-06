
// TODO: Maybe we should get rid of Color and just use this paint struct
// TODO: Make a paint builder
// TODO: move create_image, linear_gradient, box_gradient, radial_gradient to the builder
// TODO: Document all functions

use crate::math::{Rad, Transform2D};
use super::{Color, ImageId, LineCap, LineJoin, VAlign};

#[derive(Clone, Debug)]
pub struct Paint {
    transform: Transform2D,
    extent: [f32; 2],
    radius: f32,
    feather: f32,
    inner_color: Color,
    outer_color: Color,
    image: Option<ImageId>,
    shape_anti_alias: bool,
    stroke_width: f32,
    miter_limit: f32,
    line_cap: LineCap,
    line_join: LineJoin,
    font_name: String,
	font_size: u32,
	letter_spacing: f32,
	font_blur: f32,
	text_valign: VAlign,
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
            shape_anti_alias: true,
            stroke_width: 1.0,
            miter_limit: 10.0,
            line_cap: Default::default(),
            line_join: Default::default(),
            font_name: String::from("NotoSans-Regular"),
            font_size: 16,
            letter_spacing: 0.0,
            font_blur: 0.0,
            text_valign: VAlign::default(),
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
	pub fn create_image<A: Into<Rad>>(id: ImageId, cx: f32, cy: f32, w: f32, h: f32, angle: A, alpha: f32) -> Paint {
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
    
    pub fn transform(&self) -> Transform2D {
        self.transform
    }

    pub fn set_transform(&mut self, transform: Transform2D) {
        self.transform = transform;
    }

    pub fn extent(&self) -> [f32; 2] {
        self.extent
    }

    pub fn set_extent(&mut self, extent: [f32; 2]) {
        self.extent = extent;
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    pub fn set_radius(&mut self, radius: f32) {
        self.radius = radius;
    }

    pub fn feather(&self) -> f32 {
        self.feather
    }

    pub fn set_feather(&mut self, feather: f32) {
        self.feather = feather;
    }

    pub fn inner_color(&self) -> Color {
        self.inner_color
    }

    pub fn set_inner_color(&mut self, color: Color) {
        self.inner_color = color;
    }

    pub fn outer_color(&self) -> Color {
        self.outer_color
    }

    pub fn set_outer_color(&mut self, color: Color) {
        self.outer_color = color;
    }
    
    pub fn image(&self) -> Option<ImageId> {
        self.image
    }

    pub fn set_image(&mut self, image: Option<ImageId>) {
        self.image = image;
    }
	
    pub fn set_color(&mut self, color: Color) {
        self.transform = Transform2D::identity();
        self.radius = 0.0;
        self.feather = 1.0;
        self.inner_color = color;
        self.outer_color = color;
    }
    
    /// Returns boolean if the shapes drawn with this paint will be antialiased.
    pub fn shape_anti_alias(&self) -> bool {
        self.shape_anti_alias
    }
    
    /// Sets whether shapes drawn with this paint will be anti aliased. Enabled by default.
    pub fn set_shape_anti_alias(&mut self, value: bool) {
        self.shape_anti_alias = value;
    }
    
    /// Returns the current stroke line width.
    pub fn stroke_width(&self) -> f32 {
        self.stroke_width
    }
    
    /// Sets the stroke width for shapes stroked with this paint.
    pub fn set_stroke_width(&mut self, width: f32) {
        self.stroke_width = width;
    }
    
    /// Getter for the miter limit
    pub fn miter_limit(&self) -> f32 {
        self.miter_limit
    }
    
    /// Sets the limit at which a sharp corner is drawn beveled.
    ///
    /// If the miter at a corner exceeds this limit, LineJoin is replaced with LineJoin::Bevel.
    pub fn set_miter_limit(&mut self, limit: f32) {
        self.miter_limit = limit;
    }
    
    /// Returns the current line cap for this paint.
    pub fn line_cap(&self) -> LineCap {
        self.line_cap
    }
    
    /// Sets how the end of the line (cap) is drawn
    ///
    /// By default it's set to LineCap::Butt
    pub fn set_line_cap(&mut self, cap: LineCap) {
        self.line_cap = cap;
    }
    
    /// Returns the current line join for this paint.
    pub fn line_join(&self) -> LineJoin {
        self.line_join
    }
    
    /// Sets how sharp path corners are drawn.
    ///
    /// By default it's set to LineJoin::Miter
    pub fn set_line_join(&mut self, join: LineJoin) {
        self.line_join = join;
    }
    
    /// Returns the font name that is used when drawing text with this paint
    pub fn font_name(&self) -> &str {
        &self.font_name
    }

    /// Sets the font name for text drawn with this paint
    ///
    /// This needs to be the Fonts postscript name. Eg. "NotoSans-Regular"
    pub fn set_font_name(&mut self, name: String) {
        self.font_name = name;
    }

    pub fn font_size(&self) -> u32 {
        self.font_size
    }

    pub fn set_font_size(&mut self, size: u32) {
        self.font_size = size;
    }

    pub fn letter_spacing(&self) -> f32 {
        self.letter_spacing
    }

    pub fn set_letter_spacing(&mut self, spacing: f32) {
        self.letter_spacing = spacing;
    }

    pub fn font_blur(&self) -> f32 {
        self.font_blur
    }

    pub fn set_font_blur(&mut self, blur: f32) {
        self.font_blur = blur;
    }

    pub fn text_valign(&self) -> VAlign {
        self.text_valign
    }

    pub fn set_text_valign(&mut self, valign: VAlign) {
        self.text_valign = valign;
    }
}
