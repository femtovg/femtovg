use std::{
    hash::{Hash, Hasher},
    ops::{Add, AddAssign, Div, DivAssign, Index, IndexMut, Mul, MulAssign, Neg, Sub, SubAssign},
};

use fnv::FnvHasher;

#[derive(Copy, Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

impl Add<Vector> for Position {
    type Output = Self;

    #[inline]
    fn add(self, other: Vector) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl Sub<Vector> for Position {
    type Output = Self;

    #[inline]
    fn sub(self, other: Vector) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl Sub for Position {
    type Output = Vector;

    #[inline]
    fn sub(self, other: Self) -> Vector {
        Vector {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl Position {
    pub(crate) fn equals(p1: Self, p2: Self, tol: f32) -> bool {
        (p2 - p1).mag2() < tol * tol
    }

    pub(crate) fn segment_distance(pos: Self, ppos: Self, qpos: Self) -> f32 {
        let pq = qpos - ppos;
        let dpos = pos - ppos;
        let d = pq.mag2();
        let mut t = pq.dot(dpos);

        if d > 0.0 {
            t /= d;
        }

        t = t.clamp(0.0, 1.0);

        let dpos = (ppos - pos) + pq * t;

        dpos.mag2()
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Vector {
    pub x: f32,
    pub y: f32,
}

impl Vector {
    pub fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }

    pub fn x(x: f32) -> Self {
        Self { x, y: 0.0 }
    }
    pub fn y(y: f32) -> Self {
        Self { x: 0.0, y }
    }

    pub fn with_basis(self, basis_x: Self, basis_y: Self) -> Self {
        basis_x * self.x + basis_y * self.y
    }

    pub fn cross(self, other: Self) -> f32 {
        self.orthogonal().dot(other)
    }

    #[inline]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    #[inline]
    pub fn mag2(self) -> f32 {
        self.dot(self)
    }

    #[inline]
    pub fn orthogonal(self) -> Self {
        Self { x: self.y, y: -self.x }
    }

    #[inline]
    pub fn from_angle(angle: f32) -> Self {
        let (y, x) = angle.sin_cos();
        Self { x, y }
    }

    #[inline]
    pub fn angle(&self) -> f32 {
        self.y.atan2(self.x)
    }

    pub fn normalize(&mut self) -> f32 {
        let d = self.x.hypot(self.y);

        if d > 1e-6 {
            let id = 1.0 / d;
            self.x *= id;
            self.y *= id;
        }

        d
    }
}

impl Add for Vector {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl Sub for Vector {
    type Output = Self;

    #[inline]
    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl Neg for Vector {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        Self { x: -self.x, y: -self.y }
    }
}

impl Mul<f32> for Vector {
    type Output = Self;

    #[inline]
    fn mul(self, other: f32) -> Self {
        Self {
            x: self.x * other,
            y: self.y * other,
        }
    }
}

impl MulAssign<f32> for Vector {
    #[inline]
    fn mul_assign(&mut self, other: f32) {
        self.x *= other;
        self.y *= other;
    }
}

pub fn quantize(a: f32, d: f32) -> f32 {
    (a / d + 0.5).trunc() * d
}

/// 2×3 matrix (2 rows, 3 columns) used for 2D linear transformations. It can represent transformations such as translation, rotation, or scaling.
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Transform2D(pub [f32; 6]);

impl Transform2D {
    /// Creates an identity transformation with no translation, rotation, or scaling applied.
    pub fn identity() -> Self {
        Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
    }

    /// Creates a new transformation matrix.
    ///
    /// The parameters are interpreted as matrix elements as follows:
    ///   [a c e]
    ///   [b d f]
    ///   [0 0 1]
    pub fn new(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Self {
        Self([a, b, c, d, e, f])
    }

    /// Creates a translation transformation matrix.
    pub fn new_translation(x: f32, y: f32) -> Self {
        let mut new = Self::identity();
        new.translate(x, y);
        new
    }

    /// Sets the translation of the current transformation matrix.
    pub fn translate(&mut self, tx: f32, ty: f32) {
        self[4] = tx;
        self[5] = ty;
    }

    /// Sets the scaling of the current matrix.
    pub fn scale(&mut self, sx: f32, sy: f32) {
        self[0] = sx;
        self[1] = 0.0;
        self[2] = 0.0;
        self[3] = sy;
        self[4] = 0.0;
        self[5] = 0.0;
    }

    /// Set the rotation of the current matrix.
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

    /// Sets the horizontal skew of the current matrix.
    pub fn skew_x(&mut self, a: f32) {
        self[0] = 1.0;
        self[1] = 0.0;
        self[2] = a.tan();
        self[3] = 1.0;
        self[4] = 0.0;
        self[5] = 0.0;
    }

    /// Sets the vertical skew of the current matrix.
    pub fn skew_y(&mut self, a: f32) {
        self[0] = 1.0;
        self[1] = a.tan();
        self[2] = 0.0;
        self[3] = 1.0;
        self[4] = 0.0;
        self[5] = 0.0;
    }

    /// Multiplies the current transformation matrix with another matrix.
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

    /// Premultiplies the current transformation matrix with another matrix.
    pub fn premultiply(&mut self, other: &Self) {
        let mut other = *other;
        other.multiply(self);
        *self = other;
    }

    /// Inverts the current transformation matrix.
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

    /// Returns the inverse of the current transformation matrix.
    pub fn inversed(&self) -> Self {
        let mut inv = *self;
        inv.inverse();
        inv
    }

    /// Transforms a point using the current transformation matrix.
    pub fn transform_point(&self, sx: f32, sy: f32) -> (f32, f32) {
        let dx = sx * self[0] + sy * self[2] + self[4];
        let dy = sx * self[1] + sy * self[3] + self[5];
        (dx, dy)
    }

    /// Calculates the average scale factor of the current transformation matrix.
    pub fn average_scale(&self) -> f32 {
        let sx = self[0].hypot(self[2]);
        let sy = self[1].hypot(self[3]);

        (sx + sy) * 0.5
    }

    /// Converts the current transformation matrix to a 3×4 matrix format.
    pub fn to_mat3x4(self) -> [f32; 12] {
        [
            self[0], self[1], 0.0, 0.0, self[2], self[3], 0.0, 0.0, self[4], self[5], 1.0, 0.0,
        ]
    }

    /// Generates a cache key for the current transformation matrix.
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

impl Add for Transform2D {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self([
            self[0] + rhs[0],
            self[1] + rhs[1],
            self[2] + rhs[2],
            self[3] + rhs[3],
            self[4] + rhs[4],
            self[5] + rhs[5],
        ])
    }
}

impl AddAssign for Transform2D {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Transform2D {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self([
            self[0] - rhs[0],
            self[1] - rhs[1],
            self[2] - rhs[2],
            self[3] - rhs[3],
            self[4] - rhs[4],
            self[5] - rhs[5],
        ])
    }
}

impl SubAssign for Transform2D {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl Mul for Transform2D {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        let t0 = self[0] * rhs[0] + self[1] * rhs[2];
        let t1 = self[0] * rhs[1] + self[1] * rhs[3];
        let t2 = self[2] * rhs[0] + self[3] * rhs[2];
        let t3 = self[2] * rhs[1] + self[3] * rhs[3];
        let t4 = self[4] * rhs[0] + self[5] * rhs[2] + rhs[4];
        let t5 = self[4] * rhs[1] + self[5] * rhs[3] + rhs[5];

        Self([t0, t1, t2, t3, t4, t5])
    }
}

impl MulAssign for Transform2D {
    fn mul_assign(&mut self, rhs: Self) {
        *self = *self * rhs;
    }
}

impl Div for Transform2D {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        self * rhs.inversed()
    }
}

impl DivAssign for Transform2D {
    fn div_assign(&mut self, rhs: Self) {
        *self = *self / rhs;
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

    pub fn intersect(&self, other: Self) -> Self {
        let minx = self.x.max(other.x);
        let miny = self.y.max(other.y);
        let maxx = (self.x + self.w).min(other.x + other.w);
        let maxy = (self.y + self.h).min(other.y + other.h);

        Self::new(minx, miny, 0.0f32.max(maxx - minx), 0.0f32.max(maxy - miny))
    }

    pub fn contains_rect(&self, other: &Self) -> bool {
        other.is_empty()
            || (self.x <= other.x
                && other.x + other.w <= self.x + self.w
                && self.y <= other.y
                && other.y + other.h <= self.y + self.h)
    }

    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let w = (self.x + self.w).min(other.x + other.w) - x;
        let h = (self.y + self.h).min(other.y + other.h) - y;

        let result = Self { x, y, w, h };
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0. || self.h <= 0.
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

impl Bounds {
    pub(crate) fn contains(&self, x: f32, y: f32) -> bool {
        (self.minx..=self.maxx).contains(&x) && (self.miny..=self.maxy).contains(&y)
    }
}
