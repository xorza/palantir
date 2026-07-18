# Aperture design, performance, and consolidation review

Reviewed 2026-07-17.

## Scope

This review covered every production Rust and WGSL file under `src/`, the
animation derive crate, `AGENTS.md`, `README.md`, the animation and layout
design notes, and the current CPU profile. Tests and benchmarks were read only
where needed to understand an invariant or prescribe validation; they were not
reviewed as production modules.

The architecture is already unusually deliberate: retained frame data,
allocation-free steady state, paint damage, shaping, layout caches, and renderer
batching all have clear ownership and measured trade-offs. The best remaining
work is not a broad rewrite. It is concentrated in:

- release-mode invariant enforcement at API/serialization boundaries;
- coordinate and lifecycle ownership that currently crosses window/transform
  boundaries;
- several cheap hot-path operations that the existing design no longer needs;
- cache invalidation that is broader than the data dependency it protects;
- small duplicated policies that have already started to drift.

The batches below are ordered by priority. Each batch is intended to be
implemented and validated independently.

## Batch 1 — Release safety and visible correctness

- [x] **Represent full and partial repaint selection with distinct types.**
  `src/renderer/backend/mod.rs:421-435` documents that an empty scissor list
  under a partial plan must crash, but enforces this only with `debug_assert!`.
  In release, the empty list selects the full path at
  `src/renderer/backend/mod.rs:565-633`, clears the target, and replays a draw
  list that was culled for partial damage. That can erase undamaged pixels.
  Replace the overloaded empty-list convention with a named result such as
  `RepaintScissors::{Full, Partial(NonEmptyScissors)}` returned from
  `src/renderer/backend/viewport.rs:35-55`; constructing an empty `Partial`
  must panic before render-pass selection. Validate full and partial selection
  directly, including a regression proving an invalid partial conversion
  cannot reach `LoadOp::Clear`.

- [x] **Make widget interaction coordinates explicitly logical under ancestor
  transforms.** `ResponseState::pointer_local` is computed as a surface-space
  offset at `src/input/mod.rs:988`, while `ResponseState::transform` documents
  the required inverse conversion at `src/input/response.rs:250-262`.
  Slider, Splitter, Scroll, and DragValue mix surface-space offsets or drag
  deltas with logical geometry at `src/widgets/slider.rs:68-87`,
  `src/widgets/splitter/mod.rs:135-146`,
  `src/widgets/scroll/mod.rs:516-537,577-637`, and
  `src/widgets/drag_value/mod.rs:321-344`. TextEdit already performs the
  correct conversion at `src/widgets/text_edit/input.rs:104-122`. Add one
  canonical logical-coordinate surface on `ResponseState`/`Ui` for points and
  vectors, then migrate all built-in widgets. Validate Slider endpoints,
  Splitter minimum panes, Scroll pivots/thumbs/pages, and DragValue speed under
  0.5x, 1x, and 2x ancestor scales.

- [x] **Resolve overlay placement after measuring its current body.**
  `OverlayPosition::resolve` requests a settling pass only when no prior size
  exists (`src/widgets/overlay_position/mod.rs:66-104`). Popup supplies the
  previous response size (`src/widgets/popup/mod.rs:140-145`) and Tooltip
  retains `last_size` across frames and hides
  (`src/widgets/tooltip/mod.rs:170-202`). If content, theme, padding, or a size
  cap changes, the current frame uses the stale extent and schedules no
  follow-up, so an edge-adjacent overlay can remain clipped or incorrectly
  flipped until unrelated input. Move placement into a side-layer policy/node
  evaluated after child measurement, carrying an anchor rect, preferred side,
  and alignment. This also lets ComboBox stop reducing its trigger to a point
  at `src/widgets/combo_box.rs:116-125`. Validate dynamic grow/shrink with no
  later input at all four viewport edges, ComboBox alignment, and convergence
  without perpetual relayout.

