# Skip cosmic-text reshape for clean Text nodes

Stage 3 (damage rendering) ships the per-node dirty set
(`Damage.dirty: Vec<NodeId>`, `src/ui/damage/mod.rs:52,127`) but no
consumer reuses it yet. `damage-rendering.md` calls reshape-skip
"the biggest single win available." This doc plans that work.

## Investigation: what already exists

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

`TextCacheKey = (text_hash, size_q, max_w_q)`. Steady state is
already a single `FxHashMap` lookup — **no reshape happens** when
the same `(content, size, max_w)` triple repeats. The `Buffer` is
also reused on the renderer side via `BufferLookup::get`
(`src/text/cosmic.rs:138`), so glyphon never reshapes either.

So "skip reshape" in the literal sense is already done by content.
The remaining costs per frame for an unchanged Text node are:

1. `TextMeasurer::measure` dispatch + `RefCell::borrow_mut` +
   `key_for(text, ...)` (hashes the full string).
2. `HashMap::get(&key)` + clone the small `MeasureResult`.
3. `LayoutEngine::set_text_shape(node, ShapedText { measured, key })`
   into `LayoutResult.text_shapes` (which is reset to all-`None`
   every frame in `LayoutResult::reset`, `src/layout/result.rs:31`).
4. The leaf-content-size walk in `leaf_content_size`
   (`src/layout/mod.rs:250`) and any intrinsic queries
   (`src/layout/intrinsic.rs:113`) that traverse the same shape.

Cost (2) is small. Cost (1) — hashing the full string — is the
real per-frame charge for every visible label. For paragraph text
or many labels it adds up.

### `CosmicMeasure.cache` has no eviction

