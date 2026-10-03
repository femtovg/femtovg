//! Arbitrary-path clipping on the stencil plane: the clip stack, its
//! replay per target and the quads that arm and resolve it.

use super::*;

#[derive(Debug)]
pub(crate) struct ClipGeometry {
    pub(crate) vertices: Box<[Vertex]>,
}

#[derive(Debug)]
pub(crate) struct ClipEntry {
    pub(crate) geometry: Rc<ClipGeometry>,
    pub(crate) fill_rule: FillRule,
    pub(crate) target: RenderTarget,
    pub(crate) bounds: Bounds,
    pub(crate) prior_armed: Rect,
    pub(crate) armed: Rect,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ClipPlaneState {
    pub(crate) count: usize,
    pub(crate) dirty: bool,
    pub(crate) armed: Rect,
}

impl<T> Canvas<T>
where
    T: Renderer,
{
    /// Drops the clips above `depth`. Their target planes are reconciled only
    /// when drawing resumes on them, so consecutive restores coalesce.
    pub(crate) fn pop_clips_to(&mut self, depth: usize) {
        if self.clip_stack.len() <= depth {
            return;
        }

        let mut removed = HashMap::<RenderTarget, (usize, Rect)>::new();
        for entry in self.clip_stack.drain(depth..) {
            removed
                .entry(entry.target)
                .and_modify(|(count, _)| *count += 1)
                .or_insert((1, entry.prior_armed));
        }
        for (target, (count, armed)) in removed {
            let plane = self
                .clip_planes
                .get_mut(&target)
                .expect("a clip entry has a target plane");
            plane.count -= count;
            plane.dirty = true;
            plane.armed = armed;
        }
    }

    pub(crate) fn reconcile_current_clip_plane(&mut self) {
        // A suppressed layer records no current-target draws, so defer stencil
        // repair instead of marking an unrecorded replay clean.
        if self.commands_suppressed() {
            return;
        }
        let target = self.current_render_target;
        let dirty = self.clip_planes.get(&target).is_some_and(|plane| plane.dirty);
        if !dirty {
            return;
        }
        self.clip_planes.get_mut(&target).unwrap().dirty = false;
        self.replay_clip_stack();
        if self.clip_planes.get(&target).is_some_and(|plane| plane.count == 0) {
            self.clip_planes.remove(&target);
        }
    }

    /// Whether the stencil clip gates draws into the current render target:
    /// some clip on the stack was taken on it.
    pub(crate) fn clip_active(&self) -> bool {
        self.clip_planes
            .get(&self.current_render_target)
            .is_some_and(|plane| plane.count != 0)
    }

    pub(crate) fn forget_clip_target(&mut self, target: RenderTarget) {
        if !self.clip_planes.contains_key(&target) {
            return;
        }
        let removed: Vec<usize> = self
            .clip_stack
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| (entry.target == target).then_some(index))
            .collect();
        if !removed.is_empty() {
            for state in &mut self.state_stack {
                state.clip_depth -= removed.partition_point(|&index| index < state.clip_depth);
            }
            self.clip_stack.retain(|entry| entry.target != target);
        }
        self.clip_planes.remove(&target);
    }

