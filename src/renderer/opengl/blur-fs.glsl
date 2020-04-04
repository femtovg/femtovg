
precision mediump float;

varying vec2 ftcoord;

uniform sampler2D image;

uniform bool horizontal;
uniform vec2 image_size;



void main() {
    float weight[5];
    weight[0] = 0.2270270270;
    weight[1] = 0.1945945946;
    weight[2] = 0.1216216216;
    weight[3] = 0.0540540541;
    weight[4] = 0.0162162162;

    vec2 tex_offset = 1.0 / image_size;
    vec3 result = texture2D(image, ftcoord).rgb * weight[0];

    if(horizontal) {
        for(int i = 1; i < 5; ++i) {
            result += texture2D(image, ftcoord + vec2(tex_offset.x * float(i), 0.0)).rgb * weight[i];
            result += texture2D(image, ftcoord - vec2(tex_offset.x * float(i), 0.0)).rgb * weight[i];
        }
    } else {
        for(int i = 1; i < 5; ++i) {
            result += texture2D(image, ftcoord + vec2(0.0, tex_offset.y * float(i))).rgb * weight[i];
            result += texture2D(image, ftcoord - vec2(0.0, tex_offset.y * float(i))).rgb * weight[i];
        }
    }

    gl_FragColor = vec4(result, 1.0);
}
