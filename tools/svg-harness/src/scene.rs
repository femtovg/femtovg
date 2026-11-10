//! Draws a usvg tree onto a femtovg canvas: groups as layers with their opacity, masks, blend modes and filters,
//! paths with their paints, images, and text.

use std::collections::HashMap;

use femtovg::{
    renderer::WGPURenderer, Color, CompositeOperation, FillRule, ImageFilter, ImageFlags, ImageId, LayerEffects,
    MaskKind, Paint, Path, PixelFormat, RenderTarget, Transform2D,
};

use crate::{
    filters::{self, FilterSpace, Filters, LayerChain, NoisePlan, ShadowChain},
    stats::Counters,
    text::{Fonts, TextMode},
};

pub type Canvas = femtovg::Canvas<WGPURenderer>;

/// Features `--without` leaves out, to find which of them moves a frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Without {
    pub text: bool,
    pub clip: bool,
    /// Groups with a blend mode other than normal are not drawn.
    pub blend: bool,
    pub shadow: bool,
    pub turbulence: bool,
    /// Every group is drawn as if it had no filter.
    pub filters: bool,
}

/// How the SVG is framed and drawn.
#[derive(Clone, Debug)]
pub struct SceneOptions {
    pub frame: (u32, u32),
    /// The square the SVG's size is fitted into: its side and its top-left corner, in frame pixels.
    pub box_size: f32,
    pub box_at: (f32, f32),
    /// The point the zoom scales about, in frame pixels; the box centre by default.
    pub pivot: Option<(f32, f32)>,
    pub background: Color,
    /// Clip to the SVG's viewport, as a browser hides its overflow.
    pub viewport_clip: bool,
    pub text: TextMode,
    pub without: Without,
}

impl Default for SceneOptions {
    fn default() -> Self {
        Self {
            frame: (460, 260),
            box_size: 200.0,
            box_at: (130.0, 30.0),
            pivot: None,
            background: Color::white(),
            viewport_clip: true,
            text: TextMode::Outlines,
            without: Without::default(),
        }
    }
}

impl SceneOptions {
    pub fn pivot(&self) -> (f32, f32) {
        self.pivot
            .unwrap_or((self.box_at.0 + self.box_size / 2.0, self.box_at.1 + self.box_size / 2.0))
    }
}

pub fn ts_to_t2d(t: usvg::Transform) -> Transform2D {
    Transform2D([t.sx, t.ky, t.kx, t.sy, t.tx, t.ty])
}

/// A usvg group's identity for the maps of precaptured images.
pub fn key(group: &usvg::Group) -> usize {
    group as *const usvg::Group as usize
}

fn stop_list(stops: &[usvg::Stop]) -> Vec<(f32, Color)> {
    stops
        .iter()
        .map(|s| {
            let c = s.color();
            let mut col = Color::rgb(c.red, c.green, c.blue);
            col.set_alphaf(s.opacity().get());
            (s.offset().get(), col)
        })
        .collect()
}

/// While a group's shadow is drawn alone (`draw_shadow_only`): every paint is this colour, whose alpha is the flood
/// opacity, and every path is shifted by this root-space offset.
#[derive(Clone, Copy)]
pub struct ShadowCopy {
    color: Color,
    dx: f32,
    dy: f32,
}

/// Draws scenes; keeps what one frame's walk shares and what a run counts.
pub struct Painter {
    pub(crate) opts: SceneOptions,
    pub stats: Counters,
    pub(crate) fonts: Fonts,
    /// User units to device pixels at the root: the zoom times the box fit.
    pub(crate) scale: f32,
    /// The canvas transform at the root of the scene walk (zoom and box fit applied), so a rect a filter needs in
    /// root device space can be computed inside a layer, whose canvas transform carries the layer store's shift.
    pub(crate) root_device: Transform2D,
    /// What a node's `abs_transform` is relative to: identity, except under a colour glyph of flattened text.
    pub(crate) abs_base: usvg::Transform,
    pub(crate) shadow_copy: Option<ShadowCopy>,
    /// Mask contents, rendered before the frame draws, by referencing group.
    pub(crate) masks: HashMap<usize, (ImageId, MaskKind)>,
    /// Coverage images of the clips that are no union of paths, by referencing group and place in its clip chain.
    pub(crate) clip_coverage: HashMap<(usize, usize), ImageId>,
    /// Source-graphic captures for `feBlend` chains, by group.
    pub(crate) blend_sources: HashMap<usize, ImageId>,
    /// Images made while drawing one frame, deleted at its end (femtovg defers that past the flush that reads them),
    /// so a persistent canvas does not hold every frame's.
    frame_images: Vec<ImageId>,
}

