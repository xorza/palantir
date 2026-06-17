# Palantir performance investigation — findings

_Investigation date: 2026-06-17. Companion to `docs/cpu-arm-profiling.md`
(the perf-counter profile that motivated it). All `file:line` citations
verified against the code at time of writing._

## Bottom line up front

Palantir is **already aggressively optimized** — SoA columns, arena reuse,
MeasureCache, a whole-frame cascade skip, FxHash passthrough, lowered chrome,
tail-only GPU uploads. The workload is **retiring-bound** (IPC ~3.3, near-zero
branch/cache miss per `docs/cpu-arm-profiling.md`), so the _only_ lever is
**executing fewer instructions**. Micro-architectural tuning is pointless;
deleting redundant per-frame work is everything.

> **Confirmed the hard way (2026-06-17):** the original "keystone" — shrinking
> `Brush`/`Background` by boxing the gradient variants (#1) — was implemented and
> **regressed the frame bench ~2 %**. _Shrinking a struct ≠ deleting work_; on a
> retiring-bound workload, struct-size wins only pay off if they remove
> instructions (copies, recursion, probes), not bytes. See #1's tombstone.

Two reframings that change what's worth doing:

1. **`docs/cpu-arm-profiling.md` is partly stale.** O1 (intrinsic cache) is
   _already shipped_ (`src/layout/cache/mod.rs:67` `root_intrinsics`). O3's
   stated mechanism is wrong (only 1 of ~8 queries is a hash probe). O4's
   clone is mislocated (`src/widgets/mod.rs:100`, not `widget_look.rs:69`).
   O6's "O(1) union" is unsafe as written. Corrections at the end.

2. **The cascade — the doc's "#1 hotspot in every arm" — already has a
   whole-frame skip.** `src/ui/mod.rs:636-643` ("O5 stage 0"): when
   `cascade_fingerprint()` matches the prior frame, the entire cascade is
   reused verbatim. So cascade cost only appears when _something changed_; the
   remaining win (O5) is a _per-subtree_ cache for **partial-change** frames,
   not idle frames.

Findings below are ranked by impact × tractability.

---

## Tier 1 — High value, low risk (do these first)

### 2. ~~Kill the per-widget `WidgetLook` clone on the resting path~~ — **TRIED & NEUTRAL (2026-06-17)**

`src/widgets/mod.rs:100`, `src/animation/mod.rs:235`

Implemented the `tick` settled-fast-path part: return the caller's already-owned
`target` instead of `row.current.clone()` (provably bit-identical — every settle
path snaps `current = target`, and the arm already checked `target == row.target`;
all tests + visual goldens passed unchanged). Clean back-to-back frame bench:

| arm       | baseline  | after     | Δ      |
| --------- | --------- | --------- | ------ |
| cached    | 334.02 µs | 337.18 µs | +0.9 % |
| partial   | 287.90 µs | 289.30 µs | +0.5 % |
| resizing  | 459.04 µs | 459.70 µs | +0.1 % |
| scrolling | 411.03 µs | 416.22 µs | +1.3 % |

**Within noise — no measurable change.** And it's neutral _by construction_: the
caller (`button_look`) builds `target: AnimatedLook` and passes it **by value**
regardless, so on the settled path both forms do exactly _one_ 168 B memcpy into
the return slot — cloning `row.current` vs moving `target` are the same cost. The
swap doesn't delete a copy, it picks which equal value to move. The genuinely
redundant copy is the unconditional `style.pick(state).clone()` at
`widgets/mod.rs:100` (built every frame to end the `ui.theme` borrow even on the
settled path) — but deleting _that_ needs a borrow-restructure so `animate` can
read the look in place, and at 168 B × ~160 widgets ≈ 27 KB/frame it is **far
below the bench's ~1 % noise floor** anyway (the frame's cost is the O(n) passes
over ~800 nodes, not these micro-copies). Reverted. **Do not re-attempt the
tick-level swap** — it cannot help; the value is built by the caller.

### 3. `response_for` quiescence — compute once per frame, not per widget

`src/input/mod.rs:849`

O3 is real (2.4–2.5% every arm, ~70 widgets paying it with zero input) but the
_mechanism_ in the profiling doc is wrong: only `entry_idx_of` is a hash probe;
the rest are plain `Option`/array compares plus two 3-iteration loops
(`active_drag`, `double_click`). Best fix: cache an `is_quiescent` bool **once
per frame** (like `frame_line_px` already is at `mod.rs:535`), and split
`response_for` so the geometry half (rect/layout_rect/disabled — needed for
theme picking) is built while the interaction half defaults out.

**Impact: ~2% every arm + real idle frames. Risk: low.**

### 5. Fuse transform + DPR scale into one precomputed `TranslateScale` in `compose`

`src/renderer/frontend/composer/mod.rs:476-492,525-547`

