
use std::convert::From;
use std::ops::{Index, IndexMut, Deref, DerefMut};

#[derive(Copy, Clone, Default, Debug, PartialEq, PartialOrd)]
pub struct Rad(pub f32);

impl From<Deg> for Rad {
    fn from(deg: Deg) -> Self {
        Rad(deg.0.to_radians())
    }
}

impl Deref for Rad {
    type Target = f32;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Rad {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Copy, Clone, Default, Debug, PartialEq, PartialOrd)]
pub struct Deg(pub f32);

impl From<Rad> for Deg {
    fn from(rad: Rad) -> Self {
        Deg(rad.0.to_degrees())
    }
}

impl Deref for Deg {
    type Target = f32;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Deg {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub struct Transform2D(pub [f32; 6]);

// TODO: Implement std::ops::* on this
impl Transform2D {

    pub fn identity() -> Self {
        Self([
            1.0, 0.0,
            0.0, 1.0,
            0.0, 0.0
        ])
    }

    pub fn translate(&mut self, tx: f32, ty: f32) {
        self[0] = 1.0; self[1] = 0.0;
        self[2] = 0.0; self[3] = 1.0;
        self[4] = tx; self[5] = ty;
    }

    pub fn scale(&mut self, sx: f32, sy: f32) {
        self[0] = sx; self[1] = 0.0;
        self[2] = 0.0; self[3] = sy;
        self[4] = 0.0; self[5] = 0.0;
    }

    pub fn rotate<R: Into<Rad>>(&mut self, a: R) {
        let a = a.into();
        let cs = a.cos();
        let sn = a.sin();

        self[0] = cs; self[1] = sn;
        self[2] = -sn; self[3] = cs;
        self[4] = 0.0; self[5] = 0.0;
    }

    pub fn skew_x<R: Into<Rad>>(&mut self, a: R) {
        self[0] = 1.0; self[1] = 0.0;
        self[2] = a.into().tan(); self[3] = 1.0;
        self[4] = 0.0; self[5] = 0.0;
    }

    pub fn skew_y<R: Into<Rad>>(&mut self, a: R) {
        self[0] = 1.0; self[1] = a.into().tan();
        self[2] = 0.0; self[3] = 1.0;
        self[4] = 0.0; self[5] = 0.0;
    }

    pub fn multiply(&mut self, other: &Self) {
        let t0 = self[0] * other[0] + self[1] * other[2];
        let t2 = self[2] * other[0] + self[3] * other[2];
        let t4 = self[4] * other[0] + self[5] * other[2] + other[4];
        self[1] = self[0] * other[1] + self[1] * other[3];
        self[3] = self[2] * other[1] + self[3] * other[3];
        self[5] = self[4] * other[1] + self[5] * other[3] + other[5];
        self[0] = t0;
        self[2] = t2;
        self[4] = t4;
    }

    pub fn premultiply(&mut self, other: &Self) {
        let mut other = *other;
        other.multiply(self);
        *self = other;
    }

    pub fn inverse(&mut self) {
        let t = *self;
        let det = t[0] as f64 * t[3] as f64 - t[2] as f64 * t[1] as f64;

        if det > -1e-6 && det < 1e-6 {
            *self = Self::identity();
        }

        let invdet = 1.0 / det;

        self[0] = (t[3] as f64 * invdet) as f32;
        self[2] = (-t[2] as f64 * invdet) as f32;
        self[4] = ((t[2] as f64 * t[5] as f64 - t[3] as f64 * t[4] as f64) * invdet) as f32;
        self[1] = (-t[1] as f64 * invdet) as f32;
        self[3] = (t[0] as f64 * invdet) as f32;
        self[5] = ((t[1] as f64 * t[4] as f64 - t[0] as f64 * t[5] as f64) * invdet) as f32;
    }

    pub fn inversed(&self) -> Self {
        let mut inv = *self;
        inv.inverse();
        inv
    }

    pub fn transform_point(&self, dx: &mut f32, dy: &mut f32, sx: f32, sy: f32) {
        *dx = sx*self[0] + sy*self[2] + self[4];
        *dy = sx*self[1] + sy*self[3] + self[5];
    }

    pub fn average_scale(&self) -> f32 {
        let sx = (self[0]*self[0] + self[2]*self[2]).sqrt();
        let sy = (self[1]*self[1] + self[3]*self[3]).sqrt();

        (sx + sy) * 0.5
    }

    pub fn to_mat3x4(self) -> [f32; 12] {
        [
            self[0], self[1], 0.0, 0.0,
            self[2], self[3], 0.0, 0.0,
            self[4], self[5], 1.0, 0.0,
        ]
    }
}

impl Default for Transform2D {
    fn default() -> Self {
        Self::identity()
    }
}

impl Index<usize> for Transform2D {
    type Output = f32;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl IndexMut<usize> for Transform2D {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

#[derive(Copy, Clone, Default, Debug, PartialEq, PartialOrd)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn intersect(&self, other: Rect) -> Rect {
        let minx = self.x.max(other.x);
        let miny = self.y.max(other.y);
        let maxx = (self.x+self.w).min(other.x+other.w);
        let maxy = (self.y+self.h).min(other.y+other.h);

        Rect::new(minx, miny, 0.0f32.max(maxx - minx), 0.0f32.max(maxy - miny))
    }
}