    /// Intersects the clip region with `path` under the current transform,
    /// with `fill_rule` as the clip-rule: Canvas 2D `clip()`, SVG `clip-path`
    /// and `clip-rule`. Drawing after this call is limited to the
    /// intersection of every clip taken on the current render target.
    ///
    /// Clip edges are not antialiased: unlike a fill or stroke edge, a pixel
    /// is either inside the clip or outside it.
    ///
    /// Clips are part of the saved state - [`restore`](Self::restore) drops
    /// the clips taken since the matching [`save`](Self::save) - and belong
    /// to the render target they were taken on: a clip on the canvas still
    /// applies when a layer opened on it is composited back, and a clip taken
    /// inside a layer or while rendering into an image gates only that layer
    /// or image.
    ///
    /// [`clear_rect`](Self::clear_rect) is not clipped; a Canvas 2D
    /// `clearRect` under a clip is a fill of the rect with an opaque paint
    /// under [`CompositeOperation::DestinationOut`].
    pub fn clip_path(&mut self, path: &Path, fill_rule: FillRule) {
        if self.saturated() {
            // Nothing draws past the depth limit, so nothing to gate; the
            // clip would only cost geometry.
            return;
        }
        self.reconcile_current_clip_plane();
        let target = self.current_render_target;
        let transform = self.state().transform;
        let (vertices, bounds) = {
            let path_cache = path.cache(&transform, self.tess_tol, self.dist_tol);
            (path_cache.winding_triangles(), path_cache.bounds)
        };
        let geometry = Rc::new(ClipGeometry {
            vertices: vertices.into_boxed_slice(),
        });
        let target_rect = self.render_target_rect();
        let path_rect = Self::clip_bounds(bounds, target_rect);
        let previous_armed = self.clip_planes.get(&target).map_or(target_rect, |plane| plane.armed);

        if !self.clip_active() {
            self.emit_clip_reset(true);
        }
        self.emit_clip_fill(&geometry, fill_rule, previous_armed);
        let armed = previous_armed.intersect(path_rect);
        self.clip_stack.push(ClipEntry {
            geometry,
            fill_rule,
            target,
            bounds,
            prior_armed: previous_armed,
            armed,
        });
        let plane = self.clip_planes.entry(target).or_insert(ClipPlaneState {
            count: 0,
            dirty: false,
            armed: target_rect,
        });
        plane.count += 1;
        plane.armed = armed;
        self.state_mut().clip_depth = self.clip_stack.len();
    }

    /// Re-establishes the current render target's stencil clip plane from
    /// the stack: disarmed when no clip on it survives, otherwise reset to
    /// visible and re-intersected with the survivors (a few stencil-only
    /// draws, no color work).
    pub(crate) fn replay_clip_stack(&mut self) {
        let target = self.current_render_target;
        let entries: Vec<_> = self
            .clip_stack
            .iter()
            .filter(|entry| entry.target == target)
            .map(|entry| (entry.geometry.clone(), entry.fill_rule, entry.bounds))
            .collect();
        if entries.is_empty() {
            self.emit_clip_reset(false);
            return;
        }
        self.emit_clip_reset(true);
        let mut previous_armed = self.render_target_rect();
        let target_rect = previous_armed;
        let mut armed_values = Vec::with_capacity(entries.len());
        for (geometry, fill_rule, bounds) in entries {
            self.emit_clip_fill(&geometry, fill_rule, previous_armed);
            let armed = previous_armed.intersect(Self::clip_bounds(bounds, target_rect));
            armed_values.push((previous_armed, armed));
            previous_armed = armed;
        }
        for (entry, (prior_armed, armed)) in self
            .clip_stack
            .iter_mut()
            .filter(|entry| entry.target == target)
            .zip(armed_values)
        {
            entry.prior_armed = prior_armed;
            entry.armed = armed;
        }
        if let Some(plane) = self.clip_planes.get_mut(&target) {
            plane.armed = previous_armed;
        }
    }

    pub(crate) fn render_target_rect(&self) -> Rect {
        let (width, height) = self.render_target_size();
        Rect::new(0.0, 0.0, width, height)
    }

    pub(crate) fn clip_bounds(bounds: Bounds, target: Rect) -> Rect {
        if ![bounds.minx, bounds.miny, bounds.maxx, bounds.maxy]
            .into_iter()
            .all(f32::is_finite)
            || bounds.minx > bounds.maxx
            || bounds.miny > bounds.maxy
        {
            return Rect::default();
        }
        Rect::new(
            bounds.minx - 1.0,
            bounds.miny - 1.0,
            bounds.maxx - bounds.minx + 2.0,
            bounds.maxy - bounds.miny + 2.0,
        )
        .intersect(target)
    }

