# Hot-path redesign notes

Audit + redesign brainstorm for the per-frame pipeline. Goal: identify
data-structure / access-pattern changes that would improve IPC, cache
behavior, or raw CPU time. Pre-commitment exploration — none of these
are decided. Two passes happened: a first survey (now mostly disproved
by inspection) and a second pass focused on cross-frame caching. This
doc is the second-pass conclusion.

## Baseline numbers

`PALANTIR_BENCH_MODE=cpu cargo bench --bench frame --features internals`

| Arm | Mean (latest) |
|---|---|
| `frame/cached_cpu` | 95.74 µs |
| `frame/partial_cpu` | 137.31 µs |
| `frame/resizing_cpu` | **1.385 ms** |

Bench variance is ~2–4 µs frame-to-frame on the cached/partial arms;
treat anything under 5% as noise on those, and ~5% on resizing.

`cached_cpu` is the steady-state floor (record + measure-cache hits +
arrange + cascade + encode + compose, no real damage). `partial_cpu`
adds one mutating counter text per iter → real damage rect + paint.
`resizing_cpu` busts the MeasureCache each iter, so the full measure
pass runs — **14.7× the cached cost**. Resizing is the only arm that
takes more than 200 µs.

## What's actually expensive (verified by reading the code)

| Pass | Cost rough | Notable |
|---|---|---|
| Record | included in walk | rebuilds tree every frame |
| Measure | ~35% CPU | dominates on cache-miss frames; text shaping is most of the cost |
| Arrange | small | separate tree walk |
| Cascade | small | separate tree walk |
| Encode | <1% post-cache-removal | per-shape `match` dispatch |
| Compose | <1% | per-cmd `match` dispatch |
| `compute_hashes` | ~5–10 µs | 6th tree walk in `post_record` |

**Two distinct leverage points:**
- **Resizing (1.41 ms)** — the entire bottleneck is text re-shaping when
  wrap width changes. Improving this is the only way to make the
  user-perceptible "resize the window" experience snappier.
- **Cached floor (96 µs)** — already imperceptible, but additional
  headroom enables 240 Hz comfortably or supports much heavier UIs.
  Lever: cache more aggressively across frames; eliminate redundant
  tree walks.

## Profile findings (4th pass — `perf record` on `resizing_cpu`)

**Major reframe: the "cosmic-text dominates resize" hypothesis was
wrong.** A 10-second `perf record -F 3000 --call-graph dwarf` of the
`resizing_cpu` bench arm (11K samples on cpu_core) shows:

**Self-% in palantir code (Rust binary, 78% of total cycles):**

| Function | Self % | Estimated µs/frame at 1.4 ms |
|---|---|---|
| `cascade::CascadesEngine::run` | 7.12% | ~100 µs |
| `composer::Composer::compose` | 5.43% | ~76 µs |
| `layoutengine::LayoutEngine::measure` | 5.42% | ~76 µs |
| `encoder::encode_node` | 3.70% | ~52 µs |
| `layout::intrinsic::compute` | 3.03% | ~42 µs |
| `forest::Forest::open_node` | 2.69% | ~38 µs |
| `Tree::post_record` (compute_hashes) | 2.01% | ~28 µs |
| `damage::DamageEngine::compute` | 1.88% | ~26 µs |
| `composer::quad_forces_flush` | 1.73% | ~24 µs |
| `text_backend::encode::try_emit_cached` (glyphon) | 1.62% | ~23 µs |
| `frame_arena::lower_background` | 1.43% | ~20 µs |
| `Shapes::add` | 1.36% | ~19 µs |
| `layoutengine::arrange` | 1.31% | ~18 µs |
| `soa_rs::Soa::push` | 1.26% | ~18 µs |
| `text::shape_unbounded` | 1.11% | ~16 µs |

**Distribution by DSO (entire process):**
- `frame-*` (palantir code) — 77.6%
- `libnvidia-eglcore` (driver via wgpu) — 11.6%
- `libc.so.6` (malloc/free) — 10.0%

### Implications

**1. Text shaping is NOT the resize bottleneck.**
Combined `shape_unbounded` + `text_backend::encode` + glyphon = ~3–4%
of resize cost. Idea A's predicted savings drop from "hundreds of µs"
to **~30–50 µs**. Still worth doing but no longer the single most
leveraged item.

