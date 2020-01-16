
pub type Vector2D = euclid::default::Vector2D<f32>;
pub type Point2D = euclid::default::Point2D<f32>;
pub type Size2D = euclid::default::Size2D<f32>;
pub type Rect = euclid::default::Rect<f32>;
pub type Transform2D = euclid::default::Transform2D<f32>;
pub type Angle = euclid::Angle<f32>;

// TODO: Revise visibility of these methods
// TODO: Refactor transform API so that it's not awkward to use
// TODO: 3D transform

pub trait Transform2DExt {
    fn average_scale(&self) -> f32;
    fn to_mat3x4(&self) -> [f32; 12];
    fn create_skew_x(a: Angle) -> Self;
    fn create_skew_y(a: Angle) -> Self;
}

impl Transform2DExt for Transform2D {
    fn create_skew_x(a: Angle) -> Self {
        Self::row_major(
            1.0, 0.0,
            a.radians.tan(), 1.0,
            0.0, 0.0
        )
    }

    fn create_skew_y(a: Angle) -> Self {
        Self::row_major(
            1.0, a.radians.tan(),
            0.0, 1.0,
            0.0, 0.0
        )
    }

    fn average_scale(&self) -> f32 {
        let sx = (self.m11*self.m11 + self.m21*self.m21).sqrt();
        let sy = (self.m12*self.m12 + self.m22*self.m22).sqrt();

        (sx + sy) * 0.5
    }

    fn to_mat3x4(&self) -> [f32; 12] {
        [
            self.m11, self.m12, 0.0, 0.0,
            self.m21, self.m22, 0.0, 0.0,
            self.m31, self.m32, 1.0, 0.0,
        ]
    }
}

pub fn quantize(a: f32, d: f32) -> f32 {
    (a / d + 0.5).trunc() * d
}

pub fn triarea2(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let acx = cx - ax;
    let acy = cy - ay;

    acx*aby - abx*acy
}

pub fn pt_equals(x1: f32, y1: f32, x2: f32, y2: f32, tol: f32) -> bool {
    let dx = x2 - x1;
    let dy = y2 - y1;

    dx*dx + dy*dy < tol*tol
}

pub fn cross(dx0: f32, dy0: f32, dx1: f32, dy1: f32) -> f32 {
    dx1*dy0 - dx0*dy1
}

pub fn dist_pt_segment(x: f32, y: f32, px: f32, py: f32, qx: f32, qy: f32) -> f32 {
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

// TODO: fix this.. move it to point
pub fn normalize(x: &mut f32, y: &mut f32) -> f32 {
    let d = ((*x)*(*x) + (*y)*(*y)).sqrt();

    if d > 1e-6 {
        let id = 1.0 / d;
        *x *= id;
        *y *= id;
    }

    d
}
