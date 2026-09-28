//! Image filters and their passes: chains over an image, the pass planner,
//! blur quadrature and the work each pass costs.

use super::*;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FilterScratchImages {
    pub(crate) chain: [Option<ImageId>; 2],
    pub(crate) blur: Option<ImageId>,
    // A blend's backdrop, placed into an image of the chain's size just
    // before its pass; one serves every blend in the chain in turn.
    pub(crate) blend: Option<ImageId>,
}

impl FilterScratchImages {
    pub(crate) fn images(self) -> impl Iterator<Item = ImageId> {
        self.chain.into_iter().flatten().chain(self.blur).chain(self.blend)
    }
}

/// The largest standard deviation a blur chain, a layer filter or a shadow
/// renders; above it the sigma is clamped. A cost guard: the split below
/// runs `(sigma / 8)^2` passes of two full-size draws each, so this is 256
/// passes - sigma 128 device pixels is a CSS `blur(40px)` at a 3x device
/// pixel ratio, and reaches 386 px - and an absurd or non-finite sigma
/// cannot plan an unbounded pass count. Browsers stop at Skia's `kMaxSigma`
/// of 532 (SkBlurImageFilter.cpp, a 1000 px box kernel), which they reach by
/// downscaling or running-sum box blurs, not by pass count; that path is
/// femtovg/femtovg#325's.
pub(crate) const MAX_CHAIN_BLUR_SIGMA: f32 = 128.0;

/// The standard deviation a chain renders for a requested `sigma`: `None`
/// for a degenerate one (zero, negative, NaN), which the coefficient
/// sanitization renders as a copy, else the value clamped to
/// [`MAX_CHAIN_BLUR_SIGMA`]. The one place the pass split and the store
/// padding read a blur's sigma, so the passes a chain runs and the reach a
/// layer or shadow pads for cannot disagree.
pub(crate) fn chain_blur_sigma(sigma: f32) -> Option<f32> {
    (sigma > 0.0).then(|| sigma.min(MAX_CHAIN_BLUR_SIGMA))
}

/// How a Gaussian blur of `sigma` runs within the shader's per-pass bound
/// ([`renderer::MAX_BLUR_SIGMA`]): `(passes, sigma per pass)`. Gaussians
/// compose in quadrature - k passes of sigma s blur like one pass of
/// s * sqrt(k) - so a sigma above the bound B is exactly k = ceil((sigma / B)^2)
/// passes of sigma / sqrt(k), each at most B: sigma 16 is four passes of 8,
/// sigma 23 nine of 23/3. A sigma within the bound, or a degenerate one, is
/// one pass with the value untouched, so small blurs render exactly as they
/// did before the split existed. The cost is quadratic in sigma (each pass is
/// two full-size draws), which is what the ceiling above bounds.
pub(crate) fn blur_passes(sigma: f32) -> (usize, f32) {
    let bound = renderer::MAX_BLUR_SIGMA;
    match chain_blur_sigma(sigma) {
        Some(sigma) if sigma > bound => {
            let ratio = sigma / bound;
            let passes = (ratio * ratio).ceil() as usize;
            (passes, sigma / (passes as f32).sqrt())
        }
        _ => (1, sigma),
    }
}

pub(crate) const MAX_FILTER_PASSES: usize = 257;

/// What a blend charges per pixel: the pass reads the image and its
/// backdrop, and placing the backdrop clears the scratch and draws into it.
pub(crate) const BLEND_SAMPLES: u64 = 4;

/// The work a layer's blend mode charges: one blend pass over the store,
/// the copy of the backdrop included.
pub(crate) fn blend_work(width: usize, height: usize) -> u64 {
    (width as u64)
        .saturating_mul(height as u64)
        .saturating_mul(BLEND_SAMPLES)
}

pub(crate) fn filter_work(filters: &[ImageFilter], width: usize, height: usize) -> u64 {
    let samples = filters.iter().fold(0u64, |total, filter| {
        let per_pixel = match filter {
            ImageFilter::GaussianBlur { sigma } => {
                let sigma = if *sigma > 0.0 {
                    sigma.min(renderer::MAX_BLUR_SIGMA)
                } else {
                    1e-3
                };
                let radius = (3.0 * sigma).ceil() as u64;
                2 * (1 + 2 * radius.saturating_sub(1))
            }
            ImageFilter::Turbulence { num_octaves, .. } => (8 * u64::from((*num_octaves).min(10))).max(1),
            ImageFilter::Blend { .. } => BLEND_SAMPLES,
            _ => 1,
        };
        total.saturating_add(per_pixel)
    });
    (width as u64).saturating_mul(height as u64).saturating_mul(samples)
}

