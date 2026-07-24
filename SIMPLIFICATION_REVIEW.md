# Aperture simplification review

Reviewed 2026-07-25 against the working tree observed during the audit.

## Scope and baseline

The review covers production code in `src/` and `anim-derive/src/`, with tests,
benches, examples, and the showcase used as supporting evidence. The focus is
structural simplification, consolidation, code deletion, and measurable
performance opportunities. Correctness problems found while tracing those
paths are listed separately.

`cargo clippy --all-targets -- -D warnings` is clean. This is a static review,
not a claim that an unbenchmarked optimization is faster. Performance changes
below have an explicit benchmark gate.

The current working tree already handles close requests on occluded windows and
retires a closed window's `GpuView` render owner. Those previously identified
problems are resolved and are not findings here.

`src/widgets/scroll/mod.rs` changed concurrently after its finding was
captured. Recheck C2 against that edit before scheduling work; the concurrent
change was left untouched.

## Executive conclusion

Aperture's local implementations are generally careful and already tuned. The
largest remaining simplifications are not small helper cleanups; they are
protocol deletions:

1. Remove same-frame record replay, or sharply constrain it.
2. Preserve authoring order through the renderer instead of recovering it with
   overlap indexes and scheduling rules.
3. Replace independently maintained layout/cascade/damage snapshots with fewer
   shared representations.

Those three areas account for most of the framework-wide invariants, retained
scratch, cache schemas, and "keep these paths synchronized" comments. They also
carry the highest regression risk, so each should begin as a benchmarked
experiment rather than a broad rewrite.

## Recommended order

| Order | Work | Expected payoff | Effort | Required proof |
| --- | --- | --- | --- | --- |
| 1 | Fix the correctness findings | Removes known invalid states before refactoring | Small–medium | Focused tests plus `scripts/test-all.sh` |
| 2 | Prototype author-ordered render batches | Potentially deletes the composer's largest compensating subsystem | Medium | `composer`, `frame`, and GPU benches; visual suite |
| 3 | Prototype a single record pass | Deletes a cross-framework lifecycle protocol | Large | Frame/input/animation benches, allocation tests, showcase |
| 4 | Consolidate cascade and damage history | Fewer retained schemas and invalidation gates | Large | `cascade` and `damage` benches; visual suite |
| 5 | Simplify the measure cache and grid state together | Removes manual capture/restore coupling | Large | Every `caches` arm and layout tests |
| 6 | Apply targeted deletions and delegation cleanup | Low-risk code and API reduction | Small | Full feature matrix |

## High-leverage structural findings

### 1. Record replay is a second lifecycle protocol

`Ui::frame` can run a cold-start warmup, the visible record pass, and an action
or relayout settling pass (`src/ui/mod.rs:162`). Supporting that behavior
requires:

- `App::update` versus replayable `App::record` (`src/app.rs`);
- input swapping, queue draining, and `frame_had_action`
  (`src/input/mod.rs:258`, `src/input/mod.rs:761`);
- discarded widget IDs and special rollover handling
  (`src/scene/seen_ids.rs:127`, `src/scene/seen_ids.rs:245`);
- delayed cache/state sweeping (`src/ui/mod.rs:422`);
- animation's once-per-frame guard;
- an O(N) cascade ID snapshot because the live recording map is cleared
  between passes (`src/scene/cascade/mod.rs:333`).

This complexity is framework-wide and every action frame repeats
record/layout/cascade work.

**Direction:** prototype a single-pass contract. Actions may mutate state during
record, but changes that affect content already emitted earlier in the pass
schedule another frame instead of replaying the entire pass immediately. First
frame pointer routing can likewise settle on the next frame rather than through
a hidden blackout record. If same-frame settling is indispensable, constrain it
to an explicit local mechanism instead of replaying arbitrary `App::record`.

**Payoff:** removal of the discarded-pass ID set, input drain protocol,
`App::record` replay rules, pass-specific cache lifetime rules, and several
same-frame guards. It may also allow `App::update` and `App::record` to collapse
back to one lifecycle entry point.

**Gate:** compare `frame` and input-throughput benches for idle, hover, click,
text edit, and relayout cases. Pin whether one additional presented frame of
latency is acceptable for state changes recorded before their trigger.

### 2. Fixed per-kind GPU replay creates a large ordering subsystem

The backend replays each group as quads, text, meshes, images, then curves.
`Composer` must split groups whenever this fixed order would invert overlapping
authoring-order draws (`src/renderer/frontend/composer/mod.rs:34`). That policy
owns:

