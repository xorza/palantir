# Paint-only frame mode (a.k.a. "paint tick")

**Status:** proposal. No code yet.
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

Paint-only frame mode is the optimization: detect "nothing structural
changed since last frame, the only reason we're awake is a
time-driven paint", skip record + measure + arrange + cascade
entirely, and run only the work that actually produces different
pixels.

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

### Frame lifecycle proposed (paint-tick path)

```
Ui::frame(now)
 ├─ bump frame_id, fps_ema, drain wakes, mark frame_state Pending
 ├─ if can_paint_tick():        ← NEW eligibility check
 │   ├─ paint_tick:
 │   │   ├─ tree retained as-is (no pre_record, no record, no post_record)
 │   │   ├─ layout retained as-is (no layout_engine.run)
 │   │   ├─ cascades retained as-is (no cascades_engine.run)
 │   │   ├─ paint_anim_damage(forest, paint_anims, prev_now, now)
 │   │   │     → DamageRegion of nodes whose quantized sample flipped
 │   └─ return FrameReport { damage, … } (skip if region empty)
 ├─ else: existing record_pass(A) [+ B] + finalize_frame path
```

Eligibility (all must hold; any failure → fall through to record path):

1. **No structural request from caller**: `repaint_requested` and
   `relayout_requested` both false coming into the frame.
2. **No input-driven repaint pending**: every `on_input` call since
   the last `frame` returned `InputDelta::requests_repaint == false`.
   This is the existing flag — we OR them into a new
   `input_dirty_since_last_frame` bit on `InputState`, cleared at the
   top of each `frame`.
3. **No state mutation that affects paint**: `StateMap` mutations go
   through `Ui::state_mut`, which today doesn't flag a redraw — we
   add a `state_dirty` bit set on any `state_mut` borrow, cleared at
   `frame` top. (Cheap and conservative: a `state_mut` borrow
   doesn't always mutate, but the common case is mutation, and a
   spurious record-path frame is correctness-safe.)
4. **Display unchanged**: same `Display` as last frame's
   `damage_engine.prev_surface`. Resize or scale change forces full.
5. **Last frame submitted**: `frame_state.was_last_submitted()`.
   The host failed-present invalidation already lives here.
6. **First frame**: no — `damage_engine.prev_surface` is `None`,
   forces full anyway.
7. **`AnimMap` quiescent**: every typed map's rows are `settled ==
   true`. An in-flight tween changes value-tweened paint, which
   today requires record (the widget reads `ui.animate(...)` in
   its record closure). Paint-tick can't observe `AnimMap` reads
   because the user closure doesn't run.
8. **At least one `PaintAnim` exists and its `next_wake ≤ now`**.
   Otherwise there's nothing for paint-tick to *do* — the wake came
   from somewhere else and the safe response is a record pass.

If all conditions hold, we take the paint-tick branch. Otherwise the
record path, exactly as today.

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
pub(crate) paint_anims: SparseColumn<PaintAnimEntry>,
//   keyed by shape-index (offset into tree.shapes.records),
//   so one node can have an animated caret rect and a static
//   chrome on the same node.

struct PaintAnimEntry {
    anim: PaintAnim,
    node: NodeId,          // for damage rect lookup
    last_quantum: i32,     // updated in paint-tick / record post_record
}
```

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
let mod_ = tree.paint_anims
    .get_for_shape(shape_idx)
    .map(|e| e.anim.sample(ui.time))
    .unwrap_or(PaintMod::identity());