/// The passes a filter list runs as: runs of adjacent color matrices folded
/// where that is exact ([`ImageFilter::fold_with`]), each Gaussian blur
/// above the shader's per-pass bound split into the passes that compose to
/// it ([`blur_passes`]), plus an identity pass when the flip count comes out
/// even, so every chain shape leaves storage flipped once - which makes the
/// empty list a copy. The result is never empty; `None` rejects a plan above
/// [`MAX_FILTER_PASSES`]. What [`Canvas::filter_image_chain`] executes and
/// what a layer's scratch reservation is sized from, so the two cannot disagree.
pub(crate) fn filter_passes(filters: &[ImageFilter]) -> Option<Vec<ImageFilter>> {
    if filters.len() > MAX_FILTER_PASSES {
        return None;
    }
    // Fold first, split second: a blur never folds today, but the split
    // passes are adjacent blurs, and expanding after the fold keeps them
    // from being folded back should a fold of blurs ever exist. ImageFilter
    // is Copy, so neither list deep-copies anything.
    let mut folded: Vec<ImageFilter> = Vec::with_capacity(filters.len().min(MAX_FILTER_PASSES));
    for filter in filters {
        if let Some(prev) = folded.last_mut() {
            if let Some(merged) = prev.fold_with(*filter) {
                *prev = merged;
                continue;
            }
        }
        folded.push(*filter);
    }
    // The capacity covers every split pass plus the parity pass, so the list
    // never reallocates.
    let count = folded.iter().try_fold(0usize, |count, filter| {
        let passes = match filter {
            ImageFilter::GaussianBlur { sigma } => blur_passes(*sigma).0,
            _ => 1,
        };
        count.checked_add(passes).filter(|count| *count <= MAX_FILTER_PASSES)
    })?;
    let mut passes: Vec<ImageFilter> = Vec::with_capacity(count + 1);
    for filter in folded {
        match filter {
            ImageFilter::GaussianBlur { sigma } => {
                let (count, sigma) = blur_passes(sigma);
                passes.extend(std::iter::repeat_n(ImageFilter::GaussianBlur { sigma }, count));
            }
            other => passes.push(other),
        }
    }
    // A color-matrix pass flips the image (the render-target convention),
    // the two-pass Gaussian blur preserves it - however many of them a
    // split adds, the parity is the unsplit chain's.
    let flips = passes.iter().filter(|f| f.flips_output()).count();
    if flips % 2 == 0 {
        if passes.len() == MAX_FILTER_PASSES {
            return None;
        }
        passes.push(ImageFilter::identity());
    }
    Some(passes)
}

