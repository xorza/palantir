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

- [x] **Separate non-overflow scroll bounds from zoom rubber-band bounds.** `ScrollLayoutState` now derives raw leading/trailing endpoints once, collapses settled underflow at the leading edge, and retains the ordered raw interval only for zoomable wheel pan (`src/layout/scroll/mod.rs:114-175,200-237`). `Scroll` explicitly selects that zoom-underflow behavior while ordinary scrolls settle to semantic natural bounds (`src/widgets/scroll/mod.rs:562-582`). The wheel table covers both directions on undersized ordinary content, and the zoom regression hand-checks pointer-pivot offset followed by further underflow pan (`src/widgets/scroll/tests.rs:82-120,586-616`).

## Batch 4 — High: layout and analytic-geometry correctness

- [x] **Cap Grid's min-content floor by `Track::max` in both Hug and Fill resolution.** The shared `content_floor` canonicalizes `min-content.max(Track.min).min(Track.max)` and now drives Hug bounds, both Fill clamp sites, and Grid intrinsic aggregation, so every lower bound is ordered beneath the cap (`src/layout/grid/mod.rs:849-852,919-999,1093-1095`). The existing Hug and Fill clamp tests now share a rigid-first-column fixture: cramped Hug caps a 200px intrinsic floor at 150px, while Fill caps it at 100px and gives the exact 300px remainder to its sibling (`src/layout/grid/tests.rs:20-45,180-218,220-285`).

- [x] **Track WrapStack line occupancy explicitly instead of using `line_main > 0`.** `LinePack` now carries main/cross extents plus an explicit occupied bit; the shared packing path uses it for wrapping and gap insertion, while measure and arrange both use it for final-line flushing (`src/layout/wrapstack/mod.rs:38-94,142-164,198-295`). A table-driven regression pins lone and followed zero-main children on both axes, including exact parent size, child size, and gap position (`src/layout/wrapstack/tests.rs:26-108`).

- [x] **Define and enforce `Element` min/max precedence at the authoring boundary.** Both setters now validate the candidate/current pair through one per-axis debug assertion, so negative, NaN, and inverted bounds fail at the setter that creates them without adding release cost to the per-widget authoring path (`src/forest/element/mod.rs:250-263,318-330`). Regression tests accept equality in both setter orders and reject all four x/y × setter-order inversions (`src/forest/element/tests.rs:158-206`).

- [x] **Reject all zero-area triangles before lowering.** `triangle_paint_empty` normalizes the absolute cross-product area by the longest squared edge and applies the shared paint tolerance, making the authoring no-op gate scale- and winding-independent while dropping repeated, collinear, and near-degenerate inputs before `ShapeRecord` construction (`src/shape.rs:735-746,767-774`). A threshold table covers both windings, exact degeneracy, and matching below/above-cutoff triangles at 1× and 100× scale (`src/shape.rs:843-928`).

## Batch 5 — High: persistent state, animation, and edit-history correctness

- [x] **Key tooltip state by the recorded trigger ID.** Per-trigger state now uses the recorded trigger ID directly, so the normal removed-ID sweep reclaims it; only `TooltipGlobal` retains a synthetic singleton key. Both hover and warmup timestamps are `Duration`, with exact monotonic subtraction and no long-uptime float conversion (`src/widgets/tooltip/mod.rs:19-39,120-174,206-207`). Regressions prove trigger removal evicts only the per-trigger row and that a 250 ms delay remains exact after `2^24` seconds (`src/widgets/tooltip/tests.rs:90-155`).

- [x] **Reset animation-mode state when `AnimSpec` changes kind without a target change.** `AnimRow` now stores an `AnimKind`, and every kind transition clears velocity and elapsed time while restarting the segment from the current value before steady-state or retarget short-circuits (`src/animation/mod.rs:78-160,224-252`). Exact same-target regressions pin mid-flight Spring-to-Duration and Duration-to-Spring-to-Duration restarts (`src/animation/tests.rs:912-972`).

- [x] **Do not mutate undo/redo history for a rejected TextEdit insertion.** `Editor` now computes the UTF-8-safe prefix that fits after planned selection deletion and returns before `record_edit` when neither deletion nor insertion would occur (`src/widgets/text_edit/model.rs:189-224`). Exact regressions prove a rejected capped insertion preserves the redo tail and an active Delete coalescing group, while replacement of a selection at the cap still lands normally (`src/widgets/text_edit/tests/undo.rs:59-112`, `src/widgets/text_edit/tests/apply_key.rs:288-312`).

## Batch 6 — High: make the public boundary coherent

- [x] **Internalize the renderer driver and make `lib.rs` the only supported type namespace.** Every production implementation parent is crate-private and the blanket private-interface lint suppression is gone (`src/lib.rs:8-28`); supported `OffscreenHost`/`WinitHost` facades and consumer vocabulary are re-exported only at the root (`src/lib.rs:30-151`). `RecordStore`, `Frontend`, `WindowRenderer`, its frame methods, `FrameTarget`, and `FramePresent` remain internal driver details (`src/record_store.rs:57`; `src/renderer/frontend/mod.rs:31-68`; `src/host/window_renderer.rs:33-48,284-393,598-631`). `RenderPlan`/`RenderKind` and the detailed damage region stay crate-private while `FrameReport::paint` exposes the stable skip/full/partial classification, with exact coverage of every plan shape (`src/ui/frame_report.rs:15-60,82-165`). The only feature-gated external surfaces are the dedicated benchmark namespace and narrow integration-test helpers; neither publishes production implementation types (`src/lib.rs:8-10,30-31`; `src/bench/mod.rs:1-28`).

