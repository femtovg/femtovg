//! What fits: the transient image budget, the filter work budget, the state
//! depth limit and the store sizing against the texture limit.

use super::*;

/// How a Gaussian blur of `sigma` runs within the shader's per-pass bound
/// ([`renderer::MAX_BLUR_SIGMA`]): `(passes, sigma per pass)`. Gaussians
/// compose in quadrature - k passes of sigma s blur like one pass of
/// s * sqrt(k) - so a sigma above the bound B is exactly k = ceil((sigma / B)^2)
/// passes of sigma / sqrt(k), each at most B: sigma 16 is four passes of 8,
/// sigma 23 nine of 23/3. A sigma within the bound, or a degenerate one, is
/// one pass with the value untouched, so small blurs render exactly as they
/// did before the split existed. The cost is quadratic in sigma (each pass is
/// two full-size draws), which is what the ceiling above bounds.
/// The blur padding a store of `extent` px can afford under the backend's
/// texture `limit`, given that stores round up to `granularity`: the full
/// `pad` when it fits, else what leaves the rounded store within the limit,
/// never negative. A layer or shadow at the limit then captures with its
/// reach truncated at the store edge rather than passing through.
pub(crate) fn bounded_pad(pad: f32, extent: f32, limit: usize, granularity: usize) -> f32 {
    if pad <= 0.0 {
        return 0.0;
    }
    let room = limit as f32 - extent.ceil() - granularity as f32;
    pad.min((room * 0.5).floor().max(0.0))
}

/// `lo..hi` grown by up to `before` and `after` and rounded outward, as
/// far as a store of `limit` pixels leaves room; short of room, each side
/// gets its share of what is left.
pub(crate) fn reach_within(lo: f32, hi: f32, before: f32, after: f32, limit: usize) -> (f32, f32) {
    let fit = (limit - limit % transient::LAYER_GRANULARITY) as f32;
    let grown = ((lo - before).floor(), (hi + after).ceil());
    if before + after <= 0.0 || grown.1 - grown.0 <= fit {
        return grown;
    }
    let (lo, hi) = (lo.floor(), hi.ceil());
    let scale = ((fit - (hi - lo)).max(0.0) / (before + after)).min(1.0);
    (lo - (before * scale).floor(), hi + (after * scale).floor())
}

/// The deepest the state stack goes, WebKit's canvas limit. A level costs
/// about a hundred bytes plus whatever it clips, so this bounds the stack
/// near 1.6 MiB however deeply an untrusted scene nests. Past it the canvas
/// saturates: a `save()` or `begin_layer()` becomes one entry that its
/// `restore()` or `end_layer()` pairs with, and until the last of those
/// entries pops nothing draws and the changes made to the state are
/// discarded - the conservative reading of every effect a layer past the
/// limit could have declared.
pub(crate) const MAX_STATE_DEPTH: usize = 16 * 1024;

pub(crate) const DEFAULT_FILTER_WORK_BUDGET: u64 = 4 * 1024 * 1024 * 1024;

