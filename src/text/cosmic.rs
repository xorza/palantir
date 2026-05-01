//! Real text shaping via [`cosmic_text`]. Caches one `Buffer` per
//! `(text, font_size, max_width)` triple so steady-state measurement is
//! `HashMap` lookup only — no reshape, no allocation.
//!
//! Hash collisions are theoretically possible (we key on a 64-bit hash of the
//! text rather than storing the full string), but at typical UI scales the
//! cost of resolving them — verifying with the cached buffer's source string
//! on every hit — outweighs the cost of accepting the negligible risk. The
//! cache is per-`Ui` and rebuilt whenever `Ui` is dropped.

use super::TextMeasure;
use crate::primitives::Size;
use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Hash, Eq, PartialEq)]
struct CacheKey {
    text_hash: u64,
    /// `font_size_px * 64`, rounded. Quantizing to 1/64 px is far below any
    /// visible difference and lets us key on `u32` instead of `f32`.
    size_q: u32,
    /// `max_width_px * 64`, rounded; `u32::MAX` encodes `None`.
    max_w_q: u32,
}

const MAX_W_NONE: u32 = u32::MAX;

fn quantize(v: f32) -> u32 {
    (v.max(0.0) * 64.0).round() as u32
}

fn key_for(text: &str, size_px: f32, max_w_px: Option<f32>) -> CacheKey {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    CacheKey {
        text_hash: h.finish(),
        size_q: quantize(size_px),
        max_w_q: max_w_px.map(quantize).unwrap_or(MAX_W_NONE),
    }
}

struct CacheEntry {
    /// Shaped buffer. Kept alive (rather than dropped after measure) so the
    /// renderer can reuse it without reshaping when wired up. Read once the
    /// glyphon path lands; for now the field exists purely to anchor the
    /// shaped state until then.
    #[allow(dead_code)]
    buffer: Buffer,
    measured: Size,
}

/// Real-shaping [`TextMeasure`]. Owns a [`FontSystem`] (system fonts only at
/// the moment — no bundled font yet) and a cache of shaped `Buffer`s keyed
/// on the inputs that affect shaping.
pub struct CosmicMeasure {
    font_system: FontSystem,
    cache: HashMap<CacheKey, CacheEntry>,
}

impl CosmicMeasure {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            cache: HashMap::new(),
        }
    }

    /// Borrow the underlying `FontSystem` (e.g. to register additional fonts).
    pub fn font_system(&mut self) -> &mut FontSystem {
        &mut self.font_system
    }
}

impl Default for CosmicMeasure {
    fn default() -> Self {
        Self::new()
    }
}

impl TextMeasure for CosmicMeasure {
    fn measure(&mut self, text: &str, font_size_px: f32, max_width_px: Option<f32>) -> Size {
        if text.is_empty() || font_size_px <= 0.0 {
            return Size::ZERO;
        }
        let key = key_for(text, font_size_px, max_width_px);
        if let Some(entry) = self.cache.get(&key) {
            return entry.measured;
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
        measured
    }
}
