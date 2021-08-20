
precision highp float;

uniform mat4 scissorMat;
uniform mat4 paintMat;
uniform vec4 innerCol;
uniform vec4 outerCol;
uniform vec2 scissorExt;
uniform vec2 scissorScale;
uniform vec2 extent;
uniform float radius;
uniform float feather;
uniform float strokeMult;
uniform float strokeThr;
uniform int texType;
uniform int shaderType;
uniform int hasMask;
uniform vec2 imageBlurFilterDirection;
uniform float imageBlurFilterSigma;
uniform vec3 imageBlurFilterCoeff;

uniform sampler2D tex;
uniform sampler2D masktex;
uniform vec2 viewSize;

varying vec2 ftcoord;
varying vec2 fpos;

float sdroundrect(vec2 pt, vec2 ext, float rad) {
    vec2 ext2 = ext - vec2(rad,rad);
    vec2 d = abs(pt) - ext2;
    return min(max(d.x,d.y),0.0) + length(max(d,0.0)) - rad;
}

// Scissoring
float scissorMask(vec2 p) {
    vec2 sc = (abs((mat3(scissorMat) * vec3(p,1.0)).xy) - scissorExt);
    sc = vec2(0.5,0.5) - sc * scissorScale;
    return clamp(sc.x,0.0,1.0) * clamp(sc.y,0.0,1.0);
}

#ifdef EDGE_AA
// Stroke - from [0..1] to clipped pyramid, where the slope is 1px.
float strokeMask() {
    return min(1.0, (1.0-abs(ftcoord.x*2.0-1.0))*strokeMult) * min(1.0, ftcoord.y);
    // Using this smoothstep preduces maybe better results when combined with fringe_width of 2, but it may look blurrier
    // maybe this should be controlled via flag
    //return smoothstep(0.0, 1.0, (1.0-abs(ftcoord.x*2.0-1.0))*strokeMult) * smoothstep(0.0, 1.0, ftcoord.y);
}
#endif

void main(void) {
    vec4 result;

    float scissor = scissorMask(fpos);

#ifdef EDGE_AA
    float strokeAlpha = strokeMask();

    if (strokeAlpha < strokeThr) discard;
#else
    float strokeAlpha = 1.0;
#endif

    if (shaderType == 0) {
        // Gradient

        // Calculate gradient color using box gradient
        vec2 pt = (mat3(paintMat) * vec3(fpos, 1.0)).xy;

        float d = clamp((sdroundrect(pt, extent, radius) + feather*0.5) / feather, 0.0, 1.0);
        vec4 color = mix(innerCol,outerCol,d);

        result = color;
    } else if (shaderType == 3) {
        // Image-based Gradient; sample a texture using the gradient position.

        // Calculate gradient color using box gradient
        vec2 pt = (mat3(paintMat) * vec3(fpos, 1.0)).xy;

        float d = clamp((sdroundrect(pt, extent, radius) + feather*0.5) / feather, 0.0, 1.0);
        vec4 color = texture2D(tex, vec2(d, 0.0));//mix(innerCol,outerCol,d);

        result = color;
    } else if (shaderType == 1) {
        // Image

        // Calculate color from texture
        vec2 pt = (mat3(paintMat) * vec3(fpos, 1.0)).xy / extent;

        vec4 color = texture2D(tex, pt);

        if (texType == 1) color = vec4(color.xyz * color.w, color.w);
        if (texType == 2) color = vec4(color.x);

        // Apply color tint and alpha.
        color *= innerCol;

        result = color;
    } else if (shaderType == 2) {
        // Stencil fill
        result = vec4(1,1,1,1);
    } else if (shaderType == 4) {
        // Filter Image

        float sampleCount = ceil(1.5 * imageBlurFilterSigma);

        vec3 gaussian_coeff = imageBlurFilterCoeff;

        vec4 color_sum = texture2D(tex, fpos.xy / extent) * gaussian_coeff.x;
        float coefficient_sum = gaussian_coeff.x;
        gaussian_coeff.xy *= gaussian_coeff.yz;

        for (float i = 1.0; i <= 12.0; i += 1.) {
            // Work around GLES 2.0 limitation of only allowing constant loop indices
            // by breaking here. Sigma has an upper bound of 8, imposed on the Rust side.
            if (i >= sampleCount) {
                break;
            }
            color_sum += texture2D(tex, (fpos.xy - i * imageBlurFilterDirection) / extent) * gaussian_coeff.x;
            color_sum += texture2D(tex, (fpos.xy + i * imageBlurFilterDirection) / extent) * gaussian_coeff.x;
            coefficient_sum += 2.0 * gaussian_coeff.x;

            // Compute the coefficients incrementally:
            // https://developer.nvidia.com/gpugems/gpugems3/part-vi-gpu-computing/chapter-40-incremental-computation-gaussian
            gaussian_coeff.xy *= gaussian_coeff.yz;
        }

        vec4 color = color_sum / coefficient_sum;

        if (texType == 1) color = vec4(color.xyz * color.w, color.w);
        if (texType == 2) color = vec4(color.x);

        result = color;
    }

    if (hasMask == 1) {
        // Textured tris
        vec4 mask = texture2D(masktex, ftcoord);
        mask = vec4(mask.x);

        //if (texType == 1) mask_color = vec4(mask_color.xyz * mask_color.w, mask_color.w);
        //if (texType == 2) mask_color = vec4(mask_color.x);

        mask *= scissor;
        result *= mask;
    } else if (shaderType != 2 && shaderType != 4) { // Not stencil fill
        // Combine alpha
        result *= strokeAlpha * scissor;
    }

    gl_FragColor = result;
}
