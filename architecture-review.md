# Aperture architecture and code-flow review

Reviewed against `c53e451a` on 2026-07-16. Scope: all production Rust modules under `src/`, the shader sources, `Cargo.toml`, and the production-facing call sites in benches/integration harnesses. Tests and existing `test_support` code were read only when needed to confirm whether an invariant already had coverage.

## Executive summary

The core pipeline is well-shaped: authoring data is lowered once, layout is separated into measure/arrange drivers, cascade and damage have explicit cross-frame state, and the renderer has a clear CPU frontend/GPU backend boundary. The main architectural defect is ownership: several resources documented as frame- or window-local are actually app-global. That conflicts with `PaintOnly`, which intentionally retains one window's tree and frame-local handles across frames. In a multi-window host this can produce wrong geometry, wrong gradients, or a text-cache panic.

After those ownership bugs, the highest-value work is a group of bounded correctness fixes in input routing and layout. The largest organizational opportunity is to finish moving benchmarks behind narrow `internals` entry points and make `lib.rs` the only public namespace; the current public module tree exposes implementation modules, duplicate type paths, and a `WindowRenderer` API that external code cannot construct or call coherently.

## Current code flow

```text
WinitHost / OffscreenHost
  -> WindowRenderer::cpu_frame
     -> Ui::frame
        -> classify FullRecord vs PaintOnly
        -> record [optional second pass]
        -> measure -> arrange -> cascade
        -> state/cache sweep -> damage -> RenderPlan
     -> Frontend::build
        -> encode Ui scene -> RenderCmdBuffer
        -> compose -> RenderBuffer
  -> WgpuBackend::submit
     -> upload resources -> schedule draw groups -> present/copy
```

The critical multi-window failure is:

```text
window A FullRecord: tree A stores spans into shared payloads A
window B FullRecord: clears the same store and writes payloads B
window A PaintOnly:  retains tree A, skips record, reads payloads B
```

## What should remain unchanged

- Keep `Tree`/lowered `ShapeRecord` separate from the public `Shape` authoring enum. That boundary is doing useful work and keeps the hot tree storage compact.
- Keep inherent implementations with their owning structs. Large files alone are not a reason to scatter an inherent API across extension traits or unrelated modules.
- Keep the five-stage record/measure/arrange/cascade/encode flow explicit. The useful simplifications are at ownership and data-contract boundaries, not by folding passes together.
- Keep measure as the only broad cross-frame layout cache until a replacement is benchmarked. Encode/compose cache removal remains consistent with the current design.

## Batch 1 — Critical: make retained record payloads truly per-window

- [x] **Give every `WindowRenderer` its own `RecordStore` and pass its payloads explicitly to GPU submission.** `HostContext` used to own one store and clone it into every `Ui` (`src/host/context.rs:27-35`, `src/ui/mod.rs:151-165`) and the one backend (`src/renderer/backend/mod.rs:167-172,205-217`). `Ui::record_pass` clears it (`src/ui/mod.rs:451-459`), while `PaintOnly` deliberately retains the old tree and payload spans (`src/ui/mod.rs:184-195,219-230`). Serialized record-to-submit execution does not protect retained spans between different windows' frames. Reserve a fresh store in `WindowRendererBuilder::build`, give that handle to the window's `Ui`, and pass `payloads: &RecordPayloads` through `Submission` instead of storing them on `WgpuBackend`. Update the ownership docs in `record_store.rs` and `host/context.rs`. Verify with two windows containing different mesh/polyline/text payload lengths: record A, record B, then drive an animation-only paint on A and assert A's geometry/pixels remain unchanged.

## Batch 2 — Critical: stop retaining shared-cache physical identities

- [x] **Do not store an evictable gradient atlas row as retained scene identity.** `LoweredGradient` captures `LutRow` in `RecordPayloads` (`src/record_store.rs:30-37,74-80`), but the shared atlas reuses rows after 255 live gradients (`src/renderer/gradient_atlas/mod.rs:50-68,177-205`). The epoch protection covers rows registered during the current submit only (`src/renderer/gradient_atlas/mod.rs:87-103,234-267`); a `PaintOnly` window does not re-lower/re-register its gradients. Retain a gradient definition/content handle and resolve it to a row on every frontend build or submit, including `PaintOnly`, or make the CPU/GPU atlas owner-scoped. Add an interleaved two-window test that fills the atlas, evicts A's row from B, then paints A without recording.

