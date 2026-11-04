# Palantir redesign proposal (review of 2026-11-03)

Nine reviewers read the crate, one slice each. They did not build or run it. This file merges
their 245 findings. It names the root causes and gives the work in batches, with the highest
impact first. Each batch changes one system. IDs refer to the findings index at the end.

`✔` marks a claim that was read again in the code during the merge. A claim with no mark is
the reviewer's own reading only.

## Overall picture

The pipeline design is sound. The reviewers found no defect in the damage walk, the row
matcher, clear folding, occlusion pruning or the `SeenIds` fast path, and the tests are
strong: exact values, differential references, table sweeps. The problems come from a small
number of root causes, and these causes repeat in every layer.

### R1. One fact has many owners

The crate often stores one fact in several places, and code or debug asserts keep the copies
in step.

- **Where a widget was last frame:** `SeenIds` (current and previous tables, plus the
  positional route with `cascade_paired`), `Cascade::by_id`, the 64-bit identity fold in
  `MeasureSnapshot`, and the damage `prev` (SC-05, LT-04) ✔.
- **Which panes are live:** `PaneWalk.keys`, `RenderBuffer.live_panes`, `PaneCaptures.held`
  and the `Backdrops` cache. Each is rebuilt or scanned linearly (RD-03, RD-08, RD-09,
  RD-19) ✔.
- **The frame descriptor:** `FrameSpec`, `TargetKey`, `HostDisplay`, `Display` and
  `FrameStamp`. `pixel_snap` passes through five hops (HS-07).
- **Whether the harness paints:** `depth`, `target` and `core.device` (HS-08).
- **Layout columns:** declared four times (LT-03). Grid tracks are in a per-grid store and in a
  per-depth stack (LT-02).
- **Node fields:** written in `Configure`, `Configurator`, `ThemeDefaults`, `authored_*`, and
  `Node::set`/`fill`. About 100 methods (UI-07).
- **Other copies:** `Hsv` and `Okhsv` (PR-05). `OffscreenHost` and `Harness` (HS-05).
  `Singletons` beside `StateMap` (UI-06). Two ways to close an overlay (UI-05).

### R2. Hand-written keys and hashes

A key that lists its inputs by hand, with no exhaustive destructure, misses an input. This
is the source of the silent correctness bugs:

- The layout hash leaves out the `Stack`/`WrapStack` axis ✔ (SC-01). The measure cache then
  replays the old arrangement after an axis change.
- `CascadeKey` leaves out the root layer (SC-03).
- The exact rect hash and the quantized per-node hash disagree (SC-19).
- A hash fold stands in for an exact identity in `MeasureSnapshot` (LT-04) ✔ and in
  `TextProbe::text_hash` (LT-15).

The shape hashes destructure every field, so the compiler forces a decision. The layout tail
and `CascadeKey` do not.

### R3. Policy written again for each resource

Each resource has its own copy of a shared rule:

- **Growth:** `DynamicBuffer`, the blur params, the `Backdrops` group, and the extra growth
  tier of the raster atlas (GB-02, GB-10, GB-11).
- **Decay:** only the blur textures decay. `DynamicBuffer`, the staging belt and the stencil
  keep their high-water mark for the life of the window, against the "Scratch decays"
  decision (GB-02, GA-03, GA-04).
- **Packing:** three packers (GB-12).
- **Eviction:** four schemes. The raster clock, the backdrop LRU, `HeldTables`, and the
  gradient MRU.
- **Upload:** two loops that stage rows (GB-22).
- **Spatial index:** four indexing policies over `TileIndex` (RD-02).
- **Depth scratch:** three idioms (LT-08).

### R4. Bools and Options where an enum would hold the invariant

- **Frame shape:** `FrameShape` holds four bools, and `submit` takes its resources back out
  of `Option`s with `expect`/`unreachable!` (GA-05, GA-08) ✔.
- **Overlay spec:** `OverlaySpec` asserts a link between `Backdrop` and `ClickOutside` at run
  time (UI-09).
- **Mixed states:**
  - `Ident` holds a recipe and its result in one enum (SC-06).
  - `activate()` writes `Up` beside `Drag::Active` (UI-12).
  - `TabDrag { tab: Option, target: Option }` (WG-18).
  - `HostCore` holds `Option<DeviceHalf>` (HS-08).
- **Pairs that must agree:** a pipeline and its layout (GB-07), a depth used as a stencil mask
  (GB-18), and a UV width that the atlas budget does not set (GB-19) ✔.

### R5. Change detection and scheduling watch inputs, not results

- `set_theme_for` compares by `Rc` identity ✔, so a call every frame keeps the window awake
  for ever (UI-02).
- `set_user_scale` and `set_debug_overlay` do not request a frame on the headless hosts
  (UI-04).
- A winit frame leads by its schedule, not by what it ran. A caret blink in one window makes
  every other window record again at the refresh rate (HS-01).
- `Remote::settle` counts inputs but not queries, so it can read the frame before a query
  (HS-02).
- Settle treats every timed wake as settled (HS-22).
- One `keeps_structure` gate covers two kinds of validity, so each keystroke rebuilds the
  whole cascade ✔ (SC-04).

### R6. One mechanism with two meanings

A `KeyFilter` scope means "the keys this widget takes" and also "this overlay owns the planes
below". Every button declares a Space/Enter scope ✔. So a pass-through popup or toast that
holds a button silences all keyboard and pointer events in the main window ✔ (UI-01).

### R7. Validation at entry only

Values are checked where they enter, but arithmetic after that point can leave the domain,
and some `Result` paths panic:

- `min_luminance_for` returns a `Result` but panics in `after_storing` ✔. `Theme::from_parts`
  then panics for a light popup with a mid-grey `text_muted` under `float_material` (PR-01).
- Spring or `OutBack` overshoot makes a negative radius or width. That value is a release
  panic at `Chrome` validation (PR-04).
- A gradient stop accepts HDR and then clamps it without notice (PR-08).
- NaN chrome is fixed in release, not refused, at the public `Widget::record` (SC-02).
- `Idx16` panics at 65,535 rows, which a 300×250 grid reaches (SC-18).
- The font-family key table panics at 65,537 families (LT-10).
- The bound-number store rounds after it snaps, so the value leaves the step grid ✔ (WG-01).

### R8. Resources shared by the backend but planned for one window

- `CutoutPlan` is per window but packs into the shared `CutoutTables` atlas, so two windows
  thrash (GB-03).
- `SingleQuadBuffer::is_uploaded` stays true after frame 1 ✔ (GB-05).
- The blur expiry clock counts prepares from every window, so it expires N times too fast
  (GB-23).
- The gradient-atlas epoch across windows is not verified.

### R9. The authoring surface is copied for each field

`const fn` setters cannot come from a trait, so setter families are copied by hand:

- `chrome`/`default_chrome` 18 times, with a test that polices the copies (WG-12).
- Eight typography setters, three times (WG-11).
- Preference quintets (UI-14).
- The 27 fields of `UncheckedTheme`, three times (UI-17).
- 16 per-slot `stock` functions (UI-18).
- The `Configure` mirror (UI-07).

### R10. Interaction falls short of the canonical practice

| Area | Findings |
|---|---|
| Text editing | No goal column (WG-02). Triple-click selects all in multi-line (WG-03) ✔. No Ctrl+Y (WG-19). Undo groups with no bound (WG-20). |
| Keyboard | Scroll takes no keys and does not reveal focus (WG-06). Menubar Down/Up do nothing (WG-07). Radio groups (WG-08). Shift+Space is swallowed (UI-21). |
| Pointer | Tooltip does not hide on a press or Escape (WG-05). Splitter drops the grab offset (WG-21). Dock shows a drop it refuses (WG-04). |
| Platform | Wayland clipboard (HS-15). Tiler detection on DXGI and GLES (GA-01) ✔. |

### R11. Test seams in production state

- `PaneWalk.on` ✔ (RD-04).
- `shadow_grid: bool` ✔ (GB-20).
- `#[cfg(test)]` stencil usage ✔ (GA-07).
- `WgpuBackend.pass_traffic` mirror ✔ (GA-11).
- The payload IR, whose second consumer is only for tests (RD-07).

## Decisions in AGENTS.md the reviewers dispute

Each item needs your answer before its batch starts.

1. **"Refused: a one-window type for the two headless hosts"** (HS-05). The reviewer
   proposes a private `HeadlessWindow` that both hosts compose, not a merge of the public
   types. The winit host stays multi-window. The copies already drift (HS-10, HS-11).
2. **"A wrapper finishes the widget it wraps through its public setters, never a hook"**
   (WG-15). The code breaks this rule eight times, because `DockView` needs a strip protocol.
   You can amend the rule to allow `pub(crate)` protocol entry points, or make the hooks
   public.
3. **"A pick binds a reference"** (WG-16). `TabbedView` binds `&mut usize`, which is correct,
   because close and reorder make a reference invalid. The rule needs a stated exception.
4. **"A `Response` borrows the `Ui`"** (UI-10). The borrow makes the owned twins
   `ResponseSnapshot` and `ValueEdges`, and puts `'a` on every response type. No measurement
   supports the lazy probe. The proposal is to measure an eager `Response` first.

## Batches

