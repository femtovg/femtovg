//! Clipping to a path: the clip stack, the stencil plane an arbitrary path
//! is rasterized into, its replay per target and the quads that arm and
//! resolve it, and the shapes that skip the stencil ([`shape`]).

use super::*;

mod shape;
pub(crate) use shape::{ClipCoverage, RoundedBox};

#[derive(Debug)]
pub(crate) struct ClipGeometry {
    pub(crate) vertices: Box<[Vertex]>,
}

#[derive(Debug)]
pub(crate) struct ClipEntry {
    pub(crate) target: RenderTarget,
    pub(crate) kind: ClipKind,
}

#[derive(Debug)]
pub(crate) enum ClipKind {
    /// Rasterized into the target's stencil clip plane.
    Stencil {
        geometry: Rc<ClipGeometry>,
        fill_rule: FillRule,
        bounds: Bounds,
        prior_armed: Rect,
        armed: Rect,
    },
    /// Evaluated by each draw's fragment shader, in device space, as the
    /// coverage worked out when the clip was taken. The innermost one on a
    /// target is the one in force: each is stacked as what it leaves of the
    /// one before it.
    Shape { shape: RoundedBox, coverage: ClipCoverage },
}

/// The last scissor and clip shape a draw met together, and the one box
/// they make, if they do: draws under the same two skip working it out.
#[derive(Copy, Clone, Debug)]
pub(crate) struct ScissoredShape {
    scissor: Scissor,
    shape: RoundedBox,
    both: Option<(RoundedBox, ClipCoverage)>,
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
            if let ClipKind::Stencil { prior_armed, .. } = entry.kind {
                removed
                    .entry(entry.target)
                    .and_modify(|(count, _)| *count += 1)
                    .or_insert((1, prior_armed));
            }
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
    /// A clip whose path outlines a rectangle (under any transform), a
    /// rounded rectangle or an ellipse is antialiased: its edge takes
    /// coverage as a fill's does. The edge of any other clip is not: a pixel
    /// is either inside it or outside. A shape loses its antialiasing when
    /// it only partly overlaps a shape already clipping the target (two
    /// rectangles with parallel sides excepted), and once something that
    /// reaches past it is drawn under it with a composite operation that
    /// changes the destination where the source is transparent
    /// ([`CompositeOperation::Copy`], `SourceIn`, `SourceOut`,
    /// `DestinationIn`, `DestinationAtop`).
    ///
    /// A draw that stays inside an antialiased clip is not clipped: an edge
    /// it shares with the clip is its own. A draw that reaches past the clip
    /// takes the clip's coverage over its own, so that where the two share
    /// an edge a pixel half inside is a quarter covered - except an upright
    /// rectangle filled under an upright rectangular clip, which is cut to
    /// the clip. A scissor that makes one box with the clip - around it,
    /// inside it, or two rectangles with parallel sides - clips with it as
    /// that box: an edge the two share takes coverage once.
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
        if self.clip_to_shape(path) {
            return;
        }
        self.reconcile_current_clip_plane();
        let target = self.current_render_target;
        let (geometry, bounds) = self.stencil_geometry(path, &self.state().transform);
        let target_rect = self.render_target_rect();
        let path_rect = Self::clip_bounds(bounds, target_rect);
        let previous_armed = self.clip_planes.get(&target).map_or(target_rect, |plane| plane.armed);

