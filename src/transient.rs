//! The pool of transient offscreen images: layer backing stores, filtered
//! results, filter-chain scratches and shadow coverage.
//!
//! Every transient lives until the next flush, when all of them are deleted.
//! Between a release and that flush an image is free for the next acquire of
//! the same size and flags, so a frame's peak transient memory is what is
//! live at once - nesting depth times layer size - not the sum over every
//! layer, chain and shadow it draws. Reuse needs no synchronization: commands
//! execute in order, so a later layer drawing into an image an earlier
//! layer's composite reads is well-defined, and each consumer clears or fully
//! overwrites the image it takes.

use crate::image::ImageStore;
use crate::renderer::Renderer;
use crate::{ErrorKind, ImageFlags, ImageId, ImageInfo, PixelFormat};

/// Default cap on live transient memory. A Raspberry Pi Zero's GPU share is
/// 64-128 MB in total, so integrations targeting it set a smaller budget; see
/// [`crate::Canvas::set_transient_image_budget`].
pub(crate) const DEFAULT_BUDGET: usize = 256 * 1024 * 1024;

/// Layer stores round up to this many pixels per axis. Sibling layers whose
/// scissors or blur reaches differ by a few pixels then request the same
/// size and share one pooled store instead of each holding its own; the cost
/// is at most 63 px per axis (a tenth of a 1080 px layer) and the extra area
/// is clipped away at the composite.
pub(crate) const LAYER_GRANULARITY: usize = 64;

/// Shadow coverage rounds more finely. Shadows are many, small and
/// differently sized (text, icons), and their coverage images are cleared and
/// blurred over their whole area: modelled on a frame of 600 glyph-sized
/// shadows, exact sizes allocate 650 images per frame, 8 px rounding 60 (for
/// 18 % more fill), 64 px rounding 8 (for 235 % more fill).
pub(crate) const SHADOW_GRANULARITY: usize = 8;

pub(crate) fn round_up(n: usize, granularity: usize) -> usize {
    n.div_ceil(granularity) * granularity
}

#[derive(Debug)]
pub(crate) struct TransientPool {
    /// Every live transient, deleted at the flush.
    pub(crate) images: Vec<ImageId>,
    /// Transients whose last consumer command has been recorded; a subset of
    /// `images`, taken by the next acquire of the same size and flags.
    pub(crate) free: Vec<ImageId>,
    /// Bytes held by `images`, against `budget`.
    bytes: usize,
    budget: usize,
}

impl TransientPool {
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            images: Vec::new(),
            free: Vec::new(),
            bytes: 0,
            budget,
        }
    }

    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn set_budget(&mut self, bytes: usize) {
        self.budget = bytes;
    }

    /// A pooled image of exactly `width` x `height` with `flags` if one is
    /// free, otherwise a fresh RGBA8 image counted against the budget.
    pub(crate) fn acquire<T: Renderer>(
        &mut self,
        images: &mut ImageStore<T::Image>,
        renderer: &mut T,
        width: usize,
        height: usize,
        flags: ImageFlags,
    ) -> Result<ImageId, ErrorKind> {
        let reusable = self.free.iter().position(|&id| {
            images
                .info(id)
                .is_some_and(|info| info.width() == width && info.height() == height && info.flags() == flags)
        });
        if let Some(at) = reusable {
            return Ok(self.free.swap_remove(at));
        }
        let bytes = width.saturating_mul(height).saturating_mul(4);
        if self.bytes.saturating_add(bytes) > self.budget {
            return Err(ErrorKind::TransientImageBudgetExceeded);
        }
        let id = images.alloc(renderer, ImageInfo::new(flags, width, height, PixelFormat::Rgba8))?;
        self.bytes = self.bytes.saturating_add(bytes);
        self.images.push(id);
        Ok(id)
    }

    /// Returns an image to the pool once every command that reads it has
    /// been recorded. Whoever takes it next must clear or fully overwrite it,
    /// as layers and filter passes do.
    pub(crate) fn release(&mut self, id: ImageId) {
        debug_assert!(self.images.contains(&id), "released image is not a transient");
        debug_assert!(!self.free.contains(&id), "transient released twice");
        self.free.push(id);
    }

    /// Deletes every transient except those in `held` (the stores of layers
    /// still open across the flush), which stay live and in use.
    pub(crate) fn release_all<T: Renderer>(
        &mut self,
        images: &mut ImageStore<T::Image>,
        renderer: &mut T,
        held: &[ImageId],
    ) {
        self.free.clear();
        let mut kept = Vec::new();
        for id in std::mem::take(&mut self.images) {
            if held.contains(&id) {
                kept.push(id);
            } else {
                self.bytes = self.bytes.saturating_sub(image_bytes(images, id));
                images.remove(renderer, id);
            }
        }
        self.images = kept;
    }
}

/// Bytes an image occupies, for the accounting.
fn image_bytes<I>(images: &ImageStore<I>, id: ImageId) -> usize {
    images
        .info(id)
        .map(|info| {
            let bpp = match info.format() {
                PixelFormat::Gray8 => 1,
                PixelFormat::Rgb8 => 3,
                PixelFormat::Rgba8 => 4,
            };
            info.width() * info.height() * bpp
        })
        .unwrap_or(0)
}
