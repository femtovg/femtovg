//! usvg filter chains as femtovg layer passes: the chains this backend can run, the drop-shadow idioms it draws
//! with the canvas shadow, and the `feTurbulence` chains it renders into an image.

use std::sync::Arc;

use femtovg::{BlendMode, Color, CompositeOperation, ImageFilter, Transform2D, TurbulenceKind};
use usvg::filter::{ColorInterpolation, CompositeOperator, Filter, Input, Kind, Primitive};

use crate::scene::ts_to_t2d;

pub type Filters = [Arc<Filter>];

/// The linear part of the map from a group's user space to device pixels: the frame's scale times the group's
/// absolute transform. A filter's parameters are user units along the group's axes; the layer runs in device pixels
/// along the frame's. An offset maps exactly; a blur's deviations and a morphology's radii take the extent of the
/// mapped kernel on each device axis, exact for scales and quarter turns (Blink filters in the element's own space and
/// transforms the result).
#[derive(Clone, Copy)]
pub struct FilterSpace {
    sx: f32,
    kx: f32,
    ky: f32,
    sy: f32,
}

impl FilterSpace {
    pub fn of(group: &usvg::Group, scale: f32) -> Self {
        let t = group.abs_transform();
        Self {
            sx: t.sx * scale,
            kx: t.kx * scale,
            ky: t.ky * scale,
            sy: t.sy * scale,
        }
    }

    /// A user-space vector in device pixels.
    pub fn vector(&self, x: f32, y: f32) -> (f32, f32) {
        (self.sx * x + self.kx * y, self.ky * x + self.sy * y)
    }

    /// A Gaussian's deviations along the device axes: the marginals of the mapped distribution.
    fn deviations(&self, sigma_x: f32, sigma_y: f32) -> (f32, f32) {
        (
            (self.sx * sigma_x).hypot(self.kx * sigma_y),
            (self.ky * sigma_x).hypot(self.sy * sigma_y),
        )
    }

    /// A rectangular kernel's half-extents along the device axes.
    fn radii(&self, radius_x: f32, radius_y: f32) -> (f32, f32) {
        (
            (self.sx * radius_x).abs() + (self.kx * radius_y).abs(),
            (self.ky * radius_x).abs() + (self.sy * radius_y).abs(),
        )
    }

    /// The one deviation a shadow blurs by: the larger device one.
    pub fn shadow_sigma(&self, sigma: f32) -> f32 {
        let (x, y) = self.deviations(sigma, sigma);
        x.max(y)
    }
}

/// The group's filter as layer passes, each in the colour space its primitive asks for: blurs, colour matrices,
/// alpha transfers, morphologies and offsets chained from the source graphic or its alpha. Stops at a primitive it
/// cannot map, before a final `feBlend` (see `blur_blend`), and at a final `feMerge` or `feComposite` that composites
/// the source graphic with the chain's result (`with_source`).
pub struct LayerChain {
    pub passes: Vec<ImageFilter>,
    /// The passes end in linearRGB.
    pub linear: bool,
    /// Nothing was left out.
    pub complete: bool,
    /// The Porter-Duff operator the group's filtered layer and the group itself composite under, and whether the
    /// layer draws first.
    pub with_source: Option<(CompositeOperation, bool)>,
}

impl LayerChain {
    /// The passes as a layer's filters, back in sRGB at the end.
    pub fn filters(&self) -> Vec<ImageFilter> {
        let mut filters = self.passes.clone();
        if self.linear {
            filters.push(color_space_pass(false));
        }
        filters
    }
}