impl<T> Canvas<T>
where
    T: Renderer,
{
    /// Whether the stack is past [`MAX_STATE_DEPTH`]: nothing draws until
    /// the entries past it are popped.
    pub(crate) fn saturated(&self) -> bool {
        !self.overflow.is_empty()
    }

    /// Records a save or layer past [`MAX_STATE_DEPTH`] and says so; below
    /// the limit it records nothing and the caller pushes a real level. The
    /// first entry snapshots the deepest real state, which setters keep
    /// writing into meanwhile; popping the last entry restores the snapshot.
    pub(crate) fn push_past_depth_limit(&mut self, layer: bool) -> bool {
        if !self.saturated() && self.state_stack.len() < MAX_STATE_DEPTH {
            return false;
        }
        if self.overflow.is_empty() {
            self.overflow_state = *self.state_stack.last().unwrap();
        }
        if !self.warned_overflow {
            self.warned_overflow = true;
            log::warn!("the state stack is {MAX_STATE_DEPTH} deep: nothing draws until it unwinds");
        }
        self.overflow.push(layer);
        true
    }

    /// Pops the innermost entry past the limit, reporting whether it was a
    /// layer; the last one restores the state saved when saturation began.
    pub(crate) fn pop_past_depth_limit(&mut self) -> Option<bool> {
        let layer = self.overflow.pop()?;
        self.restore_after_saturation();
        Some(layer)
    }

    /// Pops through the innermost layer entry past the limit; false, popping
    /// nothing, when none of the entries is a layer.
    pub(crate) fn pop_layer_past_depth_limit(&mut self) -> bool {
        let Some(boundary) = self.overflow.iter().rposition(|&layer| layer) else {
            return false;
        };
        self.overflow.truncate(boundary);
        self.restore_after_saturation();
        true
    }

    pub(crate) fn restore_after_saturation(&mut self) {
        if self.overflow.is_empty() {
            *self.state_stack.last_mut().unwrap() = self.overflow_state;
        }
    }

    pub(crate) fn defer_image_deletion(&mut self, id: ImageId) {
        // A layer's store or scratch is the canvas's own.
        if self.transients.owns(id) || self.images.info(id).is_none() || !self.pending_image_deletions.insert(id) {
            return;
        }
        if self.current_render_target == RenderTarget::Image(id) {
            self.set_render_target(RenderTarget::Screen);
        }
        for layer in &mut self.layers {
            if layer.previous_target == RenderTarget::Image(id) {
                layer.previous_target = RenderTarget::Screen;
            }
        }
        self.forget_clip_target(RenderTarget::Image(id));
    }

    /// Acquires a transient offscreen image from the pool; see `transient.rs`.
    pub(crate) fn acquire_transient_image(
        &mut self,
        width: usize,
        height: usize,
        flags: ImageFlags,
    ) -> Result<ImageId, ErrorKind> {
        self.transients
            .acquire(&mut self.images, &mut self.renderer, width, height, flags)
    }

    pub(crate) fn acquire_transient_image_reserving(
        &mut self,
        width: usize,
        height: usize,
        flags: ImageFlags,
        headroom: usize,
    ) -> Result<ImageId, ErrorKind> {
        self.transients
            .acquire_reserving(&mut self.images, &mut self.renderer, width, height, flags, headroom)
    }

    /// Returns a transient image to the pool once every command that reads it
    /// has been recorded.
    pub(crate) fn release_transient_image(&mut self, id: ImageId) {
        // The pool may hand the image to the next layer; clip state from a
        // discarded layer's store must not follow it there.
        self.forget_clip_target(RenderTarget::Image(id));
        self.transients.release(&self.images, id);
    }

    /// Cancels a reservation before it records commands. Fresh images are
    /// freed immediately; reused images remain alive for earlier commands.
    pub(crate) fn rollback_transient_image(&mut self, id: ImageId) {
        self.forget_clip_target(RenderTarget::Image(id));
        self.transients.rollback(&mut self.images, &mut self.renderer, id);
    }

    /// Returns the backend-estimated bytes charged to the transient budget,
    /// including conservative attachment and blur-scratch reservations. This
    /// is an admission estimate, not a renderer-wide live-allocation counter.
    pub fn transient_image_bytes(&self) -> usize {
        self.transients.bytes()
    }

    /// Caps the estimated cost admitted for transient layer, filter, mask, and
    /// shadow images. The default is 128 MiB. Work that does not fit degrades
    /// as documented by the operation. Smaller targets should set a
    /// platform-appropriate value.
    pub fn set_transient_image_budget(&mut self, bytes: usize) {
        self.transients.set_budget(bytes);
    }

    /// Caps texture-sampling work recorded between flushes by filter chains,
    /// turbulence and shadow blurs. The default is roughly 4 billion samples.
    /// Work that does not fit degrades as documented by the operation.
    pub fn set_filter_work_budget(&mut self, samples: u64) {
        self.filter_work_budget = samples;
    }

    pub(crate) fn reserve_filter_work(&mut self, work: u64) -> bool {
        if self.filter_work.saturating_add(work) > self.filter_work_budget {
            return false;
        }
        self.filter_work = self.filter_work.saturating_add(work);
        true
    }

    pub(crate) fn refund_filter_work(&mut self, work: u64) {
        self.filter_work = self.filter_work.saturating_sub(work);
    }

    /// Deletes the frame's transient images, except those of layers still
    /// open across the flush: a layer's draws so far already live in its
    /// capture and the ones still to come must land in the same image, and
    /// its effects draw at `end_layer` through the images reserved for them
    /// with it.
    pub(crate) fn release_transient_images(&mut self) {
        let held: Vec<ImageId> = self.layers.iter().flat_map(LayerRecord::images).collect();
        self.transients.release_all(&mut self.images, &mut self.renderer, &held);
    }

    pub(crate) fn release_pending_images(&mut self) {
        // What an open layer still reads at its composite: its mask and
        // its blends' backdrops.
        let held: HashSet<ImageId> = self
            .layers
            .iter()
            .flat_map(|layer| {
                let backdrops = layer.effects.filters.iter().filter_map(|filter| match filter {
                    ImageFilter::Blend { backdrop, .. } => Some(*backdrop),
                    _ => None,
                });
                layer.effects.mask.map(|mask| mask.image).into_iter().chain(backdrops)
            })
            .collect();
        let releasable: Vec<ImageId> = self
            .pending_image_deletions
            .iter()
            .filter(|id| !held.contains(id))
            .copied()
            .collect();
        for id in releasable {
            self.pending_image_deletions.remove(&id);
            self.images.remove(&mut self.renderer, id);
        }
    }
}