**2. No single hotspot — the pipeline is evenly distributed.**
The top 5 functions (cascade, composer, measure, encode, intrinsic)
account for ~25% combined. Halving any single one saves only ~5% of
resize cost (~70 µs).

**3. `libc` is 10% (mostly malloc/free) — and the hot-loop chunk is
text shaping.**

Filtering perf samples by stack content: most libc hits are warmup
(font loading via `fontdb`, naga shader compile, wgpu setup,
`dlopen`). The actual hot-loop allocation site is in
`src/text/cosmic.rs:236-289`:

```rust
let key = key_for(text, ..., max_width_px, ...);  // includes max_w_q
if let Some(entry) = self.cache.get(&key) { return ... }
// Cache miss path:
let mut buffer = Buffer::new(&mut self.font_system, metrics);
buffer.set_size(max_width_px, None);
buffer.set_text(text, ...);
buffer.shape_until_scroll(...);
self.cache.insert(key, CacheEntry { buffer, ... });
```

The cache key includes `max_w_q` (quantized wrap width
— `src/text/mod.rs:580`). On resize, the wrap width changes every
frame → cache miss → **fresh `Buffer` allocated per text shape per
frame**. With ~500 text shapes in the fixture, that's ~500 fresh
`Buffer` allocations per resize iteration. Each `Buffer` contains a
`Vec<BufferLine>` plus per-line shape buffers and layout runs.

**This refines Idea A:** the lever is *allocation churn*, not CPU.
Cosmic-text's `Buffer::set_size` + `shape_until_scroll` should re-flow
in place when only width changes (cached `BufferLine`s reuse cached
glyph shaping); need to verify with the cosmic-text source. If it
works, the fix is small:
1. Key the cache by `(text, font, line_height, family, halign)` —
   drop `max_w_q` from the key.
2. Store `current_max_w` on the cache entry.
3. On width-change hit, mutate the entry: `buffer.set_size(new_w)` +
   `buffer.shape_until_scroll(false)`. No allocation.

**4. Cascade (7%) is the single biggest palantir hotspot.**
`CascadeCache` exists but **misses on resize** because `rect_q` (the
quantized root rect) changes every frame. The cache that's "~99%
effective on cached/partial" is mostly dead weight on resize.

**5. `compute_hashes` is 2% (28 µs) on resize.**
Pick 1 (fold into record) saves ~half of that → ~15 µs. On a 1.4 ms
budget that's 1%. Still useful but not transformative.

**6. wgpu/Vulkan CPU-side work is 12%.**
Out of palantir's direct control. The composer's CPU-side cmd-buffer
encoding (`compose` + `quad_forces_flush` = 7.2%) feeds wgpu's CPU
queue submission. Reducing per-frame quad count would reduce both.

### Revised resize attack surface

No silver bullet. Each pass costs roughly the same. The path to a
meaningfully faster resize requires either:

- **A. Across-the-board work reduction via aggressive caching that
  survives `rect_q` changes.** The Cascade cache key includes `rect_q`
  — if cascade output could be expressed *relative* to the root rect
  (a delta cache), resize would preserve hits. Big architecture change.
- **B. Eliminate the libc 10%.** Find per-frame allocations in the hot
  loop. Cheap if discoverable.
- **C. Compress the entire pipeline by ~10% via walk fusion** (Pick 1
  + Pick 2 + Pick 3 collectively). Saves walk overhead across all
  passes.

## Validation findings (3rd pass — read the code)

Key discoveries that re-shaped the ranking:

**1. Cascade already has its own cross-frame cache.**
`src/ui/cascade/cache.rs` (`CascadeCache`) keyed by
`(WidgetId, subtree_hash, parent_prefix, rect_q)`. Per its own
`docs/roadmap/cascade-cache.md`, ~99% steady-state coverage on
cached/partial workloads. On hit, blits the entire subtree's
`Cascade`, `subtree_paint_rect`, `EntryRow`, and paint span columns
verbatim. **Implication:**
- **Pick 2 (fuse arrange + cascade) is heavily invalidated** — cascade
  is already mostly skipped via cache. Fusing into arrange would either
  lose the cache short-circuit or require restructuring the cache to
  work mid-arrange.
- **Idea E (drop `Cascade.paint_rect`) is complicated** — `paint_rect`
  feeds `quantize_rect` which is part of the `CascadeCache` `ProbeKey`.
  Removing the column requires rebuilding cache validity.

