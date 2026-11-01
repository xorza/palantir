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

1. **A device loss is handled at a few host checkpoints, not at the wgpu boundary.** Any wgpu call
   can find the loss, but hosts check `is_lost` only at fixed points. Each wgpu precondition a loss
   breaks (map at creation, configure before acquire) is guarded case by case, and the unguarded
   ones panic under `panic = "abort"`. Recovery reruns startup for the backend only: not for the
   instance, not for the harness, and only once.
2. **Two players of one window.** `Harness` and `Remote` share only the internal `Stage`. Their
   bounds, queries, step labels, device handling and hit rules drifted, so darkroom keeps a
   `Backend` enum that rewrites their errors. `PlayError` stores formatted caller input (`step`)
   and `f32` seconds.
3. **One trait does two jobs.** `Animatable` serves both spring rows and the theme tree. The theme
   pays for a spring contract it never runs (about 60 hand-written lines in
   `TextStyleOverrides`), and the derive grew a theme-only hook (`before_lerp`) and a hand-kept
   parallel walker (`for_each_text`, 15 impls).
4. **Validity is checked one value at a time, never on the resolved face.** A theme file can load
   with overrides or font steps that make text paint nothing; the combined check lives only in
   `scale_text`.
5. **Snapshot copies beside live reads.** `DragValueTheme` copies `button` and `text_edit` at
   `from_parts`, while `ComboBox` reads `button` live; `frost_floats` takes a palette beside the
   theme that holds the inks; the texture limit has four holders; map state is written three ways.
6. **Leftover time policy and redundant animation state.** The substep accumulator and the spring
   `dt` contract outlived the explicit integrator. "No motion" has three spellings and "row
   alive" four signals, each with per-frame cost.
7. **Caches whose soundness rule lives in a doc comment.** Damage skips a subtree by "same
   authoring and same rect", which `ARRANGE_DEPENDS_ONLY_ON_SLOT = false` denies for scrollbars.
   The grid breaks "width in, height out" for indefinite Fill tracks.
8. **Growth policy decided per owner.** Raster atlases have byte budgets; the backdrop results
   atlas and the blur scratch have none, and `grown()` passes a size past the device ceiling.
9. **Rules encoded at the call site, not in the owning type.** Platform conventions match in six
   modules; lane validity differs between `Spacing` and `Corners`; error `source` policy differs
   per variant; five app settings each keep a change flag the host ORs by hand.
10. **Composition and fallbacks are opt-in per widget.** `ThemeDefaults` lacks setters a widget
    needs, so widgets override caller layout or preset it in constructors; wrappers finish inner
    widgets through crate-private hooks; disabled looks exist only where a widget wears a look
    plan; key grants claim whole key classes.

### Target design

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

Batches are sorted by impact, bugs first. Each leaves the build and tests green. Whoever addresses
an item deletes it.

### Batch 1: device loss must not end the process

Files: `src/gpu/resource/dynamic_buffer.rs`, `src/gpu/pipeline/curve_pipeline/mod.rs`,
`src/gpu/surface/window_surface.rs`, `src/gpu/surface/surface_manager/mod.rs`,
`src/host/winit/runtime.rs`, `src/host/winit/mod.rs`, `src/host/core.rs`,
`src/host/offscreen/tests/recovery.rs`, `src/gpu/surface/map_state.rs`,
`src/gpu/surface/capture.rs`, `src/host/window_driver/mod.rs`.

- [ ] **BUG** `dynamic_buffer.rs:120-124`: the grow path calls
  `.get_mapped_range_mut().expect(..)` on a `mapped_at_creation` buffer, which returns `Err` on a
  device found lost earlier in the same submit (`gradient.upload` and `GpuPaint::paint` run
  first). Target: a fill-at-creation helper in `gpu` that writes only if the range maps and always
  unmaps; the grow path uses it.
- [ ] **BUG** `curve_pipeline/mod.rs:78`: `create_buffer_init` panics inside wgpu's util on a lost
  device, so `WgpuBackend::new` aborts a recovery onto a device lost again. Target: the same
  helper, or an unmapped buffer filled by `queue.write_buffer`.
- [ ] **BUG** `window_surface.rs:99-109,120-127`: `prepare` sets `applied` whether or not
  `configure` succeeded; acquire on a never-configured surface hits wgpu's fatal `NotConfigured`.
  Target: set `applied` only if `!gpu.is_lost()` after `configure`; `acquire` returns
  `Acquired::Lost` while `applied.is_none()`.