impl Painter {
    pub fn new(opts: SceneOptions) -> Self {
        Self {
            opts,
            stats: Counters::default(),
            fonts: Fonts::default(),
            scale: 1.0,
            root_device: Transform2D::identity(),
            abs_base: usvg::Transform::identity(),
            shadow_copy: None,
            masks: HashMap::new(),
            clip_coverage: HashMap::new(),
            blend_sources: HashMap::new(),
            frame_images: Vec::new(),
        }
    }

    pub fn text_context(&self) -> femtovg::TextContext {
        self.fonts.context.clone()
    }

    /// Draws the whole scene once under the pivot zoom: the clear, the framing transform, the precaptures and the
    /// node walk. Flushing is the caller's.
    pub fn render(&mut self, canvas: &mut Canvas, tree: &usvg::Tree, zoom: f32) {
        self.fonts.db = Some(tree.fontdb().clone());
        let (w, h) = self.opts.frame;
        let bg = self.opts.background;
        canvas.clear_rect(0, 0, w, h, bg);
        // A blend mode reads the backdrop under its group, the page background included (Chromium blends a
        // top-level child with the page), and the screen cannot be read back, so such a scene is drawn in a layer
        // that holds the background.
        let page_layer = filters::has_blend_group(tree.root());
        if page_layer {
            self.begin_layer(canvas, &LayerEffects::new());
            let mut page = Path::new();
            page.rect(0.0, 0.0, w as f32, h as f32);
            let mut paint = Paint::color(bg);
            paint.set_anti_alias(false);
            canvas.fill_path(&page, &paint);
        }
        canvas.save();
        let (px, py) = self.opts.pivot();
        canvas.translate(px, py);
        canvas.scale(zoom, zoom);
        canvas.translate(-px, -py);

        let size = tree.size();
        let fit = self.opts.box_size / size.width().max(size.height());
        let (box_x, box_y) = self.opts.box_at;
        canvas.translate(box_x, box_y);
        // The viewport is the SVG's own width and height (usvg letterboxes the viewBox inside it), not the square
        // box: a wide file's overflow below its bottom edge is hidden in a browser too.
        if self.opts.viewport_clip {
            canvas.scissor(0.0, 0.0, size.width() * fit, size.height() * fit);
        }
        canvas.scale(fit, fit);
        self.root_device = canvas.transform();
        self.scale = zoom * fit;
        self.masks.clear();
        self.precapture_masks(canvas, tree.root().children(), usvg::Transform::identity());
        self.precapture_blend_sources(canvas, tree.root().children());
        self.draw_nodes(canvas, tree.root().children());
        canvas.restore();

        self.blend_sources.clear();
        self.clip_coverage.clear();
        if page_layer {
            canvas.end_layer();
        }
        for image in self.frame_images.drain(..) {
            canvas.delete_image(image);
        }
    }

    pub(crate) fn begin_layer(&mut self, canvas: &mut Canvas, fx: &LayerEffects) {
        self.stats.layers += 1;
        if !canvas.begin_layer(fx) {
            self.stats.layers_passed_through += 1;
        }
    }

    /// A frame-sized image for a precapture, deleted with the frame.
    pub(crate) fn frame_image(&mut self, canvas: &mut Canvas, flags: ImageFlags) -> Option<ImageId> {
        let (w, h) = self.opts.frame;
        let image = canvas
            .create_image_empty(w as usize, h as usize, PixelFormat::Rgba8, flags)
            .ok()?;
        self.frame_images.push(image);
        Some(image)
    }

