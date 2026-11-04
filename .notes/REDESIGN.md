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
3. Settled: a `Response` is owned and probed at `show` (UI-10), at a measured 9 µs a record
   pass (AGENTS.md "A `Response` is owned").

## Batches

The order is by impact: user-visible bugs and panics first, then perf with wide reach, then
the reduction of complexity. Each batch stays inside one system, so one review covers it. A
batch marked **API** changes the public surface and needs your go-ahead (AGENTS.md "Public
API").

### Batch 10. Authoring surface and the overlay protocol (`widget_core`, `widgets`, `theme`) **API**

Root causes R9 and R1. This batch removes the most code and changes the most public surface.

- State the bound of the input queue and decide what a full one does (the rest of UI-27).

Done in this batch: decisions 1 and 2 (WG-15, WG-16), UI-05, UI-06, UI-09, UI-14, UI-16,
UI-17, UI-18, UI-20, UI-23, UI-26, UI-28, UI-07, UI-08, UI-10, WG-09, WG-10, WG-13, WG-14, WG-17, WG-18, WG-22, WG-23, WG-24, WG-25,
and the wake and watch bounds of UI-27. UI-09 carries `ClickOutside`, now `Block` or `Dismiss`, in
the backdrop, not a new `Capture`, a name the GPU capture holds; a pass-through popup is
`click_outside(None)`. WG-14 keeps a row for each widget, as both placements run one
`TriggerFrame`; a trigger has one menu row. WG-24 is `ComboBoxList` for the combo, and the dock's
part ids are methods of `TabGroupId` and `DockPath`. `OverlayScope::record` answers the `OverlayResponse`, its
`opened` probe included, which `OverlayTurn` was half of. WG-10 keeps `DragValue` writing on each keystroke, as
egui and Qt default to, so it keeps its own draft; its editor now records until its own blur, and
`stop_editing` acts as `EditableLabel`'s does. A field's focus memory carries the record frame it ran in, so a
field its widget closed unseen reads no stale focus. UI-17 took serde's remote derive on a private twin (`#[serde(remote = "Theme")]`), not
`remote = "Self"`: that derive adds a public inherent `Theme::deserialize` that skips the check.
Its failure stays a rule text, as every `domain::file` rule is: serde carries only a message
through `de::Error::custom`, so an enum no caller could match. UI-18 kept `TextStyle::stock`, a
`const` base that `Theme::stock` cannot be. UI-10 made `Response` owned: `ResponseSnapshot`, `ValueEdges`, `EditEdges`
and the lifetimes of the response types are gone, and `Ui::response_for` answers a `Response`.
WG-11 and WG-12 are kept as written, by the owner's call: the per-widget setters stay hand
copies, kept symmetrical (AGENTS.md "A setter each widget repeats"); the consuming half of UI-07
stays too, as sugar over `Configurator`. UI-07's getters are one `Widget::authored()` view,
`Authored`. UI-08 was a wrong doc: `ThemeDefaults` is already exported only through `widget::`,
and its doc now says what an app's `default_*` does and whom the trait serves. UI-19 is kept as it is: the
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

**UI-27 · bound · low · no API change** — `src/ui/frame_runtime/mod.rs:100` (`repaint_wakes`), `src/input_state/input_queue/mod.rs:23` (`pending`), `src/input_state/watches.rs:13-15`
- **What:** these grow without a stated limit. `Watches` pushes duplicates by design.
- **Direction:** state a cap and decide what happens when it is reached.

**RD-16 · perf · `composer/session.rs:136-141`**
- **What:** each rounded clip push copies its whole ancestor chain into `rounded_clips` with `extend_from_within`. The pool grows by the sum of depths, bounded at 255 per chain.
- **Direction:** WebRender uses clip-chain nodes with parent links. Here the backend reads contiguous spans, so this is acceptable unless deep rounded nesting shows up in a profile.
- **Confidence:** high (on the cost). **Public API:** no. **Root cause:** flat pool chosen for the backend.

**Already logged:** `PaneBreak.since` is a single bounding box, so an L-shaped layout breaks the pass needlessly (`composer/pane_break.rs:191`). `.notes/PERF.md` shelves it. `.notes/ISSUES.md` and `.notes/TODO.md` hold nothing else in this scope.
