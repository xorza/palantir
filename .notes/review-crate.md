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

## Host and window

- **The swapchain owns its configuration.** `WindowSurface` compares the
  desired and the applied `SurfaceConfiguration` and reconfigures in
  `prepare`. On `Reconfigured`, the winit `Window` invalidates the driver's
  target contents. `TargetKey` is `(physical, format)` and loses `vsync`.
- **One host-options vocabulary.** A crate-private `HostOptions` holds the
  settings that both builders share (fonts, shaper, clipboard, GPU stats,
  pixel snap) and builds `HostCoreConfig` in one place.
- **Window levels leave through `Ui`.** Cursor, vsync and IME area are read
  from `Ui`. `FrameReport` reports only what the frame did and what it owes.
- **Errors by layer.** A device failure is `WinitHostError::Gpu`, not a
  window-surface error.

## Widgets

- **Keyboard interaction is a `widget_core` primitive.** An activation
  helper on `Widget` declares the scope, reads the chords through
  `Widget::key_pressed` and folds a keyboard activation into
  `ResponseState`, so `clicked()` means the same on every widget. One travel
  type takes a unit and the grain of the bound number type. Slider,
  ColorField, ColorStrip, DragValue and Splitter use it.
- **`Widget::response` returns `ResponseSnapshot`.** The id and the state
  travel together into `Response::new`.
- **A widget-supplied node value goes through `default_*`.** A caller's
  setter always wins over the theme.
- **The tab strip owns its geometry.** A crate-private strip scan (chip hits
  plus a drop slot from unclipped extents) and one index module
  (`moved_idx`, `removed_active`) serve `TabStrip`, `TabbedView`, `DockView`
  and `DockState`.
- **One split clamp.** `Splitter` takes a ratio floor beside `min_pane`, and
  `DockView` passes `RATIO_MIN`.
- **Retained widget state exposes operations, not fields.** `ScrollState`
  owns every offset and zoom write (`reveal`, `coerce_zoom`). TextEdit gets a
  `Composition` satellite for IME and a `History` type for undo.

## Paint and geometry

- **A geometry emits canonical shader parameters.** Linear `(t0, t1)` is the
  projection of the unit square on the direction. A conic angle is wrapped to
  `0..TAU` before the f16 pack.
- **The color type owns its interpolation.** `RgbaF32`'s `Animatable` works
  on premultiplied values (CSS Color 4 §12.3, which the brush module already
  states). Alpha and channels are clamped once, where a color is lowered.
- **One function per geometric fact, on the primitive it describes.**
  `Corners::inner(size, padding)` fits first, then reduces each corner by the
  larger adjacent inset. One text snap-slack function over the local inked
  size. One CPU corner-fit function. The cascade, the encoder, the composer
  and the cutout plan call them.
- **Validation happens at the setter.** Every shape setter checks its input
  with `#[track_caller]`. The release-silent `LowerShape::has_nan` gate goes.
  `#[track_caller]` and `# Panics` follow each call chain down to its assert.

## Layout

- **The exact offer is the cache identity.** The snapshot keeps the raw
  offer. A hit holds on bit-equality, or when the offer is finite and at least
  `stable_from`. The 1 px grid stays only as a policy of the drivers that make
  discrete decisions (text wrap, line packing).
- **Every measured grid cell contributes to its tracks** (CSS Grid §12.5,
  §12.7.1; WPF `MeasureCellsGroup`). Spanning cells distribute their excess
  over the content-sized tracks they cross. Fill tracks of an indefinite or
  hugging grid take their max-content contribution, as Stack's `fill_main`
  does.
- **Per-node layout state has one copy per frame.** Forest-wide columns that
  measure and arrange write directly, indexed by a layer base. A hit copies
  prev → cur once, and `end_frame` swaps. (Structural: after the bugs.)

## Frame cycle

- **The frame plan checks the record baseline.** Frame entry compares theme
  identity and the font epoch with the last record. A move forces
  `FullRecord`. The damage PaintOnly arm then only asserts.
- **`FrameRuntime` owns the frame stamp.** The display and the time of the
  current frame live together, with `begin_frame` and `end_frame`. The
  screenshot request moves to `WindowRequests`, and the landed image to
  `WindowFrameState`.

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

## Theme

