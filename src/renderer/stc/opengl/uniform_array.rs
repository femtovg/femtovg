
use super::StcPaint;

const UNIFORMARRAY_SIZE: usize = 11;

pub struct UniformArray([f32; UNIFORMARRAY_SIZE * 4]);

impl Default for UniformArray {
    fn default() -> Self {
        Self([
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ])
    }
}

impl UniformArray {
    pub fn size() -> usize {
        UNIFORMARRAY_SIZE
    }

    pub fn as_ptr(&self) -> *const f32 {
        self.0.as_ptr()
    }

    pub fn set_scissor_mat(&mut self, mat: [f32; 12]) {
        self.0[0..12].copy_from_slice(&mat);
    }

    pub fn set_paint_mat(&mut self, mat: [f32; 12]) {
        self.0[12..24].copy_from_slice(&mat);
    }

    pub fn set_inner_col(&mut self, col: [f32; 4]) {
        self.0[24..28].copy_from_slice(&col);
    }

    pub fn set_outer_col(&mut self, col: [f32; 4]) {
        self.0[28..32].copy_from_slice(&col);
    }

    pub fn set_scissor_ext(&mut self, ext: [f32; 2]) {
        self.0[32..34].copy_from_slice(&ext);
    }

    pub fn set_scissor_scale(&mut self, scale: [f32; 2]) {
        self.0[34..36].copy_from_slice(&scale);
    }

    pub fn set_extent(&mut self, ext: [f32; 2]) {
        self.0[36..38].copy_from_slice(&ext);
    }

    pub fn set_radius(&mut self, radius: f32) {
        self.0[38] = radius;
    }

    pub fn set_feather(&mut self, feather: f32) {
        self.0[39] = feather;
    }

    pub fn set_stroke_mult(&mut self, stroke_mult: f32) {
        self.0[40] = stroke_mult;
    }

    pub fn set_stroke_thr(&mut self, stroke_thr: f32) {
        self.0[41] = stroke_thr;
    }

    pub fn set_tex_type(&mut self, tex_type: f32) {
        self.0[42] = tex_type;
    }

    pub fn set_shader_type(&mut self, shader_type: f32) {
        self.0[43] = shader_type;
    }
}

impl From<StcPaint> for UniformArray {
    fn from(gpu_paint: StcPaint) -> Self {
        let mut arr = Self::default();

        arr.set_scissor_mat(gpu_paint.scissor_mat);
        arr.set_paint_mat(gpu_paint.paint_mat);
        arr.set_inner_col(gpu_paint.inner_col);
        arr.set_outer_col(gpu_paint.outer_col);
        arr.set_scissor_ext(gpu_paint.scissor_ext);
        arr.set_scissor_scale(gpu_paint.scissor_scale);
        arr.set_extent(gpu_paint.extent);
        arr.set_radius(gpu_paint.radius);
        arr.set_feather(gpu_paint.feather);
        arr.set_stroke_mult(gpu_paint.stroke_mult);
        arr.set_stroke_thr(gpu_paint.stroke_thr);
        arr.set_shader_type(gpu_paint.shader_type);
        arr.set_tex_type(gpu_paint.tex_type);

        arr
    }
}
