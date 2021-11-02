pub struct AnySpace;

pub type Position = euclid::Vector2D<f32, AnySpace>;

pub(crate) trait PositionExt {
    fn mirror(self) -> Self;
    fn orthogonal(self) -> Self;
    fn from_angle(angle: f32) -> Self;
}

impl PositionExt for Position {
    /*
    #[inline]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }
    */

    #[inline]
    fn mirror(self) -> Self {
        Self::new(-self.x, self.y)
    }

    #[inline]
    fn orthogonal(self) -> Self {
        Self::new(self.y, -self.x)
    }

    #[inline]
    fn from_angle(angle: f32) -> Self {
        Self::new(angle.cos(), angle.sin())
    }
}