- **A theme value is stored in the type its sink uses.** The spinner stores
  a period as `Duration`. The duration codec round-trips through `f64`, which
  is exact to the nanosecond.
- **Theme bundles are comparable values** (`PartialEq`), so round-trip tests
  compare values, not RON text.

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

## Batch 5: Host frame step and time

Files: `src/host/{core.rs, clock.rs, window_driver/,
offscreen.rs, winit/window/, winit/runtime.rs}`, `src/internals/ui_harness/`,
`src/ui/mod.rs` (one doc).

- [ ] **`UiHarness` repeats the display mint.** `ui_harness/mod.rs:639-651`.
  Target: one display mint on `FrameSpec` that both call.

## Batch 6: Swapchain configuration and the winit window

Files: `src/gpu/surface/{window_surface.rs, surface_manager/}`,
`src/host/window_driver/`, `src/host/winit/window/`.

- [ ] **`TargetKey` mixes target identity and swapchain configuration.**
  `window_driver/mod.rs:111-136, 326-336, 494-504`,
  `winit/window/mod.rs:305-314, 327-332, 435-443`. Target: `WindowSurface`
  owns desired vs applied configuration, `prepare(gpu) -> Reconfigured`;
  `TargetKey` and `FrameSpec` drop `vsync`.
- [ ] **The winit per-window policy is untested.**
  `winit/window/mod.rs:78-116, 240-443`. Target: satellites `Visibility`,
  `AppliedLevels`, `PresentSchedule`; a pure `present_policy` over an
  `AcquireOutcome`, tested table-driven with exact back-off values.
- [ ] **`SurfaceManager` keeps an unused `Gpu`, and the texture cap is read
  twice.** `surface_manager/mod.rs:41-44, 88`, `core.rs:43-46`. Target:
  startup moves the `Gpu` out; the resize clamp reads `HostCore`'s cap.
- [ ] **Host tests.** Move `app_lifecycle_follows_frame_plan_and_record_replays`
  (`winit/tests.rs:47-79`) to `src/ui/tests/frames/`, so the no-winit lane
  runs it. Reduce `builder_retains_defaults_and_overrides` to the overrides.

## Batch 7: Public host surface

All **[public]**. Files: `src/host/`, `src/window/`,
`src/ui/frame_report.rs`, `src/display/mod.rs`, `src/gpu/device/gpu.rs`,
`src/gpu/error.rs`.

- [ ] **Builders spell shared settings differently.** `offscreen.rs:34-117`,
  `winit/mod.rs:143-193`, `winit/config.rs:407-428`, `runtime.rs:61-69`.
  Target: `HostOptions`; both builders get `fonts` (`const`), `shaper`,
  `pixel_snap`, `collect_gpu_stats`, `system_clipboard`.
- [ ] **Window levels leave through three doors.** `frame_report.rs:43-44`.
  Target: `Ui::ime_area()` beside `cursor` and `vsync`;
  `FrameReport::ime_area` goes; `WindowOutput` field renamed `ime_area`.
- [ ] **A device failure reads as a surface failure.**
  `winit/error/mod.rs:256-281`, `gpu/error.rs:130-157`. Target:
  `WinitHostError::Gpu { source }`; `SurfaceError::Device` goes.
- [ ] **`RequestedGpu::headless` gives a device nobody outside the crate can
  render with.** `gpu/device/gpu.rs:15-25`. Target: `Gpu::device()` and
  `Gpu::queue()`, or a target factory on the host.
- [ ] **`WindowPlacement::pos` is physical.** `window_placement.rs:10-11`,
  `window_config.rs:53-57`. Target: `physical_pos`, `with_physical_pos`.
- [ ] **`Display::from_physical` has no public consumer.**
  `display/mod.rs:101-122`. Target: `pub(crate)`.

## Batch 8: Theme values and the spinner

Files: `src/theme/`, `src/widgets/spinner/`, `src/theme/tests/`.

- [ ] **A valid theme `spinner.speed` panics.** `theme/spinner.rs:18-20`,
  `widgets/spinner/mod.rs:119`. `1e-20` passes `positive`, then
  `Duration::from_secs_f32(TAU / speed)` panics each frame; a code-built `0.0`
  panics too. Target: `SpinnerTheme::period: Duration`, checked non-zero on
  load. **[public]** and a theme-file format change.
