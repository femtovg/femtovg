//! Image filters and their passes: chains over an image, the pass planner,
//! the blur pyramid, the morphology split and the work each pass costs.

use super::*;

/// A pyramid level: how many times a pass's image is halved along x and
/// along y from the chain's own size.
pub(crate) type Level = [u8; 2];

/// The level of a pass at the chain's own size.
pub(crate) const FULL: Level = [0, 0];

#[derive(Clone, Debug, Default)]
pub(crate) struct FilterScratchImages {
    pub(crate) chain: [Option<ImageId>; 2],
    /// The horizontal scratch of a full-size two-pass filter, a blur or a
    /// morphology.
    pub(crate) two_pass: Option<ImageId>,
    // A blend's backdrop, placed into an image of the chain's size just
    // before its pass; one serves every blend in the chain in turn.
    pub(crate) blend: Option<ImageId>,
    /// The pyramid levels the plan's passes render at, each with its
    /// images, in the order the plan reaches them.
    pub(crate) levels: Vec<(Level, LevelImages)>,
}

/// The images of one pyramid level: the one the chain is halved into, and,
/// where a blur runs at this level, its target and horizontal scratch.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LevelImages {
    pub(crate) down: Option<ImageId>,
    pub(crate) blurred: Option<ImageId>,
    pub(crate) blur: Option<ImageId>,
}

impl FilterScratchImages {
    pub(crate) fn images(&self) -> impl Iterator<Item = ImageId> + '_ {
        self.chain
            .iter()
            .flatten()
            .copied()
            .chain(self.two_pass)
            .chain(self.blend)
            .chain(
                self.levels
                    .iter()
                    .flat_map(|(_, level)| [level.down, level.blurred, level.blur])
                    .flatten(),
            )
    }

    /// The images reserved at pyramid `level`.
    fn level(&self, level: Level) -> LevelImages {
        self.levels
            .iter()
            .find(|(at, _)| *at == level)
            .map(|(_, images)| *images)
            .expect("images were reserved at every level the plan uses")
    }
}

/// What a filter pass's command carries besides its filter and images.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PassDraw {
    /// A blend's placed backdrop and the pass's inputs beyond its mode.
    pub(crate) backdrop: Option<(ImageId, BlendPass)>,
    /// The matrices a two-draw filter carries in its draws.
    pub(crate) fused: Fused,
    /// The rect the result is drawn inside, in the target's texel rows.
    pub(crate) crop: Option<[u32; 4]>,
}

/// One pass of a filter plan: `filter`, rendered into an image at pyramid
/// `level` - [`FULL`] the chain's own size, `[m, n]` the chain halved m
/// times along x and n times along y.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pass {
    pub(crate) filter: ImageFilter,
    pub(crate) level: Level,
    /// The matrices a two-draw filter carries in its draws.
    pub(crate) fused: Fused,
    /// The rect the pass's result is clipped to ([`ImageFilter::Crop`]), as
    /// given: x, y, width, height.
    pub(crate) crop: Option<[f32; 4]>,
}

impl Pass {
    pub(crate) fn at(filter: ImageFilter, level: Level) -> Self {
        Self {
            filter,
            level,
            fused: Fused::default(),
            crop: None,
        }
    }

    /// The size of the image this pass renders into, for a chain of
    /// `width` x `height`.
    pub(crate) fn size(&self, width: usize, height: usize) -> (usize, usize) {
        level_size(width, height, self.level)
    }
}

/// Pyramid `level` of a `width` x `height` image: halved `level[0]` times
/// along x and `level[1]` times along y, rounded up, never empty.
pub(crate) fn level_size(width: usize, height: usize, level: Level) -> (usize, usize) {
    let halve = |n: usize, times: u8| n.div_ceil(1 << times).max(1);
    (halve(width, level[0]), halve(height, level[1]))
}

/// How many times an axis is halved at most: the ceiling below, halved this
/// often, is the shader's per-pass bound.
pub(crate) const MAX_LEVELS: usize = 6;

/// The largest standard deviation a blur chain, a layer filter or a shadow
/// renders on an axis; above it the sigma is clamped, so an absurd or
/// non-finite sigma plans a bounded pyramid. Skia stops at 532 (`kMaxSigma`,
/// a 1000 px box kernel); the reach, three sigma, is what bounds a store at
/// this value.
pub(crate) const MAX_CHAIN_BLUR_SIGMA: f32 = 512.0;

/// The standard deviation a chain renders for a requested `sigma`: `None`
/// for a degenerate one (zero, negative, NaN), which the coefficient
/// sanitization renders as a copy, else the value clamped to
/// [`MAX_CHAIN_BLUR_SIGMA`]. The one place the pyramid and the store
/// padding read a blur's sigma, so the passes a chain runs and the reach a
/// layer or shadow pads for cannot disagree.
pub(crate) fn chain_blur_sigma(sigma: f32) -> Option<f32> {
    (sigma > 0.0).then(|| sigma.min(MAX_CHAIN_BLUR_SIGMA))
}

/// How a Gaussian blur of `sigma_x` by `sigma_y` runs within the shader's
/// per-pass bound B ([`renderer::MAX_BLUR_SIGMA`]). Within it on both axes,
/// one pass at the chain's size with the values untouched. Above it on an
/// axis, the way Skia's GPU blur does: identity passes halve the chain
/// along that axis n = ceil(log2(sigma / B)) times (a bilinear tap at an
/// exact half scale averages the two texels it sits between), the blur runs at that
/// level with sigma / 2^n - in (B/2, B] - on the halved axis and the
/// requested value on an axis left whole, and an identity pass scales the
/// result back up, bilinearly. A blur of sigma 77 is then four halvings, a
/// blur over a sixteenth of the pixels and one full-size copy, where
/// quadrature passes of B were 93 full-size blurs; the box prefilter of the
/// halving widens the blur by under a tenth of a percent. A degenerate
/// sigma is the copy it always was on its axis.
pub(crate) fn blur_passes(sigma_x: f32, sigma_y: f32) -> Vec<Pass> {
    let bound = renderer::MAX_BLUR_SIGMA;
    let depth = |sigma: f32| -> u8 {
        chain_blur_sigma(sigma)
            .filter(|sigma| *sigma > bound)
            .map_or(0, |sigma| (sigma / bound).log2().ceil() as u8)
    };
    let depths = [depth(sigma_x), depth(sigma_y)];
    let deepest = depths[0].max(depths[1]);
    if deepest == 0 {
        return vec![Pass::at(ImageFilter::GaussianBlur { sigma_x, sigma_y }, FULL)];
    }
    debug_assert!(usize::from(deepest) <= MAX_LEVELS);
    // Halved with its axis; an axis left whole keeps its value.
    let at_depth = |sigma: f32, depth: u8| {
        if depth == 0 {
            sigma
        } else {
            sigma.min(MAX_CHAIN_BLUR_SIGMA) / f32::from(1u16 << depth)
        }
    };
    let mut passes = Vec::with_capacity(usize::from(deepest) + 2);
    passes.extend((1..=deepest).map(|halvings| {
        Pass::at(
            ImageFilter::identity(),
            [halvings.min(depths[0]), halvings.min(depths[1])],
        )
    }));
    passes.push(Pass::at(
        ImageFilter::GaussianBlur {
            sigma_x: at_depth(sigma_x, depths[0]),
            sigma_y: at_depth(sigma_y, depths[1]),
        },
        depths,
    ));
    passes.push(Pass::at(ImageFilter::identity(), FULL));
    passes
}

/// The largest morphology radius one shader draw covers on an axis: the
/// loop's constant bound, as for the blur.
pub(crate) const MAX_MORPHOLOGY_RADIUS: f32 = 24.0;

/// A morphology radius as the shader runs it: whole device pixels, rounded
/// as Skia rounds them (Gecko takes the ceiling); zero for a degenerate
/// value (negative, non-finite), which leaves the axis alone. The one place
/// the passes, the work and a layer's reach read a radius.
pub(crate) fn morphology_radius(radius: f32) -> f32 {
    if radius.is_finite() && radius > 0.0 {
        radius.round()
    } else {
        0.0
    }
}

/// An offset's shift as the shader runs it: the value, or zero for a
/// non-finite one.
pub(crate) fn offset_pixels(shift: f32) -> f32 {
    if shift.is_finite() {
        shift
    } else {
        0.0
    }
}