- [ ] **BUG** `runtime.rs:183-200`: loss is checked only at the top of `draw`; a loss found in the
  frame's submit or present leaves the window on its previous frame until an unrelated wake.
  Target: after `win.frame(..)`, `if self.core.gpu().is_lost() { self.repaint_all() }`.
- [ ] **BUG** `runtime.rs:206-225`, `winit/mod.rs:420-423`: recovery is tried once and any failure
  exits the event loop, losing every window's state; health is not re-read after `core.attach`.
  Target: retry `reopen → attach` with the capped backoff `PresentSchedule` uses, until a healthy
  device holds; fail after N attempts.
- [ ] **BUG** (DX12) `surface_manager/mod.rs:42,109-123`: `reopen` enumerates through the startup
  instance, whose DXGI factory goes stale after a driver update, so recovery can pick the removed
  adapter. Target: `SurfaceManager` keeps the `OwnedDisplayHandle`; `reopen` builds a new instance
  with `GpuRequest::windowed_instance` and renews every surface on it, as startup does.
- [ ] Docs `map_state.rs`, `capture.rs:179`, `window_driver/mod.rs:396-397`: "a lost device runs
  it too, with a failure" holds for `Device::destroy` only; on a HAL loss wgpu-core's `maintain`
  returns early and pending maps never fire. Target: say the callback may never run, and that
  every wait also checks `is_lost`.
- [ ] Tests `offscreen/tests/recovery.rs`, `core.rs`: add a scene that grows a buffer after the
  loss; run `HostCore::attach` over two drivers (extend the two-window core test); cover
  `WindowSurface` acquire before a successful configure.

### Batch 2: the players report the truth

Files: `src/host/harness/{mod.rs,play.rs,harness_builder.rs}`, `src/driver/{locator.rs,play.rs}`,
`src/host/winit/remote/mod.rs`, `src/host/core.rs`, `src/host/offscreen/mod.rs`,
`src/gpu/surface/{texel_readback.rs,owned_target/mod.rs}`, `src/gpu/device/gpu.rs`,
`examples/showcase/shot/{report.rs,script.rs}`.

- [ ] **BUG** `harness/play.rs:79-108,136-161`: on a lost device `shot` and a settle with a
  pending capture run `settle_frames` full frames and return `Unsettled` ("an animation runs on").
  Target: both loops return `PlayError::DeviceLost` once `core.gpu().is_lost()`.
- [ ] **BUG** `harness/play.rs:139-157`: the busy deadline is set once, so an app that keeps
  sending progress wakes for over 30 s fails with `Busy` ("sent no wake in 30 s"); each woken
  frame also spends one of `settle_frames`. Target: restart the deadline on each wake; count only
  frames that asked a repaint. Same bound in `Remote::wait_frame` (`remote/mod.rs:277-307`).