Every shape arm does `current_transform.apply_rect(rect)` (4 mul + 4 add)
**then** `.scaled_by(scale)` (4 more mul) — two affine passes where one
suffices — and recomputes `current_transform.scale * scale` _per draw_ though
it only changes on Push/Pop. Maintain `current_phys: TranslateScale` (updated
on Push/Pop/frame-start) and a fused `Rect::scaled_snap_by`. Roughly **halves
per-quad coordinate math**; quads dominate, compose is ~25% of a full repaint.

**Impact: high. Risk: low-medium (keep snap bit-identical — visual goldens
cover it).**

### 6. `StateMap`: use `downcast_unchecked` and fix the false doc

`src/ui/state.rs:34-43,54-61`

The module doc claims "no `Any` downcast on the hot path" — **false**: every
`state_mut`/`try_state` does a `TypeId` HashMap probe + a vtable-checked
`downcast`. The `.expect()` already proves the type is correct, so
`downcast_*_unchecked` is justified (removes the type-check branch); for the
handful of `T`s, a linear `Vec<(TypeId, …)>` beats hashing.

**Impact: per stateful widget per frame. Risk: low.** Fix the doc regardless.

---

## Tier 2 — Structural / algorithmic (bigger ceiling, more work)

### 7. Per-subtree cascade delta-cache (O5, partial-change frames)

`src/ui/cascade/mod.rs`

The whole-frame skip (stage 0) handles idle; this handles "one leaf changed →
full re-walk." Cache per-subtree cascade output keyed on the same `subtree_hash`
MeasureCache uses; on a pure-translate parent change, translate cached
`subtree_paint_rects`/entry rects by the delta instead of re-deriving per node.
Biggest single line item, most invasive — pairs with #8.

**Risk: high.** Do after Tier 1.

### 8. Subtree-translate damage (O6, done safely)

`src/ui/damage/mod.rs:586-708`

A scroll changes every descendant's `cascade_input` (parent transform differs)
→ defeats both subtree-skip tiers → per-node O(n·m) diff + two rects/shape
flooding `DamageRegion::add` (the 11.5% on scroll). But the profiling doc's
"O(1) union(old,new)" is unsafe — a full-viewport scroll's union trips
`FULL_REPAINT_THRESHOLD` anyway, and clipped viewports need clip-intersection.
Real fix: detect translate-only at the **subtree root**, emit
`union(prev,curr).intersect(clip)` once, and reuse the existing subtree-skip
jump. Requires snapshotting prev `subtree_paint_rect`.

**Risk: high — lean on `src/ui/damage/tests.rs`.**

### 9. Incrementalize / fold `compute_hashes`

`src/forest/tree/mod.rs:228-308`

The hashing pass that _produces_ the MeasureCache key is itself an unconditional
O(total-nodes) reverse sweep every frame — you pay full re-hashing of ~800
nodes to discover they're all cache hits. Fold the node-hash into `close_node`
(data still hot from `open_node`, eliminates the separate pass) or dirty-skip
unchanged subtrees.

**Highest ceiling, highest risk** (every cross-frame cache key depends on it) —
prototype behind the bench.

### 10. Reuse `SeenIds.curr` from `prev` on unchanged-structure frames

`src/forest/seen_ids.rs`

`curr` is rebuilt with ~800 inserts/frame, but on a no-structural-change frame
it's _identical_ to `prev` — which the cascade fingerprint already detects. A
no-op rollover path eliminates the inserts on every steady-state frame.

**Risk: medium.**

---

## Tier 3 — Unconventional bets

- **Decouple the cache quantum from the text-shaping quantum**
  (`src/layout/cache/mod.rs:142`, `layoutengine.rs:140`). `available_q`
  quantizes to 1px because _text wrap_ needs it — but non-wrap-text subtrees
  produce bit-identical `desired` for a 3px-different available, yet still miss
  under animation/resize. Add a `subtree_has_wrap_text` packed bit (room next to
  `subtree_has_grid` in `SubtreeEnd`) and coarsen the quantum (4–8px) for
  subtrees without it. Big win for animated/resizing UIs; the sub-pixel error is
  invisible (the code already says so). **The cleverest win here.**

- **Damage-gated skip of the quad instance buffer re-upload**
  (`src/renderer/backend/.../quad_pipeline`). Text uses tail-upload, but quads
  rewrite the full instance buffer every frame even on `Damage::Skip`/unchanged
  frames. You already know nothing changed — skip the belt write. **Low risk;
  verify vs `alloc_free_gpu`.**