- [x] **Make retained text buffers resilient to shared-cache eviction.** `TextShaper` remains app-global and its layout-reuse map remains keyed by `(WidgetId, ordinal)`, so matching IDs in different windows can still cause avoidable reshape misses. Correctness no longer depends on that entry remaining live, however: retained `ShapeRecord::Text` data carries the source text and canonical `TextCacheKey`, the encoder calls `ensure_buffer` before emitting every visible run, and `CosmicMeasure` reconstructs an evicted buffer before the backend lookup (`src/renderer/frontend/encoder/mod.rs:300-306`, `src/text/mod.rs:505-516`, `src/text/cosmic.rs:520-538`). The shared-context idle-window regression covers eviction followed by `PaintOnly` (`src/ui/tests.rs:597-680`). Owner-scoping is therefore unnecessary for correctness; revisit it only if multi-window benchmarks demonstrate meaningful reuse-cache thrash.

## Batch 3 — High: input and scroll routing correctness

- [x] **Discard events that have no current route instead of leaving them for a future widget.** Scroll and zoom now update widget accumulators only when their respective hit target exists; off-target subscribers still receive the separate pointer-event stream. Key and text events enter the keyboard queue only when focus or a matching subscription can observe them, and pointer/key/text actions set `frame_had_action` only when routed (`src/input/mod.rs:590-707`). Sequence regressions cover inert-area scroll followed by moving over a scroll target and unfocused key/text followed by focusing `TextEdit` (`src/input/tests/scroll_routing.rs:98-130`, `src/widgets/text_edit/tests/click.rs:44-79`).

- [x] **Only claim `Sense::PINCH` for zoom-enabled scrolls.** Scroll constructors now claim only `Sense::SCROLL`; `with_zoom` delegates to `with_zoom_config`, which preserves the configured sense flags while adding `Sense::PINCH` (`src/widgets/scroll/mod.rs:393-405,451-464`). The nested regression confirms a non-zoom inner scroll remains the wheel target while pinch routes to its zoomable outer ancestor, then verifies only the ancestor zooms (`src/widgets/scroll/tests.rs:46-80`).

- [ ] **Separate non-overflow scroll bounds from zoom rubber-band bounds.** `natural_bounds` sorts raw `[0, content * zoom - viewport]` endpoints (`src/layout/scroll/mod.rs:143-154`). For non-overflowing, non-zoomable content this creates a negative interval; negative wheel input enters empty viewport space and `clamp_to_natural` preserves it (`src/layout/scroll/mod.rs:180-210`). Preserve ordered underflow bounds only where pivot zoom needs them. For settled/non-zoom scrolls use semantic bounds whose high end is at least the leading bound. Add both positive- and negative-wheel non-overflow tests.

## Batch 4 — High: layout and analytic-geometry correctness

- [ ] **Cap Grid's min-content floor by `Track::max` in both Hug and Fill resolution.** Hug computes `lo = max(hug_min, track.min)` without applying `track.max` (`src/layout/grid/mod.rs:908-920`), so cramped layout can emit a size above the cap (`src/layout/grid/mod.rs:924-948`). Fill repeats the uncapped floor and can call `candidate.clamp(lo, max)` with `lo > max`, which panics (`src/layout/grid/mod.rs:970-999`). Canonicalize the content floor as `hug_min[i].max(t.min).min(t.max)` and derive `hi >= lo`. Extend the existing Grid min/max tests with a rigid child whose min-content exceeds the track cap for both Hug and Fill.

- [ ] **Track WrapStack line occupancy explicitly instead of using `line_main > 0`.** `would_wrap`, gap insertion, and the final measure flush use accumulated main extent as the nonempty-line sentinel (`src/layout/wrapstack/mod.rs:49-83,130-162`). A zero-width active child with nonzero height can disappear from measured cross-size; when followed by another child, its configured gap is omitted and measure can diverge from arrange. Thread a child count/occupied bit through `pack_child` and base wrapping, gaps, and final flush on that state. Test a lone zero-main/nonzero-cross child and one followed by a normal child in both horizontal and vertical wrapping.

- [ ] **Define and enforce `Element` min/max precedence at the authoring boundary.** Independent `min_size`/`max_size` setters only check non-negativity (`src/forest/element/mod.rs:303-321`), while layout later calls `f32::clamp(min, max)` (`src/layout/engine.rs:250-270`, `src/layout/support.rs:147-168`). `min > max` therefore panics deep in layout. Prefer the same ordered-bound contract used by `Track`: reject the setter call that makes the pair invalid, with an immediate message. Add setter-order and per-axis boundary tests.

