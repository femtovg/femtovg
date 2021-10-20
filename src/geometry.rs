use std::hash::{Hash, Hasher};
use std::ops::{Index, IndexMut};

use fnv::FnvHasher;

pub fn quantize(a: f32, d: f32) -> f32 {
    (a / d + 0.5).trunc() * d
}

pub fn pt_equals(x1: f32, y1: f32, x2: f32, y2: f32, tol: f32) -> bool {
    let dx = x2 - x1;
    let dy = y2 - y1;

    dx * dx + dy * dy < tol * tol
}

pub fn cross(dx0: f32, dy0: f32, dx1: f32, dy1: f32) -> f32 {
    dx1 * dy0 - dx0 * dy1
}

pub fn dist_pt_segment(x: f32, y: f32, px: f32, py: f32, qx: f32, qy: f32) -> f32 {
    let pqx = qx - px;
    let pqy = qy - py;
    let dx = x - px;
    let dy = y - py;
    let d = pqx * pqx + pqy * pqy;
    let mut t = pqx * dx + pqy * dy;

    if d > 0.0 {
        t /= d;
    }

    if t < 0.0 {
        t = 0.0;
    } else if t > 1.0 {
        t = 1.0;
    }

    let dx = px + t * pqx - x;
    let dy = py + t * pqy - y;

    dx * dx + dy * dy
}

// TODO: fix this.. move it to point
pub fn normalize(x: &mut f32, y: &mut f32) -> f32 {
    let d = ((*x) * (*x) + (*y) * (*y)).sqrt();

    if d > 1e-6 {
        let id = 1.0 / d;
        *x *= id;
        *y *= id;
    }

    d
}

/// 2×3 matrix (2 rows, 3 columns) used for 2D linear transformations. It can represent transformations such as translation, rotation, or scaling.
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct Transform2D(pub [f32; 6]);

// TODO: Implement std::ops::* on this
impl Transform2D {
    /// Creates an identity transformation with no translation, rotation or scaling applied.
    pub fn identity() -> Self {
        Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
    }

    pub fn new_translation(x: f32, y: f32) -> Self {
        let mut new = Self::identity();
        new.translate(x, y);
        new
    }

    pub fn translate(&mut self, tx: f32, ty: f32) {
        // self[0] = 1.0; self[1] = 0.0;
        // self[2] = 0.0; self[3] = 1.0;
        self[4] = tx;
        self[5] = ty;
    }

    pub fn scale(&mut self, sx: f32, sy: f32) {
        self[0] = sx;
        self[1] = 0.0;
        self[2] = 0.0;
        self[3] = sy;
        self[4] = 0.0;
        self[5] = 0.0;
    }

    pub fn rotate(&mut self, a: f32) {
        let cs = a.cos();
        let sn = a.sin();

        self[0] = cs;
        self[1] = sn;
        self[2] = -sn;
        self[3] = cs;
        self[4] = 0.0;
        self[5] = 0.0;
    }

    pub fn skew_x(&mut self, a: f32) {
        self[0] = 1.0;
        self[1] = 0.0;
        self[2] = a.tan();
        self[3] = 1.0;
        self[4] = 0.0;
        self[5] = 0.0;
    }

    pub fn skew_y(&mut self, a: f32) {
        self[0] = 1.0;
        self[1] = a.tan();
        self[2] = 0.0;
        self[3] = 1.0;
        self[4] = 0.0;
        self[5] = 0.0;
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

    pub fn transform_point(&self, sx: f32, sy: f32) -> (f32, f32) {
        let dx = sx * self[0] + sy * self[2] + self[4];
        let dy = sx * self[1] + sy * self[3] + self[5];
        (dx, dy)
    }

    pub fn average_scale(&self) -> f32 {
        let sx = (self[0] * self[0] + self[2] * self[2]).sqrt();
        let sy = (self[1] * self[1] + self[3] * self[3]).sqrt();

        (sx + sy) * 0.5
    }

    pub fn to_mat3x4(self) -> [f32; 12] {
        [
            self[0], self[1], 0.0, 0.0, self[2], self[3], 0.0, 0.0, self[4], self[5], 1.0, 0.0,
        ]
    }

    pub fn cache_key(&self) -> u64 {
        let mut hasher = FnvHasher::default();

        for i in 0..6 {
            self.0[i].to_bits().hash(&mut hasher);
        }

        hasher.finish()
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
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn intersect(&self, other: Rect) -> Rect {
        let minx = self.x.max(other.x);
        let miny = self.y.max(other.y);
        let maxx = (self.x + self.w).min(other.x + other.w);
        let maxy = (self.y + self.h).min(other.y + other.h);

        Rect::new(minx, miny, 0.0f32.max(maxx - minx), 0.0f32.max(maxy - miny))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub struct Bounds {
    pub minx: f32,
    pub miny: f32,
    pub maxx: f32,
    pub maxy: f32,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            minx: 1e6,
            miny: 1e6,
            maxx: -1e6,
            maxy: -1e6,
        }
    }
}
