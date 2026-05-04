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
use crate::primitives::size::Size;
use glyphon::cosmic_text::{
    Attrs, AttrsOwned, Buffer, Family, FontSystem, Metrics, Shaping, fontdb,
};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Bundled fonts shipped with the crate. Inter for proportional / UI body,
/// JetBrains Mono for monospace. Both OFL 1.1.
const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
const INTER_BOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-Bold.ttf");
const JBMONO_REGULAR: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
const JBMONO_BOLD: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf");

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
    /// Width of the widest unbreakable run, in logical px. Computed once on
    /// insert from the unbounded shaping result and reused for every later
    /// `measure` call that hits this entry.
    intrinsic_min: f32,
}

/// Real-shaping text measurer. Owns a [`FontSystem`] (system fonts via
/// [`CosmicMeasure::new`], or just the bundled Inter + JetBrains Mono via
/// [`CosmicMeasure::with_bundled_fonts`]) and a cache of shaped `Buffer`s
/// keyed on the inputs that affect shaping.
///
/// `default_attrs` is what `measure` uses when shaping — apps that need a
/// different family/weight/style would build their own [`Attrs`] eventually,
/// but today every run goes through the same default.
pub struct CosmicMeasure {
    font_system: FontSystem,
    cache: HashMap<TextCacheKey, CacheEntry>,
    default_attrs: AttrsOwned,
}

impl CosmicMeasure {
    /// Use the OS's font set. Picks up whatever system fonts are installed
    /// (slow on cold start, nondeterministic across machines — fine for apps,
    /// avoid in tests). Default family is `SansSerif`.
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            cache: HashMap::new(),
            default_attrs: AttrsOwned::new(&Attrs::new()),
        }
    }

    /// Use only the bundled fonts (Inter + JetBrains Mono, regular + bold).
    /// No system font scan: fast, deterministic, and gives the same metrics
    /// on every machine. Default family is `Inter`.
    pub fn with_bundled_fonts() -> Self {
        let sources = [INTER_REGULAR, INTER_BOLD, JBMONO_REGULAR, JBMONO_BOLD]
            .into_iter()
            .map(|b| fontdb::Source::Binary(Arc::new(b)));
        let font_system = FontSystem::new_with_fonts(sources);
        Self {
            font_system,
            cache: HashMap::new(),
            default_attrs: AttrsOwned::new(&Attrs::new().family(Family::Name("Inter"))),
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

    /// Split borrow: `font_system` + `lookup`. Glyphon's `prepare` needs
    /// `&mut FontSystem` while we iterate `RenderBuffer.text_runs` and look
    /// up buffers — borrowck won't let us call `buffer_for` and
    /// `font_system_mut` simultaneously through `&mut self`. This method
    /// hands out the disjoint pieces.
    pub fn split_for_render(&mut self) -> RenderSplit<'_> {
        RenderSplit {
            font_system: &mut self.font_system,
            lookup: BufferLookup { cache: &self.cache },
        }
    }
}

/// Disjoint borrow handed out by [`CosmicMeasure::split_for_render`].
pub struct RenderSplit<'a> {
    pub font_system: &'a mut FontSystem,
    pub lookup: BufferLookup<'a>,
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
                intrinsic_min: 0.0,
            };
        }
        let key = key_for(text, font_size_px, max_width_px);
        if let Some(entry) = self.cache.get(&key) {
            return MeasureResult {
                size: entry.measured,
                key,
                intrinsic_min: entry.intrinsic_min,
            };
        }

        let metrics = Metrics::new(font_size_px, font_size_px * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, max_width_px, None);
        buffer.set_text(
            &mut self.font_system,
            text,
            &self.default_attrs.as_attrs(),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        let mut max_w = 0.0_f32;
        let mut total_h = 0.0_f32;
        let mut intrinsic_min = 0.0_f32;
        let mut current_word_w = 0.0_f32;
        for run in buffer.layout_runs() {
            max_w = max_w.max(run.line_w);
            total_h = total_h.max(run.line_top + run.line_height);
            for g in run.glyphs {
                let cluster = &run.text[g.start..g.end];
                let is_break = cluster.chars().all(|c| c.is_whitespace());
                if is_break {
                    intrinsic_min = intrinsic_min.max(current_word_w);
                    current_word_w = 0.0;
                } else {
                    current_word_w += g.w;
                }
            }
            // Hard line break (\n) terminates a run — also closes any
            // in-progress word.
            intrinsic_min = intrinsic_min.max(current_word_w);
            current_word_w = 0.0;
        }
        let measured = Size::new(max_w.ceil(), total_h.ceil());
        self.cache.insert(
            key,
            CacheEntry {
                buffer,
                measured,
                intrinsic_min,
            },
        );
        MeasureResult {
            size: measured,
            key,
            intrinsic_min,
        }
    }
}