- [ ] **BUG** `driver/locator.rs:141-147`: `out.push(reached(..)?)` returns `Covered` at the first
  covered match, though another match is the one the input reaches (a modal's "Cancel" over the
  page's). Target: skip covered matches like clipped ones; `Covered` only when none is reachable;
  `Ambiguous`/`Nth` over reachable matches. Test with a duplicate label under an overlay.
- [ ] **BUG** `driver/play.rs:69-94`: a `Drag` shorter than `DRAG_THRESHOLD` plays as a click and
  returns `Ok`; `Harness::drag_to` panics on the same input. Target: refuse it after both points
  resolve (**API**: `PlayError::ShortDrag { travel }`).
- [ ] **BUG** `remote/mod.rs:239-261`: `receive` fails any request once `is_hidden` reads true
  after one 50 ms tick, though queries run without a frame. Target: the hidden check only on waits
  that need a frame (`shot`, `wait_frame`); queries wait up to `WAIT`.
- [ ] `harness/mod.rs:576-577`: `point_in` hits with `sense != Sense::NONE`, the "any sensing
  widget" rule AGENTS.md refuses; `hit_at` uses `Sense::hovers`. Target: one routing function
  shared with `locator::reached`.
- [ ] `driver/locator.rs:188-215`: a press on a disabled widget plays and returns `Ok`. Target:
  Click, DoubleClick and Drag refuse it (**API**: `LocateError::Disabled`), as Playwright's
  actionability check does.
- [ ] `harness/mod.rs:479`: `pinch` passes a factor of 0, negative or NaN, which `play` refuses
  as `BadPinch`. Target: release `assert!` plus `# Panics` (cold call, Input validation rule 2).
- [ ] `harness/mod.rs:637-644`, `offscreen/mod.rs:243-248`: `wait_for_gpu` is written twice and
  discards whether the copies landed. Target: `HostCore::wait_for_gpu(&self, &WindowDriver) ->
  bool`; each host forwards it (the bool return is **API**).
- [ ] `texel_readback.rs:89-95` vs `owned_target/mod.rs:128-137`: the readback unmaps a pending
  map "so the next read can map again", and `copy_texels` drops the readback anyway. Target: keep
  the unmap and the buffer; delete `self.readback = None` and update `owned_target/tests.rs`.
- [ ] `gpu.rs:144-167`: the work-done marker in `drain` gives `wait_until` nothing its `landed`
  spin does not; the docs claim "for at most `WAIT_TIMEOUT`" for `wait_until`, whose spin has no
  bound. Target: `drain` is the health check and `poll(Wait)`; correct the bound in the docs.
- [ ] **BUG** (example) `shot/report.rs:121-127` vs `script.rs:438-450`: `quoted` writes text
  through `Debug`, whose `\n`, `\t`, `\u{..}` `unquote` reads back as `n`, `t`, `u{..}`. Target:
  quote with `\"` and `\\` only.

### Batch 3: keyboard activation claims only its keys

Files: `src/widget_core/widget/mod.rs`, `src/input/key_class.rs`, `src/input_state/mod.rs`,
`src/widgets/toggle_row/{mod.rs,tests.rs}`.

- [ ] **BUG** `widget/mod.rs:198-202`: `activation_response` folds `KeyFilter::TEXT` into the
  scope, documented as "bare Space and Enter", but `KeyClass::Text` is all printable characters
  and bare Enter. A focused Button, Checkbox, Switch, RadioButton, MenuItem, Expander header,
  ComboBox, ColorButton or ColorSwatch takes every letter key and bare Enter from its ancestors,
  so single-letter app shortcuts stop after a click, and `ToggleRow` claims Enter against "a
  toggle takes Space alone". Target: an activation grant that claims only its chords (Space, plus
  Enter where the activation takes it), not the class. Test: a focused button and a focused
  checkbox, with an ancestor reading a letter and Enter.

### Batch 4: layout and damage correctness

Files: `src/cascade/engine.rs`, `src/damage/plane_walk.rs`, `src/layout/drivers/scrollbars/`,
`src/layout/drivers/grid/{mod.rs,measure.rs,axis_scratch.rs,placed_child.rs}`,
`src/damage/tests/`, `src/layout/drivers/grid/tests/`.

- [ ] **BUG** `plane_walk.rs:139`, `cascade/engine.rs:415`: when scroll content grows or shrinks at
  a fixed offset, the thumbs move but the bars overlay keeps its subtree hash and `cascade_input`,
  so damage classifies it `SubtreeUnchanged`; in `BarMode::Reserved` the gutter keeps stale
  pixels. Target: fold the referenced viewport's `scroll_content` into the Scrollbars node's
  `cascade_input`. Test: grow content at offset 0, expect damage over the gutter.
- [ ] **BUG** `grid/measure.rs:250`, `axis_scratch.rs:182-190`, `placed_child.rs:83-88`: in a Hug
  grid with two or more Fill tracks, cells measured at INF keep their width while arrange splits
  `Σ fill_content` by weight, so a 200-wide cell in a 110-wide column covers its neighbor; spans
  over Fill tracks add nothing. Target: CSS Grid §12.7.1, `fr = max_i(content_i / weight_i)` with
  spans, so each track gets at least its content. Test: two Fill columns of unequal content.
- [ ] `grid/mod.rs:98-185`, `measure.rs:52-84,150-198`: the track-contribution fold is written
  three times with drifts. Target: one `fold_contributions(..)` used by `Grid::intrinsic` and both
  measure axes; the fix above lands in it once.

### Batch 5: animation clock and rows

