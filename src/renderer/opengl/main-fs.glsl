
precision highp float;

#define UNIFORMARRAY_SIZE 14

#define TAU 6.28318530717958647692528676655900577

uniform vec4 frag[UNIFORMARRAY_SIZE];

#define scissorMat mat3(frag[0].xyz, frag[1].xyz, frag[2].xyz)
#define paintMat mat3(frag[3].xyz, frag[4].xyz, frag[5].xyz)
#define innerCol frag[6]
#define outerCol frag[7]
#define scissorExt frag[8].xy
#define scissorScale frag[8].zw
#define extent frag[9].xy
#define radius frag[9].z
#define feather frag[9].w
#define strokeMult frag[10].x
#define strokeThr frag[10].y
#define texType int(frag[10].z)
#define shaderType int(frag[10].w)
#define glyphTextureType int(frag[11].x)
#define imageBlurFilterDirection frag[11].yz
#define imageBlurFilterSigma frag[11].w
#define imageBlurFilterCoeff frag[12].xyz

uniform sampler2D tex;
uniform sampler2D glyphtex;
uniform vec2 viewSize;

varying vec2 ftcoord;
varying vec2 fpos;

 #define SHADER_TYPE_FillGradient 0
 #define SHADER_TYPE_FillImage 1
 #define SHADER_TYPE_Stencil 2
 #define SHADER_TYPE_FillImageGradient 3
 #define SHADER_TYPE_FilterImage 4
 #define SHADER_TYPE_FillColor 5
 #define SHADER_TYPE_TextureCopyUnclipped 6
 #define SHADER_TYPE_FillGradientConical 8
 #define SHADER_TYPE_FillImageGradientConical 9

float sdroundrect(vec2 pt, vec2 ext, float rad) {
    vec2 ext2 = ext - vec2(rad,rad);
    vec2 d = abs(pt) - ext2;
    return min(max(d.x,d.y),0.0) + length(max(d,0.0)) - rad;
}

// Scissoring
float scissorMask(vec2 p) {
    vec2 sc = (abs((scissorMat * vec3(p,1.0)).xy) - scissorExt);
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

vec4 renderGradient() {
    // Calculate gradient color using box gradient
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy;

    float d = clamp((sdroundrect(pt, extent, radius) + feather*0.5) / feather, 0.0, 1.0);
    return mix(innerCol,outerCol,d);
}

// Image-based Gradient; sample a texture using the gradient position.
vec4 renderImageGradient() {
    // Calculate gradient color using box gradient
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy;

    float d = clamp((sdroundrect(pt, extent, radius) + feather*0.5) / feather, 0.0, 1.0);
    return texture2D(tex, vec2(d, 0.0));//mix(innerCol,outerCol,d);
}

float conicalAngleFraction() {
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy;
    // atan returns a value between -pi and pi.
    // normally you'd use atan(pt.y,pt.x) but its switched
    // around here to be clockwise and start from the top.
    return (-atan(pt.x,pt.y) / TAU) + 0.5;
}

vec4 renderGradientConical() {
    float d = conicalAngleFraction();
    return mix(innerCol,outerCol,d);
}

vec4 renderImageGradientConical() {
    float d = conicalAngleFraction();
    return texture2D(tex, vec2(d, 0.0));
}

vec4 renderImage() {
    // Calculate color from texture
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy / extent;

    vec4 color = texture2D(tex, pt);

    if (texType == 1) color = vec4(color.xyz * color.w, color.w);
    if (texType == 2) color = vec4(color.x);

    // Apply color tint and alpha.
    color *= innerCol;
    return color;
}

vec4 renderPlainTextureCopy() {
    vec4 color = texture2D(tex, ftcoord);
    if (texType == 1) color = vec4(color.xyz * color.w, color.w);
    if (texType == 2) color = vec4(color.x);
    // Apply color tint and alpha.
    color *= innerCol;
    return color;
}

vec4 renderFilteredImage() {
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

    return color;
}

void main(void) {
    vec4 result;

#ifdef EDGE_AA
    float strokeAlpha = 1.0;
#if SELECT_SHADER != 6
    strokeAlpha = strokeMask();
    if (strokeAlpha < strokeThr) discard;
#endif
#else
    float strokeAlpha = 1.0;
#endif

#if SELECT_SHADER == SHADER_TYPE_FillGradient
    // Gradient
    result = renderGradient();
#elif SELECT_SHADER == SHADER_TYPE_FillImageGradient
    // Image-based Gradient; sample a texture using the gradient position.
    result = renderImageGradient();
#elif SELECT_SHADER == SHADER_TYPE_FillImage
    // Image
    result = renderImage();
#elif SELECT_SHADER == SHADER_TYPE_FillColor
    // Plain color fill
    result = innerCol;
#elif SELECT_SHADER == SHADER_TYPE_TextureCopyUnclipped
    // Plain texture copy, unclipped
    gl_FragColor = renderPlainTextureCopy();
    return;
#elif SELECT_SHADER == SHADER_TYPE_Stencil
    // Stencil fill
    result = vec4(1,1,1,1);
#elif SELECT_SHADER == SHADER_TYPE_FilterImage
    // Filter Image
    result = renderFilteredImage();
#elif SELECT_SHADER == SHADER_TYPE_FillGradientConical
    result = renderGradientConical();
#elif SELECT_SHADER == SHADER_TYPE_FillImageGradientConical
    result = renderImageGradientConical();
#else
#error A shader variant must be selected with the SELECT_SHADER pre-processor variable
#endif

    float scissor = scissorMask(fpos);

#ifdef ENABLE_GLYPH_TEXTURE
    // Textured tris
    vec4 mask = texture2D(glyphtex, ftcoord);

    if (glyphTextureType == 1) {
        mask = vec4(mask.x);
    } else {
        result = vec4(1, 1, 1, 1);
        mask = vec4(mask.xyz * mask.w, mask.w);
    }

    mask *= scissor;
    result *= mask;
#else
#if SELECT_SHADER != SHADER_TYPE_Stencil && SELECT_SHADER != SHADER_TYPE_FilterImage
        // Not stencil fill
        // Combine alpha
        result *= strokeAlpha * scissor;
#endif
#endif

    gl_FragColor = result;
}
