# Aperture remaining design, performance, and consolidation review

Reviewed 2026-07-17; completed findings pruned 2026-07-18.

## Scope

The original review covered every production Rust and WGSL file under `src/`,
the animation derive crate, `AGENTS.md`, `README.md`, the animation and layout
design notes, and the current CPU profile. Tests and benchmarks were read only
where needed to understand an invariant or prescribe validation; they were not
reviewed as production modules.

Completed findings have been removed after checking the current code. Four
findings remain:

- prototype narrower cascade invalidation without weakening correctness;
- consolidate two duplicated or unnecessarily repeated policies;
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

## Supplemental full-module review — 2026-07-18

This follow-up pass re-read every production Rust and WGSL file under `src/`,
the animation derive crate and manifests, the local architecture/design notes,
and the current review. Tests were consulted only to verify contracts and
prescribe regressions. The seven-item count above describes the earlier pruned
pass; the three findings below are additional. These supplemental batches are
ordered by priority and are independently implementable.

## Batch 5 — Medium: Reject malformed values at their owning boundary

- [ ] **Establish one text-metric invariant across theme loading, layout, and
  shaping.** `TextStyle` exposes raw `font_size_px` and `line_height_mult` and
  derives unrestricted deserialization at
  `src/widgets/theme/text_style.rs:14-40`; its helpers perform no validation at
  `src/widgets/theme/text_style.rs:65-97`. `Theme` validates only
  `text_scale`, mutates it before multiplying every stored size, and does not
  preflight overflow at `src/widgets/theme/mod.rs:100-157`. At the shaping
  boundary, `ShapeParams` is also raw at `src/text/mod.rs:170-184`: mono
  accepts every non-empty input and computes with negative/NaN metrics at
  `src/text/mod.rs:699-727`, while cosmic checks only
  `font_size_px <= 0.0` before quantization and `Metrics::new` at
  `src/text/cosmic.rs:53-55,311-335`; `Shape::is_noop` ignores both metrics at
  `src/shape.rs:827-834`. Introduce a named, invariant-bearing text-metrics
  value with font size and line height finite and above the UI epsilon, use it for
  deserialization and every mono/cosmic dispatch, and preflight all scaled
  styles before atomically updating a theme. Define finite wrap-width semantics
  separately. Table-test zero, negative, sub-EPS, NaN, and infinity across
  theme TOML, direct/reuse shaping, wrap/clip/ellipsis, and Text/TextEdit
  recording; theme input must fail deserialization and runtime shaping must
  return the exact invalid/no-command result without entering cache or renderer
  state.

- [ ] **Enforce the public `Mesh` index invariant while constructing the
  mesh.** `Mesh::vertex` truncates `usize` to `u32`, `triangle` accepts
  arbitrary indices, and `append` performs unchecked rebasing at
  `src/primitives/mesh.rs:125-158`. `is_noop` checks only vertex presence and
  triangle-count divisibility at `src/primitives/mesh.rs:102-107`, after which
  lowering copies malformed indices directly into the shared GPU payload at
  `src/forest/shapes/mod.rs:224-243`. Use checked vertex-index conversion,
  assert every triangle index is in range, and checked-add rebased indices in
  `append`, so authoring mistakes fail at their source rather than producing
  robust-access geometry. Validate each invalid triangle position, the largest
  valid boundary, exact append rebasing, and unchanged procedural
  vertices/indices/hashes/bounds.

## Batch 6 — Medium: Make snap-only animation explicit

- [ ] **Represent gradient brushes as snap-only spring fields instead of
  emulating vector arithmetic.** `Brush` claims every transition involving a
  gradient settles on the first spring tick at
  `src/primitives/brush/mod.rs:599-606`, but mismatched `sub` returns its left
  operand at `src/primitives/brush/mod.rs:626-630`. `spring::step` treats that
  operand as displacement at `src/animation/spring.rs:110-128`, and retargeting
  uses the same invalid algebra to decide whether velocity aids the new motion
  at `src/animation/mod.rs:386-403`. Gradient→Solid starts with zero-magnitude
  gradient displacement and snaps, while Solid→Gradient can spring the solid
  toward transparent for many frames before snapping; carried velocity makes
  the asymmetry worse. Add an explicit fieldwise spring-normalization hook to
  `Animatable` and its derive: Solid/Solid retains color math, while any pair
  involving a gradient installs the target brush with zero field velocity
  without stopping sibling fields in compound values. Validate both directions
  for all gradient variants, gradient→gradient, nonzero carried velocity,
  derived `Background`/look types, unchanged Solid/Solid trajectories, and no
  surplus repaint ticks after genuine fields settle.

## Targeted text-carrier consolidation review — 2026-07-18

This follow-up traced the production path from `InternedStr` authoring through
`RecordStore` normalization, `ShapeRecord`, layout, and encoding, then checked
Aperture's manifest and its primary Darkroom consumer. The transient-label and
single-recorded-representation findings were completed on 2026-07-18:
text-taking widgets now defer borrowed/owned input into the active arena,
`InternedStr` is arena-only, every `RecordedText` is one private `(Span, hash)`,
and Darkroom's per-record scene projection stores arena handles directly.

## Text changes intentionally excluded

- Do not merge `InternedStr` and `RecordedText` into one `Rc`-owning carrier.
  Recorded shapes would then keep the active arena's strong count above one,
  forcing `clear_text` to rotate arenas every record pass
  (`src/record_store.rs:101-118`). The phase split is what lets recorded spans
  remain owner-free while escaped authoring handles retain their exact bytes.