Files: `src/ui/frame_runtime/mod.rs`, `src/common/time/mod.rs`, `src/animation/` (`spring.rs`,
`animation_row.rs`, `animation_map_typed.rs`, `mod.rs`, `phase.rs`, `bench.rs`),
`src/ui/mod.rs`, `src/shape/paint_animation/{mod.rs,curves.rs}`, `src/widgets/expander/mod.rs`,
`src/widgets/switch/mod.rs`.

- [ ] **BUG** `frame_runtime/mod.rs:204-213`: the accumulator spends time only at 1/240 s, so at
  240 Hz and faster frames alternate `dt = 0` and `dt = 2×period`, and each `dt = 0` frame still
  records in full. The stall it guarded is gone (`animation/tests/spring.rs:400-405`). Target:
  delete `dt_accum` and `ANIMATION_SUBSTEP_DT`; `dt` is the clamped wall delta; drop
  `Spring::step`'s `dt <= MAX_ANIMATION_DT` debug assert (`spring.rs:89`).
- [ ] **BUG** `animation_row.rs:104-116`: a Duration→Duration `conform` keeps `elapsed`, so a new
  length or ease jumps the value (OutCubic 120 ms at 60 ms shows 87.5 %, at 200 ms 65.7 %); it
  fires at t = 0.5 of a theme transition, where `SlotDefaults::animation` snaps. Target: rescale
  `elapsed` to keep progress, or restart from `cur`. Test it.
- [ ] **BUG** `animation_map_typed.rs:82-90`, `ui/mod.rs:1205`: a non-finite target is only
  `debug_assert!`ed; in release NaN never equals the stored target, so the window repaints
  forever. Target: release `assert!` on the retarget path only, `#[track_caller]` on
  `Ui::animate` (Input validation rule 2).
- [ ] **BUG** `paint_animation/mod.rs:282-310` vs `335-352`: `next_wake` finds step boundaries in
  integer nanoseconds, `phase` in f32, and they disagree at boundaries (810 `(n, k)` pairs with
  n < 200), so a step shows one step late. Target: `(elapsed_ns % period_ns) / step_ns` in both.
  Test: every wake samples a new step.
- [ ] **BUG** `expander/mod.rs:170,231`: `openness > 0.0` reads the raw spring, so an underdamped
  close collapses and re-shows the body. Target now: clamp to `0..=1`. Target with **API**: an
  animate sibling returning `{ value, settled }`; the body hides only when settled at 0.
- [ ] `animation/mod.rs:43-55`: an instant or `None` spec costs two hash probes per call to remove
  a row that rarely exists (1,000 probes a frame for 500 fields); the sweep evicts anyway.
  Target: the instant path returns `target` without touching the map.
- [ ] `animation_row.rs:278-282`, `animation_map_typed.rs:109,200-209`: liveness has three
  signals; the sweep writes `touched = false` into every 856-byte row every record frame, and
  `removed` evicts a row its caller ticked under an unrecorded id. Target: keep only
  `advanced_at == render_frame_id`, read-only; `AnimationMap` sweeps itself.
- [ ] `ui/mod.rs:395`: after a hidden span the theme snaps, paint animations jump to wall time, and
  rows resume a frozen segment. Target: the clock hands consumers one `resumed` bit; rows in
  flight land on their target.
- [ ] `switch/mod.rs:101-117`: two keyed rows (px and color) where one 0..1 row serves, as the
  expander and Flutter's `Switch` do; settling at 1e-4 px instead of 1e-4 of the travel costs
  about 12 to 50 extra full frames per toggle. Target: one `f32::from(checked)` row, geometry and
  color lerped from it.
- [ ] Cycle `animation_row.rs:4` ⇄ `animation_map_typed.rs:4` (and `phase.rs:4`): move
  `TickResult` into `animation_row.rs`.
- [ ] `bench.rs:51-79`: only the all-retarget path is measured. Add `steady` and `instant` cases.
- [ ] `animation_map_typed.rs:229-232`: a test mints frame ids from a `static AtomicU64`, against
  "no mutable statics". Target: derive the id from the map's rows or take it from the caller.
- [ ] Nits: `animation_spec/mod.rs:54` `MEDIUM` is unused; `animation/mod.rs:68`
  `try_typed_mut` can be private; `animation_row.rs:93` `.min(1.0)` is redundant.

### Batch 6: theme validity and theme data

Files: `src/theme/` (`mod.rs`, `metrics.rs`, `text_style.rs`, `text_style_overrides.rs`,
`color_picker.rs`, `expander.rs`, `modal.rs`, `tests/`), `src/ui/mod.rs`,
`src/ui/theme_transition.rs`, `src/primitives/packed/serde/checked/`,
`palantir-anim-derive/README.md`.