- [ ] **[public] The spinner's stroke is named `thickness`.**
  `widgets/spinner/mod.rs:81-114`, `theme/spinner.rs:21-26`. Target:
  `Spinner::width`, `SpinnerTheme::{width_ratio, min_width}`; AGENTS.md
  cites another `*_ratio` example in place of `thickness_ratio`.
- [ ] **The duration codec loses precision.** `theme/serde.rs:14, 22`. 300 ms
  reads back as 300 000 012 ns. Target: `f64`; the error text names a
  duration.
- [ ] **[public] Theme bundles have no `PartialEq`.** Target: derive it on
  every bundle, `SlotDefaults` and `Theme`; the round-trip test compares
  values.
- [ ] **The theme-file fuzz draws a subset.** `theme/tests/file_values.rs:20,
  64-75`. Target: draw the dock and tab strips, an open context menu and a
  shown tooltip; add `1e-30` and `1e30` to `SUSPECTS`.
- [ ] **[public] `TabsTheme` names.** `theme/tabs.rs:26-35, 114`. Target:
  `selected`/`unselected`, `cap_focused`/`cap_unfocused`, `cap_thickness`.
- [ ] **`ExpanderTheme.looks` is not flattened.** `theme/expander.rs:20-21`.
  Theme-file format change.
- [ ] **`WidgetLook::SLOT_LOOK` belongs to wear.** `theme/look/mod.rs:42`.
  Target: a constant beside `Widget::wear`.
- [ ] **Names.** `LINE_HEIGHT_MULT` → `LINE_HEIGHT_FACTOR`
  (`theme/text_style.rs:14`), `chevron_pts` → `chevron_points`
  (`theme/combo_box.rs:32`).
- [ ] **File names.** `theme/serde.rs` shadows the crate → `duration_codec.rs`;
  `SlotDefaults` → `theme/look/slot_defaults.rs`.
- [ ] **Docs.** `color_picker.rs:148`, `tooltip.rs:29, 32`; doc comments
  above their `#[serde]` attributes (`toggle.rs:29-41`, `expander.rs:34-40`,
  `menu_item.rs:20`, `theme_slot.rs:58`).
- [ ] **Tests.** One `response(..)` helper in `state_looks.rs` (copies at
  :28, :105, :127, :204). Move `style_override.rs` to `src/widgets/tests/`.

## Batch 9: Paint parameters and color animation

Rendering changes: run the golden suite first, then after. Files:
`src/primitives/paint/{brush/gradient/, color/}`, `src/primitives/packed/`,
`src/animation/`.

- [ ] **Linear gradients are wrong outside `[0, π/2]`.**
  `linear_geometry.rs:28-31`; shader `quad_pipeline/shader.wgsl:516-525`. At
  angle π the whole box paints stop 0; at π/4 the last stop is at 71 %.
  Target: `t0 = min(cos,0) + min(sin,0)`, `t1 = max(cos,0) + max(sin,0)`;
  correct the doc; a table test over the four quadrants.
- [ ] **Color tweens interpolate straight alpha.** `color/mod.rs:31-62`,
  `brush/mod.rs:175-183`. A fade to `TRANSPARENT` paints c/4 at the midpoint.
  Target: premultiplied `Animatable`; change `color/tests.rs:128-146`.
- [ ] **Overshoot paints alpha above 1.** `easing.rs:45-50`,
  `spring.rs:91-101`. Target: clamp alpha to `0..=1` and channels to `≥ 0` in
  `RgbaF16::from`. `RgbaF16::is_noop` rejects negative alpha as
  `RgbaF32::is_noop` does (`rgba_f16.rs:31-35`, `half_simd/mod.rs:121-123`).
  Test with `OutBack` and a bouncy spring.
- [ ] **The conic angle is packed raw into f16.** `conic_geometry.rs:35-38`.
  Target: `rem_euclid(TAU)`; test a large angle.
- [ ] **`CircularArc::bbox` overflows on a huge angle.**
  `circular_arc/mod.rs:70-73`. Target: reduce the start angle mod TAU first.
- [ ] **`hex(u32)` drops the top byte.** `srgba_u8.rs:36-39`,
  `color/mod.rs:198-201`. Target: `#[track_caller]` assert `rgb <= 0xFFFFFF`
  under `# Panics`.
