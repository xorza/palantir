# Skip cosmic-text reshape for clean Text nodes

**Status:** Layer A (per-`WidgetId` reuse cache + sweep on removed
widgets) shipped. Bench and Layer B (cosmic cache eviction) still
pending. See "What shipped" below for the final design — which
diverges from the original plan in a few places — and the original
investigation further down for context.

## What shipped (Layer A)

The reuse cache lives on **`TextMeasurer`**, not on `LayoutEngine` as
the original plan proposed. Putting it next to the dispatcher means
the dispatch-skip and the cache live behind one façade, and `Ui` only
threads removed-widget state once instead of twice.

### Key types (`src/text/mod.rs`)

```rust
pub(crate) struct TextReuseEntry {
    hash: NodeHash,
    unbounded: MeasureResult,
    wrap: Option<WrapReuse>,
}

pub(crate) struct WrapReuse {
    target_q: u32,
    result: MeasureResult,
}

pub struct TextMeasurer {
    cosmic: Option<SharedCosmic>,
    pub(crate) measure_calls: u64,
    pub(crate) reuse: FxHashMap<WidgetId, TextReuseEntry>,
}
```

`hash` is the `NodeHash(u64)` newtype from `src/tree/hash.rs` so type
safety prevents accidental confusion with `WidgetId` and other 64-bit
handles in signatures.

### Public surface

- `shape_unbounded(wid, hash, src, font_size_px) -> MeasureResult` —
  identity-cached unbounded shape. Refreshes the entry on `NodeHash`
  shift and clears the stale wrap slot.
- `shape_wrap(wid, src, font_size_px, target, target_q) -> MeasureResult`
  — identity-cached wrap shape. Hits the cache when `target_q`
  matches; otherwise dispatches and writes the result back.
- `sweep_removed(removed: &[WidgetId])` — drops entries whose
  `WidgetId` disappeared this frame.

`measure_raw` doesn't exist as a method anymore: a free `dispatch`
function inside the module bottoms out the cache misses, taking
`&self.cosmic` so the cached methods can hold a `&mut TextReuseEntry`
across the dispatch call (disjoint field borrows).

Both `shape_unbounded` and `shape_wrap` do **one hash lookup per
call**: `Entry::Occupied`/`Vacant` for the former, `get_mut` for the
latter (the no-prime branch dispatches without caching).

### Hot-path call graph

`shape_text` (in `LayoutEngine::leaf_content_size`):

1. `text.shape_unbounded(wid, hash, src, size)` — get the unbounded
   result.
2. Compare `available_w < unbounded.size.w` to decide wrap.
3. If wrap: `text.shape_wrap(wid, src, size, target, target_q)`.
4. Else: use `unbounded` as-is.

`intrinsic::leaf` only ever wants unbounded; calls `shape_unbounded`
directly.

### Wrap-target quantization

`quantize_wrap_target(v) = (v.max(0.0) * 10.0).round() as u32` lives
in `src/layout/mod.rs` — layout policy, not text concern. ~0.1
logical-px granularity. Coarser wouldn't change line breaks (smallest
glyph advance is several px); finer would defeat reuse on animated
parents.

### Removed-widget sweep

The sweep is fed by `SeenIds` (`src/ui/seen_ids.rs`), a per-frame
`WidgetId` tracker that owns:

- collision detection (`record(id) -> bool` for `Ui::node`'s assert),
- removed-widget diff (`begin_frame` swaps `curr ↔ prev` and clears
  `curr`; `end_frame` produces `removed: Vec<WidgetId>`),
- the `removed()` slice consumed by both `Damage::compute` and
  `TextMeasurer::sweep_removed`.

This unifies what was originally going to be two independent
`seen_ids` walks (one for damage, one for text reuse).

### Test surface (`src/ui/tests.rs`)

Six tests, all observed via `ui.text.measure_calls` (a `pub(crate)
u64` field — no test-only accessor methods):

- `text_reshape_skipped_when_unchanged_across_frames` — steady-state
  frame: 0 extra dispatches.
- `text_reshape_runs_when_content_changes` — content change → 1.
- `wrapping_text_reshape_skipped_when_unchanged` — wrapped, unchanged
  → 0.
- `intrinsic_query_reuses_cached_text_measure` — intrinsic-only path
  reuses → 0.
- `text_reuse_evicts_disappeared_widgets` — sweep evicts within one
  frame.
- `wrap_target_change_preserves_unbounded_cache` — wrap target shifts
  but content unchanged → 1 (the wrap reshape only).

## Still pending

### Bench / wallclock numbers (Step 5 of original plan)

`benches/layout.rs` runs without cosmic installed (`mono_measure`
fallthrough), so it can't see the win. Need a cosmic-enabled variant
with N=100 static text labels to measure absolute µs/frame and
demonstrate the dispatch savings translate into wall-clock.

### Layer B — `CosmicMeasure.cache` eviction

`CosmicMeasure.cache` (content-keyed, owns shaped `Buffer`s) has no
eviction. A log viewer cycling through 10000 distinct strings keeps
every shaped buffer alive forever. Plan: refcount `TextCacheKey` by
live `WidgetId`, sweep on widget disappearance (mirrors the same
`SeenIds.removed()` diff). Defer until a real workload shows the
unbounded cache as a problem.

---

## Original investigation (preserved for context)

### `CosmicMeasure.cache` already dedupes by content

`src/text/cosmic.rs`:

```rust
struct CacheEntry { buffer: Buffer, measured: Size, intrinsic_min: f32 }

pub struct CosmicMeasure {
    cache: HashMap<TextCacheKey, CacheEntry>,
    ...
}

pub fn measure(&mut self, text, font_size_px, max_width_px) -> MeasureResult {
    let key = key_for(text, font_size_px, max_width_px);
    if let Some(entry) = self.cache.get(&key) { return ... entry ... }
    // else: shape, insert, return
}
```

`TextCacheKey = (text_hash, size_q, max_w_q)`. Steady state is already
a single `FxHashMap` lookup — **no reshape happens** when the same
`(content, size, max_w)` triple repeats. The `Buffer` is also reused
on the renderer side via `BufferLookup::get`, so glyphon never
reshapes either.

So "skip reshape" in the literal sense was already done by content.
The remaining costs per frame for an unchanged Text node were:

1. `TextMeasurer::measure` dispatch + `RefCell::borrow_mut` +
   `key_for(text, ...)` (hashes the full string).
2. `HashMap::get(&key)` + clone the small `MeasureResult`.
3. `LayoutEngine::set_text_shape(node, …)` writeback.

Cost (1) — hashing the full string — was the real per-frame charge
for every visible label. Layer A removes it for unchanged widgets.

### Authoring hash captures the right inputs

`Tree.compute_hashes` (`src/tree/hash.rs`) folds every `Shape`
including `Shape::Text { text, font_size_px, wrap, color, align }`
into the per-node `NodeHash`. So if the hash is unchanged AND the
parent's available width is unchanged, the `MeasureResult` from last
frame is reusable verbatim.

### Sequencing constraint

`Tree::compute_hashes` had to move *before* layout in `Ui::end_frame`
(it used to run after, just before damage). The reordering is pure:
hashes don't depend on layout output. Damage still reads the same
hashes; nothing else needed to move.

## Risks / things to watch (still relevant)

- **`avail_w` quantization granularity.** Currently 10× (~0.1 px).
  Coarser would broaden cache hit rate at the cost of edge-case
  correctness (sub-pixel jitter in animated layouts could in theory
  shift a single character to a new line). Tunable.

- **`TextCacheKey` identity across frames.** Layer A stores the
  `key` from a prior frame's measure inside `MeasureResult`. If
  `CosmicMeasure.cache` ever evicted that key (Layer B), the
  renderer's `BufferLookup::get` would return `None` and the run
  would silently drop. Layer B's eviction must respect outstanding
  `text_reuse` references. Until Layer B ships, non-issue (cache
  never evicts).

- **`#[track_caller]` interaction.** Reuse hinges on stable
  `WidgetId`s. The CLAUDE.md note about `#[track_caller]` not
  propagating through closures applies — give widgets in helpers
  explicit ids. Wrong ids would just miss reuse, not corrupt
  anything.

## Acceptance bar

- ✅ Steady-state showcase: zero `text.measure` dispatches per frame
  for unchanged Text nodes. Pinned by tests.
- ✅ All existing tests pass; new tests pin reuse + invalidation.
- ⏳ Bench shows measurable CPU drop on the static-labels workload.
- ✅ Removed widgets don't leak `text_reuse` entries.
- ⏳ (Layer B) `CosmicMeasure.cache` size stays bounded under
  string-churn stress.
