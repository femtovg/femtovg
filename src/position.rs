use std::ops::{Add, Mul, Neg, Sub};

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Position {
    pub x: f32,
    pub y: f32,
}

impl Position {
    #[inline]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    #[inline]
    pub fn mirror(self) -> Self {
        Self { x: -self.x, y: self.y }
    }

    #[inline]
    pub fn orthogonal(self) -> Self {
        Self { x: self.y, y: -self.x }
    }

    #[inline]
    pub fn from_angle(angle: f32) -> Self {
        Self {
            x: angle.cos(),
            y: angle.sin(),
        }
    }

    #[inline]
    pub fn angle(&self) -> f32 {
        self.y.atan2(self.x)
    }

    pub fn normalize(&mut self) -> f32 {
        let d = (self.x * self.x + self.y * self.y).sqrt();

        if d > 1e-6 {
            let id = 1.0 / d;
            self.x *= id;
            self.y *= id;
        }

        d
    }
}

impl Add for Position {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl Sub for Position {
    type Output = Self;

    #[inline]
    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl Neg for Position {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        Self { x: -self.x, y: -self.y }
    }
}

impl Mul<f32> for Position {
    type Output = Self;

    #[inline]
    fn mul(self, other: f32) -> Self {
        Self {
            x: self.x * other,
            y: self.y * other,
        }
    }
}