impl<T> Canvas<T>
where
    T: Renderer,
{
    /// Renders the given `source_image` into `target_image` while applying a filter effect.
    ///
    /// The target image must have the same size as the source image. The filtering is recorded
    /// as a drawing command and run by the renderer when [`Self::flush()`] is called.
    ///
    /// The filtering does not take any transformation set on the Canvas into account nor does it
    /// change the current rendering target.
    ///
    /// This is one shader pass, and a Gaussian blur pass renders a standard
    /// deviation of at most 8 device pixels (the shader's kernel is bounded
    /// at 24 taps per side, a GLES 2.0 loop constraint): a larger `sigma` is
    /// clamped to 8 here. For a blur above that use
    /// [`filter_image_chain`](Self::filter_image_chain), which splits it
    /// into passes that compose to the requested sigma.
    ///
    /// [`ImageFilter::Turbulence`] reads nothing from `source_image` - it only takes the output
    /// size from it - and keeps a small per-seed cache of lattice textures (512 KB each, the
    /// last four seeds used) alive across flushes.
    /// Unsafe in-place sampling filters, over-budget work and a blur that
    /// cannot reserve its transient scratch leave the target unchanged.
    pub fn filter_image(&mut self, target_image: ImageId, filter: ImageFilter, source_image: ImageId) {
        if let ImageFilter::Blend { .. } = filter {
            // A blend places its backdrop in a scratch first: the chain owns
            // that, and a one-blend chain is the one pass.
            let _ = self.filter_image_chain(target_image, std::slice::from_ref(&filter), source_image);
            return;
        }
        let Ok((image_width, image_height)) = self.image_size(source_image) else {
            return;
        };
        if self.image_info(target_image).is_err() {
            return;
        }
        if target_image == source_image
            && !matches!(
                filter,
                ImageFilter::GaussianBlur { .. } | ImageFilter::Turbulence { .. }
            )
        {
            return;
        }
        let work = filter_work(std::slice::from_ref(&filter), image_width, image_height);
        if !self.reserve_filter_work(work) {
            return;
        }
        let blur_scratch = if matches!(filter, ImageFilter::GaussianBlur { .. }) {
            match self.acquire_transient_image(image_width, image_height, ImageFlags::PREMULTIPLIED) {
                Ok(image) => Some(image),
                Err(_) => {
                    self.refund_filter_work(work);
                    return;
                }
            }
        } else {
            None
        };
        let recorded = self.filter_image_with_scratch(target_image, filter, source_image, blur_scratch, None);
        if let Some(image) = blur_scratch {
            self.release_transient_image(image);
        }
        if !recorded {
            self.refund_filter_work(work);
        }
    }

    pub(crate) fn filter_image_with_scratch(
        &mut self,
        target_image: ImageId,
        filter: ImageFilter,
        source_image: ImageId,
        blur_scratch: Option<ImageId>,
        // A blend's placed backdrop and the pass's inputs beyond its mode.
        backdrop: Option<(ImageId, BlendPass)>,
    ) -> bool {
        debug_assert_eq!(
            matches!(filter, ImageFilter::GaussianBlur { .. }),
            blur_scratch.is_some()
        );
        debug_assert_eq!(matches!(filter, ImageFilter::Blend { .. }), backdrop.is_some());
        debug_assert!(
            target_image != source_image
                || matches!(
                    filter,
                    ImageFilter::GaussianBlur { .. } | ImageFilter::Turbulence { .. }
                )
        );
        if let Some(scratch) = blur_scratch {
            debug_assert!(scratch != source_image && scratch != target_image);
        }
        let Ok((image_width, image_height)) = self.image_size(source_image) else {
            return false;
        };

        // The renderer will receive a RenderFilteredImage command with two triangles attached that
        // cover the image and the source image. A turbulence pass generates rather than samples,
        // so it binds its noise lattice where the source would go; the source still sizes the quad.
        let sampled = match filter {
            ImageFilter::Turbulence { seed, .. } => match self.turbulence_lattice(seed) {
                Ok(lattice) => lattice,
                Err(_) => return false,
            },
            _ => source_image,
        };
        let mut cmd = Command::new(CommandType::RenderFilteredImage { target_image, filter });
        cmd.image = Some(sampled);
        cmd.filter_scratch = blur_scratch;
        if let Some((placed, pass)) = backdrop {
            cmd.glyph_texture = GlyphTexture::ColorTexture(placed);
            cmd.blend_pass = pass;
        }

        let vertex_offset = self.verts.len();

        let image_width = image_width as f32;
        let image_height = image_height as f32;

        let quad_x0 = 0.0;
        let quad_y0 = -image_height;
        let quad_x1 = image_width;
        let quad_y1 = image_height;

        let texture_x0 = -(image_width / 2.);
        let texture_y0 = -(image_height / 2.);
        let texture_x1 = (image_width) / 2.;
        let texture_y1 = (image_height) / 2.;

        self.verts.push(Vertex::new(quad_x0, quad_y0, texture_x0, texture_y0));
        self.verts.push(Vertex::new(quad_x1, quad_y1, texture_x1, texture_y1));
        self.verts.push(Vertex::new(quad_x1, quad_y0, texture_x1, texture_y0));
        self.verts.push(Vertex::new(quad_x0, quad_y0, texture_x0, texture_y0));
        self.verts.push(Vertex::new(quad_x0, quad_y1, texture_x0, texture_y1));
        self.verts.push(Vertex::new(quad_x1, quad_y1, texture_x1, texture_y1));

        cmd.triangles_verts = Some((vertex_offset, 6));

        self.append_cmd(cmd);
        if let Some(plane) = self.clip_planes.get_mut(&RenderTarget::Image(target_image)) {
            plane.dirty = true;
        }
        true
    }

    /// The lattice texture for a turbulence seed, built on first use and kept
    /// for the [`turbulence::LATTICE_CACHE_CAPACITY`] most recently used seeds.
    /// An evicted lattice is deleted at the next flush, once any command
    /// already recorded against it has run.
    pub(crate) fn turbulence_lattice(&mut self, seed: i32) -> Result<ImageId, ErrorKind> {
        if let Some(at) = self.turbulence_lattices.iter().position(|(s, _)| *s == seed) {
            let entry = self.turbulence_lattices.remove(at);
            self.turbulence_lattices.push(entry);
            return Ok(entry.1);
        }
        let texels = turbulence::lattice_texels(seed);
        let id = self.create_image(turbulence::lattice_source(&texels), turbulence::lattice_flags())?;
        self.turbulence_lattices.push((seed, id));
        if self.turbulence_lattices.len() > turbulence::LATTICE_CACHE_CAPACITY {
            let (_, evicted) = self.turbulence_lattices.remove(0);
            self.defer_image_deletion(evicted);
        }
        Ok(id)
    }

    pub(crate) fn prepare_turbulence_lattices(&mut self, filters: &[ImageFilter]) -> Result<(), ErrorKind> {
        for filter in filters {
            if let ImageFilter::Turbulence { seed, .. } = filter {
                self.turbulence_lattice(*seed)?;
            }
        }
        Ok(())
    }

    /// Applies a list of image filters as one chain, `filters[0]` first —
    /// the execution model behind a Canvas `ctx.filter` list
    /// (`"blur(5px) brightness(1.2)"`) and SVG filter chains.
    ///
    /// Runs of adjacent color-matrix filters fold into a single matrix on the
    /// CPU (see [`ImageFilter::fold_with`]), so a run of color operations costs
    /// one GPU pass - as long as each matrix but the last stays within [0, 1];
    /// one that can overflow (`brightness(>1)`, `contrast`, `sepia`) keeps its
    /// own pass so its clamp still happens, matching how browsers clamp per
    /// filter function. A Gaussian blur whose standard deviation is above the
    /// 8 device pixels one shader pass covers runs as `ceil((sigma / 8)^2)`
    /// passes of `sigma / sqrt(passes)`: Gaussians compose in quadrature, so
    /// four passes of sigma 8 are exactly one blur of sigma 16, and nine of
    /// 23/3 one of 23 - the full reach, where the single-pass
    /// [`filter_image`](Self::filter_image) would clamp to 8. The pass count
    /// grows with the square of the sigma, so sigma is capped at 128 and one
    /// operation is capped at 257 total planned passes. Passes that do not fold ping-pong between at most two
    /// transient scratch images sized like the source; a blur plan reserves
    /// one more full-size horizontal scratch, so peak transient
    /// memory is twice the source image, or three times across a blur - bounded
    /// regardless of chain length or pass count either way. The scratches are
    /// freed at the next flush. All color work is in unpremultiplied sRGB with
    /// output clamped to [0, 1] per pass, so an alpha-amplifying matrix feeding
    /// a blur cannot blow out later passes.
    ///
    /// The target ends up in the same orientation convention as a single
    /// color-matrix [`filter_image`](Self::filter_image) call: content stored
    /// vertically flipped, sampled upright via [`ImageFlags::FLIP_Y`], and
    /// carrying premultiplied alpha - create chain targets with
    /// `ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y` so semi-transparent
    /// results composite once, not twice. An empty list degrades to a plain
    /// copy under that same convention. A chain whose flip parity comes out
    /// even (for example a lone blur) pays one extra identity pass for that
    /// uniformity; blur-only callers who want the single-pass form can call
    /// `filter_image` directly.
    ///
    /// Returns [`ErrorKind::ImageIdNotFound`] when either image is missing,
    /// [`ErrorKind::RenderTargetError`] when a single sampling pass would read
    /// and write the same image, [`ErrorKind::FilterPassLimitExceeded`] when
    /// the chain exceeds the per-operation pass cap,
    /// [`ErrorKind::FilterWorkBudgetExceeded`] when it exceeds the command
    /// stream's work budget, and [`ErrorKind::TransientImageBudgetExceeded`]
    /// when its scratches do not fit; either way no pass runs and
    /// `target_image` is left as it was. A layer's chain never
    /// stops here - its scratches are reserved at
    /// [`begin_layer`](Self::begin_layer) with the layer's store.
    ///
    /// The chain borrows `source_image` and `target_image` without taking
    /// ownership - both may be caller-managed or acquired transients (a layer
    /// capture pass, say, can feed its layer in as `source_image` and keep
    /// releasing it on its own schedule). Only the internal scratches are
    /// tied to the flush lifecycle, so composing this under group effects
    /// adds no copies and no extra retained images.
    pub fn filter_image_chain(
        &mut self,
        target_image: ImageId,
        filters: &[ImageFilter],
        source_image: ImageId,
    ) -> Result<(), ErrorKind> {
        let passes = filter_passes(filters).ok_or(ErrorKind::FilterPassLimitExceeded)?;
        let (width, height) = self.image_size(source_image)?;
        self.image_info(target_image)?;
        if target_image == source_image && filters.is_empty() {
            return Ok(());
        }
        if target_image == source_image && passes.len() == 1 && !matches!(passes[0], ImageFilter::Turbulence { .. }) {
            return Err(ErrorKind::RenderTargetError(
                "a single-pass filter cannot read and write the same image".into(),
            ));
        }
        for filter in &passes {
            if let ImageFilter::Blend { backdrop, .. } = filter {
                self.image_info(*backdrop)?;
            }
        }
        let work = filter_work(&passes, width, height);
        if !self.reserve_filter_work(work) {
            return Err(ErrorKind::FilterWorkBudgetExceeded);
        }
        if let Err(err) = self.prepare_turbulence_lattices(&passes) {
            self.refund_filter_work(work);
            return Err(err);
        }
        let scratch = self
            .acquire_filter_scratches(
                width,
                height,
                passes.len(),
                passes
                    .iter()
                    .any(|filter| matches!(filter, ImageFilter::GaussianBlur { .. })),
                passes.iter().any(|filter| matches!(filter, ImageFilter::Blend { .. })),
                2,
                0,
            )
            .inspect_err(|_| self.refund_filter_work(work))?;
        self.run_filter_passes(target_image, &passes, source_image, scratch, false, (0.0, 0.0));
        Ok(())
    }

    /// Acquires the scratches a chain of `passes` ping-pongs between: none
    /// for a single pass, which writes its target directly, one for two, two
    /// beyond, whatever the chain's length. Scratches hold premultiplied
    /// filter output; the flag keeps every consumer (filter passes and
    /// composites) reading them under the same alpha convention - without it,
    /// semi-transparent content is premultiplied a second time at each read
    /// and darkens per pass. Holds nothing on failure.
    pub(crate) fn acquire_filter_scratches(
        &mut self,
        width: usize,
        height: usize,
        passes: usize,
        needs_blur: bool,
        needs_blend: bool,
        chain_limit: usize,
        headroom: usize,
    ) -> Result<FilterScratchImages, ErrorKind> {
        let mut scratch = FilterScratchImages::default();
        for i in 0..passes.saturating_sub(1).min(chain_limit) {
            match self.acquire_transient_image_reserving(width, height, ImageFlags::PREMULTIPLIED, headroom) {
                Ok(id) => scratch.chain[i] = Some(id),
                Err(err) => {
                    for id in scratch.images() {
                        self.rollback_transient_image(id);
                    }
                    return Err(err);
                }
            }
        }
        if needs_blur {
            match self.acquire_transient_image_reserving(width, height, ImageFlags::PREMULTIPLIED, headroom) {
                Ok(id) => scratch.blur = Some(id),
                Err(err) => {
                    for id in scratch.images() {
                        self.rollback_transient_image(id);
                    }
                    return Err(err);
                }
            }
        }
        if needs_blend {
            match self.acquire_transient_image_reserving(width, height, ImageFlags::PREMULTIPLIED, headroom) {
                Ok(id) => scratch.blend = Some(id),
                Err(err) => {
                    for id in scratch.images() {
                        self.rollback_transient_image(id);
                    }
                    return Err(err);
                }
            }
        }
        Ok(scratch)
    }

    /// Runs `passes` from `source_image` to `target_image` through the
    /// scratches. `origin` is where the source's (0, 0) sits in the space a
    /// blend's backdrop rect is given in: the layer's root origin for a
    /// layer's chain, (0, 0) for a chain over the caller's own image.
    pub(crate) fn run_filter_passes(
        &mut self,
        target_image: ImageId,
        passes: &[ImageFilter],
        source_image: ImageId,
        scratch: FilterScratchImages,
        target_as_scratch: bool,
        origin: (f32, f32),
    ) {
        debug_assert!(!target_as_scratch || target_image != source_image);
        let mut src = source_image;
        // Storage orientation of `src` at each pass: a render target (FLIP_Y)
        // holds its rows the other way up from an upload, and every pass but
        // a blur turns the result over once.
        let mut src_flipped = self
            .images
            .info(source_image)
            .is_some_and(|info| info.flags().contains(ImageFlags::FLIP_Y));
        let last = passes.len() - 1;
        for (i, filter) in passes.iter().enumerate() {
            let dst = if i == last || (target_as_scratch && (last - i).is_multiple_of(2)) {
                target_image
            } else {
                let index = if target_as_scratch { 0 } else { i % 2 };
                scratch.chain[index].expect("a scratch was acquired for every pass but the last")
            };
            let blur_scratch = matches!(filter, ImageFilter::GaussianBlur { .. })
                .then(|| scratch.blur.expect("a blur scratch was reserved"));
            let backdrop = match filter {
                ImageFilter::Blend {
                    backdrop,
                    x,
                    y,
                    width,
                    height,
                    ..
                } => {
                    let placed = scratch.blend.expect("a blend scratch was reserved");
                    self.place_blend_backdrop(placed, *backdrop, (x - origin.0, y - origin.1, *width, *height));
                    // The placement is a draw into an image: stored the way
                    // a render target is, so upside down against an upright
                    // source at this pass.
                    let pass = BlendPass {
                        backdrop_flipped: !src_flipped,
                        ..BlendPass::default()
                    };
                    Some((placed, pass))
                }
                _ => None,
            };
            let _ = self.filter_image_with_scratch(dst, *filter, src, blur_scratch, backdrop);
            if filter.flips_output() {
                src_flipped = !src_flipped;
            }
            src = dst;
        }
        for id in scratch.images() {
            self.release_transient_image(id);
        }
    }
}

