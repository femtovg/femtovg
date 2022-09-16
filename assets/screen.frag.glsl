#version 300 es     
precision mediump float;

in vec2 vert;

out vec4 color;

uniform sampler2D screenTexture;

// invert color
void main() {
    vec2 TexCoords = vec2((vert.x + 1.0) / 2.0, (vert.y + 1.0) / 2.0); 
    vec3 col = texture(screenTexture, TexCoords).rgb;
    color = vec4(vec3(1.0 - col), 1.0);
}