- [x] **Centralize benchmarks behind a narrow `internals` facade before sealing modules.** Every Criterion and `dhat` driver now lives under `src/bench/` in a hierarchy mirroring its production subsystem: damage/cascade, text atlas/shape, layout caches, input, composer, and allocation workloads call crate-private production types directly from `src/bench/{ui,renderer,text,layout,input,allocation}`. The cross-subsystem frame driver and shared allocation fixture have their own neutral `src/bench/frame/` hierarchy (`src/bench/frame/mod.rs:1-63`; `src/bench/frame/fixture.rs:1-109`). One `internals`-gated facade exports benchmark entry functions plus the benchmark-owned `FrameFixture`, with no production implementation structs (`src/lib.rs:8-10`; `src/bench/mod.rs:7-28`). Criterion targets are five-line wiring callers except the nine-line custom-config frame target; each six-line `dhat` target retains only its required global allocator and one facade call (`benches/frame.rs`; `benches/alloc_free.rs`; `benches/alloc_free_gpu.rs`; `benches/alloc_resize.rs`).

## Batch 7 — Medium: tighten public data contracts

- [x] **Represent only supported brushes in each `Shape` variant.** Triangle fill, Text color, and Mesh tint now store `Color`, while Line/Bezier/Arc use `CurveBrush`, whose public variants are limited to solid color and linear gradient (`src/shape.rs:61-192`; `src/primitives/brush.rs:459-492`). `Stroke` is likewise solid-color-only, eliminating the remaining gradient-bearing stroke state (`src/primitives/stroke.rs:5-51`). Shape lowering shares solid and linear helpers and contains no unsupported-brush assertions or unwraps (`src/forest/shapes/lower.rs:64-128,265-375`). Exact unit coverage pins supported curve-brush conversion/no-op behavior and rejects a gradient triangle fill at the builder boundary (`src/shape.rs:939-990`).

- [x] **Repair float `Eq`/`Hash` consistency and centralize content hashing.** Exact `eq_bits` plus f32/Vec2/Size/Rect helpers now collapse only signed zero for equality-compatible public hashes; `Color`, `Size`, `Rect`, `Sizing`, packed `Sizes`, and `Track` all route through that contract (`src/primitives/approx.rs:20-80`; `src/primitives/color.rs:22-30`; `src/primitives/size.rs:10-14`; `src/primitives/rect/mod.rs:12-16`; `src/layout/types/sizing.rs:67-75,112-119`; `src/layout/types/track.rs:83-89`). Separate visual helpers collapse zero noise and NaN payloads only at content-cache boundaries: element/grid hashes, shape records, mesh payloads, cascade fingerprints/inputs, and approximate-default row elision now agree on the same semantics (`src/forest/element/columns.rs:79-145`; `src/layout/types/track.rs:70-73,112-124`; `src/forest/shapes/hash.rs:23-202`; `src/primitives/mesh.rs:113-126`; `src/ui/cascade/mod.rs:507-551,749-798`). Polyline hashing walks canonical points and the already-lowered `ColorU8` slice (`src/forest/shapes/lower.rs:199-254`). Exact regressions cover signed-zero public hashes, canonical visual NaNs, zero-noise cache equivalence, lowered-color equivalence, and the retained 32-byte cascade-prefix hot path (`src/primitives/approx.rs:168-201`; `src/primitives/color.rs:634-641`; `src/primitives/rect/tests.rs:12-19`; `src/primitives/size.rs:151-158`; `src/forest/tree/tests.rs:250-283`; `src/ui/cascade/tests.rs:28-53`).

- [x] **Reject excess sequence values and duplicate fields in shared lane deserialization.** The shared visitor now probes once beyond the fourth lane with `IgnoredAny` and reports an invalid length when a fifth value exists; map fields reject a second occurrence before it can overwrite the first (`src/primitives/lane_serde.rs:83-113`). Its expectation text now documents the intentionally supported scalar, one-element, two-element, four-element, and named-table forms (`src/primitives/lane_serde.rs:65-71`). Direct `SeqDeserializer`/`MapDeserializer` regressions prove accepted lane expansion and missing-field defaults remain unchanged while lengths 0, 3, and 5 plus duplicate keys return exact serde errors (`src/primitives/lane_serde.rs:118-190`).

- [ ] **Make `TextRectGrid` overflow structurally safe in release builds.** Tile buckets store `u16` rect indices, and `push` relies only on a `debug_assert!` before the narrowing cast (`src/renderer/frontend/composer/text_grid.rs:30-31,112-129`). A batch beyond the index range wraps to old rects and can miss a paint-order conflict. Keep hot-path assertions debug-only, but prevent the invalid state by flushing before the capacity limit, switching to `u32`, or entering a correct linear-overlap fallback. Add an overflow-boundary test without constructing a full GPU frame.

## Batch 8 — Medium: bounded CPU hot-path wins

- [ ] **Partition higher-kind overlap tracking by paint tier and add union pre-rejects.** `Composer::higher_kind_conflict` scans the whole `higher_kind_rects` vector for each Mesh/Image (`src/renderer/frontend/composer/mod.rs:382-411`), even though same-tier entries cannot conflict. Many same-tier draws therefore take quadratic predicate work. Store retained per-tier rect sets (or at minimum per-tier ranges) with union AABBs; scan only later-replaying tiers after an O(1) union miss test, and use an aggregate union for kind-blind queries. Extend `src/bench/renderer/frontend/composer.rs`, which currently measures only the Curve early-return path, with same-tier Mesh/Image and mixed-tier overlap/non-overlap cases.

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