        if !self.clip_active() {
            self.emit_clip_reset(true);
        }
        self.emit_clip_fill(&geometry, fill_rule, previous_armed);
        let armed = previous_armed.intersect(path_rect);
        self.clip_stack.push(ClipEntry {
            target,
            kind: ClipKind::Stencil {
                geometry,
                fill_rule,
                bounds,
                prior_armed: previous_armed,
                armed,
            },
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

    /// Takes the clip as a shape for the fragment shader, if `path` outlines
    /// one and it combines with the shape already in force into one: a draw
    /// carries a single shape. `false` leaves the clip to the stencil.
    fn clip_to_shape(&mut self, path: &Path) -> bool {
        let Some(shape) = RoundedBox::fit(path) else {
            return false;
        };
        let mut shape = shape.transformed(&self.state().transform);
        let Some(mut coverage) = shape.coverage(self.fringe_width) else {
            return false;
        };
        if let Some((current, current_coverage)) = self.clip_shape() {
            if let Some(both) = current.intersection(&shape, self.fringe_width) {
                if both == current {
                    // The shape in force already clips to less.
                    return true;
                }
                let Some(both_coverage) = both.coverage(self.fringe_width) else {
                    return false;
                };
                (shape, coverage) = (both, both_coverage);
            } else if coverage.contains(&current) {
                return true;
            } else if !current_coverage.contains(&shape) {
                return false;
            }
        }
        self.clip_stack.push(ClipEntry {
            target: self.current_render_target,
            kind: ClipKind::Shape { shape, coverage },
        });
        self.state_mut().clip_depth = self.clip_stack.len();
        true
    }

    /// A path's winding fans under `transform`, for the stencil, and their bounds.
    fn stencil_geometry(&self, path: &Path, transform: &Transform2D) -> (Rc<ClipGeometry>, Bounds) {
        let path_cache = path.cache(transform, self.tess_tol, self.dist_tol);
        let geometry = ClipGeometry {
            vertices: path_cache.winding_triangles().into_boxed_slice(),
        };
        (Rc::new(geometry), path_cache.bounds)
    }

    /// The clip shape in force as a draw meets it, and the scissor left to
    /// apply beside it: none when the scissor and the shape make one box
    /// ([`RoundedBox::with_scissor`]), which then stands for both.
    fn scissored_shape(&mut self) -> Option<(RoundedBox, ClipCoverage, Scissor)> {
        let (shape, coverage) = self.clip_shape()?;
        let scissor = self.state().scissor;
        let Some(extent) = scissor.extent else {
            return Some((shape, coverage, scissor));
        };
        let both = match self.last_scissored_shape {
            Some(last) if last.scissor == scissor && last.shape == shape => last.both,
            _ => {
                let scissor_box = RoundedBox {
                    frame: scissor.transform,
                    extent,
                    radii: [scissor.radius; 2],
                };
                let both = shape.with_scissor(&scissor_box, self.fringe_width);
                self.last_scissored_shape = Some(ScissoredShape { scissor, shape, both });
                both
            }
        };
        Some(match both {
            Some((both, coverage)) => (both, coverage, Scissor::default()),
            None => (shape, coverage, scissor),
        })
    }

    /// The clip shape and the scissor a draw over `bounds` carries; `bounds`
    /// is asked for only under a shape.
    pub(crate) fn draw_clip(&mut self, bounds: impl FnOnce() -> Bounds) -> (Option<ClipCoverage>, Scissor) {
        let (clip, scissor, _) = self.fill_clip(bounds, None);
        (clip, scissor)
    }

    /// The clip shape and the scissor a draw over `bounds` carries, and for
    /// an antialiased fill - `fill`, its path and transform - the rect to
    /// fill in the path's place ([`RoundedBox::shared_rect`]).
    ///
    /// The shape takes nothing from a draw it holds whole, or from that rect:
    /// such a draw carries no shape - or, after a draw that carried one, a
    /// coverage of one everywhere, so that the renderer goes on with the
    /// shader variant it has bound. An operation that changes the
    /// destination where its source is transparent would change the pixels
    /// outside the shape too, so before a draw that reaches them the shapes
    /// move to the stencil.
    pub(crate) fn fill_clip(
        &mut self,
        bounds: impl FnOnce() -> Bounds,
        fill: Option<(&Path, &Transform2D)>,
    ) -> (Option<ClipCoverage>, Scissor, Option<RoundedBox>) {
        let Some((shape, coverage, scissor)) = self.scissored_shape() else {
            self.shape_carried = false;
            return (None, self.state().scissor, None);
        };
        let held = coverage.holds(&bounds());
        let rect = match fill {
            Some((path, transform)) if !held => shape.shared_rect(path, transform, self.fringe_width),
            _ => None,
        };
        if held || rect.is_some() {
            return (self.shape_carried.then_some(ClipCoverage::EVERYWHERE), scissor, rect);
        }
        self.shape_carried = self.state().composite_operation.takes_coverage();
        if self.shape_carried {
            return (Some(coverage), scissor, None);
        }
        let target = self.current_render_target;
        // The shapes beneath the one in force contain it, but each would be
        // in force once it is gone: all of the target's shapes move.
        let mut moved = false;
        for index in 0..self.clip_stack.len() {
            let ClipEntry {
                target: on,
                kind: ClipKind::Shape { shape, .. },
            } = self.clip_stack[index]
            else {
                continue;
            };
            if on != target {
                continue;
            }
            let (geometry, bounds) = self.stencil_geometry(&shape.path(), &shape.frame);
            // The replay below works out what each entry leaves armed.
            self.clip_stack[index].kind = ClipKind::Stencil {
                geometry,
                fill_rule: FillRule::NonZero,
                bounds,
                prior_armed: Rect::default(),
                armed: Rect::default(),
            };
            let armed = self.render_target_rect();
            let plane = self.clip_planes.entry(target).or_insert(ClipPlaneState {
                count: 0,
                dirty: false,
                armed,
            });
            plane.count += 1;
            plane.dirty = true;
            moved = true;
        }
        if moved {
            self.reconcile_current_clip_plane();
        }
        (None, self.state().scissor, None)
    }

    /// The shape clip in force on the current render target, and its coverage.
    pub(crate) fn clip_shape(&self) -> Option<(RoundedBox, ClipCoverage)> {
        let target = self.current_render_target;
        self.clip_stack.iter().rev().find_map(|entry| match entry.kind {
            ClipKind::Shape { shape, coverage } if entry.target == target => Some((shape, coverage)),
            _ => None,
        })
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
            .filter_map(|entry| match &entry.kind {
                ClipKind::Stencil {
                    geometry,
                    fill_rule,
                    bounds,
                    ..
                } => Some((geometry.clone(), *fill_rule, *bounds)),
                ClipKind::Shape { .. } => None,
            })
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
        let stencils = self
            .clip_stack
            .iter_mut()
            .filter(|entry| entry.target == target)
            .filter_map(|entry| match &mut entry.kind {
                ClipKind::Stencil { prior_armed, armed, .. } => Some((prior_armed, armed)),
                ClipKind::Shape { .. } => None,
            });
        for ((prior_armed, armed), (prior, now)) in stencils.zip(armed_values) {
            *prior_armed = prior;
            *armed = now;
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

/// A clip only the stencil can take: the rect with a notch in its top side,
/// so it has the rect's bounds and is no box.
#[cfg(test)]
pub(crate) fn notched_rect(x: f32, y: f32, w: f32, h: f32) -> Path {
    let mut path = Path::new();
    path.move_to(x, y);
    path.line_to(x, y + h);
    path.line_to(x + w, y + h);
    path.line_to(x + w, y);
    path.line_to(x + w * 0.5, y + h * 0.25);
    path.close();
    path
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

    let clip = notched_rect(10.0, 10.0, 50.0, 50.0);
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
    let clip = notched_rect(10.0, 10.0, 80.0, 80.0);

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
    let clip = notched_rect(10.0, 10.0, 80.0, 80.0);
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
    let outer = notched_rect(10.0, 20.0, 30.0, 40.0);
    let inner = notched_rect(15.0, 25.0, 10.0, 10.0);
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
    let clip = notched_rect(0.0, 0.0, 16.0, 32.0);
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
    let clip = notched_rect(0.0, 0.0, 16.0, 32.0);
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

    let clip = notched_rect(0.0, 0.0, 50.0, 50.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.filter_image(target, ImageFilter::gaussian_blur(2.0), source);
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

    let outer = notched_rect(4.0, 4.0, 56.0, 56.0);
    canvas.clip_path(&outer, FillRule::NonZero);
    canvas.save();
    let inner = notched_rect(8.0, 8.0, 32.0, 32.0);
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
    let clip = notched_rect(0.0, 0.0, 32.0, 64.0);
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

/// What the draws of a flush carried as their clip shape, in order.
#[cfg(test)]
fn drawn_clips(commands: &[Command]) -> Vec<Option<ClipCoverage>> {
    commands
        .iter()
        .filter_map(|cmd| match &cmd.cmd_type {
            CommandType::ConvexFill { params }
            | CommandType::Stroke { params }
            | CommandType::Triangles { params }
            | CommandType::ConcaveFill {
                fill_params: params, ..
            }
            | CommandType::StencilStroke { params1: params, .. } => Some(params.clip),
            _ => None,
        })
        .collect()
}

/// What each draw of a flush carried: no shape, a coverage of one
/// everywhere, or a shape's.
#[cfg(test)]
fn carried(commands: &[Command]) -> Vec<&'static str> {
    drawn_clips(commands)
        .iter()
        .map(|clip| match clip {
            None => "none",
            Some(coverage) if *coverage == ClipCoverage::EVERYWHERE => "everywhere",
            Some(_) => "shape",
        })
        .collect()
}

#[cfg(test)]
fn stencil_clip_commands(commands: &[Command]) -> usize {
    commands
        .iter()
        .filter(|cmd| matches!(cmd.cmd_type, CommandType::ClipFill | CommandType::ClipReset { .. }))
        .count()
}

/// A rectangle, a rounded rectangle and an ellipse clip without the stencil:
/// the draws under them carry the shape, in device pixels, until the restore.
#[test]
fn a_box_clip_is_a_shape_on_the_draws_under_it() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(200, 200, 2.0);
    // No rect: a rect under a rect clip would be drawn as what they share.
    let fill = notched_rect(0.0, 0.0, 200.0, 200.0);
    let mut stroke = Path::new();
    stroke.move_to(0.0, 0.0);
    stroke.line_to(200.0, 200.0);
    let paint = Paint::color(Color::black());

    let mut rounded = Path::new();
    rounded.rounded_rect(20.0, 40.0, 80.0, 40.0, 10.0);
    let mut circle = Path::new();
    circle.circle(50.0, 50.0, 30.0);
    let mut rect = Path::new();
    rect.rect(20.0, 40.0, 80.0, 40.0);
    let half_pixel = 0.5; // the fringe at a device pixel ratio of two
    for (clip, extent, radii) in [
        (&rect, [20.0, 40.0], [0.0, 0.0]), // a rect's first side runs down
        (&rounded, [40.0, 20.0], [10.0, 10.0]),
        (&circle, [30.0, 30.0], [30.0, 30.0]),
    ] {
        canvas.save();
        canvas.translate(10.0, 0.0);
        canvas.clip_path(clip, FillRule::NonZero);
        assert!(!canvas.clip_active(), "the stencil plane is not armed");
        canvas.reset_transform();
        canvas.fill_path(&fill, &paint);
        canvas.stroke_path(&stroke, &paint);
        canvas.restore();
        canvas.fill_path(&fill, &paint);
        canvas.flush_to_output(());

        let commands = recorded.borrow();
        assert_eq!(stencil_clip_commands(&commands), 0);
        let clips = drawn_clips(&commands);
        let [Some(filled), Some(stroked), None] = clips[..] else {
            panic!("fill and stroke clipped, the fill after the restore not: {clips:?}");
        };
        assert_eq!(filled, stroked);
        for (got, want) in filled
            .extent
            .iter()
            .chain(&filled.radii)
            .zip(extent.iter().chain(&radii))
        {
            assert!((got - want / half_pixel).abs() < 1e-2, "{filled:?}");
        }
        // Taken under the translation, 10 further right: the point lies 15
        // units inside the boxes and 25 inside the circle, two pixels a unit.
        let (center_x, depth) = if radii[0] == 30.0 { (60.0, 50.0) } else { (70.0, 30.0) };
        assert!((filled.distance([center_x, 55.0]) + depth).abs() < 1e-2, "{filled:?}");
    }
}

/// A draw the shape holds whole carries no shape - a fill, a stroke with its
/// width and its miters, glyph quads - and one that reaches past the shape
/// carries it. After a draw that carried the shape, a held one carries a
/// coverage of one, for the renderer to stay on the variant it has bound.
/// Held, a draw under an operation coverage cannot bound leaves the shape a
/// shape.
#[test]
fn a_draw_the_shape_holds_carries_no_shape() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let mut clip = Path::new();
    clip.rounded_rect(10.0, 10.0, 80.0, 80.0, 20.0);
    canvas.clip_path(&clip, FillRule::NonZero);

    let triangle = |x: f32, y: f32, size: f32| {
        let mut path = Path::new();
        path.move_to(x, y);
        path.line_to(x + size, y);
        path.line_to(x, y + size);
        path.close();
        path
    };
    let line = |x0: f32, y0: f32, x1: f32, y1: f32, x2: f32, y2: f32| {
        let mut path = Path::new();
        path.move_to(x0, y0);
        path.line_to(x1, y1);
        path.line_to(x2, y2);
        path
    };
    let wide = paint.clone().with_line_width(6.0).with_line_join(LineJoin::Round);
    let mitered = paint.clone().with_line_width(6.0).with_miter_limit(4.0);
    let quad = |x: f32, y: f32| {
        [(0.0, 0.0), (8.0, 8.0), (8.0, 0.0), (0.0, 0.0), (0.0, 8.0), (8.0, 8.0)]
            .map(|(dx, dy)| Vertex::new(x + dx, y + dy, 0.0, 0.0))
    };
    let flavor = PaintFlavor::Color(Color::black());

    // Held: inside the straight sides, clear of the corner arcs.
    canvas.fill_path(&triangle(30.0, 30.0, 40.0), &paint);
    canvas.stroke_path(&line(30.0, 40.0, 50.0, 60.0, 70.0, 40.0), &wide);
    canvas.render_triangles(&quad(40.0, 40.0), &Transform2D::identity(), &flavor, GlyphTexture::None);
    // Not held: past a side, in a corner the arc cuts, a stroke whose width
    // or whose miter reaches out, and a quad on the edge.
    canvas.fill_path(&triangle(30.0, 30.0, 70.0), &paint);
    canvas.fill_path(&triangle(11.0, 11.0, 10.0), &paint);
    canvas.stroke_path(&line(12.0, 40.0, 12.0, 50.0, 12.0, 60.0), &wide);
    canvas.stroke_path(&line(30.0, 16.0, 50.0, 22.0, 70.0, 16.0), &mitered);
    canvas.render_triangles(&quad(86.0, 40.0), &Transform2D::identity(), &flavor, GlyphTexture::None);

    canvas.global_composite_operation(CompositeOperation::Copy);
    canvas.fill_path(&triangle(30.0, 30.0, 40.0), &paint);
    assert!(
        canvas.clip_shape().is_some() && !canvas.clip_active(),
        "held: nothing to bound"
    );
    canvas.fill_path(&triangle(30.0, 30.0, 70.0), &paint);
    assert!(
        canvas.clip_shape().is_none() && canvas.clip_active(),
        "past the shape: the stencil"
    );

    canvas.flush_to_output(());
    assert_eq!(
        carried(&recorded.borrow()),
        [
            "none",
            "none",
            "none",
            "shape",
            "shape",
            "shape",
            "shape",
            "shape",
            "everywhere",
            "none"
        ],
        "three held, five not, and the two copies: one held, one on the stencil"
    );

    // Past the clip's restore a draw carries nothing again.
    canvas.reset();
    canvas.save();
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.fill_path(&triangle(30.0, 30.0, 70.0), &paint);
    canvas.fill_path(&triangle(30.0, 30.0, 40.0), &paint);
    canvas.restore();
    canvas.fill_path(&triangle(30.0, 30.0, 40.0), &paint);
    canvas.flush_to_output(());
    assert_eq!(carried(&recorded.borrow()), ["shape", "everywhere", "none"]);
}

/// An antialiased upright rect under an upright rect clip is drawn as the
/// rect the two share, with no clip: one convex fill with its fringe. A
/// rounded twin, a fill without antialiasing and a fill that is no rect keep
/// their outline and carry the shape.
#[test]
fn an_upright_rect_under_an_upright_rect_clip_is_what_they_share() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let drawn = renderer.last_verts.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let rect = |x: f32, y: f32, w: f32, h: f32| {
        let mut path = Path::new();
        path.rect(x, y, w, h);
        path
    };
    let mut triangle = Path::new();
    triangle.move_to(0.0, 0.0);
    triangle.line_to(100.0, 0.0);
    triangle.line_to(0.0, 100.0);
    triangle.close();
    let bounds_drawn = |cmd: &Command| {
        let (start, count) = cmd.drawables[0].fill_verts.unwrap();
        let verts = &drawn.borrow()[start..start + count];
        let pick = |f: fn(f32, f32) -> f32, seed: f32, of: fn(&Vertex) -> f32| verts.iter().map(of).fold(seed, f);
        [
            pick(f32::min, f32::INFINITY, |v| v.x),
            pick(f32::min, f32::INFINITY, |v| v.y),
            pick(f32::max, f32::NEG_INFINITY, |v| v.x),
            pick(f32::max, f32::NEG_INFINITY, |v| v.y),
        ]
    };

    canvas.save();
    canvas.clip_path(&rect(10.0, 20.0, 80.0, 60.0), FillRule::NonZero);
    canvas.fill_path(&rect(0.0, 0.0, 100.0, 100.0), &paint);
    canvas.fill_path(&rect(50.0, 20.0, 80.0, 30.0), &paint);
    canvas.fill_path(&rect(0.0, 0.0, 100.0, 100.0), &paint.clone().with_anti_alias(false));
    canvas.fill_path(&triangle, &paint);
    canvas.restore();
    let mut rounded = Path::new();
    rounded.rounded_rect(10.0, 20.0, 80.0, 60.0, 15.0);
    canvas.clip_path(&rounded, FillRule::NonZero);
    canvas.fill_path(&rounded, &paint);
    canvas.fill_path(&rect(10.0, 20.0, 80.0, 60.0), &paint);
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    let fills: Vec<&Command> = commands
        .iter()
        .filter(|cmd| matches!(cmd.cmd_type, CommandType::ConvexFill { .. }))
        .collect();
    assert_eq!(
        carried(&commands),
        ["none", "none", "shape", "shape", "shape", "shape"],
        "what the clip cuts to a rect carries nothing"
    );
    // The fill's own vertices are a half fringe inside its outline.
    assert_eq!(
        bounds_drawn(fills[0]),
        [10.5, 20.5, 89.5, 79.5],
        "around: the clip's rect"
    );
    assert_eq!(
        bounds_drawn(fills[1]),
        [50.5, 20.5, 89.5, 49.5],
        "across: what both cover"
    );
    assert_eq!(
        bounds_drawn(fills[2]),
        [0.0, 0.0, 100.0, 100.0],
        "without antialiasing: as it is"
    );
    assert!(
        canvas.clip_shape().is_some() && !canvas.clip_active(),
        "no stencil in any of this"
    );
}

/// A scissor and the clip shape that make one box reach a draw as that box
/// alone: a draw that crosses it carries the box and no scissor, one the
/// box holds neither. A scissor that makes no one box with the shape stays.
#[test]
fn a_scissor_and_the_shape_reach_a_draw_as_one_box() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let mut triangle = Path::new();
    triangle.move_to(0.0, 0.0);
    triangle.line_to(100.0, 0.0);
    triangle.line_to(0.0, 100.0);
    triangle.close();
    let mut small = Path::new();
    small.move_to(30.0, 30.0);
    small.line_to(50.0, 30.0);
    small.line_to(30.0, 50.0);
    small.close();
    let mut clip = Path::new();
    clip.rounded_rect(10.0, 20.0, 80.0, 60.0, 15.0);
    let scissors = |commands: &[Command]| -> Vec<bool> {
        commands
            .iter()
            .filter_map(|cmd| match &cmd.cmd_type {
                CommandType::ConvexFill { params } => Some(params.scissor_mat != [0.0; 12]),
                _ => None,
            })
            .collect()
    };

    // The viewport of an export: a scissor with the clip's own bounds.
    canvas.scissor(10.0, 20.0, 80.0, 60.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.fill_path(&triangle, &paint);
    canvas.fill_path(&small, &paint);
    // A scissor across the clip's corners makes no one box with it.
    canvas.scissor(20.0, 20.0, 80.0, 60.0);
    canvas.fill_path(&triangle, &paint);
    canvas.fill_path(&small, &paint);
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    assert_eq!(carried(&commands), ["shape", "everywhere", "shape", "everywhere"]);
    assert_eq!(scissors(&commands), [false, false, true, true]);
    let carried = drawn_clips(&commands)[0].unwrap();
    assert_eq!(
        (carried.extent, carried.radii),
        ([40.0, 30.0], [15.0, 15.0]),
        "the clip, which the scissor holds"
    );
}

/// One shape per draw: a shape inside the one in force takes its place, one
/// around it adds nothing, a rect across a rect leaves their intersection,
/// and any other overlap goes to the stencil.
#[test]
fn nested_shape_clips_keep_one_shape_in_force() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let rect = |x: f32, y: f32, w: f32, h: f32| {
        let mut path = Path::new();
        path.rect(x, y, w, h);
        path
    };
    let fill = notched_rect(0.0, 0.0, 100.0, 100.0);
    let paint = Paint::color(Color::black());
    let half_width_in_force = |canvas: &mut Canvas<RecordingRenderer>| {
        canvas.fill_path(&fill, &paint);
        canvas.flush_to_output(());
        drawn_clips(&recorded.borrow()).last().unwrap().unwrap().extent[0]
    };

    canvas.clip_path(&rect(10.0, 10.0, 80.0, 80.0), FillRule::NonZero);
    canvas.save();
    canvas.clip_path(&rect(20.0, 20.0, 40.0, 40.0), FillRule::NonZero);
    assert_eq!(canvas.clip_stack.len(), 2, "inside: stacked");
    assert_eq!(half_width_in_force(&mut canvas), 20.0);
    canvas.clip_path(&rect(0.0, 0.0, 100.0, 100.0), FillRule::NonZero);
    assert_eq!(canvas.clip_stack.len(), 2, "around: nothing to add");
    assert_eq!(half_width_in_force(&mut canvas), 20.0);
    canvas.restore();
    assert_eq!(half_width_in_force(&mut canvas), 40.0, "the outer shape is back");

    // Overlapping rects with parallel sides: what both cover, x 50..90.
    canvas.clip_path(&rect(50.0, 50.0, 80.0, 80.0), FillRule::NonZero);
    assert_eq!(half_width_in_force(&mut canvas), 20.0);
    assert!(!canvas.clip_active(), "still no stencil");

    // A circle that overlaps the rect is no single shape with it.
    let mut circle = Path::new();
    circle.circle(50.0, 50.0, 30.0);
    canvas.clip_path(&circle, FillRule::NonZero);
    assert!(canvas.clip_active(), "the stencil takes it");
    assert_eq!(half_width_in_force(&mut canvas), 20.0, "the rect stays in force");
    assert_eq!(stencil_clip_commands(&recorded.borrow()), 2, "armed and filled");
}

/// A rounded rect nested in its twin, or in one a fraction of a pixel off -
/// the double clip design tools export - stays one shape.
#[test]
fn a_rounded_clip_nested_in_its_twin_stays_a_shape() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    let rounded = |x: f32, y: f32| {
        let mut path = Path::new();
        path.rounded_rect(x, y, 80.0, 60.0, 16.0);
        path
    };
    canvas.clip_path(&rounded(10.0, 10.0), FillRule::NonZero);
    canvas.clip_path(&rounded(10.0, 10.0), FillRule::NonZero);
    assert_eq!(canvas.clip_stack.len(), 1, "its twin adds nothing");
    // Shifted along a side, the two share a narrower box with the same
    // corners: the left ones of one, the right ones of the other.
    canvas.clip_path(&rounded(10.3, 10.0), FillRule::NonZero);
    canvas.clip_path(&rounded(14.0, 10.0), FillRule::NonZero);
    assert!(!canvas.clip_active());
    let (shape, _) = canvas.clip_shape().unwrap();
    assert!(
        (shape.extent[0] - 38.0).abs() < 1e-3 && (shape.radii[0] - 16.0).abs() < 1e-3,
        "{shape:?}"
    );

    canvas.clip_path(&rounded(18.0, 14.0), FillRule::NonZero);
    assert!(
        canvas.clip_active(),
        "shifted across a corner, what they share has corners of its own: the stencil's"
    );
}

/// A composite operation that changes the destination where the source is
/// transparent cannot be held to a shape by coverage: the target's shapes
/// move to the stencil before the draw and stay there.
#[test]
fn a_draw_coverage_cannot_bound_moves_the_shapes_to_the_stencil() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let fill = notched_rect(0.0, 0.0, 100.0, 100.0);
    let paint = Paint::color(Color::black());
    let mut outer = Path::new();
    outer.circle(50.0, 50.0, 45.0);
    let mut inner = Path::new();
    inner.rect(40.0, 40.0, 20.0, 20.0);

