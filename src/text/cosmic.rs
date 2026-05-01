//! Real text shaping via [`cosmic_text`]. Caches one `Buffer` per
//! `(text, font_size, max_width)` triple so steady-state measurement is
//! `HashMap` lookup only — no reshape, no allocation.
//!
//! The renderer (`WgpuBackend`) downcasts the trait object to this concrete
//! type to reach the cached `Buffer`s and the `FontSystem` for
//! `glyphon::TextRenderer::prepare`.
//!
//! Hash collisions are theoretically possible (we key on a 64-bit hash of the
//! text rather than storing the full string), but at typical UI scales the
//! cost of resolving them — verifying with the cached buffer's source string
//! on every hit — outweighs the cost of accepting the negligible risk.

use super::{MeasureResult, TextCacheKey};
use crate::primitives::Size;
use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const MAX_W_NONE: u32 = u32::MAX;

fn quantize(v: f32) -> u32 {
    (v.max(0.0) * 64.0).round() as u32
}

fn key_for(text: &str, size_px: f32, max_w_px: Option<f32>) -> TextCacheKey {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    let mut text_hash = h.finish();
    // Avoid colliding with INVALID. Probability astronomically low; flip a
    // bit if it happens so the renderer never silently drops a real run.
    if text_hash == 0 {
        text_hash = 1;
    }
    TextCacheKey {
        text_hash,
        size_q: quantize(size_px),
        max_w_q: max_w_px.map(quantize).unwrap_or(MAX_W_NONE),
    }
}

struct CacheEntry {
    /// Shaped buffer. Looked up by [`TextCacheKey`] at render time so glyphon
    /// can build a `TextArea` without reshaping.
    buffer: Buffer,
    measured: Size,
}

/// Real-shaping [`TextMeasure`]. Owns a [`FontSystem`] (system fonts only at
/// the moment — no bundled font yet) and a cache of shaped `Buffer`s keyed
/// on the inputs that affect shaping.
pub struct CosmicMeasure {
    font_system: FontSystem,
    cache: HashMap<TextCacheKey, CacheEntry>,
}

impl CosmicMeasure {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            cache: HashMap::new(),
        }
    }

    /// Borrow the underlying `FontSystem` (e.g. to register additional fonts,
    /// or for `glyphon::TextRenderer::prepare`).
    pub fn font_system_mut(&mut self) -> &mut FontSystem {
        &mut self.font_system
    }

    /// Look up the shaped buffer for `key`. Returns `None` for keys that
    /// were never measured this `CosmicMeasure` instance — including
    /// [`TextCacheKey::INVALID`].
    pub fn buffer_for(&self, key: TextCacheKey) -> Option<&Buffer> {
        if key.is_invalid() {
            return None;
        }
        self.cache.get(&key).map(|e| &e.buffer)
    }

    /// Split borrow: `(font_system, cache_lookup)`. Glyphon's `prepare` needs
    /// `&mut FontSystem` while we iterate `RenderBuffer.text_runs` and look
    /// up buffers — borrowck won't let us call `buffer_for` and
    /// `font_system_mut` simultaneously through `&mut self`. This method
    /// hands out the disjoint pieces.
    pub fn split_for_render(&mut self) -> (&mut FontSystem, BufferLookup<'_>) {
        (&mut self.font_system, BufferLookup { cache: &self.cache })
    }
}

/// Read-only view into the buffer cache. Constructed by
/// [`CosmicMeasure::split_for_render`]; held alongside a `&mut FontSystem`.
pub struct BufferLookup<'a> {
    cache: &'a HashMap<TextCacheKey, CacheEntry>,
}

impl<'a> BufferLookup<'a> {
    pub fn get(&self, key: TextCacheKey) -> Option<&'a Buffer> {
        if key.is_invalid() {
            return None;
        }
        self.cache.get(&key).map(|e| &e.buffer)
    }
}

impl Default for CosmicMeasure {
    fn default() -> Self {
        Self::new()
    }
}

impl CosmicMeasure {
    pub fn measure(
        &mut self,
        text: &str,
        font_size_px: f32,
        max_width_px: Option<f32>,
    ) -> MeasureResult {
        if text.is_empty() || font_size_px <= 0.0 {
            return MeasureResult {
                size: Size::ZERO,
                key: TextCacheKey::INVALID,
            };
        }
        let key = key_for(text, font_size_px, max_width_px);
        if let Some(entry) = self.cache.get(&key) {
            return MeasureResult {
                size: entry.measured,
                key,
            };
        }

        let metrics = Metrics::new(font_size_px, font_size_px * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, max_width_px, None);
        buffer.set_text(
            &mut self.font_system,
            text,
            &Attrs::new(),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        let mut max_w = 0.0_f32;
        let mut total_h = 0.0_f32;
        for run in buffer.layout_runs() {
            max_w = max_w.max(run.line_w);
            total_h = total_h.max(run.line_top + run.line_height);
        }
        let measured = Size::new(max_w.ceil(), total_h.ceil());
        self.cache.insert(key, CacheEntry { buffer, measured });
        MeasureResult {
            size: measured,
            key,
        }
    }
}
