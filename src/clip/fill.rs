//! Fills thinner than a pixel, drawn through coverage masks. A fill's fringe
//! antialiases each edge for itself, so two edges closer than a pixel each
//! add their ramp: a sliver a tenth of a pixel wide comes out nearly eight
//! times as dark as its area (#327). Such a fill is rasterized on the CPU
//! instead, by the clip masks' exact coverage, into one atlas, and drawn as
//! a quad that reads its mask as a glyph's is read: the paint, the scissor
//! and every clip apply to it as they do to text. Skia's Graphite does the
//! same for small paths (`RasterPathAtlas`, at most 256 of them a flush).

use std::collections::HashMap;

use super::mask::MaskKey;
use crate::{text::Atlas, ImageId};

/// A fill is thin when twice its area over its perimeter - its mean width -
/// is under this, in device pixels.
pub(crate) const THIN_FILL_WIDTH: f32 = 1.0;

/// The most pixels a thin fill's bounds may span to be drawn through a mask.
pub(crate) const THIN_FILL_SPAN: f32 = 65_536.0;

/// The atlas's side in pixels at first, a byte each: a quarter of a megabyte.
const ATLAS_FIRST: usize = 512;

/// The side the atlas grows to at most, doubling when a frame's masks find
/// no room: four megabytes, more than the thin fills of any corpus frame at
/// 1080p took (1.9 MB).
const ATLAS_MOST: usize = 2048;

/// The most masks rasterized in a frame, as Skia's 256 a flush: past it a
/// thin fill is filled as any other, so that a scene whose thin fills move
/// every frame does not rasterize and upload them all again each time.
const MASKS_A_FRAME: usize = 256;

/// The most requests remembered bit for bit.
const ASKED_KEPT: usize = 4096;

/// What the atlas has for a new mask.
pub(crate) enum Placement {
    At([u32; 2]),
    /// No room, but the atlas can grow.
    Grow,
    Refused,
}

/// Where a mask lies in the atlas and the rect of the target it covers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlacedMask {
    pub(crate) atlas: [u32; 2],
    pub(crate) rect: [i32; 4],
}

/// The atlas and what it holds.
#[derive(Debug)]
pub(crate) struct FillMasks {
    enabled: bool,
    pub(crate) image: Option<ImageId>,
    packer: Atlas,
    masks: HashMap<MaskKey, [u32; 2]>,
    /// The masks of requests asked for bit for bit as before: the fill rule,
    /// the path's words, the transform, the tolerances and the target's size.
    asked: HashMap<Vec<u32>, PlacedMask>,
    made: usize,
    /// Masks the frame found again, by request or by outline.
    found: usize,
    /// Masks the frame before made that the atlas still holds.
    made_before: usize,
    full: bool,
    side: usize,
    // The images of atlases outgrown this frame, freed once it is drawn.
    retired: Vec<ImageId>,
    coverage: super::coverage::Coverage,
    // A mask's pixels on their way into the atlas, kept for the next.
    scratch: Vec<u8>,
    /// The atlas's pixels, which the masks a frame makes are written into
    /// and uploaded from once, as the rectangle they span, when it is drawn.
    pub(crate) pixels: Vec<u8>,
    /// That rectangle, `[x0, y0, x1, y1]`, and its rows side by side.
    dirty: Option<[usize; 4]>,
    upload: Vec<u8>,
    /// Every mask rasterized, for the tests.
    #[cfg(test)]
    pub(crate) rasterized: usize,
}

impl Default for FillMasks {
    fn default() -> Self {
        Self {
            enabled: true,
            image: None,
            packer: Atlas::new(ATLAS_FIRST, ATLAS_FIRST),
            masks: HashMap::new(),
            asked: HashMap::new(),
            made: 0,
            found: 0,
            made_before: 0,
            full: false,
            side: ATLAS_FIRST,
            retired: Vec::new(),
            coverage: Default::default(),
            scratch: Vec::new(),
            pixels: Vec::new(),
            dirty: None,
            upload: Vec::new(),
            #[cfg(test)]
            rasterized: 0,
        }
    }
}

impl FillMasks {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub(crate) fn side(&self) -> usize {
        self.side
    }

    pub(crate) fn asked(&mut self, request: &[u32]) -> Option<PlacedMask> {
        let mask = self.asked.get(request).copied();
        self.found += usize::from(mask.is_some());
        mask
    }

    pub(crate) fn remember(&mut self, request: Vec<u32>, mask: PlacedMask) {
        // A scene moved by whole pixels finds its masks by their outlines,
        // and asks anew every frame.
        if self.asked.len() >= ASKED_KEPT {
            self.asked.clear();
        }
        self.asked.insert(request, mask);
    }