#[test]
fn a_deleted_image_cannot_be_reselected_after_flush() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let image = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(image));
    canvas.delete_image(image);
    canvas.flush_to_output(());

    canvas.set_render_target(RenderTarget::Image(image));
    assert_eq!(canvas.current_render_target, RenderTarget::Screen);
    assert!(canvas.image_info(image).is_err());
}

/// A blend's placement is its own draw: recorded at the depth limit and
/// past it, where drawing on the target is suppressed, as the pass is.
#[test]
fn a_blends_placement_is_recorded_at_and_past_the_depth_limit() {
    use renderer::CommandType;
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let image = |canvas: &mut Canvas<RecordingRenderer>| {
        canvas
            .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
            .unwrap()
    };
    let (source, target, backdrop) = (image(&mut canvas), image(&mut canvas), image(&mut canvas));
    let blend = ImageFilter::Blend {
        mode: crate::BlendMode::Multiply,
        backdrop,
        x: 0.0,
        y: 0.0,
        width: 16.0,
        height: 16.0,
    };
    for depth in [MAX_STATE_DEPTH, MAX_STATE_DEPTH + 10] {
        while canvas.state_stack.len() + canvas.overflow.len() < depth {
            canvas.save();
        }
        assert_eq!(canvas.saturated(), depth > MAX_STATE_DEPTH);
        canvas.filter_image_chain(target, &[blend], source).unwrap();
        canvas.flush_to_output(());
        let commands = recorded.borrow();
        let count = |pick: &dyn Fn(&renderer::Command) -> bool| commands.iter().filter(|c| pick(c)).count();
        assert_eq!(
            count(&|c| matches!(c.cmd_type, CommandType::RenderFilteredImage { .. })),
            1
        );
        assert_eq!(
            count(&|c| matches!(c.cmd_type, CommandType::ClearRect { .. })),
            1,
            "cleared at depth {depth}"
        );
        assert_eq!(count(&|c| c.image == Some(backdrop)), 1, "placed at depth {depth}");
    }
}

