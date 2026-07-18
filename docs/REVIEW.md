# Aperture remaining design, performance, and consolidation review

Reviewed 2026-07-17; completed findings pruned 2026-07-18.

## Scope

The original review covered every production Rust and WGSL file under `src/`,
the animation derive crate, `AGENTS.md`, `README.md`, the animation and layout
design notes, and the current CPU profile. Tests and benchmarks were read only
where needed to understand an invariant or prescribe validation; they were not
reviewed as production modules.

Completed findings have been removed after checking the current code. Seven
findings remain:

- prototype narrower cascade invalidation without weakening correctness;
- consolidate five duplicated or unnecessarily repeated policies;
- decide how Grid Fill tracks contribute to the Grid's intrinsic size.

The batches below remain ordered by priority and can be implemented and
validated independently.

## Batch 1 — Cache and frame-lifecycle improvement

- [ ] **Prototype incremental cascade invalidation rather than weakening the
  global fingerprint.** `cascade_fingerprint` folds each root's full subtree
  authoring hash at `src/ui/cascade/mod.rs:494-534`, so any paint-only content
  change reruns the complete cascade at `src/ui/mod.rs:532-555`. The current
  profile measures cascade self-time around 37-39 microseconds in partial,
  scrolling, and resizing frames
  (`docs/frame-cpu-profile-2026-07-17.md:98-117`). Separate stable
  geometry/ancestor-state columns from paint-row refresh, or invalidate
  subtrees and repair ancestor paint bounds. Do not simply omit paint from the
  current fingerprint: that would reuse stale paint arenas and cascade hashes.
  Validate exact equivalence against a forced-full cascade over transform,
  clip, visibility, scroll, side-layer, reorder, and paint-only mutations, then
  benchmark partial and scrolling arms.

## Batch 2 — Consolidate duplicated policies

- [ ] **Make direct-text ordinal assignment a single source of truth.**
  `TextShapeInput` and its shared iterators do not carry an ordinal
  (`src/layout/support.rs:35-115`), so intrinsic sizing assigns and
  overflow-checks `(WidgetId, ordinal)` at
  `src/layout/intrinsic.rs:174-232` while normal shaping independently repeats
  the counter and overflow policy at `src/layout/engine.rs:758-777`. Both feed
  the same identity cache and must never drift. Have the shared iterator yield
  the checked ordinal with `TextShapeInput`, then consume it in both paths.
  Validate multiple direct text runs interleaved with non-text shapes and the
  ordinal overflow boundary.

- [ ] **Consolidate raw RGBA8 ownership and validation.** `Image` and
  `WindowIcon` independently store straight-alpha RGBA8 dimensions and bytes,
  with separate validators at `src/primitives/image.rs:58-93` and
  `src/window.rs:60-87`. Both expose fields publicly, so callers can bypass the
  constructors; both compute `width * height * 4` without checked
  multiplication. `WindowIcon` also permits zero dimensions, after which winit
  silently drops the malformed icon at `src/host/winit/mod.rs:530-536`.
  Introduce a shared invariant-bearing RGBA8 pixel buffer with non-zero
  dimensions and checked length arithmetic; make Image and WindowIcon thin
  semantic wrappers. Validate zero dimensions, arithmetic overflow, wrong
  length, valid image upload, and valid platform-icon conversion.

- [ ] **Route polyline width through the canonical scalar no-op predicate.**
  `DrawPolylinePayload::is_noop` hand-rolls `width <= 0.0` at
  `src/renderer/frontend/cmd_buffer/payload.rs:284-296`, while neighboring
  stroke payloads use `noop_f32` at
  `src/renderer/frontend/cmd_buffer/payload.rs:490-550` and authoring already
  uses the same canonical policy at `src/shape.rs:784-799`. The duplicate
  disagrees for NaN and sub-EPS positive widths, allowing an internally
  malformed payload into composer and shader math. Replace the comparison with
  `noop_f32(self.width)` and extend the payload gate table with NaN, sub-EPS,
  zero, and positive widths.

- [ ] **Build Stack planning data in one child walk without unifying the
  Stack/Grid solvers.** `stack_plan` walks every active child for counts,
  weights, and non-Fill sums at `src/layout/stack/mod.rs:141-179`;
  `push_fill_entries` immediately walks them again at
  `src/layout/stack/mod.rs:106-138`. Both measure and arrange pay the duplicate
  traversal at `src/layout/stack/mod.rs:210-270,338-365`. Populate the
  `StackPlan` and Fill scratch slice together in one pass, leaving
  `freeze_distribute` and the documented Stack/Grid freeze-cadence divergence
  at `src/layout/stack/mod.rs:45-57` untouched. Validate every Stack sizing
  mode and benchmark wide/deep stacks.

- [ ] **Add a paired intrinsic query for Grid Hug cells.** Every span-1
  Hug-column cell requests `MinContent` and `MaxContent` back-to-back at
  `src/layout/grid/mod.rs:397-420`. On a cold subtree these are separate
  recursive walks; at a text leaf they reach the same unbounded shaping input
  and select two metrics from the same result at
  `src/layout/intrinsic.rs:174-228`. Add a targeted `intrinsic_range` query that
  fills both per-node cache slots in one recursion while retaining the
  single-slot API for Stack's min-only case. Validate exact equivalence for all
  layout drivers, inspect intrinsic compute counts, and compare forced-miss and
  resize benchmarks before keeping the added API.

## Open design decision

- [ ] **Decide whether Grid Fill tracks contribute their content floor to the
  Grid's own intrinsic size.** The design note says Fill contributes content
  intrinsic while ignoring weight (`src/layout/intrinsic.md:67-74`). Grid
  measure follows that floor policy at `src/layout/grid/mod.rs:397-429,889-914`,
  but `grid::intrinsic` contributes only `Track.min` and skips non-Hug cells at
  `src/layout/grid/mod.rs:951-1019`. An ancestor Stack can therefore allocate
  from a zero Grid floor, after which the Grid discovers a rigid Fill-cell
  floor and overflows rather than letting a shrinkable sibling surrender
  space. Choose and document the intended semantics before changing code. Pin
  the decision with a Fill Grid track containing a Fixed descendant, both as a
  Hug root and as one of several Stack Fill siblings.

## Tempting changes intentionally excluded

- Do not fuse widget-ID resolution with endpoint reservation or fuse shape
  lowering with hashing. Both were implemented and benchmarked as regressions;
  see `docs/frame-cpu-profile-2026-07-17.md:119-184`.
- Do not enable a project-wide x86-64-v3 target. It measured faster but violates
  the supported CPU baseline; see
  `docs/frame-cpu-profile-2026-07-17.md:86-96`.
- Do not merge the Stack and Grid Fill solvers without first changing their
  documented semantics. Their freeze cadence is intentionally different.
- Do not consolidate the composer's geometry-to-scissor conversion with the
  backend's damage-to-scissor conversion. They deliberately differ in snapping,
  outward rounding, and antialias padding.
- Cargo dependency analysis found no unused Aperture dependencies.