- [ ] **`Span: From<Range<usize>>` truncates; `From<Range<u32>>` underflows.**
  `common/span.rs:37-53`. Target: assert the fit and `start <= end`
  (**[public]** if it becomes `TryFrom`).
- [ ] **Dead NaN handling in `Mesh`.** `mesh/mod.rs:108-111, 158-167,
  239-242`.
- [ ] **A visual tolerance is used as a time floor.**
  `animation_spec/mod.rs:5, 98-104`. Target: a named `MIN_DURATION` with its
  reason.
- [ ] **Clipboard backend is `Box<dyn>` over a closed set.**
  `common/clipboard/mod.rs:20-23, 90-95`. Target: an enum.
- [ ] **Docs.** `rect/mod.rs:17-18` (hash is exact), `urect/mod.rs:155-158`
  (u32 → f32 is not exact above 2^24), `chrome.rs:18` (`corners`, not
  `radius`), `paint/mod.rs:1-2`.
- [ ] **Interruption breaks motion.** `animation_row.rs:81-104`,
  `animation_map_typed.rs:144-176`. Target: a spring retarget keeps its
  velocity; a duration retarget whose new target is the segment's start
  value runs from the reversing-adjusted start over the shortened duration
  (CSS Transitions 2). Tests in `animation/tests/retarget.rs`.

## Batch 10: Geometric facts computed once

Rendering changes: run the golden suite. Files:
`src/primitives/geometry/corners/`, `src/renderer/frontend/{encoder,
composer}/`, `src/cascade/paint_rect.rs`,
`src/gpu/pipeline/quad_pipeline/cutout_plan/`.

- [ ] **The rounded-clip mask uses unfitted corners.**
  `encoder/layer_ctx.rs:421-436` vs `:384`. Chrome 100×30, radius 20,
  padding 10/2: mask 10, correct 5. Target: `Corners::inner(size, padding)`
  for the mask and the inset shadow. Extend `encoder/tests/clipping.rs:52-80`
  with overflowing radii and uneven padding.
- [ ] **The text bounds pad shrinks with zoom.** `composer/session.rs:666-680`
  vs `cascade/paint_rect.rs:50-64`. Target: one slack function over the local
  inked size. Extend `composer/tests/batching.rs:768-805` with a transform
  scale below 1.
- [ ] **Two CPU copies of the corner fit.** `cutout_plan/mod.rs:530-546`,
  `corners/mod.rs:94-112`. Target: one `pub(crate)` function.
- [ ] **Placement math is rebuilt at each draw.**
  `composer/session.rs:152-160, 192-198`, and about 8 `physical_scale` sites.
  Target: the transform stack stores the composed physical map;
  `push_clip` calls `scaled_rect`. Keep `(T·x)·dpr` order; check
  `composer/tests/scaling.rs`.
- [ ] **Local-rect resolution has two spellings.** `encoder/geometry.rs:1-15`,
  `cascade/paint_rect.rs:292, 302`. Target: one method on the rect; delete
  the module.
- [ ] **Text batch hits flush the group for nothing.**
  `composer/session.rs:956-981`. Target: skip the flush when the hit batch's
  `last_group` is below the in-flight group.
- [ ] **Docs.** `render_buffer/mod.rs:52`, `session.rs:378-379`,
  `render_target_draw.rs:52-58`, `renderer/mod.rs:3`.

## Batch 11: Layout correctness

Files: `src/layout/{cache/, drivers/grid/, axis_slot.rs, engine.rs, pass.rs}`,
`src/widget_core/configure.rs`, `src/scene/tree/mod.rs`, AGENTS.md.

- [ ] **A cache hit at a same-bucket offer serves another offer.**
  `cache/mod.rs:160-183`, `engine.rs:94`, `wrapstack/mod.rs:57-68`. At DPR
  1.5, logical 1000.667 and 1001.333 share key 1001; text keeps a wrap width
  of 500 where cold uses 501. Target: store the raw offer; hit on
  bit-equality or `offer.is_finite() && offer >= stable_from`;
  `matches_forest` and `keeps_last_run` share the test.
- [ ] **Fill-column cells overflow a hugging grid.**
  `grid/measure.rs:88-90, 161-166`. Target: the grid size includes each Fill
  track's measured contribution. Add a Hug-grid Fill-column wrapping-text case
  to `hug_grid.rs`.