/// Turbulence lattices are cached per seed with a fixed capacity: a repeated
/// seed reuses its texture, the least recently used seed is queued for
/// deletion (so a command recorded against it still runs before the delete),
/// and a flush deletes only the evicted ones.
#[test]
fn turbulence_lattice_cache_is_bounded() {
    use crate::{ImageFilter, TurbulenceKind};
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let src = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let dst = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::FLIP_Y)
        .unwrap();
    let noise = |seed: i32| ImageFilter::Turbulence {
        base_frequency: [0.1, 0.1],
        num_octaves: 2,
        seed,
        stitch_tiles: false,
        kind: TurbulenceKind::FractalNoise,
        transform: Transform2D::identity(),
    };

    for seed in 1..=turbulence::LATTICE_CACHE_CAPACITY as i32 {
        canvas.filter_image(dst, noise(seed), src);
    }
    assert_eq!(canvas.turbulence_lattices.len(), turbulence::LATTICE_CACHE_CAPACITY);
    assert!(canvas.pending_image_deletions.is_empty());

    // A cache hit reorders, allocating nothing.
    canvas.filter_image(dst, noise(1), src);
    assert_eq!(canvas.turbulence_lattices.len(), turbulence::LATTICE_CACHE_CAPACITY);
    assert_eq!(canvas.turbulence_lattices.last().unwrap().0, 1);
    assert!(canvas.pending_image_deletions.is_empty());

    // One past capacity evicts the least recently used seed (2, since 1 was
    // just touched) after the next flush, not straight from the image store.
    let evicted = canvas.turbulence_lattices[0].1;
    canvas.filter_image(dst, noise(99), src);
    assert_eq!(canvas.turbulence_lattices.len(), turbulence::LATTICE_CACHE_CAPACITY);
    assert!(canvas.turbulence_lattices.iter().all(|(s, _)| *s != 2));
    assert!(canvas.pending_image_deletions.contains(&evicted));
    assert!(
        canvas.images.info(evicted).is_some(),
        "evicted lattice stays alive until flush"
    );

    canvas.flush_to_output(());
    assert!(canvas.pending_image_deletions.is_empty());
    assert!(canvas.images.info(evicted).is_none());
    assert_eq!(canvas.turbulence_lattices.len(), turbulence::LATTICE_CACHE_CAPACITY);

    for seed in 100..109 {
        canvas.filter_image(dst, noise(seed), src);
    }
    let lattice_ids: HashSet<_> = canvas
        .commands
        .iter()
        .filter(|command| matches!(command.cmd_type, CommandType::RenderFilteredImage { .. }))
        .filter_map(|command| command.image)
        .collect();
    assert_eq!(lattice_ids.len(), 9);
}