- [ ] **BUG** `theme/mod.rs:96`, `text_style_overrides.rs:174-179`: a theme file checks each
  override alone, never against `text`; `line_height_factor: Some(1e-6)` loads and makes a
  1.4e-5 px line that paints nothing (AGENTS.md Input validation rule 4). Target:
  `Theme::faces_valid`, walking `for_each_text` and applying each override to the ambient; the
  load (`#[serde(try_from = "UncheckedTheme")]`) and `scale_text` use it. Add the case to
  `theme_deserialization_rejects_invalid_text_metrics`.
- [ ] **BUG** `metrics.rs:35-44`: `TextSteps` load through `checked::length`, which accepts 0, so
  `body: 0.0` loads and every ambient text paints nothing. Target: a `checked::font_size`
  deserializer shared with `UncheckedTextStyle`; a release `assert!` in `from_parts`.
- [ ] `ui/mod.rs:314-321`, `theme_transition.rs:79`: a scheme change starts a 300 ms run even when
  both schemes hold the same `Rc`, re-recording the tree for about 18 frames. Target: return early
  in `follow_theme` on `Rc::ptr_eq`.
- [ ] `color_picker.rs:95,97,108,175,191`: `VALUE_FONT_PX = 12.0`, a label at 10 px, `gap: 6.0`
  and `VALUE_PADDING = 5.0` are set by hand, against "`Metrics` carries the type ramp". Target:
  `m.text.caption`, a declared step for the label, `m.space.*`; extend
  `every_recipe_reads_its_geometry_from_the_metrics`.
- [ ] `color_picker.rs:100-110`: `mono_states` lists four states by hand. Target:
  `looks.each_mut()`.
- [ ] `expander.rs:75-78`: hand-rolled `a + (b − a)·t`. Target: `f32::lerp`, exact at `t = 1`.
- [ ] `palantir-anim-derive/README.md:8,11`: `use palantir::Animatable` does not exist. Target:
  `palantir::widget::Animatable`.
- [ ] `modal.rs:38`: the comment names `RgbaF32::new`; the code is `with_alpha8(128)`. Delete it.
- [ ] Tests: `tests/text_scale.rs:210-211` copies two error strings (name the constants); assert
  `Rc::as_ptr(ui.theme())` is equal across two transition runs (the no-allocation claim at
  `theme_transition.rs:21-22`); assert `faces_valid` at t = 0.25, 0.5, 0.75 between the stock pair
  and a scaled theme.

### Batch 7: widget correctness and fallbacks

Files: `src/widgets/{editable_label,spinner,menu_bar,tooltip,context_menu,grid.rs,panel,
splitter,tabs,dock,text_edit,scroll,combo_box}/`, `src/widget_core/{configure.rs,configurator.rs,
response.rs}`, `src/scene/node/mod.rs`, `src/ui/state/mod.rs`.

- [ ] **BUG** `editable_label/mod.rs:82-92,136-158`: the editor drops the caller's `disabled`, so
  `EditableLabel::edit` opens a live editor on a disabled label. Target: when disabled, `show`
  closes the editor and `edit` is a no-op; forward node flags through one exhaustive helper
  beside `Node::adopt_placement`, so a new flag fails to compile until it picks a side.
- [ ] **BUG** `spinner/mod.rs:57-66,91-101`: `Spinner::new().size(48.0)` gives a 48 px node with a
  theme-size arc in its corner; `diameter` repeats `Configure::size`. Target: the side from
  `authored_size()` when fixed, else the theme's (**API**: remove `diameter`).
- [ ] `menu_bar/mod.rs:239-241`: titles are keyed by index, so a menu added conditionally moves
  every later title's hover, animation and open slot. Target: key by a label hash or caller key,
  as `TabbedView::keyed`; keep `idx` for arrow order.
- [ ] `configure.rs:414-473`, `scene/node/mod.rs:157-179`: `default_padding`, `default_margin`,
  `default_gap`, `default_min_size`, `default_max_size` check only when they fill, and lack
  `# Panics`. Target: check before the `is_none` test; add `# Panics`.
