# Crate review

Scope: the whole `palantir` crate and `palantir-anim-derive`, read in twelve
slices. The host / window slice had a dedicated design pass. Items marked
**[public]** change the exported surface and wait for a go-ahead (AGENTS.md
"Public API"). The owner's decisions are in "Decisions".

These bugs were checked against the code: the linear gradient axis, the
nested-overlay panic, the window command order, the input queue admission,
the Scroll disabled fold, the PaintOnly gate, the rounded-clip mask radius and
the measure-cache hit test.

# Part 1: Proposal

## Paint and geometry

- **Validation happens at the setter.** Every shape setter checks its input
  with `#[track_caller]`. The release-silent `LowerShape::has_nan` gate goes.
  `#[track_caller]` and `# Panics` follow each call chain down to its assert.

## Layout

- **Per-node layout state has one copy per frame.** Forest-wide columns that
  measure and arrange write directly, indexed by a layer base. A hit copies
  prev → cur once, and `end_frame` swaps. (Structural: after the bugs.)

## Resources

- **Atlas eviction matches the allocator's unit of reclaim.** Evict whole
  buckets, or use pages, or `etagere::AtlasAllocator`. Single-slot eviction
  on `BucketedAtlasAllocator` frees no space until a bucket empties.
- **GPU timing state is per render owner**, like `GpuViewTargets`.
- **Uploads stage once**, through `StagingBelt::allocate`.

## Data identity and global state

- **A cache hit on text is verified** by a byte compare against the text
  the cached buffer holds.
- **Trusted tables are checked**: `IconTable::baked` checks order and spans
  in a `const` loop. An icon id is bound to its set.
- **No process statics** that the outside world does not force: the
  `FontFamily` name table and the premultiply table move to their owners.

## Tests and visibility

- **An item is `pub` only when code outside the crate calls it.** Test-only
  reach-ins move under `#[cfg(test)]`, so the dead-code lint can find a wrong
  gate.
- **A test lives in the suite of the layer it asserts.** Suites under
  `primitives`, `text`, `theme` and `layout/cache` that drive `Ui` or widgets
  move to the layer they test.

## Decisions

Sources: `.notes/research/2026-10-26-review-decisions.md`.

- **Foreign crates are re-exported whole, only when public signatures name
  their types.** `pub use wgpu;` and `pub use glam;` stay. The partial glam
  re-exports (`IVec2`, `UVec2`, `Vec2`) and the second paths of `prelude`
  go. Refused: no re-exports (each caller must track palantir's exact wgpu
  version by hand); the current partial set (two paths for about 60 items).
  Source: Effective Rust Item 24, egui-wgpu and eframe practice.
- **`golden` reads and writes palantir's `Image`.** Its `pub use image` goes,
  and tests/visual takes `image` as a dev-dependency for its crops. Refused:
  keeping `RgbaImage` in the `golden` signatures.
- **`missing_docs` stays on for the library**, with its reason beside it: a
  published crate documents every exported item. Refused: removing it.
- **A stroke's line is `width`, also on a spinner.** Refused: a shape-based
  exception for rings. Source: Compose Material 3 and Flutter `strokeWidth`.
- **Interruption follows the canonical models.** A retargeted spring keeps
  its velocity. A duration retarget back to the segment's start value uses
  the CSS reversing-adjusted start and shortening factor. Refused: zeroing an
  opposing velocity (a C¹ break); a full restart on reversal. Sources:
  SwiftUI (WWDC23), CSS Transitions 2.
- **`Platform` stays public as the input-convention selector**, for app
  labels such as "⌘S" vs "Ctrl+S". Refused: making it crate-private.
- **The solver keeps the table-layout shrink, and the docs name it.** Hug
  shrink is the CSS 2.1 §17.5.2.2 interpolation, grow is WPF star, and
  overflow aligns to the leading edge. Refused: CSS Flexbox §9.7 scaled
  shrink.
- **A shaped-text cache hit is verified by a byte compare** against the
  text the cached buffer holds; the key stays a hash. Refused: std
  `RandomState`, foldhash and FxHash, none of which is exact; a source copy in
  the key, which would make it neither `Copy` nor small.
