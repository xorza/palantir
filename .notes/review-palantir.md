# Review: palantir

Scope: the whole crate (`src/`, `tests/`, `benches/`, `examples/`, `palantir-anim-derive`), at
`a26bb3a97`. Tests at review time: 2,275 lib tests in 2.35 s parallel (7.2 s serial), 45 alloc
tests in 0.76 s. No test passes 1 s.

## Part 1: Proposal

The crate is in good shape. The pipeline is flat and retained, no per-frame allocation was found
on the encode, compose or GPU paths, and the numeric core (f16 lanes, sRGB, Oklab, springs) is
correct. The findings sit at the edges. They come from seven root causes. The design below removes
each cause, not each symptom.

### R1. A sentinel or a check that gives a legal value a second meaning

The code uses a value to mean "absent" or "no-op" where that value is also a legal input, or does
arithmetic before the check that bounds it.

- `LoweredBackdrop` reads all-zero lanes as "no pane", but saturation 0 is a legal gray pane.
- `TextShape::is_noop` includes a transparent color, but text is a layout input, so a transparent
  label loses its size.
- `before_storing` decrements a `u8` derived from a negative value. Release wraps it to 255.
- `Placement::available` matches the cap and the origin as alternatives, so a cap enlarges the
  extent.
- `ZoomFactor::new` admits subnormals whose inverse is infinite.

**Target.** Pane-ness, no-op-ness and validity are decided once, at lowering or at the checked
constructor, from the predicate that owns the rule (`BackdropFilter::is_noop`, `domain::*`), and
carried as a type (`Option<LoweredBackdrop>`, a checked newtype). A record-time drop applies only
to what no later stage reads. Paint-only no-ops are dropped at emit, as they are today.

### R2. One truth stated in two places

Each pair below drifted or can drift. The target keeps one owner and derives the other.

| Truth | Copies | Owner after |
| --- | --- | --- |
| Device limits palantir needs | `DeviceRequirements::against` sweep vs `met_by` check | `against` alone |
| Disabled input | `ResponseState::merge_disabled` vs nothing in `pointer_actions` | the press `Capture` |
| Shortcut key names | `write_key` vs `parse_key` | one named-key table |
| Pane capture row | `PaneRow` copy, `PaneTicket`, `BackdropRow`, `live_panes` | `PaneCapture`, embedded |
| Tier rects | `TierRects.rects` + `RectGrid.rects`, two unions | `TierRects` over a `TileIndex` |
| Window facts | `WindowFacts` vs `WindowFrameState` fields | `WindowFrameState { facts }` |
| `LayoutMode` tags | encode file vs decode file | one file, `TAG_*` consts |
| Device epoch | `Ui::device_epoch` vs `FrameRuntime::recorded` | `FrameRuntime` |
| Frame timing | `collect_gpu_stats: bool` vs `FrameTiming` | `FrameTiming` |
| Text-hash collisions | three docs, two code policies | `text/key.rs` |
| Subtree-skip soundness | cascade "only while every rect held" vs damage "by induction" | `layout` module doc + oracle |
| Edit edges | `ValueEdges` (test only), `Edit`, tuples | `ValueEdges` in production |

### R3. Modules filed by where state is keyed, not by who owns the policy

These produce every cycle beyond the two AGENTS.md records, and they weaken the two it records.

- `common::clipboard` holds an `Image`, so the leaf layer imports `primitives`.
- `checked::font_size_rule` borrows `GlyphFont`'s rule, so `primitives` imports `text`.
- `BackendResources` lives in `gpu::device` and pulls in icons, gradients and the shaper.
- `Backbuffer` stores the blur's `TargetSource`, so `surface` imports `backdrop`.
- `PassRecorder` lives in `frame`, and five pipeline families import it back.
- The stencil pipeline states live in `surface`.
- `RenderTargetDraw` carries the app's `GpuPaintRef` callback through the CPU→GPU contract.
  AGENTS.md's reason for the gpu ⇄ renderer cycle is wrong: `ui` sits to the right of `gpu`, so
  `ui → gpu` is allowed, and moving the trait alone cuts nothing.
- `App` is the frame cycle's callback, so it belongs beside `FrameCycle`, not in its own top-level
  module.

**Target module layout.**