- **Bake the cumulative physical transform at encode time**
  (`src/renderer/frontend/encoder/mod.rs` → `composer`). The composer
  re-derives world+physical coords for every draw every frame; the cascade
  already has screen rects. Baking physical transform into payloads removes the
  whole transform-stack machinery from compose (subsumes #5 at the source).
  **High risk/effort — prototype only after Tier 1; flag as "maybe too early"
  per project posture.**

---

## Smaller wins (batched, low effort)

- **Box `ShapeRecord::Curve`** (`src/forest/shapes/record.rs:372`) — the 88 B
  `Curve` variant sets the enum to 96 B but appears in _zero_ production widgets
  (showcase only). Boxing it would drop the hot per-frame shape buffer ~17%.
  **But heed #1's tombstone:** `ShapeRecord` is _higher_ multiplicity than `Brush`
  (~500/frame) and is also trivially droppable today; boxing one variant gives
  every record drop glue + makes `paint_bbox_local`/hash/cascade pay a pointer
  chase. The footprint win is real but the Brush experiment showed footprint ≠
  frame-time here. **Prototype behind the bench before trusting the ~17 %; do not
  ship on the size argument alone.**
- **Composer emptiness-gate on `quad_forces_flush`**
  (`src/renderer/frontend/composer/mod.rs:334`) — for quad-only groups (common),
  skip the two `any_overlap` calls + slice scan via a cached "any text open"
  flag. Complements the known O2.
- **`closed_text_grid` → flat `Vec<URect>`** (`composer/mod.rs:70`) — empty in
  the common single-batch case yet pays a per-frame viewport reshape + per-flush
  clear.
- **Single-occluder fast path in occlusion `prune`**
  (`src/renderer/frontend/composer/occlusion.rs:84`) — skip the
  `prefix_max_cover` alloc/scan when there's ≤1 occluder.
- **Fold scroll states into one hasher in `cascade_fingerprint`**
  (`src/ui/mod.rs:689`) — drop the per-scroll-state `Hasher::new()/finish()`.
- **`Element` (104 B) passed by value** through 3 hops (`src/forest/mod.rs:208`)
  — verify with `cargo-show-asm` whether copies materialize before refactoring.
- **Zero-copy / aligned cmd payload reads** (`src/renderer/frontend/cmd_buffer/mod.rs:660`)
  — most payloads are 4-byte aligned; reserve `pod_read_unaligned` for the
  align-8 text payload and borrow `&T` for the large arms instead of copying out.

---

## Verified already-optimal — do _not_ chase these

Confirmed tuned; leave them alone: the **FxHash `Hasher` + `.pod()`**
(`src/common/hash.rs`); the **glyph atlas** (no re-rasterize/re-upload,
grow-blits rects via `copy_texture_to_texture`); **`lower_background`'s solid
path** (early-returns, no atlas); **soa-rs `push`** (no redundant reserve);
**lazy collision counters & paint-anim columns**; **`DynamicBuffer`
tail-upload + grow-mapped path for text**; the **single measure dispatch** (no
WPF grow loop). `lower_background` cross-frame memoization is _not_ viable — the
content hash is computed by the lowering itself, so no cheaper key exists.

## Doc corrections (independent of any code change)

- `docs/cpu-arm-profiling.md`: O1 already shipped (residual is per-node, #4); O3
  mechanism (1 hash, not 8); O4 site is `src/widgets/mod.rs:100` (fixed by deleting
  the clone, #2 — _not_ by boxing `Brush`, which was tried and regressed, see #1's
  tombstone); O6 "O(1) union" is unsafe.
- `src/ui/state.rs:1-5`: "no `Any` downcast on the hot path" is false.

---

## Suggested order of attack

**Status after 2026-06-17 (three Tier-1 items closed, all null):** #1 (Brush
boxing) _regressed_ ~2 %; #2 (clone elision) is _neutral by construction_; #4
(per-node intrinsics) is _effectively already shipped_. The pattern is decisive:
on this ~800-node / ~500-text-shape synthetic workload, **per-node "delete a
copy / shrink a struct" wins sit below the ~1 % bench noise floor.** The frame is
dominated by the unavoidable O(n) passes (record, the measure walk even on cache
hits, cascade, encode, compose), not by micro-copies. Tuning at that granularity
is finished.

What's left that could actually move the needle **eliminates a whole pass or skips
large subtrees**, not bytes:

- **#9 (incrementalize `compute_hashes`)** — the strongest untried lever. It is an
  _unconditional_ O(~800-node) reverse hash sweep **every frame, even idle ones**
  — you pay a full re-hash just to discover everything is a cache hit. Folding the
  node-hash into `close_node` (data still hot from `open_node`) or dirty-skipping
  unchanged subtrees deletes a real per-frame pass. Highest ceiling, highest risk
  (every cross-frame cache key depends on it) — prototype behind the bench.
- **#7 / #8 (per-subtree cascade + damage delta-cache)** — the only Tier-2 with a
  real ceiling on _partial/scroll_ frames; invasive, needs the bench in the loop.

Everything below #9/#7 in value is micro-tuning that this workload won't register.
**Two caveats before spending more effort:** (1) measure every candidate behind
the frame bench _before_ trusting its doc label — "high impact, low risk" is
exactly what #1/#2/#4 each claimed; (2) consider whether the synthetic bench is
even the right workload — per project posture, a structural change with no
motivating real workload is "too early." If perf isn't currently blocking
anything, the honest call is to **shelve here**: the cheap wins are gone and the
remaining ones are high-risk rewrites that need a concrete reason to exist.