/// A masked layer opened at the depth limit still applies its mask: the
/// mask's draws are the layer's own, not drawing on the target.
#[test]
fn a_mask_applies_at_the_depth_limit() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    while canvas.state_stack.len() < MAX_STATE_DEPTH - 1 {
        canvas.save();
    }
    let effects = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&effects));
    assert_eq!(canvas.state_stack.len(), MAX_STATE_DEPTH);
    assert!(!canvas.saturated());
    let mut rect = Path::new();
    rect.rect(0.0, 0.0, 8.0, 8.0);
    canvas.fill_path(&rect, &Paint::color(Color::black()));
    canvas.end_layer();
    canvas.flush_to_output(());
    let commands = recorded.borrow();
    assert!(commands.iter().any(|c| c.image == Some(mask)), "the mask normalised");
    let destination_in = CompositeOperationState::new(CompositeOperation::DestinationIn);
    assert!(
        commands.iter().any(|c| c.composite_operation == destination_in),
        "the coverage applied"
    );
}

/// A layer whose true blur reach would pad its store past the backend's
/// texture limit still captures: the pad shrinks to what the limit leaves,
/// so the blur loses reach at the store edge instead of the layer passing
/// through with every effect dropped.
#[test]
fn a_layer_at_the_texture_limit_keeps_its_blur_with_a_bounded_pad() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer {
        max_texture_size: 2048,
        ..RecordingRenderer::default()
    };
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(1920, 1080, 1.0);
    // Sigma 40 wants 122 px of pad: 2164 px wide, past the limit.
    let blur = LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 40.0 }]);
    assert!(
        canvas.begin_layer(&blur),
        "the layer captures instead of passing through"
    );
    let record = canvas.layers.last().unwrap();
    assert!(record.image.is_some());
    assert!(
        record.width <= 2048 && record.height <= 2048,
        "store {}x{}",
        record.width,
        record.height
    );
    // Pad 32 (what a 1920 px span leaves under 2048 with a 64 px rounding
    // margin): 1984 wide, within the limit.
    assert_eq!(record.width, 1984, "the store uses what the limit leaves");
    canvas.end_layer();

    // The same blur on a small scissor keeps its full reach.
    canvas.scissor(100.0, 100.0, 200.0, 200.0);
    assert!(canvas.begin_layer(&blur));
    let record = canvas.layers.last().unwrap();
    assert_eq!(record.origin, (100.0 - 122.0, 100.0 - 122.0));
    canvas.end_layer();
}

/// The reach rule alone: the full reach when it fits, a proportional share
/// of the room when it does not, nothing when there is none.
#[test]
fn a_reach_takes_only_the_room_the_limit_leaves() {
    assert_eq!(reach_within(0.0, 1000.0, 90.0, 90.0, 2048), (-90.0, 1090.0));
    assert_eq!(reach_within(0.0, 1920.0, 90.0, 90.0, 2048), (-64.0, 1984.0));
    assert_eq!(reach_within(0.0, 2048.0, 1.0, 0.0, 2048), (0.0, 2048.0));
    assert_eq!(reach_within(0.0, 1920.0, 120.0, 40.0, 2048), (-96.0, 1952.0));
    assert_eq!(reach_within(0.0, 1920.0, 0.0, 0.0, 2048), (0.0, 1920.0));
}

/// The pad rule alone: the full pad when it fits, what the rounded store can
/// afford when it does not, never negative.
#[test]
fn a_bounded_pad_never_pushes_a_store_past_the_limit() {
    assert_eq!(bounded_pad(122.0, 1920.0, 2048, 64), 32.0);
    assert_eq!(bounded_pad(122.0, 200.0, 2048, 64), 122.0);
    assert_eq!(bounded_pad(122.0, 2048.0, 2048, 64), 0.0);
    assert_eq!(bounded_pad(0.0, 1920.0, 2048, 64), 0.0);
}