    /// Pushes a triangle strip over the whole current render target and
    /// returns its vertex range: the stencil quads that arm, disarm and
    /// resolve the clip plane must reach every pixel of the target, which
    /// for a layer store is the store, not the canvas.
    pub(crate) fn push_target_quad(&mut self) -> (usize, usize) {
        self.push_clip_quad(self.render_target_rect())
    }

    pub(crate) fn push_clip_quad(&mut self, rect: Rect) -> (usize, usize) {
        let offset = self.verts.len();
        let x1 = rect.x + rect.w;
        let y1 = rect.y + rect.h;
        self.verts.push(Vertex::new(rect.x, y1, 0.5, 1.0));
        self.verts.push(Vertex::new(x1, y1, 0.5, 1.0));
        self.verts.push(Vertex::new(rect.x, rect.y, 0.5, 1.0));
        self.verts.push(Vertex::new(x1, rect.y, 0.5, 1.0));
        (offset, 4)
    }

    pub(crate) fn emit_clip_reset(&mut self, visible: bool) {
        let mut cmd = Command::new(CommandType::ClipReset { visible });
        cmd.triangles_verts = Some(self.push_target_quad());
        self.append_cmd(cmd);
    }

    pub(crate) fn emit_clip_fill(&mut self, geometry: &ClipGeometry, fill_rule: FillRule, resolve: Rect) {
        let mut cmd = Command::new(CommandType::ClipFill);
        cmd.fill_rule = fill_rule;

        let offset = self.verts.len();
        self.verts.extend_from_slice(&geometry.vertices);
        if !geometry.vertices.is_empty() {
            cmd.drawables.push(Drawable {
                fill_verts: Some((offset, geometry.vertices.len())),
                ..Drawable::default()
            });
        }

        cmd.triangles_verts = Some(self.push_clip_quad(resolve));
        self.append_cmd(cmd);
    }
}

/// `clear_rect` clears the whole stencil unless a clip is armed on the
/// target it clears, when only the winding bits may go: the command carries
/// that decision so a tiler takes its tile clear whenever it can.
#[test]
fn clear_rect_keeps_the_clip_plane_only_while_a_clip_is_armed() {
    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let keep_clips = |canvas: &mut Canvas<RecordingRenderer>| -> Vec<bool> {
        canvas.flush_to_output(());
        let commands = recorded_commands.borrow();
        commands
            .iter()
            .filter_map(|cmd| match cmd.cmd_type {
                CommandType::ClearRect { keep_clip, .. } => Some(keep_clip),
                _ => None,
            })
            .collect()
    };

    canvas.clear_rect(0, 0, 100, 100, Color::white());
    assert_eq!(
        keep_clips(&mut canvas),
        vec![false],
        "no clip: the whole stencil is cleared"
    );

    let mut clip = Path::new();
    clip.rect(10.0, 10.0, 50.0, 50.0);
    canvas.save();
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.clear_rect(0, 0, 100, 100, Color::white());
    assert_eq!(
        keep_clips(&mut canvas),
        vec![true],
        "a clip on the screen survives the clear"
    );

    let image = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(image));
    canvas.clear_rect(0, 0, 64, 64, Color::white());
    assert_eq!(
        keep_clips(&mut canvas),
        vec![false],
        "the screen's clip does not gate an image target"
    );
    canvas.set_render_target(RenderTarget::Screen);
    canvas.restore();

    canvas.clear_rect(0, 0, 100, 100, Color::white());
    assert_eq!(
        keep_clips(&mut canvas),
        vec![false],
        "the clip is popped: the whole stencil is cleared again"
    );
}