- [ ] **Spanning cells do not size their tracks.** `grid/measure.rs:52-56,
  115-131`, `grid/mod.rs:113-117`. Target: distribute the excess over the Hug
  tracks crossed, in `measure::run` and `Grid::intrinsic`; fix the false
  comment. Test in `spans.rs`.
- [ ] **A grid cell past the tracks panics in release.**
  `configure.rs:176-189`, `scene/tree/mod.rs:503-523`,
  `grid/measure.rs:121-122`. Target: coerce row, column and span at layout;
  document it on `grid_cell`.
- [ ] **One cold-oracle helper.** `cache/tests/frames.rs:583, 811, 907, 980`
  vs `:27`. Target: a helper over a surface sequence that includes fractional
  sizes at scale 1.5.
- [ ] **Cache bench fixtures import `ui` and `widgets`.**
  `cache/mod.rs:384-497`. Target: move to `src/internals/`.
- [ ] **The docs name the wrong sizing contract.** AGENTS.md says
  "flex-shrink sizing". Target: AGENTS.md and the layout design name the
  table-layout shrink (CSS 2.1 §17.5.2.2), WPF star grow and leading-edge
  overflow alignment, with sources.

## Batch 12: Frame plan and `Ui` state

Files: `src/ui/{frame_runtime/, frame_cycle.rs, mod.rs, frame_report.rs,
screenshot.rs}`, `src/damage/`, `src/window/window_requests/`, `src/lib.rs`.

- [ ] **A PaintOnly frame paints a stale tree.**
  `frame_runtime/mod.rs:153-199`, `damage/engine.rs:307-318`. After
  `set_theme` through `OffscreenHost::ui()`, or a font load in another window,
  a caret blink repaints old widget colors or old text layout until input
  arrives. Target: the classify input carries the record baseline (font epoch,
  theme identity) and forces `FullRecord` on a move; the damage arm asserts.
  Tests for both cases.
- [ ] **Frame state is loose on `Ui`.** `ui/mod.rs:133-159, 368-397, 820-832`,
  `frame_cycle.rs:112-125, 205-209, 257-258, 287-288, 309-316`. Target:
  `FrameRuntime` owns `stamp` with `begin_frame`/`end_frame`; screenshot
  fields move to `WindowRequests`/`WindowFrameState`; `SeenIds` owns the
  cascade pairing; owners' fields are written through methods.
- [ ] **`FrameReport::repaint_requested` documents one of five writers.**
  `frame_report.rs:296`. Target: "another frame is owed after this one".
- [ ] **Re-export docs attach to the wrong items.** `lib.rs:139-141, 274-276,
  283-285`. Target: no doc comments on `pub use` lines.
- [ ] **Fields widened for tests.** `damage/engine.rs:165-180`,
  `damage/node_snapshot.rs:313`, `frame_runtime/mod.rs:73, 81`. Target:
  private, with accessors in each file's `internals`; `winit/tests.rs:137`
  goes through `schedule_wake`.
- [ ] **[public] `Screenshot` duplicates `Image`.** `ui/screenshot.rs:85-128`.
  Target: `Screenshot { image: Image, scale, region }`.

## Batch 13: Widget keyboard and response primitives

Files: `src/widget_core/{widget/, response.rs, configure.rs, configurator.rs}`,
`src/widgets/{slider, color_field, color_strip, axis_keys.rs, toggle_row,
checkbox, radio, switch, button, color_button, color_swatch, expander,
gpu_view, drag_value, drag_num, splitter}/`,
`src/primitives/math/domain/f64.rs`, `src/primitives/layout/sizing.rs`.

- [ ] **Slider, ColorField and ColorStrip have no input scope.**
  `slider/mod.rs:48-50, 206-240`, `color_field/mod.rs:67-69, 127`,
  `color_strip/mod.rs:80-82, 131`, `axis_keys.rs:35-63`. Arrow groups take
  their arrows, and a nested scope drops them. Target: declare
  `CARET | PAGE`, read through `Widget::key_pressed`. Extend
  `widgets/tests.rs:521-606` with the three and an `arrow_focus` parent.
- [ ] **One travel type.** `slider/mod.rs:206-240`, `axis_keys.rs:35-63`,
  `drag_value/mod.rs:220-240`, `splitter/mod.rs:174-196`. Four copies with
  different steps and Home/End precedence. Target: one type with step pairs,
  a unit and the number grain.
