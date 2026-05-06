# Text

## Later — workload-gated

- **`CosmicMeasure.cache` eviction (Layer B).** Refcount
  `TextCacheKey` by live `WidgetId`s, sweep via `SeenIds.removed()`.
- **`Shape::Text.text` allocs.** `Cow<'static, str>` for static labels;
  intern dynamic via `Arc<str>` keyed on text hash.
- **Atlas eviction under multi-font / multi-size load.**
- **Wallclock bench for the reuse cache.** `benches/layout.rs` runs
  without cosmic — add a cosmic variant for real µs/frame numbers.