    canvas.save();
    canvas.clip_path(&outer, FillRule::NonZero);
    canvas.clip_path(&inner, FillRule::NonZero);
    for operation in [
        CompositeOperation::SourceOver,
        CompositeOperation::Atop,
        CompositeOperation::DestinationOver,
        CompositeOperation::DestinationOut,
        CompositeOperation::Lighter,
        CompositeOperation::Xor,
    ] {
        canvas.global_composite_operation(operation);
        canvas.fill_path(&fill, &paint);
    }
    assert!(!canvas.clip_active(), "coverage bounds all of these");

    canvas.global_composite_operation(CompositeOperation::Copy);
    canvas.fill_path(&fill, &paint);
    assert_eq!(canvas.clip_planes[&RenderTarget::Screen].count, 2, "both shapes moved");
    assert_eq!(canvas.clip_shape(), None);
    canvas.global_composite_operation(CompositeOperation::SourceOver);
    canvas.fill_path(&fill, &paint);
    canvas.flush_to_output(());
    {
        let commands = recorded.borrow();
        let clips = drawn_clips(&commands);
        assert_eq!(clips.iter().filter(|clip| clip.is_some()).count(), 6);
        assert_eq!(
            clips[6..],
            [None, None],
            "the copy and what follows it are gated by the stencil"
        );
        assert_eq!(stencil_clip_commands(&commands), 3, "armed once, filled twice");
        let copy = commands
            .iter()
            .rposition(|cmd| matches!(cmd.cmd_type, CommandType::ClipFill))
            .unwrap();
        assert!(
            commands[copy + 1..].iter().all(|cmd| cmd.clip_active),
            "after the clip fills every draw is stencil-gated"
        );
    }