```text
common (leaf) → primitives (math::domain owns every validation kind, font length included)
  → {animation, text, input, display, window, clipboard, image_registry, diagnostics}
  → shape → icons → scene → layout → cascade → {damage, input_state}
  → renderer (ids only; no gpu import) → gpu → theme → ui (App, GpuViews) → widget_core
  → widgets → driver → host
gpu: device ← resource ← pipeline (PassRecorder, stencil states) ← frame
     device ← surface ← backdrop (blur source on RenderStream)
     wgpu_backend (BackendResources) → all of them
```

Both recorded cycles go away, and AGENTS.md drops their two bullets.

### R4. A policy held by one member of a family instead of the family's owner

- Theme orchestration (pair, transition, scheme, epoch) is four `Ui` fields and ~130 lines of `Ui`
  methods, with the epoch bump written by hand at four sites.
- The two headless hosts each write the per-frame glue: wake, record, commands, submit and system
  setters.

**Target.** A `WindowTheme` owns the theme state and bumps its
epoch on every change of `shown()`. A `SoloWindow` owns one headless window's frame step, and each
host keeps only its policies.

### R5. GPU memory that grows to a high-water mark and never decays

- One full-window 4K glass pane pins about 240 MiB: the results atlas at 4096², plus capture,
  levels and scratch. Two panes take the atlas to 8192², which is 512 MiB.
- `WgpuImageStore` keeps a premultiply buffer as large as the largest translucent image. A 64 MP
  image keeps 256 MiB.
- `BackdropBlur::lay_out` regrows per batch, so it makes textures no pass reads.

**Target.** Per-frame scratch is transient. It steps down after K idle frames, and it grows once
per frame to the frame's maximum. A cache shrinks back to its budget when its live set fits. A
staging buffer works in fixed bands, not whole images.

### R6. Sibling widgets that each answer the same question their own way

Overlay ids, style slots, outside drivers (`open`/`close`/`is_open`, `edit`/`is_editing`), key maps
per role, selected state in menus, the lifecycle shape and body return values. Most fixes change
the public API, so they wait for a go-ahead (AGENTS.md, "Public API") in `.notes/api-pass.md`.

### R7. Test oracles and fixtures copied instead of shared

There are three cascade-versus-cold comparators and three layout warm-versus-cold oracles, each
checking a different subset of columns. A Tab-table corruption or a `scroll_content` restore bug
therefore passes some suites. Four copies of the backdrop "fresh window, one frame, read back"
helper exist, and each reference frame builds a whole backend. Pairwise tests cover 7 of 16 tier
orderings. Several tests wait in real time for what a stepped waker or a lock probe proves.

**Target.** One oracle per property in `internals::oracle`, covering the union of columns. One
fixture per costly resource (a window, a shaper, a GPU frame), built once per test. Table-driven
tests over each contract's whole domain (`PaintTier::ALL²`, `PointerButton::ALL`, the value
widgets). No real sleep where the code under test takes a clock or a waker.

## Part 2: Plan

Batches run most impactful first. Each leaves `cargo drive check` and `cargo drive test` green. A
rendering change also runs `cargo drive run visual`. Whoever addresses an item deletes it.

### Batch 8: Cut the GPU cycles

Files: `src/renderer/{gpu_paint,render_buffer/render_target_draw.rs,frontend/}`,
`src/gpu/{device,resource,surface,backdrop,frame,pipeline,wgpu_backend}/`, `src/ui/mod.rs`,
`src/host/window_driver/`, `AGENTS.md`.

- [ ] `render_target_draw.rs:4,21`, `draw_image_payload.rs:8,41-45`, `frontend/mod.rs:40,83-85`,
  `plane_ctx.rs:428-436`, `session.rs:377`, `gpu_view_targets/mod.rs:80`: the app's
  `GpuPaintRef` travels through the CPU→GPU contract. Target: `RenderTargetDraw` carries ids
  (`TextureId`, owner `WidgetId`, extents, epoch), and `ImageSource::GpuView` carries the
  `TextureId` that `GpuViews::record` resolves. `GpuPaint`, `GpuPaintRef` and `GpuViews` move to
  `gpu`, `Ui` holds `GpuViews`, and the driver passes `&GpuViews` to `paint_gpu_views`.
  `collect_live_targets` moves to the driver. `ImageDraw` and `ViewPaint` fold into
  `DrawImagePayload`. renderer → gpu edges drop to 0. AGENTS.md drops the `gpu ⇄ renderer`
  bullet. Decided: ids only, as Flutter's `Texture` layer holds a `textureId` and the engine's
  `TextureRegistry` holds the producer. Refused: the callback carried as data, as egui's
  `PaintCallback` and iced's `shader::Primitive` do, which keeps the cycle and a borrowing payload.
  Cost: one hash lookup per repainted `GpuView` per frame.