**2. `compute_hashes` structure confirmed.**
`src/forest/tree/mod.rs:317-401`. Reverse-pre-order walk; for each
node hashes layout + attrs + extras + chrome + per-shape hashes + grid
defs. Then a second inner loop walks direct children to fold their
already-finalized `subtree_hash`. **Pick 1 (fold into record) still
viable** — all inputs available at `close_node` time. The child-walk
shape (`while next < subtree_end { next = ends[next]; }`) matches
exactly the child-close ordering, so an ordered hasher push at child
`close_node` reproduces it.

**3. Encoder dispatch overhead is small.**
`emit_one_shape` is called per shape; `paint_anims.sample` is already
fast-path (empty `by_shape` → `Vec::get` returns None on first probe).
The `matches!(shape, ShapeRecord::Text { .. })` after each emit is
~1 cycle per shape — sub-µs total. Confirms the doc's prior call to
defer dispatch-related ideas.

**4. Text shaping is genuinely the resize cost.**
`shape_wrap` in `src/text/mod.rs:245` dispatches to `cosmic-text`
shaping each time `target_q` changes. Reuse cache hits on content
hash + identical target, but resize changes target every frame. No
retained `cosmic_text::Buffer` per node. **Idea A is the right
target** — but the cosmic-text reshape API needs to be confirmed (no
spike yet).

## Hot-path structural survey

A thorough file-level audit of per-frame data structures and access
patterns is preserved at `.tmp/audit.md` (gitignored). Covers SoA
layout, shape buffer, cascade rows, encoder/cmd buffer, backend,
measure cache, damage, hashing, allocation surface, hot-loop tightness,
benchmark fixture composition. Regenerate with the Explore agent or by
reading cited `file:line` ranges directly.

---

# Ideas ruled out after inspection

These were proposed in the first pass and confirmed dead by reading
the actual code. Documented here so they don't get re-proposed.

## ❌ Sparse paint-anim sample driver

**Idea:** invert `tree.paint_anims.sample(shape_idx, now)` from per-shape
lookup to per-anim iteration.

**Why dead:** `sample` is **already** O(1) `Vec::get` against a sparse
`by_shape: Vec<u16>` column (`forest/tree/paint_anims.rs:91-99`). For
the common case (no anims), `by_shape` is empty and `get` returns `None`
without touching memory. There's no hash lookup to remove.

## ❌ Slot tables for `WidgetIdMap`

**Idea:** replace `WidgetIdMap<V>` with open-addressed slot table.

**Why dead:** `WidgetIdMap` already uses an `IdHasher` passthrough —
`WidgetId` is a precomputed 64-bit FxHash, so the hashmap is
`HashMap<WidgetId, V, BuildHasherDefault<IdHasher>>`
(`forest/seen_ids.rs:38-61`). hashbrown's lookup is already SIMD bucket
probing on a u64 key. Hand-rolled slot table buys 0%.

## ⏸ Tag-dispatch table for shape match

**Idea:** replace the encoder's 7-arm `match shape { ... }` with a
function-pointer table.

**Why deferred:** shape kind distribution in any real frame is heavily
skewed (fixture is ~95% Text + RoundedRect). Modern branch predictors
nail biased patterns; indirect function-pointer dispatch is usually a
**regression** on a 7-entry switch. The body work (text shape lookup,
brush dispatch) dwarfs the discriminant load.

## ⏸ Column-per-shape-kind storage

**Idea:** split `Tree.shapes.records: Vec<ShapeRecord>` into per-kind
vectors with a per-node interleave stream.

**Why deferred:** same reasoning as the dispatch table — the match isn't
the bottleneck. Structural cleanup is nice but the speed win is
doubtful without a workload showing the dispatch as hot. Revisit only
if profiling shows >5% of frame in encoder branch mispredicts.

## ⏸ Leaf-skip `MeasureCache`

**Idea:** skip the cache lookup for leaf nodes since leaf measure is
cheap.

**Why deferred:** `try_lookup` (`layout/cache/mod.rs:246-267`) is one
hashbrown probe + 16-byte compare — already cheap. Plus leaves *with
text* use the cache to skip text shaping; skipping the lookup loses the
cache hit, not just the probe cost. Net likely negative.

## ⏸ Split `subtree_hash` into `layout_hash` + `paint_hash`

**Idea:** partition the hash so paint-only mutations preserve measure
cache hits.