The cache grows monotonically. A log viewer cycling through
10 000 distinct strings keeps every shaped `Buffer` alive forever.
This is a separate problem from "skip reshape" but the natural fix
shares plumbing: tie cache lifetime to live `WidgetId`s, sweep when
they disappear (mirroring `Damage.prev`'s removed-widget sweep).

### Authoring hash already captures the right inputs

`Tree.compute_hashes` (`src/tree/hash.rs`) folds every `Shape`
including `Shape::Text { text, font_size_px, wrap, color, align }`
into the per-node 64-bit hash that Damage diffs. So if the hash is
unchanged AND the parent's available width is unchanged, the
`MeasureResult` from last frame is reusable verbatim.

### Damage runs *after* layout

`Ui::end_frame` (`src/ui/mod.rs:106`):

```text
layout_engine.run(...)         // <-- text.measure() called here
cascades.rebuild(...)
input.end_frame(...)
tree.compute_hashes()
damage.compute(...)            // <-- dirty set produced here
```

So the dirty set for **frame N** isn't known until after frame N's
layout has already run. Reuse must be driven by the dirty set from
**frame N-1** carried into frame N. That means we keep a per-frame
`WidgetId → ShapedText` snapshot from the previous frame, and at
the start of layout we ask: "is this widget's hash unchanged, was
its constraint unchanged, and do I have its prior `ShapedText`?"

## Design

### Two layers, decoupled

**Layer A — per-WidgetId reuse of `MeasureResult`.** Skip the
`text.measure()` call entirely for unchanged Text nodes. Store
last frame's `(authoring_hash, available_w_quantized) →
MeasureResult` keyed by `WidgetId`. On a hit, copy the result
into `LayoutResult.text_shapes` without touching the
`TextMeasurer`. The `TextCacheKey` inside the result still points
into `CosmicMeasure.cache`, so the renderer's `BufferLookup` works
unchanged.

**Layer B — eviction of `CosmicMeasure.cache`.** Track which
`TextCacheKey`s each live `WidgetId` references. When a `WidgetId`
disappears (already detected by `Damage::compute` via the surplus
sweep, `src/ui/damage/mod.rs:136`), drop the cache entries that no
remaining live widget references.

Layer A removes per-frame CPU on clean nodes. Layer B caps memory.
Either can ship without the other. Layer A is the headline win and
should ship first.

### Layer A in detail

**New state on `LayoutEngine` (or a sibling holder, TBD):**

```rust
struct TextReuseEntry {
    authoring_hash: u64,    // Tree.hashes[i] from prior frame
    avail_w_q: u32,         // quantized available_w that produced `result`
    result: ShapedText,     // measured + key
    intrinsic_min: f32,     // for intrinsic queries
    unbounded_size: Size,   // also for intrinsic queries
}

text_reuse: FxHashMap<WidgetId, TextReuseEntry>
```

**Hot path in `shape_text` (`src/layout/mod.rs:273`):**

```rust
let wid = tree.widget_ids[node.index()];
let curr_hash = tree.hashes[node.index()];   // computed before layout — see below
let avail_q = quantize_avail(available_w);

if let Some(entry) = self.text_reuse.get(&wid) {
    if entry.authoring_hash == curr_hash && entry.avail_w_q == avail_q {
        self.result.set_text_shape(node, entry.result);
        return entry.result.measured;
    }
}

// fall through to current code path (text.measure, then store)
let result = ...;
self.text_reuse.insert(wid, TextReuseEntry { ... });
result.measured
```

**Sequencing constraint.** `Tree::compute_hashes` currently runs
*after* layout in `end_frame`. To check "did the hash change?"
during layout, we need either:

- **Option 1** — move `compute_hashes` to *before* layout in
  `end_frame`. Hashes don't depend on layout output, so this is
  pure reordering. Existing damage-compute order is unaffected
  (it reads hashes either way). This is the cleanest fix.
- **Option 2** — incrementally hash per node inside
  `Tree::push_node`. More invasive, complicates the recorder hot
  path; rejected.

**Take Option 1.** Order becomes: record → `compute_hashes` →
layout (with reuse) → cascades → input → damage.

**Intrinsic-query path (`src/layout/intrinsic.rs:122`).** Same
treatment: if the reuse entry exists and `(hash, avail=None)`
matches, return the cached `intrinsic_min` / `unbounded_size`
without calling `text.measure`. Today this path always calls
`text.measure(..., None)`, which is also just a cache hit on
`CosmicMeasure.cache`, but the same string-hash + dispatch cost
applies. Caching the unbounded result on the `TextReuseEntry`
removes it.

**Eviction within Layer A.** Mirror `Damage.prev`'s
removed-widget sweep — at end of frame (or start of next), drop
`text_reuse` entries whose `WidgetId` isn't in `Ui.seen_ids`. Same
shape as `src/ui/damage/mod.rs:136`. Cheap because most frames
don't add/remove widgets.

**Tests.**

- Same widget, identical text → second frame's `text.measure` is
  *not* called (instrument with a counter on `TextMeasurer`).
- Same widget, text changed → reshape happens, new
  `TextReuseEntry` overwrites old.
- Same widget, parent shrinks below `intrinsic_min` → wrap path
  recomputes; reuse entry refreshed with new `avail_w_q`.
- Widget disappears → `text_reuse` entry evicted within one frame
  of disappearance.
- ID reuse (same `WidgetId`, different content) → hash mismatch,
  reshape happens.

### Layer B in detail (deferrable)

**Refcount `TextCacheKey` by live `WidgetId`.**

```rust
// On CosmicMeasure (or a sibling tracker on Ui):
key_users: FxHashMap<TextCacheKey, FxHashSet<WidgetId>>
```

When `Layer A` records or refreshes a `TextReuseEntry` for `wid`:

- Add `wid` to `key_users[new_key]`.
- If a previous entry existed with a different key, remove `wid`
  from `key_users[old_key]`; if the set is empty, drop the
  `CosmicMeasure.cache` entry too.

When `wid` disappears (Damage sweep): walk every key it owned and
remove `wid` from `key_users`; drop empty sets' cache entries.

**Why deferrable.** The leak is bounded by "distinct strings ever
shown × shaping size ≈ a few KB each." Real apps will hit Layer A's
CPU win long before they hit a memory ceiling. Ship Layer A,
measure, then decide.

### What the doc snippet got slightly wrong

`damage-rendering.md` says: *"Needs a `WidgetId → ShapedBuffer`
cache parallel to `Damage.prev`."*

We don't actually need a `WidgetId → ShapedBuffer` cache — the
`ShapedBuffer` is already cached by content in `CosmicMeasure`.
What we need is a `WidgetId → MeasureResult` cache so we can skip
the dispatch + string-hash on the hot layout path. Update the
damage doc when this lands.

## Plan / steps

Each step is a self-contained PR-able change with tests.

1. **Reorder hashes before layout in `end_frame`.**
   Move `tree.compute_hashes()` above `layout_engine.run(...)` in
   `src/ui/mod.rs:106`. Damage still reads `tree.hashes` after, so
   no compute order changes. Run full test suite — should be a
   no-op.

2. **Add reuse map + happy path in `shape_text`.**
   Introduce `text_reuse: FxHashMap<WidgetId, TextReuseEntry>` on
   `LayoutEngine`. Wire the hash+avail-width check into
   `LayoutEngine::shape_text`. New test:
   `text_reshape_skipped_when_unchanged` using a
   `text.measure` call counter (add `measure_calls: usize` to
   `TextMeasurer` behind `#[cfg(test)]`? — or expose a
   `CosmicMeasure::measure_count` accessor).