#[test]
fn consecutive_clip_restores_replay_once_before_the_next_draw() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    let mut clip = Path::new();
    clip.rect(10.0, 10.0, 80.0, 80.0);

    for _ in 0..64 {
        canvas.save();
        canvas.clip_path(&clip, FillRule::NonZero);
    }
    canvas.flush_to_output(());
    for _ in 0..64 {
        canvas.restore();
    }
    assert!(canvas.commands.is_empty());

    canvas.fill_path(&clip, &Paint::color(Color::white()));
    assert_eq!(
        canvas
            .commands
            .iter()
            .filter(|command| matches!(command.cmd_type, CommandType::ClipReset { visible: false }))
            .count(),
        1
    );
    assert!(!canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::ClipFill)));
}

#[test]
fn same_size_set_size_does_not_replay_a_clip() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    let mut clip = Path::new();
    clip.rect(10.0, 10.0, 80.0, 80.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.flush_to_output(());

    canvas.set_size(100, 100, 1.0);
    assert!(!canvas
        .commands
        .iter()
        .any(|command| { matches!(command.cmd_type, CommandType::ClipFill | CommandType::ClipReset { .. }) }));

    canvas.set_size(120, 120, 1.0);
    assert!(!canvas
        .commands
        .iter()
        .any(|command| { matches!(command.cmd_type, CommandType::ClipFill | CommandType::ClipReset { .. }) }));
    canvas.fill_path(&clip, &Paint::color(Color::white()));
    assert_eq!(
        canvas
            .commands
            .iter()
            .filter(|command| matches!(command.cmd_type, CommandType::ClipFill))
            .count(),
        1
    );
}

#[test]
fn nested_clip_resolve_is_bounded_by_the_outer_clip() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    let mut outer = Path::new();
    outer.rect(10.0, 20.0, 30.0, 40.0);
    let mut inner = Path::new();
    inner.rect(15.0, 25.0, 10.0, 10.0);
    canvas.clip_path(&outer, FillRule::NonZero);
    canvas.clip_path(&inner, FillRule::NonZero);

    let (resolve_start, resolve_len) = canvas
        .commands
        .iter()
        .filter(|command| matches!(command.cmd_type, CommandType::ClipFill))
        .nth(1)
        .and_then(|command| command.triangles_verts)
        .unwrap();
    assert_eq!(resolve_len, 4);
    let resolve = &canvas.verts[resolve_start..resolve_start + resolve_len];
    assert!(resolve.iter().all(|vertex| (9.0..=41.0).contains(&vertex.x)));
    assert!(resolve.iter().all(|vertex| (19.0..=61.0).contains(&vertex.y)));
}

#[test]
fn many_contour_clip_uses_one_winding_drawable() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    let mut clip = Path::new();
    for inset in 0..256 {
        let inset = inset as f32 * 0.01;
        clip.rect(inset, inset, 10.0, 10.0);
    }

    canvas.clip_path(&clip, FillRule::NonZero);

    let command = canvas
        .commands
        .iter()
        .find(|command| matches!(command.cmd_type, CommandType::ClipFill))
        .unwrap();
    assert_eq!(command.drawables.len(), 1);
    assert_eq!(command.drawables[0].fill_verts.map(|(_, len)| len), Some(256 * 6));
}

#[test]
fn reallocating_an_image_marks_its_clip_plane_for_replay() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let image = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(image));
    let mut clip = Path::new();
    clip.rect(0.0, 0.0, 16.0, 32.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.flush_to_output(());

    canvas
        .realloc_image(image, 48, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    assert_eq!(canvas.renderer.image_deletion_count, 1);
    assert!(canvas.clip_planes[&RenderTarget::Image(image)].dirty);
    canvas.fill_path(&clip, &Paint::color(Color::white()));
    assert!(!canvas.clip_planes[&RenderTarget::Image(image)].dirty);
    assert!(canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::ClipFill)));
}

