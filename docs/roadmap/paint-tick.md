# Paint-only frame mode (a.k.a. "paint tick")

**Status:** slice 1 partially landed (2026-05-15) — `PaintAnim`
module ships with `BlinkOpacity` only; `PaintMod` carries only an
`alpha` field. The `Rotation` / `Pulse` / `Marquee` variants need
encoder transform-mod plumbing (per-shape `TranslateScale` push
into the cmd buffer); they're held out of slice 1 along with the
`Spinner` widget, and land once the alpha-only path proves out.
Slice 2 (post-record short-circuit) still gated on the bench.

Audited against codebase 2026-05-15 — frame lifecycle,
`pre_record` / `post_record` shape, `tree.shapes` indexing,
`SubtreeRollups`, `DamageEngine`, `repaint_wakes`, `AnimMap`, and
`text_edit` caret all match current source. Storage choice for
the paint-anim column was revised (see §`PaintAnim` registry
below) — `SparseColumn<T>` doesn't exist; the tree's
sparse-extras pattern is `ExtrasIdx`-packed `Slot`s indexing
dense `*_table: Vec<T>`, and a paint-anim entry needs to be
shape-keyed (not node-keyed) anyway.
**Goal:** drive time-based visuals (caret blink, spinner, focus pulse,
marquee, indeterminate progress) without re-running record / measure /
arrange / cascade on every wake. Spend only the encode + paint passes
that the GPU actually needs, against last frame's retained pipeline
state, with damage scoped to the animated rects.

## Why

Today every animation routes through the user's record closure:
the widget computes a phase, conditionally pushes a shape, schedules
the next wake via `Ui::request_repaint_after`. On wake the host calls
`Host::run_frame` → `Ui::frame` → `record_pass` → `pre_record`
(clears the tree) → user record → `post_record` (hashes + measure +
arrange + cascade) → encode → paint.

