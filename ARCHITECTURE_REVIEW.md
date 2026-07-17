# Aperture architecture review

## Executive summary

Aperture's core decomposition is sound: immediate recording produces a compact tree and flat shape payloads, layout and cascade consume that immutable frame description, and the renderer lowers it through explicit command and render buffers. The Tree/Shape separation, cross-frame measure cache, fixed backend paint tiers, and deliberately distinct Stack/Grid solvers should remain intact.

The main risks are narrower contract gaps at subsystem boundaries. The host lifecycle replay issue is now resolved; several public numeric and layout representations still admit states their consumers cannot handle, three layout paths disagree with the documented sizing contract, and two renderer optimizations use non-conservative coverage bounds even though false negatives change pixels or paint order. Most fixes are boundary tightening and shared-policy consolidation rather than a broad rewrite.

This report contains 20 actions in six implementation batches, with fourteen now completed. The first four batches are correctness work; the last two are performance, capacity, build, and documentation cleanup. Review scope was production code under `src/`, `anim-derive`, the crate manifest, and architecture documents. Tests, benches, showcase code, and examples were excluded as review targets.

Verification baseline while reviewing:

- `cargo check -p aperture` passed.
- Each profiler feature passed independently: `profile-with-tracy` and `profile-with-puffin`.
- `cargo check -p aperture --all-features` failed inside `profiling` because both mutually exclusive backends define the same macros.

## Current flow

```text
Ui::frame
    -> App::update (once)
    -> App::record (cold warmup, pass A, optional pass B)
    -> Forest Tree + flat Shapes + RecordStore payloads
    -> post_record subtree/hash finalization
    -> Measure (cached, post-order) -> Arrange (pre-order)
    -> Cascade + hit index -> Damage
    -> Encoder -> RenderCmdBuffer
    -> Composer -> RenderBuffer groups/batches
    -> WgpuBackend schedule -> quads/text/meshes/images/curves
```

`StateMap`, `AnimMap`, `MeasureCache`, `TextShaper`, image/gradient resources, and GPU-view targets are the cross-frame state around that otherwise frame-local pipeline.

## Batch 1 — Critical/High: separate replayable UI construction from external effects

- [x] **Split the once-per-recorded-frame application phase from replayable UI recording.** Implemented as the backend-neutral `App` trait in `src/app.rs`; both `WinitHost` and `OffscreenHost` pass the app directly to `Ui::frame`, which owns the once-only update boundary before cold-start warmup or relayout replay. `src/host/winit/tests.rs` pins current display/input visibility, one update against both replay paths, and neither hook running on paint-only frames.

- [x] **Make clipboard write failure observable before Cut destroys the source.** Implemented with an injectable two-tier clipboard backend: a failed OS write makes the in-memory fallback authoritative until a later successful replacement write, and `TextEdit::cut` preserves its selection when neither destination accepts the text. Tests pin the stale-successful-read sequence, primary recovery, and rejected Cut transaction.

## Batch 2 — High: make invalid authoring states unrepresentable

- [x] **Remove payload-bearing modes from the generic public `Element` constructor.** `LayoutMode` is now crate-private; custom widgets use named `Element` constructors for the seven payloadless modes, while crate-only Grid and Scroll constructors install their payload atomically. The raw initializer is private to `Element`, and the external custom-widget example pins the supported authoring surface.

- [x] **Integrate container text without restricting generic shape recording.** Direct text on a Leaf remains layout content; direct text on containers is paint-only and shapes after arrange against the final padded width. Sparse owner discovery is folded into subtree rollup finalization, so only containers with direct text take the post-arrange shaping path. Text that must participate in Stack/Grid layout remains a child `Text` widget.

- [x] **Validate `Sizing` values and centralize zero-share construction.** `Sizing` is now opaque and exposes finite validated `fixed`, `fill`, and zero-safe `share` constructors. Splitter, ProgressBar, and Slider use `share`, so endpoint proportions become exact zero-sized fixed segments instead of invalid Fill weights. Stack and Grid accumulate weights in `f64` and share one overflow-safe proportional calculation; packed `Sizes` preserves the positive Fill invariant even for the smallest subnormal input while trusted decoding avoids validation in the layout hot path. Tests cover invalid/non-finite construction, quantization, exact widget endpoints, and `f32::MAX` sibling weights in both solvers.

- [x] **Encode the positive-finite pan/zoom invariant in `TranslateScale`.** Both fields and every constructor accept zero, negative, NaN, or infinite scale (`src/primitives/transform.rs:13`, `src/primitives/transform.rs:45`), while `apply_rect` assumes multiplication preserves a canonical positive-size rectangle (`src/primitives/transform.rs:150`). Text hit-testing divides by the scale (`src/widgets/text_edit/input.rs:107`), and composer paths use it as a positive multiplier for radii, widths, bounds, mesh instances, and text (`src/renderer/frontend/composer/mod.rs:493`, `src/renderer/frontend/composer/mod.rs:762`). Make fields private and construction validate finite translation plus positive finite scale; composition should preserve that type invariant. If mirroring is desired, it needs a separate affine representation and canonical min/max handling throughout. Validate zero/negative/non-finite rejection and nested transform composition across cascade hit-testing and composer bounds.