/// How `prim`, with the chain's last result `previous`, composites the source graphic with that result, when it
/// does: the operator, and whether the chain's layer draws first (the source graphic is then the operator's source)
/// or the source graphic draws first and the chain's layer composites with it. `feMerge` of the two with the source
/// last is `over`. `SourceAlpha` serves as the second input of `in` and `out`, which read only its alpha.
fn composites_with_source(prim: &Primitive, previous: Option<&str>) -> Option<(CompositeOperation, bool)> {
    let is_previous = |input: &Input| matches!(input, Input::Reference(r) if Some(r.as_str()) == previous);
    match prim.kind() {
        Kind::Merge(m) => matches!(m.inputs(), [below, Input::SourceGraphic] if is_previous(below))
            .then_some((CompositeOperation::SourceOver, true)),
        Kind::Composite(c) => {
            let op = match c.operator() {
                CompositeOperator::Over => CompositeOperation::SourceOver,
                CompositeOperator::In => CompositeOperation::SourceIn,
                CompositeOperator::Out => CompositeOperation::SourceOut,
                CompositeOperator::Atop => CompositeOperation::Atop,
                CompositeOperator::Xor => CompositeOperation::Xor,
                CompositeOperator::Lighter => CompositeOperation::Lighter,
                CompositeOperator::Arithmetic { .. } => return None,
            };
            let alpha_only = matches!(op, CompositeOperation::SourceIn | CompositeOperation::SourceOut);
            let is_source = |input: &Input| {
                matches!(input, Input::SourceGraphic) || (alpha_only && matches!(input, Input::SourceAlpha))
            };
            if matches!(c.input1(), Input::SourceGraphic) && is_previous(c.input2()) {
                Some((op, true))
            } else if is_previous(c.input1()) && is_source(c.input2()) {
                Some((op, false))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `root` is the canvas transform at the root of the scene, which primitive subregions are cropped in.
pub fn layer_chain(group: &usvg::Group, filters: &Filters, scale: f32, root: Transform2D) -> LayerChain {
    let space = FilterSpace::of(group, scale);
    let mut chain = LayerChain {
        passes: Vec::new(),
        linear: false,
        complete: true,
        with_source: None,
    };
    let [f] = filters else {
        chain.complete = filters.is_empty();
        return chain;
    };
    let mut previous: Option<&str> = None;
    // Fully transparent results (`feFlood` at opacity 0): blending the chain with one is the identity, the export
    // idiom `feFlood -> feBlend in=SourceGraphic in2=flood -> feGaussianBlur`.
    let mut transparent: Vec<&str> = Vec::new();
    let primitives = f.primitives();
    for (index, prim) in primitives.iter().enumerate() {
        if index + 1 == primitives.len() && previous.is_some() {
            if let Some(with_source) = composites_with_source(prim, previous) {
                chain.with_source = Some(with_source);
                break;
            }
        }
        // The chain may start from the source's alpha (the shadow chains of Sketch and Figma).
        let from_previous = |input: &Input| match (input, previous) {
            (Input::SourceGraphic | Input::SourceAlpha, None) => true,
            (Input::Reference(r), Some(p)) => r == p,
            _ => false,
        };
        let is_transparent = |input: &Input| matches!(input, Input::Reference(r) if transparent.contains(&r.as_str()));
        let pass = match prim.kind() {
            Kind::Flood(fl) if fl.opacity().get() <= 0.0 => {
                transparent.push(prim.result());
                continue;
            }
            Kind::Blend(b)
                if (from_previous(b.input1()) && is_transparent(b.input2()))
                    || (from_previous(b.input2()) && is_transparent(b.input1())) =>
            {
                previous = Some(prim.result());
                continue;
            }
            Kind::GaussianBlur(b) if from_previous(b.input()) => {
                let Some(blur) = gaussian_blur(b, space) else {
                    previous = Some(prim.result());
                    continue;
                };
                Some(blur)
            }
            Kind::ColorMatrix(cm) if from_previous(cm.input()) => color_matrix(cm.kind()),
            Kind::ComponentTransfer(ct) if from_previous(ct.input()) => alpha_transfer_matrix(ct),
            Kind::Morphology(m) if from_previous(m.input()) => Some(morphology(m, space)),
            Kind::Offset(o) if from_previous(o.input()) => {
                let (dx, dy) = space.vector(o.dx(), o.dy());
                Some(ImageFilter::Offset { dx, dy })
            }
            _ => None,
        };
        let Some(pass) = pass else {
            chain.complete = matches!(prim.kind(), Kind::Blend(_));
            break;
        };
        if previous.is_none() && matches!(primary_input(prim), Some(Input::SourceAlpha)) {
            chain.passes.push(source_alpha());
        }
        let linear = prim.color_interpolation() == ColorInterpolation::LinearRGB;
        if linear != chain.linear {
            chain.passes.push(color_space_pass(linear));
            chain.linear = linear;
        }
        chain.passes.push(pass);
        chain.passes.extend(primitive_crop(group, f, prim, root));
        previous = Some(prim.result());
    }
    // A chain this backend cannot finish does not run at all: its first passes alone (an offset without the
    // composite that reads it, a blur without the merge) land further from a browser than the source drawn raw.
    // A chain that stops at an `feBlend` has run (see `complete`).
    if !chain.complete {
        chain.passes.clear();
        chain.linear = false;
    }
    chain
}

/// The input a one-input primitive reads.
fn primary_input(prim: &Primitive) -> Option<&Input> {
    match prim.kind() {
        Kind::GaussianBlur(k) => Some(k.input()),
        Kind::ColorMatrix(k) => Some(k.input()),
        Kind::ComponentTransfer(k) => Some(k.input()),
        Kind::Morphology(k) => Some(k.input()),
        Kind::Offset(k) => Some(k.input()),
        _ => None,
    }
}

/// `SourceAlpha` as a pass: the colour matrix that keeps alpha and zeroes the colour.
fn source_alpha() -> ImageFilter {
    let mut matrix = [0.0f32; 20];
    matrix[18] = 1.0;
    ImageFilter::ColorMatrix { matrix }
}

/// An `feMorphology` in device pixels.
fn morphology(m: &usvg::filter::Morphology, space: FilterSpace) -> ImageFilter {
    let (radius_x, radius_y) = space.radii(m.radius_x().get(), m.radius_y().get());
    ImageFilter::Morphology {
        radius_x,
        radius_y,
        operator: match m.operator() {
            usvg::filter::MorphologyOperator::Erode => femtovg::MorphologyOperator::Erode,
            usvg::filter::MorphologyOperator::Dilate => femtovg::MorphologyOperator::Dilate,
        },
    }
}

/// The pass into (`to_linear`) or out of linearRGB.
pub fn color_space_pass(to_linear: bool) -> ImageFilter {
    if to_linear {
        ImageFilter::SrgbToLinearRgb
    } else {
        ImageFilter::LinearRgbToSrgb
    }
}

fn color_matrix(kind: &usvg::filter::ColorMatrixKind) -> Option<ImageFilter> {
    use usvg::filter::ColorMatrixKind;
    match kind {
        ColorMatrixKind::Matrix(v) => Some(ImageFilter::ColorMatrix {
            matrix: v[..20].try_into().ok()?,
        }),
        ColorMatrixKind::Saturate(s) => Some(ImageFilter::saturate(s.get())),
        ColorMatrixKind::HueRotate(_) | ColorMatrixKind::LuminanceToAlpha => None,
    }
}

/// A component transfer that leaves colour alone and maps alpha affinely (`feFuncA type="linear"` or a two-entry
/// `table`) as the colour matrix row `a' = slope * a + intercept`. Anything else is not translated.
fn alpha_transfer_matrix(ct: &usvg::filter::ComponentTransfer) -> Option<ImageFilter> {
    use usvg::filter::TransferFunction;
    if !matches!(ct.func_r(), TransferFunction::Identity)
        || !matches!(ct.func_g(), TransferFunction::Identity)
        || !matches!(ct.func_b(), TransferFunction::Identity)
    {
        return None;
    }
    let (slope, intercept) = match ct.func_a() {
        TransferFunction::Identity => (1.0, 0.0),
        TransferFunction::Linear { slope, intercept } => (*slope, *intercept),
        TransferFunction::Table(values) if values.len() == 2 => (values[1] - values[0], values[0]),
        _ => return None,
    };
    let mut matrix = [0.0f32; 20];
    matrix[0] = 1.0;
    matrix[6] = 1.0;
    matrix[12] = 1.0;
    matrix[18] = slope;
    matrix[19] = intercept;
    Some(ImageFilter::ColorMatrix { matrix })
}

/// The device-space bounds of the corners of `r` under `t`.
fn device_bounds(t: &Transform2D, r: usvg::Rect) -> (f32, f32, f32, f32) {
    let corners = [
        (r.x(), r.y()),
        (r.right(), r.y()),
        (r.x(), r.bottom()),
        (r.right(), r.bottom()),
    ]
    .map(|(x, y)| t.transform_point(x, y));
    let x0 = corners.iter().map(|c| c.0).fold(f32::INFINITY, f32::min);
    let y0 = corners.iter().map(|c| c.1).fold(f32::INFINITY, f32::min);
    let x1 = corners.iter().map(|c| c.0).fold(f32::NEG_INFINITY, f32::max);
    let y1 = corners.iter().map(|c| c.1).fold(f32::NEG_INFINITY, f32::max);
    (x0, y0, x1, y1)
}

/// The crop a primitive's result takes: its subregion within the filter region, in root device pixels. Every
/// primitive's result is clipped to it, not only the chain's last.
fn primitive_crop(group: &usvg::Group, f: &Filter, prim: &Primitive, root: Transform2D) -> Option<ImageFilter> {
    let ts = group.abs_transform();
    let (region, subregion) = (f.rect().transform(ts)?, prim.rect().transform(ts)?);
    let Some(r) = region.to_rect().intersect(&subregion.to_rect()) else {
        // Nothing of the result is inside both.
        return Some(ImageFilter::Crop {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        });
    };
    let (x0, y0, x1, y1) = device_bounds(&root, r);
    Some(ImageFilter::Crop {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

/// The rects a browser hard-clips a filtered group to, in root space: the filter region bounds the source graphic
/// (Blink `FilterEffect::ApplyBounds` on `SourceGraphic`), the last primitive's subregion within the region bounds
/// the result.
pub fn filter_region(group: &usvg::Group, filters: &Filters) -> Option<(usvg::NonZeroRect, usvg::NonZeroRect)> {
    let [f] = filters else { return None };
    let ts = group.abs_transform();
    let region = f.rect().transform(ts)?;
    let last = f.primitives().last()?.rect().transform(ts)?;
    let result = region.to_rect().intersect(&last.to_rect())?.to_non_zero_rect()?;
    Some((region, result))
}

/// Scissors to `r`, snapped outward to whole device pixels: Blink sizes a filter's raster to the enclosing pixel
/// rect of the region, so an edge pixel the region only partly covers is still filled in full.
pub fn scissor_to(canvas: &mut crate::scene::Canvas, r: usvg::NonZeroRect) {
    let t = canvas.transform();
    let (x0, y0, x1, y1) = device_bounds(&t, r.to_rect());
    let (x0, y0, x1, y1) = (x0.floor(), y0.floor(), x1.ceil(), y1.ceil());
    canvas.reset_transform();
    canvas.intersect_scissor(x0, y0, x1 - x0, y1 - y0);
    canvas.set_transform(&t);
}

/// The group's `feGaussianBlur` primitives as layer filters, in device pixels.
pub fn blur_filters(group: &usvg::Group, filters: &Filters, scale: f32) -> Vec<ImageFilter> {
    let space = FilterSpace::of(group, scale);
    filters
        .iter()
        .flat_map(|f| f.primitives())
        .filter_map(|prim| match prim.kind() {
            Kind::GaussianBlur(b) => gaussian_blur(b, space),
            _ => None,
        })
        .collect()
}

/// An `feGaussianBlur` in device pixels; `None` when neither axis blurs.
fn gaussian_blur(b: &usvg::filter::GaussianBlur, space: FilterSpace) -> Option<ImageFilter> {
    let (sigma_x, sigma_y) = space.deviations(b.std_dev_x().get(), b.std_dev_y().get());
    if sigma_x <= 0.0 && sigma_y <= 0.0 {
        return None;
    }
    Some(ImageFilter::GaussianBlur { sigma_x, sigma_y })
}

pub fn isotropic_blur(sigma: f32) -> ImageFilter {
    ImageFilter::GaussianBlur {
        sigma_x: sigma,
        sigma_y: sigma,
    }
}

/// The glow idiom: `feGaussianBlur` then `feComposite operator="over" in="SourceGraphic" in2=<the blur>`, the group
/// sharp over its own blur.
pub fn glow(filters: &Filters) -> bool {
    let [f] = filters else { return false };
    let [blur, composite] = f.primitives() else {
        return false;
    };
    matches!(blur.kind(), Kind::GaussianBlur(b) if matches!(b.input(), Input::SourceGraphic))
        && matches!(composite.kind(), Kind::Composite(c)
            if c.operator() == CompositeOperator::Over
                && matches!(c.input1(), Input::SourceGraphic)
                && matches!(c.input2(), Input::Reference(name) if name == blur.result()))
}

/// Whether any group in the subtree composites with a blend mode.
pub fn has_blend_group(group: &usvg::Group) -> bool {
    group.children().iter().any(|node| match node {
        usvg::Node::Group(g) => g.blend_mode() != usvg::BlendMode::Normal || has_blend_group(g),
        _ => false,
    })
}

/// Does this filter replace its source graphic rather than augment it? A chain that never reads `SourceGraphic` or
/// `SourceAlpha` synthesises its output from nothing (`feTurbulence`, `feFlood`, `feImage`), so the shape it hangs
/// off is scaffolding, typically a fill-less rect that would paint black. A backend that cannot run such a chain
/// must drop the subtree; a chain that reads the source only decorates it, so the subtree is still drawn.
pub fn replaces_source(f: &Filter) -> bool {
    let consumes = |i: &Input| matches!(i, Input::SourceGraphic | Input::SourceAlpha);
    !f.primitives().iter().any(|p| match p.kind() {
        Kind::Blend(k) => consumes(k.input1()) || consumes(k.input2()),
        Kind::Composite(k) => consumes(k.input1()) || consumes(k.input2()),
        Kind::DisplacementMap(k) => consumes(k.input1()) || consumes(k.input2()),
        Kind::ColorMatrix(k) => consumes(k.input()),
        Kind::ComponentTransfer(k) => consumes(k.input()),
        Kind::DropShadow(k) => consumes(k.input()),
        Kind::GaussianBlur(k) => consumes(k.input()),
        Kind::Morphology(k) => consumes(k.input()),
        Kind::Offset(k) => consumes(k.input()),
        Kind::Tile(k) => consumes(k.input()),
        Kind::ConvolveMatrix(k) => consumes(k.input()),
        Kind::DiffuseLighting(k) => consumes(k.input()),
        Kind::SpecularLighting(k) => consumes(k.input()),
        Kind::Merge(k) => k.inputs().iter().any(consumes),
        Kind::Flood(_) | Kind::Image(_) | Kind::Turbulence(_) => false,
    })
}

/// A drop shadow of the source graphic: `feDropShadow`, or the chain it expands to written out
/// (`feGaussianBlur in="SourceAlpha"`, `feOffset`, `feFlood`, `feComposite operator="in"`), with an `feMerge` that
/// puts the source back over it (`merged`) or without one, when the group draws only its shadow.
pub struct ShadowChain {
    pub dx: f32,
    pub dy: f32,
    pub sigma: f32,
    /// The shadow's colour in sRGB.
    pub color: Color,
    pub merged: bool,
    /// The merge runs in linearRGB (color-interpolation-filters' default): the shadow's colour there.
    pub linear_color: Option<Color>,
}

/// An sRGB-encoded colour's linearRGB value, alpha kept.
fn linear_rgb(c: Color) -> Color {
    Color::rgbaf(srgb_to_linear(c.r), srgb_to_linear(c.g), srgb_to_linear(c.b), c.a)
}

pub fn srgb_to_linear(x: f32) -> f32 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

pub fn drop_shadow(filters: &Filters) -> Option<ShadowChain> {
    let [f] = filters else { return None };
    let shorthand = f.primitives().iter().find_map(|p| match p.kind() {
        Kind::DropShadow(ds) if matches!(ds.input(), Input::SourceGraphic) => {
            Some((ds, p.color_interpolation() == ColorInterpolation::LinearRGB))
        }
        _ => None,
    });
    if let Some((ds, linear)) = shorthand {
        let color = flood_color(ds.color(), ds.opacity());
        return Some(ShadowChain {
            dx: ds.dx(),
            dy: ds.dy(),
            sigma: ds.std_dev_x().get().max(ds.std_dev_y().get()),
            color: skia_drop_shadow_color(color),
            merged: true,
            linear_color: linear.then(|| skia_drop_shadow_color(linear_rgb(color))),
        });
    }
    shadow_chain(f.primitives())
}

/// A linearRGB colour's sRGB encoding, alpha kept, rounded to 8 bits as `matrix_shadow_color` rounds an sRGB
/// matrix's constants.
fn srgb_encoded(c: Color) -> Color {
    let channel = |x: f32| {
        let s = if x <= 0.003_130_8 {
            x * 12.92
        } else {
            1.055 * x.powf(1.0 / 2.4) - 0.055
        };
        (s.clamp(0.0, 1.0) * 255.0).round() / 255.0
    };
    Color::rgbaf(channel(c.r), channel(c.g), channel(c.b), c.a)
}

/// The shadow colour of an `feDropShadow` as Chromium draws it. Skia's drop shadow colours its input before the
/// blur, in 8 bits premultiplied, so the blur spreads the flood colour's 8-bit premultiplied value: a channel that is
/// a fraction of a step there, a dark colour in linearRGB, spreads as a whole step or none.
fn skia_drop_shadow_color(c: Color) -> Color {
    let alpha = (c.a * 255.0).round();
    if alpha == 0.0 {
        return Color::rgbaf(0.0, 0.0, 0.0, 0.0);
    }
    let channel = |x: f32| ((x * c.a * 255.0).round() / alpha).min(1.0);
    Color::rgbaf(channel(c.r), channel(c.g), channel(c.b), alpha / 255.0)
}

fn flood_color(c: usvg::Color, opacity: usvg::Opacity) -> Color {
    let mut color = Color::rgb(c.red, c.green, c.blue);
    color.set_alphaf(opacity.get());
    color
}

/// The written-out chain: the blur of `SourceAlpha`, then in either order its offset (optional) and its colouring
/// (optional: `feFlood` with `feComposite operator="in"`, or an `feColorMatrix` that sets a constant colour and
/// scales alpha; without one the silhouette is `SourceAlpha`'s opaque black), and after them nothing or the merge
/// that puts the source back over the shadow. The offset may instead come before the blur: a translation and a blur
/// commute.
fn shadow_chain(prims: &[Primitive]) -> Option<ShadowChain> {
    let by_result = |input: &Input| match input {
        Input::Reference(name) => prims.iter().find(|p| p.result() == name),
        _ => None,
    };
    let (chain, merged) = match prims.last()?.kind() {
        Kind::Merge(m) => {
            let [Input::Reference(shadow), Input::SourceGraphic] = m.inputs() else {
                return None;
            };
            let chain = &prims[..prims.len() - 1];
            if chain.last()?.result() != shadow {
                return None;
            }
            (chain, true)
        }
        _ => (prims, false),
    };
    let merge_linear = merged && prims.last()?.color_interpolation() == ColorInterpolation::LinearRGB;
    // Peel the offset and the colouring off the end, each at most once, down to the blur; `accounted` counts the
    // primitives they are.
    let mut cursor = chain.last()?;
    let mut shift: Option<(f32, f32)> = None;
    let mut color: Option<Color> = None;
    // The colouring in linearRGB: a flood colour converted from sRGB; a matrix's constants as they come out of it
    // when the matrix runs in linearRGB, converted when it runs in sRGB.
    let mut linear: Option<Color> = None;
    let mut accounted = 1;
    loop {
        match cursor.kind() {
            Kind::Offset(o) if shift.is_none() => {
                shift = Some((o.dx(), o.dy()));
                cursor = by_result(o.input())?;
                accounted += 1;
            }
            Kind::Composite(c) if color.is_none() && c.operator() == CompositeOperator::In => {
                let Kind::Flood(flood) = by_result(c.input1())?.kind() else {
                    return None;
                };
                let flood = flood_color(flood.color(), flood.opacity());
                color = Some(flood);
                linear = Some(linear_rgb(flood));
                cursor = by_result(c.input2())?;
                accounted += 2;
            }
            Kind::ColorMatrix(cm) if color.is_none() => {
                let constants = matrix_shadow_color(cm.kind())?;
                // A matrix that runs in linearRGB sets linear values: the colour it paints in sRGB is their
                // encoding, as the browsers draw it.
                if cursor.color_interpolation() == ColorInterpolation::LinearRGB {
                    let constants = matrix_constants(cm.kind())?;
                    color = Some(srgb_encoded(constants));
                    linear = Some(constants);
                } else {
                    color = Some(constants);
                    linear = Some(linear_rgb(constants));
                }
                cursor = by_result(cm.input())?;
                accounted += 1;
            }
            Kind::GaussianBlur(blur) => {
                let input = match (blur.input(), shift) {
                    (Input::Reference(_), None) => match by_result(blur.input())?.kind() {
                        Kind::Offset(o) => {
                            shift = Some((o.dx(), o.dy()));
                            accounted += 1;
                            o.input()
                        }
                        _ => return None,
                    },
                    (input, _) => input,
                };
                if !matches!(input, Input::SourceAlpha) || chain.len() != accounted {
                    return None;
                }
                let (dx, dy) = shift.unwrap_or((0.0, 0.0));
                return Some(ShadowChain {
                    dx,
                    dy,
                    sigma: blur.std_dev_x().get().max(blur.std_dev_y().get()),
                    color: color.unwrap_or(Color::rgb(0, 0, 0)),
                    merged,
                    linear_color: merge_linear.then(|| linear.unwrap_or(Color::rgb(0, 0, 0))),
                });
            }
            _ => return None,
        }
    }
}

/// The colour an `feColorMatrix` paints a silhouette: a matrix whose colour rows are constants (`0 0 0 0 r`, ...)
/// and whose alpha row scales alpha (`0 0 0 k 0`) gives (r, g, b) at opacity k; any other matrix is no colouring.
fn matrix_shadow_color(kind: &usvg::filter::ColorMatrixKind) -> Option<Color> {
    let usvg::filter::ColorMatrixKind::Matrix(v) = kind else {
        return None;
    };
    let m: &[f32; 20] = v[..20].try_into().ok()?;
    let constant_row = |row: usize| m[row * 5..row * 5 + 4].iter().all(|c| *c == 0.0);
    if !(constant_row(0) && constant_row(1) && constant_row(2)) || m[15..18].iter().any(|c| *c != 0.0) || m[19] != 0.0 {
        return None;
    }
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut color = Color::rgb(channel(m[4]), channel(m[9]), channel(m[14]));
    color.set_alphaf(m[18].clamp(0.0, 1.0));
    Some(color)
}

/// A colouring matrix's constants (see `matrix_shadow_color`) unrounded: the colour in the matrix's own space.
fn matrix_constants(kind: &usvg::filter::ColorMatrixKind) -> Option<Color> {
    let usvg::filter::ColorMatrixKind::Matrix(v) = kind else {
        return None;
    };
    let unit = |i: usize| v.get(i).map(|c| c.clamp(0.0, 1.0));
    Some(Color::rgbaf(unit(4)?, unit(9)?, unit(14)?, unit(18)?))
}

/// How a merged shadow composites with its group. The group's layer receives the merge (the shadow cast by an inner
/// layer that holds the source) and applies the group's opacity, mask and blend to that result, which is what SVG
/// applies them to. When the merge runs in linearRGB, the default, the inner layer converts the source into
/// linearRGB, the shadow is cast in the merge's colour, and the group's layer converts the result back to sRGB, as
/// both Chromium and Firefox do through 8-bit linear intermediates.
pub struct ShadowMerge {
    /// The shadow's colour in the merge's colour space.
    pub color: Color,
    /// The inner layer's passes: the source into linearRGB, or none.
    pub to_merge_space: Vec<ImageFilter>,
}

pub fn shadow_merge(shadow: &ShadowChain) -> ShadowMerge {
    match shadow.linear_color {
        Some(color) => ShadowMerge {
            color,
            to_merge_space: vec![color_space_pass(true)],
        },
        None => ShadowMerge {
            color: shadow.color,
            to_merge_space: Vec::new(),
        },
    }
}

/// A blur followed by `feBlend in=<the blur> in2=SourceGraphic|SourceAlpha`: the blurred group blended over itself
/// (or its silhouette).
pub struct BlurBlend {
    pub mode: usvg::BlendMode,
    pub alpha_only: bool,
    pub linear: bool,
}

pub fn blur_blend(filters: &Filters) -> Option<BlurBlend> {
    let [f] = filters else { return None };
    let [blur, blend] = f.primitives() else { return None };
    let (Kind::GaussianBlur(b), Kind::Blend(k)) = (blur.kind(), blend.kind()) else {
        return None;
    };
    if !matches!(b.input(), Input::SourceGraphic) || !matches!(k.input1(), Input::Reference(r) if r == blur.result()) {
        return None;
    }
    let alpha_only = match k.input2() {
        Input::SourceGraphic => false,
        Input::SourceAlpha => true,
        Input::Reference(_) => return None,
    };
    Some(BlurBlend {
        mode: k.mode(),
        alpha_only,
        linear: blend.color_interpolation() == ColorInterpolation::LinearRGB,
    })
}

pub fn blend_mode(mode: usvg::BlendMode) -> BlendMode {
    use usvg::BlendMode as M;
    match mode {
        M::Normal => BlendMode::Normal,
        M::Multiply => BlendMode::Multiply,
        M::Screen => BlendMode::Screen,
        M::Overlay => BlendMode::Overlay,
        M::Darken => BlendMode::Darken,
        M::Lighten => BlendMode::Lighten,
        M::ColorDodge => BlendMode::ColorDodge,
        M::ColorBurn => BlendMode::ColorBurn,
        M::HardLight => BlendMode::HardLight,
        M::SoftLight => BlendMode::SoftLight,
        M::Difference => BlendMode::Difference,
        M::Exclusion => BlendMode::Exclusion,
        M::Hue => BlendMode::Hue,
        M::Saturation => BlendMode::Saturation,
        M::Color => BlendMode::Color,
        M::Luminosity => BlendMode::Luminosity,
        M::PlusLighter => BlendMode::PlusLighter,
    }
}

/// The separable modes whose blend function is symmetric in its inputs.
fn blend_commutes(mode: usvg::BlendMode) -> bool {
    use usvg::BlendMode as M;
    matches!(
        mode,
        M::Multiply | M::Screen | M::Darken | M::Lighten | M::Difference | M::Exclusion
    )
}

/// An `feTurbulence` (or `feFlood`) chain this backend runs as a whole into an image of its own.
pub struct NoisePlan {
    pub chain: Vec<ImageFilter>,
    /// Device-pixel size of the noise image.
    pub size: (f32, f32),
    /// That image's rect in the group's user space, so it is drawn through the same transform as the group's paths:
    /// a layer shifts device space to its own origin, so device coordinates are not stable.
    pub user_rect: (f32, f32, f32, f32),
    pub abs_transform: Transform2D,
    /// The canvas transform to draw the source's nodes under so they land on the noise image's pixels.
    pub stencil_transform: Transform2D,
    /// The result is stencilled by the source (`feComposite operator="in" in2=Source*`) rather than replacing it.
    pub stencil: bool,
    /// The chain starts with `feFlood`: the image is filled with this colour (already in the blend's colour space)
    /// over the primitive's subregion, and `chain` is empty.
    pub flood: Option<Color>,
    pub blend: Option<BlendPlan>,
}

/// The chain ends in `feBlend` (or `feComposite operator="lighter"`) between the noise and the source graphic.
#[derive(Clone, Copy)]
pub struct BlendPlan {
    pub mode: BlendMode,
    pub linear: bool,
    /// The noise's rect in root device space, for the layer's blend pass.
    pub root_rect: (f32, f32, f32, f32),
    /// `feComposite operator="in" in2=Source*` stencilled the noise before the blend.
    pub stencilled: bool,
    /// The noise is `in` and the source graphic `in2` of a mode where the order matters: the noise is the layer's
    /// content and the group's precaptured source its backdrop.
    pub swapped: bool,
}

/// Translates a group's filter into a NoisePlan when it is
/// `feTurbulence|feFlood -> (feColorMatrix | feComponentTransfer)* [-> feComposite in Source*] [-> feBlend]`. The
/// chain runs in the primitives' color-interpolation-filters space and ends with the transfer back to sRGB, as a
/// browser's does. `canvas_transform` is the canvas's current transform, `root` the scene root's.
pub fn turbulence_plan(
    canvas_transform: Transform2D,
    root: Transform2D,
    frame: (u32, u32),
    group: &usvg::Group,
    filters: &Filters,
) -> Option<NoisePlan> {
    let [f] = filters else { return None };
    let prims = f.primitives();
    let (turbulence, flood) = match prims.first()?.kind() {
        Kind::Turbulence(t) => (Some(t), None),
        Kind::Flood(fl) => (None, Some(fl)),
        _ => return None,
    };

    // The region in device pixels, clamped to the frame, and the transform that maps the group's user space (where
    // the noise lives) onto that image's pixels. A noise chain fills the filter region, a flood its own subregion.
    let abs = ts_to_t2d(group.abs_transform());
    let mut device = canvas_transform;
    device.premultiply(&abs);
    let r = if flood.is_some() { prims[0].rect() } else { f.rect() };
    let corners = [
        (r.x(), r.y()),
        (r.right(), r.y()),
        (r.x(), r.bottom()),
        (r.right(), r.bottom()),
    ]
    .map(|(x, y)| device.transform_point(x, y));
    // Inside a layer the canvas transform carries the store's shift: a store that starts above or left of the frame
    // (an ancestor's blur or shadow reach) puts the frame's corner at that shift, not at the origin. The store
    // reaches about as far past the far sides, so the far clamp moves out by twice the shift.
    let (shift_x, shift_y) = {
        let mut root = root;
        root.premultiply(&abs);
        let (root_x, root_y) = root.transform_point(r.x(), r.y());
        (corners[0].0 - root_x, corners[0].1 - root_y)
    };
    let x0 = corners
        .iter()
        .map(|c| c.0)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0);
    let y0 = corners
        .iter()
        .map(|c| c.1)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0);
    let x1 = corners
        .iter()
        .map(|c| c.0)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(frame.0 as f32 + (2.0 * shift_x).max(0.0).ceil());
    let y1 = corners
        .iter()
        .map(|c| c.1)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(frame.1 as f32 + (2.0 * shift_y).max(0.0).ceil());
    if !(x1 > x0 && y1 > y0) {
        return None;
    }
    let mut noise_transform = Transform2D::translation(-x0, -y0);
    noise_transform.premultiply(&device);
    let mut stencil_transform = Transform2D::translation(-x0, -y0);
    stencil_transform.premultiply(&canvas_transform);
    // The pixel rect back in user space, so the image lands 1:1 on the pixels it was made for.
    let inverse = device.inverse();
    let (ux0, uy0) = inverse.transform_point(x0, y0);
    let (ux1, uy1) = inverse.transform_point(x1, y1);
    let user_rect = (ux0.min(ux1), uy0.min(uy1), (ux1 - ux0).abs(), (uy1 - uy0).abs());
    let root_rect = || {
        let mut root = root;
        root.premultiply(&abs);
        let (rx0, ry0) = root.transform_point(ux0.min(ux1), uy0.min(uy1));
        let (rx1, ry1) = root.transform_point(ux0.max(ux1), uy0.max(uy1));
        (rx0.min(rx1), ry0.min(ry1), (rx1 - rx0).abs(), (ry1 - ry0).abs())
    };

    let mut chain = match turbulence {
        Some(t) => vec![ImageFilter::Turbulence {
            base_frequency: [t.base_frequency_x().get(), t.base_frequency_y().get()],
            num_octaves: t.num_octaves(),
            seed: t.seed(),
            stitch_tiles: t.stitch_tiles(),
            kind: match t.kind() {
                usvg::filter::TurbulenceKind::FractalNoise => TurbulenceKind::FractalNoise,
                usvg::filter::TurbulenceKind::Turbulence => TurbulenceKind::Turbulence,
            },
            transform: noise_transform,
        }],
        None => Vec::new(),
    };
    let mut interpolation = prims[0].color_interpolation();
    let mut stencil = false;
    let mut blend: Option<BlendPlan> = None;
    for (i, p) in prims.iter().enumerate().skip(1) {
        let previous = prims[i - 1].result();
        let last = i == prims.len() - 1;
        let from_previous = |input: &Input| matches!(input, Input::Reference(name) if name == previous);
        // The noise and the source graphic, in either order.
        let with_source = |a: &Input, b: &Input| {
            (from_previous(a) && matches!(b, Input::SourceGraphic))
                || (matches!(a, Input::SourceGraphic) && from_previous(b))
        };
        match p.kind() {
            Kind::ColorMatrix(cm) if from_previous(cm.input()) => {
                chain.push(color_matrix(cm.kind())?);
                interpolation = p.color_interpolation();
            }
            // `feComposite in` against the source stencils the noise by the source's alpha: last, the plan draws
            // the noise SourceIn; before a blend, the source is drawn into the noise DestinationIn first.
            Kind::Composite(c)
                if (last || matches!(prims[i + 1].kind(), Kind::Blend(_)))
                    && matches!(c.operator(), CompositeOperator::In)
                    && from_previous(c.input1())
                    && matches!(c.input2(), Input::SourceGraphic | Input::SourceAlpha) =>
            {
                stencil = true;
            }
            // An `feComponentTransfer` that only rescales alpha is one colour-matrix row.
            Kind::ComponentTransfer(ct) if from_previous(ct.input()) => {
                chain.push(alpha_transfer_matrix(ct)?);
                interpolation = p.color_interpolation();
            }
            // `feBlend` of the noise with the source graphic, last. For a mode where the order matters and the
            // noise is `in`, the noise becomes the layer's content (`swapped`).
            Kind::Blend(b) if last && with_source(b.input1(), b.input2()) => {
                blend = Some(BlendPlan {
                    mode: blend_mode(b.mode()),
                    linear: p.color_interpolation() == ColorInterpolation::LinearRGB,
                    root_rect: root_rect(),
                    stencilled: stencil,
                    swapped: from_previous(b.input1()) && !blend_commutes(b.mode()),
                });
                stencil = false;
            }
            // `feComposite operator="lighter"` with the source graphic, last: the clamped sum of premultiplied
            // colour and alpha, which is the blend mode plus-lighter (and commutes).
            Kind::Composite(c)
                if last
                    && matches!(c.operator(), CompositeOperator::Lighter)
                    && with_source(c.input1(), c.input2()) =>
            {
                blend = Some(BlendPlan {
                    mode: BlendMode::PlusLighter,
                    linear: p.color_interpolation() == ColorInterpolation::LinearRGB,
                    root_rect: root_rect(),
                    stencilled: stencil,
                    swapped: false,
                });
                stencil = false;
            }
            _ => return None,
        }
    }
    // The noise stays in the blend's space when a blend follows; the layer's chain converts the source into that
    // space and back.
    if turbulence.is_some() && interpolation == ColorInterpolation::LinearRGB && !blend.is_some_and(|b| b.linear) {
        chain.push(ImageFilter::LinearRgbToSrgb);
    }
    // A flood alone replaces the source; blended, it is authored in sRGB and read in the blend's space.
    let flood = flood.map(|fl| {
        let linear = blend.is_some_and(|b| b.linear);
        let c = fl.color();
        let channel = |v: u8| {
            let x = f32::from(v) / 255.0;
            if linear {
                srgb_to_linear(x)
            } else {
                x
            }
        };
        let mut color = Color::rgbf(channel(c.red), channel(c.green), channel(c.blue));
        color.set_alphaf(fl.opacity().get());
        color
    });
    Some(NoisePlan {
        chain,
        size: (x1 - x0, y1 - y0),
        user_rect,
        abs_transform: abs,
        stencil_transform,
        stencil,
        flood,
        blend,
    })
}