#[test]
fn filtering_an_image_marks_its_clip_plane_for_replay() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let source = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let target = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(target));
    let mut clip = Path::new();
    clip.rect(0.0, 0.0, 16.0, 32.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.flush_to_output(());

    canvas.filter_image(target, ImageFilter::identity(), source);
    assert!(canvas.clip_planes[&RenderTarget::Image(target)].dirty);
    canvas.fill_path(&clip, &Paint::color(Color::white()));

    let filter = canvas
        .commands
        .iter()
        .position(|command| matches!(command.cmd_type, CommandType::RenderFilteredImage { .. }))
        .unwrap();
    let replay = canvas
        .commands
        .iter()
        .rposition(|command| matches!(command.cmd_type, CommandType::ClipFill))
        .unwrap();
    assert!(replay > filter);
}

/// A filter pass draws into its own target image, never into the target a
/// clip gates, so it must be recorded ungated under an active clip: a backend
/// that stencil-tests it (OpenGL) would otherwise test the filter quad against
/// the target image's blank plane and drop the whole pass - every layer
/// filter and every blurred shadow drawn under a clip.
#[test]
fn filter_passes_are_not_gated_by_the_active_clip() {
    use renderer::CommandType;

    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let source = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let target = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();

    let mut clip = Path::new();
    clip.rect(0.0, 0.0, 50.0, 50.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.filter_image(target, ImageFilter::GaussianBlur { sigma: 2.0 }, source);
    let mut path = Path::new();
    path.rect(10.0, 10.0, 30.0, 30.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(255, 0, 0)));
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    let filter = commands
        .iter()
        .find(|c| matches!(c.cmd_type, CommandType::RenderFilteredImage { .. }))
        .expect("the filter pass is recorded");
    assert!(!filter.clip_active, "a filter pass is not gated by the clip");
    let fill = commands
        .iter()
        .find(|c| matches!(c.cmd_type, CommandType::ConvexFill { .. }))
        .expect("the fill is recorded");
    assert!(fill.clip_active, "the fill under the clip is gated");
}

#[test]
fn a_suppressed_draw_does_not_consume_a_dirty_clip_replay() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(1, 1, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();

    let mut outer = Path::new();
    outer.rect(4.0, 4.0, 56.0, 56.0);
    canvas.clip_path(&outer, FillRule::NonZero);
    canvas.save();
    let mut inner = Path::new();
    inner.rect(8.0, 8.0, 32.0, 32.0);
    canvas.clip_path(&inner, FillRule::NonZero);
    canvas.flush_to_output(());
    canvas.restore();
    assert!(canvas.clip_planes[&RenderTarget::Screen].dirty);

    canvas.set_transient_image_budget(0);
    let masked = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&masked));
    let mut rect = Path::new();
    rect.rect(0.0, 0.0, 64.0, 64.0);
    canvas.fill_path(&rect, &Paint::color(Color::white()));
    assert!(canvas.clip_planes[&RenderTarget::Screen].dirty);
    canvas.end_layer();

    canvas.fill_path(&rect, &Paint::color(Color::white()));
    assert!(!canvas.clip_planes[&RenderTarget::Screen].dirty);
    assert!(canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::ClipReset { visible: true })));
    assert!(canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::ClipFill)));
}

/// Balanced save / clip_path / restore past the limit records no clip and
/// no geometry, and reset() drops the entries past the limit with the
/// layers it discards.
#[cfg(test)]
#[test]
fn clips_past_the_depth_limit_cost_nothing_and_reset_clears_the_overflow() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    while canvas.state_stack.len() < MAX_STATE_DEPTH {
        canvas.save();
    }
    let mut clip = Path::new();
    clip.rect(0.0, 0.0, 32.0, 64.0);
    for _ in 0..32 {
        canvas.save();
        canvas.clip_path(&clip, FillRule::NonZero);
        canvas.restore();
    }
    assert!(canvas.clip_stack.is_empty());
    assert!(canvas.clip_planes.is_empty());
    assert!(!canvas.saturated());

    for _ in 0..3 {
        canvas.save();
    }
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    assert_eq!(canvas.overflow.len(), 4);
    canvas.reset();
    assert!(!canvas.saturated());
    assert!(canvas.layers.is_empty());
    assert!(canvas.warned_overflow, "one warning per canvas");
}