- [x] **Validate animation specifications at construction and deserialization.** `AnimSpec` exposes unrestricted float variants and derives `Deserialize` (`src/animation/mod.rs:68`); its constructors perform no checks (`src/animation/mod.rs:102`). NaN/infinite duration bypasses the instant path, produces NaN or zero progress, and never satisfies the settle comparison (`src/animation/mod.rs:110`, `src/animation/mod.rs:341`); unstable, non-positive, or undamped spring parameters can likewise diverge in the fixed-step Euler loop (`src/animation/spring.rs:63`). Every unsettled row requests another repaint (`src/ui/mod.rs:1128`). Use a private validated representation plus custom serde: duration must be finite and positive or canonicalized to instant, and every accepted spring must have a documented stable domain. Prefer an unconditionally stable/closed-form spring or adapt substeps to stiffness rather than accepting arbitrary finite values into a fixed step. Invalid theme data must return a serde error; every accepted spec must remain finite and settle under deterministic frame sequences.

## Batch 3 — High: restore one arrange-time sizing contract across drivers

- [x] **Centralize arrange-axis resolution so Fixed, Fill floors, alignment, and max bounds cannot drift.** Root, Canvas, Stack, WrapStack, ZStack, and Grid now share `arrange_axis`, which reads canonical per-node style and bounds directly. Fixed retains its resolved extent; Fill and Stretch preserve the measured floor, respect min/max bounds, and align after capping. Stack and Grid keep their distinct main-axis distribution solvers. Cross-driver tests sweep both axes and margins for undersized Fill, max-capped growth, Fixed under Stretch, and Center/End placement after capping; Scroll tests pin viewport shrinkage below content, and the HiDPI fixture declares explicit row caps instead of relying on the old inconsistent shrink behavior.

- [x] **Include internal gaps when Grid measures a spanned cell.** Phase 2 now passes the axis gap into `known_span_size`, which adds internal gaps only after every track in the span is resolved and otherwise preserves the infinity fallback. Exact tests cover two- and three-track wrapping text plus nested horizontal and vertical WrapStacks, pinning identical measured and arranged extents; the resolved-track unit test separately verifies `50 + 10 + 30 = 90` and an unresolved Fill span remaining infinite.

- [x] **Preserve the content floor for all-Fill WrapStack lines.** Line packing now uses every child's measured desired cross extent, including Fill children, while shared arrange-axis placement remains the sole owner of stretching and min/max enforcement. Exact axis-symmetric tests cover one and multiple all-Fill lines, a Fill child establishing a mixed line's extent, and explicit min/max bounds.

## Batch 4 — High/Medium: make renderer bounds conservative wherever they affect pixels or order

- [x] **Do not treat fractional anti-aliased quads as fully opaque covers.** Only pixel-aligned fast-path quads now record their full opaque cover; every other solid opaque quad insets its corner/stroke-safe cover by the shared physical-pixel `AA_RADIUS`. The composer and specialized WGSL consume the same constant. Exact composer tests pin the half-pixel boundary and retain aligned pruning, while an unsnapped offscreen fixture compares identical fractional layers against a clip-separated unpruned reference pixel-for-pixel.

- [x] **Use the full painted shadow bound for paint-order overlap.** Shadow quads now pass their canonical full 3-sigma physical paint bound to the existing overlap-order check, so an earlier higher-tier draw intersecting only the outer halo forces a group split and retains authoring order. The obsolete `URect::deflated` optimization is removed, and a composer regression test places earlier text exclusively in the 2-sigma to 3-sigma ring and pins the resulting group and batch order.

- [x] **Reject rounded-clip chains deeper than the stencil can represent.** The shared rounded-chain contract now owns the eight-bit stencil limit used by both composer and backend mask states. Composer rejects the 256th distinct mask through an out-of-line cold panic before extending frame storage or reaching GPU submission, while depth 255 remains valid. Exact boundary tests pin both outcomes without deriving their expectations from the implementation constant.

- [x] **Check glyph raster metadata before narrowing it into the packed atlas representation.** Atlas-owned `PackedGlyphMetadata` now checks raster dimensions and placement before any wire-type conversion. Unsupported metadata emits a diagnostic and is cached through the existing unallocated-slot path, preventing repeated rasterization and warning churn while the glyph remains active. Exact tests cover zero, every accepted `u16`/`i16` boundary, and each first out-of-range width, height, left, and top value without requiring a giant font.