/// Past MAX_STATE_DEPTH the canvas saturates: saves become one bit each,
/// every restore still pairs with its save, nothing changes the real state
/// and, once unwound, the base is exactly what it was.
#[cfg(test)]
#[test]
fn saves_past_the_depth_limit_stay_paired_and_bounded() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let total = MAX_STATE_DEPTH + 1000;
    for _ in 0..total {
        canvas.save();
        canvas.translate(1.0, 0.0);
    }
    assert_eq!(canvas.state_stack.len(), MAX_STATE_DEPTH);
    assert_eq!(canvas.overflow.len(), total - (MAX_STATE_DEPTH - 1));
    assert!(canvas.saturated());
    assert_eq!(
        canvas.overflow_state.transform.0[4],
        (MAX_STATE_DEPTH - 1) as f32,
        "the state saved when saturation began"
    );

    for _ in 0..1001 {
        canvas.restore();
    }
    assert!(!canvas.saturated());
    assert_eq!(canvas.state_stack.len(), MAX_STATE_DEPTH);
    assert_eq!(
        canvas.state().transform.0[4],
        (MAX_STATE_DEPTH - 1) as f32,
        "translates past the limit were discarded"
    );
    for _ in 0..(MAX_STATE_DEPTH - 1) {
        canvas.restore();
    }
    assert_eq!(canvas.state_stack.len(), 1);
    assert_eq!(canvas.state().transform, Transform2D::identity());
    canvas.restore();
    assert_eq!(canvas.state_stack.len(), 1, "unmatched restore at the base is ignored");
}

/// Nothing draws past the limit - the safe reading of any effect a layer
/// there could declare - and a layer past it says so with `true`, keeps its
/// boundary for end_layer(), and leaves the real layer below open.
#[cfg(test)]
#[test]
fn a_layer_past_the_depth_limit_is_suppressed_and_keeps_its_boundary() {
    use renderer::CommandType;
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mut rect = Path::new();
    rect.rect(0.0, 0.0, 10.0, 10.0);
    let fills = |recorded: &Rc<RefCell<Vec<renderer::Command>>>| {
        recorded
            .borrow()
            .iter()
            .filter(|c| matches!(c.cmd_type, CommandType::ConvexFill { .. }))
            .count()
    };

    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    while canvas.state_stack.len() < MAX_STATE_DEPTH {
        canvas.save();
    }
    let store = canvas.current_render_target;
    assert!(
        canvas.begin_layer(&LayerEffects::new().with_opacity(0.0)),
        "suppressed, hence true"
    );
    assert_eq!(canvas.layers.len(), 1);
    assert_eq!(canvas.overflow.iter().filter(|&&layer| layer).count(), 1);
    canvas.save();
    canvas.fill_path(&rect, &Paint::color(Color::black()));
    canvas.flush_to_output(());
    assert_eq!(fills(&recorded), 0, "nothing draws past the limit");
    canvas.end_layer();
    assert!(
        !canvas.saturated(),
        "end_layer closed the layer with the save left open inside it"
    );
    assert_eq!(canvas.current_render_target, store);
    assert_eq!(canvas.layers.len(), 1, "the real layer is still open");
    canvas.fill_path(&rect, &Paint::color(Color::black()));
    canvas.flush_to_output(());
    assert_eq!(fills(&recorded), 1, "drawing resumes below the limit");
    while canvas.state_stack.len() > 2 {
        canvas.restore();
    }
    canvas.end_layer();
    assert!(canvas.layers.is_empty());
    assert_eq!(canvas.current_render_target, RenderTarget::Screen);
    assert_eq!(canvas.state_stack.len(), 1);
}

/// end_layer() past the limit with no layer to close pops nothing, so a
/// later restore() still pairs with its own save and the clip that save
/// level owns stays armed until that level is popped.
#[cfg(test)]
#[test]
fn end_layer_past_the_depth_limit_without_a_layer_pops_nothing() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    canvas.save();
    let mut clip = Path::new();
    clip.rect(0.0, 0.0, 32.0, 64.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    while canvas.state_stack.len() < MAX_STATE_DEPTH {
        canvas.save();
    }
    canvas.save(); // past the limit
    canvas.end_layer(); // no layer anywhere: nothing to close
    assert_eq!(canvas.overflow.len(), 1);
    canvas.restore(); // pairs with the save past the limit
    assert!(!canvas.saturated());
    assert_eq!(canvas.clip_stack.len(), 1, "the clip is untouched");
    while canvas.state_stack.len() > 2 {
        canvas.restore();
    }
    assert_eq!(canvas.clip_stack.len(), 1, "still armed at the level that took it");
    canvas.restore();
    assert!(canvas.clip_stack.is_empty());
}
