
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
#define scissorRadius frag[12].w
#define conicStartAngle frag[13].x

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
 #define SHADER_TYPE_FillGradientConic 8
 #define SHADER_TYPE_FillImageGradientConic 9
 #define SHADER_TYPE_FilterImageColorMatrix 10
 #define SHADER_TYPE_FillGradientTwoPointRadial 11
 #define SHADER_TYPE_FillImageGradientTwoPointRadial 12

float sdroundrect(vec2 pt, vec2 ext, float rad) {
    vec2 ext2 = ext - vec2(rad,rad);
    vec2 d = abs(pt) - ext2;
    return min(max(d.x,d.y),0.0) + length(max(d,0.0)) - rad;
}

// Scissoring
float scissorMask(vec2 p) {
    if (scissorRadius > 0.0) {
        vec2 pt = (scissorMat * vec3(p,1.0)).xy;
        float distance = sdroundrect(pt, scissorExt, scissorRadius);
        return clamp(0.5 - distance * min(scissorScale.x, scissorScale.y), 0.0, 1.0);
    }

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

// Interleaved gradient noise (Jimenez 2014): a cheap, deterministic, screen-space
// ordered dither. Offsetting the gradient colour by up to +/-0.5 of an 8-bit step
// before the framebuffer quantizes it spreads the rounding spatially, breaking up
// the banding that appears between close colours over a large gradient (issue
// femtovg/femtovg#239). The offset is sub-LSB, so solid regions are unaffected and
// the Unorm write clamps it away at the 0.0/1.0 extremes.
float ditherNoise(vec2 p) {
    return fract(52.9829189 * fract(dot(p, vec2(0.06711056, 0.00583715))));
}
vec4 ditherGradient(vec4 color) {
    float d = (ditherNoise(gl_FragCoord.xy) - 0.5) / 255.0;
    return vec4(color.rgb + d, color.a);
}

vec4 renderGradient() {
    // Calculate gradient color using box gradient
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy;

    float d = clamp((sdroundrect(pt, extent, radius) + feather*0.5) / feather, 0.0, 1.0);
    // Endpoints arrive straight (unpremultiplied) so transparent stops keep
    // their hue through the mix - the interpolation Canvas and SVG gradients
    // apply; premultiply here for the compositing pipeline.
    vec4 color = ditherGradient(mix(innerCol,outerCol,d));
    return vec4(color.rgb * color.a, color.a);
}

// Image-based Gradient; sample a texture using the gradient position.
vec4 renderImageGradient() {
    // Calculate gradient color using box gradient
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy;

    float d = clamp((sdroundrect(pt, extent, radius) + feather*0.5) / feather, 0.0, 1.0);
    return ditherGradient(texture2D(tex, vec2(d, 0.0)));
}

float conicAngleFraction() {
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy;
    // Measure the angle clockwise from the positive x axis. In the gradient's
    // local space (y points down on screen), atan(pt.y, pt.x) increases in the
    // clockwise direction, so offset 0 sits at 3 o'clock and the ramp proceeds
    // clockwise, matching Canvas 2D createConicGradient. fract() wraps the angle
    // into [0, 1) for negative or large start angles.
    return fract((atan(pt.y, pt.x) - conicStartAngle) / TAU);
}

vec4 renderGradientConic() {
    float d = conicAngleFraction();
    // Straight endpoints, premultiplied post-mix (see renderGradient).
    vec4 color = ditherGradient(mix(innerCol,outerCol,d));
    return vec4(color.rgb * color.a, color.a);
}

vec4 renderImageGradientConic() {
    float d = conicAngleFraction();
    return ditherGradient(texture2D(tex, vec2(d, 0.0)));
}

// Two-point (independently centered) radial gradient: the general Canvas
// createRadialGradient(x0,y0,r0, x1,y1,r1). paintMat places the start circle at
// the origin, so in local space the start circle is (0,0) radius r0 and the end
// circle is `extent` away with radius r0 + dr (dr in feather). For a fragment pt
// we solve for the interpolation offset t where |pt - t*cd| = r0 + t*dr, i.e.
// a*t^2 - 2b*t + c = 0. `covered` reports whether any interpolated circle with a
// non-negative radius reaches the fragment; uncovered fragments are transparent.
float radialTwoPointT(out bool covered) {
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy;
    vec2 cd = extent;    // end circle center relative to start
    float r0 = radius;
    float dr = feather;  // r1 - r0
    float a = dot(cd, cd) - dr * dr;
    float b = dot(pt, cd) + r0 * dr;
    float c = dot(pt, pt) - r0 * r0;
    covered = false;
    float t = 0.0;
    // `a` is a squared length, so how close to zero it counts as depends on the
    // size of the shape: the same geometry scaled up scales `a` with the square
    // of the scale factor. Comparing it against a fixed constant would treat a
    // focal point sitting on the end circle as a cone in one scene and as a
    // degenerate in another. Measure it against the terms it came from instead,
    // which also folds in the case where both are zero and the circles coincide.
    float a_scale = max(dot(cd, cd), dr * dr);
    if (a_scale == 0.0) {
        // The two circles are the same circle. There is no sweep to walk, so
        // the whole plane takes the far end of the ramp. The Canvas algorithm
        // says to paint nothing here, but SVG resolved the opposite and its
        // test suite asserts the gradient is drawn, so follow SVG.
        t = 1.0;
        covered = true;
    } else if (abs(a) <= 1e-4 * a_scale) {
        // Degenerate cone (|cd| == |dr|): the quadratic collapses to the linear
        // equation -2b*t + c = 0. This is exactly the case where the end center
        // sits on the start circle, common in focal-style gradients.
        if (abs(b) > 1e-6) {
            t = c / (2.0 * b);
            covered = (r0 + t * dr >= 0.0);
        }
    } else {
        float disc = b * b - a * c;
        // When one circle strictly contains the other (a < 0), every fragment
        // is on some interpolated circle and the discriminant is never really
        // negative: at the focal point it is mathematically zero and rounds
        // just below, which would punch a hole exactly where the first stop
        // belongs. So a negative discriminant only counts as a miss for a > 0.
        if (a < 0.0 || disc >= 0.0) {
            float s = sqrt(max(disc, 0.0));
            // One reciprocal, two roots (a is guaranteed away from zero here).
            float inv_a = 1.0 / a;
            float root_a = (b - s) * inv_a;
            float root_b = (b + s) * inv_a;
            float t_lo = min(root_a, root_b);
            float t_hi = max(root_a, root_b);
            // Canvas draws circles from the largest offset toward the smallest and
            // does not overpaint, so the largest offset whose interpolated circle
            // has a non-negative radius is the one that claims the fragment.
            if (r0 + t_hi * dr >= 0.0) {
                t = t_hi;
                covered = true;
            } else if (r0 + t_lo * dr >= 0.0) {
                t = t_lo;
                covered = true;
            }
        }
    }
    return clamp(t, 0.0, 1.0);
}

vec4 renderGradientTwoPointRadial() {
    bool covered;
    float d = radialTwoPointT(covered);
    if (!covered) return vec4(0.0);
    return ditherGradient(mix(innerCol, outerCol, d));
}

vec4 renderImageGradientTwoPointRadial() {
    bool covered;
    float d = radialTwoPointT(covered);
    if (!covered) return vec4(0.0);
    return ditherGradient(texture2D(tex, vec2(d, 0.0)));
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
    float sampleCount = ceil(3.0 * imageBlurFilterSigma);

    vec3 gaussian_coeff = imageBlurFilterCoeff;

    vec4 color_sum = texture2D(tex, fpos.xy / extent) * gaussian_coeff.x;
    float coefficient_sum = gaussian_coeff.x;
    gaussian_coeff.xy *= gaussian_coeff.yz;

    for (float i = 1.0; i <= 24.0; i += 1.) {
        // Work around GLES 2.0 limitation of only allowing constant loop indices by
        // breaking here. Sigma is clamped to 8 on the Rust side and the kernel reaches
        // +/-3*sigma, so the tap count never exceeds this 24-iteration bound.
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

vec4 renderColorMatrix() {
    // The 4x5 color matrix is packed row-major into frag[0..4] (the scissor/paint
    // matrix slots, unused during a filter pass). Apply it in unpremultiplied
    // sRGB space, clamp to [0,1], then re-premultiply: unpremultiplying avoids
    // edge halos and the clamp keeps overflowing matrices from producing
    // out-of-range or NaN pixels.
    vec4 c = texture2D(tex, fpos.xy / extent);
    if (c.a > 0.0) {
        c.rgb /= c.a;
    }
    float r = frag[0].x * c.r + frag[0].y * c.g + frag[0].z * c.b + frag[0].w * c.a + frag[1].x;
    float g = frag[1].y * c.r + frag[1].z * c.g + frag[1].w * c.b + frag[2].x * c.a + frag[2].y;
    float b = frag[2].z * c.r + frag[2].w * c.g + frag[3].x * c.b + frag[3].y * c.a + frag[3].z;
    float a = frag[3].w * c.r + frag[4].x * c.g + frag[4].y * c.b + frag[4].z * c.a + frag[4].w;
    vec4 outc = clamp(vec4(r, g, b, a), 0.0, 1.0);
    outc.rgb *= outc.a;
    return outc;
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
#elif SELECT_SHADER == SHADER_TYPE_FillGradientConic
    result = renderGradientConic();
#elif SELECT_SHADER == SHADER_TYPE_FillImageGradientConic
    result = renderImageGradientConic();
#elif SELECT_SHADER == SHADER_TYPE_FilterImageColorMatrix
    result = renderColorMatrix();
#elif SELECT_SHADER == SHADER_TYPE_FillGradientTwoPointRadial
    result = renderGradientTwoPointRadial();
#elif SELECT_SHADER == SHADER_TYPE_FillImageGradientTwoPointRadial
    result = renderImageGradientTwoPointRadial();
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
#if SELECT_SHADER != SHADER_TYPE_Stencil && SELECT_SHADER != SHADER_TYPE_FilterImage && SELECT_SHADER != SHADER_TYPE_FilterImageColorMatrix
        // Not stencil fill
        // Combine alpha
        result *= strokeAlpha * scissor;
#endif
#endif

    gl_FragColor = result;
}