/// Successive Gaussians compound in quadrature: three sigma-8 blurs reach
/// like one of sigma 13.9, so the store pads by 44 px, not the 26 of the
/// largest alone.
#[test]
fn chained_blurs_pad_in_quadrature() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(800, 600, 1.0);
    canvas.save();
    canvas.scissor(100.0, 50.0, 200.0, 200.0);
    let blur = ImageFilter::GaussianBlur { sigma: 8.0 };
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[blur, blur, blur])));
    let record = canvas.layers.last().unwrap();
    // 200 + 2 * (ceil(3 * sqrt(3 * 64)) + 2) = 288, rounded to 320.
    assert_eq!((record.width, record.height), (320, 320));
    canvas.end_layer();
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[blur])));
    let record = canvas.layers.last().unwrap();
    // 200 + 2 * 26 = 252, rounded to 256.
    assert_eq!((record.width, record.height), (256, 256));
    canvas.end_layer();
    canvas.restore();
}

/// A blur within the shader's per-pass bound is one pass with its sigma
/// untouched, plus the parity identity - the plan it always had, so small
/// blurs render exactly as before the split existed. A degenerate sigma is
/// one pass too, for the coefficient sanitization to copy through.
#[test]
fn a_blur_within_the_shader_bound_stays_one_pass() {
    use crate::ImageFilter;
    for sigma in [0.5, 3.0, 8.0] {
        let passes = filter_passes(&[ImageFilter::GaussianBlur { sigma }]).unwrap();
        assert_eq!(passes.len(), 2, "sigma {sigma}: one blur pass and the parity identity");
        assert!(
            matches!(passes[0], ImageFilter::GaussianBlur { sigma: s } if s == sigma),
            "{:?}",
            passes[0]
        );
        assert!(matches!(passes[1], ImageFilter::ColorMatrix { matrix } if matrix == ImageFilter::IDENTITY_MATRIX));
    }
    assert_eq!(blur_passes(8.0), (1, 8.0));
    assert_eq!(blur_passes(0.0), (1, 0.0));
    assert_eq!(blur_passes(-1.0).0, 1);
    assert_eq!(blur_passes(f32::NAN).0, 1);
}