- [ ] **Integer Slider arrows do nothing when the span is under 50.**
  `slider/mod.rs:208`. Target: step `max(span/100, value.unit(decimals))`.
  Add i64 and `decimals(0)` rows to `slider/tests.rs:315-380`.
- [ ] **Large `i64` steps lose precision.** `drag_value/mod.rs:244-247`,
  `slider/mod.rs:112-113`. Above 2^53, Up can store a lower value. Target:
  step through `commit_drag`, which adds whole steps with `saturating_add`.
- [ ] **Activation written four times.** `button/mod.rs:83-92`,
  `color_button/mod.rs:74-82`, `toggle_row/mod.rs:51-57`,
  `expander/mod.rs:151, 276-284`. Space on a Button is a click; on a Checkbox
  it is not. Target: one activation helper on `Widget`. ColorSwatch becomes
  focusable when it senses a click.
- [ ] **Toggles override the caller's `gap` and `child_align`.**
  `toggle_row/mod.rs:92-95`, `slider/mod.rs:125-128`. Target: `default_gap`
  and a new `ThemeDefaults::default_child_align` **[public]**. Make
  `toggle_row/tests.rs:17-98` table-driven with `gap`.
- [ ] **Expander and GpuView set size in the constructor.**
  `expander/mod.rs:65`, `gpu_view/mod.rs:71`. Target: `default_size` in
  `show`.
- [ ] **The toggle family repeats its `show`.** `checkbox/mod.rs:57-97`,
  `radio/mod.rs:59-109`, `switch/mod.rs:56-108`. Target: `ToggleRow` owns
  `widget`, `label`, `style` and one `show`; the local is `row`, not `chrome`.
- [ ] **ColorField and ColorStrip repeat one pipeline.**
  `color_field/mod.rs:92-162`, `color_strip/mod.rs:96-167`. Target: a
  crate-private `ValueArea` with `committed` computed once.
- [ ] **`Configure::size` panics without `# Panics`.** `configure.rs:82-90`,
  `configurator.rs:58-69`, `sizing.rs:146-150, 266-284`. Target:
  `#[track_caller]` on the `From` impls; convert before `get_or_insert`.
- [ ] **NaN binding in an unbounded DragValue becomes −∞.**
  `drag_num/mod.rs:78-81, 168-173`, `limits/mod.rs:237-239`. Target: a
  non-finite anchor takes the neutral, once in `DragNum::read`; document it.
- [ ] **`decimals` above 15 prints float noise.** `drag_value/mod.rs:123-127`.
  Target: coerce to `MAX_DECIMALS` in the setter; document the kind.
- [ ] **`Limits<T>` and `domain::f64::range`.** `limits/mod.rs:7-56`,
  `domain/f64.rs:36-43`, `slider/mod.rs:46`. The generic has one production
  type; the published range kind reorders, which Slider cannot use. Target: a
  plain `f64` pair folded into `drag_num`; `domain::f64::range` validates and
  keeps the order; Slider and `DragValue::range` call it.
- [ ] **[public] `Widget::response` returns `ResponseSnapshot`.**
  `widget/mod.rs:188-193`, `response.rs:51`; seven callers re-resolve the id.
- [ ] **[public] Part ids.** `Expander::header_id`; document the part ids of
  toggles, Slider, ProgressBar and ColorButton on their owners.
- [ ] **[public] Small gaps.** `Widget::wrap_stack`, `Panel::wrap_stack`;
  `const` `authored_gap`/`authored_line_gap`; the `RadioButton`
  `T: PartialEq` bound moves to the impls.
- [ ] **Tests.** Add `ColorStrip::texel_size` to the setter sweep
  (`widgets/tests.rs:338-345`).

## Batch 14: Dock, tabs and splitter

Needs batch 2. Files: `src/widgets/{dock, tabs, splitter}/`.

- [ ] **A divider over-drag snaps back on release.**
  `dock_view.rs:354-374`, `splitter/mod.rs:158-168, 211, 349-350`,
  `dock_state.rs:381`. Target: a ratio floor on `Splitter` in
  `Seam::ratio_at`; `DockView` passes `RATIO_MIN`. Test at the view level.