    pub(crate) fn get(&mut self, key: &MaskKey) -> Option<[u32; 2]> {
        let at = self.masks.get(key).copied();
        self.found += usize::from(at.is_some());
        at
    }

    /// A place in the atlas for a new mask of `size`, if the frame has not
    /// made its share of masks and the atlas has room or can grow; a mask
    /// that does not fit an atlas at its largest marks it full, to start
    /// over once the frame is drawn.
    pub(crate) fn place(&mut self, size: [usize; 2]) -> Placement {
        if self.made >= MASKS_A_FRAME || self.full || size[0] > ATLAS_MOST || size[1] > ATLAS_MOST {
            return Placement::Refused;
        }
        let fits = size[0] <= self.side && size[1] <= self.side;
        let Some((x, y)) = fits.then(|| self.packer.add_rect(size[0], size[1])).flatten() else {
            if self.side < ATLAS_MOST {
                return Placement::Grow;
            }
            self.full = true;
            return Placement::Refused;
        };
        self.made += 1;
        #[cfg(test)]
        {
            self.rasterized += 1;
        }
        Placement::At([x as u32, y as u32])
    }

    /// Doubles the atlas's side, or more for a mask of `size` that needs
    /// it, each mask kept where it lies: the image is let go, freed once the
    /// frame is drawn, and the next one made of the pixels kept here. Masks
    /// made this frame must be in it before.
    pub(crate) fn grow(&mut self, size: [usize; 2]) {
        let old = self.side;
        let mut side = 2 * old;
        while side < ATLAS_MOST && side < size[0].max(size[1]) {
            side *= 2;
        }
        let mut pixels = vec![0; side * side];
        for (row, line) in self.pixels.chunks_exact(old).enumerate() {
            pixels[row * side..][..old].copy_from_slice(line);
        }
        self.pixels = pixels;
        self.dirty = None;
        self.side = side;
        // The old atlas held where it lies, the new room beside and above it.
        self.packer = Atlas::new(side, side);
        self.packer.add_rect(old, old);
        self.retired.extend(self.image.take());
    }

    /// Rasterizes the mask of `key` and keeps it at `at`.
    pub(crate) fn make(&mut self, key: MaskKey, at: [u32; 2]) {
        let mut pixels = std::mem::take(&mut self.scratch);
        key.rasterize_into(&mut self.coverage, &mut pixels);
        self.keep(key, at, &pixels);
        self.scratch = pixels;
    }

    /// Keeps the mask of `key`, its rows `pixels`, at `at`.
    fn keep(&mut self, key: MaskKey, at: [u32; 2], pixels: &[u8]) {
        let [width, height] = key.size();
        let [x, y] = at.map(|side| side as usize);
        for (row, line) in pixels.chunks_exact(width).enumerate() {
            let start = (y + row) * self.side + x;
            self.pixels[start..start + width].copy_from_slice(line);
        }
        let [x0, y0, x1, y1] = self.dirty.unwrap_or([x, y, x + width, y + height]);
        self.dirty = Some([x0.min(x), y0.min(y), x1.max(x + width), y1.max(y + height)]);
        self.masks.insert(key, at);
    }

    /// The masks the atlas holds, for the tests.
    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        self.masks.len()
    }

    /// The rectangle written since the last upload: its corner, its size
    /// and its rows side by side. Not the atlas's whole rows: a tall mask
    /// remade each frame of a moving scene would upload two megabytes.
    pub(crate) fn take_upload(&mut self) -> Option<([usize; 2], [usize; 2], &[u8])> {
        let [x0, y0, x1, y1] = self.dirty.take()?;
        self.upload.clear();
        for row in y0..y1 {
            let start = row * self.side;
            self.upload.extend_from_slice(&self.pixels[start + x0..start + x1]);
        }
        Some(([x0, y0], [x1 - x0, y1 - y0], &self.upload))
    }

    /// The atlas starts over once the frame is drawn: what it holds cannot
    /// be relied on.
    pub(crate) fn mark_full(&mut self) {
        self.full = true;
    }

    /// The frame is drawn: an atlas that filled starts over, its masks made
    /// again as the frames that follow ask for them. So does one whose frame
    /// found none of the masks the frame before made, making its own - a
    /// scene in motion, whose masks no frame asks for again - so that the
    /// atlas does not grow slower to pack into frame after frame. Returns the
    /// images of the atlases the frame outgrew, to be freed.
    pub(crate) fn end_frame(&mut self) -> Vec<ImageId> {
        if self.made > 0 && self.made_before > 0 && self.found == 0 {
            self.full = true;
        }
        self.made_before = self.made;
        self.made = 0;
        self.found = 0;
        if self.full {
            self.full = false;
            self.made_before = 0;
            self.packer.reset(self.side, self.side);
            self.masks.clear();
            self.asked.clear();
        }
        std::mem::take(&mut self.retired)
    }
}