## Batch 5 — Medium: remove duplicated work and silent capacity failures

- [x] **Store natural Grid track declarations without per-frame heap allocation.** `Grid` now retains arrays inline and borrowed slices by reference until `show`, then copies both axes into a capacity-retained Tree track arena addressed by `GridDef` spans. Layout reads those arena slices directly instead of retaining `Rc` ownership in axis scratch, and Splitter records stack arrays without duplicate state buffers. Exact empty/small/large layout and hash tests pin definition behavior; the natural inline 8×8 and 16×16 allocation fixtures remain at zero allocations after warmup.

- [ ] **Choose one owner for stroked-shape bbox inflation.** Curve/arc lowering already expands bounds for half-width, cap reach, and AA (`src/forest/shapes/lower.rs:383`), polyline lowering includes join/cap/AA reach (`src/forest/shapes/lower.rs:418`), and the command payload documents its bbox as already inflated (`src/renderer/frontend/cmd_buffer/payload.rs:403`). Composer then inflates it again by width and AA (`src/renderer/frontend/composer/mod.rs:1442`), increasing offscreen survivors, overlap false positives, and group flushes. Keep a clearly named geometric/centerline bbox for physical-pixel composition inflation and a separate conservative logical paint/damage bbox, or make the existing inflated bbox authoritative and stop reinflating it. Validate exact physical bounds across cap/join kinds, widths, transforms, and 0.5x/1x/2x display scales, then compare group counts.

- [ ] **Replace debug-only packed-index checks with non-aliasing capacity types.** Sparse `Slot`, `GridDefId`, and paint-animation indices reserve `u16::MAX` as a sentinel but debug-check then cast `usize` in release (`src/forest/tree/extras.rs:7`, `src/layout/types/layout_mode.rs:17`, `src/forest/tree/paint_anims.rs:250`). At 65,535 entries the sentinel or an earlier index is reused, silently binding nodes to unrelated side-table data. Introduce one checked non-sentinel index constructor with an out-of-line cold overflow path and use it for all packed registries; merely using `u16::try_from` is insufficient because it accepts the reserved maximum. Validate 65,534 as the last valid index and reject 65,535 directly at the constructor boundary.

- [ ] **Validate Tooltip timing at the same serialized boundary as animation timing.** `TooltipTheme` derives `Deserialize` while exposing raw `f32` delay and warmup (`src/widgets/theme/tooltip.rs:16`, `src/widgets/theme/tooltip.rs:30`), and the widget also accepts a raw override (`src/widgets/tooltip/mod.rs:102`). Every show converts both with `Duration::from_secs_f32`, which panics for negative, NaN, or infinite values (`src/widgets/tooltip/mod.rs:119`). Reuse a validated finite non-negative seconds type for theme fields and the builder, with custom serde and conversion outside the recording hot path. Validate zero-delay/warmup semantics and deserialize-time rejection of negative/non-finite values.

## Batch 6 — Low: simplify feature topology and repair misleading contracts

- [ ] **Select a canonical profiler backend instead of publishing mutually exclusive additive features.** The manifest exposes Tracy and Puffin as normal Cargo features (`Cargo.toml:143`), but enabling both activates duplicate macro implementations in `profiling`; `--all-features` therefore cannot build. The feature-matrix script claims broad combination coverage but tests neither backend (`scripts/test-all.sh:1`, `scripts/test-all.sh:40`). Because the host already contains Tracy-specific frame integration (`src/host/window_renderer.rs:339`), the default recommendation is to retain Tracy and remove Puffin plus its feature/dependency. If both remain necessary, isolate backend selection so one dependency feature is chosen per build and add each supported combination to the matrix. The final topology should make the supported aggregate build succeed rather than fail in a transitive crate.

- [ ] **Correct lifecycle and layout documentation that currently states the opposite of production behavior.** `GpuView::repaint(false)` says its offscreen texture remains alive and later repaint will not reinitialize (`src/widgets/gpu_view.rs:66`), while the `GpuPaint` contract and backend say a culled target is reclaimed and `init` runs again (`src/renderer/gpu_view.rs:35`, `src/renderer/backend/image_pipeline/render_target.rs:81`). Separately, the intrinsic design note says Stack pass 1 always measures the main axis at infinity (`src/layout/intrinsic.md:148`), while production deliberately propagates a committed finite main bound (`src/layout/stack/mod.rs:193`) and `AGENTS.md:52` calls that behavior canonical. Update both documents to name the actual lifetime and finite-bound rules, and link each statement to its source-of-truth implementation so future refactors do not restore the obsolete behavior.

## Open design decisions

- [ ] **Confirm that negative scale is not intended to mean mirroring.** The current pan/zoom, hit-test, rectangle, radius, and stroke paths all assume positive scale. If mirroring is a goal, it should be designed as a separate affine feature rather than admitted accidentally through `TranslateScale`.