let fill = brush.with_alpha_mul(mod_.alpha);
let xf = cascade.transform.then(mod_.transform);
out.draw_rect(xf.apply(rect), radius, &fill, stroke);
```

Identical surface and behaviour on the record path — the encoder
always applies whatever mod is registered, time-stamped at
`ui.time`. Paint-tick is just the record path with the record and
layout phases short-circuited.

### Damage on paint-tick

`DamageEngine` today diffs `(WidgetId → NodeSnapshot{rect, hash})`
against the current tree, evicts `removed`, and folds rects into a
budgeted `DamageRegion`. On paint-tick this whole walk is wrong —
the tree and hashes are unchanged from last frame.

New entry point:

```rust
impl DamageEngine {
    /// Paint-tick damage. Does not touch `self.prev` (the tree's
    /// per-widget snapshots remain valid for the next record frame).
    /// Walks `tree.paint_anims`, compares each entry's quantum
    /// against `now`, accumulates the node's `paint_rect` for each
    /// changed entry, and returns the standard
    /// `Some(Partial(region)) | Some(Full) | None` envelope.
    pub(crate) fn compute_paint_tick(
        &mut self,
        forest: &Forest,
        cascades: &Cascades,
        now: Duration,
        surface: Rect,
    ) -> Option<Damage> { … }
}
```

`self.prev` deliberately stays put: when the next record frame runs,
its per-widget hash diff still has last record's snapshot to
compare against — the paint-tick frames are *visible* changes but
not *structural* changes, and the record-path damage logic is keyed
on structural state.

### Wake scheduling

Today `request_repaint_after(after)` inserts a `Duration` deadline
into a sorted vec, drained at the top of each `frame`. Widgets call
this in their record closure when they want the next blink phase.

New: `post_record` (the record path) folds `next_wake` for every
live `PaintAnim` into `repaint_wakes` automatically. Widgets calling
`add_shape_animated` no longer call `request_repaint_after`.

The paint-tick path also folds `next_wake` from its post-paint
state — so a frame whose only purpose is animating a spinner schedules
the next spinner step itself, without ever re-entering record.

To distinguish "paint-tick eligible" wakes from regular wakes, tag
the queue: replace `Vec<Duration>` with
`Vec<(Duration, RepaintKind)>` where `RepaintKind ∈ { Anim, User }`.
Eligibility check #6 ("only paint-anim wakes are due") reads the
tag of the wakes that fired at the top of `frame`. A user wake (from
explicit `request_repaint_after`) always forces the record path.

### Input between frames

`Ui::on_input` is called by the host between `frame` calls. It runs
hit-test against last frame's cascades and updates hover/focus state
synchronously. `InputDelta::requests_repaint` already signals
whether the visible output is affected (e.g. a pointer move that
changes hover target sets it; a move over inert pixels does not).

Eligibility condition #2 OR-folds every `InputDelta::requests_repaint`
since the last frame; if any was true, paint-tick is disqualified.

Edge case: pointer entered a `:hover`-styled widget between paint-tick
frames. `InputDelta::requests_repaint` will be true → eligibility
fails → record path runs → hover wash paints. ✓

### State / animation / text caches

All cross-frame caches survive a paint-tick frame untouched, because
`finalize_frame`'s `removed` sweep is the only thing that touches
them and removed is empty when no record ran:

- `StateMap` — keyed by `WidgetId`, swept by `removed`. No record →
  no `SeenIds` rollover → `removed` empty → nothing swept. ✓
- `AnimMap` — same. Plus eligibility #7 forbids in-flight tweens, so
  there's no `tick` to run mid-paint-tick anyway. ✓
- `TextShaper` — same. Glyph runs already cached per `(WidgetId,
  ordinal)`; encode reads them via `Layout::text_shapes` for the
  retained nodes. ✓
- `MeasureCache` — not consulted on paint-tick. Stays warm for the
  next record frame. ✓

### What we actually skip — per-frame CPU comparison

Record-path frame, idle UI with one spinner:

| Pass             | Work                                              |
|------------------|---------------------------------------------------|
| `pre_record`     | clear ~12 tree columns × 1 root tree              |
| user closure     | full widget walk; spinner widget pushes its shape |
| `post_record`    | `compute_node_hashes` + `compute_subtree_hashes`  |
| `layout.run`     | `MeasureCache` hits but walks tree                |
| `cascades.run`   | walk tree, fold transform / clip / disabled       |
| `finalize_frame` | rollover, sweep, input.post_record, damage diff   |
| encode + paint   | one rect quad goes to the GPU                     |

Paint-tick frame, same scene:

| Pass                | Work                                          |
|---------------------|-----------------------------------------------|
| (skip pre/record/post/layout/cascades/finalize)            |
| `paint_anim_damage` | one `SparseColumn` walk (1 entry here)        |
| encode + paint      | one rect quad goes to the GPU                 |

For a real scene (say 200 widgets, one spinner), the skipped passes
are all O(200) walks. Paint-tick is O(1) in animated-shape count.

## Restructuring required

Most of the existing code stays untouched. The changes:

1. **`src/animation/paint.rs`** (new). `PaintAnim` + sampling +
   quantization + wake math. ~150 LOC.

2. **`src/forest/tree/mod.rs`**. Add `paint_anims: SparseColumn<…>`
   to `Tree`. Clear in `pre_record` alongside the other columns.
   Initialize `last_quantum` and fold `next_wake` in `post_record`
   (needs `now` threaded in — currently `post_record` takes no
   args, would take `Duration`).

3. **`src/forest/mod.rs`**. `Forest::add_shape_animated(shape,
   anim)` mirrors `add_shape`, pushes the shape, registers in the
   active tree's `paint_anims`.

4. **`src/ui/mod.rs`**.
   - Add `state_dirty: bool` flag, set by `state_mut`, cleared at
     `frame` top.
   - Add `input_dirty_since_last_frame: bool`, OR'd from each
     `on_input` delta, cleared at `frame` top.
   - Tag `repaint_wakes` entries with `RepaintKind`.
   - New private `can_paint_tick(&self) -> bool` implementing the
     eligibility list.
   - New private `paint_tick(&mut self) -> FrameReport`.
   - `frame` branches on `can_paint_tick()` before
     `record_pass`.
   - `add_shape_animated(&mut self, shape, anim)` public method.

5. **`src/ui/damage/mod.rs`**. Add `compute_paint_tick` (parallel to
   `compute`). Does not touch `self.prev`. Returns the same
   `Option<Damage>` envelope.

6. **`src/renderer/frontend/encoder/mod.rs`**. At each shape emit
   site (rect / text / polyline / mesh / shadow), look up
   `tree.paint_anims.get_for_shape(shape_idx)`, sample, apply
   `alpha` to the brush, compose `transform` with the cascade.
   `now` already reachable as `ui.time`.

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
4. `paint_tick_runs_when_eligible_and_only_paints_animated_rect` —
   end-to-end via a headless `Host`: full frame then paint-tick;
   assert no record closure invocation, assert damage region equals
   exactly the animated rect.
5. `paint_tick_skips_when_input_changed_hover` — eligibility gate.
6. `paint_tick_skips_when_anim_in_flight` — `AnimMap` quiescence.
7. `paint_tick_skips_when_user_request_repaint_pending` — wake-kind
   tag.
8. `paint_tick_falls_through_on_first_frame` — `prev_surface`
   `None`.
9. `paint_tick_caret_blink_matches_record_path_pixels` — two scenes,
   one running record every frame, one with paint-tick, assert the
   composer output is byte-identical at each `now`.
10. Bench: `benches/paint_tick.rs` — 200-widget steady scene with one
    spinner at 60 Hz. Compare frame time record-path-only vs
    record-once-then-paint-tick.

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
  is recorded inside the user closure today. On paint-tick the
  closure doesn't run → the overlay would stop updating. Either
  paint the last-recorded readout (acceptable, the underlying
  numbers haven't changed) or fall out of paint-tick when frame_stats
  is enabled (acceptable, it's a debug mode). Take option 1.

- **Multi-layer popups containing animations.** `Forest` has one
  `Tree` per `Layer`. A `PaintAnim` registered in the `Popup` layer
  is in `forest.trees[Popup as usize].paint_anims`. Paint-tick
  iterates layers in `Layer::PAINT_ORDER` same as the record path.
  No special handling. Tested via `paint_tick_in_popup_layer`.

- **Removing a paint-anim shape.** A paint-anim disappears when the
  caller stops calling `add_shape_animated` — i.e. on the *next*
  record frame. Until then paint-tick keeps animating it. This is
  the same semantics as any other shape on the tree, so no
  surprise. The disappearance happens during a record frame, which
  will produce its own damage diff.

- **Snap-after-quiescence.** When the only paint-anim settles (e.g.
  caret blink stops after `BLINK_STOP_AFTER_IDLE`), `next_wake`
  returns `Duration::MAX` (or similar) for that entry. If every
  entry's wake is "never", `post_record` schedules no anim wake →
  paint-tick frames stop arriving → idle. Correct.

## Slicing

Land in two slices to keep PRs reviewable.

**Slice 1: `PaintAnim` on record path only.**
Phases 1–6 from "Restructuring required" land. `Ui::frame` still
always takes the record path; eligibility check and paint-tick
method are stubs (return false / unreachable). Encoder applies
paint-mods. `text_edit` caret + a `Spinner` widget migrate.

User-visible: identical to today. Internal: paint-anim registry
exists, encoder sees it, sampling produces correct quantums, wake
folding works, all on the record path. Tests 1–3, 9 from above
pass.

**Slice 2: paint-tick fast path.**
Eligibility check goes live, `paint_tick` method runs, damage's
`compute_paint_tick` lands. Tests 4–8, 10 added.

Backout: revert slice 2 alone if profiling shows the eligibility
check itself eats more than it saves on record-path frames (it
won't — a half-dozen bool reads — but the cleanup is trivial).

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