**Why deferred:** real-world win on hover-heavy real apps; current
fixture doesn't trip it. Don't optimize without a bench arm that
demonstrates the workload. Re-prioritize after building a
`frame/hover_animation` arm.

## ❌ Subtree hash incremental during record

**Idea:** compute `compute_hashes` work during `open_node`/`close_node`
instead of as a post-record walk.

**Why kept** — see "Pick 1" below. Re-examined after the second pass
and still looks viable.

---

# Live ideas, ranked by leverage on observable bench arms

The reframe from the second pass: lean **harder** into cross-frame
caching. MeasureCache works; extend the same pattern to other
per-frame work.

## ★★★ A — Retained cosmic-text `Buffer` per text node

**The only idea here that meaningfully attacks `resizing_cpu`.**

**Today:** `TextShaper.reuse` (`text/mod.rs:141`) caches results by
`(WidgetId, ordinal)` validated by content hash. On resize, content is
unchanged but `target_q` (quantized wrap width) changes. `shape_wrap`
(`text/mod.rs:245`) appears to dispatch a fresh shaping each time the
wrap target shifts. With ~500 text shapes × ~2–3 µs each on cosmic-text
re-shape, that accounts for ~1 ms of the 1.41 ms `resizing_cpu` budget.

**Proposal:** retain the `cosmic_text::Buffer` per `(WidgetId, ordinal)`
slot, not just the measurement output. On resize:
- Same content hash → keep the `Buffer`.
- Different `target_q` → call `Buffer::set_size_opt(new_width, None)` +
  `shape_until_scroll(false)`. cosmic-text reuses cached `BufferLine`s
  internally — only line break positions and visual line layout change.

**Wins:**
- `resizing_cpu` plausibly halves (or better). **Hundreds of µs saved
  on the only arm that takes more than 200 µs.**
- Idle / non-resize frames: identical to today (existing reuse cache
  hits).

**Cost:** moderate. Storage: one `Buffer` per cached text node (~100 B
+ shaped runs). 500 text nodes → ~50 KB retained per layer. Eviction
piggybacks on `TextShaper::reuse`'s existing sweep on `removed`. Need
to verify cosmic-text's incremental re-shape behaviour matches the
assumption (no spike has been done yet).

**Confidence:** moderate. The assumption about cosmic-text's
incremental re-shape is the load-bearing claim. Spike validates or
kills this in <1 day.

**File pins:** `src/text/mod.rs:141, 245-330`, `tmp/cosmic-text/src/buffer.rs`
(check `set_size_opt` semantics).

## ★★ B — Retain shape lowering across frames

**Today:** `Shapes::add` runs per shape every frame:
1. Hashes authoring inputs (`forest/shapes/hash.rs`).
2. Lowers brush/stroke/gradient (`record.rs::lower_brush`).
3. Memcpys polyline points / mesh verts / text bytes into `FrameArena`.
4. Pushes `ShapeRecord` + hash.

With ~500 shapes/frame at fixture, this is ~5–10 µs of pure work.

**Key observation:** if a node's `subtree_hash` matches last frame's,
**every shape in that subtree is byte-identical**. MeasureCache already
detects this for `desired` / `text_shapes`; it doesn't extend to
shapes.

**Proposal:** carry `Tree.shapes` ranges across frames keyed by
`(WidgetId, subtree_hash)`. At `close_node`, if the just-closed
subtree's hash matches a cached one, splice in the cached `ShapeRecord`
range verbatim (memcpy). Skip lowering entirely.

**Wins:**
- `cached_cpu`: ~5–10 µs from elimination of lowering on stable
  subtrees.
- `partial_cpu`: counter-text mutation invalidates one subtree; other
  ~499 shapes splice from cache.

**Cost:** **heavy.** `FrameArena` becomes two-layer (retained
per-cache-hit subtree + fresh per-uncached). `ShapeRecord` payload
pointers into `FrameArena` are frame-scoped today — they need to
survive across frames either via retained per-subtree arenas or by
copying into a per-tree retained store. Real eviction logic needs to
match the existing `removed`-sweep pattern.

**Confidence:** medium-low. The win is real but the implementation
touches a lot of frame-arena invariants. Worth it only after A lands.

**File pins:** `src/forest/shapes/mod.rs`, `src/common/frame_arena.rs`,
`src/layout/cache/mod.rs` (extend `ArenaSnapshot`).

## ★★ C — Cache arranged rects in `MeasureCache`