    /// Draws `draw` into a cleared frame-sized image under `transform`.
    pub(crate) fn capture(
        &mut self,
        canvas: &mut Canvas,
        transform: Option<usvg::Transform>,
        draw: impl FnOnce(&mut Self, &mut Canvas),
    ) -> Option<ImageId> {
        let image = self.frame_image(canvas, ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y)?;
        let (w, h) = self.opts.frame;
        canvas.save();
        canvas.with_render_target(RenderTarget::Image(image), |canvas| {
            canvas.clear_rect(0, 0, w, h, Color::rgbaf(0.0, 0.0, 0.0, 0.0));
            if let Some(t) = transform {
                canvas.set_transform(&ts_to_t2d(t));
            }
            draw(self, canvas);
        });
        canvas.restore();
        Some(image)
    }

    /// The group's filters, or none under `--without filters`.
    fn filters<'g>(&self, group: &'g usvg::Group) -> &'g Filters {
        if self.opts.without.filters {
            &[]
        } else {
            group.filters()
        }
    }

    fn abs_of(&self, t: usvg::Transform) -> usvg::Transform {
        self.abs_base.pre_concat(t)
    }

    /// Every mask is captured before any drawing, so no capture has to restore into a live layer, and so are the
    /// coverage images of clips that no set of paths expresses. `base` is the transform the children's
    /// `abs_transform` is relative to: identity for the tree, the referencing group's for mask content, that and the
    /// clipPath's own for clip content (usvg gives their content transforms from their own root).
    pub(crate) fn precapture_masks(&mut self, canvas: &mut Canvas, children: &[usvg::Node], base: usvg::Transform) {
        for node in children {
            let usvg::Node::Group(group) = node else { continue };
            let at = base.pre_concat(group.abs_transform());
            if let Some(mask) = group.mask() {
                self.precapture_masks(canvas, mask.root().children(), at);
                // Mask content lives in the referencing group's user space.
                let image = self.capture(canvas, Some(at), |painter, canvas| {
                    painter.draw_nodes(canvas, mask.root().children())
                });
                if let Some(image) = image {
                    let kind = match mask.kind() {
                        usvg::MaskType::Luminance => MaskKind::Luminance,
                        usvg::MaskType::Alpha => MaskKind::Alpha,
                    };
                    self.masks.insert(key(group), (image, kind));
                }
            }
            self.precapture_clip_coverage(canvas, group, at);
            self.precapture_masks(canvas, group.children(), base);
        }
    }

    /// The groups an `feBlend` chain reads the source graphic of, drawn alone at the root transform into
    /// frame-sized images before the frame draws: its silhouette for `SourceAlpha`, converted for a linearRGB blend.
    fn precapture_blend_sources(&mut self, canvas: &mut Canvas, children: &[usvg::Node]) {
        for node in children {
            let usvg::Node::Group(group) = node else { continue };
            self.precapture_blend_sources(canvas, group.children());
            let filters = self.filters(group);
            let (alpha_only, linear) = match filters::blur_blend(filters) {
                Some(b) => (b.alpha_only, b.linear),
                None => match self
                    .turbulence_plan(canvas, group)
                    .and_then(|p| p.blend)
                    .filter(|b| b.swapped)
                {
                    Some(b) => (false, b.linear),
                    None => continue,
                },
            };
            let Some(image) = self.capture(canvas, None, |painter, canvas| {
                painter.draw_nodes(canvas, group.children())
            }) else {
                continue;
            };
            // SourceAlpha is the silhouette (black is the same in either colour space).
            let convert = match (alpha_only, linear) {
                (true, _) => {
                    let mut matrix = [0.0f32; 20];
                    matrix[18] = 1.0;
                    Some(ImageFilter::ColorMatrix { matrix })
                }
                (false, true) => Some(ImageFilter::SrgbToLinearRgb),
                (false, false) => None,
            };
            let mut source = image;
            if let Some(pass) = convert {
                if let Some(converted) = self.frame_image(canvas, ImageFlags::PREMULTIPLIED) {
                    if canvas.filter_image_chain(converted, &[pass], image).is_ok() {
                        source = converted;
                    }
                }
            }
            self.blend_sources.insert(key(group), source);
        }
    }

    fn turbulence_plan(&self, canvas: &Canvas, group: &usvg::Group) -> Option<NoisePlan> {
        if self.opts.without.turbulence {
            return None;
        }
        let filters = self.filters(group);
        filters::turbulence_plan(canvas.transform(), self.root_device, self.opts.frame, group, filters)
    }

    fn to_paint(&self, p: &usvg::Paint) -> Option<Paint> {
        if let Some(copy) = self.shadow_copy {
            let mut paint = Paint::color(copy.color);
            paint.set_anti_alias(true);
            return Some(paint);
        }
        let mut paint = match p {
            usvg::Paint::Color(c) => Paint::color(Color::rgb(c.red, c.green, c.blue)),
            usvg::Paint::LinearGradient(g) => {
                Paint::linear_gradient_stops(g.x1(), g.y1(), g.x2(), g.y2(), stop_list(g.stops()))
                    .with_gradient_transform(ts_to_t2d(g.transform()))
            }
            usvg::Paint::RadialGradient(g) => Paint::two_point_radial_gradient_stops(
                g.fx(),
                g.fy(),
                0.0,
                g.cx(),
                g.cy(),
                g.r().get(),
                stop_list(g.stops()),
            )
            .with_gradient_transform(ts_to_t2d(g.transform())),
            usvg::Paint::Pattern(_) => return None,
        };
        paint.set_anti_alias(true);
        Some(paint)
    }

    pub(crate) fn stroke_paint(&self, stroke: &usvg::Stroke) -> Option<Paint> {
        let mut paint = self.to_paint(stroke.paint())?;
        paint.set_line_width(stroke.width().get());
        paint.set_line_cap(match stroke.linecap() {
            usvg::LineCap::Butt => femtovg::LineCap::Butt,
            usvg::LineCap::Round => femtovg::LineCap::Round,
            usvg::LineCap::Square => femtovg::LineCap::Square,
        });
        paint.set_line_join(match stroke.linejoin() {
            usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => femtovg::LineJoin::Miter,
            usvg::LineJoin::Round => femtovg::LineJoin::Round,
            usvg::LineJoin::Bevel => femtovg::LineJoin::Bevel,
        });
        paint.set_miter_limit(stroke.miterlimit().get());
        if let Some(dashes) = stroke.dasharray() {
            paint.set_line_dash(dashes);
            paint.set_line_dash_offset(stroke.dashoffset());
        }
        Some(paint)
    }

    /// A span's fill and stroke as (paint, opacity, is the stroke), in paint order.
    pub(crate) fn span_passes(
        &self,
        fill: Option<&usvg::Fill>,
        stroke: Option<&usvg::Stroke>,
        order: usvg::PaintOrder,
    ) -> Vec<(Paint, f32, bool)> {
        let fill = fill.and_then(|f| Some((self.to_paint(f.paint())?, f.opacity().get(), false)));
        let stroke = stroke.and_then(|s| Some((self.stroke_paint(s)?, s.opacity().get(), true)));
        match order {
            usvg::PaintOrder::FillAndStroke => [fill, stroke],
            usvg::PaintOrder::StrokeAndFill => [stroke, fill],
        }
        .into_iter()
        .flatten()
        .collect()
    }

    /// Applies the transform of a node with absolute transform `t`; while a shadow copy is drawn, its offset goes
    /// after every transform, added to the composed matrix's translation.
    pub(crate) fn set_node_transform(&self, canvas: &mut Canvas, t: usvg::Transform) {
        canvas.set_transform(&ts_to_t2d(self.abs_of(t)));
        if let Some(copy) = self.shadow_copy {
            let mut shifted = canvas.transform();
            shifted.0[4] += copy.dx;
            shifted.0[5] += copy.dy;
            canvas.reset_transform();
            canvas.set_transform(&shifted);
        }
    }

    pub(crate) fn draw_nodes(&mut self, canvas: &mut Canvas, children: &[usvg::Node]) {
        for node in children {
            match node {
                usvg::Node::Group(group) => self.draw_group(canvas, group),
                usvg::Node::Path(path) => self.draw_path(canvas, path),
                usvg::Node::Image(image) => self.draw_image(canvas, image),
                usvg::Node::Text(text) if !self.opts.without.text => self.draw_text(canvas, text),
                usvg::Node::Text(_) => {}
            }
        }
    }

    fn draw_group(&mut self, canvas: &mut Canvas, group: &usvg::Group) {
        if self.opts.without.blend && group.blend_mode() != usvg::BlendMode::Normal {
            return;
        }
        let filters = self.filters(group);
        // A filter this backend cannot run takes its subtree with it when it replaces the source graphic: that
        // source is routinely a shape never meant to be seen, so drawing it raw is worse than drawing nothing.
        let plan = self.turbulence_plan(canvas, group);
        let chain = filters::layer_chain(group, filters, self.scale, self.root_device);
        let mapped = plan.is_some() || filters::blur_blend(filters).is_some() || chain.complete;
        if !mapped && filters.iter().any(|f| filters::replaces_source(f)) {
            self.stats.filters_skipped += 1;
            return;
        }
        let shadow = if self.opts.without.shadow {
            None
        } else {
            filters::drop_shadow(filters)
        };
        if let Some(shadow) = shadow.as_ref().filter(|s| !s.merged) {
            self.draw_shadow_only(canvas, group, filters, shadow);
            return;
        }
        let clipped = self.push_clip(canvas, group);
        canvas.save();
        let region = filters::filter_region(group, filters);
        if let Some((_, result)) = region {
            filters::scissor_to(canvas, result);
        }
        // A merged shadow is cast by an inner layer that holds the source into the group's layer, which holds
        // their composite in the merge's colour space (see `ShadowMerge`).
        let merge = shadow.as_ref().map(filters::shadow_merge);
        let stencil = plan.as_ref().is_some_and(|p| p.stencil);
        // A glow blurs in an inner layer and redraws sharp on top, as does any chain that ends with the source
        // composited with its result; an operator other than source-over needs the group's layer around both to
        // composite against the group alone. A chain the shadow state renders is not run again as filters.
        let inner: Option<(Vec<ImageFilter>, CompositeOperation, bool)> = if shadow.is_some() {
            None
        } else if let Some((op, chain_first)) = chain.with_source {
            Some((chain.filters(), op, chain_first))
        } else if filters::glow(filters) {
            Some((
                filters::blur_filters(group, filters, self.scale),
                CompositeOperation::SourceOver,
                true,
            ))
        } else {
            None
        };
        let inner = inner.as_ref().map(|(f, op, first)| (f.as_slice(), *op, *first));
        let isolating = inner.is_some_and(|(_, op, _)| op != CompositeOperation::SourceOver);
        let effects = self.group_effects(
            group,
            filters,
            &chain,
            shadow.is_some() || stencil || isolating,
            shadow.is_some() || inner.is_some(),
            merge.as_ref().is_some_and(|m| !m.to_merge_space.is_empty()),
        );
        match effects {
            Some(fx) => {
                self.begin_layer(canvas, &fx);
                if let (Some(s), Some(m)) = (&shadow, &merge) {
                    // A shadow set before begin_layer is cast once by the layer's result, the Canvas 2D layer rule.
                    let space = FilterSpace::of(group, self.scale);
                    let (dx, dy) = space.vector(s.dx, s.dy);
                    canvas.set_shadow_color(m.color);
                    canvas.set_shadow_offset(dx, dy);
                    // Canvas shadowBlur is two sigma.
                    canvas.set_shadow_blur(2.0 * space.shadow_sigma(s.sigma));
                    self.begin_layer(canvas, &LayerEffects::new().with_filters(&m.to_merge_space));
                }
                if let Some((source, _)) = region {
                    filters::scissor_to(canvas, source);
                }
                self.draw_group_body(canvas, group, plan, inner);
                if merge.is_some() {
                    canvas.end_layer();
                }
                canvas.end_layer();
            }
            None => {
                if inner.is_some() {
                    if let Some((source, _)) = region {
                        filters::scissor_to(canvas, source);
                    }
                }
                self.draw_group_body(canvas, group, plan, inner);
            }
        }
        canvas.restore();
        self.pop_clip(canvas, clipped);
    }

    /// The group's layer, when it needs one. `force_layer`: it needs one whatever its effects. `chain_elsewhere`:
    /// its filter chain is rendered elsewhere (an inner layer, or the shadow state), so this layer runs none of it.
    /// `result_linear`: the layer receives a linearRGB result (a shadow merged there) and converts it back first.
    fn group_effects(
        &self,
        group: &usvg::Group,
        filters: &Filters,
        chain: &LayerChain,
        force_layer: bool,
        chain_elsewhere: bool,
        result_linear: bool,
    ) -> Option<LayerEffects> {
        let (w, h) = self.opts.frame;
        let opacity = group.opacity().get();
        let (mut passes, mut linear) = if chain_elsewhere {
            (Vec::new(), false)
        } else {
            (chain.passes.clone(), chain.linear)
        };
        linear |= result_linear;
        if let Some(blend) = filters::blur_blend(filters) {
            if let Some(&backdrop) = self.blend_sources.get(&key(group)) {
                if blend.linear != linear {
                    passes.push(filters::color_space_pass(blend.linear));
                    linear = blend.linear;
                }
                passes.push(ImageFilter::Blend {
                    mode: filters::blend_mode(blend.mode),
                    backdrop,
                    x: 0.0,
                    y: 0.0,
                    width: w as f32,
                    height: h as f32,
                });
            }
        }
        if linear {
            passes.push(filters::color_space_pass(false));
        }
        let mask = self.masks.get(&key(group)).copied();
        // `mix-blend-mode` composites the group's layer with its backdrop. A group that isolates (clip-path,
        // `isolation: isolate`, ...) gets a plain layer when a blend inside it would otherwise see through it.
        let blend = group.blend_mode();
        let force_layer = force_layer || (group.should_isolate() && filters::has_blend_group(group));
        if opacity >= 1.0 && passes.is_empty() && mask.is_none() && !force_layer && blend == usvg::BlendMode::Normal {
            return None;
        }
        let mut fx = LayerEffects::new().with_opacity(opacity).with_filters(&passes);
        if let Some((image, kind)) = mask {
            fx = fx.with_mask(image, kind, 0.0, 0.0, w as f32, h as f32);
        }
        if blend != usvg::BlendMode::Normal {
            fx = fx.with_blend(filters::blend_mode(blend));
        }
        Some(fx)
    }

    /// The group's shadow alone: its subtree in the flood colour, shifted by the offset in root space, blurred in a
    /// layer, under the group's clip and filter region like any filtered group.
    fn draw_shadow_only(&mut self, canvas: &mut Canvas, group: &usvg::Group, filters: &Filters, shadow: &ShadowChain) {
        let clipped = self.push_clip(canvas, group);
        canvas.save();
        let region = filters::filter_region(group, filters);
        if let Some((_, result)) = region {
            filters::scissor_to(canvas, result);
        }
        let space = FilterSpace::of(group, self.scale);
        let sigma = space.shadow_sigma(shadow.sigma);
        let blur: &[ImageFilter] = if sigma > 0.0 {
            &[filters::isotropic_blur(sigma)]
        } else {
            &[]
        };
        self.begin_layer(
            canvas,
            &LayerEffects::new()
                .with_opacity(group.opacity().get())
                .with_filters(blur),
        );
        if let Some((source, _)) = region {
            filters::scissor_to(canvas, source);
        }
        let (dx, dy) = space.vector(shadow.dx, shadow.dy);
        self.shadow_copy = Some(ShadowCopy {
            color: shadow.color,
            dx,
            dy,
        });
        self.draw_nodes(canvas, group.children());
        self.shadow_copy = None;
        canvas.end_layer();
        canvas.restore();
        self.pop_clip(canvas, clipped);
    }

    /// Draws the group: plainly, or as its chain in an inner layer composited with the group itself under `inner`'s
    /// operator, the layer first and the group over, in, out of, atop or xor it, or the group first and the layer
    /// composited with it.
    fn draw_group_body(
        &mut self,
        canvas: &mut Canvas,
        group: &usvg::Group,
        plan: Option<NoisePlan>,
        inner: Option<(&[ImageFilter], CompositeOperation, bool)>,
    ) {
        let Some((filters, op, chain_first)) = inner else {
            self.draw_filtered(canvas, group, plan);
            return;
        };
        let chain_layer = |painter: &mut Self, canvas: &mut Canvas, plan: Option<NoisePlan>| {
            painter.begin_layer(canvas, &LayerEffects::new().with_filters(filters));
            painter.draw_filtered(canvas, group, plan);
            canvas.end_layer();
        };
        if chain_first {
            chain_layer(self, canvas, plan);
            canvas.save();
            canvas.global_composite_operation(op);
            self.draw_filtered(canvas, group, None);
            canvas.restore();
        } else {
            self.draw_filtered(canvas, group, None);
            canvas.save();
            canvas.global_composite_operation(op);
            chain_layer(self, canvas, plan);
            canvas.restore();
        }
    }

    /// Draws a group's children, or under a NoisePlan the filter's output in their place: the noise image alone when
    /// the chain replaces the source, the source with the noise composited SourceIn when it stencils it, or the
    /// source blended with the noise.
    fn draw_filtered(&mut self, canvas: &mut Canvas, group: &usvg::Group, plan: Option<NoisePlan>) {
        let Some(plan) = plan else {
            self.draw_nodes(canvas, group.children());
            return;
        };
        let (w, h) = plan.size;
        let Ok(noise) = canvas.create_image_empty(
            w as usize,
            h as usize,
            PixelFormat::Rgba8,
            ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y,
        ) else {
            return;
        };
        self.frame_images.push(noise);
        if let Some(color) = plan.flood {
            canvas.with_render_target(RenderTarget::Image(noise), |canvas| {
                canvas.clear_rect(0, 0, w as u32, h as u32, color);
            });
        }
        // Turbulence reads nothing from its source; it only sizes the quad.
        if !plan.chain.is_empty() {
            let _ = canvas.filter_image_chain(noise, &plan.chain, noise);
        }
        let draw_noise = |canvas: &mut Canvas| {
            canvas.save();
            canvas.set_transform(&plan.abs_transform);
            let (x, y, uw, uh) = plan.user_rect;
            let mut rect = Path::new();
            rect.rect(x, y, uw, uh);
            canvas.fill_path(&rect, &Paint::image(noise, x, y, uw, uh, 0.0, 1.0));
            canvas.restore();
        };
        if let Some(blend) = plan.blend {
            // `feComposite in` before the blend: keep the noise only where the source has coverage. A DestinationIn
            // draw touches only the pixels it covers, so the source goes through a plain layer whose composite spans
            // the whole noise image and multiplies every pixel by the source's coverage there.
            if blend.stencilled {
                canvas.save();
                canvas.with_render_target(RenderTarget::Image(noise), |canvas| {
                    canvas.reset_transform();
                    // The root viewport scissor is in canvas pixels, not the noise image's.
                    canvas.reset_scissor();
                    canvas.set_transform(&plan.stencil_transform);
                    canvas.global_composite_operation(CompositeOperation::DestinationIn);
                    self.begin_layer(canvas, &LayerEffects::new());
                    self.draw_nodes(canvas, group.children());
                    canvas.end_layer();
                });
                canvas.restore();
            }
            // The group's content is the blend's image, the noise its backdrop, placed where the filter region lies
            // in root device space; a linearRGB blend converts the content into that space and back. Swapped, the
            // noise (already in the blend's space) is the content and the group's precaptured source the backdrop.
            let source = self.blend_sources.get(&key(group)).copied();
            let swapped = blend.swapped && source.is_some();
            let (frame_w, frame_h) = self.opts.frame;
            let pass = match source.filter(|_| swapped) {
                Some(backdrop) => ImageFilter::Blend {
                    mode: blend.mode,
                    backdrop,
                    x: 0.0,
                    y: 0.0,
                    width: frame_w as f32,
                    height: frame_h as f32,
                },
                None => {
                    let (x, y, width, height) = blend.root_rect;
                    ImageFilter::Blend {
                        mode: blend.mode,
                        backdrop: noise,
                        x,
                        y,
                        width,
                        height,
                    }
                }
            };
            let chain: Vec<ImageFilter> = match (blend.linear, swapped) {
                (true, false) => vec![ImageFilter::SrgbToLinearRgb, pass, ImageFilter::LinearRgbToSrgb],
                (true, true) => vec![pass, ImageFilter::LinearRgbToSrgb],
                (false, _) => vec![pass],
            };
            self.begin_layer(canvas, &LayerEffects::new().with_filters(&chain));
            if swapped {
                draw_noise(canvas);
            } else {
                self.draw_nodes(canvas, group.children());
            }
            canvas.end_layer();
            return;
        }
        if plan.stencil {
            self.draw_nodes(canvas, group.children());
            canvas.global_composite_operation(CompositeOperation::SourceIn);
        }
        draw_noise(canvas);
    }

    fn draw_path(&mut self, canvas: &mut Canvas, svg_path: &usvg::Path) {
        use usvg::tiny_skia_path::PathSegment;
        let mut path = Path::new();
        for seg in svg_path.data().segments() {
            match seg {
                PathSegment::MoveTo(p) => path.move_to(p.x, p.y),
                PathSegment::LineTo(p) => path.line_to(p.x, p.y),
                PathSegment::CubicTo(a, b, p) => path.bezier_to(a.x, a.y, b.x, b.y, p.x, p.y),
                PathSegment::QuadTo(a, p) => path.quad_to(a.x, a.y, p.x, p.y),
                PathSegment::Close => path.close(),
            }
        }
        canvas.save();
        self.set_node_transform(canvas, svg_path.abs_transform());
        if let Some(fill) = svg_path.fill() {
            if let Some(mut paint) = self.to_paint(fill.paint()) {
                paint.set_fill_rule(match fill.rule() {
                    usvg::FillRule::NonZero => FillRule::NonZero,
                    usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                });
                canvas.set_global_alpha(fill.opacity().get());
                canvas.fill_path(&path, &paint);
                canvas.set_global_alpha(1.0);
                self.stats.fills += 1;
            }
        }
        if let Some(stroke) = svg_path.stroke() {
            if let Some(paint) = self.stroke_paint(stroke) {
                canvas.set_global_alpha(stroke.opacity().get());
                canvas.stroke_path(&path, &paint);
                canvas.set_global_alpha(1.0);
                self.stats.strokes += 1;
            }
        }
        canvas.restore();
    }

    fn draw_image(&mut self, canvas: &mut Canvas, image: &usvg::Image) {
        if !image.is_visible() {
            return;
        }
        let size = image.size();
        canvas.save();
        // usvg's transform already fits the raster's own size onto the element's rect (`preserveAspectRatio`
        // included).
        canvas.set_transform(&ts_to_t2d(self.abs_of(image.abs_transform())));
        match image.kind() {
            usvg::ImageKind::PNG(data) | usvg::ImageKind::JPEG(data) => {
                // A browser minifies a smooth image through a filtered downsample; mipmaps are the GPU's.
                // `pixelated` (optimizeSpeed) samples the base level, unfiltered.
                let flags = if image.rendering_mode() == usvg::ImageRendering::OptimizeSpeed {
                    ImageFlags::NEAREST
                } else {
                    ImageFlags::GENERATE_MIPMAPS
                };
                if let Ok(id) = canvas.load_image_mem(data, flags) {
                    self.frame_images.push(id);
                    let mut rect = Path::new();
                    rect.rect(0.0, 0.0, size.width(), size.height());
                    let mut paint = Paint::image(id, 0.0, 0.0, size.width(), size.height(), 0.0, 1.0);
                    paint.set_anti_alias(true);
                    canvas.fill_path(&rect, &paint);
                }
            }
            usvg::ImageKind::SVG(tree) => self.draw_nodes(canvas, tree.root().children()),
            // Only PNG and JPEG are decoded.
            usvg::ImageKind::GIF(_) | usvg::ImageKind::WEBP(_) => {}
        }
        canvas.restore();
    }
}