/// Gaussians compose in quadrature, so a blur above the bound B = 8 is
/// exactly k = ceil((sigma / B)^2) passes of sigma / sqrt(k): 16 is four of
/// 8, 23 is nine of 23/3, every pass within the bound and their squares
/// summing back to the requested sigma's. The ceiling keeps the plan finite
/// for an absurd sigma.
#[test]
fn a_blur_above_the_bound_splits_into_quadrature_passes() {
    use crate::ImageFilter;
    let blur_sigmas = |filters: &[ImageFilter]| -> Vec<f32> {
        filter_passes(filters)
            .unwrap()
            .iter()
            .filter_map(|f| match f {
                ImageFilter::GaussianBlur { sigma } => Some(*sigma),
                _ => None,
            })
            .collect()
    };
    assert_eq!(blur_sigmas(&[ImageFilter::GaussianBlur { sigma: 16.0 }]), vec![8.0; 4]);
    let nine = blur_sigmas(&[ImageFilter::GaussianBlur { sigma: 23.0 }]);
    assert_eq!(nine.len(), 9);
    for sigma in &nine {
        assert!((sigma - 23.0 / 3.0).abs() < 1e-5, "{sigma}");
        assert!(*sigma <= renderer::MAX_BLUR_SIGMA);
    }
    let composed: f32 = nine.iter().map(|s| s * s).sum::<f32>().sqrt();
    assert!((composed - 23.0).abs() < 1e-4, "{composed}");
    // Just past the bound: two passes, neither above it.
    let (passes, sigma) = blur_passes(8.5);
    assert_eq!(passes, 2);
    assert!((sigma - 8.5 / 2f32.sqrt()).abs() < 1e-5);
    // The ceiling bounds the plan: an infinite sigma is 128's 256 passes,
    // not 2^56 of them.
    assert_eq!(blur_passes(f32::INFINITY), blur_passes(128.0));
    assert_eq!(blur_passes(128.0), (256, 8.0));
    assert_eq!(blur_passes(1e9), blur_passes(128.0));
}

