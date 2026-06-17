# Palantir performance investigation — findings

*Investigation date: 2026-06-17. Companion to `docs/cpu-arm-profiling.md`
(the perf-counter profile that motivated it). All `file:line` citations
verified against the code at time of writing.*

## Bottom line up front

Palantir is **already aggressively optimized** — SoA columns, arena reuse,
MeasureCache, a whole-frame cascade skip, FxHash passthrough, lowered chrome,
tail-only GPU uploads. The workload is **retiring-bound** (IPC ~3.3, near-zero
branch/cache miss per `docs/cpu-arm-profiling.md`), so the *only* lever is
**executing fewer instructions**. Micro-architectural tuning is pointless;
deleting redundant per-frame work is everything.

> **Confirmed the hard way (2026-06-17):** the original "keystone" — shrinking
> `Brush`/`Background` by boxing the gradient variants (#1) — was implemented and
> **regressed the frame bench ~2 %**. *Shrinking a struct ≠ deleting work*; on a
> retiring-bound workload, struct-size wins only pay off if they remove
> instructions (copies, recursion, probes), not bytes. See #1's tombstone.

Two reframings that change what's worth doing:

1. **`docs/cpu-arm-profiling.md` is partly stale.** O1 (intrinsic cache) is
   *already shipped* (`src/layout/cache/mod.rs:67` `root_intrinsics`). O3's
   stated mechanism is wrong (only 1 of ~8 queries is a hash probe). O4's
   clone is mislocated (`src/widgets/mod.rs:100`, not `widget_look.rs:69`).
   O6's "O(1) union" is unsafe as written. Corrections at the end.

2. **The cascade — the doc's "#1 hotspot in every arm" — already has a
   whole-frame skip.** `src/ui/mod.rs:636-643` ("O5 stage 0"): when
   `cascade_fingerprint()` matches the prior frame, the entire cascade is
   reused verbatim. So cascade cost only appears when *something changed*; the
   remaining win (O5) is a *per-subtree* cache for **partial-change** frames,
   not idle frames.

Findings below are ranked by impact × tractability.

---

## Tier 1 — High value, low risk (do these first)

### 1. ~~Box the gradient variants of `Brush`~~ — **TRIED & REJECTED (2026-06-17)**
`src/primitives/brush.rs`, `background.rs`, `stroke.rs`

Implemented with `Rc` (strictly better than the `Box` above — clones stay
alloc-free, so even gradient brushes cost only a refcount bump, and the wire
format is unchanged via serde's `rc` feature). The struct shrink landed exactly
as predicted — **`Brush` 60→24 B, `Background` 168→104 B** — and all tests +
visual goldens passed. But the clean back-to-back frame bench (palantir submodule
stashed for a true baseline, both runs warm) showed a **consistent ~1.4–2.5 %
CPU *regression*** across every arm:

| arm | baseline (inline) | after (`Rc`) | Δ |
|---|---|---|---|
| cached | 334.35 µs | 342.59 µs | +2.5 % |
| partial | 289.59 µs | 296.55 µs | +2.4 % |
| resizing | 456.32 µs | 462.89 µs | +1.4 % |
| scrolling | 410.63 µs | 417.85 µs | +1.8 % |

**Why the premise was wrong.** (1) The recording hot path already passes
`&Background`/`&Brush` (both are `!Copy` precisely so they aren't copied by
value), so the smaller struct had almost nothing to speed up. (2) The real cost
*added*: today's inline `Brush` is **trivially droppable** (`needs_drop == false`
— `ArrayVec<[Stop;8]>` of `Copy` data); giving any variant an `Rc` makes
`Brush`/`Background`/`Stroke` carry **drop glue**, so every one of the many
per-frame *Solid* backgrounds now pays a discriminant-check on drop where it was
fully elided before, plus refcount inc/dec on the gradients. (3) The
"avoid per-frame allocs" angle only beats `Box` — versus the *current* inline
storage, clones were already alloc-free (`alloc_free` is strict-zero both ways),
so `Rc` adds no allocation advantage over the status quo.

Reverted. **Do not re-box the gradient payload** (with `Box` *or* `Rc`) chasing
a frame-time win — the footprint shrink is real but does not translate to CPU on
this workload, and the added drop glue is a net loss. The lesson generalizes:
*on an IPC-3.3 retiring-bound workload, deleting copies beats shrinking them*
(which is why #2 below is the correct heir to this idea).

The size shrink *itself* (memory footprint, `AnimRow` ×4) is the only thing this
would have bought; if footprint ever becomes the constraint, revisit — but pin
the frame bench first.

### 2. Kill the per-widget `WidgetLook` clone on the resting path
`src/widgets/mod.rs:100`, `src/animation/mod.rs:235`

`button_look` does `style.pick(state).clone()` **every frame for every
Button/DragValue/ComboBox** purely to end the `ui.theme` borrow — then for a
resting widget `animate`'s fast path hands it straight back. With #1 rejected the
clone stays a full 168 B `Background` copy and *cannot* be made cheap by
shrinking — so the only lever is to **delete it**, which is the correct shape for
this workload anyway. In `tick`'s settled fast-path (`animation/mod.rs:235`),
when `row.settled && row.target == target`, return the caller's already-owned
`target` instead of `row.current.clone()` — eliminates the clone for resting
widgets whenever *any* animation is live, attacking the same `__memmove` line #1
targeted but by removing the copy rather than resizing it.

**Impact: high (this is the real O4, and the direct heir to the rejected #1).
Risk: low.**

### 3. `response_for` quiescence — compute once per frame, not per widget
`src/input/mod.rs:849`

O3 is real (2.4–2.5% every arm, ~70 widgets paying it with zero input) but the
*mechanism* in the profiling doc is wrong: only `entry_idx_of` is a hash probe;
the rest are plain `Option`/array compares plus two 3-iteration loops
(`active_drag`, `double_click`). Best fix: cache an `is_quiescent` bool **once
per frame** (like `frame_line_px` already is at `mod.rs:535`), and split
`response_for` so the geometry half (rect/layout_rect/disabled — needed for
theme picking) is built while the interaction half defaults out.

**Impact: ~2% every arm + real idle frames. Risk: low.**

### 4. Restore **per-node** intrinsics on a MeasureCache hit (the real O1 residual)
`src/layout/layoutengine.rs:343-355`, `src/layout/cache/mod.rs:67,287`

O1-as-documented shipped, but it only caches the *root* node's intrinsic. When
a deep node changes, a re-measuring ancestor's `children_max_intrinsic` still
**cold-recurses** through unchanged *interior* containers (restored via blit,
never independently snapshotted) re-probing the text cache per leaf. Fix: add an
`intrinsics` arena parallel to `desired` in MeasureCache, written on the
snapshot path and `copy_from_slice`-restored on a hit — the exact machinery
`desired` already uses. `src/layout/measure-cache.md:74-76` flags this as open.

**Impact: removes the residual 5–9% intrinsic/shaping from partial/resize/scroll.
Risk: low.**

### 5. Fuse transform + DPR scale into one precomputed `TranslateScale` in `compose`
`src/renderer/frontend/composer/mod.rs:476-492,525-547`

Every shape arm does `current_transform.apply_rect(rect)` (4 mul + 4 add)
**then** `.scaled_by(scale)` (4 more mul) — two affine passes where one
suffices — and recomputes `current_transform.scale * scale` *per draw* though
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

The hashing pass that *produces* the MeasureCache key is itself an unconditional
O(total-nodes) reverse sweep every frame — you pay full re-hashing of ~800
nodes to discover they're all cache hits. Fold the node-hash into `close_node`
(data still hot from `open_node`, eliminates the separate pass) or dirty-skip
unchanged subtrees.

**Highest ceiling, highest risk** (every cross-frame cache key depends on it) —
prototype behind the bench.

### 10. Reuse `SeenIds.curr` from `prev` on unchanged-structure frames
`src/forest/seen_ids.rs`

`curr` is rebuilt with ~800 inserts/frame, but on a no-structural-change frame
it's *identical* to `prev` — which the cascade fingerprint already detects. A
no-op rollover path eliminates the inserts on every steady-state frame.

**Risk: medium.**

---

## Tier 3 — Unconventional bets

- **Decouple the cache quantum from the text-shaping quantum**
  (`src/layout/cache/mod.rs:142`, `layoutengine.rs:140`). `available_q`
  quantizes to 1px because *text wrap* needs it — but non-wrap-text subtrees
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
  `Curve` variant sets the enum to 96 B but appears in *zero* production widgets
  (showcase only). Boxing it would drop the hot per-frame shape buffer ~17%.
  **But heed #1's tombstone:** `ShapeRecord` is *higher* multiplicity than `Brush`
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

## Verified already-optimal — do *not* chase these

Confirmed tuned; leave them alone: the **FxHash `Hasher` + `.pod()`**
(`src/common/hash.rs`); the **glyph atlas** (no re-rasterize/re-upload,
grow-blits rects via `copy_texture_to_texture`); **`lower_background`'s solid
path** (early-returns, no atlas); **soa-rs `push`** (no redundant reserve);
**lazy collision counters & paint-anim columns**; **`DynamicBuffer`
tail-upload + grow-mapped path for text**; the **single measure dispatch** (no
WPF grow loop). `lower_background` cross-frame memoization is *not* viable — the
content hash is computed by the lowering itself, so no cheaper key exists.

## Doc corrections (independent of any code change)

- `docs/cpu-arm-profiling.md`: O1 already shipped (residual is per-node, #4); O3
  mechanism (1 hash, not 8); O4 site is `src/widgets/mod.rs:100` (fixed by deleting
  the clone, #2 — *not* by boxing `Brush`, which was tried and regressed, see #1's
  tombstone); O6 "O(1) union" is unsafe.
- `src/ui/state.rs:1-5`: "no `Any` downcast on the hot path" is false.

---

## Suggested order of attack

With #1 rejected, the **most optimistic remaining bet is #4 (per-node intrinsics
on a MeasureCache hit)** — highest quantified upside (removes the residual 5–9 %
intrinsic/shaping cost on *three* arms: partial, resize, scroll), low risk, and
it reuses the exact `desired`-arena machinery already proven, deleting a cold
recursion rather than shrinking a struct. Crucially it is a *work-deletion* win,
the shape the Brush failure says to favor.

Order: #2 (clone elision — cheap, self-contained, the direct heir to #1) →
**#4 (per-node intrinsics — the headline)** → #3 (quiescence) → #5 (transform
fusion) + the batched small wins → then the Tier-2 structural pair (#7/#8) with
benches. Tier 1 + small wins are mostly mechanical and self-contained; Tier 2
needs the bench harness in the loop. All of these *delete work* rather than tune
microarchitecture — the right shape for an IPC-3.3 retiring-bound workload. And
per #1's tombstone: **measure each behind the frame bench before trusting its
label** — "high impact, low risk" is exactly what #1 claimed.