- **Double-click time and radius stay constants** (500 ms, 5 px). Refused:
  platform queries and an app-set option.

# Part 2: Plan

Each batch leaves the build and tests green. A batch that needs another one
comes after it. Delete an item when it is addressed.

## Batch 16: GPU resources

Files: `src/gpu/{raster/, frame/, wgpu_backend/, pipeline/, resource/,
surface/capture.rs, device/}`, `src/renderer/gradient_atlas/`,
`src/renderer/frontend/composer/{rect_grid/, occlusion_pruner.rs}`.

- [ ] **Atlas eviction can empty a side for one raster.**
  `raster_atlas/mod.rs:423-442, 481-513`. Target: eviction by the
  allocator's reclaim unit, or `etagere::AtlasAllocator`; count evictions per
  insert.
- [ ] **GPU timings mix windows.** `wgpu_backend/mod.rs:128-134, 184-187,
  348-357`, `gpu_timings.rs:245-252`. Target: slots and stats per
  `RenderOwnerId`.
- [ ] **Two scattered gradient bakes upload most of the atlas.**
  `gradient_atlas/mod.rs:126-128, 253-285`. Target: a sorted run list of
  dirty rows, merged only when a gap is cheaper than a call (measure).
- [ ] **A screenshot allocates a full backbuffer texture.**
  `surface/capture.rs:50-138`. 66 MB at 7680×2160 for any region. Target:
  copy the region directly for 8-bit sRGB formats; a region-sized blit
  otherwise; `take(self)`.
- [ ] **Uploads stage twice and keep a peak-sized buffer.**
  `raster_atlas/mod.rs:119-125, 282-284, 315-383`. Target:
  `StagingBelt::allocate`; delete `staging_buf`.
- [ ] **Icon prewarm warms keys the frame never draws.**
  `icon_backend/mod.rs:82-103`. Target: warm the drawn keys; spread the
  rasters over frames.
- [ ] **A quad form switch re-binds unchanged groups.**
  `wgpu_backend/mod.rs:596-635, 712-735`. Target: `Binder` tracks group 0,
  vertex buffer 0 and the shadow pair apart from the pipeline. Measure with
  `record_pass`.
- [ ] **[public] A clipped `GpuView` reallocates while it scrolls.**
  `gpu_view_targets/mod.rs:303-335`. Target: capacity buckets and a sub-rect;
  changes the `GpuFrameCtx::target` doc contract.
- [ ] **One pipeline recipe.** `pipeline_recipe.rs:7-21`,
  `stencil_variant.rs:170-219`, the four families' `build_variants`.
- [ ] **One run splitter.** Move `quad_pipeline/runs.rs` to `pipeline/`;
  delete `image_runs` and `ImageRun`. Key quad runs from `buffer.quads` and
  shadow runs from `&CutoutPlan` (drop the per-frame copies at
  `quad_pipeline/mod.rs:96-97`, `cutout_tables.rs:136-137`).
- [ ] **`DeviceFeatures` and `SurfaceUsages` mirror code.**
  `device_features.rs:20-56`, `surface_usages.rs:20-59`. Target: one macro
  arm (a generic cannot be `const`).
- [ ] **One tile index.** `rect_grid/mod.rs:34-106`,
  `occlusion_pruner.rs:36-180`. Target: one `TileIndex`; bench before.
- [ ] **The premultiply table is a process static.**
  `wgpu_image_store.rs:30-35, 165-197`. Target: owned by the store.
- [ ] **Schedule tests under `wgpu_backend`.** Move to
  `frame/schedule/tests/`; `MaskPlan` fields private; test binder elision in
  `wgpu_backend/tests`.
- [ ] **Shelved: atlas growth by pages.** `raster_atlas/side.rs:143-166`. A
  grow frame clears and copies up to about 20 MiB. Structural, with no
  measured workload; the `text_atlas` bench is the trigger.

## Batch 17: Text, shapes and icons

Files: `src/text/`, `src/shape/`, `src/icons/`,
`src/primitives/paint/image/mod.rs`.