- [x] **Make valid gradient stops an invariant-bearing value.**
  Constructors validate `2..=MAX_STOPS` through `collect_stops` at
  `src/primitives/brush.rs:193-201,270-280,337-385`, but all three gradient
  structs derive `Deserialize` and expose their raw `ArrayVec` publicly at
  `src/primitives/brush.rs:168-174,248-255,313-320`. A theme file or later
  mutation can therefore create zero or one stop; rendering discovers it only
  through the release assertion in `src/renderer/gradient_atlas/bake.rs:10-15`.
  Introduce a shared `GradientStops` newtype with private storage, custom
  deserialization, and one constructor used by Linear, Radial, and Conic
  gradients. Reject non-finite serialized offsets instead of silently
  quantizing NaN through `Stop::new` at `src/primitives/brush.rs:89-108`.
  Validate deserialization of 0, 1, 2, 8, and 9 stops plus non-finite offsets;
  valid gradients must retain the current inline, allocation-free storage.

- [x] **Enforce polyline color cardinality at the authoring boundary in
  release builds.** The public contract says `add_shape` uses a hard assertion
  (`src/shape.rs:504-519`), but `assert_matches` uses only
  `debug_assert_eq!` at `src/shape.rs:522-545`. The renderer later indexes
  colors unconditionally by retained point indices at
  `src/renderer/frontend/composer/mod.rs:1061-1071`, so malformed public input
  panics far from its source or only for particular geometry. Use a release
  assertion in the cold `Shapes::add` boundary at
  `src/forest/shapes/mod.rs:70-76`, before lowering enters the hot path.
  Validate exact valid and invalid lengths for `Single`, `PerPoint`, and
  `PerSegment`, including zero-, one-, and two-point inputs.

## Batch 2 — API and ownership invariants

- [x] **Validate `Theme::text_scale` during deserialization.** `Theme` derives
  `Deserialize` at `src/widgets/theme/mod.rs:51-52`, so the private bookkeeping
  field at `src/widgets/theme/mod.rs:96-101` can be loaded as zero, negative,
  NaN, or infinity without passing through `set_text_scale`. The next scale
  change divides by that value at `src/widgets/theme/mod.rs:121-135` and
  poisons every stored font size. Either omit this derived bookkeeping field
  from the wire format and reconstruct it, or use a validating newtype/custom
  deserializer. Validate rejection of every invalid class and a valid scaled
  theme round-trip followed by another absolute scale change.

- [x] **Validate zoom configuration and host zoom factors before hot-path
  math.** `ZoomConfig` exposes unrestricted `range` and `step` fields at
  `src/widgets/scroll/mod.rs:46-58`, and `with_zoom_config` stores them without
  validation at `src/widgets/scroll/mod.rs:457-462`. They later feed `powf` and
  clamp logic at `src/widgets/scroll/mod.rs:497-515,561-568`. Arbitrary
  `InputEvent::Zoom` factors are also accumulated at
  `src/input/mod.rs:659-665`; winit creates `1 + delta` without a positivity or
  finiteness check at `src/input/mod.rs:243`. Give `ZoomConfig` a validated
  constructor/builder (`0 < min <= max`, positive finite step), make its raw
  fields non-public, and reject invalid host factors at ingress. Validate every
  invalid boundary and long valid pinch/wheel sequences remaining finite.

- [x] **Make raw state reads consistently reactive under `InputPolicy::OnDelta`.**
  `Ui::pointer_pos` auto-subscribes to pointer movement and explains why at
  `src/ui/mod.rs:1188-1203`, but `ResponseState::pointer_local` cannot observe
  that it was read. `Ui::modifiers` is also a passive `&self` getter at
  `src/ui/mod.rs:1206-1210`, while modifier changes repaint only subscribed
  consumers at `src/input/mod.rs:708-714`. Provide self-subscribing `&mut Ui`
  queries for logical widget-local pointer state and modifiers, and rename or
  restrict passive snapshots so custom widgets do not accidentally render
  stale output. Validate a hover-local indicator and Alt/Ctrl-dependent visual
  through press and release without any unrelated event.

- [x] **Move per-widget text reuse state out of the app-global shaper.**
  `HostContext` clones one `TextShaper` into every window
  (`src/host/context.rs:26-32`, `src/ui/mod.rs:132-142`), but that shared
  shaper owns a reuse map keyed only by `(WidgetId, ordinal)` at
  `src/text/mod.rs:129-150`. Auto IDs are call-site-derived and therefore
  naturally repeat across windows (`src/primitives/widget_id.rs:65-88`).
  Different windows overwrite each other's reuse rows, and either window's
  `sweep_removed` can evict the other's live row
  (`src/text/mod.rs:480-491`, `src/ui/mod.rs:567-574`). Split text state into an
  app-global Cosmic buffer/content cache and a per-`Ui` identity reuse cache.
  Validate two windows with identical IDs but different text and independent
  removals: shape dispatches must remain window-local while Cosmic buffers stay
  shared.