/// How a morphology of `radius_x` by `radius_y` runs within the shader's
/// per-draw bound ([`MAX_MORPHOLOGY_RADIUS`]): the radii rounded to whole
/// pixels, each axis in as many passes of at most the bound as its radius
/// needs - dilations and erosions by a rectangle compose by adding their
/// radii, so the split is exact - the shorter axis done first and copying
/// through (radius 0) in the remaining passes. Both radii zero is the one
/// copy pass the shader runs with no taps.
pub(crate) fn morphology_passes(radius_x: f32, radius_y: f32, operator: MorphologyOperator) -> Vec<Pass> {
    let bound = MAX_MORPHOLOGY_RADIUS;
    let radii = [morphology_radius(radius_x), morphology_radius(radius_y)];
    let count = radii
        .map(|radius| (radius / bound).ceil() as usize)
        .into_iter()
        .max()
        .unwrap_or(0)
        .max(1);
    (0..count)
        .map(|pass| {
            let left = |radius: f32| (radius - bound * pass as f32).clamp(0.0, bound);
            Pass::at(
                ImageFilter::Morphology {
                    radius_x: left(radii[0]),
                    radius_y: left(radii[1]),
                    operator,
                },
                FULL,
            )
        })
        .collect()
}

/// The taps one morphology draw takes per pixel along an axis of `radius`
/// (whole pixels, within the bound): the center and `radius` either side.
fn morphology_taps(radius: f32) -> u64 {
    1 + 2 * (morphology_radius(radius).min(MAX_MORPHOLOGY_RADIUS) as u64)
}

