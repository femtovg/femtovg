// Copies a rect of a render target into an image, one texel per fragment:
// the source texel is the fragment's own shifted by `offset`,
// with its row negated first when the source holds its rows the other way up.

struct CopyParams {
    offset: vec2<i32>,
    flip_y: i32,
    padding: i32,
}

@group(0) @binding(0) var copy_source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: CopyParams;

// One triangle covering the target; the scissor limits it to the rect.
@vertex
fn vs_copy(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_copy(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(floor(position.xy));
    let row = select(params.offset.y + pixel.y, params.offset.y - pixel.y, params.flip_y != 0);
    return textureLoad(copy_source, vec2<i32>(params.offset.x + pixel.x, row), 0);
}