- `HigherKindRects` and per-tier rectangle lists
  (`src/renderer/frontend/composer/higher_kind.rs`);
- two `TextRectGrid` instances and their tile/spill/touched machinery
  (`src/renderer/frontend/composer/text_grid.rs`);
- group anchors and per-kind batch cursors
  (`src/renderer/render_buffer/batch.rs`);
- a second cursor-driven ordering walk in
  `src/renderer/backend/schedule.rs`.

The comments record that the former text scan dominated composition, a
`TinyVec` version alone cost about 2% of a frame, and full grid clearing once
consumed about 37% of composer self-time. The optimized workaround is good, but
it is optimizing complexity created by the replay model.

**Direction:** prototype an author-ordered sequence of adjacent homogeneous
batches. Coalesce consecutive draws with the same pipeline and clip state;
switch pipelines when authoring order requires it. Keep rounded-mask setup as a
separate state transition, but stop predicting overlap in order to reorder
draws.

**Payoff:** `HigherKindRects`, `TextRectGrid`, conflict flushes, `last_group`
anchors, much of `ScheduleCursors`, and a large class of paint-order proofs can
disappear.

**Tradeoff:** more pipeline switches and draw calls on alternating paint kinds.
Do not accept this change on aesthetics alone; compare `composer`, `frame`, and
GPU timings on text-heavy, shape-heavy, and pathological alternating-kind
fixtures.

### 3. Cascade invalidation and damage history mirror the scene several times

`Ui::post_record` first compares `cascade_fingerprint`
(`src/ui/mod.rs:381`, `src/scene/cascade/mod.rs:590`). If that misses,
`CascadesEngine::can_update` independently validates display scale, counts,
static hashes, arranged rectangles, and subtree ends
(`src/scene/cascade/mod.rs:489`). `Cascades` then retains cascade inputs,
subtree hashes/ends, paint bounds, hit rows, and a paint arena. `DamageEngine`
retains another widget map and copied paint arena containing overlapping hash,
cascade, parent, and paint facts (`src/scene/damage/mod.rs`,
`src/scene/damage/snapshot.rs`).

The immediate consolidation is to make `CascadesEngine` own its outer
fingerprint and all reuse decisions. The larger opportunity is a
double-buffered frame-scene snapshot: record against the prior frozen cascade,
build the current snapshot, diff prior/current for damage, then swap. Damage
should add only information that cannot be derived from those two snapshots,
not retain another representation of every paint row.

**Payoff:** one invalidation authority and fewer hash/rect/paint copies.

**Gate:** `cascade` and `damage` benches must cover unchanged trees, localized
paint changes, structural reorders, reparenting, clipping, transforms, and
paint animation. Hash equality must never be the only proof for externally
visible geometry.

### 4. The measure cache is a manually synchronized shadow layout

Live state is split across `LayoutScratch`, `LayerLayout`, `GridHugStore`, and
text buffers. The cache introduces `CachedSubtree`, `CaptureTreeInput`,
`NodeArenas`, `MeasureSnapshot`, and bespoke capture/restore code
(`src/layout/cache/mod.rs:39`, `src/layout/cache/mod.rs:52`,
`src/layout/cache/mod.rs:130`, `src/layout/engine.rs:148`). The engine explicitly
warns that every new measure-to-arrange field needs coordinated schema,
capture, and restore edits or arrange silently corrupts.

Two experiments are worth comparing:

1. Retain subtree caching, but make the live measure-to-arrange product a named
   column bundle that the cache stores and restores directly.
2. Remove subtree caching and retain only the expensive text/intrinsic caches.
   The record/tree walk may be cheaper than maintaining the general snapshot.

The existing `caches` benchmark already has cached, forced-miss, resizing,
localized, deep, broad, heavy-text, and grid-intrinsic arms. Use all of them;
do not infer value from one steady-state root hit.

### 5. Grid track data has transient, durable, and cached forms

`AxisScratch` owns sizes, resolution flags, offsets, flexible indices, and hug
bounds (`src/layout/grid/mod.rs:59`). `GridHugStore` separately owns min/max
pools, persisted sizes, totals, and slots (`src/layout/grid/mod.rs:160`).
Measure copies resolved data into the durable store; arrange copies it back or
re-solves through a zero-total sentinel; the measure cache packs and restores
the hug arrays again.

`reset_hugs_for` still documents a grow-driven second measure pass that the
layout engine says no longer exists. That is a useful warning sign: the state
model outlived the algorithm that shaped it.

