# `frame` bench — CPU-arm profile & optimization notes

Deep profile of the four CPU arms of the `frame` bench
(`frame/{cached,partial,resizing,scrolling}_cpu`) with optimization
suggestions ranked by expected impact.

## Method

- **Machine:** AMD Ryzen 7 6800U (Zen3+, 8c/16t), 16 MiB L3, Debian
  Trixie, kernel 6.12. Homogeneous cores — no Intel-style hybrid PMU,
  so the `cpu_core/…` / `TopdownL1` paths in `scripts/bench-perf.sh`
  (written for a Raptor Lake i9) do **not** apply here; events below are
  the generic AMD set.
- **Setup:** `performance` governor, pinned to core 3 (`taskset -c 3`),
  `perf_event_paranoid=1`. Each arm run for `--profile-time 8`.
- **Build:** `CARGO_PROFILE_BENCH_DEBUG=2 cargo bench --bench frame
  --features internals --no-run`. ⚠️ **`[profile.bench]` has
  `debug = false`** in the workspace `Cargo.toml`, contradicting
  `benches/CLAUDE.md` ("already builds with optimized + debuginfo … no
  extra flags needed"). Without the override perf shows only exported
  symbols and no inline expansion. Either the doc or the profile setting
  should change.
- **perf:** `perf record -F 4000 --call-graph dwarf,16384 -e cycles`
  for stacks; `perf stat -d` for counters. Raw data under
  `palantir/tmp/prof/` (gitignored); re-run with `tmp/prof/run.sh`.

## Headline: every arm is retiring-bound

| arm | IPC | GHz | branch-miss | L1-d miss | frontend idle |
|-----------|------|-------|-------------|-----------|---------------|
| cached    | 3.27 | 4.71  | 0.16 %      | 3.50 %    | 2.48 %        |
| partial   | 3.05 | 4.69  | 0.18 %      | 2.94 %    | 3.84 %        |
| resizing  | 3.31 | 4.67  | 0.14 %      | 3.41 %    | 2.28 %        |
| scrolling | 3.37 | 4.68  | 0.18 %      | 3.27 %    | 2.26 %        |

IPC ~3.3 with negligible branch-misprediction (<0.2 %), low frontend
stalls (<4 %), and a modest L1 miss rate that the high IPC shows is
largely absorbed by L2/prefetch. **The pipeline is busy retiring
instructions, not stalling.** The lever is therefore *executing fewer
instructions* — cut redundant per-frame recomputation — not cache
layout or branch tuning. The opportunities below are ranked by how much
*work they delete*, which is the only thing that moves a retiring-bound
workload.

## Self-time hotspots by arm

Top self-time symbols (cycles), per arm:

| % (cached) | function |
|-----|----------|
| 9.7 | `cascade::CascadesEngine::run` |
| 8.3 | `composer::Composer::compose` |
| 6.3 | `composer::text_grid::TextRectGrid::any_overlap` |
| 6.3 | `encoder::encode_node` |
| 5.0 | `text_grid::TextRectGrid::push` |
| 4.9 | `forest::Forest::open_node` |
| 4.0 | `__memmove_avx_unaligned_erms` (libc) |
| 3.7 | `Composer::flush` |
| 3.4 | `Tree::post_record` |
| 3.3 | `LayoutEngine::arrange` |
| 2.9 | `FrameArena::lower_background` |
| 2.7 | `Ui::make_persistent_id` |
| 2.5 | `InputState::response_for` |

| % (partial) | function | | % (resizing) | function | | % (scrolling) | function |
|----|----|-|----|----|-|----|----|
| 10.6 | `CascadesEngine::run` | | 8.1 | `CascadesEngine::run` | | 7.7 | `CascadesEngine::run` |
| 5.7 | `Forest::open_node` | | 6.1 | `TextShaper::shape_unbounded` | | 6.3 | `DamageEngine::compute` |
| 5.1 | `__memmove_avx` | | 5.3 | `LayoutEngine::measure` | | 5.7 | `Composer::compose` |
| 5.0 | `TextShaper::shape_unbounded` | | 4.9 | `__memmove_avx` | | 5.4 | `encoder::encode_node` |
| 4.3 | `intrinsic::compute` | | 4.4 | `intrinsic::compute` | | 5.3 | `DamageRegion::add` |
| 3.9 | `Tree::post_record` | | 4.1 | `Composer::compose` | | 4.3 | `TextRectGrid::any_overlap` |
| 3.7 | `LayoutEngine::arrange` | | 4.0 | `Forest::open_node` | | 4.0 | `Forest::open_node` |

## Subsystem breakdown (cached, inclusive)

The cached arm encodes a **synthesized `Full` plan every iteration** —
`CpuHarness::frame` substitutes one when damage resolves to `Skip` (see
`benches/CLAUDE.md`). So its compose/encode weight is a **full-repaint
worst case** (resize / first-frame / heavy-scroll), *not* steady idle —
in production an idle frame skips paint entirely.

| subsystem | inclusive | note |
|-----------|-----------|------|
| compose (`Composer::compose` + text grid + flush) | ~25 % | full-tree repaint |
| record pass (`open_node`, widget `show`, lowering, hashing) | ~30 % | every node, every frame |
| cascade (`CascadesEngine::run`) | ~10 % | every node, no cross-frame cache |
| arrange | ~6 % | |
| encode (`encode_node`) | ~6 % | scales with damage area |
| `post_record` (hash rollups) | ~3.5 % | |
| measure | cheap | MeasureCache fully hits at steady state |

The other arms shift weight: **partial/resizing/scrolling** spend
heavily in **measure + intrinsic + text shaping** (O1 below), and
**scrolling** adds ~11.5 % in **damage** (O6).

---

## Optimization opportunities

### O1 — Persist intrinsic-min across frames (biggest, most concrete)

**Evidence.** In partial, `shape_unbounded` 5.0 % + `intrinsic::compute`
4.3 % ≈ **9 %**, plus `LayoutEngine::measure` at **19.5 % inclusive** —
even though *only the footer's 8-digit counter changed*. resizing:
`shape_unbounded` 6.1 % + `intrinsic` 4.4 % + `measure` 5.3 %.
scrolling: `shape_unbounded` 3.6 % + `intrinsic` 2.9 %. The callgraph
attributes `shape_unbounded` almost entirely to
`children_max_intrinsic → intrinsic::compute → LayoutEngine::intrinsic`,
and in scrolling its self-time is dominated by the *HashMap probe* of
the text reuse cache (`hashbrown … find`), i.e. the cache *check*, not
real shaping.

**Root cause.** `LayoutEngine::intrinsic` (`layoutengine.rs:307`) caches
results only in **per-frame** scratch (`scratch.intrinsics[node][slot]`,
reset every `run`). The cross-frame `MeasureCache` hit path
(`layoutengine.rs:442`) restores `desired` + text shapes but **never
restores intrinsics** (confirmed: no `intrinsic` references in
`src/layout/cache/`). So when a deep node changes, its subtree hash
rolls up and **every ancestor misses the MeasureCache**; each ancestor
then computes its `intrinsic_min` via `children_max_intrinsic`
(`support.rs:207`), which calls `layout.intrinsic(child)` for *every*
child — and for children whose own measure was a cache *hit*,
`scratch.intrinsics[child]` is NaN, so it **cold-recurses through the
entire unchanged subtree**, re-probing the text reuse cache
(`text/mod.rs:209`) once per text leaf. One footer digit → a full-tree
intrinsic re-walk.

**Fix.** Store the subtree root's intrinsic (per axis/slot) in the
`MeasureCache` entry and restore it into `scratch.intrinsics[root]` on a
hit (alongside the existing `desired` blit in `restore_after_cache_hit`,
`layoutengine.rs:159`). Then a re-measuring parent's
`children_max_intrinsic` reads the cached child intrinsic instead of
recursing. Bounds intrinsic work to *genuinely changed* subtrees — the
same locality the MeasureCache already gives `desired`.

**Expected:** removes most of the 5–9 % intrinsic+shaping cost from
partial / resizing / scrolling. **Risk:** moderate — touches the
measure-cache entry layout and hit path; pin with a test that a
localized text change does not re-probe the text cache for siblings
(extend the `cache/` integration tests). **Confidence:** high on
diagnosis, high on direction.

### O2 — Composer text-overlap grid (full-repaint paths)

**Evidence.** cached: `any_overlap` 6.3 % + `push` 5.0 % ≈ **11 %**;
resizing ~7 %, scrolling ~8 %. `quad_forces_flush`
(`composer/mod.rs:319`) calls `text_grid.any_overlap(quad_rect)` for
**every quad** to decide whether the quad must flush to preserve
paint-order vs. already-scheduled text; `push` registers every text
rect into every tile it covers.

**Caveats.** This index already *replaced* a linear scan and carries
touched-tile tracking + grow-only reshape (see `text_grid.rs` header);
a union-AABB pre-reject was explicitly considered and rejected. And
because the cached arm is a synthesized full repaint, this 11 % is a
**full-repaint cost**, near-zero on partial frames. So treat O2 as
"make full repaints cheaper," not "fix steady state."

**Ideas (need measurement, incremental):**
- The per-quad `any_overlap` still computes a tile range and indexes
  even when there is no text near the quad. A cheap **per-group text
  bounding box** (min/max of pushed rects) tested before the tile walk
  would O(1)-reject the many large background quads that sit in
  text-free regions. Re-measure against the earlier union-reject
  conclusion — that was for a *global* union; a *per-group* box may
  reject more.
- `push` touches one tile for most labels but several for wrapped
  paragraphs / the prop-grid values. A coarser push tile (register into
  fewer tiles, accept more per-tile false positives on query) trades
  push cost for query cost — worth a sweep since pushes (5 %) ≳ the
  query early-exits.

**Confidence:** medium — already-tuned code; gains are incremental.

### O3 — `response_for` quiescent-input fast path

**Evidence.** `InputState::response_for` (`input/mod.rs:1031`) is **2.4–
2.5 % in every arm**, called once per interactive widget — and the
fixture has ~70+ buttons/toggles/etc. The bench injects **zero input**,
yet each call does ~8 input-state queries (several HashMap probes:
`capture` ×2, `scroll_pixels_for`, `scroll_lines_for`, `zoom_delta_for`,
`active_drag`) that all resolve to default.

**Fix.** Add an `InputState::is_quiescent()` predicate (no pointer, no
active capture/drag, no click/scroll/zoom routed this frame). When true,
`response_for` returns a `ResponseState` with only `rect` / `layout_rect`
/ `disabled` filled (those come from `cascades`, still needed) and
everything else `Default`. Skips all per-widget input probes on idle
frames.

**Expected:** ~2 % every arm; in a **real idle UI frame** (the common
case — pointer still, nothing animating) the same win applies. **Risk:**
low — the quiescent state is well-defined and the returned struct is
already all-default in that case. **Confidence:** high.

### O4 — Per-widget look clone & `Background` size

**Evidence.** `WidgetLook::animate` 1.6–2.4 % every arm;
`__memmove_avx` 3.9–5.1 %; `lower_background` 2.1–3.0 %.
`WidgetLook::animate` (`widget_look.rs:69`) builds an owned
`AnimatedLook` by cloning a **168-byte `Background`**
(`self.background.unwrap_or_default()`, line 77) *per widget per frame*,
even for a resting button with no animation — then `ui.animate`'s
fast-path just returns it. `Background` is also copied through
`open_node → lower_background` and blitted in MeasureCache subtree
restores; it is a major contributor to the `__memmove` line.

**Fix (two independent levers):**
1. **Shrink `Background`** (fill `Brush` + `Stroke` + `Corners` +
   `Shadow` = 168 B). `Shadow` is rarely set — boxing it, or packing the
   `Brush`/`Stroke` representation, shrinks every copy across record,
   lowering, animate, and cache blits. Re-run
   `hot_struct_sizes::print_hot_struct_sizes` before/after.
2. **Skip the clone when not animating.** When `ui.animate` will no-op
   (no anim row for this id + instant/none spec — the
   `anim.by_type.is_empty()` fast-path in `ui/mod.rs:1050`), resting
   widgets still pay the owned-target clone. Let the resting path carry
   a borrowed look (`Cow`/by-ref) into paint instead of materializing
   `AnimatedLook` by value.

**Expected:** trims a chunk of the 4–5 % `__memmove` plus the 2 % look
cost. **Risk:** (1) low-mechanical but wide blast radius (struct size is
load-bearing per CLAUDE.md — re-run alloc + visual suites); (2) moderate
(reshapes the look→paint data flow). **Confidence:** high that copies
shrink; medium on exact payoff.

### O5 — Cascade delta-cache (structural, high value / high cost)

**Evidence.** `CascadesEngine::run` is the **#1 self-time symbol in
every arm** (7.7–10.6 %) and has **no cross-frame cache** — it re-walks
the whole tree each frame flattening transform/clip/disabled/visibility
and rebuilding the hit index. The scrolling arm exists precisely to
measure this: a pure `Panel::transform` shift changes nothing structural
yet pays the full cascade walk.

**Direction.** Cache per-subtree cascade output (cascade-input hash +
paint-rect rollup + hit entries) keyed on the same subtree hash the
MeasureCache uses; on a pure-translate parent change, translate the
cached `subtree_paint_rects` / entry rects by the delta instead of
re-deriving them per node (`cascade/mod.rs:490` `compute_paint_rect`,
`:529` `push_entry`, `:586` `build_cascade_prefix`). This is the largest
single line item but also the most invasive; **do O1/O3/O6 first** —
they are cheaper and partly overlap the same change-locality machinery.
**Confidence:** high on the opportunity, low on a cheap implementation.

### O6 — Damage fast-path for pure translate (scrolling)

**Evidence.** scrolling: `DamageEngine::compute` 6.3 % +
`DamageRegion::add` 5.3 % ≈ **11.5 %**, by far its largest delta vs. the
other arms. A transform shift moves an entire subtree, and damage
currently diffs old-vs-new paint rects node-by-node and unions many
rects into the region.

**Fix.** For a subtree whose only change is its inherited transform (a
pure translate — detectable from the cascade delta), the damage is
exactly `union(old_subtree_bounds, new_subtree_bounds)`, computable in
O(1) from the cached `subtree_paint_rect` + the translation, with no
per-node diff and a single `DamageRegion::add`. Falls back to the
per-node path when content actually changed. **Risk:** moderate (damage
correctness is pinned by `ui/damage/tests.rs` — lean on it).
**Confidence:** medium-high; pairs naturally with O5's cascade delta.

### O7 — Smaller, pervasive record-pass costs

The immediate-mode tax: the tree is rebuilt every frame (~800 nodes at
bench scale). Already SoA-tuned, but a few items recur in every arm:

- **`Ui::make_persistent_id` 2.0–2.7 % + `HashMap::insert` 1.8–2.0 %**
  (`ui/mod.rs:954`): per-node id hash + a `SeenIds` map insert every
  frame. `reserve` the `SeenIds`/state maps to the prior frame's node
  count to kill incremental rehashes; for the no-collision common case a
  cheaper occurrence tracker than a full `HashMap` may suffice.
- **`lower_background` 2.1–3.0 %** (`frame_arena.rs:181`): re-lowers
  every chrome (brush + stroke + shadow + content hash) every frame even
  when the `Background` is value-identical. Cheap for solids; the
  gradient-atlas path costs more. Lower priority — value-hashing to
  memoize may cost as much as lowering for the common solid case.
- **`Tree::post_record` 2.8–3.9 %**: subtree hash rollups feeding all
  the cross-frame caches. Inherent, but it is the enabler for O1/O5/O6 —
  worth keeping cheap.

---

## Suggested order of attack

1. **O1 (intrinsic cache)** — concrete, self-contained, removes 5–9 %
   from three of four arms; the diagnosis is airtight.
2. **O3 (response_for fast path)** — ~2 % everywhere, low risk, also
   helps real idle frames.
3. **O4 (Background size / look clone)** — shrinks the pervasive
   `__memmove`; mechanical but verify struct-size + alloc suites.
4. **O6 (damage pure-translate)** then **O5 (cascade delta-cache)** —
   the structural pair; biggest ceiling, highest cost. Do after the
   cheaper wins, reusing the same change-locality machinery.
5. **O2 (composer)** — incremental, full-repaint-only; measure before
   investing, the index is already tuned.

All of these *delete work* rather than tune microarchitecture, which is
the right shape for an IPC-3.3 retiring-bound workload.

## Appendix

- Raw perf data + reports: `palantir/tmp/prof/{flat,incl,stat,graph}-*`
  and `*.data` (gitignored). Re-run: `bash tmp/prof/run.sh`.
- Per-frame wall time at bench scale (criterion, this build):
  `partial_cpu` ≈ 167 µs/frame; the four arms are within ~1.5× of each
  other.
- Doc drift to fix: `[profile.bench] debug = false` vs. `benches/CLAUDE.md`'s
  "already builds with … debuginfo."