    canvas.restore();
    canvas.fill_path(&fill, &paint);
    assert!(
        canvas.clip_stack.is_empty() && !canvas.clip_active(),
        "the restore pops them as stencil clips"
    );

    for operation in [
        CompositeOperation::SourceIn,
        CompositeOperation::SourceOut,
        CompositeOperation::DestinationIn,
        CompositeOperation::DestinationAtop,
    ] {
        assert!(
            !CompositeOperationState::new(operation).takes_coverage(),
            "{operation:?}"
        );
    }
}

/// A shape belongs to the target it was taken on, as a stencil clip does:
/// it gates a layer's composite and not its content, and a shape taken
/// inside the layer gates only that.
#[test]
fn a_shape_clip_gates_only_draws_into_its_target() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let fill = notched_rect(0.0, 0.0, 100.0, 100.0);
    let paint = Paint::color(Color::black());
    let mut outer = Path::new();
    outer.circle(50.0, 50.0, 40.0);
    let mut inner = Path::new();
    inner.rect(60.0, 60.0, 30.0, 30.0);

    canvas.clip_path(&outer, FillRule::NonZero);
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    canvas.fill_path(&fill, &paint);
    canvas.clip_path(&inner, FillRule::NonZero);
    canvas.fill_path(&fill, &paint);
    canvas.end_layer();
    canvas.flush_to_output(());
    let clips = drawn_clips(&recorded.borrow());
    let [None, Some(in_layer), Some(composite)] = clips[..] else {
        panic!("{clips:?}");
    };
    assert_eq!(in_layer.radii, [0.0, 0.0], "the layer's own rect, in its store's space");
    assert_eq!(composite.radii, [40.0, 40.0], "the canvas's circle on the composite");

    // An image target's shape leaves with the image.
    let image = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(image));
    canvas.clip_path(&inner, FillRule::NonZero);
    assert_eq!(canvas.clip_stack.len(), 2);
    canvas.set_render_target(RenderTarget::Screen);
    canvas.delete_image(image);
    assert_eq!(canvas.clip_stack.len(), 1, "only the screen's circle is left");
}

/// The unclipped image blit would bypass a shape as it would the stencil.
#[test]
fn an_image_blit_under_a_shape_clip_takes_the_masked_path() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let image = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let mut clip = Path::new();
    clip.circle(16.0, 16.0, 12.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    let mut blit = Path::new();
    blit.rect(0.0, 0.0, 32.0, 32.0);
    let mut paint = Paint::image(image, 0.0, 0.0, 32.0, 32.0, 0.0, 1.0);
    paint.set_anti_alias(false);
    canvas.fill_path(&blit, &paint);
    canvas.flush_to_output(());
    let clips = drawn_clips(&recorded.borrow());
    assert!(matches!(clips[..], [Some(_)]), "{clips:?}");
    assert!(recorded.borrow().iter().all(|cmd| !matches!(
        cmd.cmd_type,
        CommandType::Triangles {
            params: Params {
                shader_type: ShaderType::TextureCopyUnclipped,
                ..
            }
        }
    )));
}