- [ ] `device/backend_resources.rs:3-6`: `device` imports the image store, icons, gradients and
  the shaper. Target: `BackendResources` moves to `gpu::wgpu_backend`, its only consumer with
  `host/core.rs`.
- [ ] `device_requirements.rs:5`: `device` takes `IMMEDIATES_BYTES` from `pipeline`. Target:
  `device_requirements` owns the floor, and `pipeline` asserts at compile time that its push fits.
- [ ] `surface/backbuffer.rs:3,21,79-90`: `Backbuffer` stores the blur's `TargetSource`. Target:
  the source lives on `RenderStream` beside the backbuffer and resets when `ensure_backbuffer`
  remakes it.
- [ ] `frame/pass_recorder/mod.rs:3`, `pipeline/*/mod.rs` imports of `frame`: `PassRecorder`
  lives in `frame` but needs only `viewport_push`. Target: `pass_recorder` and `PassState` move
  into `gpu::pipeline`.
- [ ] `pipeline/stencil_variant.rs:5`, `quad_pipeline/mod.rs:23`: `Stencil::{test,stamp,clear}_state`
  are pipeline state kept in `surface`. Target: move them into `pipeline::stencil_variant`.

### Batch 9: Ui ownership

Files: `src/ui/{mod.rs,frame_cycle.rs,theme_transition.rs,frame_runtime/,bench.rs}`,
`src/window/window_frame_state.rs`, `src/host/harness/mod.rs`, `tests/alloc/fixtures/widgets.rs`.

- [ ] `ui/mod.rs:130-137,268-336,401-426,1637-1641`, `frame_cycle.rs:58,64,79-81`: four theme
  fields with the epoch bump written at four sites. Target: `ui/window_theme.rs::WindowTheme
  { pair, transition, scheme, epoch }` with `shown`, `get`, `set(scheme, theme, Motion)`,
  `set_spec`, `advance(dt, resolved, Motion)`, `edit` and `epoch`. Every change of `shown()`
  bumps the epoch inside. `Ui`'s public methods call it. The transition rules move to plain-input
  tests, and `ui/tests/theme_transition.rs` and `color_scheme.rs` keep only the frame-level
  contract.
- [ ] `theme_transition.rs:20-22,99-120`: a third theme mid-transition clones a shared
  `Rc<Theme>` (about 15 KB) after warmup. Target: keep both buffers as spares, and write into a
  spare when `Rc::get_mut` is `None`. Add a third theme to `tests/alloc/fixtures/widgets.rs:286-323`.
- [ ] `ui/mod.rs:161-164,190,209-210`, `frame_runtime/mod.rs:43-44,247-253`: `Ui::device_epoch`
  copies the recorded baseline's epoch. Target: one field in `FrameRuntime`, seeded at `Ui::new`,
  with `device_restored` computed in `take_frame_plan`.
- [ ] `window_frame_state.rs:11-43`, `ui/mod.rs:778-793`: the four window facts are written twice
  and copied field by field. Target: `WindowFrameState { facts: WindowFacts, screenshot }`, with
  `WindowFacts: Default` not gated on winit.
- [ ] `host/harness/mod.rs:35-36`, `ui/bench.rs:86-87`, `ui/mod.rs:1517`: docs describe the
  removed 1/240 s step, and claim `input_ctx` never runs in release. Target: rule 8 says "clamped
  to 0.1 s", the bench comment gives its real reason, and the `input_ctx` sentence goes.

### Batch 10: Hosts

Files: `src/host/{harness,offscreen,winit/remote,core.rs,host_options.rs,window_driver/tests.rs}`,
`src/gpu/surface/surface_manager/mod.rs`, `src/gpu/wgpu_backend/mod.rs`.

- [ ] `harness/mod.rs:346-356,680-693`, `offscreen/mod.rs:228-238,281-298`: the two headless hosts
  duplicate the frame step and the system setters. Target: `pub(super) SoloWindow { core, driver,
  wake }` with `frame`, `submit` (skipped on a lost device) and the two system setters.
- [ ] `offscreen/mod.rs:179-186`, `harness_builder.rs:60-67`, `winit/config.rs:28-34`,
  `core.rs:261-268`, `host_options.rs:45-51`: five `HostOptions` literals with an inline clipboard
  `cfg`. Target: `const fn HostOptions::new(fonts)`.