3. **Extend reuse to intrinsic queries.**
   Cache `unbounded_size` and `intrinsic_min` on
   `TextReuseEntry`; reuse from `intrinsic.rs:122`. Test:
   intrinsic queries on a clean Text node don't call
   `text.measure`.

4. **Eviction of `text_reuse` for removed widgets.**
   At end of `end_frame`, sweep `text_reuse` against
   `Ui.seen_ids` — same shape as `Damage`'s sweep. Test: removed
   widget's reuse entry is gone next frame; long-running churn
   doesn't grow the map unbounded.

5. **Bench / showcase.**
   Add a benchmark with N=100 static text labels rendered for K
   frames. Compare measure-call count and wallclock before/after.
   If feasible, add a showcase tab toggling reuse on/off so the
   win is visible to the eye (frame time HUD).

6. **(Optional, deferred) Layer B — `CosmicMeasure.cache` eviction.**
   Refcount `TextCacheKey` by live `WidgetId`; sweep on widget
   disappearance. Only if a real workload shows the unbounded
   cache as a problem — write a stress test first
   (`cache_grows_until_capped` for the new path,
   `cache_grows_unbounded_today` to demonstrate the leak).

7. **Doc updates.**
   Fix the "Wanted (identity-based reuse)" preamble in
   `damage-rendering.md` (the dirty *set* already exists; the
   pending work is consumers). Tighten the "Skip cosmic-text
   reshape" bullet to reference what actually shipped.

## Risks / things to watch

- **`avail_w` quantization granularity.** Reuse fires only when
  `avail_w_q` matches exactly. Animated parent width (smooth
  resize) will defeat reuse on every frame. Quantize to
  ~0.5 logical px? Or only quantize when wrap is actually engaged
  (i.e. when `avail < unbounded.size.w`); for non-wrapping text the
  available width doesn't change the result and can be ignored.
  **Lean toward the second** — it sidesteps the granularity tradeoff.

- **`TextCacheKey` identity across frames.** Layer A stores the
  `key` from a prior frame's measure. If `CosmicMeasure.cache`
  ever evicted that key (Layer B), the renderer's
  `BufferLookup::get` would return `None` and the run would silently
  drop. Layer B's eviction must respect outstanding `text_reuse`
  references — that's exactly what the refcount provides.
  Until Layer B ships, this is a non-issue (cache never evicts).

- **`#[track_caller]` interaction.** Reuse hinges on stable
  `WidgetId`s. The CLAUDE.md note about `#[track_caller]` not
  propagating through closures already says: "give widgets in
  helpers explicit ids." Same story here — wrong IDs would just
  miss reuse, not corrupt anything, but call it out in the test
  for `Text::with_id`.

- **Hash ordering within a frame.** Since hashes now compute
  before layout but recording is the only producer of hash
  inputs, the order is fine. But any future code that mutates
  `Tree` mid-layout would silently desync. Add an assertion in
  debug builds: `tree.hashes.len() == tree.node_count()` at
  layout entry.

## Acceptance bar

- ✅ Steady-state showcase: zero `text.measure` calls per frame
  for unchanged Text nodes (verify via counter).
- ✅ All existing tests pass; new tests pin reuse behaviour and
  invalidation.
- ✅ Bench shows measurable CPU drop on the static-labels
  workload (target: ≥50% reduction in layout-pass time on a
  100-label scene).
- ✅ Removed widgets don't leak `text_reuse` entries.
- ⏳ (Layer B) `CosmicMeasure.cache` size stays bounded under a
  string-churn stress test.
