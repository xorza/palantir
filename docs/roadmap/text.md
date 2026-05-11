# Text

## Later — workload-gated

- **`CosmicMeasure.cache` eviction (Layer B).** Refcount
  `TextCacheKey` by live `WidgetId`s, sweep via `SeenIds.removed()`.
- **`Shape::Text.text` dynamic-string interning.** Static labels already
  ride `Cow<'static, str>`; intern dynamic via `Arc<str>` keyed on text hash.
- **Atlas eviction under multi-font / multi-size load.**
- **Wallclock bench for the reuse cache.** `benches/layout.rs` runs
  without cosmic — add a cosmic variant for real µs/frame numbers.
- **Glyph atlas sub-pixel variants.** GPUI rasterizes up to 16
  sub-pixel-positioned variants per glyph for crisp text without
  full sub-pixel rendering; cosmic+glyphon ships one. Worth a
  quality bake-off when type rendering becomes a complaint.

## Non-goals

- **OS-native shaping (CoreText / DirectWrite / HarfBuzz-direct).**
  GPUI uses the platform shaper for a measurable speed win on
  macOS/Windows; the cost is a per-platform integration surface and
  divergent layout across OSes. We've bet on cosmic-text for
  consistency. Park unless a profiled workload shows shaping is the
  bottleneck *and* portability regression is acceptable.