- [ ] `surface_manager/mod.rs:36,150-154`, `wgpu_backend/mod.rs:183-187`, `core.rs:69-73`:
  `collect_gpu_stats: bool` and `FrameTiming` hold one choice. Target: `HostGpuConfig::timing:
  FrameTiming`, with `FrameTiming::requested_features()`.
- [ ] `harness/mod.rs:849-1088`: a `#[cfg(test)] mod unit` sits between `internals` and `tests`.
  Target: the reach-ins go into `internals` under `#[cfg(test)]`, and `Passes<R>`, `frame_passes`
  and `step_passes` go to `host/harness/tests/passes.rs`.
- [ ] `winit/remote/tests.rs:268-287,412-427`, `remote/mod.rs:38`: about 270 ms of fixed sleeps.
  Target: `Remote::new` takes its tick, and tests use about 5 ms. The worker waits on a channel
  the test signals. The `Wait` step uses 10 ms.
- [ ] `harness/tests/play.rs:332-336,364-386`: `a_settle_waits_for_a_busy_apps_wake` sleeps about
  300 ms on a thread. Target: an app whose `record` wakes its own `Waker` for N frames drives the
  `is_pending` branch (`harness/play.rs:181-183`). Keep one short real wait.
- [ ] `window_driver/tests.rs:1019-1051`: `a_region_rounds_out_to_whole_physical_pixels` builds 5
  GPU backends where 3 serve. Target: build the two format scenes once, before the region loop.

### Batch 11: Renderer and damage data

Files: `src/renderer/{pane_captures,render_buffer,frontend/composer}/`, `src/damage/`,
`src/cascade/`, `src/layout/mod.rs`, `src/internals/oracle.rs`.

- [ ] `pane_captures/mod.rs:72-82,129,148-150`, `backdrop_row.rs:11-28`, `session.rs:558-571`,
  `frontend/mod.rs:86-89`: one pane is copied into four rows. Target: `PaneCapture { key,
  footprint, plan, region }` in `render_buffer::backdrop_row`. `PaneTicket` and `BackdropRow`
  embed it. `settle` reads keys from the tickets, not a `Vec<PaneRow>` copy, and `live_panes` comes
  from the ledger. Update `pane_captures/tests.rs:90-99`.
- [ ] `higher_kind_rects.rs:21-60`, `rect_grid/mod.rs:19-55`: past 32 rects, each rect and the
  union are stored twice. Target: `TierRects` holds a `TileIndex` directly, as `OcclusionPruner`
  does. `RectGrid` stays for the text grids.
