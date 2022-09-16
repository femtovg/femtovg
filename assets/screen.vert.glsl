#version 300 es     
const vec2 verts[6] = vec2[6](
    vec2(-1.0f, 1.0f),
    vec2(-1.0f, -1.0f),
    vec2(1.0f, -1.0f),
    vec2(-1.0f, 1.0f),
    vec2(1.0f, -1.0f),
    vec2(1.0f, 1.0f)
);

out vec2 vert;

void main() {
    vert = verts[gl_VertexID];
    gl_Position = vec4(vert, 0.0, 1.0);
}