**Direction:** redesign this with the measure cache. Give each grid one durable
`GridResolved` result containing the min/max constraints, solved sizes, and
solve input. Measure writes it once and arrange reads it once. Cache that same
result rather than repacking selected arrays.

### 6. Scroll output is sparse data stored as a dense per-node column

Every layer allocates and zeroes one `Size` in `scroll_content` for every node
(`src/layout/mod.rs:31`), the measure snapshot duplicates it for every node,
and each subtree hit copies the full slice. Only the scroll driver writes it
(`src/layout/scroll/mod.rs:44`), and production has one read path
(`src/widgets/scroll/mod.rs:244`).

**Direction:** store results by scroll ordinal or as a sparse `(NodeId, Size)`
arena. Avoid a hashmap on the hot path if a compact scroll index can be assigned
while recording.

**Gate:** measure memory and layout time on both tiny trees and large trees with
few scroll containers. A new per-node index that costs more than the removed
eight-byte column would defeat the change.

### 7. The command buffer is a transient duplicate renderer schema

`Frontend::build` immediately feeds encoder output to the composer
(`src/renderer/frontend/mod.rs`). Nevertheless, every paint operation has a
`CmdKind`, a Pod payload, a recording method, a decoded `Command` variant, and a
composer match arm (`src/renderer/frontend/cmd_buffer/`). `GpuView` additionally
needs a parallel side channel because callbacks are not Pod.

The packed stream has a valid footprint argument, but it has no independent
lifetime and no cache consumes it.

**Direction:** after deciding the ordering model, make the encoder write into a
composer/batch sink directly. Keep `ShapeRecord` as the retained logical scene
form and `RenderBuffer` as the physical GPU form; remove the intermediate
descriptor arena and decode vocabulary.

**Gate:** compare encode+compose time and allocations. A direct sink must retain
scratch capacity and must not reintroduce large tagged enums.

### 8. Input routing and frame scheduling form one distributed state machine

`InputState` owns persistent gesture state, event queues, subscriptions,
capture, focus, `frame_had_action`, `had_input_since_last_frame`, and
`repaint_requested_since_last_frame` (`src/input/mod.rs:258`). `Ui::frame`
projects those into `FrameClassifyInput`; `FrameRuntime` combines them with
wake reasons and output validity (`src/ui/frame.rs:94`).

**Direction:** have input ingestion accumulate one explicit `InputFrameOutcome`
containing the facts scheduling consumes. Drain it once at frame entry. If
record replay is removed, `frame_had_action` and its separate reset semantics
can disappear entirely.

Closely related, disabled response state is projected three times:
`InputState::response_for`, `Ui::response_for`, then `WidgetEntry`, which
temporarily applies current disabled state but restores the stale raw bit in
the returned response (`src/ui/mod.rs:922`, `src/widgets/mod.rs:124`).
Choose one public meaning—prefer current effective disabled state—and compute it
once.

### 9. Overlay behavior is duplicated behind leaky abstractions

`Popup` owns positioning, layer selection, chrome, outside-click policy,
keyboard capture, and dismissal (`src/widgets/popup/mod.rs:119`).
`ContextMenu` exposes a hand-picked subset of `Node` setters, builds its own
body, then overwrites `Popup`'s crate-visible `node`
(`src/widgets/context_menu/mod.rs:57`). `Tooltip` and `Modal` rebuild related
layer/chrome/placement behavior (`src/widgets/tooltip/mod.rs:55`,
`src/widgets/modal.rs:29`).

**Direction:** introduce one crate-private overlay recorder configured by
layer, placement, backdrop/input policy, keyboard policy, and body node.
`Popup`, `ContextMenu`, `Tooltip`, and `Modal` should remain ergonomic public
facades, but none should reach into another facade's representation.

### 10. Text-edit orchestration is mostly transfer plumbing

One frame moves related layout facts through `LayoutInput`,
`ResolvedLayout`, `GeometryInput`, `FinalGeometry`, `ViewUpdateInput`,
`ViewUpdate`, and `PaintInput` (`src/widgets/text_edit/view.rs:135-307`).
`TextEdit::show` also reacquires the same `TextEditState` row repeatedly around
those calls (`src/widgets/text_edit/mod.rs:193-347`).

**Direction:** center the module on a `TextEditFrame`/session value that owns the
borrow-independent inputs and produces one named output. Move the state row out
once, run input/layout/geometry/view updates on that value, then put it back.
Keep painting separate because it needs `&mut Ui`.