- [ ] `splitter/mod.rs:78-80`, `tab_strip.rs:111-115`, `dock/dock_view.rs:79`,
  `tabs/tabbed_view.rs:106`, `text_edit/mod.rs:175-178`, `scroll/mod.rs:211`: size and clip preset
  in constructors, so `authored_size()`/`authored_clip()` report values the caller never set.
  Target: `default_*` in `show`.
- [ ] `context_menu/menu_item.rs:89-93`, `combo_box/mod.rs:152-156`, `editable_label/mod.rs:116`:
  `justify` and `child_align` overwrite the caller's choice. Target (**API**: `default_justify`,
  `default_line_gap`, and the rest `ThemeDefaults` lacks): use them here.
- [ ] `ui/state/mod.rs:5`: the doc calls two `T`s under one id a caller bug; the design keys rows
  by `(id, T)` and relies on it. Target: rewrite the doc.
- [ ] Docs: `tooltip/mod.rs` says identity is the trigger's (it is `trigger_id.with("bubble")`);
  `context_menu/mod.rs:62-70` the same for `for_id`; `Tooltip::on` stores `&ResponseSnapshot`
  where its siblings copy it (copy it); `grid.rs:92` cites a comment `Panel::show` lacks.
- [ ] `panel/mod.rs:126-140`: test-only `Panel::wrap_stack_on` duplicates `wrap_stack`. Delete it.
- [ ] Tests: add `EditableLabelResponse` to the no-deref list (`response.rs:139-144`); tests for
  the two bugs above.

### Batch 8: texture growth and the device ceiling

Files: `src/gpu/backdrop/{backdrops/mod.rs,backdrop_blur/mod.rs}`,
`src/gpu/resource/{packed_atlas.rs,wgpu_image_store.rs}`, `src/gpu/raster/raster_atlas/side.rs`,
`src/renderer/{gradient_atlas/mod.rs,frontend/mod.rs,shared_device.rs}`, `src/host/core.rs`,
`src/gpu/device/gpu.rs`, `src/gpu/wgpu_backend/mod.rs`, `src/gpu/frame/{schedule/mod.rs,
frame_passes.rs}`.

- [ ] **BUG** `backdrops/mod.rs:284-323`, `backdrop_blur/mod.rs:429-442,560-566,940-942`: a break's
  jobs stack down the residual scratch with no bound, and `grown()` passes an edge past the
  ceiling (`.min(ceiling).max(edge)`), so `create_texture` can exceed `max_texture_dimension_2d`
  (20 glass cards 300 px tall pass 4096). Target: place jobs with a `ShelfPacker` or wrap at the
  ceiling; split a break that still does not fit; `grown` asserts `want <= ceiling`.
- [ ] `backdrops/mod.rs:78,186-194,333-357`: the `Rgba16Float` results atlas doubles to the
  device maximum before it evicts, with no byte budget (one full-width pane on 7680×2160 makes
  it 512 MiB; the ceiling allows 8 GiB). Target: a byte budget in `AtlasSpec`, `evict_lru` before
  a grow past it, and a texture of its own for a slot wider than half the edge.
- [ ] Texture limit: `raster_atlas/side.rs:28`, `backdrops/mod.rs:190`, `backdrop_blur/mod.rs:945`
  read `device.limits()` themselves; `AttachedStore` (`wgpu_image_store.rs:36-48,72-80,137`) keeps
  its own copy and raw `device`/`queue` clones; `HostCore::max_texture_dim` (`core.rs:138-140`)
  re-reads the `Gpu`. Target: one holder, `SharedDevice::texture_limit`, threaded to constructors;
  `AttachedStore` holds a `Gpu`.
- [ ] `wgpu_backend/mod.rs:558-570` vs `982-1014`: "present the backbuffer" is written twice.
  Target: `Backbuffer::present_onto(encoder, target, blit)`.
- [ ] `frontend/mod.rs:80-88`: `live_panes` re-derived by its own walk. Target:
  `live_panes.extend(scene.panes().map(|p| p.key))`.
- [ ] `schedule/mod.rs:217-221`, `frame_passes.rs:116-121`: a damage-canvas frame clears in the
  load op and then draws `PreClear` again. Target: `Schedule` takes `pre_clear` = the pass loads.
- [ ] Stale docs: `gradient_atlas/mod.rs:116-119` (growth uploads nothing now);
  `packed_atlas.rs:144` (`edge_u16` says 16384; NVIDIA allows 32768).

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