**Today:** MeasureCache restores `desired` on hit; `arrange` still runs
as a full pre-order walk computing each child's final `Rect`. For a
cache-hit subtree, arranged rects are a pure function of `parent rect +
cached layout`. With the same parent rect (common steady-state),
arranged rects are byte-identical to last frame's.

**Proposal:** extend `ArenaSnapshot` to include `relative_rects:
&[Rect]` — each child's offset relative to the subtree root, captured
at arrange time. On cache hit:
1. Restore `desired` (today).
2. Translate cached `relative_rects` by the new root rect's `min`;
   splat into `layout.rect[i]`.
3. Skip arrange recursion for the entire subtree.

If parent rect happens to be identical to last frame's, the translation
is identity and it's a pure memcpy.

**Wins:**
- `cached_cpu`: arrange becomes a sequence of memcpys at MeasureCache-
  hit roots. Currently ~5–10 µs; could drop to ~1 µs.

**Cost:** moderate. Snapshot grows by ~16 B × ~500 nodes = ~8 KB
retained. Arrange's recursion gains a cache-check entry-point.

**Confidence:** medium. The logic is conceptually parallel to the
existing `desired` cache. The wrinkle is detecting when parent rect
changes break the identity (still cheap: compare last frame's parent
rect to this frame's at the hit point).

**File pins:** `src/layout/cache/mod.rs`, `src/layout/layoutengine.rs:592`
(arrange entry).

## ★★ Pick 1 — Fold `compute_hashes` into record

**Today:** `Tree::compute_hashes` (`forest/tree/mod.rs:317-410`) is a
reverse-pre-order walk over every node in `post_record`. For each node
it hashes `LayoutCore` + `NodeFlags` + extras + chrome hash + per-shape
hashes + grid defs. With ~800 nodes, walk overhead alone is ~5–10 µs
(~5–10% of cached_cpu).

**Key observation:** **all inputs are already known at `close_node`
time** — that's literally when the last child has finished and the
node is about to be sealed.

**Proposal:** at `open_node`, push a fresh `Hasher` onto a per-frame
stack. At `close_node`, write the node's layout/attrs/extras into the
hasher, fold in children's already-finalized `subtree_hash`es in
record order, finalize, store. Delete the post-record walk.

**Wins:**
- ~5–10 µs (~7% of `cached_cpu`).

**Cost:** small. `forest/mod.rs::open_node/close_node` grow a hasher
stack. Child-hash ordering needs care — push children's `subtree_hash`
into parent's hasher at child `close_node` time, in record order.

**Confidence:** high. No new data flow; inputs are demonstrably
available at the right moment.

**File pins:** delete `src/forest/tree/mod.rs:317-410`,
`src/forest/mod.rs::open_node/close_node`.

## ★ D — Interleave leaf measure into record

**Today:** record builds the tree; measure walks it. A leaf's measure
is a pure function of its own layout — no children to wait for, no
parent context needed. ~70% of nodes in the fixture are leaves.

**Proposal:** at `close_node`, if `LayoutCore.mode` is leaf-trivial
(Text, Fixed RoundedRect, etc.), compute `desired[i]` inline. Measure
pass becomes a walk over only non-leaf nodes — the panel/grid/stack
containers that need bottom-up resolution.

**Wins:**
- All arms — measure-phase cost drops by ~70% of nodes; record-phase
  cost rises slightly. Net ~3–5 µs on `cached_cpu`.

**Cost:** small-to-moderate. `open_node`/`close_node` gain leaf
detection + inline measure call. Measure dispatch needs to skip
pre-measured leaves.

**Confidence:** medium. The save is small enough that the win can
easily be eaten by record-phase cache pressure. Needs benching.

**File pins:** `src/forest/mod.rs::close_node`, `src/layout/layoutengine.rs`.

## ⏸ E — Drop `Cascade.paint_rect` (COMPLICATED after validation)

**Original claim:** `Cascade.paint_rect` (16 B/node) is read by
encoder for cull; the encoder could accumulate inline.

**Validation finding:** `paint_rect` (via `quantize_rect`) is part of
the `CascadeCache::ProbeKey` (`src/ui/cascade/cache.rs:39-46`).
Removing the column means redesigning the cache key. Not a free
extraction.

**Status:** **defer.** Possible eventually as part of a broader
cascade-cache rework, but not the small free win the first pass
claimed. The `~1–2 µs` win was already marginal; not worth the cache
redesign.

## ⏸ Pick 2 — Fuse arrange + cascade (DOWNGRADED after validation)

**Original claim:** arrange + cascade are both full pre-order walks
reading overlapping columns; fuse to save ~5–8 µs walk overhead.

**Validation finding:** **cascade has its own cross-frame cache**
(`src/ui/cascade/cache.rs`) with ~99% steady-state coverage on
cached/partial workloads. On hit, an entire subtree's cascade walk is
**already skipped** via blit. The "full walk every frame" premise is
false.

**Revised assessment:**
- The cache miss path (resizing, fresh content) does walk both arrange
  and cascade. There, fusion still saves ~5 µs.
- But the cached/partial workloads — the ones where saving µs matters
  for steady-state floor — see most of cascade short-circuited
  already.
- Fusing cascade into arrange would either lose the cache short-
  circuit (regression on the common case) or require redesigning
  `CascadeCache` to operate mid-arrange (heavy).

**Status:** **defer.** Only revisit if profiling shows cascade walks
dominating a workload where the cache *can't* hit (genuinely-dynamic
trees).

