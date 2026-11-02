# Review: palantir

Eight reviewers ran over the crate: theming and theme interpolation, animation, harness and remote,
device loss, layout with cascade and damage, rendering, the authoring layer, and the base layers
with the desktop host. Each finding below was kept only when it could be defended. Bugs are marked
**BUG**. Items that change the public API are marked **API** and wait for a go-ahead (AGENTS.md,
"Public API"); the plan lists them under the batch that needs them, and the last section lists
them together.

Test timings at the time of the review: lib tests 8.5 s (all features), 5.1 s (no default
features), visual 3.8 s. Five tests take over 1 s on one thread (Batch 11).

## Proposal

### Root causes

1. **The device travels raw beside the type that guards it.** `Gpu` owns what a call on the device
   must respect: its health, `create_filled_buffer`, and the turn every pipeline build takes
   (AGENTS.md, "Pipeline builds"). About 60 fns in `gpu` take `&wgpu::Device`, some with a
   `&wgpu::Queue` beside it, and a few take `&Gpu`, so one constructor family mixes `&gpu` and
   `&gpu.device` at its call sites, and each new guard threads a new signature chain down to the
   call it guards.
2. **A device loss is handled at a few host checkpoints, not at the wgpu boundary.** Any wgpu call
   can find the loss, but hosts check `is_lost` only at fixed points. Each wgpu precondition a loss
   breaks (map at creation, configure before acquire) is guarded case by case, and the unguarded
   ones panic under `panic = "abort"`. Recovery reruns startup for the backend only: not for the
   instance, not for the harness, and only once.
3. **Two players of one window.** `Harness` and `Remote` share only the internal `Stage`. Their
   bounds, queries, step labels, device handling and hit rules drifted, so darkroom keeps a
   `Backend` enum that rewrites their errors. `PlayError` stores formatted caller input (`step`)
   and `f32` seconds.
4. **One trait does two jobs.** `Animatable` serves both spring rows and the theme tree. The theme
   pays for a spring contract it never runs (about 60 hand-written lines in
   `TextStyleOverrides`), and the derive grew a theme-only hook (`before_lerp`) and a hand-kept
   parallel walker (`for_each_text`, 15 impls).
5. **Validity is checked one value at a time, never on the resolved face.** A theme file can load
   with overrides or font steps that make text paint nothing; the combined check lives only in
   `scale_text`.
6. **Snapshot copies beside live reads.** `DragValueTheme` copies `button` and `text_edit` at
   `from_parts`, while `ComboBox` reads `button` live; `frost_floats` takes a palette beside the
   theme that holds the inks; the texture limit has four holders; map state is written three ways.
7. **Leftover time policy and redundant animation state.** The substep accumulator and the spring
   `dt` contract outlived the explicit integrator. "No motion" has three spellings and "row
   alive" four signals, each with per-frame cost.
8. **Caches whose soundness rule lives in a doc comment.** Damage skips a subtree by "same
   authoring and same rect", which `ARRANGE_DEPENDS_ONLY_ON_SLOT = false` denies for scrollbars.
   The grid breaks "width in, height out" for indefinite Fill tracks.
9. **Growth policy decided per owner.** Raster atlases have byte budgets; the backdrop results
   atlas and the blur scratch have none, and `grown()` passes a size past the device ceiling.
10. **Rules encoded at the call site, not in the owning type.** Platform conventions match in six
    modules; lane validity differs between `Spacing` and `Corners`; error `source` policy differs
    per variant; five app settings each keep a change flag the host ORs by hand.
11. **Composition and fallbacks are opt-in per widget.** `ThemeDefaults` lacks setters a widget
    needs, so widgets override caller layout or preset it in constructors; wrappers finish inner
    widgets through crate-private hooks; disabled looks exist only where a widget wears a look
    plan; key grants claim whole key classes.

### Target design

- **`&Gpu` is the one device handle in `gpu`**: every fn that reaches the device or its queue
  takes `&Gpu`, never `&wgpu::Device` or a `(device, queue)` pair. Only the code that runs before
  a `Gpu` exists takes the raw device: `DeviceHealth::watch`, `DeviceRequirements::met_by`.
- **Loss-aware wgpu calls.** Every wgpu call whose precondition a loss breaks goes through a helper
  in `gpu`: a fill-at-creation buffer helper that writes only if the range maps; a
  `WindowSurface` that refuses acquire until a configure succeeded. Every frame path re-reads
  `Gpu::is_lost` after the frame.