`MeasureCache` makes measure-on-stable-subtree cheap (subtree hash hit
blits last frame's result) and `Damage::Partial` keeps paint scoped,
but the *record walk itself* still runs, and the user's record closure
runs unconditionally. For a 2 Hz caret blink this is fine. For a
60 Hz spinner, a focus pulse, or a multi-widget marquee strip, we
spend the full CPU pipeline 60 times per second to flip an opacity
bit or rotate a quad — work that has no structural effect on the
tree, layout, or hit index.

Paint-only frame mode is the optimization, in two parts:

1. **Centralize the animation contract** (slice 1). `PaintAnim` is a
   registration: "this shape's alpha/transform is a known function of
   `now`". Widget code stops doing phase math and wake scheduling —
   it just calls `add_shape_animated(shape, anim)` once per record.
   The encoder samples the function at paint time. This alone deletes
   meaningful boilerplate and makes the user closure cheap.

2. **Short-circuit downstream when the tree is hash-stable** (slice 2).
   After `post_record` computes subtree hashes, if every layer root's
   hash matches last full frame's snapshot, skip `cascades.run` and
   the finalize damage-diff. Compute damage from the paint-anim quantum
   diff instead. The user closure and `post_record` still run — that's
   the price of getting correctness from the hash instead of from
   manually-tracked dirty flags — but everything downstream that scales
   with tree size is skipped.

## Scope

In scope, paint-tick eligible:

- Caret blink (visibility flip on a rect).
- Spinner / busy indicator (rotation of a quad or polyline).
- Focus pulse / hover wash ease-out tail (alpha ramp on a chrome).
- Indeterminate progress marquee (horizontal translation of a rect
  through a clip).
- Shimmer placeholders (gradient phase along an axis).

Out of scope (must take the full record path):

- Any animation whose value drives layout (collapsing panel height,
  resizing column, growing tooltip). These change `Sizing` /
  `desired` and require measure.
- Cross-widget choreography that adds/removes nodes mid-animation.
- The existing `AnimMap` tween rows that drive widget *state* (hover
  fade colour, press scale) — those already settle within a few
  frames of record runs and aren't the steady-state cost.

The split is sharp: a `PaintAnim` registration is a contract that the
animation does not affect tree structure, layout, or hit index. Type
system enforces it: `PaintAnim` operates only on fill brush + per-quad
transform; nothing it can change feeds back into measure / arrange.

## Architecture

### Frame lifecycle today (record path)

```
Ui::frame(now)
 ├─ bump frame_id, fps_ema, drain wakes, mark frame_state Pending
 ├─ record_pass(A):
 │   ├─ pre_record  → forest.pre_record() clears every tree
 │   ├─ open viewport node + run user closure → fills tree, shapes
 │   ├─ post_record → hashes, MeasureCache, arrange, cascade
 ├─ (if action_flag || relayout): record_pass(B) same thing again
 ├─ finalize_frame:
 │   ├─ ids.rollover() → removed set
 │   ├─ sweep removed across text / layout / state / anim caches
 │   ├─ input.post_record(cascades)
 │   └─ damage_engine.compute(forest, cascades, removed, surface)
 └─ FrameReport { damage, repaint_after, … }
```

### Frame lifecycle proposed (post-record short-circuit)

```
Ui::frame(now)
 ├─ bump frame_id, fps_ema, drain wakes, mark frame_state Pending
 ├─ record_pass(A):
 │   ├─ pre_record  → forest.pre_record() clears every tree
 │   ├─ user closure runs (cheap on stable widgets; spinner/caret
 │   │  re-register their PaintAnim shape, no per-frame phase math)
 │   ├─ post_record → hashes, MeasureCache, arrange
 │
 ├─ if tree_unchanged_since_last_record():   ← NEW post-record check
 │   ├─ skip cascades.run        (reuse last frame's cascades)
 │   ├─ skip finalize_frame diff (no structural changes possible)
 │   ├─ damage_engine.compute_paint_anim_only(forest, now, surface)
 │   │     → DamageRegion of nodes whose quantized sample flipped
 │   └─ return FrameReport { damage, … }
 ├─ else: existing cascades.run + finalize_frame path
```

The check is post-record, not pre-record. After `post_record` computes
subtree hashes, comparing root subtree hashes against last record's
snapshot proves the tree is bit-identical — no enumeration of
"dirty sources" required. State mutation, input changes, and AnimMap
ticks all naturally surface as a different hash and fall through to
the full path. The user closure still runs (~µs at 200 widgets) but
that buys correctness derived from the existing hash plumbing instead
of manually-tracked dirty flags.

Short-circuit predicate (all must hold; any failure → full path):

1. **Tree hash-stable**: every layer's root `subtree_hash` matches
   last record's snapshot. This is the load-bearing condition; the
   others below are guards on state outside the tree.
2. **Display unchanged**: same `Display` as last frame's
   `damage_engine.prev_surface`. Resize or scale change forces full.
3. **Last frame submitted**: `frame_state.was_last_submitted()`.
4. **Not the first frame**: `damage_engine.prev_surface` is `Some`.
5. **At least one `PaintAnim` exists and its `next_wake ≤ now`**.
   Otherwise there's nothing for paint-tick to *do* — fall through
   so damage produces its usual "no changes" `None`.

No `state_dirty` flag, no `input_dirty_since_last_frame` flag, no
wake-kind tagging. The hash *is* the dirty bit.

### `PaintAnim` registry

```rust
// src/animation/paint.rs (new module)

#[derive(Clone, Copy, Debug)]
pub enum PaintAnim {
    /// Solid for `half_period_s`, hidden for the next `half_period_s`,
    /// repeating from `started_at`. Caret-blink shape.
    BlinkOpacity { half_period_s: f32, started_at: Duration },
    /// Rotate the shape around its centre at `rad_per_s`.
    Rotation { rad_per_s: f32, started_at: Duration },
    /// Sinusoidal alpha between `min` and `max` at `freq_hz`.
    Pulse { freq_hz: f32, min: f32, max: f32, started_at: Duration },
    /// Translate the shape along an axis cyclically through a window
    /// of width `span_px`. For indeterminate progress marquee.
    Marquee { px_per_s: f32, span_px: f32, axis: Axis2,
              started_at: Duration },
}

pub(crate) struct PaintMod {
    pub(crate) alpha: f32,                  // multiplies fill α
    pub(crate) transform: TranslateScale,   // composes with cascade
}

impl PaintAnim {
    pub(crate) fn sample(self, now: Duration) -> PaintMod { … }

    /// Quantized state for change-detection. Returns a small integer
    /// that flips iff `sample(now)` would produce a visually-different
    /// output than `sample(prev)`. `BlinkOpacity` → 0|1 (the visibility
    /// bit). `Rotation` → `(angle * QUANTUM_INV).round() as i32`
    /// (sub-pixel rotation steps collapse). `Pulse` → α-bucket. Drives
    /// damage's "is this node dirty" check without a full hash.
    pub(crate) fn quantum(self, now: Duration) -> i32 { … }

    /// Earliest `Duration` from `now` at which `quantum` will next
    /// change. Folded across all live entries to set the next wake.
    pub(crate) fn next_wake(self, now: Duration) -> Duration { … }
}
```

Storage on `Tree`:

```rust
// src/forest/tree/mod.rs
//
// Shape-indexed (not node-indexed) because one node can have an
// animated caret rect AND a static chrome on the same node. The
// existing sparse-extras pattern (`ExtrasIdx` + `bounds_table` /
// `panel_table` / `chrome_table`) is node-keyed and doesn't fit;
// this lives next to `tree.shapes` instead.
pub(crate) paint_anims: Vec<PaintAnimEntry>,
//   pushed by `add_shape_animated` in parallel with the matching
//   `tree.shapes.records.push(...)`; cleared in `pre_record` like
//   every other per-frame column. Each entry carries the shape's
//   index so post-record / paint-tick can look up `paint_rect`.

struct PaintAnimEntry {
    anim: PaintAnim,
    shape_idx: u32,        // into `tree.shapes.records`
    node: NodeId,          // for damage rect lookup
    last_quantum: i32,     // updated in paint-tick / record post_record
}
```

For O(1) lookup at encoder-emit time (item 6 below), add a parallel
`paint_anim_by_shape: Vec<u16>` (one slot per shape, `u16::MAX` for
"no anim", same niche convention as `ExtrasIdx::Slot`). Cleared and
grown alongside `shapes.records`. Reads at the per-shape emit site
are one indexed load + a branch on the sentinel.

Lifecycle:

- **Record path**: `Ui::add_shape_animated(shape, anim)` pushes the
  shape into `tree.shapes` and registers a `PaintAnimEntry` against
  the freshly-allocated shape index. `pre_record` clears the column
  like every other tree column. `post_record` initializes
  `last_quantum` to `anim.quantum(now)` and folds `next_wake` into
  `Ui::repaint_wakes` automatically (so widgets stop calling
  `request_repaint_after` themselves for these shapes).

- **Paint-tick path**: column survives untouched (we don't run
  `pre_record`). `paint_anim_damage` walks it, compares `quantum(now)`
  vs `last_quantum`, updates `last_quantum`, and adds the node's
  `paint_rect` to the dirty region for each changed entry. Then
  schedules the next wake from the min of all `next_wake`s.

### Encoder integration

`encode_node` currently calls `out.draw_rect(rect, radius, &fill,
stroke)` and analogues for text / polyline / mesh. Wrap the emit
sites so the per-shape paint-mod is applied:

```rust
let slot = tree.paint_anim_by_shape[shape_idx];
let mod_ = if slot != u16::MAX {
    tree.paint_anims[slot as usize].anim.sample(now)
} else {
    PaintMod::identity()
};
let fill = brush.with_alpha_mul(mod_.alpha);
let xf = cascade.transform.then(mod_.transform);
out.draw_rect(xf.apply(rect), radius, &fill, stroke);
```

Identical surface and behaviour on the record path — the encoder
always applies whatever mod is registered, time-stamped at
`ui.time`. Paint-tick is just the record path with the record and
layout phases short-circuited.

### Damage on the short-circuit path

`DamageEngine` today diffs `(WidgetId → NodeSnapshot{rect, hash})`
against the current tree, evicts `removed`, and folds rects into a
budgeted `DamageRegion`. On the short-circuit path the structural
diff is redundant — hashes are unchanged by definition.

New entry point:

```rust
impl DamageEngine {
    /// Paint-anim-only damage. Does not touch `self.prev` (the
    /// per-widget snapshots remain valid for the next full frame).
    /// Walks `tree.paint_anims`, compares each entry's quantum
    /// against `now`, accumulates the node's `paint_rect` for each
    /// changed entry, and returns the standard
    /// `Some(Partial(region)) | Some(Full) | None` envelope.
    pub(crate) fn compute_paint_anim_only(
        &mut self,
        forest: &Forest,
        cascades: &Cascades,
        now: Duration,
        surface: Rect,
    ) -> Option<Damage> { … }
}
```

`self.prev` deliberately stays put: when the next *structurally*
different frame runs, its per-widget hash diff still has last full
frame's snapshot to compare against — paint-anim-only frames are
*visible* changes but not *structural* changes.

Cascades are also reused as-is — the short-circuit predicate proves
they would be recomputed identically. The encoder reads the same
`Cascades` it consumed last frame; lifetime is fine because cascades
live on `Ui` across frames already.

### Wake scheduling

Today `request_repaint_after(after)` inserts a `Duration` deadline
into a sorted vec, drained at the top of each `frame`. Widgets call
this in their record closure when they want the next blink phase.

New: `post_record` folds `next_wake` for every live `PaintAnim` into
`repaint_wakes` automatically. Widgets calling `add_shape_animated`
no longer call `request_repaint_after`. No wake-kind tagging
needed — every wake routes through the same record-then-check path.
`post_record` will be required to take `Duration now` (it currently
takes no args).

### What we actually skip — per-frame CPU comparison

Record-path frame, idle UI with one spinner:

| Pass             | Work                                              |
|------------------|---------------------------------------------------|
| `pre_record`     | clear ~8 tree columns × 1 root tree               |
| user closure     | full widget walk; spinner widget re-registers its `PaintAnim` shape (no phase math, no manual wake) |
| `post_record`    | `compute_node_hashes` + `compute_subtree_hashes`  |
| `layout.run`     | `MeasureCache` root-hit blits cached subtree      |
| `cascades.run`   | walk tree, fold transform / clip / disabled       |
| `finalize_frame` | rollover, sweep, input.post_record, damage diff   |
| encode + paint   | one rect quad goes to the GPU                     |

Post-record short-circuit, same scene:

| Pass                       | Work                                   |
|----------------------------|----------------------------------------|
| `pre_record`               | clear ~8 tree columns                  |
| user closure               | full widget walk, no phase math        |
| `post_record`              | hashes (paid; this is what we check)   |
| `layout.run`               | `MeasureCache` root-hit, O(1)          |
| (skip cascades + finalize damage-diff)                              |
| `compute_paint_anim_only`  | one `SparseColumn` walk (1 entry here) |
| encode + paint             | one rect quad goes to the GPU          |

The win is more modest than a pre-record gate would deliver — we pay
the user closure + hashing — but the closure is already cheap when
PaintAnim absorbs the phase math, and hashing is what makes the
short-circuit *correct* without flag bookkeeping. Bench (item #10
below) decides whether the remaining cascades + finalize savings
justify slice 2 at all.

## Restructuring required

Most of the existing code stays untouched. The changes:

1. **`src/animation/paint.rs`** (new). `PaintAnim` + sampling +
   quantization + wake math. ~150 LOC.

2. **`src/forest/tree/mod.rs`**. Add `paint_anims: Vec<PaintAnimEntry>`
   + parallel `paint_anim_by_shape: Vec<u16>` (sentinel `u16::MAX` =
   "no anim", same convention as `ExtrasIdx::Slot::ABSENT`) to
   `Tree`. Clear in `pre_record` alongside the other per-frame
   columns. Initialize `last_quantum` and fold `next_wake` in
   `post_record` (needs `now` threaded in — currently
   `Tree::post_record` / `Forest::post_record` take `&mut self` only,
   would take `Duration`; `Ui::frame_inner` already has `self.time`
   handy at the call site).

3. **`src/forest/mod.rs`**. `Forest::add_shape_animated(shape,
   anim)` mirrors `add_shape`, pushes the shape, registers in the
   active tree's `paint_anims`.

4. **`src/ui/mod.rs`**.
   - New private `tree_unchanged_since_last_record(&self) -> bool`:
     compares every layer root's `subtree_hash` against last full
     frame's snapshot. Caches the snapshot on `Ui` after each full
     frame.
   - New private `paint_anim_only_pass(&mut self) -> FrameReport`:
     skips `cascades.run` + finalize damage-diff, calls
     `compute_paint_anim_only`, sweeps nothing (no `removed`).
   - `frame` branches *after* `record_pass`, before `cascades.run`,
     on the predicate + the four guards (display, last submitted,
     prev_surface present, at least one anim wake due).
   - `add_shape_animated(&mut self, shape, anim)` public method.
   - No new dirty flags. No wake-kind tagging.

5. **`src/ui/damage/mod.rs`**. Add `compute_paint_anim_only` (parallel
   to `compute`). Does not touch `self.prev`. Returns the same
   `Option<Damage>` envelope.

6. **`src/renderer/frontend/encoder/mod.rs`**. At each shape emit
   site (rect / text / polyline / mesh / shadow), index
   `tree.paint_anim_by_shape[shape_idx]`, branch on the sentinel,
   on a hit fetch `tree.paint_anims[slot]`, sample, apply `alpha`
   to the brush, compose `transform` with the cascade. `encode`
   takes `&Ui` so `ui.time` is reachable, but `encode_node` (the
   per-node callee at line 252) currently takes
   `(&Tree, &LayerLayout, &Cascades, …)` — thread `Duration now`
   in alongside.

7. **`src/host.rs`**. No changes — `Host::run_frame` already only
   knows about `FrameReport`, and `report.skip_render()` already
   handles the "damage region empty" case. Paint-tick that produces
   no dirty rects falls naturally into the skip path.

8. **Widget migration** (not in slice 1):
   - `text_edit` caret: drop `caret_visible` branch + manual wake,
     use `add_shape_animated(caret_rect,
     PaintAnim::BlinkOpacity { half_period_s: 0.5, started_at:
     state.last_caret_change })`.
   - New `Spinner` widget as second consumer, with showcase tab.

## Testing & pinning

Each phase pinned by a test before moving on. All in `lib.rs` /
`src/animation/paint/tests.rs` style — no integration test detour.

1. `paint_anim_quantum_flips_on_period_boundary` — sampling math.
2. `paint_anim_next_wake_aligns_with_next_quantum` — wake math.
3. `add_shape_animated_registers_in_tree_column` — record-path
   plumbing.
4. `paint_anim_only_runs_when_tree_hash_stable` — end-to-end via a
   headless `Host`: full frame then second frame with only `now`
   advancing; assert cascades.run + finalize damage-diff did not
   run (sentinel counter or trace hook), assert damage region
   equals exactly the animated rect.
5. `paint_anim_only_falls_through_on_hover_change` — pointer move
   that changes hover bumps the chrome subtree hash → full path.
6. `paint_anim_only_falls_through_when_anim_in_flight` — an
   `AnimMap` tween changes a colour read in the record closure →
   bumps node hash → full path.
7. `paint_anim_only_falls_through_on_first_frame` — `prev_surface`
   `None`.
8. `paint_anim_only_caret_blink_matches_record_path_pixels` — two
   scenes, one running cascades + damage every frame, one with the
   short-circuit; assert the composer output is byte-identical at
   each `now`.
9. Bench: `benches/paint_tick.rs` — 200-widget steady scene with one
   spinner at 60 Hz. Compare frame time:
   (a) full record path every frame,
   (b) short-circuit path,
   (c) hypothetical pre-record gate (manual flags, for reference).
   This bench is the slice-2 gating decision: if (b) vs (a) is
   <0.5ms savings, defer or drop slice 2.

## Risks and open questions

- **Quantization granularity for rotation.** A spinner rotating at
  `2π rad/s` quantized to `2π / 64` steps changes quantum every
  ~15 ms — i.e. matches a 60 Hz display. Below 60 Hz the damage
  budget naturally clamps. Above 60 Hz quantization aliases.
  Probably fine; revisit if smooth-rotation widgets complain.

- **Frame-time correctness when `dt` is irregular.** Paint-tick reads
  `ui.time` directly; sampling math uses `now - started_at`. No
  accumulator state. Robust to dropped frames and host-driven late
  wakes.

- **Profiler frame markers.** Tracy `non_continuous_frame!("frame")`
  brackets `Host::frame_and_render`. Paint-tick frames still produce
  a frame marker, just a thinner one. No special handling.

- **Debug overlay frame_stats counter.** The "rendered N nodes" stat
  is recorded inside the user closure. On the short-circuit path the
  closure *does* run, so the readout stays live for free. (This is
  one of the small wins of post-record vs. pre-record gating.)

- **Multi-layer popups containing animations.** `Forest` has one
  `Tree` per `Layer`. A `PaintAnim` registered in the `Popup` layer
  is in `forest.trees[Popup as usize].paint_anims`. Paint-tick
  iterates layers in `Layer::PAINT_ORDER` same as the record path.
  No special handling. Tested via `paint_tick_in_popup_layer`.

- **Removing a paint-anim shape.** With the post-record approach,
  this is a non-issue: removing a shape changes the owner node's
  subtree hash, bumping the tree out of hash-stable state and forcing
  the full path on that frame. No stale animation possible.

- **Snap-after-quiescence.** When the only paint-anim settles (e.g.
  caret blink stops after `BLINK_STOP_AFTER_IDLE`), `next_wake`
  returns `Duration::MAX` (or similar) for that entry. If every
  entry's wake is "never", `post_record` schedules no anim wake →
  paint-tick frames stop arriving → idle. Correct.

## Slicing

Land in two slices. Slice 1 stands on its own; slice 2 is gated on
the bench result from slice 1.

**Slice 1: `PaintAnim` registry on the existing record path.**
Phases 1–3, 6 from "Restructuring required" land plus widget
migration. `Ui::frame` always takes the full path. Encoder applies
paint-mods at each emit site. `post_record` folds `next_wake` into
`repaint_wakes`. `text_edit` caret and a new `Spinner` widget
migrate to `add_shape_animated`, dropping their manual phase math
and wake scheduling.

User-visible: identical to today. Internal: paint-anim registry
exists, encoder sees it, sampling produces correct quantums, wake
folding works, all on the record path. Tests 1–3 and 8 (caret-blink
pixel parity) pass. Item 9 (bench) lands too — both axes (a) and (b)
just measure the full record path against itself initially; the
slice 2 axis (c) is wired but inactive.

Slice 1 delivers most of the value the doc cares about: declarative
animation API, centralized wake folding, deletion of widget-side
phase boilerplate.

**Slice 2: post-record short-circuit.**
Phase 4 (the `tree_unchanged_since_last_record` predicate +
`paint_anim_only_pass` branch in `Ui::frame`) and phase 5
(`compute_paint_anim_only`) land. Tests 4–7 added.

**Slice 2 is gated.** Run the bench from slice 1 first. If the
short-circuit path doesn't save >0.5 ms on the 200-widget + spinner
scene, defer or drop slice 2 — `MeasureCache` already amortizes
measure, and the remaining cascades + finalize cost may not justify
the added branch and snapshot bookkeeping. If the bench says ship,
land slice 2 as a clean follow-up; backout is one revert.

## Why this fits Palantir's posture

- Per-frame allocation: zero. `paint_anims` is a `SparseColumn`
  with retained capacity, same lifetime story as every other tree
  column. Paint-tick reuses encoder + composer + GPU buffers
  already shaped for the record path.
- API surface: one new method (`add_shape_animated`) + one new
  enum (`PaintAnim`). Caret blink and spinner become declarative.
- Code health: cuts widget-side boilerplate (phase math, wake
  scheduling). Animation logic centralized; widget code reads as
  "this shape blinks" instead of "if visible push, then reschedule
  next wake at …".
- Ship in measurable slices: slice 1 lands the registry on the
  existing record path, slice 2 unlocks the optimization. Each
  slice is independently useful and independently testable.