- [x] **Preserve `None` for GPU pass categories absent from a measured frame.**
  `GpuPassStats::last_kind_ms` promises this distinction at
  `src/renderer/backend/gpu_pass_stats.rs:101-107`, and `clear_kinds` repeats
  it at `src/renderer/backend/gpu_pass_stats.rs:123-128`. However,
  `publish_timestamps` zero-initializes every bucket and publishes every enum
  variant at `src/renderer/backend/gpu_timings.rs:471-488`, turning absent work
  into `Some(0)`. Track a parallel `seen` array and publish only seen
  categories; a category that genuinely ran for a rounded zero duration should
  remain `Some(0)`. Update the timestamp tests to require an absent Mesh
  category to remain `None`.

## Batch 3 — Low-risk hot-path simplifications

- [ ] **Consume the already-owned `WidgetLook` instead of cloning its
  `Background` twice.** `resolve_look` clones the selected look at
  `src/widgets/theme/mod.rs:186-206`, then `WidgetLook::animate(&self)` clones
  the large background again at
  `src/widgets/theme/widget_look.rs:68-84`. Toggle and Switch repeat the same
  owned-clone-then-borrowed-clone pattern at
  `src/widgets/toggle.rs:31-45,68-74` and
  `src/widgets/switch.rs:70-84`. Add a consuming animation/target path that
  moves the first clone into `AnimatedLook`; retain a borrowed path only for
  callers that genuinely need it. Validate identical animation targets and
  transitions, then compare all existing release frame benchmark arms. This is
  also the first concrete source change recommended by
  `docs/frame-cpu-profile-2026-07-17.md:143-154`.

- [ ] **Skip `MeasureCache` probes for leaves, which are deliberately never
  inserted.** `LayoutEngine::measure` unconditionally computes the cache key,
  quantizes availability, and probes at `src/layout/engine.rs:493-510`, while
  the write path explicitly excludes leaves because leaf snapshots measured as
  overhead at `src/layout/engine.rs:590-600`. The intrinsic path similarly
  probes the cross-frame cache for a leaf at
  `src/layout/engine.rs:341-355`, although no leaf snapshot can satisfy it.
  Gate both lookups on `style.mode != LayoutMode::Leaf`. Validate existing
  cache correctness and compare forced-miss and resize layout benchmarks,
  especially leaf-heavy trees.

- [ ] **Give `BarMode::Hidden` a real no-bar path.** Hidden Scroll widgets still
  derive four IDs, perform four response lookups, read pointer state, compute
  drag/page geometry, and build both bar plans at
  `src/widgets/scroll/mod.rs:538-637,718-735`. They also request a cold-mount
  settling pass whose documented purpose is only thumb visibility at
  `src/widgets/scroll/mod.rs:646-655`; the late render gate merely discards the
  result at `src/widgets/scroll/mod.rs:737-757`. Gate all bar response,
  geometry, plan, and bar-only relayout work on `bar_mode != Hidden`, retaining
  only pan/zoom/viewport state. Validate hidden cold mount has no bar IDs or
  bar-induced second pass while pan and zoom remain exact; benchmark many
  hidden Scroll scopes.

- [ ] **Do not shape paint-only container text on an explicitly hidden owner.**
  The post-arrange container-text loop skips only `Collapsed` at
  `src/layout/engine.rs:456-466`, so a `Hidden` container still shapes and
  appends paint-only text every frame even though it cannot render. Require
  `visibility().is_visible()` before shaping. Validate that a hidden container
  preserves its layout, emits no shaped paint run, and shapes correctly when
  made visible. Ancestor-hidden pruning can follow only if effective
  visibility becomes available without adding another tree walk.

- [ ] **Add a non-allocating mutable state probe.** `Ui` exposes allocating
  `state_mut` and immutable `try_state`, but no `try_state_mut`
  (`src/ui/mod.rs:1074-1092`). DragValue therefore probes and then performs a
  second lookup to mutate at `src/widgets/drag_value/mod.rs:305-318,438-448`;
  `ContextMenu::close`, documented as a no-op for a never-opened menu, instead
  allocates a state row at `src/widgets/context_menu/mod.rs:161-177`. Add
  `StateMap::try_get_mut` and `Ui::try_state_mut`, then migrate these sites.
  Validate close-before-open leaves the typed store empty and existing rows
  mutate in place.