- **One recovery path for all hosts.** Each host owns a device source that opens a device exactly
  as startup does (winit: a new instance and first surface; harness: its stored headless request;
  offscreen: the app's `Gpu`). `HostCore::recover(source, drivers)` loops, with the host's
  backoff, until a healthy device is attached.
- **One player contract.** `Harness` and `Remote` expose the same method set (`play`, `settle`,
  `shot`, `find`, `widgets`, queries) under one play-limits config. `PlayError` carries data
  (`Duration`, no `step`) and returns `DeviceLost` on every path. Locator reachability is one
  function shared by `point_in` and `reached`, which filters covered and disabled matches before
  ambiguity. The window side of the remote protocol is one type that both the winit window and
  the test loop drive.
- **`Lerp` for the theme tree, `Animatable: Lerp` for springs.** One derive emits `lerp` and the
  text-slot walk.
- **`Theme::faces_valid`** as the one face predicate, used by the load (`serde(try_from)`),
  `scale_text` and `from_parts`.
- **Every `Theme` field is a primary slot or read live from another slot at `show`**, never a copy
  made at `from_parts`.
- **One `AnimationClock { dt, render_frame_id, resumed }`** per frame, with `dt` the clamped wall
  delta, consumed alike by rows, `ThemeTransition` and paint animations. Rows live while
  `advanced_at == render_frame_id`; the instant path never touches the map.
- **`cascade_input` holds everything a subtree reads from outside itself**, including the
  referenced viewport's `scroll_content`, so cascade and damage skips stay sound for any driver.
- **One grid track-contribution fold** that does CSS Grid's "find the size of an fr" for
  indefinite free space.
- **One growth policy for growable textures**: byte budget plus the device ceiling from `Gpu`,
  `assert!(want <= ceiling)`, 2D packing, evict before growing past the budget.
- **One `PassRecorder`** owning the render pass and its elided state, and one tier lifecycle:
  `prepare(ctx, frame)` and `draw_batch(rec, pipelines, batch)`.
- **One reusable `MapState`** owned with its buffer, shared by `Capture`, `TexelReadback` and the
  query tracer.
- **`Platform` owns every input convention as a method**; lane types have one asserting
  constructor and one crate-private saturating one; errors show a cause in `Display` or in
  `source()`, never both; app settings share one change signal.
- **Every node value a widget supplies goes through `default_*` in `show`**, checked before the
  fill; one exhaustive `Node` adoption helper for widgets that swap themselves for another;
  activation has its own key grant.

## Plan

Sorted by impact, bugs first. Each leaves the build and tests green. Whoever addresses an item deletes it.

### Batch 9: base layers and the desktop host

Files: `src/host/winit/{input/mod.rs,native.rs,window/mod.rs,error/}`,
`src/window/{window_geometry.rs,window_commands.rs}`, `src/ui/mod.rs`,
`src/primitives/geometry/spacing/mod.rs`, `src/text/error.rs`, `src/display/user_scale/mod.rs`.

- [ ] **BUG** `winit/input/mod.rs:176`: `KeyText::new` stops at 14 bytes, so a longer composed text
  (an 18-byte emoji ZWJ sequence) loses its tail. Target: emit the first part as `KeyDown` and the
  rest as `InputEvent::ImeCommit`. Test with 20 bytes.
- [ ] **BUG** `native.rs:82-97`: a saved position is kept only if its top-left lies on a monitor;
  a window snapped to the left edge on Windows reports x ≈ -7 and loses it. Target: keep it when a
  title-bar band of the restored rect intersects a monitor; add the case to the table test.
- [ ] **BUG** `window_geometry.rs`, `ui/mod.rs:810`: `inner_size` is the live size, so geometry
  saved while maximized un-maximizes to full screen. Target: the window keeps its last
  non-maximized size from `Resized` and publishes it through `WindowFacts`, as Qt's
  `saveGeometry`.
- [ ] **BUG** `winit/window/mod.rs:417`: the IME caret converts with `effective_scale()`, pointer
  input with `translation_scale()`. Target: `translation_scale()`.
- [ ] **BUG** `spacing/mod.rs:101`: `impl Add for Spacing` asserts each lane fits f16, so two large
  spacings panic, against "a sum the crate derives saturates". Target: saturate; test overflow.
- [ ] `winit/error/mod.rs:71-98`, `text/error.rs:30`: five variants print the cause in `Display`
  and return it from `source()`, so chain reporters print it twice; the `Gpu` arm already follows
  the rule. Target: one or the other; update `error/tests.rs`.
- [ ] Docs: `window_commands.rs:12` says `Open` reconfigures a live window (it is ignored with a
  warning); `user_scale/mod.rs:5` says "chrome scale" (refused word: "interface scale").

### Batch 10: structure without behavior change

Files as named per item.

- [ ] Rendering, `gpu/frame/binder.rs:56-66`, `image_pipeline/mod.rs:141-146`,
  `wgpu_backend/mod.rs:223-224,866-872`: `MainBinder` and `RenderPass` travel as two `&mut`
  arguments, so any draw can bypass the elision (the image path must remember `forget_group`).
  Target: one `PassRecorder` owning both.
- [ ] Rendering, `wgpu_backend/mod.rs:676-705,909-951` and every tier: each tier has its own
  lifecycle shape, and mesh, image and curve repeat the same boilerplate. Target:
  `prepare(ctx, frame)` and `draw_batch(rec, pipelines, batch)`; a shared instanced-pipeline
  struct.
- [ ] Map state, `query_tracer/mod.rs:529-546`, `map_state.rs`: the tracer keeps its own map bits
  because `MapState` allocates an `Arc` per map. Target: a reusable `MapState` owned with its
  buffer (`map_read(&self, &Buffer)` resets it), shared by `Capture`, `TexelReadback` and the
  tracer.
- [ ] Theme walk, `theme/mod.rs:293-344` and 15 `for_each_text` impls: about 300 lines of
  destructuring for one paired caller, each ending in `unreachable!`. Target: the derive emits the
  walk (`#[text]` fields forward); otherwise one single-tree walk plus a pairing helper.
- [ ] Platform conventions, `winit/input/mod.rs:125,188`, `input/shortcut/mod.rs:77,330,366,378`,
  `input/keyboard/modifiers/mod.rs:62`, `widgets/text_edit/text_nav.rs:23-47`: matches on
  `Platform` outside `common/platform.rs`. Target: methods on `Platform`; tests stay per platform.
- [ ] App settings, `common/app_setting.rs`, `winit/runtime.rs:266`: five change flags ORed by
  hand. Target: one shared change signal raised by `AppSetting::set`.
- [ ] `diagnostics/mod.rs:22`: `Diagnostics` wraps one setting. Target:
  `UiResources::debug_overlay()`.
- [ ] Layout, `layout/engine.rs:37,67` and `cache/mod.rs:118`: two per-root tables walked in
  lockstep. Target: one `Vec<RootRun { key, plane, slot }>`.
- [ ] Cycle `layout/pass.rs:23,73` ⇄ `layout_scratch.rs:13`: move `ReplayOrigin` into
  `layout_scratch.rs`.
- [ ] `layout/pass.rs:98,174`: `measure_per_axis_hug` is ZStack/Canvas policy on the pass;
  `grid_track_state_mut` duplicates `grid_mut().track_state`. Target: a free fn in `drivers/`;
  drop the duplicate.
- [ ] `layout/axis_slot.rs:34,229-246`: `AxisSlot` built two ways. Target: `AxisSlot::new(..)`;
  `unbounded` calls it.
- [ ] `cascade/mod.rs:40-62`: `by_id` and `entries` are indexed directly from `input_state`.
  Target: private fields with `contains(id)` and `entry(WidgetLocation)`.
- [ ] `input_state/mod.rs:64-117`: 25 fields and a 250-line `apply`. Target: `Pointer`,
  `Keyboard` and `Ime` owners beside `Focus`; `apply` dispatches.
- [ ] Remote protocol, `remote/mod.rs:42-89`, `winit/window/mod.rs:145-172,319-325`,
  `runtime.rs:164-169`, `remote/tests.rs:57-128`: the window side is spread over three files and
  re-implemented by the test loop. Target: one window-side type both drive.

### Batch 11: tests

- [ ] Over 1 s on one thread (the guide's limit):
  `gpu::backdrop::tests::neighbors::more_stale_panes_than_a_partial_frame_holds_paint_whole`
  (1.57 s), `a_pane_growing_back_paints_once_under_overlapping_repaints` (1.06 s),
  `clear::a_pane_with_nothing_under_it_shows_the_clear` (1.05 s),
  `gpu::surface::stencil::tests::a_stencil_on_memory_buffers_held_clears_to_zero` (1.04 s),
  `clear::a_pane_shows_the_clear_or_a_capture_as_what_lies_under_it_changes` (1.03 s). Target:
  smaller surfaces or fewer rounds with the same assertions.
- [ ] Layout drivers are tested only through `Harness`, `Ui` and widgets, so a widget default
  moves layout expectations; `scrollbars/` has no tests. Target: a `crate::internals` builder that
  records `Node`s into a `Forest`, run through `LayoutEngine` on the mono shaper.
- [ ] Both players lack `play` tests for Drag, Scroll, Pinch (with `BadPinch`), Key with
  modifiers, and `Wait` on `Remote`. Target: one table of steps run on a `Harness` and a served
  `Remote`.
- [ ] `damage/tests/shapes.rs:181,250`: hover and unhover copy one 25-line fixture. Target: one
  test parameterized by direction.
- [ ] Test modules past 40 % of their file, to split into `foo/{mod.rs,tests.rs}`:
  `gpu/pipeline/quad_pipeline/quad_draw.rs` (63 %), `gpu/surface/surface_usages.rs` (56 %),
  `renderer/gradient_atlas/bake.rs` (50 %), `gpu/frame/frame_passes.rs` (47 %),
  `gpu/frame/binder.rs` (45 %), `renderer/gradient_atlas/dirty_rows.rs` (44 %),
  `gpu/surface/stencil.rs` (43 %), `shape/paint/lens_optics.rs` (41 %),
  `shape/paint/spectrum_tables.rs` (41 %).

## Waiting for a go-ahead

### Public API changes

- **Harness recovery**: `Harness::is_device_lost` and `recover`, or the harness reopening its own
  headless device, so darkroom's MCP session survives a TDR (Batch 2 needs one of them).
- **`PlayError`**: `Duration` for `NoFrame`/`Busy`; drop `step` from `Unsettled`/`Busy` (darkroom
  rewrites it, and the two players format it differently); add `ShortDrag`; add
  `LocateError::Disabled`.
- **`Remote`**: `find`; a play-limits config or `with_wait(Duration)` (darkroom cannot set its
  bounds for the window backend); `&mut self` on `play`, `settle`, `shot` in place of `Cell`s.
- **`wait_for_gpu`** returning whether the copies landed.
- **`Theme::frost_floats`** without the `Palette` argument: the inks come from the theme frosted.
- **`Lerp` split** from `Animatable`, with its own derive.
- **`DragValueTheme`** reshaped to `{ looks, defaults, caret, caret_width, selection,
  placeholder }`, the editor slot derived at `show`; 8 dead `selected` looks go.
- **Drop the bundles' `Default`** (each is the dark recipe), or name the scheme.
- **`ComboBox::menu_style`**.
- **`Expander::open/close/is_open(ui, id)`** in place of the `&mut bool` binding named `open`.
- **Wrapper hooks made public**: `PopupTrigger::layer`, `PopupTrigger::menu_look`,
  `Popup::menu_look`, `Splitter::min_ratio`, `TabStrip::drop_caret`.
- **`default_justify`, `default_line_gap`** and the rest of `ThemeDefaults`.
- **`Spinner::diameter`** removed.
- **`EditableLabel`**: focusable, opened by Enter or F2, with `stop_editing(ui, id)`.
- **An animate sibling returning `{ value, settled }`**.
- **One spelling of "no motion"**: `AnimationSpec` with serde default `SNAP`, or `SNAP` removed.
- **Reduced motion for paint animations**: a decorative marker on `PaintAnimation`, or a doc gate
  on `add_shape_animated`.
- **`Easing::curve(self) -> PaintCurve`**; `PaintCurve`'s doc corrected to `[0, 1]`.
- **`Span`**: `TryFrom<Range<usize>>` in place of a `From` that panics.
- **`Corners::new`** asserting lanes as `Spacing::new` does, with a crate-private saturating twin.
- **Disabled looks** for Slider and the color widgets (a look plan with a disabled state changes
  their theme types).

### Recorded decisions this review questions

- **"The app uploads its images again when `Ui::device_restored()` is true."** The signal is per
  window for host-wide state, so an app with two windows uploads each image twice, and no in-tree
  consumer calls it (darkroom's previews stay blank after a winit recovery). Proposal: keep the
  policy, put the truth on the handle as `ImageHandle::is_resident()`, cleared by
  `WgpuImageStore::attach`.
- **`ExpiryWheel::grow_buckets` grows every bucket at once** so no bucket allocates after warmup.
  With 256 buckets and 32-byte tickets, one burst of N inserts keeps 256 × N × 32 B for the
  session (N = 4000: 32 MiB, twice, for the shaped and encoded caches). The choice is between that
  RAM and one bucket doubling in a later frame.