- [ ] **`ImageShape::fit` validates nothing.** `shape/image.rs:53-56`,
  `paint/image/mod.rs:31-37`. NaN is dropped in release, ∞ reaches the
  shader. Target: `ImageFit::validate` (offset kind, positive scale), called
  with `#[track_caller]` under `# Panics`.
- [ ] **Delete the `has_nan` gate.** `shape/mod.rs:72-73`,
  `shapes/mod.rs:46-58, 109-121`, every shape's `has_nan`. Target: keep only
  the debug assert on the lowered record; a per-setter `# Panics` table
  replaces the gate test. Needs the item above.
- [ ] **Polyline color counts panic at lowering.** `polyline.rs:51-67,
  106-138, 170`. Target: `per_point` and `per_segment` assert the count.
- [ ] **A zero-length curve with a round or square cap paints nothing.**
  `curve.rs:99-115`. Target: emit the cap alone (disc or square).
- [ ] **The recycle pool cap makes resize drags allocate.**
  `shaped_buffer_cache.rs:21, 251-255`. Target: size the pool from the
  observed peak; trim only on a cold signal.
- [ ] **A shaped-text hit is not verified.** `text/key.rs:33-34`,
  `text/request.rs:33-40` (debug-only check). Target: `TextShapeKey` stays a
  24-byte hash key. A hit compares the request's bytes (this pass's text
  arena) with the text the cached cosmic `Buffer` already holds: for
  unbounded and `Wrap` entries its lines plus endings are the source, as
  `LineMap::new` (`probe/mod.rs:331-351`) already walks. Only a
  `Clip`/`Ellipsis` entry, whose buffer holds `prefix + "…"`
  (`cosmic/mod.rs:640-650`), keeps its source in a `String` recycled with
  the buffer, so a miss allocates nothing after warmup.
- [ ] **`IconTable::baked` checks nothing.** `icon_table.rs:44-54`. Target:
  `const` checks of order and spans under `# Panics`.
- [ ] **Probe queries rescan the text.** `probe/mod.rs:331-351`. Target: scan
  only for `Clip`/`Ellipsis`; one `LineMap` per probe.
- [ ] **`FontFamily` is a process-global table.** `font_family.rs:8-10, 48,
  77-95`. Target: a named family, mapped to a key index per shaper.
  **[public]** (type semantics).
- [ ] **Mono and cosmic disagree.** `text/mono.rs:38-83`. Target: mono splits
  hard breaks and wraps on break offsets.
- [ ] **[public] `IconTableError::Unreadable` drops its cause.**
  `svg_facts.rs:108-109`, `icons/error.rs:11-15`.
- [ ] **[public] A cross-set `IconId` draws the wrong icon.**
  `icon_set.rs:50-66`. Target: `by_name` returns the set-bound handle.
- [ ] **[public] Glyph and probe names.** `PlacedGlyph { x, y }` → physical;
  `raster_scale` → `raster_factor` (and `GpuFrameCtx`); `byte_at(pos: Vec2)`;
  `selection_rects(range, out: &mut Vec<Rect>)`.
- [ ] **Tests at the wrong layer.** `text/tests/fonts.rs`,
  `text/tests/mod.rs:30`, `text/wrap/tests.rs:115` move to `ui` and `layout`.
  `a_scanned_shaper_arrives_usable` uses `FontScope::Bundled`.

## Batch 18: Primitives public surface

All **[public]**. Files: `src/primitives/`, `src/common/platform.rs`,
`palantir-anim-derive/`.

- [ ] **Panic chains lose the caller.** `stops/mod.rs:107-174`,
  `gradient_builder.rs:35-63`, `color_ramp.rs:24-35`, the three geometries'
  `new`/`two_stop`, `translate_scale/mod.rs:89-97`. Target: `#[track_caller]`
  through each link and `# Panics` on each public item. (Attributes and docs
  only; not a signature change.)
- [ ] **`RgbaF32::from_str` errors with `&'static str`.**
  `color/mod.rs:243-288`. Target: `ParseColorError { Length, Digit }`.