- [ ] **Reject all zero-area triangles before lowering.** `Shape::is_noop` rejects only the all-three-coincident case (`src/shape.rs:735-765`). Collinear points, including one repeated vertex, reach `sdf_triangle`; zero winding and zero-length edges can make the analytic SDF paint the entire padded bbox or propagate invalid arithmetic (`src/renderer/backend/quad.wgsl:180-195,386-397`). Use a scale-aware cross-product area test at the authoring no-op gate and cover collinear, repeated-vertex, reversed-winding, and near-degenerate inputs.

## Batch 5 — High: persistent state, animation, and edit-history correctness

- [ ] **Key tooltip state by the recorded trigger ID.** Tooltip stores state under `trigger_id.with("tooltip")` (`src/widgets/tooltip/mod.rs:124-148`), but that synthetic ID is never a recorded node. `StateMap` sweeps exact IDs reported removed by the forest (`src/ui/state.rs:52-59`), so transient tooltip rows never die and can retain stale timing state. Store `TooltipState` under `trigger_id`; typed stores already isolate it from other state types. Keep only the intentional global singleton. In the same change, use `Duration` rather than `f32` seconds for tooltip timestamps to avoid long-uptime precision loss.

- [ ] **Reset animation-mode state when `AnimSpec` changes kind without a target change.** `AnimRow` does not remember its previous mode (`src/animation/mod.rs:118-146`), and Spring-to-Duration cleanup occurs only inside `row.target != target` (`src/animation/mod.rs:251-280`). Changing a live animation from Spring to Duration at the same target then reuses stale `segment_start`/`elapsed` in the duration integrator (`src/animation/mod.rs:312-340`), which can jump backward. Store an `AnimKind` discriminant and reset mode-specific fields whenever it changes, independently of retargeting. Test mid-flight Spring-to-Duration and Duration-to-Spring-to-Duration transitions.

- [ ] **Do not mutate undo/redo history for a rejected TextEdit insertion.** `replace_selection` records history before mutation (`src/widgets/text_edit/model.rs:218-225`), while `insert_capped` can reject all input at `max_chars` (`src/widgets/text_edit/model.rs:189-215`). Typing at the cap with no selection clones the whole buffer, opens/coalesces an undo unit, and clears redo despite no edit. Preflight whether selection deletion or at least one character insertion will occur, and return before `record_edit` for a true no-op. Verify redo and edit grouping survive a rejected insertion.

## Batch 6 — High: make the public boundary coherent

- [ ] **Internalize the renderer driver and make `lib.rs` the only supported type namespace.** `lib.rs:5-12` globally suppresses `private_interfaces`/`private_bounds` and publishes most implementation parents (`lib.rs:16-34`) while also root-reexporting their types, producing duplicate canonical paths. It publishes `RecordStore`, `WindowRenderer`, and `FramePresent` (`lib.rs:67,87-88`), yet `WindowRenderer` can only be built internally and its public methods require private `WgpuBackend` (`src/host/window_renderer.rs:215-271,324-395`; `src/renderer/backend/mod.rs:138-217`). Keep `WinitHost` and `OffscreenHost` as the supported facades; make the renderer/frontend/backend, host driver, forest, layout, input, UI, and widget implementation modules crate-private. Root-reexport legitimate consumer types currently reachable only through nested paths, such as `MeasureResult` and `FrameProcessing`. Keep renderer-only `RenderPlan`/`RenderKind` crate-private by replacing the public `FrameReport.plan` field with a public paint classification/query, unless external hosts genuinely need a coherent public damage-region API. Then remove the blanket lint allow. If custom-host integration is intended, publish one complete host-renderer builder instead of the current half-public driver.

- [ ] **Finish colocating benchmarks behind narrow `internals` calls before sealing modules.** Only composer and cascade currently use in-source `bench.rs`; the remaining top-level benchmark drivers total roughly 2,600 lines, led by `benches/frame.rs` (849) and `benches/damage.rs` (559). Those two are also the remaining direct reason benches import `renderer::frontend`, `ui::frame_report`, and nested damage `test_support` (`benches/frame.rs:54-56`, `benches/damage.rs:15`). Move frame/damage first into `src/renderer/frontend/bench.rs` and `src/ui/damage/bench.rs`, leaving five-line Criterion entry files like composer/cascade. Then move text atlas/shape, caches, and input benchmarks by subsystem. Allocation benchmarks may keep the global-allocator shell externally, but their fixtures and private reach-ins should live next to the production type. Expose only gated bench entry functions from a single `#[cfg(feature = "internals")]` facade.

## Batch 7 — Medium: tighten public data contracts