/// The split changes the pass count, not the chain's shape: a blur pass
/// preserves the flip, so [blur 16] ends with the parity identity like
/// [blur 8] does and [blur 16, brightness] does not, like [blur 8,
/// brightness]; and every chain ping-pongs through the same scratch pair -
/// min(2, passes - 1) of them, however many passes a split adds.
#[test]
fn a_split_blur_keeps_the_chain_parity_and_scratch_count() {
    use crate::ImageFilter;
    let ends_with_identity = |filters: &[ImageFilter]| {
        matches!(
            filter_passes(filters).unwrap().last(),
            Some(ImageFilter::ColorMatrix { matrix }) if *matrix == ImageFilter::IDENTITY_MATRIX
        )
    };
    let small = ImageFilter::GaussianBlur { sigma: 8.0 };
    let big = ImageFilter::GaussianBlur { sigma: 16.0 };
    let bright = ImageFilter::brightness(1.2);
    assert!(ends_with_identity(&[small]) && ends_with_identity(&[big]));
    assert!(!ends_with_identity(&[small, bright]) && !ends_with_identity(&[big, bright]));
    assert_eq!(filter_passes(&[big, bright]).unwrap().len(), 5);

    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    for filters in [&[big][..], &[big, bright], &[big, bright, big]] {
        let scratch = canvas
            .acquire_filter_scratches(64, 64, filter_passes(filters).unwrap().len(), true, false, 2, 0)
            .unwrap();
        assert_eq!(
            scratch.chain.iter().flatten().count(),
            2,
            "{filters:?}: one scratch pair"
        );
        assert!(scratch.blur.is_some());
        for id in scratch.images() {
            canvas.release_transient_image(id);
        }
    }
    assert_eq!(
        canvas.transients.images.len(),
        3,
        "the scratches are reused across plans"
    );
}

