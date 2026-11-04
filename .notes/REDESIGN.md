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
  `FrameStamp`. `pixel_snap` passes through five hops (HS-07) ✔.
- **Whether the harness paints:** `depth`, `target` and `core.device` (HS-08) ✔.
- **Layout columns:** declared four times (LT-03) ✔. Grid tracks are in a per-grid store and in a
  per-depth stack (LT-02) ✔.
- **Node fields:** written in `Configure`, `Configurator`, `ThemeDefaults`, `authored_*`, and
  `Node::set`/`fill`. About 100 methods (UI-07).
- **Other copies:** `Hsv` and `Okhsv` (PR-05) ✔. `OffscreenHost` and `Harness` (HS-05) ✔.
  `Singletons` beside `StateMap` (UI-06). Two ways to close an overlay (UI-05).

### R2. Hand-written keys and hashes

A key that lists its inputs by hand, with no exhaustive destructure, misses an input. This
is the source of the silent correctness bugs:

- The layout hash leaves out the `Stack`/`WrapStack` axis ✔ (SC-01). The measure cache then
  replays the old arrangement after an axis change.
- `CascadeKey` leaves out the root layer (SC-03).
- The exact rect hash and the quantized per-node hash disagree (SC-19).
- A hash fold stands in for an exact identity in `MeasureSnapshot` (LT-04) ✔ and in
  `TextProbe::text_hash` (LT-15) ✔.

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
- **Spatial index:** four indexing policies over `TileIndex` (RD-02) ✔.
- **Depth scratch:** three idioms (LT-08) ✔.

### R4. Bools and Options where an enum would hold the invariant

- **Frame shape:** `FrameShape` holds four bools, and `submit` takes its resources back out
  of `Option`s with `expect`/`unreachable!` (GA-05, GA-08) ✔.
- **Overlay spec:** `OverlaySpec` asserts a link between `Backdrop` and `ClickOutside` at run
  time (UI-09).
- **Mixed states:**
  - `Ident` holds a recipe and its result in one enum (SC-06) ✔.
  - `activate()` writes `Up` beside `Drag::Active` (UI-12).
  - `TabDrag { tab: Option, target: Option }` (WG-18).
  - `HostCore` holds `Option<DeviceHalf>` (HS-08) ✔.
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
| Keyboard | Scroll takes no keys and does not reveal focus (WG-06). Menubar Down/Up do nothing (WG-07). Radio groups (WG-08) ✔. Shift+Space is swallowed (UI-21). |
| Pointer | Tooltip does not hide on a press or Escape (WG-05). Splitter drops the grab offset (WG-21). Dock shows a drop it refuses (WG-04). |
| Platform | Wayland clipboard (HS-15). Tiler detection on DXGI and GLES (GA-01) ✔. |

### R11. Test seams in production state

- `PaneWalk.on` ✔ (RD-04).
- `shadow_grid: bool` ✔ (GB-20).
- `#[cfg(test)]` stencil usage ✔ (GA-07).
- `WgpuBackend.pass_traffic` mirror ✔ (GA-11).
- The payload IR, whose second consumer is only for tests (RD-07) ✔.

## Decisions in AGENTS.md the reviewers dispute

Each item needs your answer before its batch starts.

1. Settled: the rule allows `pub(crate)` protocol entry points between crate widgets (WG-15).
2. Settled: a pick over a list the widget reorders binds the index (WG-16).
3. **"A `Response` borrows the `Ui`"** (UI-10). The borrow makes the owned twins
   `ResponseSnapshot` and `ValueEdges`, and puts `'a` on every response type. No measurement
   supports the lazy probe. The proposal is to measure an eager `Response` first.

## Batches

The order is by impact: user-visible bugs and panics first, then perf with wide reach, then
the reduction of complexity. Each batch stays inside one system, so one review covers it. A
batch marked **API** changes the public surface and needs your go-ahead (AGENTS.md "Public
API").

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
  Settle decision 1 (WG-15).
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
- Decision 3: measure an eager `Response`, then decide (UI-10).
- State the bound of the input queue and decide what a full one does (the rest of UI-27).

Done in this batch: decisions 1 and 2 (WG-15, WG-16), UI-20, UI-23, UI-26, UI-28, WG-17,
WG-22, WG-23, WG-25, and the wake and watch bounds of UI-27. UI-19 is kept as it is: the
`theme_transition_alloc_free` test holds a transition to the buffers it keeps.

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
| 10 | UI-05 to UI-10, UI-14, UI-16, UI-17, UI-18, UI-19, UI-20, UI-23, UI-26 to UI-28, WG-09 to WG-18, WG-22 to WG-25 |

These findings are kept as they are, with no change: SC-20 (the `SeenIds` size is justified
by a measurement) and RD-16 (the copies of the rounded-clip chain are accepted unless a
profile shows them). `.notes/ISSUES.md` already logs the `Staging::abort` race.

## Findings in full

Each open finding as its reviewer wrote it, grouped by the batch that takes it. A batch reads
its own findings here; the line numbers are from the review and drift as the code moves.

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

**UI-27 · bound · low · no API change** — `src/ui/frame_runtime/mod.rs:100` (`repaint_wakes`), `src/input_state/input_queue/mod.rs:23` (`pending`), `src/input_state/watches.rs:13-15`
- **What:** these grow without a stated limit. `Watches` pushes duplicates by design.
- **Direction:** state a cap and decide what happens when it is reached.

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

**WG-18 · idiom (illegal states) · several state rows**
- **Where:** `popup/mod.rs:182, 198` (`state::<Option<Landing>>`), `dock/dock_view.rs:531, 542` (`Option<TabMenuTarget<T>>`), `dock/tab_drag.rs:139-142`, `tabs/tab_strip.rs:89-92, 393-396`.
- **What's wrong:**
  - `Option<T>` is stored as the state type, although the map already expresses absence.
  - `TabDrag { tab: Option<T>, target: Option<DropTarget> }` allows a target with no tab.
  - `TabStrip` keeps two rows on one id (`CarriedChip`, `StripState`), each just an `Option<u64>`.
- **Direction:** store `Landing` and `TabMenuTarget` directly, removing rows on close. Make `TabDrag { tab, target: Option<_> }` live inside `Option`. Merge the strip rows.
- Confidence: high · public API change: no · root cause: none named.

**WG-24 · api (low) · `combo_box/mod.rs:236-263`, `dock/dock_view.rs:347`**
- **What's wrong:** associated "drive from outside" functions hang off dummy generic instances: `type Keyed = ComboBox<'static,'static,&'static str, fn(..)>` and `impl DockView<'static, ()>`.
- **Why it matters:** this works only through inference quirks and puts odd types in the docs.
- **Direction:** a non-generic marker type per widget for the `TriggerState` key and the id functions.
- Confidence: medium · public API change: yes · root cause: none named.

**RD-16 · perf · `composer/session.rs:136-141`**
- **What:** each rounded clip push copies its whole ancestor chain into `rounded_clips` with `extend_from_within`. The pool grows by the sum of depths, bounded at 255 per chain.
- **Direction:** WebRender uses clip-chain nodes with parent links. Here the backend reads contiguous spans, so this is acceptable unless deep rounded nesting shows up in a profile.
- **Confidence:** high (on the cost). **Public API:** no. **Root cause:** flat pool chosen for the backend.

**Already logged:** `PaneBreak.since` is a single bounding box, so an L-shaped layout breaks the pass needlessly (`composer/pane_break.rs:191`). `.notes/PERF.md` shelves it. `.notes/ISSUES.md` and `.notes/TODO.md` hold nothing else in this scope.