- [ ] `occlusion_pruner.rs:38,66-125`: decide and compact in two passes through `drop_idxs`.
  Target: one read/write loop and `truncate`. Fold `tests/pruning.rs`'s 11 single `prune_*` tests
  and `brushes.rs`'s `windowed_rect_is_not_an_opaque_cover` into one `(label, under, over,
  pruned)` table. Delete the duplicate `no_stroke_full_cover` case and the "Phase 3" comments. Add
  a turned full-viewport row to `clear_fold_absorbs_covers_and_rejects_non_qualifying`.
- [ ] `curve_caps.rs:19`, `paint_order.rs:222`: behavior-selecting `bool`s. Target: a `CapEnds`
  flag set, and `TextClip::{Whole, Cut}`, which `OpenBatch` also holds.
- [ ] `row_matcher.rs:206,226,240-251`, `pane_walk.rs:92-95`: `gone_place` scans every row per
  moved or removed row (n² per node), even when no pane exists. Target: skip it when the pane walk
  is off. When it is on, build a prefix-max lookup once per node into a retained buffer.
- [ ] `cascade/engine.rs:63-64,157-159,436-445` vs `damage/plane_walk.rs:28-29,137-138`: the
  cascade says a clean subtree may be skipped only while every rect held, and the damage walk
  skips it by induction. Target: state the invariant once in `layout`'s module doc, and prove it
  with an oracle case that diffs a cascade with `skips_clean_subtrees` forced on. Then drop
  `rects_kept` or guard `SubtreeUnchanged` the same way.
- [ ] `cascade/tests/incremental.rs:534-609`, `cascade/mod.rs:385-440`, `internals/oracle.rs:133-158`:
  three comparators, each blind to a different set of columns. Target: one
  `Cascade::assert_same_as` over the union of columns, `panes` and `shape_scales` included.

### Batch 12: Test consolidation

Files: tests under `src/layout/cache/`, `src/input_state/`, `src/renderer/`, `src/primitives/`,
`src/gpu/`, `src/widgets/`.

- [ ] `layout/cache/tests/frames.rs:32-111`, `internals/oracle.rs:102-130`: three warm-versus-cold
  layout oracles. Only one checks `scroll_content`. Target: one oracle in `internals::oracle` over
  every column, with an optional "the warm frame hit" guard.
- [ ] `layout/cache/tests/reuse.rs:419-476`: a subset of `workloads.rs:54-69`. Target: move its two
  extra assertions into the `workloads` test and delete it.
- [ ] `input_state/tests/drag.rs:65-526`: four contracts tested once per button or fixture. Target:
  one table over `PointerButton::ALL` for latch, delta, the `started` edge, the bystander and the
  sub-threshold click.
- [ ] `input_state/tests/click.rs:361,442,482`, `capture.rs:14-115`: Harness tests repeat rows of
  the unit table. Target: give the unit table a `window` column. Keep one table-driven Harness test
  for `count` reaching the response.
- [ ] `composer/tests/{batching,curves,brushes,scaling}.rs`: tier ordering is tested pairwise, 7 of
  16 pairs, no Icon pair. Target: one table over `PaintTier::ALL²`, overlapping and disjoint.
- [ ] `composer/tests/batching.rs`: six quad/text grouping tests each assert a subset of one
  layout. Target: one table that asserts the whole layout per case.
- [ ] `composer/tests/curves.rs`, `batching.rs`: about 300 lines of inline payloads. Target:
  `CurveBuilder`, `QuadBuilder::turned` and `kind`, `TestPolyline::bounds`. Move
  `compose_spins_polyline_about_bbox_center` to `curves.rs`.
- [ ] `gradient_atlas/bake/tests.rs`, `gradient_atlas/tests/bake.rs`: `bake::row` tested in two
  places. Target: one file. `lut_row_layout`'s size asserts become `const` asserts in
  `bake/mod.rs`.
- [ ] `geometry/corners/tests.rs`, `geometry/spacing/tests.rs`: six tests each run the shared lane
  codec. Target: codec tests on `TestLanes`, the precision contract once on `F16x4`, and one table
  per type for its own `LaneCodec` choices.
- [ ] `gpu/backdrop/tests/{mod,clear,dispersion,neighbors}.rs`: four copies of one helper, and a
  backend per reference frame (13 tests, about 1.2 s serial). Target: one fixture that keeps a
  reference window per test and repaints it whole through `Damage::Full`.
- [ ] `single_quad_buffer/tests.rs:15-19`, `gpu_view_targets/tests.rs:100-106`,
  `gpu_gradient_atlas.rs:157-167`, `backdrops/tests.rs:55`, `backdrop_blur/tests.rs:205,428`,
  `text_rig.rs:83`: belt, encoder and `GpuCtx` set up by hand seven times. Target:
  `HeadlessTestGpuLease::frame(|ctx| …)` in `test_gpu.rs`, which finishes, submits and waits.
- [ ] `gpu/device/gpu.rs:293-323`: `pipeline_builds_on_one_device_take_turns` proves the turn with a
  50 ms sleep. Target: while the turn is held, `clone.pipeline_turn.try_lock()` is `WouldBlock`.
- [ ] `gpu/device/gpu.rs:337-395`: `wait_until_waits_for_a_map_another_thread_runs` runs 50 rounds
  (0.52 s) where 12 give odds of 1.9e-6. Target: `ROUNDS = 12` with the odds in the doc, and
  `yield_now` in the pollers.
- [ ] `widgets/tests/theme_file_values.rs:132-286`: 12 shards rebuild the font database per
  document, about 1.75 s serial. Target: one `TextShaper` per shard through
  `HarnessBuilder::shaper`. Measure again, reduce the shard count, and fix the stale "0.3 s"
  comment.
- [ ] `widgets/tests/style_override.rs:178-193`: a `TextStyleOverrides` test in the widgets suite.
  Target: move it beside the theme's override tests.

### Batch 13: Widgets, no API change

Files: `src/widgets/{combo_box,drag_value,color_picker,color_button,value_area.rs,slider,color_field,color_strip}/`,
`src/widget_core/value_response.rs`, `src/primitives/math/domain/f64.rs`.

- [ ] `combo_box/mod.rs:107-142`: closed, Up and Down step the pick, and the doc credits that to
  the WAI-ARIA select-only combobox, which opens the list on those keys. Target: the APG keys.
  Closed, Down, Up, Home and End open the list as a keyboard open does. Down lands on the current
  pick, Up and Home on the first row, End on the last. No key changes the pick while the list is
  closed. Update the combo tests. Decided: the WAI-ARIA APG select-only combobox
  (w3.org/WAI/ARIA/apg/patterns/combobox/examples/combobox-select-only), as macOS's
  `NSPopUpButton` opens on the arrows too. Refused: the Win32 drop-down list's closed stepping,
  which changes a value the user cannot see listed. Landing on the current pick needs
  `MenuItem::selected` from `api-pass.md`. Until then, Down lands on the first row.
- [ ] `drag_value/mod.rs:118-132`: `range` hand-rolls its check and names no domain kind. Target:
  `domain::f64::bounds` (ordered, infinity allowed, no NaN), named in the doc.
- [ ] `widget_core/value_response.rs:27-46`, `color_picker/mod.rs:232-237`,
  `color_button/mod.rs:86-90`: the (changed, committed) pair is modeled four ways. Target:
  `ValueEdges` and `edges()` become `pub(crate)` production code. Delete `EditEdges` and the test
  tuples.
- [ ] `slider/tests.rs:27-80`, `drag_value/tests/{support,scrub}.rs`,
  `color_field/tests.rs:88-111,350-369`, `color_strip/tests.rs:103-122,193-208`: one deferred-commit
  contract tested per widget, and `value_area` has no tests. Target: one table over Slider,
  DragValue, ColorField and ColorStrip, and a `value_area` test for both `AreaValue`s. The
  DragValue helper's two `bool`s go.
- [ ] `button/mod.rs:54,60`, `radio/mod.rs:41,47`, `color_strip/mod.rs:89`, `switch/mod.rs:46`,
  `color_field/mod.rs:85`, `menu_separator.rs:28`: `style` docs differ, and `MenuSeparator` says
  "unstyled". Target: one sentence form that names and links the slot.

### Batch 14: Build, tooling and small items

Files: `Cargo.toml`, `clippy.toml`, `src/internals/`, `examples/showcase/shot/mod.rs`,
`src/primitives/paint/image/mod.rs`, `src/gpu/raster/text_backend/encode/cache/mod.rs`,
`src/gpu/frame/gpu_timings/mod.rs`, grid and cascade names.

- [ ] `Cargo.toml:113,148`: `strum` is a regular dependency used only by `composer/bench.rs`.
  Target: optional, enabled by `internals`.
- [ ] `Cargo.toml:133-134`: the `image` dev-dependency repeats the optional one `internals`
  enables. Target: delete it, and confirm with `cargo drive check`.
- [ ] `clippy.toml:6-118`: ten wgpu types used in `gpu` are unguarded, `Backends` is listed but
  unused, and the regeneration grep misses grouped imports. Target: add the types, and add a
  `cargo drive` command that collects every wgpu type under `src/` and fails on one not listed.
- [ ] `internals/mod.rs:11`, `demo_swatches.rs:1-5`: `pub` with no outside caller, and its doc
  names a share that is gone. Target: a private submodule of `frame_fixture`, and
  `frame_fixture/chrome.rs:52-54` names its constants.
- [ ] `examples/showcase/shot/mod.rs:368-386`: `write_png` copies each shot texel by texel (14 MiB
  at 2,576 px). Target: `image::save_buffer` over `Image::as_srgba8`.
- [ ] `image/mod.rs:207-247`: `fitted` holds about 9× the source and reads column-wise (184 MiB for
  a 3200×2000 shot). Target: decode on the fly, precompute the x taps, and run rows outer with a
  ring of the rows the vertical tent reads.
- [ ] `encode/cache/mod.rs:343-354`: a comment contradicts its gate, and a `#[cfg(test)]` repeats.
  Target: fix the comment and drop the repeat.
- [ ] `gpu_timings/mod.rs:69-168`: `GpuTimings` wraps `Tracer` and forwards eight methods. Target:
  `GpuTimings` is the enum.
- [ ] `layout/drivers/grid/measure.rs:22-23`, `grid/mod.rs:107`, `cascade/paint.rs:65`,
  `plane_cascade.rs:111`: `n_*` counts, which AGENTS.md refuses. Target: `row_count`,
  `col_count`, `track_count`, `node_count`.