/// The taps one blur draw takes per pixel along an axis of `sigma`: the
/// shader's kernel reaches 3 sigma either side of the center tap, and a
/// degenerate axis is the single center tap of a copy.
fn axis_taps(sigma: f32) -> u64 {
    let sigma = if sigma > 0.0 {
        sigma.min(renderer::MAX_BLUR_SIGMA)
    } else {
        1e-3
    };
    let radius = (3.0 * sigma).ceil() as u64;
    1 + 2 * radius.saturating_sub(1)
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

/// The samples a plan over a `width` x `height` chain takes: each pass's
/// per-pixel cost over the pixels of its level.
pub(crate) fn filter_work(passes: &[Pass], width: usize, height: usize) -> u64 {
    passes.iter().fold(0u64, |total, pass| {
        let per_pixel = match pass.filter {
            ImageFilter::GaussianBlur { sigma_x, sigma_y } => axis_taps(sigma_x) + axis_taps(sigma_y),
            ImageFilter::Morphology { radius_x, radius_y, .. } => morphology_taps(radius_x) + morphology_taps(radius_y),
            ImageFilter::Turbulence { num_octaves, .. } => (8 * u64::from(num_octaves.min(10))).max(1),
            ImageFilter::Blend { .. } => BLEND_SAMPLES,
            _ => 1,
        };
        let (w, h) = pass.size(width, height);
        total.saturating_add((w as u64).saturating_mul(h as u64).saturating_mul(per_pixel))
    })
}

/// The passes a filter list runs as: runs of adjacent color matrices folded
/// where that is exact ([`ImageFilter::fold_with`]), a zero offset being the
/// identity matrix, each Gaussian blur above the shader's per-pass bound
/// run down the pyramid and back ([`blur_passes`]), each morphology as the
/// passes its radii need ([`morphology_passes`]), and a matrix beside a
/// pyramid's halving or scale back up folded into that pass, which is a
/// matrix too. The plan says nothing about orientation:
/// [`plan_stores_flipped`] follows the image through the passes, and the
/// caller reads its target the way they leave it. An empty list plans no pass;
/// `None` rejects a plan above [`MAX_FILTER_PASSES`]. What
/// [`Canvas::filter_image_chain`] executes and what a layer's scratch
/// reservation is sized from, so the two cannot disagree.
pub(crate) fn filter_passes(filters: &[ImageFilter]) -> Option<Vec<Pass>> {
    if filters.len() > MAX_FILTER_PASSES {
        return None;
    }
    // Fold first, expand second: the pyramid's identity passes sit at other
    // levels than their neighbours, and expanding after the fold keeps them
    // out of it. ImageFilter is Copy, so neither list deep-copies anything.
    let mut folded: Vec<ImageFilter> = Vec::with_capacity(filters.len().min(MAX_FILTER_PASSES));
    for filter in filters {
        let filter = match *filter {
            ImageFilter::Offset { dx, dy } => {
                let (dx, dy) = (offset_pixels(dx), offset_pixels(dy));
                if dx == 0.0 && dy == 0.0 {
                    ImageFilter::identity()
                } else {
                    ImageFilter::Offset { dx, dy }
                }
            }
            other => other,
        };
        if let Some(prev) = folded.last_mut() {
            if let Some(merged) = prev.fold_with(filter) {
                *prev = merged;
                continue;
            }
        }
        folded.push(filter);
    }
    // A crop rides the pass before it: that pass draws inside the rect only.
    // With no pass before it, it is a copy of its own.
    let crop_onto = |passes: &mut Vec<Pass>, rect: [f32; 4]| match passes.last_mut() {
        Some(last) if last.level == FULL => last.crop = Some(last.crop.map_or(rect, |first| crop_both(first, rect))),
        _ => passes.push(Pass {
            crop: Some(rect),
            ..Pass::at(ImageFilter::identity(), FULL)
        }),
    };
    let mut passes: Vec<Pass> = Vec::with_capacity(folded.len() + MAX_LEVELS + 2);
    for filter in folded {
        match filter {
            ImageFilter::GaussianBlur { sigma_x, sigma_y } => passes.extend(blur_passes(sigma_x, sigma_y)),
            ImageFilter::Morphology {
                radius_x,
                radius_y,
                operator,
            } => passes.extend(morphology_passes(radius_x, radius_y, operator)),
            ImageFilter::Crop { x, y, width, height } => crop_onto(&mut passes, [x, y, width, height]),
            other => passes.push(Pass::at(other, FULL)),
        }
        if passes.len() > MAX_FILTER_PASSES {
            return None;
        }
    }
    // Fold again across the pyramid's identity passes. A matrix pass and a
    // resampling pass merge into one matrix pass reading what the first
    // read and writing where the second wrote, which keeps the resampling
    // exact only when one of the two was at a single size: a matrix before
    // the first halving rides that halving, a matrix after the scale back
    // up rides that; two halvings stay two.
    let mut folded_passes: Vec<Pass> = Vec::with_capacity(passes.len());
    let mut previous_in = FULL;
    for pass in passes {
        if let Some(prev) = folded_passes.last_mut() {
            let one_size = previous_in == prev.level || prev.level == pass.level;
            if one_size {
                let merged = prev.filter.fold_with(pass.filter);
                if let (Some(merged), Some(crop)) = (merged, folded_crop(prev.crop, &pass)) {
                    *prev = Pass {
                        crop,
                        ..Pass::at(merged, pass.level)
                    };
                    continue;
                }
            }
            previous_in = prev.level;
        }
        folded_passes.push(pass);
    }
    // Fuse the matrices beside a two-draw filter into its draws: an
    // alpha-only matrix just before it, read at the filter's own size, is
    // what every tap of its first draw reads; any matrix just after it, at
    // its size, is applied by its second draw before storing - one draw
    // fewer each, the same arithmetic.
    let mut fused: Vec<Pass> = Vec::with_capacity(folded_passes.len());
    let mut previous_in = FULL;
    for pass in folded_passes {
        if let Some(prev) = fused.last_mut() {
            if let ImageFilter::ColorMatrix { matrix } = pass.filter {
                if prev.filter.two_pass() && prev.level == pass.level && prev.fused.post_matrix.is_none() {
                    if let Some(crop) = folded_crop(prev.crop, &pass) {
                        prev.fused.post_matrix = Some(matrix);
                        prev.crop = crop;
                        continue;
                    }
                }
            }
            if let ImageFilter::ColorMatrix { matrix } = prev.filter {
                if pass.filter.two_pass()
                    && prev.level == pass.level
                    && previous_in == prev.level
                    && alpha_only(&matrix)
                    && !pass.fused.source_alpha
                    && prev.crop.is_none()
                {
                    *prev = Pass {
                        fused: Fused {
                            source_alpha: true,
                            ..pass.fused
                        },
                        ..pass
                    };
                    continue;
                }
            }
            previous_in = prev.level;
        }
        fused.push(pass);
    }
    Some(fused)
}

/// What two crops, one after the other, leave: the rect both hold.
fn crop_both([ax, ay, aw, ah]: [f32; 4], [bx, by, bw, bh]: [f32; 4]) -> [f32; 4] {
    let (x, y) = (ax.max(bx), ay.max(by));
    let (right, bottom) = ((ax + aw).min(bx + bw), (ay + ah).min(by + bh));
    [x, y, (right - x).max(0.0), (bottom - y).max(0.0)]
}

/// The crop of the one pass that a pass cropped to `first` and `then`, a
/// pass after it that maps each pixel by itself, make together - or `None`
/// when no one crop leaves what the two do. Where `then`'s own crop lies
/// inside `first` the first crop took nothing that is left. Anywhere else
/// it left transparent black, which `then` must leave as it is for the
/// crop to come after it instead: a color matrix does unless it adds to
/// alpha, a transfer always. A crop is a rect of full-size pixels, so the
/// pass that takes it over must draw at full size: a pyramid's first
/// halving does not.
fn folded_crop(first: Option<[f32; 4]>, then: &Pass) -> Option<Option<[f32; 4]>> {
    let Some(first) = first else {
        return Some(then.crop);
    };
    if then.level != FULL {
        return None;
    }
    let [ax, ay, aw, ah] = first;
    let inside = then
        .crop
        .is_some_and(|[bx, by, bw, bh]| ax <= bx && ay <= by && bx + bw <= ax + aw && by + bh <= ay + ah);
    let keeps_clear = match then.filter {
        ImageFilter::ColorMatrix { matrix } => matrix[19] <= 0.0,
        ImageFilter::LinearRgbToSrgb | ImageFilter::SrgbToLinearRgb => true,
        _ => false,
    };
    if inside {
        Some(then.crop)
    } else if keeps_clear {
        Some(Some(then.crop.map_or(first, |then| crop_both(first, then))))
    } else {
        None
    }
}

/// Whether a matrix keeps alpha and nothing else - `SourceAlpha` as a
/// color matrix - which a two-draw filter's first draw can read directly.
fn alpha_only(matrix: &[f32; 20]) -> bool {
    matrix[..15].iter().all(|v| *v == 0.0) && matrix[15..] == [0.0, 0.0, 0.0, 1.0, 0.0]
}

/// Whether a plan leaves its result stored the way a render target is, rows
/// bottom up, given whether its source is: each pass in turn by
/// [`ImageFilter::stores_flipped`].
pub(crate) fn plan_stores_flipped(passes: &[Pass], source_flipped: bool) -> bool {
    passes
        .iter()
        .fold(source_flipped, |flipped, pass| pass.filter.stores_flipped(flipped))
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
    /// deviation of at most 8 device pixels per axis (the shader's kernel is
    /// bounded at 24 taps per side, a GLES 2.0 loop constraint): a larger
    /// sigma is clamped to 8 here, as a morphology radius is clamped to 24.
    /// For a blur above that use
    /// [`filter_image_chain`](Self::filter_image_chain), which renders it at
    /// a downsampled size where it fits one pass, and for a wider morphology
    /// too, which it runs as the passes that sum to it.
    ///
    /// [`ImageFilter::Turbulence`] reads nothing from `source_image` - it only takes the output
    /// size from it - and keeps a small per-seed cache of lattice textures (512 KB each, the
    /// last four seeds used) alive across flushes.
    /// Unsafe in-place sampling filters, over-budget work and a blur that
    /// cannot reserve its transient scratch leave the target unchanged.
    pub fn filter_image(&mut self, target_image: ImageId, filter: ImageFilter, source_image: ImageId) {
        if matches!(filter, ImageFilter::Blend { .. } | ImageFilter::Crop { .. }) {
            // A blend places its backdrop in a scratch first, and a crop is
            // the rect a pass of the plan draws inside: the chain owns both,
            // and a chain of the one filter is the one pass.
            let _ = self.filter_image_chain(target_image, std::slice::from_ref(&filter), source_image);
            return;
        }
        let Ok((image_width, image_height)) = self.image_size(source_image) else {
            return;
        };
        if self.image_info(target_image).is_err() {
            return;
        }
        if target_image == source_image && !filter.two_pass() && !matches!(filter, ImageFilter::Turbulence { .. }) {
            return;
        }
        let source_flipped = self
            .images
            .info(source_image)
            .is_some_and(|info| info.flags().contains(ImageFlags::FLIP_Y));
        let filter = match filter.oriented(source_flipped) {
            // One draw per axis: the radius the shader can take.
            ImageFilter::Morphology {
                radius_x,
                radius_y,
                operator,
            } => ImageFilter::Morphology {
                radius_x: morphology_radius(radius_x).min(MAX_MORPHOLOGY_RADIUS),
                radius_y: morphology_radius(radius_y).min(MAX_MORPHOLOGY_RADIUS),
                operator,
            },
            ImageFilter::Offset { dx, dy } => ImageFilter::Offset {
                dx: offset_pixels(dx),
                dy: offset_pixels(dy),
            },
            other => other,
        };
        let work = filter_work(&[Pass::at(filter, FULL)], image_width, image_height);
        if !self.reserve_filter_work(work) {
            return;
        }
        let blur_scratch = if filter.two_pass() {
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
        let recorded =
            self.filter_image_with_scratch(target_image, filter, source_image, blur_scratch, PassDraw::default());
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
        draw: PassDraw,
    ) -> bool {
        debug_assert_eq!(filter.two_pass(), blur_scratch.is_some());
        debug_assert_eq!(matches!(filter, ImageFilter::Blend { .. }), draw.backdrop.is_some());
        debug_assert!(
            target_image != source_image || filter.two_pass() || matches!(filter, ImageFilter::Turbulence { .. })
        );
        if let Some(scratch) = blur_scratch {
            debug_assert!(scratch != source_image && scratch != target_image);
        }
        // The quad covers the target: a pass between images of different
        // sizes (a pyramid's halving and its scale back up) resamples the
        // whole source over it, as the renderers set the shader's extent to
        // the target too.
        let Ok((image_width, image_height)) = self.image_size(target_image) else {
            return false;
        };

        // The renderer will receive a RenderFilteredImage command with two triangles attached that
        // cover the target. A turbulence pass generates rather than samples, so it binds its noise
        // lattice where the source would go.
        let sampled = match filter {
            ImageFilter::Turbulence { seed, .. } => match self.turbulence_lattice(seed) {
                Ok(lattice) => lattice,
                Err(_) => return false,
            },
            _ => source_image,
        };
        debug_assert!(filter.two_pass() || (!draw.fused.source_alpha && draw.fused.post_matrix.is_none()));
        let mut cmd = Command::new(CommandType::RenderFilteredImage { target_image, filter });
        cmd.image = Some(sampled);
        cmd.filter_scratch = blur_scratch;
        cmd.fused = draw.fused;
        cmd.crop = draw.crop;
        if let Some((placed, pass)) = draw.backdrop {
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

    pub(crate) fn prepare_turbulence_lattices(&mut self, passes: &[Pass]) -> Result<(), ErrorKind> {
        for pass in passes {
            if let ImageFilter::Turbulence { seed, .. } = pass.filter {
                self.turbulence_lattice(seed)?;
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
    /// filter function. A Gaussian blur whose standard deviation on an axis
    /// is above the 8 device pixels one shader pass covers runs the way
    /// Skia's GPU blur does: the image is halved along that axis until the
    /// sigma, halved with it, fits one pass, blurred there, and scaled back
    /// up bilinearly - the full reach, where the single-pass
    /// [`filter_image`](Self::filter_image) would clamp to 8, for about one
    /// copy of the image plus a small blur, and the halving widens the blur
    /// by under a tenth of a percent. Sigma is capped at 512 and one
    /// operation at 257 planned passes. A morphology whose radius on an axis
    /// is above the 24 pixels one draw covers runs as passes whose radii sum
    /// to it, which is exact for its rectangle; an offset is one pass, and a
    /// zero one folds away. Passes that do not fold ping-pong between at
    /// most two transient scratch images sized like the source; a blur
    /// within the bound or a morphology reserves one more full-size
    /// horizontal scratch, a blur above the bound the pyramid's levels (under
    /// a third of the source in all when both axes are halved) and its
    /// scratches at the blur's level, so peak transient memory is twice the
    /// source image, or three times across a blur - bounded regardless of
    /// chain length or sigma either way. The scratches are freed at the next
    /// flush. All color work is in unpremultiplied sRGB with output clamped
    /// to [0, 1] per pass, so an alpha-amplifying matrix feeding a blur
    /// cannot blow out later passes.
    ///
    /// The result lands in the target the way the target's flags say it is
    /// read: a target created with `ImageFlags::PREMULTIPLIED |
    /// ImageFlags::FLIP_Y`, the convention of a single color-matrix
    /// [`filter_image`](Self::filter_image) call, composites upright, and so
    /// does one without `FLIP_Y`. Every single draw turns the stored image
    /// over, the two-draw filters leave it as it was and noise lands the
    /// way any draw into an image does, so the chain adds one copy pass
    /// when its passes would store the result the other way from the flag -
    /// a lone blur into a `FLIP_Y` target, say - and none otherwise. An
    /// empty list is one copy under the `FLIP_Y`
    /// convention. Create targets premultiplied so semi-transparent results
    /// composite once, not twice.
    ///
    /// A chain may run in place (`target_image` the same as `source_image`):
    /// a single sampling pass turns the image over, so it is followed by the
    /// copy that reads it back the right way up, and nothing samples what it
    /// writes; a two-draw filter goes through its scratch.
    ///
    /// Returns [`ErrorKind::ImageIdNotFound`] when either image is missing,
    /// [`ErrorKind::FilterPassLimitExceeded`] when
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
        let mut passes = filter_passes(filters).ok_or(ErrorKind::FilterPassLimitExceeded)?;
        let (width, height) = self.image_size(source_image)?;
        self.image_info(target_image)?;
        if target_image == source_image && filters.is_empty() {
            return Ok(());
        }
        let stored_flipped = |canvas: &Self, image: ImageId| {
            canvas
                .images
                .info(image)
                .is_some_and(|info| info.flags().contains(ImageFlags::FLIP_Y))
        };
        // How the passes leave the result stored against how the target is
        // read; an empty list is the one copy it always was.
        let other_way =
            plan_stores_flipped(&passes, stored_flipped(self, source_image)) != stored_flipped(self, target_image);
        if passes.is_empty() || other_way {
            if passes.len() == MAX_FILTER_PASSES {
                return Err(ErrorKind::FilterPassLimitExceeded);
            }
            passes.push(Pass::at(ImageFilter::identity(), FULL));
        }
        for pass in &passes {
            if let ImageFilter::Blend { backdrop, .. } = pass.filter {
                self.image_info(backdrop)?;
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
            .acquire_filter_scratches(width, height, &passes, 2, 0)
            .inspect_err(|_| self.refund_filter_work(work))?;
        self.run_filter_passes(target_image, &passes, source_image, scratch, false, (0.0, 0.0));
        Ok(())
    }

    /// Acquires the scratches a plan's `passes` run through: the full-size
    /// images its full-size passes ping-pong between - none for one such
    /// pass, which writes its target directly, one for two, two beyond, up
    /// to `chain_limit` - a horizontal scratch for a full-size two-pass
    /// filter (a blur or a morphology), a blend's backdrop, and for each
    /// pyramid level in use the image the chain is halved into plus, where
    /// a blur runs there, its target and scratch. Scratches hold premultiplied filter output; the flag keeps
    /// every consumer (filter passes and composites) reading them under the
    /// same alpha convention - without it, semi-transparent content is
    /// premultiplied a second time at each read and darkens per pass. Holds
    /// nothing on failure.
    pub(crate) fn acquire_filter_scratches(
        &mut self,
        width: usize,
        height: usize,
        passes: &[Pass],
        chain_limit: usize,
        headroom: usize,
    ) -> Result<FilterScratchImages, ErrorKind> {
        let is_two_pass = |pass: &Pass| pass.filter.two_pass();
        let full = passes.iter().filter(|pass| pass.level == FULL).count();
        let chain = full.saturating_sub(1).min(chain_limit);
        let two_pass = passes.iter().any(|pass| pass.level == FULL && is_two_pass(pass));
        let blend = passes
            .iter()
            .any(|pass| matches!(pass.filter, ImageFilter::Blend { .. }));
        // Each level in use, in plan order, and whether a blur runs there.
        let mut levels: Vec<(Level, bool)> = Vec::new();
        for pass in passes.iter().filter(|pass| pass.level != FULL) {
            match levels.iter_mut().find(|(level, _)| *level == pass.level) {
                Some((_, blurred)) => *blurred |= is_two_pass(pass),
                None => levels.push((pass.level, is_two_pass(pass))),
            }
        }
        let mut wanted: Vec<(usize, usize)> = Vec::with_capacity(chain + 2 + 3 * levels.len());
        wanted.extend(std::iter::repeat_n(
            (width, height),
            chain + usize::from(two_pass) + usize::from(blend),
        ));
        for (level, blurred) in &levels {
            let size = level_size(width, height, *level);
            wanted.extend(std::iter::repeat_n(size, if *blurred { 3 } else { 1 }));
        }
        let mut acquired: Vec<ImageId> = Vec::with_capacity(wanted.len());
        for (w, h) in wanted {
            match self.acquire_transient_image_reserving(w, h, ImageFlags::PREMULTIPLIED, headroom) {
                Ok(id) => acquired.push(id),
                Err(err) => {
                    for id in acquired {
                        self.rollback_transient_image(id);
                    }
                    return Err(err);
                }
            }
        }
        // Handed out in the order they were asked for.
        let mut next = acquired.into_iter();
        let mut scratch = FilterScratchImages::default();
        for slot in scratch.chain.iter_mut().take(chain) {
            *slot = next.next();
        }
        if two_pass {
            scratch.two_pass = next.next();
        }
        if blend {
            scratch.blend = next.next();
        }
        for (level, blurred) in levels {
            let down = next.next();
            let (blurred, blur) = if blurred {
                (next.next(), next.next())
            } else {
                (None, None)
            };
            scratch.levels.push((level, LevelImages { down, blurred, blur }));
        }
        debug_assert!(next.next().is_none());
        Ok(scratch)
    }

    /// Runs `passes` from `source_image` to `target_image` through the
    /// scratches. `origin` is where the source's (0, 0) sits in the space a
    /// blend's backdrop rect is given in: the layer's root origin for a
    /// layer's chain, (0, 0) for a chain over the caller's own image.
    pub(crate) fn run_filter_passes(
        &mut self,
        target_image: ImageId,
        passes: &[Pass],
        source_image: ImageId,
        scratch: FilterScratchImages,
        target_as_scratch: bool,
        origin: (f32, f32),
    ) {
        debug_assert!(!target_as_scratch || target_image != source_image);
        let mut src = source_image;
        // Storage orientation of `src` at each pass: a render target (FLIP_Y)
        // holds its rows the other way up from an upload, and each pass
        // leaves it as `ImageFilter::stores_flipped` says.
        let mut src_flipped = self
            .images
            .info(source_image)
            .is_some_and(|info| info.flags().contains(ImageFlags::FLIP_Y));
        // The full-size passes alternate between the target (or the two
        // scratches) so that the last of them lands in the target; a pyramid
        // level's passes write that level's own images.
        let mut full_left = passes.iter().filter(|pass| pass.level == FULL).count();
        for pass in passes {
            let two_pass = pass.filter.two_pass();
            let (dst, two_pass_scratch) = if pass.level == FULL {
                full_left -= 1;
                let dst = if full_left == 0 {
                    target_image
                } else if target_as_scratch {
                    if full_left.is_multiple_of(2) {
                        target_image
                    } else {
                        scratch.chain[0].expect("a scratch was acquired for every full pass but the last")
                    }
                } else {
                    scratch.chain[(full_left - 1) % 2].expect("a scratch was acquired for every full pass but the last")
                };
                (dst, scratch.two_pass)
            } else {
                let level = scratch.level(pass.level);
                let dst = if two_pass {
                    level.blurred.expect("a blurred image was reserved at the blur's level")
                } else {
                    level.down.expect("an image was reserved at every level in use")
                };
                (dst, level.blur)
            };
            let two_pass_scratch =
                two_pass.then(|| two_pass_scratch.expect("a scratch was reserved for every two-pass filter"));
            // An offset is given for an upright image; the source may be
            // stored the other way up at this point of the chain.
            let filter = &pass.filter.oriented(src_flipped);
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
            // The crop in the rows the pass's result is stored in, rounded
            // out to whole pixels and cut to the image.
            let crop = pass.crop.map(|[x, y, width, height]| {
                let (image_width, image_height) = self.image_size(dst).unwrap_or((0, 0));
                let (image_width, image_height) = (image_width as f32, image_height as f32);
                // `max` and `min` pass over an edge that is no number, so a
                // rect with one holds no pixel.
                let left = (x - origin.0).floor().max(0.0).min(image_width);
                let top = (y - origin.1).floor().max(0.0).min(image_height);
                let right = (x + width - origin.0).ceil().max(left).min(image_width);
                let bottom = (y + height - origin.1).ceil().max(top).min(image_height);
                let first_row = if filter.stores_flipped(src_flipped) {
                    image_height - bottom
                } else {
                    top
                };
                [
                    left as u32,
                    first_row as u32,
                    (right - left) as u32,
                    (bottom - top) as u32,
                ]
            });
            let draw = PassDraw {
                backdrop,
                fused: pass.fused,
                crop,
            };
            let _ = self.filter_image_with_scratch(dst, *filter, src, two_pass_scratch, draw);
            src_flipped = filter.stores_flipped(src_flipped);
            src = dst;
        }
        debug_assert_eq!(src, target_image, "the plan's last pass lands in the target");
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
    let blur = ImageFilter::gaussian_blur(8.0);
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

/// A blur within the shader's per-pass bound is one pass at the chain's
/// size with its sigmas untouched - the pass it always was, so small blurs
/// render exactly as before the pyramid existed. A degenerate sigma is one
/// pass too, for the coefficient sanitization to copy through.
#[test]
fn a_blur_within_the_shader_bound_stays_one_pass() {
    use crate::ImageFilter;
    for sigma in [0.5, 3.0, 8.0] {
        let passes = filter_passes(&[ImageFilter::gaussian_blur(sigma)]).unwrap();
        assert_eq!(passes.len(), 1, "sigma {sigma}: one blur pass");
        assert!(
            matches!(passes[0].filter, ImageFilter::GaussianBlur { sigma_x, sigma_y } if sigma_x == sigma && sigma_y == sigma),
            "{:?}",
            passes[0]
        );
        assert_eq!(passes[0].level, FULL);
    }
    for (sigma_x, sigma_y) in [(8.0, 8.0), (0.0, 0.0), (-1.0, f32::NAN), (3.0, 8.0)] {
        let plan = blur_passes(sigma_x, sigma_y);
        assert_eq!(plan.len(), 1, "sigma {sigma_x} by {sigma_y}");
        assert_eq!(plan[0].level, FULL);
    }
}

/// The level and blur sigmas of each pass of a blur plan: `None` for an
/// identity pass.
#[cfg(test)]
fn plan_shape(sigma_x: f32, sigma_y: f32) -> Vec<(Level, Option<(f32, f32)>)> {
    blur_passes(sigma_x, sigma_y)
        .iter()
        .map(|pass| {
            let sigma = match pass.filter {
                ImageFilter::GaussianBlur { sigma_x, sigma_y } => Some((sigma_x, sigma_y)),
                ImageFilter::ColorMatrix { matrix } => {
                    assert_eq!(matrix, ImageFilter::IDENTITY_MATRIX);
                    None
                }
                other => panic!("{other:?}"),
            };
            (pass.level, sigma)
        })
        .collect()
}

/// A blur above the bound runs down the pyramid and back: the chain is
/// halved until the sigma, halved with it, fits one pass - sigma 16 once (a
/// blur of 8 over a quarter of the pixels), 23 twice (5.75), 512 six times
/// (8) - the blur runs there, and an identity pass scales the result back to
/// the chain's size. The ceiling keeps the plan at six levels for an absurd
/// sigma.
#[test]
fn a_blur_above_the_bound_runs_down_the_pyramid() {
    assert_eq!(
        plan_shape(16.0, 16.0),
        vec![([1, 1], None), ([1, 1], Some((8.0, 8.0))), (FULL, None)]
    );
    assert_eq!(
        plan_shape(23.0, 23.0),
        vec![
            ([1, 1], None),
            ([2, 2], None),
            ([2, 2], Some((5.75, 5.75))),
            (FULL, None)
        ]
    );
    assert_eq!(
        plan_shape(8.5, 8.5),
        vec![([1, 1], None), ([1, 1], Some((4.25, 4.25))), (FULL, None)]
    );
    let deepest = plan_shape(512.0, 512.0);
    assert_eq!(deepest.len(), MAX_LEVELS + 2);
    assert_eq!(deepest[MAX_LEVELS], ([MAX_LEVELS as u8; 2], Some((8.0, 8.0))));
    assert_eq!(plan_shape(f32::INFINITY, f32::INFINITY), plan_shape(512.0, 512.0));
    assert_eq!(plan_shape(1e9, 1e9), plan_shape(512.0, 512.0));
    // Level sizes halve and round up, never to nothing.
    assert_eq!(level_size(1080, 1080, [4, 4]), (68, 68));
    assert_eq!(level_size(5, 3, [6, 6]), (1, 1));
}

/// Each axis is halved by its own depth: a streak of sigma 16 along x
/// alone halves x once and leaves y whole, blurring (8, 0) at that level;
/// 16 by 4 keeps y's sigma 4 at full height; 0 by 23 halves y twice; and
/// 16 by 23 stops halving x after the first level while y goes on, so the
/// blur runs at [1, 2] with (8, 5.75).
#[test]
fn a_blur_halves_each_axis_by_its_own_depth() {
    use crate::ImageFilter;
    assert_eq!(
        plan_shape(16.0, 0.0),
        vec![([1, 0], None), ([1, 0], Some((8.0, 0.0))), (FULL, None)]
    );
    assert_eq!(
        plan_shape(0.0, 16.0),
        vec![([0, 1], None), ([0, 1], Some((0.0, 8.0))), (FULL, None)]
    );
    assert_eq!(
        plan_shape(16.0, 4.0),
        vec![([1, 0], None), ([1, 0], Some((8.0, 4.0))), (FULL, None)]
    );
    assert_eq!(
        plan_shape(0.0, 23.0),
        vec![
            ([0, 1], None),
            ([0, 2], None),
            ([0, 2], Some((0.0, 5.75))),
            (FULL, None)
        ]
    );
    assert_eq!(
        plan_shape(16.0, 23.0),
        vec![
            ([1, 1], None),
            ([1, 2], None),
            ([1, 2], Some((8.0, 5.75))),
            (FULL, None)
        ]
    );
    assert_eq!(plan_shape(3.0, 5.0), vec![(FULL, Some((3.0, 5.0)))]);
    assert_eq!(
        filter_passes(&[ImageFilter::GaussianBlur {
            sigma_x: 16.0,
            sigma_y: 0.0
        }])
        .unwrap()
        .len(),
        3
    );
    assert_eq!(level_size(64, 64, [1, 0]), (32, 64));
}

/// A pyramid blur's scratches are the pyramid's: the level the chain is
/// halved into, with the blur's target and scratch at that size, and no
/// full-size scratch at all - the scale back up writes the target - 3/4 of
/// the chain where the quadrature passes held three full-size scratches; a
/// streak's level is half as wide and as tall as the chain. A matrix after
/// the blur rides the scale back up, so [blur 16, brightness] is three
/// passes like [blur 16].
#[test]
fn a_pyramid_blur_shrinks_its_scratches() {
    use crate::ImageFilter;
    let small = ImageFilter::gaussian_blur(8.0);
    let big = ImageFilter::gaussian_blur(16.0);
    let bright = ImageFilter::brightness(1.2);
    assert_eq!(filter_passes(&[big]).unwrap().len(), 3);
    let with_matrix = filter_passes(&[big, bright]).unwrap();
    assert_eq!(with_matrix.len(), 3);
    assert!(matches!(with_matrix[2].filter, ImageFilter::ColorMatrix { .. }) && with_matrix[2].level == FULL);

    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let held = |canvas: &Canvas<RecordingRenderer>, scratch: &FilterScratchImages| -> usize {
        scratch
            .images()
            .map(|id| {
                let (w, h) = canvas.image_size(id).unwrap();
                w * h
            })
            .sum()
    };
    let passes = filter_passes(&[big]).unwrap();
    let scratch = canvas.acquire_filter_scratches(64, 64, &passes, 2, 0).unwrap();
    assert!(
        scratch.two_pass.is_none(),
        "no blur at the chain's size, so no full-size blur scratch"
    );
    assert_eq!(
        scratch.chain.iter().flatten().count(),
        0,
        "the scale back up is the one full-size pass and writes the target"
    );
    assert_eq!(scratch.levels.len(), 1);
    let level = scratch.level([1, 1]);
    for image in [level.down, level.blurred, level.blur] {
        assert_eq!(canvas.image_size(image.unwrap()).unwrap(), (32, 32));
    }
    assert_eq!(held(&canvas, &scratch), 3 * 32 * 32);
    for id in scratch.images() {
        canvas.release_transient_image(id);
    }

    let streak = ImageFilter::GaussianBlur {
        sigma_x: 16.0,
        sigma_y: 0.0,
    };
    let passes = filter_passes(&[streak]).unwrap();
    let scratch = canvas.acquire_filter_scratches(64, 64, &passes, 2, 0).unwrap();
    let level = scratch.level([1, 0]);
    for image in [level.down, level.blurred, level.blur] {
        assert_eq!(canvas.image_size(image.unwrap()).unwrap(), (32, 64));
    }
    assert_eq!(held(&canvas, &scratch), 3 * 32 * 64);
    for id in scratch.images() {
        canvas.release_transient_image(id);
    }

    let passes = filter_passes(&[small]).unwrap();
    let scratch = canvas.acquire_filter_scratches(64, 64, &passes, 2, 0).unwrap();
    assert!(
        scratch.two_pass.is_some() && scratch.levels.is_empty(),
        "a blur within the bound keeps its full-size scratch and no pyramid"
    );
    for id in scratch.images() {
        canvas.release_transient_image(id);
    }
}

/// A morphology plans in whole pixels and splits at the per-draw bound
/// per axis, exactly: 30 by 5 is a pass of 24 by 5 and one of 6 by 0, and
/// radii that round to nothing are the one copy pass. Like a blur it keeps
/// the image the way up it was; an offset is one flipping pass, and a zero
/// or non-finite one is the identity, folding into a neighbouring matrix.
#[test]
fn a_morphology_splits_in_whole_pixels_and_an_offset_is_one_pass() {
    use crate::ImageFilter;
    let radii = |radius_x: f32, radius_y: f32| -> Vec<(f32, f32)> {
        morphology_passes(radius_x, radius_y, MorphologyOperator::Dilate)
            .iter()
            .map(|pass| match pass.filter {
                ImageFilter::Morphology { radius_x, radius_y, .. } => {
                    assert_eq!(pass.level, FULL);
                    (radius_x, radius_y)
                }
                other => panic!("{other:?}"),
            })
            .collect()
    };
    assert_eq!(radii(2.4, 0.0), vec![(2.0, 0.0)]);
    assert_eq!(radii(2.5, 2.6), vec![(3.0, 3.0)]);
    assert_eq!(radii(30.0, 5.0), vec![(24.0, 5.0), (6.0, 0.0)]);
    assert_eq!(radii(0.0, 50.0), vec![(0.0, 24.0), (0.0, 24.0), (0.0, 2.0)]);
    assert_eq!(radii(0.0, 0.0), vec![(0.0, 0.0)]);
    assert_eq!(radii(f32::NAN, -1.0), vec![(0.0, 0.0)]);
    assert_eq!(radii(24.0, 0.0), vec![(24.0, 0.0)]);
    let dilate = ImageFilter::Morphology {
        radius_x: 2.0,
        radius_y: 2.0,
        operator: MorphologyOperator::Dilate,
    };
    let passes = filter_passes(&[dilate]).unwrap();
    assert_eq!(passes.len(), 1, "the morphology alone");
    assert!(passes[0].filter.two_pass());
    assert!(plan_stores_flipped(&passes, true) && !plan_stores_flipped(&passes, false));

    let shift = ImageFilter::Offset { dx: 3.0, dy: -4.0 };
    assert_eq!(filter_passes(&[shift]).unwrap().len(), 1);
    assert!(shift.stores_flipped(false) && !shift.stores_flipped(true) && !shift.two_pass());
    let still = ImageFilter::Offset { dx: 0.0, dy: f32::NAN };
    let passes = filter_passes(&[ImageFilter::brightness(0.5), still]).unwrap();
    assert_eq!(passes.len(), 1, "a zero offset folds into the matrix before it");
    assert!(matches!(passes[0].filter, ImageFilter::ColorMatrix { .. }));
    assert_eq!(
        filter_passes(&[still]).unwrap().len(),
        1,
        "a zero offset alone is the identity copy"
    );
    assert!(matches!(
        ImageFilter::Offset { dx: 1.0, dy: 2.0 }.oriented(true),
        ImageFilter::Offset { dx: 1.0, dy: -2.0 }
    ));
    assert!(matches!(
        ImageFilter::Offset { dx: 1.0, dy: 2.0 }.oriented(false),
        ImageFilter::Offset { dx: 1.0, dy: 2.0 }
    ));

    // A morphology reserves the two-pass scratch a blur does, and no chain
    // scratch for the one pass it is.
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let passes = filter_passes(&[dilate]).unwrap();
    let scratch = canvas.acquire_filter_scratches(64, 64, &passes, 2, 0).unwrap();
    assert!(scratch.two_pass.is_some() && scratch.chain.iter().flatten().count() == 0);
    for id in scratch.images() {
        canvas.release_transient_image(id);
    }
}

/// A matrix beside a pyramid's resampling pass rides it: an alpha-only
/// matrix before a sigma-16 blur becomes the first halving, a brightness
/// after it the scale back up, so [alpha, blur 77, brightness] is the four
/// halvings, the blur and one scale back up - six passes, not eight - while
/// two halvings never merge into one, which would resample differently.
#[test]
fn a_matrix_rides_the_pyramid_halving_or_scale_back_up() {
    use crate::ImageFilter;
    let alpha = ImageFilter::ColorMatrix {
        matrix: [
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 1.0, 0.0,
        ],
    };
    let bright = ImageFilter::brightness(0.5);
    let kinds = |filters: &[ImageFilter]| -> Vec<(char, Level)> {
        filter_passes(filters)
            .unwrap()
            .iter()
            .map(|pass| {
                let kind = match pass.filter {
                    ImageFilter::ColorMatrix { matrix } if matrix == ImageFilter::IDENTITY_MATRIX => 'i',
                    ImageFilter::ColorMatrix { .. } => 'm',
                    ImageFilter::GaussianBlur { .. } => 'b',
                    _ => '?',
                };
                (kind, pass.level)
            })
            .collect()
    };
    assert_eq!(
        kinds(&[alpha, ImageFilter::gaussian_blur(16.0)]),
        vec![('m', [1, 1]), ('b', [1, 1]), ('i', FULL)]
    );
    assert_eq!(
        kinds(&[ImageFilter::gaussian_blur(16.0), bright]),
        vec![('i', [1, 1]), ('b', [1, 1]), ('m', FULL)]
    );
    assert_eq!(
        kinds(&[alpha, ImageFilter::gaussian_blur(77.0), bright]),
        vec![
            ('m', [1, 1]),
            ('i', [2, 2]),
            ('i', [3, 3]),
            ('i', [4, 4]),
            ('b', [4, 4]),
            ('m', FULL)
        ]
    );
    // Two zero offsets are one copy; a copy before a morphology stays.
    assert_eq!(
        filter_passes(&[
            ImageFilter::Offset { dx: 0.0, dy: 0.0 },
            ImageFilter::Offset { dx: 0.0, dy: 0.0 }
        ])
        .unwrap()
        .len(),
        1
    );
}

/// The matrices beside a two-draw filter ride its draws: `SourceAlpha` as a
/// matrix before a blur or a morphology is read by its first draw, a matrix
/// after it is applied by its second, so a Sketch shadow chain - alpha,
/// dilate, a zero offset, a colouring matrix - is one pass, and alpha, a
/// sigma-2 blur and a brightness are one too. A matrix that is not
/// alpha-only stays its own pass before the filter, a pyramid blur takes
/// its matrices on its resampling passes instead, and two filters in a row
/// fuse nothing between them.
#[test]
fn the_matrices_beside_a_two_draw_filter_ride_its_draws() {
    use crate::ImageFilter;
    let alpha = ImageFilter::ColorMatrix {
        matrix: [
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 1.0, 0.0,
        ],
    };
    let tint = ImageFilter::ColorMatrix {
        matrix: [
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.25, 0.0,
        ],
    };
    let dilate = ImageFilter::Morphology {
        radius_x: 2.0,
        radius_y: 2.0,
        operator: MorphologyOperator::Dilate,
    };
    let sketch = filter_passes(&[alpha, dilate, ImageFilter::Offset { dx: 0.0, dy: 0.0 }, tint]).unwrap();
    assert_eq!(sketch.len(), 1);
    assert!(matches!(sketch[0].filter, ImageFilter::Morphology { .. }));
    assert!(sketch[0].fused.source_alpha);
    assert!(matches!(sketch[0].fused.post_matrix, Some(m) if m[18] == 0.25));
    assert!(plan_stores_flipped(&sketch, true));

    let blurred = filter_passes(&[alpha, ImageFilter::gaussian_blur(2.0), ImageFilter::brightness(0.5)]).unwrap();
    assert_eq!(blurred.len(), 1);
    assert!(blurred[0].fused.source_alpha && blurred[0].fused.post_matrix.is_some());

    let bright_first = filter_passes(&[ImageFilter::brightness(0.5), ImageFilter::gaussian_blur(2.0)]).unwrap();
    assert_eq!(bright_first.len(), 2, "a matrix that is not alpha-only keeps its pass");
    assert!(!bright_first[1].fused.source_alpha);

    let pyramid = filter_passes(&[alpha, ImageFilter::gaussian_blur(16.0), tint]).unwrap();
    assert_eq!(pyramid.len(), 3, "the matrices ride the halving and the scale back up");
    assert!(!pyramid[1].fused.source_alpha && pyramid[1].fused.post_matrix.is_none());

    let two = filter_passes(&[ImageFilter::gaussian_blur(2.0), dilate]).unwrap();
    assert_eq!(two.len(), 2);
    assert!(two
        .iter()
        .all(|pass| !pass.fused.source_alpha && pass.fused.post_matrix.is_none()));
}

/// A plan's result is stored the way its passes leave it: a single draw
/// turns the image over and a two-draw filter does not, from either kind of
/// source, while the noise generator reads nothing and stores the way a
/// draw into a target lands whatever its source - so the passes after it
/// count from there.
#[test]
fn a_plan_follows_the_image_through_its_passes() {
    use crate::{ImageFilter, TurbulenceKind};
    let noise = ImageFilter::Turbulence {
        base_frequency: [0.1, 0.1],
        num_octaves: 1,
        seed: 1,
        stitch_tiles: false,
        kind: TurbulenceKind::FractalNoise,
        transform: crate::Transform2D::identity(),
    };
    let bright = ImageFilter::brightness(0.5);
    let blur = ImageFilter::gaussian_blur(2.0);
    let stored = |filters: &[ImageFilter], source_flipped: bool| {
        plan_stores_flipped(&filter_passes(filters).unwrap(), source_flipped)
    };
    for source_flipped in [false, true] {
        assert_eq!(stored(&[bright], source_flipped), !source_flipped);
        assert_eq!(stored(&[blur], source_flipped), source_flipped);
        assert_eq!(
            stored(&[bright, blur, ImageFilter::LinearRgbToSrgb], source_flipped),
            source_flipped
        );
        assert!(stored(&[noise], source_flipped));
        assert!(stored(&[noise, blur], source_flipped));
        assert!(!stored(&[noise, bright], source_flipped));
        assert!(stored(&[noise, bright, ImageFilter::LinearRgbToSrgb], source_flipped));
        assert!(!stored(&[bright, noise, ImageFilter::LinearRgbToSrgb], source_flipped));
    }
}

/// A chain copies its result once more only when the passes would leave it
/// stored the other way from how its target is read: into a `FLIP_Y`
/// target from an upload, a lone blur (no flip) gets the copy and a matrix
/// (one flip) does not; into a target without the flag it is the reverse;
/// and an empty list is the one copy.
#[test]
fn a_chain_copies_only_when_its_target_reads_the_other_way() {
    use crate::ImageFilter;
    let recorded = |filters: &[ImageFilter], flip_target: bool| -> usize {
        let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
        canvas.set_size(64, 64, 1.0);
        let source = canvas
            .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
            .unwrap();
        let flags = if flip_target {
            ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y
        } else {
            ImageFlags::PREMULTIPLIED
        };
        let target = canvas.create_image_empty(16, 16, PixelFormat::Rgba8, flags).unwrap();
        canvas.filter_image_chain(target, filters, source).unwrap();
        canvas
            .commands
            .iter()
            .filter(|command| matches!(command.cmd_type, CommandType::RenderFilteredImage { .. }))
            .count()
    };
    let blur = ImageFilter::gaussian_blur(2.0);
    let bright = ImageFilter::brightness(0.5);
    assert_eq!(recorded(&[blur], true), 2);
    assert_eq!(recorded(&[bright], true), 1);
    assert_eq!(recorded(&[blur], false), 1);
    assert_eq!(recorded(&[bright], false), 2);
    assert_eq!(recorded(&[], true), 1);
    assert_eq!(recorded(&[], false), 1);
}

#[test]
fn filter_work_matches_shader_sampling_and_resets_at_flush() {
    let blur = ImageFilter::gaussian_blur(8.0);
    assert_eq!(filter_work(&[Pass::at(blur, FULL)], 10, 10), 9_400);
    // A morphology taps its radius either side of the center on each axis,
    // an offset is one tap.
    let dilate = ImageFilter::Morphology {
        radius_x: 2.0,
        radius_y: 3.0,
        operator: MorphologyOperator::Dilate,
    };
    assert_eq!(filter_work(&[Pass::at(dilate, FULL)], 10, 10), 1_200);
    assert_eq!(
        filter_work(&[Pass::at(ImageFilter::Offset { dx: 1.0, dy: 1.0 }, FULL)], 10, 10),
        100
    );
    // One axis blurred, the other copied: 47 taps plus the copy's one.
    let streak = ImageFilter::GaussianBlur {
        sigma_x: 8.0,
        sigma_y: 0.0,
    };
    assert_eq!(filter_work(&[Pass::at(streak, FULL)], 10, 10), 4_800);
    // Sigma 16 over 64 x 64: a quarter-size halving, the blur of 8 over a
    // quarter of the pixels and a full-size scale back up - a fifteenth of
    // the four full-size passes of quadrature (1,540,096). The streak of 16
    // halves x alone, so its level is half the chain.
    assert_eq!(
        filter_work(&blur_passes(16.0, 16.0), 64, 64),
        1_024 + 94 * 1_024 + 4_096
    );
    assert_eq!(filter_work(&blur_passes(16.0, 0.0), 64, 64), 2_048 + 48 * 2_048 + 4_096);
    let turbulence = |num_octaves| ImageFilter::Turbulence {
        base_frequency: [0.1, 0.1],
        num_octaves,
        seed: 1,
        stitch_tiles: false,
        kind: TurbulenceKind::Turbulence,
        transform: Transform2D::identity(),
    };
    assert_eq!(filter_work(&[Pass::at(turbulence(0), FULL)], 10, 10), 100);
    assert_eq!(filter_work(&[Pass::at(turbulence(10), FULL)], 10, 10), 8_000);

    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(16, 16, 1.0);
    let source = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    // Read through FLIP_Y, the way a one-flip chain leaves it: no copy pass.
    let target = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::FLIP_Y)
        .unwrap();
    let blend = ImageFilter::Blend {
        mode: crate::BlendMode::Multiply,
        backdrop: source,
        x: 0.0,
        y: 0.0,
        width: 16.0,
        height: 16.0,
    };
    assert_eq!(
        filter_work(&[Pass::at(blend, FULL)], 10, 10),
        400,
        "two reads, a clear and a draw"
    );
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

/// A crop rides the pass before it - a blur's second draw, the scale back
/// up of a blur run down the pyramid - and with no pass before it is a copy
/// of its own; two in a row leave what both hold. What a pass clipped away
/// stays away: a pass after a crop folds into the cropped one only where
/// one crop leaves what the two would ([`folded_crop`]).
#[test]
fn a_crop_rides_the_pass_before_it_and_passes_fold_across_it_where_one_crop_does() {
    use crate::ImageFilter;
    let crop = |x: f32, width: f32| ImageFilter::Crop {
        x,
        y: 2.0,
        width,
        height: 10.0,
    };
    let (bright, gray) = (ImageFilter::brightness(0.5), ImageFilter::grayscale(1.0));
    let blur = ImageFilter::gaussian_blur(2.0);
    // Each pass: whether it is two draws, its crop, whether a matrix rides its second draw.
    let plan = |filters: &[ImageFilter]| -> Vec<(bool, Option<[f32; 4]>, bool)> {
        filter_passes(filters)
            .unwrap()
            .iter()
            .map(|pass| (pass.filter.two_pass(), pass.crop, pass.fused.post_matrix.is_some()))
            .collect()
    };
    let rect = Some([4.0, 2.0, 20.0, 10.0]);
    assert_eq!(plan(&[bright, crop(4.0, 20.0)]), [(false, rect, false)]);
    assert_eq!(plan(&[blur, crop(4.0, 20.0)]), [(true, rect, false)]);
    assert_eq!(plan(&[crop(4.0, 20.0)]), [(false, rect, false)], "alone: a copy");
    assert_eq!(plan(&[crop(4.0, 20.0), bright]), [(false, rect, false)]);
    assert_eq!(
        plan(&[bright, crop(4.0, 20.0), crop(10.0, 30.0)]),
        [(false, Some([10.0, 2.0, 14.0, 10.0]), false)],
        "what both hold"
    );

    // A matrix after a crop folds into the cropped pass, or into a blur's
    // second draw, when one crop leaves what the two passes would: its own
    // crop lies inside the first, or it leaves transparent black as it is,
    // as a matrix that adds nothing to alpha does.
    let mut opaque = [0.0; 20];
    (opaque[0], opaque[6], opaque[12], opaque[19]) = (1.0, 1.0, 1.0, 1.0);
    let opaque = ImageFilter::ColorMatrix { matrix: opaque };
    let narrow = Some([10.0, 2.0, 14.0, 10.0]);
    assert_eq!(plan(&[bright, gray]).len(), 1);
    assert_eq!(plan(&[bright, gray, crop(4.0, 20.0)]), [(false, rect, false)]);
    assert_eq!(plan(&[bright, crop(4.0, 20.0), gray]), [(false, rect, false)]);
    assert_eq!(
        plan(&[bright, crop(4.0, 20.0), gray, crop(10.0, 30.0)]),
        [(false, narrow, false)],
        "a matrix that keeps transparent black: what both crops hold"
    );
    assert_eq!(
        plan(&[bright, crop(4.0, 20.0), opaque]),
        [(false, rect, false), (false, None, false)],
        "outside the crop the second matrix draws"
    );
    assert_eq!(
        plan(&[bright, crop(4.0, 20.0), opaque, crop(10.0, 30.0)]),
        [(false, rect, false), (false, Some([10.0, 2.0, 30.0, 10.0]), false)],
        "and inside its own crop, past the first one's edge"
    );
    assert_eq!(
        plan(&[bright, crop(4.0, 20.0), opaque, crop(4.0, 20.0)]),
        [(false, rect, false)],
        "the same crop twice is one crop"
    );
    assert_eq!(
        plan(&[bright, crop(4.0, 20.0), opaque, crop(10.0, 14.0)]),
        [(false, narrow, false)]
    );
    assert_eq!(plan(&[blur, gray]), [(true, None, true)]);
    assert_eq!(plan(&[blur, gray, crop(4.0, 20.0)]), [(true, rect, true)]);
    assert_eq!(plan(&[blur, crop(4.0, 20.0), gray]), [(true, rect, true)]);
    assert_eq!(
        plan(&[blur, crop(4.0, 20.0), gray, crop(4.0, 20.0)]),
        [(true, rect, true)]
    );
    assert_eq!(
        plan(&[blur, crop(4.0, 20.0), opaque]),
        [(true, rect, false), (false, None, false)]
    );
    // A blur reads past its own pixels, so nothing before it gives up its crop.
    assert_eq!(
        plan(&[bright, crop(4.0, 20.0), blur, crop(4.0, 20.0)]),
        [(false, rect, false), (true, rect, false)]
    );

    let wide = filter_passes(&[ImageFilter::gaussian_blur(40.0), crop(4.0, 20.0)]).unwrap();
    assert!(wide.len() > 2, "run down the pyramid");
    assert_eq!(wide.iter().filter(|pass| pass.crop.is_some()).count(), 1);
    let last = wide.last().unwrap();
    assert_eq!((last.level, last.crop), (FULL, rect));

    // A matrix rides a pyramid's first halving, but not with its crop: the
    // rect is of full-size pixels, and the halving draws at half size.
    let uncropped = filter_passes(&[bright, ImageFilter::gaussian_blur(40.0)]).unwrap();
    let cropped = filter_passes(&[bright, crop(4.0, 20.0), ImageFilter::gaussian_blur(40.0)]).unwrap();
    assert_eq!(cropped.len(), uncropped.len() + 1);
    assert_eq!((cropped[0].level, cropped[0].crop), (FULL, rect));
    assert!(cropped[1..].iter().all(|pass| pass.crop.is_none()));
}

/// A pass draws inside its crop only: the command carries the rect, rounded
/// out to whole pixels and cut to the image, in the rows the pass's result
/// is stored in - counted from the bottom when a single draw from an upload
/// stores it the way a render target is, from the top when a blur's two
/// draws leave it as the source was.
#[test]
fn a_cropped_pass_carries_the_rect_in_the_rows_it_stores() {
    use crate::ImageFilter;
    let crops = |filters: &[ImageFilter]| -> Vec<Option<[u32; 4]>> {
        let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
        canvas.set_size(64, 64, 1.0);
        let source = canvas
            .create_image_empty(64, 32, PixelFormat::Rgba8, ImageFlags::empty())
            .unwrap();
        let target = canvas
            .create_image_empty(
                64,
                32,
                PixelFormat::Rgba8,
                ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y,
            )
            .unwrap();
        canvas.filter_image_chain(target, filters, source).unwrap();
        canvas
            .commands
            .iter()
            .filter(|command| matches!(command.cmd_type, CommandType::RenderFilteredImage { .. }))
            .map(|command| command.crop)
            .collect()
    };
    let crop = |x: f32, y: f32, width: f32, height: f32| ImageFilter::Crop { x, y, width, height };
    let bright = ImageFilter::brightness(0.5);
    let blur = ImageFilter::gaussian_blur(2.0);
    assert_eq!(
        crops(&[bright, crop(4.0, 2.0, 20.0, 10.0)]),
        [Some([4, 20, 20, 10])],
        "one draw from an upload: rows from the bottom"
    );
    assert_eq!(
        crops(&[blur, crop(4.0, 2.0, 20.0, 10.0)]),
        [Some([4, 2, 20, 10]), None],
        "two draws, then the copy that turns the result over for its target"
    );
    assert_eq!(
        crops(&[blur, crop(4.3, 2.6, 20.5, 10.25)]),
        [Some([4, 2, 21, 11]), None],
        "rounded out"
    );
    assert_eq!(
        crops(&[blur, crop(-5.0, -5.0, 1000.0, 1000.0)])[0],
        Some([0, 0, 64, 32])
    );
    assert_eq!(crops(&[blur, crop(100.0, 2.0, 20.0, 10.0)])[0], Some([64, 2, 0, 10]));
    assert_eq!(
        crops(&[blur, crop(f32::NAN, 2.0, 20.0, 10.0)])[0].map(|rect| rect[2]),
        Some(0)
    );
    assert_eq!(crops(&[bright]), [None]);
}