## ◆ Pick 3 — Drop the cmd buffer; fuse encode + compose

**Today:** encoder produces `RenderCmdBuffer` (SoA: kinds + starts +
payloads). Composer reads it, scales to physical px, snaps, groups by
scissor, batches text, emits `RenderBuffer.quads`. For fixture's ~1300
cmds × ~55 B avg, that's **~70 KB written then ~70 KB read back** of
pure throwaway memory traffic.

**Proposal:** replace `Encoder → CmdBuffer → Composer → RenderBuffer`
with `Encoder → RenderBuffer` + a small `ComposeState` (clip stack,
scissor groups, transform stack, text scene) threaded through the
walk.

**Wins:**
- ~140 KB memory bandwidth/frame eliminated.
- One walk instead of two. ~5–15 µs.
- One enum dispatch layer removed (the `CmdKind` match in composer).
- Whole module deleted (`src/renderer/frontend/cmd_buffer/`).

**Cost:** **large.** Cmd buffer is the documented "single canonical
correctness gate" for noop emit; policy needs revising. Text batching
needs full-frame visibility — `TextScene` carries it. Week of work +
test thrash.

**Confidence:** moderate. The two passes duplicate ~70% of the walk
shape. Worth doing only after A + B + C show that walk-fusion is
actually the dominant lever — until then it's invasive without
proof.

**File pins:** `src/renderer/frontend/encoder/mod.rs`, all of
`src/renderer/frontend/cmd_buffer/`, all of
`src/renderer/frontend/composer/`.

---

# Re-prioritized recommendation

**Post-profile re-ranking** — A downgraded, allocation hunting elevated:

| Rank | Idea | Bench arm hit | Effort | Confidence |
|---|---|---|---|---|
| **1** | **Hunt per-frame allocations** (libc = 10% of resize) | resize (~tens of µs) | small per fix | high |
| **2** | **Pick 1 — fold `compute_hashes` into record** | all (~15 µs resize, ~5–10 µs cached) | small-mod | high |
| **3** | **A — retained cosmic-text `Buffer`** | resize (~30–50 µs) | moderate | spike first |
| **4** | **Cascade delta-cache surviving `rect_q` changes** | resize (~80 µs) | heavy | high upside, hard |
| **5** | **C — cache arranged rects in MeasureCache** | `cached_cpu` (~5–10 µs) | moderate | medium |
| 6 | **D — interleave leaf measure into record** | all (~3–5 µs) | small | medium |
| 7 | **B — retain shape lowering** | `cached_cpu` + `partial_cpu` (~5–10 µs) | heavy | medium-low |
| 8 | **Pick 3 — drop cmd buffer** | all (~5–15 µs) | heavy | moderate |
| — | ~~E (drop `Cascade.paint_rect`)~~ | ~~all~~ | — | killed by cache key |
| — | ~~Pick 2 (fuse arrange + cascade)~~ | ~~all~~ | — | killed by cascade cache |

## The big reframe

**A (retained cosmic-text Buffer) is the single most leveraged item in
this whole document.** Resizing takes 1.41 ms; nothing else takes more
than 200 µs. Every other idea nibbles at a sub-100-µs floor that's
already imperceptible at 60 Hz.

The first pass over-weighted the cached/partial arms because they're
where I could see micro-structure. Resizing is harder to break apart
without profiling, but the magnitude difference makes it the obvious
prize.

