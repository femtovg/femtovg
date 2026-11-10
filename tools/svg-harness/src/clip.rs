//! Clip paths: a chain of clipPaths as canvas clips, and the clipPaths no set of paths can express as coverage
//! images applied as layer masks.

use femtovg::{FillRule, LayerEffects, MaskKind, Path};

use crate::scene::{key, Canvas, Painter};

/// What `push_clip` took, in order: `false` a clip on the canvas (a `restore` is owed), `true` a coverage layer (an
/// `end_layer`).
#[derive(Default)]
pub struct ClipTaken(Vec<bool>);

/// Whether a clipPath needs a coverage image: a child carries a `clip-path` of its own or is text. Such a child clips
/// to its own intersection, and the region is the union of those intersections, which no `clip_paths` call (an
/// intersection of unions) can express. Browsers paint such a clip as a mask: Blink's `ClipPathClipper` falls back to
/// mask-based clipping, Gecko's `SVGClipPathFrame` paints the children into a mask surface, each under its own clip.
fn needs_coverage(clip: &usvg::ClipPath) -> bool {
    fn walk(nodes: &[usvg::Node]) -> bool {
        nodes.iter().any(|node| match node {
            usvg::Node::Group(g) => g.clip_path().is_some() || walk(g.children()),
            usvg::Node::Text(_) => true,
            _ => false,
        })
    }
    walk(clip.root().children())
}

impl Painter {
    /// Captures the coverage of every clip in `group`'s chain that needs one, after whatever the clip content itself
    /// needs (its children's clips, their masks), innermost first. `at` is the group's transform from the root.
    pub(crate) fn precapture_clip_coverage(&mut self, canvas: &mut Canvas, group: &usvg::Group, at: usvg::Transform) {
        if self.opts.without.clip {
            return;
        }
        let mut chain = group.clip_path();
        let mut depth = 0;
        while let Some(clip) = chain {
            // Clip content lives in the referencing group's user space, under the clipPath's transform.
            let clip_base = at.pre_concat(clip.transform());
            self.precapture_masks(canvas, clip.root().children(), clip_base);
            if needs_coverage(clip) {
                // Each child as usvg converted it (opaque black, under its clip-rule) and under its own clip, which
                // `push_clip` applies as for any group: the alpha is the union of the children's intersections.
                let image = self.capture(canvas, Some(clip_base), |painter, canvas| {
                    painter.draw_nodes(canvas, clip.root().children())
                });
                if let Some(image) = image {
                    self.clip_coverage.insert((key(group), depth), image);
                }
            }
            chain = clip.clip_path();
            depth += 1;
        }
    }

    /// Pushes the group's clip path. A clipPath may itself carry `clip-path`: the region is the intersection of the
    /// chain, each clipPath in the group's user space under its own transform, so each is taken in turn.
    pub(crate) fn push_clip(&mut self, canvas: &mut Canvas, group: &usvg::Group) -> ClipTaken {
        let mut taken = ClipTaken::default();
        if self.opts.without.clip {
            return taken;
        }
        let (w, h) = self.opts.frame;
        let mut chain = group.clip_path();
        let mut depth = 0;
        while let Some(clip) = chain {
            if let Some(&image) = self.clip_coverage.get(&(key(group), depth)) {
                self.stats.clip_coverage_layers += 1;
                let fx = LayerEffects::new().with_mask(image, MaskKind::Alpha, 0.0, 0.0, w as f32, h as f32);
                self.begin_layer(canvas, &fx);
                taken.0.push(true);
            } else {
                canvas.save();
                clip_to(canvas, clip, group.abs_transform().pre_concat(clip.transform()));
                taken.0.push(false);
            }
            chain = clip.clip_path();
            depth += 1;
        }
        taken
    }

    pub(crate) fn pop_clip(&mut self, canvas: &mut Canvas, taken: ClipTaken) {
        for layer in taken.0.into_iter().rev() {
            if layer {
                canvas.end_layer();
            } else {
                canvas.restore();
            }
        }
    }
}

/// Clips to one clipPath's children, each a path of its own under its own clip-rule, their transforms baked in from
/// `base`: the clip is their union. Clip content may be nested in groups (usvg wraps a `<use>` in one).
fn clip_to(canvas: &mut Canvas, clip: &usvg::ClipPath, base: usvg::Transform) {
    fn collect(nodes: &[usvg::Node], base: usvg::Transform, paths: &mut Vec<(Path, FillRule)>) {
        use usvg::tiny_skia_path::PathSegment;
        for node in nodes {
            match node {
                usvg::Node::Group(g) => collect(g.children(), base, paths),
                usvg::Node::Path(p) => {
                    let rule = match p.fill().map(|f| f.rule()) {
                        Some(usvg::FillRule::EvenOdd) => FillRule::EvenOdd,
                        _ => FillRule::NonZero,
                    };
                    let ct = base.pre_concat(p.abs_transform());
                    let map = |x: f32, y: f32| (ct.sx * x + ct.kx * y + ct.tx, ct.ky * x + ct.sy * y + ct.ty);
                    let mut path = Path::new();
                    for seg in p.data().segments() {
                        match seg {
                            PathSegment::MoveTo(q) => {
                                let (x, y) = map(q.x, q.y);
                                path.move_to(x, y);
                            }
                            PathSegment::LineTo(q) => {
                                let (x, y) = map(q.x, q.y);
                                path.line_to(x, y);
                            }
                            PathSegment::QuadTo(a, q) => {
                                let (ax, ay) = map(a.x, a.y);
                                let (x, y) = map(q.x, q.y);
                                path.quad_to(ax, ay, x, y);
                            }
                            PathSegment::CubicTo(a, b, q) => {
                                let (ax, ay) = map(a.x, a.y);
                                let (bx, by) = map(b.x, b.y);
                                let (x, y) = map(q.x, q.y);
                                path.bezier_to(ax, ay, bx, by, x, y);
                            }
                            PathSegment::Close => path.close(),
                        }
                    }
                    paths.push((path, rule));
                }
                _ => {}
            }
        }
    }
    let mut paths = Vec::new();
    collect(clip.root().children(), base, &mut paths);
    // A clipPath without children clips everything out.
    let paths: Vec<(&Path, FillRule)> = paths.iter().map(|(path, rule)| (path, *rule)).collect();
    canvas.clip_paths(&paths);
}