This is a maintainability consolidation first. Benchmark before claiming the
saved map probes matter.

### 11. `Node` delegation is repetitive and has already caused drift

There are 21 identical `Configure` implementation blocks in `src/widgets`.
More importantly, composite widgets manually translate a `Node` into another
widget or multiple wrappers. `DragValue` and `Scroll` demonstrate how fields
can be lost.

**Direction:** use a small crate-private delegation macro or derive for the
identical `Configure` impls. Add explicit `Node` transformation helpers for
composites, with exhaustive destructuring inside the helper and named policies
for fields intentionally consumed or rejected. Do not add generic accessors for
every `Node` field.

### 12. GPU wire concerns leak into unrelated CPU concepts

`FillKind` contains brush kind, spread, shadow kinds, triangle geometry, a
composer fast-path bit, and a window-mask bit
(`src/primitives/fill_wire.rs:30`). The composer relies on exact
`FillKind::SOLID` equality for opaque-cover optimizations, triangle data reuses
corner/gradient lanes, and WGSL manually decodes the packed word.

**Direction:** split the CPU vocabulary into paint kind and flags, then pack
them only at the GPU wire boundary. Triangle/shadow payload construction should
own their lane reuse. Keep the final `u32` wire layout if it is still optimal;
the simplification is containment, not necessarily a wider instance.

No-op policy has a similar ownership problem. Authoring shapes, lowered paint
types, and command payloads each implement overlapping predicates, while the
command buffer calls itself canonical and immediately documents exceptions.
Choose one correctness gate before expensive lowering and treat later checks as
debug assertions or explicitly named defensive guards.

### 13. Layout driver identity is repeated across dispatch trees

Measure, arrange, and intrinsic sizing each match every `LayoutMode`
(`src/layout/engine.rs:736`, `src/layout/engine.rs:787`,
`src/layout/intrinsic/mod.rs:197`). Adding a driver requires synchronized edits,
and Scroll delegates differently in each phase.

**Direction:** generate the three exhaustive dispatches from one driver roster
or place all three operations together per mode. Avoid dynamic trait dispatch
on the node hot path; this is a compile-time consolidation.

### 14. Physical module boundaries do not match dependency ownership

`scene::Node` owns layout vocabulary while layout imports scene trees;
authoring `Shape` exposes a renderer-owned image handle while renderer consumes
scene records; `Ui` owns widget theme while widgets call back into `Ui`.

A directory-only reorganization would create churn without simplifying these
dependencies. Let the changes above move ownership first. Afterwards, form a
small authoring/core vocabulary layer, a recorded-scene layer, and a renderer
wire layer, then rename modules to match the resulting graph.

## Correctness findings to fix before broad refactors

### C1. `DragValue` loses normal widget configuration in edit mode

The chip resolves the caller-configured `Node`, but `show_editing` constructs a
fresh `TextEdit` carrying only ID, alignment, size bounds, selection behavior,
and theme (`src/widgets/drag_value/mod.rs:262`, `src/widgets/drag_value/mod.rs:396`).
Padding, margin, parent alignment, grid placement, canvas position, clipping,
visibility, and other configuration disappear while focused. Entering edit mode
can therefore move, resize, or re-clip the widget.

Transfer the full applicable node policy through one explicit helper and add a
table-driven test covering every `Configure` field.

### C2. `Scroll`'s documented compile-time drift guard is ineffective

`scroll_wrappers` claims an exhaustive `Node` destructure, but ends it with
`..` (`src/widgets/scroll/mod.rs:371`). A newly added field compiles while
entering neither wrapper.

Remove `..`, list `clip` and `transform` explicitly as consumed fields, and make
new `Node` fields fail compilation until their wrapper policy is chosen.

### C3. Modal keyboard ownership disagrees with layer ownership

Modal content paints above popups and consumes pointer input through its
backdrop, but dismissal reads the uncaptured keyboard stream
(`src/widgets/modal.rs:100`). Any popup capture makes that stream empty
(`src/input/mod.rs:428`). A visually topmost modal can therefore miss Escape
while a lower popup owns keyboard input.

Modal should preempt lower-layer keyboard capture or participate in one
layer-ordered overlay keyboard policy.

### C4. Popup capture becomes authoritative after popup bodies read events

`capture_keyboard` appends candidates, but `finish_record` selects the
last/topmost owner only after all popup bodies have run
(`src/input/mod.rs:443`, `src/input/mod.rs:460`). Popup and context-menu bodies
read captured keys before that point. When the top popup appears, disappears,
or reorders, the transition frame can deliver a key to the previous popup or
to none.