## Suggested execution order

1. **Spike A.** Confirm that `cosmic_text::Buffer::set_size_opt` +
   `shape_until_scroll` is meaningfully cheaper than rebuilding a
   fresh `Buffer`. Half-day experiment. If it pays, productize.
2. **Pick 1** (fold `compute_hashes`) — small, high confidence,
   proves the walk-fusion thesis.
3. **E** (drop `Cascade.paint_rect`) — falls out cleanly, free L1
   relief.
4. **D** (interleave leaf measure) — bench-gated; revert if it
   regresses record cost.
5. **C** (cached arranged rects) — natural extension of MeasureCache.
6. **Pick 2** (fuse arrange + cascade) — clean structural win.
7. **B** (retain shape lowering) — only if 1–6 didn't get steady-state
   below ~70 µs; heavy.
8. **Pick 3** (drop cmd buffer) — only after everything above; biggest
   surgery, lowest marginal return.

Before doing **B**, **C**, or anything paint-anim-related: add a
`frame/text_heavy` and a `frame/hover_animation` bench arm. Those
moves' payoffs don't show on the current fixture.

---

# Experiment log

Concrete things tried + outcomes. Useful for future trial-and-error
calibration.

## E1 — Hoist `Shape::is_noop` from `Shapes::add` to `Ui::add_shape`

**Hypothesis:** filter noop shapes one indirection earlier.

**Outcome:** cached_cpu +1.3% (within noise), partial/resizing
unchanged. Reverted. Bench fixture has effectively zero noop shapes in
steady state, so the hoist's early-exit never fires on the hot path.

## E2 — `chrome: tree.chrome(id).copied()` → `tree.chrome(id)`

**Hypothesis:** avoid the 48-byte `ChromeRow` copy per chromed node;
field uses are `Copy`-scalar reads that work fine through `&ChromeRow`.

**Outcome:** within bench variance (±2% spread on identical code makes
2-µs changes invisible). Kept as a cleanup — semantically the right
thing.

## E3 — `text_ordinal: u32` → `&mut u32` parameter on `emit_one_shape`

**Hypothesis:** fold the post-emit `matches!(shape, ShapeRecord::Text)`
check into the function body, eliminating one match per shape.

**Outcome:** **partial_cpu regressed +4.3% (significant), reverted.**
The mutable reference parameter appears to defeat register allocation
for `text_ordinal` across the call site. The `matches!` cost was
already trivial (~1 cycle) and clearly not worth disturbing the
optimizer.

**Lesson:** micro-changes that *touch the hot loop's call signatures*
can regress more than they save. Future small-experiment candidates
should stick to data-flow changes that the compiler can clearly
absorb.

## Bench variance calibration

Across ~7 runs of the unchanged code on the current machine:
- `cached_cpu`: 95.7 – 98.4 µs (~3% spread)
- `partial_cpu`: 134 – 143 µs (~7% spread)
- `resizing_cpu`: 1.38 – 1.41 ms (~2% spread)

**Conclusion: measurable wins need to be >5% on cached/resizing,
>8% on partial.** Changes smaller than that vanish in run-to-run
noise. Any future "small experiment" expecting <5 µs savings on
cached_cpu is below the detection floor — chase the bigger ideas (A,
Pick 1, C) where the predicted savings clear the noise floor.

# Status

Brainstorm + small experiments. No production-affecting changes
committed yet. Chrome borrow cleanup applied (E2) — kept as
semantic-clarity win even though too small to measure.

Baseline bench committed to `benches/results/asus-rog-arch.txt` under
the note "baseline before small redesign experiments" — see the file
for the full machine-history strip.

Especially: do not start on **A** without confirming that resizing's
cost is in cosmic-text shaping (perf record + flamegraph on
`resizing_cpu`). The 1 ms estimate is back-of-the-envelope. Baseline bench committed to
`benches/results/asus-rog-arch.txt` under the note "baseline before
is_noop early-exit in Ui::add_shape" — that prior hoist experiment is
documented in this branch's history but was reverted because it
showed no measurable change on the current fixture (the workload has
~zero noop shapes in steady state). The pattern repeats here: re-
validate against a workload that actually exercises the targeted code
path before committing.

Especially: do not start on **A** without confirming that resizing's
cost is in cosmic-text shaping (perf record + flamegraph on
`resizing_cpu`). The 1 ms estimate is back-of-the-envelope.