- [ ] **A private `Num` bound and lossy `From`.** `math/num/mod.rs:146-170`.
  Target: a sealed public marker; drop `f64` and the 64-bit and size types.
- [ ] **`as_array` computes.** `primitives/mod.rs:9-14`. Target: `to_array`.
- [ ] **`ColorCoords::new` is a conversion.** Target: `from_color`.
- [ ] **Checked values without getters.** `TranslateScale`, `Track`; add
  `inverse_point`.
- [ ] **Gaps.** `RgbaF32::hexa`; `IntoIterator for &GradientStops`.
- [ ] **`#[animate(skip)]` aliases `snap`.** `anim-derive/src/lib.rs:228-233`.
- [ ] **`Platform`'s doc invites widgets to branch on the OS.**
  `common/platform.rs:3, 14`. Target: the doc names it the input-convention
  selector, for app labels such as "⌘S" vs "Ctrl+S".

## Batch 19: Test infrastructure and visibility

Files: `src/internals/`, `src/bench/`, `src/golden/`, `tests/`,
`src/renderer/frontend/composer/{bench.rs, tests/, mod.rs}`,
`src/primitives/layout/visibility/tests.rs`, `Cargo.toml`.

- [ ] **`Goldens::orphans` hides read errors.** `golden/mod.rs:158-160`.
  Target: empty only on `NotFound`; panic with the path otherwise.
- [ ] **About 30 `UiHarness` methods and `Passes` are `pub` for crate tests
  only.** `ui_harness/mod.rs:166-516` and below, `ui_harness/passes.rs`,
  `bench/mod.rs:16`. Target: `pub(crate)` under `#[cfg(test)]`; `bench::fixture`
  `pub(crate)`; rewrite the tier paragraph at `mod.rs:52-55`.
- [ ] **Each golden comparison leases the GPU for the adapter name.**
  `tests/visual/goldens.rs:17-19`. Target: build the suite once.
- [ ] **Dead warmup floor in the alloc probe.**
  `tests/alloc/harness/mod.rs:256-260`.
- [ ] **Copied fixtures.** One `ICON_SVG` and an icon-parking helper in
  `tests/alloc/fixtures/mod.rs`. `ComposeRig` moves to the composer's
  `internals` so `composer/bench.rs:44-60` uses it with
  `TEST_MAX_TEXTURE_DIM`.
- [ ] **A probabilistic race loop with file I/O.** `golden/tests.rs:199`.
  Target: a few iterations, or a direct test of the staged rename.
- [ ] **`rect_grid` widened for a bench.** `composer/mod.rs:28-29`. Target:
  private; `composer::bench` calls it.
- [ ] **Tests at the wrong layer.** `primitives/layout/visibility/tests.rs`
  cases move to the layout, encoder and input suites.
- [ ] **File names.** `internals/dock_fixture.rs`; `bench/kind.rs` and
  `bench/backends.rs`.
- [ ] **Docs.** `tests/alloc/gates/on_gpu.rs:10` (no CI skip by name),
  `ui_harness/tests/fixtures.rs:26`.
- [ ] **`[profile.dev] opt-level = 1` has no reason.** `Cargo.toml:106`.
- [ ] **[public] Partial glam re-exports and `prelude`.** `lib.rs:186-194,
  351-355`. Target: `pub use glam;` whole beside `pub use wgpu;`; the three
  glam items and `prelude` go; callers write `palantir::glam::Vec2`.
- [ ] **[public] `golden` names `image` types.** `golden/mod.rs:9, 41, 88,
  197, 214`. Target: `Goldens` and `DiffReport` take and hold palantir's
  `Image`; `pub use image` goes; tests/visual takes `image` as a
  dev-dependency for `imageops`.
- [ ] **`missing_docs` has no reason.** `lib.rs:9`. Target: a one-line
  reason beside it.
- [ ] **The global guide forbids the re-exports the crate keeps.**
  `~/.claude/CLAUDE.md` "No crate re-exports another crate". Target: one
  exception, "a crate whose public signatures name a foreign type
  re-exports that crate whole".