- [ ] **Represent only supported brushes in each `Shape` variant.** Triangle, Text, and Mesh publicly carry `Brush`, but lowering accepts only solid color and panics later (`src/shape.rs:54-68,147-190`; `src/forest/shapes/lower.rs:333-359`; `src/forest/shapes/mod.rs:171-203,236-251`). Make those fields `Color` and keep `Brush` only on shapes that really support gradients. For Line/Bezier/Arc, replace unrestricted `Brush` with a stroke-specific solid-or-linear type or validate in the public constructor so radial/conic cannot survive to `assert_curve_brush` (`src/forest/shapes/lower.rs:364-379`). This makes invalid states unrepresentable and removes late panics.

- [ ] **Repair float `Eq`/`Hash` consistency and centralize content hashing.** `Color`, `Rect`, and `Size` use float `PartialEq` but raw-byte `Hash` (`src/primitives/color.rs:4-26`, `src/primitives/rect/mod.rs:5-16`, `src/primitives/size.rs:3-14`), so values containing `-0.0` and `0.0` compare equal but hash differently, violating the `Hash` contract. `canon_bits` claims to be shared by float content hashes (`src/primitives/approx.rs:20-36`), yet record/layout hashes still use raw bytes/bits (`src/forest/shapes/hash.rs:41-78,151-204`, `src/forest/element/columns.rs:78-99`, `src/layout/types/sizing.rs:58-67`, `src/layout/types/track.rs:75-80`). Add canonical f32/Vec2/Size/Rect hash helpers, use equality-compatible canonicalization for public `Hash`, and use the documented visual canonicalization at content-cache boundaries. Hash polyline's already-lowered `ColorU8` data rather than the larger authoring `Color` slice. Test signed zero, canonical NaNs where applicable, and sub-EPS cache equivalence.

- [ ] **Reject excess sequence values and duplicate fields in shared lane deserialization.** `LaneVisitor::visit_seq` reads at most four values and never probes for a fifth (`src/primitives/lane_serde.rs:82-96`), so malformed arrays silently lose their tail. `visit_map` overwrites duplicate named fields (`src/primitives/lane_serde.rs:98-106`). Return serde errors for both cases while preserving the intentionally supported scalar, one-element, two-element, four-element, and missing-map-field forms.

- [ ] **Make `TextRectGrid` overflow structurally safe in release builds.** Tile buckets store `u16` rect indices, and `push` relies only on a `debug_assert!` before the narrowing cast (`src/renderer/frontend/composer/text_grid.rs:30-31,112-129`). A batch beyond the index range wraps to old rects and can miss a paint-order conflict. Keep hot-path assertions debug-only, but prevent the invalid state by flushing before the capacity limit, switching to `u32`, or entering a correct linear-overlap fallback. Add an overflow-boundary test without constructing a full GPU frame.

## Batch 8 — Medium: bounded CPU hot-path wins

- [ ] **Partition higher-kind overlap tracking by paint tier and add union pre-rejects.** `Composer::higher_kind_conflict` scans the whole `higher_kind_rects` vector for each Mesh/Image (`src/renderer/frontend/composer/mod.rs:382-411`), even though same-tier entries cannot conflict. Many same-tier draws therefore take quadratic predicate work. Store retained per-tier rect sets (or at minimum per-tier ranges) with union AABBs; scan only later-replaying tiers after an O(1) union miss test, and use an aggregate union for kind-blind queries. Extend `src/renderer/frontend/composer/bench.rs`, which currently measures only the Curve early-return path, with same-tier Mesh/Image and mixed-tier overlap/non-overlap cases.

- [ ] **Cache kept polyline directions in `PolylineScratch`.** After coincidence filtering, composer repeatedly normalizes the same segment direction for start/end planes and again for join chrome (`src/renderer/frontend/composer/mod.rs:1085-1192`). Add a retained `directions: Vec<Vec2>`, fill it once for the kept segments, and reuse exact stored values. This removes roughly four normalizations per segment while preserving bit-identical shared joint planes.

- [ ] **Re-absorb overlaps after `DamageRegion`'s at-cap forced merge.** Normal insertion repeatedly absorbs intersecting rectangles, but the cap path overwrites one slot with a union and returns (`src/ui/damage/region/mod.rs:168-233`). The grown rectangle can overlap a neighbor, violating the module's normal clustering invariant, duplicating pass work, and inflating coverage (`src/ui/damage/region/mod.rs:154-163`). Remove the chosen slot, union it into the candidate, rerun the existing absorption loop over the remaining at-most-seven rectangles, then push.