The order is by impact: user-visible bugs and panics first, then perf with wide reach, then
the reduction of complexity. Each batch stays inside one system, so one review covers it. A
batch marked **API** changes the public surface and needs your go-ahead (AGENTS.md "Public
API").

### Batch 6b. Radio group (`widgets`) **API**

What Batch 6 left: a radio group with one Tab stop whose arrow keys move focus and the pick
together, as the APG radio group does. It needs a group container or `arrow_focus` that writes
the pick, which is new public surface (WG-08).

### Batch 9b. GPU public surface (`gpu`) **API**

What Batch 9 left, as each changes the public surface:

- Add `From` for `GpuRequestError`'s sources, so `?` converts (GA-16).
- An `OwnedTarget` keeps its view, so an offscreen frame makes no view; `RenderTarget` is public
  (GA-17).

### Batch 10. Authoring surface and the overlay protocol (`widget_core`, `widgets`, `theme`) **API**

Root causes R9 and R1. This batch removes the most code and changes the most public surface.

- One macro generates each consuming setter from its borrowing form, `chrome`/`default_chrome`
  on the nine containers, and the eight typography setters. Delete the test that polices the
  copies (UI-07, WG-11, WG-12).
- One `authored() -> &AuthoredNode` view replaces 21 `authored_*` getters (UI-07). Remove
  `ThemeDefaults` from the builders the app sees (UI-08).
- `ui.close_overlay()` replaces `CloseHandle` and the body parameter (UI-05). `OverlaySpec`
  becomes `Backdrop::{None, Root(Capture), Barrier(WidgetId, Capture)}` (UI-09). `OverlayScope`
  owns the `opened` probe.
- One overlay trigger: `Menu::below` is a dressed `PopupTrigger` (WG-14). One id function for
  each overlay kind (WG-13). A non-generic marker for the associated functions (WG-24).
  Settle decision 2 (WG-15).
- One draft protocol: the color-picker hex field and `DragValue` editing use
  `TextEdit::buffered` (WG-09, WG-10).
- Merge `Singletons` into the state map (UI-06).
- Store a state row as `T`, not `Option<T>`. Merge the two `TabStrip` rows (WG-18).
- `Preference<T> { System, Fixed(T) }` with `ui.preferences()` (UI-14). Use the serde remote
  idiom for `UncheckedTheme`, with an error enum (UI-17). Remove the per-slot `stock` (UI-18).
  Rename the private `ToggleKind` trait (WG-17).
- Group the retained memory for each widget (`state`, `animation`, `gpu_views`, singletons)
  into one `Memory` with one `sweep(removed)`. Group the host seam (`window_requests`,
  `window_frame`, `gpu_pass_stats`, `waker`) into one struct (UI-16).
- Decision 4: measure an eager `Response`, then decide (UI-10).
- `Theme::lerp` still copies both ends each transition frame, for its text-axis hook to edit;
  share the axes once when a run starts and blend the shared ends by reference (UI-20).
- Remove the theme-transition spares (UI-19). Give each field of `FrameRuntime` one access
  path (UI-26). State the bounds of `repaint_wakes`, the input queue and `Watches` (UI-27).
  `faces_valid` takes `&self` (UI-28).
- Smaller items: `focus_end_within` public (UI-23). A `TabKey` newtype (WG-25). Fix the doc
  for `TabbedView` (WG-16). Dress `Menu::context` only when it is open (WG-22).
  `TextEdit.text` is not an `Option` (WG-23).

### Batch 11. Headless host consolidation (`host`, `display`, `window`)

Root cause R1. Settle decision 1 first.

- A private `HeadlessWindow { core, driver, wake, display facts, clock }` and one settings
  struct, used by `OffscreenHost` and `Harness`. Their builders share it (HS-05).
- Add `HostCore::frame(…)`, so each host keeps only its target and its present (HS-06). With
  it, a test of `WinitRuntime`'s scheduling (draw, follow, recover) on a fake loop and clock,
  and `Remote` on a clock it is handed in place of real sleeps (HS-23).
  `assert_runs_palantir` moves into `HostCore` (HS-09).
- `FrameSpec { target: TargetKey, … }`. Delete `HostDisplay`, `HostCore.pixel_snap` and
  `DriverConfig` (HS-07).
- `enum HarnessPaint { Record, Compose, Gpu { target } }`. Split `HostCore` by type, not by
  `Option` (HS-08).
- One `SystemPreferences` input for every host, the double-click interval included (HS-11).
- **API:**
  - Matching verbs on `Harness` and `Remote`, or a public trait from `driver::play::Stage`
    (HS-12).
  - `FrameSummary` becomes crate-private (HS-13).
  - One `SystemScale` constructor (HS-17).
  - A type for logical and physical window sizes (HS-18).
  - A palantir enum for `PowerPreference` (HS-19).
  - Getters on `Display` (HS-26).
- Remove the pass-throughs that are only for tests (HS-20). Clean up `Cargo.toml` (HS-24).
  Enable Wayland clipboard (HS-15).

### Batch 12. Layout internals (`layout`, `text`)

Root causes R1 and R3. Internal only.

- Measure and arrange columns become one SoA type each. The scratch, the output and the
  snapshot use it (LT-03).
- All grid track arrays go in the per-grid store. Delete `GridDepthStack` (LT-02).
  `track_aggregator` becomes a `DepthScratch` (LT-08).
- Text caches: give the reuse row its true role and refresh it once a frame (LT-06).
  `CacheEntry` holds a `ShapedBuffer` (LT-07). Container spans replay on a copy hit (LT-12).
- Smaller items:
  - Drop `ARRANGE_DEPENDS_ONLY_ON_SLOT` and `ReplayOp` (LT-11).
  - Keep `PlaneInput` whole (LT-16).
  - Make the engine fields private (LT-17).
  - Use a newtype for each `Measured` space (LT-18).
  - Use a flag set for `single_req` (LT-19).
- Anchored popups measure against the side they pick (LT-13).
- Text identity becomes an edit generation, not a hash (LT-15, **API**).
- Test truncation in layout against cosmic, not only the mono metric (LT-20).

### Batch 13. Renderer and shape IR (`renderer`, `shape`)

- Use one `RectSet` with lazy indexing past a threshold for `RectGrid`, `TierRects`,
  `closed_grid` and the pruner (RD-02).
- The brush resolver returns the GPU lanes directly. Remove `BrushSource` (RD-05). Record the
  payload IR as a decision, or fold it into the lowering (RD-07). Embed `GroupBatch` in
  `TextBatch` and remove `PerGroupBatch` (RD-10).
- `ComposeSession::finish(self)` replaces the work done in `Drop` (RD-15). `from_damage`
  takes a `Damage` (RD-12).
- One `pane_fill` helper for reduced transparency (SC-07).
- Remove the forwarding layers: `NodeChrome` passes straight through and `NodeCols` goes
  (SC-08, SC-09). The `RecordStore` forwards go (SC-13).
- The mesh arena becomes two `Vec`s (SC-14). One exhaustive `pre_record` (SC-15). Use
  `ContentHash` everywhere (SC-16).
- Give the lossy `From` conversions names (SC-12). Share one function for the inherited
  disabled and visible values (SC-10).
- Move these items: `ImageTooLarge` and `TextureLimit` out of `renderer` (RD-01).
  `RenderOwnerId` into `gpu` (RD-11). The spectrum tables into `gpu` (SC-17).
- Smaller items: RD-13, RD-14, RD-17, RD-18, RD-21.

### Batch 14. Primitive types and their public surface (`primitives`, `common`, `animation`) **API**

- `ColorCoords { model, h, s, v }` replaces `Hsv`/`Okhsv` and the parallel enum pairs
  (PR-05).
- `Size` for every extent and `Vec2` for offsets (PR-06). `Range<usize>` in `ImePreedit`
  (PR-19).
- One checked lane packer generated from `LaneCodec` (PR-10). One niche-index encoding
  (PR-13).
- One gradient default, Oklab (PR-11). Drop the dead `Eq` (PR-12). Drop `is_snap` (PR-14).
- Move the theme-file validators beside `domain` (PR-09). Make the domain aliases delegate
  (PR-18).
- The `TextSlots` derive leaves the public derive crate (PR-17).
- Smaller items: PR-07, PR-15, PR-16, PR-20 to PR-23. `GpuFrameCtx`/`GpuInitCtx`
  `#[non_exhaustive]` (GA-14). `LocateError` holds points (UI-25).

## Findings index

Prefixes:

| Prefix | Slice |
|---|---|
| GA | gpu device, surface, backend, frame |
| GB | gpu pipeline, raster, resource, backdrop |
| RD | renderer and damage |
| SC | scene, shape, cascade |
| LT | layout and text |
| PR | primitives, common, animation |
| UI | ui, input, theme, widget_core |
| WG | widgets |
| HS | host and platform |

| Batch | Findings |
|---|---|
| 6b | WG-08 |
| 9b | GA-16, GA-17 |
| 10 | UI-05 to UI-10, UI-14, UI-16, UI-17, UI-18, UI-19, UI-20, UI-23, UI-26 to UI-28, WG-09 to WG-18, WG-22 to WG-25 |
| 11 | HS-05 to HS-09, HS-23, HS-11 to HS-13, HS-15, HS-17 to HS-20, HS-24 to HS-26 |
| 12 | LT-02, LT-03, LT-06, LT-07, LT-08, LT-11 to LT-13, LT-15 to LT-20 |
| 13 | RD-01, RD-02, RD-05, RD-07, RD-10 to RD-18, RD-21, SC-06 to SC-10, SC-12 to SC-17 |
| 14 | PR-05 to PR-07, PR-09 to PR-23, GA-14, UI-25 |

These findings are kept as they are, with no change: SC-20 (the `SeenIds` size is justified
by a measurement) and RD-16 (the copies of the rounded-clip chain are accepted unless a
profile shows them). `.notes/ISSUES.md` already logs the `Staging::abort` race.

## Findings in full

Each open finding as its reviewer wrote it, grouped by the batch that takes it. A batch reads
its own findings here; the line numbers are from the review and drift as the code moves.

### Batch 6b

**WG-08 · bug (APG) · `radio/mod.rs`, `toggle_row/mod.rs:78`**
- **What's wrong:** every RadioButton is its own Tab stop and ignores the arrow keys. A caller's `arrow_focus` on the container only moves focus; it does not check the radio.
- **Why it matters:** the APG radio group has one Tab stop, and the arrows move focus and check together (selection follows focus). The color picker's model switch (`color_picker/mod.rs:569-580`) inherits this.
- **Direction:** add a group container, or make a focused radio's arrow keys write the neighbouring value. This needs a group notion.
- Confidence: high on behavior, medium on direction · public API change: likely · root cause: no group abstraction for discrete picks.

### Batch 9b

**GA-16 — idiom — `device/requested_gpu.rs:110,132,155`**
- **What:** Three `.map_err(|source| GpuRequestError::X { source })` calls where `From` impls would let `?` convert. The source types are distinct, so `From` is unambiguous. `SurfaceStartError` already does it this way.
- **Direction:** Add `From<RequestAdapterError>`, `From<UnmetRequirements>` and `From<RequestDeviceError>`.
- **Confidence:** high · **Public API:** adds impls · **Root cause:** inconsistent error plumbing.

**GA-17 — perf — `wgpu_backend/mod.rs:481-484`**
- **What:** Each Direct-route or overlay frame calls `surface_texture.create_view(..)`: a new wgpu-core view (on Vulkan a `vkCreateImageView`) every frame. `RenderTarget` carries only `&Texture`.
- **Why it matters:** This is unavoidable for a swapchain image, but an `OwnedTarget` an offscreen host renders into every frame could hold its view. That makes it a per-frame allocation the posture forbids.
- **Direction:** Let `RenderTarget` optionally carry a view its owner keeps; the swapchain path keeps making one per frame.
- **Confidence:** med · **Public API:** possibly (`RenderTarget` is public) · **Root cause:** the target type erases whether its owner can keep things.

### Batch 10

**UI-05 · complexity · high · public API** — `src/widget_core/close_handle.rs`, `src/widget_core/overlay_stack.rs`
- **What:** a body asks its overlay to close in two ways. Dialog content calls `CloseHandle::close()`, a `Cell<bool>` passed as `&CloseHandle` into every Popup, Modal, Menu and PopupTrigger body closure. `MenuItem` instead calls `ui.overlays().close_innermost()` (`widgets/menu/menu_item.rs:148`). `OverlayScope::record` ORs the two together.
- **Why it matters:** two routes to one result, and every body signature carries an extra parameter.
- **Direction:** keep the `OverlayStack` and expose a public `ui.close_overlay()`, as egui 0.32's `ui.close()` closes the innermost closable container. Then drop `CloseHandle` and the closure parameter.
- **Root cause:** the same signal is implemented twice.

**UI-06 · complexity · med · public API** — `src/ui/singletons.rs` vs `src/ui/state/mod.rs`
- **What:** `Singletons` (a map from `TypeId` to `Box<dyn Any>` with its own `Slot::Lent` lending) behaves exactly like a `StateMap` row under an id no node records. `with_state` already documents that such a row "lives as long as the Ui".
- **Why it matters:** two type-keyed stores, two lend/put protocols and two panic messages for one thing. egui's `IdTypeMap` covers both, with `Id::NULL` for globals.
- **Direction:** implement `with_singleton::<S>` as `with_state::<S>` under a fixed per-type id, or drop it.
- **Root cause:** the same signal is implemented twice.

**UI-07 · complexity · med · public API** — `src/widget_core/configure.rs`, `src/widget_core/configurator.rs`, `src/widget_core/widget/mod.rs:301-433`
- **What:** each node field is written four or five times:
  - the consuming `Configure` method (30),
  - the borrowing `Configurator` method (41),
  - the `ThemeDefaults::default_*` consuming form (11),
  - a `Widget::authored_*` getter (21),
  - plus `Node::set_*` and `fill_*` in `scene`.
- **Usage:** 13 of the 21 `authored_*` getters have no caller outside `widget/mod.rs`. Several of the eight that are used rely on in-module or test callers alone.
- **Why it matters:** about 100 methods with duplicated docs that drift.
- **Direction:** generate the consuming forms from the borrowing ones with one macro (bulk is a valid reason for a macro), and replace `authored_*` with one read-only `authored() -> &AuthoredNode` view. Related to the TODO entry "widget-authoring surface".
- **Root cause:** the authoring surface is mirrored per field.

**UI-08 · api/doc · med · public API** — `src/widget_core/configure.rs:389`
- **What:** the doc says "an app chaining `.default_padding(..)` onto a `Button` overrides nothing". In fact an app's `default_padding(x)` fills `padding` first (`Node::fill_padding`), and the theme's own fill in `wear` (`widget/mod.rs:240`) then does nothing. So for an app it is just a second spelling of `.padding(x)`.
- **Direction:** fix the doc. Better, keep `ThemeDefaults` off the app-facing builders (export it only through `widget::`) so apps have one setter.
- **Root cause:** the authoring surface is mirrored per field.

**UI-09 · idiom/types · med · public API** — `src/widget_core/overlay_scope.rs:96`
- **What:** `assert_eq!(spec.backdrop.owns_input(), spec.click_outside != ClickOutside::PassThrough, …)` guards a combination the types allow, and two of the three `Backdrop` variants combine wrongly with one of the policies.
- **Direction:** make `Backdrop::{None, Root(Capture), Barrier(WidgetId, Capture)}` with `Capture::{Block, Dismiss}`. `ClickOutside` stays on the builders and maps into it.
- **Root cause:** the code asserts an invariant that the types could hold instead.

**UI-10 · api/complexity · med · public API (flags an AGENTS decision)** — `src/widget_core/response.rs`
- **What:** "A Response borrows the Ui immutably" so it can probe lazily. The borrow forces owned twins (`ResponseSnapshot`, `ValueEdges` in `value_response.rs`) and puts `'a` on `InnerResponse`, `ValueResponse` and every `*Response`. AGENTS cites no measurement for the lazy probe; egui's `Response` is owned and eager.
- **Direction:** measure an eager `response_for` in `integration/frame/*`. If it costs only a few percent, make `Response` the owned snapshot and delete `ResponseSnapshot` and `ValueEdges`.
- **Root cause:** the borrowed `Response` handle.

**UI-14 · complexity · med · public API** — `src/ui/mod.rs:302-435`
- **What:** three app-global preferences (scheme, motion, transparency), each with getter, setter, system getter, crate-private system setter and an `is_*`, add about 15 `Ui` methods. They forward to 18 `AppSettings` accessors. `SchemePreference` and `ReducePreference` are the same three-state enum shape. The three also snapshot differently: scheme and transparency are resolved once per frame, motion is read live (as documented).
- **Direction:** one `Preference<T> { System, Fixed(T) }`, and one `ui.preferences()` / `set_preferences(...)` pair over a plain struct, as egui's `ctx.options_mut`.
- **Root cause:** pass-throughs, with the surface mirrored per setting.

**UI-16 · complexity · med · no API change** — `src/ui/mod.rs:129-161` (16 fields, 92 `pub` methods and about 50 `pub(crate)`)
- **What:** `Ui` is the hub, and many methods forward two hops (`Ui::focus_policy` → `InputState::focus_policy` → field; the same for `set_input_policy`, `focused`, `modifiers`, `pointer_pos` and others). `finalize_frame` sweeps four per-widget stores one by one (`animation`, `state`, `gpu_views`, `text`).
- **Direction:** group retained per-widget memory (`state`, `animation`, `gpu_views`, singletons) into one `Memory` with one `sweep(removed)`, as egui's `Memory` does. Group the host seam (`window_requests`, `window_frame`, `gpu_pass_stats`, `waker`) into one struct. Make `InputState`'s plain levels `pub(super)` fields instead of getters.
- **Root cause:** `Ui` as an aggregation hub with pass-through layers.

**UI-17 · idiom · med · no API change** — `src/theme/mod.rs:426-526`, `src/theme/text_style.rs:194`, `src/theme/text_style_overrides.rs:250`
- **What:** `UncheckedTheme` repeats all 27 fields three times (the struct, the destructure, the rebuild) to validate on load. Its `type Error = &'static str` breaks the crate's "errors are enums" rule.
- **Direction:** use serde's validation idiom: `#[serde(remote = "Self")]` on the derive plus a hand-written `Deserialize` that calls it and then `faces_valid`. Put the failures in an error enum.
- **Root cause:** the surface is mirrored per field.

**UI-18 · complexity · low · public API** — 16 per-slot `pub fn stock(scheme)` in `src/theme/*.rs` (for example `button.rs:98`)
- **What:** each equals `Theme::stock(scheme).<slot>`. Every caller outside `src/theme/` is a test.
- **Direction:** keep `from_parts` and drop the per-slot `stock`, or accept it as documented sugar.

**UI-19 · complexity · low · no API change** — `src/ui/theme_transition.rs:21, 82-112, 158-163`
- **What:** the two `spares` buffers plus the swap logic in `follow` save one `Rc<Theme>` allocation per theme change, not per frame. That is a lot of state machine for an event that happens rarely.
- **Direction:** drop the spares, allocate a fresh `Rc` per run, and simplify the third-theme branch.
- **Root cause:** applying the per-frame allocation rule to per-event work.

**UI-20 · perf · low · no API change** — `src/ui/theme_transition.rs:144-149`
- **What:** `Theme::lerp(Theme::clone(&run.from), Theme::clone(&run.to), t)` copies two whole themes each transition frame (no heap involved), then overwrites the third.
- **Direction:** run `share_text_axes` once when the run starts, and lerp by reference into `run.shown`.

**UI-23 · api · low · public API** — `src/ui/mod.rs:1454` vs `:1460`
- **What:** `focus_first_within` is `pub` but `focus_end_within(…, TabEnd)` is crate-private, so an app can't land on the last stop (the "End opens on the last row" pattern).
- **Direction:** export `TabEnd` and the general form.

**UI-26 · idiom · low · no API change** — `src/ui/frame_runtime/mod.rs:116, 60`
- **What:** `transparency`, `dt` and `render_frame_id` are `pub(super)` fields and also have getters. `FrameCycle` writes `frame_runtime.transparency =` directly (`frame_cycle.rs:56`) while `Ui` reads through `transparency()`.
- **Direction:** pick one access path.

**UI-27 · bound · low · no API change** — `src/ui/frame_runtime/mod.rs:100` (`repaint_wakes`), `src/input_state/input_queue/mod.rs:23` (`pending`), `src/input_state/watches.rs:13-15`
- **What:** these grow without a stated limit. `Watches` pushes duplicates by design.
- **Direction:** state a cap and decide what happens when it is reached.

**UI-28 · idiom · low · no API change** — `src/theme/mod.rs:341`
- **What:** `faces_valid(&mut self)` only reads, but needs `&mut` because `TextSlots` has only a mutable walk.
- **Direction:** give the derive a read-only walk.

**WG-09 · complexity · `color_picker/mod.rs:430-439, 469-483`**
- **What's wrong:** the hex field hand-rolls a draft: `TextEdit::new(&mut self.state.hex)` plus "rewrite from the color when `ui.focus() != Some(hex_id)`" plus parse on `committed`.
- **Why it matters:** this is exactly what `TextEdit::buffered` (`text_edit/mod.rs:164`) exists for, per AGENTS' "A field can edit a draft of its own". It is two ways to do one thing, and they can drift (focus test vs `was_focused`, the Escape path).
- **Direction:** use `TextEdit::buffered(ui.fmt(..hex..))` and parse `committed_text`. That also drops `PickerState::hex`.
- Confidence: high · public API change: no · root cause: draft protocol re-implemented per widget (see Themes).

**WG-10 · complexity · `drag_value/mod.rs:33-46, 337-402` vs `editable_label/mod.rs:228-269`**
- **What's wrong:** both widgets swap a chip for an inline editor, with `start_editing`/`stop_editing`/`is_editing` under GTK names, but they implement it differently:
  - DragValue keeps its own `Editing { buffer, original, end }` and re-parses into the binding every frame.
  - EditableLabel uses `TextEdit::buffered`.
  - `stop_editing(Submitted)` means "set `end`" in one and "clear focus and let the blur commit" in the other.
- **Direction:** one swap-to-editor helper over `TextEdit::buffered`. DragValue would keep only `original` for its live-write mode, or parse only on commit, which is Qt's `keyboardTracking(false)`.
- Confidence: medium · public API change: no · root cause: draft protocol duplicated.

**WG-11 · complexity/api · `text.rs:85-144`, `text_edit/mod.rs:207-267`, `editable_label/mod.rs:86-144` + `with_overrides` 314-342**
- **What's wrong:** the same eight typography setters (`color`, `font_size`, `line_height_factor`, `family`, `weight`, `slant`, `bold`, `italic`) are hand-copied three times. EditableLabel then forwards them axis by axis through a 30-line `with_overrides`.
- **Direction:** add one `overrides(TextStyleOverrides)` setter, so a wrapper forwards in one call. Alternatively, generate the eight per-axis setters with one macro, since `const fn` cannot live in a trait.
- Confidence: high · public API change: yes (adds a setter) · root cause: const setters can't be trait-provided.

**WG-12 · complexity · `chrome`/`default_chrome` duplicated on nine types**
- **Where:** `panel/mod.rs:321-340`, `grid.rs:414-432`, `scroll/mod.rs:556-575`, `popup/mod.rs:228-248`, `modal/mod.rs:160-179`, `tooltip/mod.rs:208-228`, `block/mod.rs:202-220`, `popup_trigger/mod.rs:73-88`, `menu/mod.rs:272-288`.
- **What's wrong:** 18 identical bodies, kept in step by a test (`widgets/tests/mod.rs:93`).
- **Disagreement with AGENTS:** a test that polices duplication is the symptom; the canonical fix is one implementation. Since `const fn` rules out a trait, a declarative macro (or a `ChromeSlot` value type with both setters) is justified bulk reduction.
- Confidence: high · public API change: no (if done by macro) · root cause: const setters can't be trait-provided.

**WG-13 · complexity/api · `menu/mod.rs:249`, `menu_bar/mod.rs:370`, `combo_box/mod.rs:243`, `color_button/mod.rs:90`**
- **What's wrong:** `Menu::menu_id`, `MenuBar::menu_id` and `ComboBox::menu_id` are all `trigger.with("menu")`. `ColorButton::popup_id` is `PopupTrigger::popup_id`. These are pass-throughs that publish the same derivation four times.
- **Direction:** keep one canonical id function per overlay kind and document which one each wrapper uses.
- Confidence: high · public API change: yes · root cause: per-wrapper driving surface (AGENTS "State between frames") applied to pure functions too.

**WG-14 · complexity · `menu/mod.rs:115-126` vs `popup/popup_trigger/mod.rs:100-125`**
- **What's wrong:** `Menu::below(trigger)` is `PopupTrigger::on(trigger)` plus `menu_look` and `Layer::Menu`, with a separate `TriggerState<Menu>` row. Both toggle on click, hang below, land focus with `MenuLanding::First`, and expose open/close/is_open.
- **Direction:** make `Menu::below` a dressed `PopupTrigger` (one `TriggerFrame` path), or document why the two rows must differ.
- Confidence: medium · public API change: no · root cause: overlay-trigger protocol has parallel entry points.

**WG-15 · layering/AGENTS conflict · crate-private wrapper hooks**
- **Where:** `tabs/tab_strip.rs:176` (`dock_drag`), `:75` (`TabStripResponse::unscanned`), `:198`/`:220` (`scan`, `drop_slot`); `menu/mod.rs:185, 205` (`show_as`, `settle`); `popup_trigger/mod.rs:110` (`show_as`); `popup/mod.rs:211` (`menu_look`).
- **What's wrong:** AGENTS says "A wrapper finishes the widget it wraps through its public setters, never a hook only the wrapper calls". These are exactly such hooks.
- **Disagreement with AGENTS:** the code shows the rule is unworkable as written; DockView genuinely needs a strip protocol. Amend the rule to allow `pub(crate)` protocol entry points, or make the hooks public.
- Confidence: high · public API change: maybe · root cause: rule written for theming, applied to protocols.

**WG-16 · api/doc · `tabs/tabbed_view.rs:47-50, 76`**
- **What's wrong:** the doc says TabbedView "Mirrors ComboBox (same binding …)", but it binds `&mut usize`, whereas ComboBox binds `&'o S`. AGENTS refuses index binding for a pick.
- **Why it matters:** the index is defensible here, because close and reorder invalidate a reference, but then the doc is wrong and AGENTS needs a stated exception.
- **Direction:** fix the doc and add the exception to AGENTS.
- Confidence: high · public API change: no · root cause: rule/doc drift.

**WG-17 · idiom/naming · `toggle_row/mod.rs:36` vs `theme/toggle_kind.rs:5`**
- **What's wrong:** `pub(crate) trait ToggleKind` (impls Check/Pip/Knob) sits beside the public `enum palantir::ToggleKind {Checkbox, Radio, Switch}`. That is one concept represented twice under one name.
- **Direction:** rename the trait (e.g. `ToggleFace`), or key `slot`/`pill` off the public enum.
- Confidence: high · public API change: no · root cause: no single owner of "which toggle".

**WG-18 · idiom (illegal states) · several state rows**
- **Where:** `popup/mod.rs:182, 198` (`state::<Option<Landing>>`), `dock/dock_view.rs:531, 542` (`Option<TabMenuTarget<T>>`), `dock/tab_drag.rs:139-142`, `tabs/tab_strip.rs:89-92, 393-396`.
- **What's wrong:**
  - `Option<T>` is stored as the state type, although the map already expresses absence.
  - `TabDrag { tab: Option<T>, target: Option<DropTarget> }` allows a target with no tab.
  - `TabStrip` keeps two rows on one id (`CarriedChip`, `StripState`), each just an `Option<u64>`.
- **Direction:** store `Landing` and `TabMenuTarget` directly, removing rows on close. Make `TabDrag { tab, target: Option<_> }` live inside `Option`. Merge the strip rows.
- Confidence: high · public API change: no · root cause: none named.

**WG-22 · perf (minor) · `menu/mod.rs:190-192`, `text_edit/mod.rs:493-496`, `editor.rs:91`, `input_pass.rs:210`**
- **What's wrong:** `Menu::show_as` runs `dressed()` before it knows whether the menu is open: an `Rc` bump, `menu.panel.clone()` and five default setters. Every TextEdit calls `Menu::context(..).show` every frame and clones the `Clipboard` handle twice a frame. All of this is wasted while closed.
- **Direction:** check `TriggerState` open first and dress only when open; take the clipboard only on a matching action.
- Confidence: high on the code path, low on the cost · public API change: no · root cause: none named.

**WG-23 · idiom · `text_edit/mod.rs:123, 318-321`**
- **What's wrong:** `text: Option<Bound<'a>>` exists only so `show` can `.take().expect("taken once")`.
- **Direction:** destructure `self` in `show`, or pass `Bound` beside the builder, so the type holds a non-optional field.
- Confidence: high · public API change: no · root cause: none named.

**WG-24 · api (low) · `combo_box/mod.rs:236-263`, `dock/dock_view.rs:347`**
- **What's wrong:** associated "drive from outside" functions hang off dummy generic instances: `type Keyed = ComboBox<'static,'static,&'static str, fn(..)>` and `impl DockView<'static, ()>`.
- **Why it matters:** this works only through inference quirks and puts odd types in the docs.
- **Direction:** a non-generic marker type per widget for the `TriggerState` key and the id functions.
- Confidence: medium · public API change: yes · root cause: none named.

**WG-25 · api (low) · `dock_view.rs:242-244`, `tabbed_view.rs:125`, `tab_item.rs:306`**
- **What's wrong:** tab identity is a raw `u64` made from `WidgetId::from_hash(x).0`, which exposes WidgetId internals. `TabItem.key: u64` invites collisions with hand-picked numbers.
- **Direction:** a `TabKey` newtype from `impl Hash`.
- Confidence: medium · public API change: yes · root cause: none named.

### Batch 11

**HS-05 · complexity · `src/host/offscreen/mod.rs:35-45, 88-98, 292-368`; `src/host/harness/mod.rs:118-149, 328-365, 661-714`; `src/host/harness/harness_builder.rs:64-157`**
`OffscreenHost` and `Harness` duplicate each other:
- **Fields:** `core`, `driver`, `wake: Arc<WakeSignal>`, `system_scale`, `refresh_millihertz`, time, `window_commands`.
- **Methods (~13, near-identical docs):** `WINDOW`, `ui`, `ui_mut`, `advance`, `at`, `set_system_scale`, `set_refresh_millihertz`, three `set_system_*`, `wait_for_gpu`, `is_device_lost`, `recover`.
- **Builders:** both carry the same 9 fields and setters (system_scale, refresh, three system prefs, plus `HostOptions`).

AGENTS.md refuses "a one-window type for the two headless hosts, which splits the window by how many a host has". I think that is wrong: a private `HeadlessWindow { core, driver, wake, display facts, clock }` plus a shared settings struct is composition, not a second frame path, and the winit host stays multi-window. Sibling drift is already visible: Offscreen has no closure-sugar `frame`, and `window_commands` differs (HS-10).
*Direction:* extract the shared struct and builder settings; the public methods delegate. *Conf:* high. *API:* no (internal), or yes if the two merge. *Root cause:* host composition copied instead of owned once.

**HS-06 · complexity · `src/host/offscreen/mod.rs:375-388`; `src/host/harness/mod.rs:725-740`; `src/host/winit/window/mod.rs:354-368`**
"The frame step is the core's", yet each host hand-assembles the same sequence: take wake → `request_repaint` → build `FrameSpec` → `core.record` → drain window output → `core.submit` unless lost. Only the target and the drain sink differ.
*Direction:* one `HostCore::frame(driver, spec, app, depth, target: Option<RenderTarget>, commands)`, so each host keeps only its target and present. *Conf:* high. *API:* no. *Root cause:* as HS-05.

**HS-07 · complexity · `src/host/window_driver/mod.rs:108-148, 292-301`; `src/display/host_display.rs:174-197`; `src/display/frame_stamp.rs`; `src/host/core.rs:48, 154-165`**
Five overlapping per-frame descriptors:
- `FrameSpec{physical, format, gives_copy, system_scale, refresh_millihertz, now}`
- `TargetKey{physical, format, gives_copy}`, derived from FrameSpec field by field
- `HostDisplay{physical, system_scale, pixel_snap, refresh_millihertz}`
- `Display` = HostDisplay + user_scale
- `FrameStamp{display, time}`

`pixel_snap` is copied HostOptions → HostCore → DriverConfig → WindowDriver → HostDisplay → Display.
*Direction:* `FrameSpec { target: TargetKey, system_scale, refresh_millihertz, now }`. Drop `HostDisplay` (the driver builds `Display` with `ui.user_scale()`). Drop `HostCore.pixel_snap` and `DriverConfig`. *Conf:* high. *API:* no. *Root cause:* one fact, many shapes.

**HS-08 · complexity/illegal state · `src/host/harness/mod.rs:121-127, 317-326, 665-679, 733-758`; `src/host/harness/play.rs:409, 453`; `src/host/core.rs:46-47, 121, 135-150, 201, 216`**
"This harness paints" is stored three ways: `depth: FrameDepth`, `target: Option<OwnedTarget>`, and `core.device: Option<DeviceHalf>`. Each check reads a different one: `shot`/`capture` test `depth`, `drive` tests `target`, `wait_for_gpu`/`is_device_lost` test `core.paints()`. `physical` duplicates `target.size()`. `HostCore`'s `Option` plus `.expect(NO_DEVICE)` taxes the winit and offscreen hosts, which always have a device.
*Direction:* `enum HarnessPaint { Record, Compose, Gpu { target } }` holding the size. Split HostCore's CPU-only half by type rather than by `Option`. *Conf:* high. *API:* no. *Root cause:* derived state stored.

**HS-09 · layering · `src/host/offscreen/mod.rs:406-410`; `src/host/harness/mod.rs:695`; `src/host/harness/harness_builder.rs:220`**
The shared precondition `assert_runs_palantir` lives in the offscreen module, and the harness reaches into its sibling host for it. When the harness trips it, the panic reads "offscreen host device cannot run Palantir".
*Direction:* move it into `HostCore::new` / `HostCore::attach`, which every painting host runs. *Conf:* high. *API:* no. *Root cause:* HS-05.

**HS-23 · test · `src/host/winit/runtime.rs:423-472`; `src/host/winit/remote/tests.rs:258, 298`; `src/host/harness/tests/play.rs:372`**
Nothing tests `WinitRuntime` scheduling (`draw`, follow, `schedule`, `recover`); only pure helpers are covered (`earliest_wake`, `FramePresent::followed`), which is how HS-01 passes. Remote reads `Instant::now()` and `thread::sleep` directly, so its tests sleep for real (a 300 ms query, 4 ticks), against "no real sleep where the code takes a clock". *Conf:* high. *API:* no. *Root cause:* no seam for the clock or the event loop.

**HS-11 · complexity/two routes · `src/host/winit/system_preferences.rs:273-278, 346-357`; `src/host/offscreen/mod.rs:95-97, 158-182, 212-218, 295-312`; `src/host/harness/harness_builder.rs:68-70, 114-138, 243-249`; `src/ui/mod.rs:370-377, 420`**
System preferences take two routes. The winit host builds a `SystemPreferences` value and writes `resources.settings()` directly. The headless builders each carry three loose fields and go through `Ui::set_system_*`. Neither headless host can set the double-click interval that the winit host follows.
*Direction:* move `SystemPreferences` (minus the mundy read) out of `winit/`, take it in every builder, and apply it through one door. *Conf:* high. *API:* yes if the setters collapse. *Root cause:* HS-05.

**HS-12 · api · `src/host/harness/play.rs:350-437`; `src/host/winit/remote/mod.rs:164-229`; consumer `darkroom/src/driver/backend.rs:45-46`**
`Harness` and `Remote` offer the same verbs in different shapes: `play(app, actions)` vs `play(actions)`, `settle(app)` vs `settle()`, `shot(app, region)` vs `shot(region)`, `find(app, at)` vs `find(at)`. The only consumer re-unifies them in an enum (`Backend::Offscreen/Window`). The shared abstraction exists, but only crate-private (`driver::play::Stage`).
*Direction:* a public trait for "a window an agent drives", or matching signatures. *Conf:* med. *API:* yes. *Root cause:* HS-05.

**HS-13 · api · `src/host/winit/frame_summary.rs:8-24`; `src/host/winit/handle.rs:119-133`**
`FrameSummary` is public, but `sent_inputs` counts only events from the crate-private `send_input`, and `busy` is a Remote internal. darkroom uses `last_frame()` only as "is the window open" (`window_backend.rs:65`).
*Direction:* keep the summary crate-private and expose `is_open(window)` / `is_hidden(window)`. *Conf:* med. *API:* yes. *Root cause:* the Remote protocol leaks into the public surface.

**HS-15 · hygiene/bug · `Cargo.toml:90-92`; `src/clipboard/mod.rs:344-350`**
`arboard` is built with `default-features = false, features = ["image-data"]`, so without `wayland-data-control`. On a Wayland session the system clipboard either goes through XWayland's X11 selection or `Clipboard::new()` fails and the host quietly uses memory only. egui-winit uses `smithay-clipboard` with the window's Wayland display on Wayland.
*Direction:* enable `wayland-data-control`, or add a Wayland backend fed the display handle. *Conf:* med (features verified; runtime behaviour per arboard docs). *API:* no. *Root cause:* platform service chosen without the Wayland case.

**HS-17 · api/two routes · `src/display/system_scale/mod.rs:256-288`**
`SystemScale::new(f32) -> Option<Self>` and `TryFrom<f32>` (Result) are two public constructors for one rule. AGENTS.md says data from outside is "`Option` for one rule". *Direction:* keep one. *Conf:* high. *API:* yes. *Root cause:* one fact, many shapes.

**HS-18 · api/units · `src/window/window_config.rs:16-19`; `src/window/window_geometry.rs:7-8`; `src/host/harness/mod.rs:176`**
Logical window sizes are integer `UVec2` (`WindowConfig::inner_size`, `WindowGeometry::inner_size`), while every other logical length is f32 `Size`, and `UVec2` means *physical* in `Harness::new(size)`, `Display::physical` and `resize`. Nothing in the type tells the reader which unit. *Direction:* `Size` for logical, or a physical newtype. *Conf:* med. *API:* yes. *Root cause:* a unit not in the type.

**HS-19 · api · `src/host/winit/mod.rs:203-210`; `src/host/winit/config.rs:382-386`**
`WinitHostBuilder::power_preference(wgpu::PowerPreference)` is the one raw wgpu type in the host builders, and needs two `expect(disallowed_types)`. Elsewhere the surface wraps wgpu (`Gpu`, `TargetFormat`, `Vsync`). *Direction:* a palantir enum. *Conf:* med. *API:* yes. *Root cause:* inconsistent wrapping.

**HS-20 · complexity · `src/host/harness/mod.rs:872-887, 1035-1055`; `src/host/window_driver/mod.rs:715-729`**
Test-only pass-throughs duplicate public API:
- `Harness::layout_rect` is the same as public `arranged`.
- `focus`, `clear_focus`, `is_focus_within`, `set_focus` forward to `ui()`.
- `engines`, `engines_mut` and `parts` exist on both WindowDriver and Harness, and `parts()` subsumes the other two.

*Direction:* keep `parts()` and the public readers; drop the rest. *Conf:* high. *API:* no. *Root cause:* forwarding layers.

**HS-24 · hygiene · `Cargo.toml:213-216, 30-31`**
`[profile.bench] inherits = "release"` is redundant: bench already inherits release. `docs.rs all-features = false` leaves the public `golden` module off docs.rs. *Conf:* high. *API:* no.

**HS-25 · idiom/api · `src/diagnostics/gpu_pass_stats.rs:128-140`**
`Inner` stores `Option<u64>` nanoseconds and every getter converts to `Duration`; it could store `Option<Duration>`. Public `PipelineStats::compute_shader_invocations` is documented as always zero for palantir's passes. *Conf:* high. *API:* the field is. *Root cause:* a unit not in the type.

**HS-26 · api · `src/display/mod.rs:36-56, 123-128`**
`Display` is handed out read-only, yet has all-public mutable fields and a public `raster_eq`, which is the damage engine's comparison. *Direction:* getters, `raster_eq` crate-private. *Conf:* med. *API:* yes. *Root cause:* internal detail exported.

### Batch 12

**LT-02 · complexity · `drivers/grid/axis_scratch.rs:19-36`, `grid_track_store.rs:43-55, 164-168`, `grid_depth_stack.rs`, `axis_scratch.rs:114-128`** — conf: med-high · public API: no · root cause: per-depth scratch kept after a per-grid store was added
- **What's wrong:** `GridTrackStore` already gives every grid its own per-frame span (`max_pool`, `min_pool`, `sizes_pool`, `totals_pool`). Alongside it, `GridDepthStack` keeps a `Vec<GridScratch>` per nesting depth holding `sizes`, `offsets`, `extents` and `resolved` for the same tracks. Measure copies `sizes` into the store (`record_resolution`) and arrange copies them back (`resolve_or_reuse`), with `enter`/`exit` pairing and depth asserts.
- **Direction:** Keep every per-track array in the per-grid store, which nested grids cannot overwrite. The transient `hugs`/`flexible` lists become one shared scratch, and `GridDepthStack`, `GridScratch`, the copies and the pairing go away.

**LT-03 · complexity · `layout_scratch.rs:50-72`, `cache/node_arenas.rs:505-518`, `cache/mod.rs:45-59, 71-83`, `engine.rs:173-186`, `layout_scratch.rs:114-177`** — conf: med · public API: no · root cause: the snapshot is a copy of columns with no shared column type
- **What's wrong:** The per-node column set (desired, floor, stable_from, local, intrinsics, available, rect, scroll_content, text_spans) is declared four times: `LayoutScratch` and `PlaneLayout` fields, `NodeArenas`, `CaptureTreeInput` and `CachedSubtree`. It is destructured again in `clear`, `capture_tree` and `restore_after_cache_hit`. The doc comment itself lists the three places a new column must be added.
- **Direction:** One or two SoA column types (measure columns, arrange columns), each defined once, with `extend_from` and a range view. These would serve the scratch, the output and the snapshot, and `CaptureTreeInput`/`CachedSubtree` become borrows of them.

**LT-06 · complexity / perf · `text/system.rs:110-117, 336-362`, `intrinsic/intrinsic_pass.rs:401`, `layout/pass.rs:448`, `text/key.rs:42-44`** — conf: med · public API: no · root cause: overlapping text caches with misstated roles
- **What's wrong:**
  - The module doc says reuse rows are "for retention, not speed".
  - But `refresh` serves the stored `root` and `wrap.extent` without touching the shaper. That makes the row a measurement cache that outlives buffer eviction, which is why it must keep its own `text: String` copy.
  - The same extent then lives in three places: the row, `CacheEntry.extent`, and `MeasureSnapshot.text_shapes`.
  - Each measured leaf calls `refresh` twice per frame (intrinsic `root()`, then `measure()`): two hash probes and two O(len) byte compares. `key.rs` budgets one.
- **Direction:** State the row's real role. Then either thread the refreshed row from the intrinsic query into measure, or narrow the row to `{key, bound}`.

**LT-07 · complexity · `text/cosmic/shaped_buffer_cache.rs:159-165, 277-301` vs `cache_entry.rs:11-36`; `cosmic/geometry.rs:14-24, 70-77`** — conf: high · public API: no · root cause: value types copying each other's fields
- **What's wrong:** `ShapedBuffer` repeats five of `CacheEntry`'s fields field for field, and `insert` unpacks one into the other. Likewise `ShapedGeometry.size` duplicates `TextExtent.size`, which only a `debug_assert_eq` holds equal.
- **Direction:** `CacheEntry { shaped: ShapedBuffer, dies_at, ticket_seq }`, and have `GlyphInk::extent` return only the ink.

**LT-08 · complexity · `drivers/grid/mod.rs:111-115, 183`, `grid_ctx.rs:299-305`, `layout/depth_scratch.rs`** — conf: high · public API: no · root cause: no single depth-scratch idiom
- **What's wrong:** There are three ways to keep depth-shared scratch: `DepthScratch<T>` (stack, wrapstack), `GridDepthStack` (a Vec of per-depth structs), and `track_aggregator`, a hand-written mark/resize/truncate bump stack with the same semantics as `DepthScratch`.
- **Direction:** Make `track_aggregator` a `DepthScratch<f32>`; LT-02 removes the second idiom.

**LT-11 · complexity · `drivers/mod.rs:40-76, 106-147`, `layout/pass.rs:300`** — conf: med · public API: no · root cause: one fact declared twice
- **What's wrong:** "This driver reads outside its subtree" is declared twice: `ARRANGE_DEPENDS_ONLY_ON_SLOT` (read through `ReplayOp`) and `outside_input() -> Option<Size>`. A `debug_assert` holds them equal.
- **Direction:** Drop the const and `ReplayOp`. `arrange` already holds the `PlaneLayout`, so it can ask `OutsideInputOp{..}.dispatch(node, mode).is_none()`.

**LT-12 · perf · `layout/engine.rs:171-200`** — conf: med · public API: no · root cause: container text sits outside the snapshot
- **What's wrong:** `capture_tree` runs before the container-text pass, so container runs are never in the snapshot. Every layout run therefore re-measures every container run (a row lookup plus an O(len) compare), even inside subtrees that hit the cache and were replayed at the same inner width.
- **Direction:** For a node that was copy-replayed at the same width, restore its container spans from the previous frame.

**LT-13 · api / layout · `primitives/layout/placement.rs:48-53`, `anchor/mod.rs:181-194`** — conf: med · public API: no (behavior only) · root cause: measure is not bound to the chosen side
- **What's wrong:** An anchored root is measured against the whole surface and then flipped or shifted. A body larger than the room on either side is clamped on its preferred side over its own anchor, even when the opposite side has more room.
- **Direction:** Do as Floating UI's flip + size do: pick the side with the most room and bound the measure by that room, so a long menu scrolls instead of covering its trigger.

**LT-15 · api · `text/probe/mod.rs:73-91`** — conf: med · public API: yes · root cause: identity approximated by a hash
- **What's wrong:** `TextProbe::text_hash` and `hash_of` publish a 64-bit hash as "same string as last frame?". `TextEdit` keys its undo stack on it and hashes the whole buffer every frame. `key.rs` itself rejects hash-only identity ("No layer serves a collision").
- **Direction:** Give the app an exact identity (an edit counter or generation on the buffer) and retire the hash surface. This is an API change and needs your go-ahead.

**LT-16 · complexity · `layout/plane_input.rs`, `pass.rs:47-94, 183-191`, `intrinsic/intrinsic_pass.rs:214-227`** — conf: high · public API: no · root cause: a thin wrapper
- **What's wrong:** `PlaneInput` is built, destructured at once into `tree`/`interned_text` fields on both passes, and rebuilt in `LayoutPass::intrinsic_pass`.
- **Direction:** Have both passes hold `input: PlaneInput<'a>`.

**LT-17 · layering · `layout/engine.rs:35-38`, `intrinsic_pass.rs:237-239, 324-329, 401`, `pass.rs:1-3`** — conf: med · public API: no · root cause: the stated encapsulation isn't enforced
- **What's wrong:** `pass.rs` says drivers reach scratch only through accessors "so one driver can't grow a dependency on another's". But `LayoutEngine.scratch`, `.text` and `.cache` are `pub(crate)`, `IntrinsicPass` reaches `engine.scratch.grid.track_aggregator` directly, and `grid_mut` hands out the whole `GridCtx`.
- **Direction:** Make the fields private to `layout`, or drop the claim.

**LT-18 · idiom · `layout/measured.rs:7-12`** — conf: med · public API: no · root cause: two coordinate spaces in one type
- **What's wrong:** `Measured` means a driver's pre-padding inner answer in one place and a node's margin-inclusive outer answer in another; only prose tells them apart. The cache returns the outer one as `hit.root`.
- **Direction:** Use a newtype per space so mixing them is a type error.

**LT-19 · idiom · `intrinsic/intrinsic_query.rs:127-184`** — conf: low · public API: no · root cause: a set encoded as an Option
- **What's wrong:** `single_req: Option<LenReq>`, where `None` means both, encodes a two-member set; `retain` and `includes` decode it.
- **Direction:** A two-flag `flag_set!` (MIN|MAX) would read directly.

**LT-20 · test · `layout/tests/*`, `text/mono.rs:52-85`** — conf: med · public API: no · root cause: the model and production differ
- **What's wrong:** Tests are exact-valued and good. But layout tests run on the mono metric, whose truncation (`(first * glyph_w).min(max)`) skips the ellipsis reservation and cluster cut that cosmic applies. Layout-to-truncation interplay is therefore only checked against a model.
- **Missing coverage:** a Fill grid inside a Hug parent (LT-01) and anchored overflow (LT-13).

### Batch 13

**RD-01 · layering · `image_registry/mod.rs:24`, `image_store.rs:5`, `image_handle.rs:4`**
- **What:** `use crate::renderer::error::ImageTooLarge;` — `image_registry` sits left of `renderer` in the module order, so importing from it breaks the AGENTS.md rule that a module imports only modules to its left. `renderer/error.rs` holds nothing but this one image error, and `TextureLimit::accepts` returns it.
- **Why it matters:** the order exists to prevent cycles; this edge points the wrong way.
- **Direction:** move `ImageTooLarge` (and probably `TextureLimit`) into `image_registry` or `primitives`. The `palantir::ImageTooLarge` re-export keeps its path.
- **Confidence:** high. **Public API:** no. **Root cause:** a type placed by who checks it, not by who owns it.

**RD-02 · complexity · `frontend/composer/rect_grid/mod.rs:17`, `higher_kind_rects.rs:21`, `paint_order.rs:394-412`, `occlusion_pruner.rs:36`**
- **What:** `RectGrid { tiles: TileIndex, rects: Vec<URect>, union }` and `TierRects { rects, union, tiles: TileIndex, indexed, unshaped }` are the same structure. They differ only in when they index: eagerly on push, or lazily on query past `LINEAR_RECTS`. A third variant is `closed_grid`, filled lazily from pending batches, and `OcclusionPruner` builds its index lazily past 16 entries.
- **Why it matters:** there are four indexing policies over one `TileIndex` and seven grow-only `TileIndex` instances. Each policy was tuned separately and each needs its own tests.
- **Direction:** one "rect set" type with lazy indexing past a threshold. `HigherKindRects` becomes `[RectSet; COUNT]` plus a union, and the text grids use the same type.
- **Confidence:** med-high. **Public API:** no. **Root cause:** spatial index grown per call site.

**RD-05 · complexity · `frontend/payload/brush_source.rs:30`, `resolved_gradient.rs`, `gpu_fill.rs`, `encoder/plane_ctx.rs:117-120`**
- **What:** a fill passes through four types: `ShapeBrush` → `BrushSource::{Solid, Gradient(ResolvedGradient)}` → `GpuFill` + `FillAxis`. `BrushSource` lives only between `GradientPass::source` and `DrawQuadPayload::rect`, plus a match on solid-vs-gradient in `paint_pane`.
- **Why it matters:** two intermediate types for one lowering step.
- **Direction:** have the resolver return the lanes directly (`GpuFill` plus `FillAxis`), with `kind.is_gradient()` answering the pane-tint question.
- **Confidence:** med. **Public API:** no. **Root cause:** intermediate representations stacked between record and row.

**RD-07 · design (undocumented) · `frontend/paint_sink/mod.rs:186-254`**
- **What:** the payload layer is a full intermediate representation: `PaintSink` plus `PaintPayload` double dispatch, 8 `Draw*Payload` types, `Spin`, `StrokeBounds`, `GpuFill` and `BrushSource`. It sits between `ShapeRecord` and the `RenderBuffer` rows. Its only production sink is `ComposeSession`; the second consumer is `PaintCapture`, used by tests and benches.
- **Why it matters:** AGENTS.md records no decision for this representation, as it does elsewhere. egui and vello lower shapes directly.
- **Direction:** record the decision (capture and replay for composer tests and benches), or fold the payloads into the lowering.
- **Confidence:** med. **Public API:** no. **Root cause:** a test seam promoted to architecture.

**RD-10 · complexity · `render_buffer/per_group_batch.rs`**
- **What:** `PerGroupBatch` is a trait with a single getter, `last_group()`. Its two impls are `TextBatch` and `GroupBatch`, and two generic helpers in `gpu/frame/schedule` use it.
- **Direction:** embed a `GroupBatch` in `TextBatch` (`{ batch: GroupBatch, scissor, rounded_clips }`), or pass a projection.
- **Confidence:** med. **Public API:** no. **Root cause:** a trait used as a field accessor.

**RD-11 · layering · `renderer/render_owner_id.rs`**
- **What:** `renderer` never uses `RenderOwnerId` or `RenderOwners`. Only `gpu` (the `Backdrops` cache), `host` and `ui::resources` do.
- **Direction:** move it into `gpu`; both of its users sit right of `gpu` in the order.
- **Confidence:** high. **Public API:** no. **Root cause:** placement.

**RD-12 · idiom · `render_plan.rs:25`**
- **What:** `from_damage(damage: Option<Damage>, ..) -> Option<Self>` takes an `Option` and returns early on it, against "push ifs up".
- **Direction:** the caller (`ui/frame_cycle.rs:170`) maps the option, and the constructor takes a `Damage`.
- **Confidence:** high. **Public API:** no. **Root cause:** a convenience constructor.

**RD-13 · doc/precision · `render_plan.rs:16-22`**
- **What:** `cull_margin` returns `(AA_PADDING + 1.0) / scale`. The `+1` is the up-to-one-pixel slack from `WholePixels` rounding in `logical_rect_to_physical_scissor`, and nothing says so. The constant also belongs to the scissors, not the plan.
- **Direction:** name the rounding slack, and consider moving the constant to `repaint_scissors`.
- **Confidence:** high. **Public API:** no. **Root cause:** a derived constant split across modules.

**RD-14 · bug (debug-only) · `encoder/collision_overlay.rs:323`**
- **What:** the magenta outlines are drawn on partial frames with no damage entry. Once the collision is fixed, the part of the outline outside the damage stays on screen until the next full frame.
- **Direction:** force a full frame while `forest.collisions` changes, or feed the outlines into damage.
- **Confidence:** med. **Public API:** no. **Root cause:** paint that bypasses damage.

**RD-15 · idiom · `composer/session.rs:96-104`**
- **What:** finishing the frame (closing the last batch, flushing the last group) is done in `Drop`, and `Frontend::build` calls `drop(sink)` explicitly. A `let _ = composer.begin(..)` would close it at once.
- **Direction:** an explicit `finish(self)`, the canonical form for an end-of-session step that order depends on.
- **Confidence:** med. **Public API:** no. **Root cause:** finalization hidden in a destructor.

**RD-16 · perf · `composer/session.rs:136-141`**
- **What:** each rounded clip push copies its whole ancestor chain into `rounded_clips` with `extend_from_within`. The pool grows by the sum of depths, bounded at 255 per chain.
- **Direction:** WebRender uses clip-chain nodes with parent links. Here the backend reads contiguous spans, so this is acceptable unless deep rounded nesting shows up in a profile.
- **Confidence:** high (on the cost). **Public API:** no. **Root cause:** flat pool chosen for the backend.

**RD-17 · complexity · `gradient_atlas/mod.rs:68,92`**
- **What:** every resident `ColorRamp` (about 42 B) is stored twice: as the `index` key and as `RowSlot.key`. The slot copy exists only so eviction can remove the index entry.
- **Direction:** a `hashbrown::HashTable<u32>` that hashes through `slots`.
- **Confidence:** med. **Public API:** no. **Root cause:** a map plus a reverse map.

**RD-18 · doc · `gradient_atlas/mod.rs:53`**
- **What:** "32 MB" against the AGENTS.md MiB rule. 16384 rows × 2048 B is 32 MiB.
- **Confidence:** high. **Public API:** no. **Root cause:** naming rule.

**RD-21 · test · `encoder/tests/damage_cull.rs:43`**
- **What:** the filter tests assert `count_draw_rects` only. The user's rule is to check which elements survive a filter.
- **Direction:** assert the surviving rects or ids.
- **Confidence:** high. **Public API:** no. **Root cause:** tests by count.

**Already logged:** `PaneBreak.since` is a single bounding box, so an L-shaped layout breaks the pass needlessly (`composer/pane_break.rs:191`). `.notes/PERF.md` shelves it. `.notes/ISSUES.md` and `.notes/TODO.md` hold nothing else in this scope.

**SC-06 · complexity/idiom · `src/scene/node/ident.rs:20-41`, `src/scene/forest.rs:119-124`.** `Ident` holds both a recipe and its result.
- `Ident` mixes three recipes (`Auto`, `Hash`, `Verbatim`) with the result (`Resolved(ResolvedId)`).
- `raw_id` has an `unreachable!` arm for `Resolved`, and `Forest::widget_id`'s `_ =>` arm sends `Resolved` into that panic. `seen_ids::Recipe` stores an `Ident` that can only be `Auto` or `Hash`.
- Direction: split into an `IdRecipe` enum and a widget-side `enum { Recipe(IdRecipe), Resolved(ResolvedId) }`, so the forest only ever receives a recipe.
- Confidence: high. Public API: no. Root cause: two states in one enum (an illegal state that can be represented).

**SC-07 · complexity · `src/shape/lower/mod.rs:94-108`, `src/shape/rect.rs:136-141`.** The reduced-transparency rule is written twice.
- "A pane under reduced transparency paints its solid infill and no filter" is implemented in both places, with the same comment, and the two differ: chrome also gates on `!has_nan()`.
- The rule's input, `RecordStore.transparency`, is lowering policy stored on a payload store.
- Direction: one helper, `(fill, backdrop) = lower::pane_fill(filter, fill, reduction)`, called by both. Pass the reduction to it rather than storing it on the store.
- Confidence: high. Public API: no. Root cause: chrome lowering and shape lowering grew side by side.

**SC-08 · complexity · `src/scene/forest.rs:53-60,180-186`, `src/scene/tree/mod.rs:104-112`.** Two structs carry the same chrome and ring pair.
- `NodeChrome { chrome: Option<&Chrome>, ring }` is converted into `ChromeInput { chrome: &Chrome, ring, store }`, and an absent chrome is replaced by `&Chrome::NONE` along the way.
- Direction: pass `NodeChrome` and `&mut RecordStore` to `Tree::open_node` and decide row presence once there.
- Confidence: high. Public API: no. Root cause: a forwarding layer.

**SC-09 · complexity · `src/scene/node/node_cols.rs`, `src/scene/node/mod.rs:305-331`.** `NodeCols` is a one-hop carrier.
- It exists only between `Node::cols` and `Tree::open_node`, which then mutates it: clip downgrade, disabled bit, padding fold.
- Direction: let `Tree::open_node` take `&Node` plus the id, or have `Node::cols` return the `NodeRecord` fields and side rows directly.
- Confidence: med. Public API: no. Root cause: three representations of one node (`Node` → `NodeCols` → `NodeRecord` plus side rows).

**SC-10 · complexity · `src/scene/tree/mod.rs:490-517` vs `src/cascade/engine.rs:393-394`.** Inherited disabled and visible are computed twice.
- At record time (`OpenFrame.ancestor_or_self_disabled`, `effectively_visible`) and again in the cascade walk.
- The record-time values gate `PaneEntry` and `PaintAnimationEntry`; the cascade's gate paint spans. Nothing ties the two except the debug paint-row assert.
- Owner-disabled crosses planes through `set_disabled` on the root, but invisibility has no matching path. Today both sides agree; nothing keeps them agreeing.
- Direction: share one `fn inherit(parent, attrs, vis)` used by both, or derive the record-time gates from the cascade's result.
- Confidence: med. Public API: no. Root cause: record runs one frame ahead of the cascade.

**SC-12 · idiom · `src/shape/paint/lowered_shadow.rs:305-321`, `src/shape/paint/shape_stroke.rs:160-178`.** Two `From` impls lose data.
- `From<Shadow> for LoweredShadow` saturates to f16, and `From<Stroke> for ShapeStroke` quantizes to f16 and collapses every invisible stroke to `NONE`.
- std's guidance is that `From` is lossless and value-preserving; darkroom's rule is that a lossy conversion is a named method.
- Direction: name them, for example `LoweredShadow::lower(shadow)` and `ShapeStroke::normalized_from`.
- Confidence: high. Public API: no. Root cause: lowering is written as conversions.

**SC-13 · complexity · `src/shape/record_store/mod.rs:112-161`.** `RecordStore` forwards to its own fields.
- `intern_str`, `intern_fmt`, `reuse`, `text`, `record_text` and `intern_gradient` are one-line forwards to `TextStore` and `RecordedGradients`. `intern_str` and `reuse` are private forwards to forwards.
- Direction: expose `text` and `gradients` as `pub(crate)` fields with their own methods (they are already types with invariants), or inline the private hops.
- Confidence: high. Public API: no. Root cause: a forwarding layer.

**SC-14 · complexity · `src/shape/record_store/mod.rs:37`, `primitives/geometry/mesh/mod.rs:52-64`.** The mesh arena is a public `Mesh`.
- `stage_mesh` pushes into its fields directly, so `max_idx`, `cached_hash` and `cached_bbox` are stale by design ("arena meshes never hash").
- Direction: two `Vec`s (`mesh_vertices`, `mesh_idxs`), as the polyline payloads are stored.
- Confidence: high. Public API: no. Root cause: an authoring type reused as storage. The per-frame vertex copy itself is already logged ("mesh once registrations", TODO).

**SC-15 · complexity · `src/scene/tree/mod.rs:148-157`, `src/scene/tree/rollup_inputs.rs:40-58`.** Clearing one struct happens in two places.
- `Shapes.records` is cleared in `Tree::pre_record`, while `Shapes.hashes` is swapped out and cleared in `RollupInputs::take_from`. Its siblings `paint_animations`, `panes` and `roots` are cleared in a third list.
- Direction: one `Tree::pre_record` that destructures `Self` exhaustively and states for every field whether it is cleared, swapped or kept.
- Confidence: med. Public API: no. Root cause: the cross-frame rollup reuse was added beside the clear.

**SC-16 · idiom · `shape/record/mod.rs:58,83`, `shape/paint/shape_brush.rs:15`, `shape/paint/curve_ramp.rs`, `primitives/text/recorded_text.rs:11`.** Content hashes use two types.
- They are bare `u64` in records (`content_hash`, `hash`) beside the `ContentHash` newtype used elsewhere.
- Direction: use `ContentHash` everywhere a content hash is stored.
- Confidence: high. Low impact. Public API: no.

**SC-17 · layering · `src/shape/paint/spectrum_tables/`.** GPU-only tables live in `shape`.
- The CIE observer tables are read only by `gpu` (`gpu/backdrop/backdrops`, `quad_pipeline`). Lowering needs only the `SPECTRUM_VIOLET` constant, through `LensOptics::violet_index`.
- Direction: move the table builder to `gpu` and keep the constant beside `LensOptics`.
- Confidence: med. Public API: no.

### Batch 14

**PR-05 · complexity/api · `paint/color/hsv.rs:147`, `okhsv/mod.rs:23`, `color_coords/mod.rs:115-219`, `color_model.rs:44-102`**
`Hsv` and `Okhsv` are two public structs with identical fields `{h, s, v}`, the same `new`/`to_color`/`from_color(color, fallback_hue)`. `ColorCoords` is an enum of the two in which every method (`hue`, `saturation`, `value`, `with_*` ×3) matches both arms to reach the same field. `ColorModel` repeats the tag. Parallel enum pairs follow below it: `HueSlice`/`HueSliceKind`, `HueBlock`/`HueBlockKind`, `HsvColumn`/`OkhsvColumn`, `HsvBlock`/`OkhsvBlock`.
Fix: make `ColorCoords { model: ColorModel, h, s, v }` (or one triple plus the model), dispatch once per conversion, and drop the per-accessor matches. Confidence med. Public API change. Root cause: the model tag is duplicated into the type structure.

**PR-06 · api/complexity · `primitives/identity/icon_handle.rs:33`, API surface lines 418/774/941/1099**
A float extent has two public spellings. `Rect.size`, `Sizing` and `Size::INF` use `Size`, while `IconHandle::view_box() -> Vec2` (documented as "the size to give its node"), `IconDefinition::view_box`, `ComboBoxTheme/ExpanderTheme::arrow_size: Vec2` and `RadialGeometry::radius: Vec2` use `Vec2`. The int side differs again: `URect.size: UVec2`, not a `USize`.
Fix: use `Size` wherever the value is an extent, and keep `Vec2` for positions and offsets (euclid/kurbo separate `Size` from `Vector` the same way). Confidence med. Public API change. Root cause: no rule for extent versus vector.

**PR-07 · complexity · `paint/color/oklab.rs:84-105`**
The hand-rolled `cbrt` (bit-pattern seed plus three f64 Halley steps) is justified as platform-deterministic, but its own test's oracle is `f64::from(x).cbrt() as f32`. That expression has the same determinism property: f64 libm differs by at most an ulp across platforms, and only near f32 rounding ties survive the cast, which is exactly the caveat the custom doc admits. The comparison in the doc is against `f32::cbrt`, which is the wrong baseline.
Fix: replace it with `(f64::from(x).cbrt()) as f32`, unless a measured perf need is shown. Confidence med-low. No API change. Root cause: the bespoke version was argued against the wrong alternative.

**PR-09 · layering · `src/primitives/packed/serde/checked/mod.rs`**
The file-scalar validators for every theme field (length, font_size, angle, refractive_index, fraction, padding…) live under `packed::serde`. They have nothing to do with the packed encodings: they are `domain` applied to serde. The optional variants also repeat the `read` pattern by hand (`optional_factor`, `optional_lane_factor`, `optional_font_size`, `lane_length`).
Fix: move them beside `domain` (for example `math::domain::serde`) and build the `Option`/lane variants from one generic reader. Confidence med. No API change. Root cause: validation spread across modules by where it was first needed.

**PR-10 · complexity · `primitives/geometry/corners/mod.rs:101-108`, `spacing/mod.rs:374-381`, `packed/serde/mod.rs:177-191`**
`Corners::checked` and `Spacing::from_array` are the same checked pack and differ only in the lane predicate, which `LaneCodec::lane_is_valid` already names. `LaneCodec::to_lane_array` is a pass-through to the macro's `to_array`. `from_lane_array` validates for `Spacing` but not for `Corners`, an inconsistency.
Fix: generate the checked constructor and the `LaneCodec` plumbing in `f16x4_lanes!` from the codec's predicate. Confidence med. No API change. Root cause: two hand copies beside a macro that already generates the rest.

**PR-11 · api · `paint/brush/gradient/conic_geometry.rs:29` vs `color_ramp.rs:243`**
`ConicGradient::new` defaults to `Interpolation::Linear`, while `ColorRamp::new` and `Interpolation::default()` give Oklab. A ramp built by hand into a conic `Gradient { ramp: ColorRamp::new(..), .. }` therefore interpolates differently from `ConicGradient::new`. CSS Color 4 uses Oklab for every gradient, and the code comment itself says "An `Oklch{hue}` interpolation would be the right default". `sealed::Geometry::DEFAULT_INTERPOLATION` exists only for this deviation, and `from_stops` duplicates `ColorRamp::new`.
Fix: one default (Oklab), with `from_stops = ColorRamp::new(stops)`, and drop the constant. Confidence med. Behavior change. Root cause: a per-kind default with a single deviation.

**PR-12 · idiom · `paint/brush/gradient/mod.rs:95`**
`Gradient<G>` derives `Eq`, which yields `impl<G: Eq> Eq`. No geometry is `Eq` (all hold `f32`), so the derive is dead. Fix: remove it. Confidence high. Public API: invisible. Root cause: derive by habit.

**PR-13 · complexity · `common/idx16.rs:148` vs `common/non_max_u32.rs:229`**
Two niche-index encodings for one idea: `Idx16` stores the index plus one in a `NonZeroU16`, and `NonMaxU32` stores the inverted value in a `NonZeroU32`. Both make zero mean `None`. They also have asymmetric `to_raw`: a method on `Idx16`, a free fn on `NonMaxU32`.
Fix: one encoding and one API shape, generic or a pair from one macro. Confidence med. No API change. Root cause: added independently.

**PR-14 · complexity · `animation/animation_spec/mod.rs:126,135`**
`is_instant(self)` and `is_snap(&self)` are the same predicate, the second only to fit serde's `fn(&T)`. Fix: make `is_instant(&self)` (it is `Copy`) and drop `is_snap`. Confidence high. Public API: `is_instant` re-signed. Root cause: serde's signature drove a second name.

**PR-15 · complexity · `common/hash/mod.rs:17-19` vs `animation/animation_key.rs:17`**
The hash module says "use it instead of `FxHasher::default()`", yet `AnimationKey::of` uses `FxHasher` directly. Fix: route it through `common::hash::Hasher`, or a `clippy.toml` disallow. Confidence high. No API change. Root cause: a convention with no enforcement.

**PR-16 · idiom/docs · `paint/color/mod.rs:372-378`**
`Display for RgbaF32` carries two overlapping doc paragraphs that state the same wire-format rule twice ("Wire format: CSS hex…" and "The form FromStr reads back…"). Fix: keep one. Confidence high. No API change.

**PR-17 · layering · `palantir-anim-derive/src/lib.rs:61-67,128-150`**
The public derive crate exports `TextSlots`, whose expansion names `crate::theme::TextSlots`/`ThemeText`. It compiles only inside palantir, so any other user gets a broken expansion. Its sibling derives use `::palantir::widget::…`.
Fix: move palantir-private derives behind a doc-hidden path, or a separate internal macro crate. Confidence med. Public surface of the derive crate. Root cause: private tooling shipped in a public crate.

**PR-18 · complexity · `primitives/math/domain/mod.rs:128,240` and `106,278`**
`is_factor` has exactly `is_length`'s body (`v >= 0 && v <= MAX`), and `is_angle` has exactly `is_offset`'s (finite), each pair with its own checker and rule string. `refractive_index` is a public kind with a single caller (`Lens`). Every kind is public API under `widget::domain`. A semantic alias may be intended, but the module then carries 18 kinds × (predicate + checker + RULE) plus vec2 and f64 twins.
Fix: keep the names if the semantics matter, but define the aliases as delegations, and consider making single-use kinds private. Confidence med. Possible public API removal. Root cause: validation that grows a kind per field.

**PR-19 · api · `common/span/mod.rs:230`, `input/ime_preedit.rs:11`, `icons/icon_table.rs:22`**
`Span { start: u32, len: u32 }`, an internal arena handle, is public as `ImePreedit::cursor: Option<Span>`, where std's `Range<usize>` is the idiomatic byte range (winit hands `(usize, usize)`). `Span::range()` computes `start + len as usize` with no overflow guard, which can wrap on 32-bit targets.
Fix: a `Range<usize>` in the IME API, keeping `Span` for the icon blob's const data. Confidence med-low. Public API change. Root cause: an internal storage type leaked as a value type.

**PR-20 · complexity · `primitives/geometry/arrow.rs:234`, `packed/fill_axis.rs:242`**
`Arrow { size }` is a one-field struct whose two methods could be `arrow::points(size)`. `FillAxis` is a newtype over `F16x4` with only pass-throughs (`from_lanes`, `lanes`, `scaled_saturating`, `From<F16x4>`) and no invariant. That is acceptable as a type tag, but it is a pure forwarder. Confidence low. No API change.

**PR-21 · layering · `paint/color/color_model.rs:26`**
`ColorModel::label()` returns English UI text ("Okhsv", "HSV") from a primitive. Fix: move it to the picker widget or theme. Confidence low. Public API change.

**PR-22 · idiom · `paint/image/mod.rs:30`, `image/icon_fit.rs:418`**
The enum variants `ImageFit::None` and `IconFit::None` shadow `Option::None` under a glob import or in match arms. The CSS name is "none", but Rust APIs usually spell it `Natural` or `Intrinsic`. Confidence low. Public API change.

**PR-23 · idiom · `paint/color/rgba_f16.rs:68` vs `packed/half_simd/mod.rs:45`**
`RgbaF16` derives only `PartialEq`, while the `F16x4` it wraps is `Eq` and its sibling wrappers (`Corners`, `Spacing`, `FillAxis`) derive `Eq`. Confidence low. No public API impact.

**GA-14 — api — `device/gpu_frame_ctx.rs:9`, `device/gpu_init_ctx.rs:8`**
- **What:** `GpuFrameCtx` and `GpuInitCtx` are public structs with all-public fields and no `#[non_exhaustive]`.
- **Why it matters:** Apps can construct them, and every added field (likely — e.g. a `text` handle in the frame context) is a breaking change. Context structs handed to callbacks are usually `#[non_exhaustive]`.
- **Direction:** Add `#[non_exhaustive]` (this needs a go-ahead).
- **Confidence:** high · **Public API:** yes · **Root cause:** public plain data used where a sealed context is meant.

**UI-25 · idiom · low · no API change** — `src/driver/error.rs:22-27`
- **What:** `LocateError::Ambiguous { matches: Vec<String> }` holds pre-formatted "n: at (x, y)" strings, so a caller can't read the points.
- **Direction:** hold `Vec<Found>` and format in `Display`.
