
attribute vec2 vertex;
attribute vec2 tcoord;

varying vec2 ftcoord;

void main() {
    ftcoord = tcoord;
    gl_Position = vec4(vertex, 0.0, 1.0);
}