#[test]
fn filter_work_matches_shader_sampling_and_resets_at_flush() {
    let blur = ImageFilter::GaussianBlur { sigma: 8.0 };
    assert_eq!(filter_work(&[blur], 10, 10), 9_400);
    let turbulence = |num_octaves| ImageFilter::Turbulence {
        base_frequency: [0.1, 0.1],
        num_octaves,
        seed: 1,
        stitch_tiles: false,
        kind: TurbulenceKind::Turbulence,
        transform: Transform2D::identity(),
    };
    assert_eq!(filter_work(&[turbulence(0)], 10, 10), 100);
    assert_eq!(filter_work(&[turbulence(10)], 10, 10), 8_000);

    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(16, 16, 1.0);
    let source = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let target = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let blend = ImageFilter::Blend {
        mode: crate::BlendMode::Multiply,
        backdrop: source,
        x: 0.0,
        y: 0.0,
        width: 16.0,
        height: 16.0,
    };
    assert_eq!(filter_work(&[blend], 10, 10), 400, "two reads, a clear and a draw");
    // Admission at the edge: the placement is part of what is charged.
    canvas.set_filter_work_budget(4 * 16 * 16);
    canvas.filter_image_chain(target, &[blend], source).unwrap();
    canvas.flush_to_output(());
    canvas.set_filter_work_budget(4 * 16 * 16 - 1);
    assert!(matches!(
        canvas.filter_image_chain(target, &[blend], source),
        Err(ErrorKind::FilterWorkBudgetExceeded)
    ));
    canvas.flush_to_output(());
    canvas.set_filter_work_budget(512);
    let color = ImageFilter::brightness(0.5);
    canvas.filter_image_chain(target, &[color], source).unwrap();
    canvas.filter_image_chain(target, &[color], source).unwrap();
    assert!(matches!(
        canvas.filter_image_chain(target, &[color], source),
        Err(ErrorKind::FilterWorkBudgetExceeded)
    ));
    assert_eq!(canvas.filter_work, 512);
    canvas.flush_to_output(());
    assert_eq!(canvas.filter_work, 0);
    canvas.filter_image_chain(target, &[color], source).unwrap();
}