- [ ] **Retain temporary UI formatting/selection capacity across frames.** Context-menu shortcut labels allocate a `String` with `to_string()` on every open record pass (`src/widgets/context_menu/mod.rs:291-310`); format them into `Ui`'s `RecordStore` with `ui.fmt(format_args!(...))`. Multiline selection rectangles spill after 16 lines (`src/text/mod.rs:54-59`), but TextEdit constructs and drops a new `SelectionRects` each frame (`src/widgets/text_edit/mod.rs:540-550`), so a long selection reallocates every paint. Retain and clear that buffer on `TextEditState` or equivalent per-widget scratch. Add allocation-audit fixtures for an open shortcut menu and a long selected document after warmup.

## Batch 9 — Lower priority: benchmark-led rendering and cache changes

- [ ] **Prototype a static index buffer for curve strips.** The curve pipeline emits 96 non-indexed vertices per 16-segment instance (`src/renderer/backend/curve_pipeline.rs:30-32,129-139`), and the vertex shader recomputes the cubic/arc basis and tangent for duplicated triangle-list vertices (`src/renderer/backend/curve.wgsl:247-300`). A fixed 96-index pattern over 34 unique cross-section vertices should preserve batching while allowing the post-transform cache to reduce expensive vertex work by about 65%. Keep this only if pipeline-statistics and curve-pass timings improve on the target adapters; include join-chrome instances in correctness comparisons.

- [ ] **Tighten drop-shadow bounds around the shifted shadow.** `shadow_paint_rect_local` inflates both sides by `abs(offset)` (`src/forest/shapes/record.rs:266-296`), so a large positive offset needlessly damages and shades equally far on the negative side. Use `(source + offset).inflate(3 * sigma + max(spread, 0))`, then simplify the coupled shader reconstruction (`src/renderer/backend/quad.wgsl:340-350`) around the shifted bbox center. Verify bbox math and pixel output for positive/negative offsets, spread, blur, and inset shadows.

- [ ] **Benchmark a selective-root measure-cache policy before redesigning storage.** Every non-leaf miss snapshots its entire subtree (`src/layout/engine.rs:556-613`, `src/layout/cache/mod.rs:301-372`). A depth-`N` chain copies/stores `N + (N-1) + ...`, or O(N²), while balanced trees are O(N log N) on forced misses. First add deep-chain and broad-tree forced-miss benchmarks, then compare caching only selected roots/branch points against a flat previous-frame column snapshot. Do not replace the current cache unless steady-state hit rate and resize/forced-miss time improve together.

- [ ] **Use edit deltas instead of full TextEdit snapshots only if document-scale editing is a supported goal.** Each undo unit clones the complete buffer and up to 128 units are retained (`src/widgets/text_edit/model.rs:69-84,119-135`). This is O(buffer length) time and memory per edit group. For long documents, store named replacement operations with affected range, removed/inserted text, and caret/selection before/after, preserving current typing/delete coalescing. For short form fields, keep the simpler snapshot model after fixing no-op history in Batch 5.

## Batch 10 — Low priority: dependency and documentation hygiene

- [ ] **Keep demo/test-only dependencies out of the library build graph.** `rayon` is a normal dependency (`Cargo.toml:35`) but is used only by `tests/visual/diff.rs`; move it to dev-dependencies. `tracing-subscriber` is normal (`Cargo.toml:48`) but is used only by `src/bin/showcase/main.rs:82-87`; make it optional behind an explicit showcase feature/required binary feature, or move the showcase to an example and use a dev-dependency. Verify Aperture still builds standalone as well as inside the Darkroom workspace.

- [ ] **Make public documentation warning-free after sealing the API.** `cargo doc --no-deps` currently emits 97 warnings in this workspace run, mostly because public implementation modules document private internals, plus genuine stale links such as `Ui::damage_filter`, `ResponseState::clicked`, `WindowRenderer::new`, and `Configure::background`. Narrowing modules in Batch 6 will remove much of the noise; fix remaining broken/redundant links and add `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` to the API/documentation verification path.

## Recommended execution order

Implement Batches 1 and 2 before doing performance work; they change ownership identities that later benchmarks must measure correctly. Batches 3-5 are independent correctness batches and can follow one at a time. Batch 6 should precede the dependency/docs cleanup because it determines the final public surface. Batches 8-9 should land only with targeted before/after benchmarks.

For every code batch, use the repository verification gate:

```sh
cargo test && cargo fmt --all && cargo check && cargo clippy --all-targets -- -D warnings
```

Additionally run the targeted multi-window, visual, allocation, and Criterion probes named in each item; the standard suite alone does not currently exercise the critical retained-state interleavings.