## Batch 4 — Cache and frame-lifecycle improvements

- [ ] **Stop invisible paint animations from scheduling frames forever.**
  Animated shapes are registered regardless of their own or an ancestor's
  visibility at `src/forest/mod.rs:233-249`. The wake fold then considers every
  animation and ignores its stored owner node at
  `src/forest/mod.rs:116-135` and
  `src/forest/tree/paint_anims.rs:207-225`. A hidden or collapsed `Spin`
  therefore requests another immediate frame indefinitely despite being unable
  to paint. Extend the existing recording-time cascade state in
  `src/forest/tree/recording.rs:21-39` with effective visibility and omit the
  active animation row while its owner is effectively invisible; keep the
  authored shape so visibility transitions can resume it. Validate self-hidden,
  self-collapsed, ancestor-hidden, and hide/show transition cases.

- [ ] **Invalidate encoded text by glyph-slot generation, not by any atlas
  eviction.** `GlyphAtlas` exposes one global `eviction_count` at
  `src/renderer/backend/text/atlas.rs:105-125`; every encoded run stores it and
  misses after any eviction at
  `src/renderer/backend/text/encode.rs:84-101,181-213`. The slow path also
  discards an entire new arena span if one eviction occurs during its walk at
  `src/renderer/backend/text/encode.rs:254-349`. Give reusable `GlyphSlot`s
  individual generations, store the generation beside each encoded glyph, and
  validate it during the cache-hit loop that already touches every slot. Miss
  only a run referencing a changed slot. Validate with two disjoint runs where
  one loses a slot, and benchmark mixed stable/churning text plus
  `text_atlas/zoom_cold`.

- [ ] **Choose the least-recently-used eligible glyph when an atlas scan is
  already required.** Slots maintain `last_use`, but `evict_one` selects the
  first eligible hash-map entry at
  `src/renderer/backend/text/atlas.rs:445-470`. Victim choice is therefore hash
  iteration order and can evict a previous-frame glyph while much older glyphs
  remain. The scan is already O(n); use `min_by_key(last_use)` without adding
  an intrusive LRU. Add a deterministic victim test and compare
  rasterization/eviction counts in atlas-pressure benchmarks.

- [ ] **Recycle a bounded pool of evicted Cosmic Text buffers during
  continuous resize.** Cache misses construct fresh buffers at
  `src/text/cosmic.rs:321-340,456-480`, while LRU maintenance drops their
  internal vector capacities at `src/text/cosmic.rs:563-579`. The current
  profile attributes unique-width resize to 343.38 blocks and 182,981 bytes per
  frame, overwhelmingly through `cosmic_text::Buffer`
  (`docs/frame-cpu-profile-2026-07-17.md:202-223`). Keep a small bounded recycle
  pool and reset/reuse buffers for new wrap widths. Validate exact shaping
  output, retained-capacity bounds, zero allocations in the steady-state
  `alloc_free` arm, allocation reduction in unique-width resize, and CPU time
  across all four frame arms.

- [ ] **Prototype incremental cascade invalidation rather than weakening the
  global fingerprint.** `cascade_fingerprint` folds each root's full subtree
  authoring hash at `src/ui/cascade/mod.rs:510-533`, so any paint-only content
  change reruns the complete cascade at `src/ui/mod.rs:535-558`. The current
  profile measures cascade self-time around 37-39 microseconds in partial,
  scrolling, and resizing frames
  (`docs/frame-cpu-profile-2026-07-17.md:98-117`). Separate stable
  geometry/ancestor-state columns from paint-row refresh, or invalidate
  subtrees and repair ancestor paint bounds. Do not simply omit paint from the
  current fingerprint: that would reuse stale paint arenas and cascade hashes.
  Validate exact equivalence against a forced-full cascade over transform,
  clip, visibility, scroll, side-layer, reorder, and paint-only mutations, then
  benchmark partial and scrolling arms.

## Batch 5 — Consolidate duplicated policies