- [ ] **Drop slots use clipped chip rects.** `dock_view.rs:280-283`,
  `pane_geometry.rs:49-54, 104-107`, `tabbed_view.rs:284-287`. Target: a
  `TabStrip` drop-slot query over unclipped chip extents.
- [ ] **Chip hits, drop slots and index arithmetic computed two or three
  times.** `tab_strip.rs:435-446` vs `dock_view.rs:207-227`;
  `tabbed_view.rs:275-307` vs `dock_state.rs:339-345`, `tab_group.rs:415-421`.
  Target: `TabStrip::scan` and one index module. `TabbedView` gets the dock's
  caret and Escape cancel; a cancelled drag no longer reorders.
- [ ] **`DockView::style` is ignored for drop classification.**
  `dock_view.rs:262-267`. Target: `edge_fraction` and `caret_width` move to
  the drop policy beside `AllowedSplits`. **[public]**
- [ ] **[public] `TabbedView` does not follow the page on `Closed`.**
  `tabbed_view.rs:29-34, 218-241`. Target: apply the removal arithmetic.
- [ ] **[public] `DockState` mutators outside `apply`.**
  `dock_state.rs:284-291, 388-396`. `find_or_insert` panics on a stale group.
  Target: an `OpenTab` operation that tolerates a stale group.
- [ ] **[public] `TabGroup` fields carry invariants.** `tab_group.rs:15-22`.
  Target: private fields and getters as `DockSplit` has; `NodeIdx` doc gets
  `# Panics` for `DockState::node`.
- [ ] **Tests.** A disabled Splitter mid-drag.

## Batch 15: Scroll and TextEdit

Files: `src/widgets/{scroll, text_edit}/`.

- [ ] **A newly disabled Scroll pans for a frame.** `scroll/mod.rs:451`,
  `scroll/bars.rs:76-77`. Target: `self.widget.response(ui)`; fold
  `disabled` into the bar responses. Add a disabled row to a panning test.
- [ ] **Word delete and Ctrl+Y.** `text_edit/input_pass.rs:191-193`,
  `action.rs:366`. Target: word delete on the platform chords; Ctrl+Y redo on
  Windows. Change `tests/apply_key.rs:408-416`.
- [ ] **The stored zoom outlives its config.** `scroll/mod.rs:395-402`,
  `scroll_state.rs:196-207`. Target: `ScrollState::coerce_zoom` at the top of
  `apply_input`; drop the dead branch.
- [ ] **The IME rect uses the scroll offset before the caret follow.**
  `text_edit/mod.rs:499-516`. Target: read after `state.view.update`.
- [ ] **IME composition has no owner.** `text_edit/mod.rs:68-74, 412-514`.
  Target: a `Composition` satellite; `preedit_cursor` becomes a local.
- [ ] **The undo history has no type.** `edit_state.rs:18-24, 118-144`,
  `editor.rs`. Target: `History` in `history.rs`; `char_count` exact by
  delta.
- [ ] **`InputPass` borrows the whole state row.** `input_pass.rs:54, 78-89`.
  Target: `&mut EditState` and `&ViewState`.
- [ ] **Three copies of the edit edges.** `text_edit/mod.rs:572-645`. Target:
  `EditEdges` is the production type.
- [ ] **`TrackPage` only renames fields.** `scroll_state.rs:131-152, 295-312`.
- [ ] **A Scroll transform is a `debug_assert!`.** `scroll/mod.rs:124-127`.
  Target: release `assert!` with `# Panics`.
- [ ] **Docs.** `zoom_config.rs:9`, `scroll/mod.rs:327` (Ctrl is Cmd on
  macOS), `text_edit/mod.rs:275` and `tests/measure.rs:1-2` (placeholder),
  `scroll/tests/panning.rs:146`.
- [ ] **[public] Scroll state from outside.** `Scroll::offset`, `zoom`,
  `scroll_to`; `ScrollState::reveal` used by the caret follow.
- [ ] **[public] `ZoomConfig` getters.** Store `min`, `max`; derive `Copy`,
  `PartialEq`.
- [ ] **Tests.** Setter sweep rows for `TextEdit::font_size`,
  `TextEdit::color`, `Scroll::pan_by`, `Scroll::content_margin`. One
  table test for soft-wrap motions. One `Field` fixture in
  `text_edit/tests/mod.rs`; move `theme.rs:430, 451`; merge the two axis
  panning tests.

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