Resolve keyboard ownership before dispatching overlay key actions, or defer
overlay keyboard consumption until the candidate stack is final.

### C5. A valid frame can exhaust the gradient atlas and panic

Only 255 rows are usable (`src/renderer/gradient_atlas/mod.rs:51`). Rows touched
in the current epoch cannot be evicted because draw payloads already contain
their row IDs; `lru_victim` panics when all rows are protected
(`src/renderer/gradient_atlas/mod.rs:276`). More than 255 distinct gradient
stop/interpolation combinations in one frame is valid content, not a logic
error.

Use a capacity derived from device limits, grow/recreate the atlas before row
IDs are finalized, or spill to another texture/page. Add a test that renders
more than the old limit with distinct colors.

### C6. `HostHandle::run_on_main` silently drops owned work

`run_on_main` discards `send_event` failure and returns no delivery status
(`src/host/winit/handle.rs:89`). An event-loop shutdown race can destroy an
owned closure and its application-state mutation without observation.

Return a delivery `Result` or an explicit task handle. Repaint and quit may
remain best-effort, but arbitrary state-changing work should not share that
policy.

### C7. Dynamic `ComboBox` options use positional identity

Every option creates `MenuItem::new` from one loop call site and applies the
response to `options[i]` (`src/widgets/combo_box/mod.rs:128`). Repeated auto IDs
are disambiguated by sibling occurrence order, while clicks route from the
previous frozen cascade. Insertion, filtering, removal, or reordering between
event routing and record can make a click on the old row select a different
current value.

Require a stable key per option, or accept an option iterator yielding
`(key, label)` and salt each menu item with that key.

### C8. Offscreen target validation happens after application state advances

`frame_offscreen` validates only scale, derives size/format, runs the full CPU
frame, and then submits to an arbitrary `wgpu::Texture`
(`src/host/offscreen.rs:201`, `src/host/offscreen.rs:225`). Size equality is
only a debug assertion in `WindowDriver`; dimension, sample count, usage, and
format compatibility are not rejected at the public boundary.

Validate the complete target contract before `cpu_frame` and return a typed
error. Invalid external input must not advance app state, animation time,
caches, or frame identity.

## Direct deletion and code-reduction candidates

### D1. Remove unused production surface

- `URect16` and its conversions have no production use
  (`src/primitives/urect/mod.rs:110`). Its documentation still describes an
  old `TextRun` representation.
- `Mesh::with_known_bbox` is used only by its own test and permits a bad AABB
  to corrupt culling/order without detection
  (`src/primitives/mesh.rs:203`).
- `Corners::{top_bottom, diag_main, diag_anti}` are used only by their own
  tests, and `Scroll::overlay_bars` has no workspace caller.

Given the explicit no-compatibility posture, delete these rather than carrying
speculative API.

### D2. Keep one derive attribute spelling

`#[animate(snap)]` and `#[animate(skip)]` are aliases in
`anim-derive/src/lib.rs:177`, while every in-tree use spells `snap`. Remove the
unused `skip` spelling and its diagnostic/documentation branch.

### D3. Compile the mono text shaper out of production invariants

Normal construction always installs cosmic shaping, but `ShaperInner` stores
`Option<CosmicMeasure>` and production methods retain fallback dispatch
(`src/text/mod.rs:142`, `src/text/mod.rs:302`). The only `None` constructor is
the test/internals-only `TextShaper::test_mono`.

Gate the fallback variant and module, or make the fallback a separate test
implementation. A production `TextShaper` should make "cosmic exists" a type
invariant instead of checking or expecting it at runtime.

### D4. Do not add compatibility scaffolding while deleting

These deletions should remove callers, tests, docs, and syntax in the same
change. Do not leave aliases, wrappers, deprecated methods, or re-exports.

## Explicit non-goals

- Do not unify Stack and Grid's fill solvers without first choosing the desired
  mixed min/max freeze semantics; their divergence is documented and tested.
- Do not reintroduce encode or compose caches. Existing measurements say their
  contribution was below 1%; the opportunities above remove work and schemas
  instead.
- Do not begin with a top-level directory shuffle. Move ownership through
  working refactors, then let the final dependency graph dictate names.
- Do not trade retained scratch for fresh per-frame collections. Every proposed
  stream, batch, sparse result, or session must reuse capacity after warmup.