- [ ] **Make direct-text ordinal assignment a single source of truth.**
  Intrinsic sizing manually assigns and overflow-checks `(WidgetId, ordinal)`
  at `src/layout/intrinsic.rs:170-219`; normal shaping independently repeats
  the counter and overflow policy at `src/layout/engine.rs:779-789`. Both feed
  the same identity cache and must never drift. Have the shared iterator in
  `src/layout/support.rs:61-83` yield the checked ordinal with
  `TextShapeInput`, then consume it in both paths. Validate multiple direct text
  runs interleaved with non-text shapes and the ordinal overflow boundary.

- [ ] **Consolidate raw RGBA8 ownership and validation.** `Image` and
  `WindowIcon` independently store straight-alpha RGBA8 dimensions and bytes,
  with separate validators at `src/primitives/image.rs:58-93` and
  `src/window.rs:58-86`. Both expose fields publicly, so callers can bypass the
  constructors; both compute `width * height * 4` without checked
  multiplication. `WindowIcon` also permits zero dimensions, after which winit
  silently drops the malformed icon at `src/host/winit/mod.rs:487-493`.
  Introduce a shared invariant-bearing RGBA8 pixel buffer with non-zero
  dimensions and checked length arithmetic; make Image and WindowIcon thin
  semantic wrappers. Validate zero dimensions, arithmetic overflow, wrong
  length, valid image upload, and valid platform-icon conversion.

- [ ] **Route polyline width through the canonical scalar no-op predicate.**
  `DrawPolylinePayload::is_noop` hand-rolls `width <= 0.0` at
  `src/renderer/frontend/cmd_buffer/payload.rs:284-296`, while neighboring
  stroke payloads use `noop_f32` at
  `src/renderer/frontend/cmd_buffer/payload.rs:490-550` and authoring already
  uses the same canonical policy at `src/shape.rs:785-799`. The duplicate
  disagrees for NaN and sub-EPS positive widths, allowing an internally
  malformed payload into composer and shader math. Replace the comparison with
  `noop_f32(self.width)` and extend the payload gate table with NaN,
  sub-EPS, zero, and positive widths.

- [ ] **Build Stack planning data in one child walk without unifying the
  Stack/Grid solvers.** `stack_plan` walks every active child for counts,
  weights, and non-Fill sums at `src/layout/stack/mod.rs:153-188`;
  `push_fill_entries` immediately walks them again at
  `src/layout/stack/mod.rs:114-137`. Both measure and arrange pay the duplicate
  traversal at `src/layout/stack/mod.rs:217-261,344-355`. Populate the
  `StackPlan` and Fill scratch slice together in one pass, leaving
  `freeze_distribute` and the documented Stack/Grid freeze-cadence divergence
  at `src/layout/stack/mod.rs:45-57` untouched. Validate every Stack sizing
  mode and benchmark wide/deep stacks.

- [ ] **Add a paired intrinsic query for Grid Hug cells.** Every span-1
  Hug-column cell requests `MinContent` and `MaxContent` back-to-back at
  `src/layout/grid/mod.rs:397-421`. On a cold subtree these are separate
  recursive walks; at a text leaf they reach the same unbounded shaping input
  and select two metrics from the same result at
  `src/layout/intrinsic.rs:175-191`. Add a targeted `intrinsic_range` query that
  fills both per-node cache slots in one recursion while retaining the
  single-slot API for Stack's min-only case. Validate exact equivalence for all
  layout drivers, inspect intrinsic compute counts, and compare forced-miss and
  resize benchmarks before keeping the added API.

## Open design decision

- [ ] **Decide whether Grid Fill tracks contribute their content floor to the
  Grid's own intrinsic size.** The design note says Fill contributes content
  intrinsic while ignoring weight (`src/layout/intrinsic.md:67-71`). Grid
  measure follows that rule at `src/layout/grid/mod.rs:397-421,886-914`, but
  `grid::intrinsic` contributes only `Track.min` and skips non-Hug cells at
  `src/layout/grid/mod.rs:956-1014`. An ancestor Stack can therefore allocate
  from a zero Grid floor, after which the Grid discovers a rigid Fill-cell
  floor and overflows rather than letting a shrinkable sibling surrender
  space. Choose and document the intended semantics before changing code.
  Pin the decision with a Fill Grid track containing a Fixed descendant, both
  as a Hug root and as one of several Stack Fill siblings.

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
