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

### 1. Box the gradient variants of `Brush` — the keystone struct win
`src/primitives/brush.rs:456`, `background.rs`, `stroke.rs`

`Brush` is **60 B** because it inlines `Radial(RadialGradient = 60 B)`, yet
`Solid(Color)` (16 B) is the verified 99% path. Worse, `Background` (**168 B**)
carries `Brush` *twice* (fill 60 + stroke's brush 60 = 120 of 168 B), and
`AnimRow<AnimatedLook>` stores **four** copies per animating widget.

```rust
enum Brush {
    Solid(Color),                 // 16 B inline — the hot path
    Linear(Box<LinearGradient>),  // 8 B
    Radial(Box<RadialGradient>),  // 8 B
    Conic(Box<ConicGradient>),    // 8 B
}
```

→ `Brush` 60→**20 B**, `Stroke` 64→24, `Background` 168→**~88 B**. `Brush` is
*not* `Pod` (it's lowered to `ShapeBrush` for the GPU), so nothing blocks this;
the only size pin test checks `LinearGradient` itself (`brush.rs:685`), not the
enum. Solid clone stays alloc-free; only the rare gradient pays a heap alloc.
This single change shrinks **every** copy across record/lower/animate/cache-blit
and quarters the animation-row footprint — directly attacking the `__memmove`
line (3.9–5.1% per arm).

**Impact: high. Risk: low. Effort: low-medium.**

### 2. Kill the per-widget `WidgetLook` clone on the resting path
`src/widgets/mod.rs:100`, `src/animation/mod.rs:235`

`button_look` does `style.pick(state).clone()` **every frame for every
Button/DragValue/ComboBox** purely to end the `ui.theme` borrow — then for a
resting widget `animate`'s fast path hands it straight back. Two complementary
fixes: (a) #1 makes the clone cheap; (b) in `tick`'s settled fast-path
(`animation/mod.rs:235`), when `row.settled && row.target == target`, return
the caller's already-owned `target` instead of `row.current.clone()` —
eliminates the 168 B clone for resting widgets whenever *any* animation is live.

**Impact: high (this is the real O4). Risk: low.**

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
  (showcase only). Boxing it drops the hot per-frame shape buffer ~17%. **Best
  impact/effort ratio in the record pass.**
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
  mechanism (1 hash, not 8); O4 site is `src/widgets/mod.rs:100` + the keystone
  is boxing `Brush`; O6 "O(1) union" is unsafe.
- `src/ui/state.rs:1-5`: "no `Any` downcast on the hot path" is false.

---

## Suggested order of attack

#1 (Brush boxing) → #2 (clone elision) → #4 (per-node intrinsics) →
#3 (quiescence) → #5 (transform fusion) + the batched small wins → then the
Tier-2 structural pair (#7/#8) with benches. Tier 1 + small wins are mostly
mechanical and self-contained; Tier 2 needs the bench harness in the loop. All
of these *delete work* rather than tune microarchitecture — the right shape for
an IPC-3.3 retiring-bound workload.
