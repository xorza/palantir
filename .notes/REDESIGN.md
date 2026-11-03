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
  `MeasureSnapshot`, and the damage `prev` (SC-05, LT-04).
- **Which panes are live:** `PaneWalk.keys`, `RenderBuffer.live_panes`, `PaneCaptures.held`
  and the `Backdrops` cache. Each is rebuilt or scanned linearly (RD-03, RD-08, RD-09,
  RD-19).
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
- A hash fold stands in for an exact identity in `MeasureSnapshot` (LT-04) and in
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
  of `Option`s with `expect`/`unreachable!` (GA-05, GA-08).
- **Overlay spec:** `OverlaySpec` asserts a link between `Backdrop` and `ClickOutside` at run
  time (UI-09).
- **Mixed states:**
  - `Ident` holds a recipe and its result in one enum (SC-06).
  - `activate()` writes `Up` beside `Drag::Active` (UI-12).
  - `TabDrag { tab: Option, target: Option }` (WG-18).
  - `HostCore` holds `Option<DeviceHalf>` (HS-08).
- **Pairs that must agree:** a pipeline and its layout (GB-07), a depth used as a stencil mask
  (GB-18), and a UV width that the atlas budget does not set (GB-19).

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
  whole cascade (SC-04).

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
- `SingleQuadBuffer::is_uploaded` stays true after frame 1 (GB-05).
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
| Text render | No gamma or contrast on glyph coverage (GB-15). Inter's `opsz` is never set (LT-14). |
| Platform | Wayland clipboard (HS-15). Tiler detection on DXGI and GLES (GA-01). |

### R11. Test seams in production state

- `PaneWalk.on` (RD-04).
- `shadow_grid: bool` (GB-20).
- `#[cfg(test)]` stencil usage (GA-07).
- `WgpuBackend.pass_traffic` mirror (GA-11).
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
5. **"A staging buffer moves an image in fixed bands"** (GB-16). wgpu-core keeps each band's
   staging until submit, so the peak is still the whole image. You can stream the bands
   through one buffer with a submit between them, or correct the text.
6. **"Scratch decays"** is written as a rule, but only the blur follows it (R3). The proposal
   applies it in the shared primitive. That is batch 5.

## Batches

The order is by impact: user-visible bugs and panics first, then perf with wide reach, then
the reduction of complexity. Each batch stays inside one system, so one review covers it. A
batch marked **API** changes the public surface and needs your go-ahead (AGENTS.md "Public
API").

### Batch 5b. GPU construction and shared atlases (`gpu/pipeline`, `gpu/resource`, `gpu/backdrop`)

What Batch 5 left. The decay policy, the display-frame clock and the oversize staging landed.

- Build pipelines lazily: the backdrop composites, the shadow forms and the stencil-test twins,
  on the first frame that draws one. This needs a census of the frame's draws before its passes
  record, as the passes borrow the pipelines (GB-01).
- Build the composite's sampler and spectrum buffer with `PaneResources` (GB-21).
- Put the cutout tables in the shared atlas at stable `(r, σ)` placements, with `ShelfPacker`
  and deallocation. Plan only the spans per window (GB-03, GB-12).
- One atlas type with a soft and a hard budget (GB-11). One helper for row uploads on `GpuCtx`
  (GB-22).
- Decide on image bands (decision 5) (GB-16).

### Batch 6. Widget behavior parity (`widgets`, the text editor)

Root cause R10. Each item is small and visible to the user.

| Widget | Changes |
|---|---|
| `bound_num` | Under a grid, the grid point is the value, with no decimals rounding after it ✔ (WG-01). Add a test with a step finer than the decimals. |
| Text editor | Goal column (WG-02). Triple-click selects the hard line in multi-line ✔ (WG-03). Ctrl+Y on non-mac (WG-19). Undo groups break on a pause or a word boundary (WG-20). |
| Tooltip | Hide on a press or Escape until the pointer leaves (WG-05). |
| Dock | One `DockState` predicate decides both the drop wash and `move_tab` (WG-04). |
| Menubar | Down/Up open on the first or last item (WG-07). |
| Scroll | Keys, and reveal of the focused descendant (WG-06). |
| Radio | A group with arrow keys, so selection follows focus. This needs `arrow_focus` support (WG-08, **API**). |
| Splitter | Keep the grab offset (WG-21). |
| Checkbox | One clamp, and a shared `boxed` (WG-26). |

### Batch 7. Text quality (`gpu/raster` shader, `text/cosmic`)

- Correct coverage for gamma and contrast, keyed on the text's luma, as gpui, WebRender and
  Skia do (GB-15). This changes every golden.
- Set Inter's `opsz` to the font size in shaping, ink and raster (LT-14). The shape store then
  reuses a shape across sizes only within one optical size: `TextShapeKey::shape_identity` must
  carry it.

### Batch 8. One owner for widget and pane identity (`scene`, `cascade`, `layout`, `damage`, `renderer`)

Root cause R1. Structural perf, and removal of duplicate maps.

- `SeenIds` keeps the table of the pass the cascade was built from. Remove `Cascade::by_id`,
  or remove the positional route with `cascade_paired` (SC-05).
- The measure snapshot keys by last frame's endpoint. Remove the identity fold (LT-04).
- Compute the frame's pane keys once, as a sorted set. The damage engine, `PaneCaptures`,
  `RenderBuffer` and the `Backdrops` eviction share it. That replaces the linear `contains`
  and `retain` (RD-03, RD-09, Backdrops `evict`). Add a key check per pane ticket (RD-19).
  Derive `PaneWalk.on` (RD-04). Clear `dirty_panes` once (RD-20).
- Split `keeps_structure`. The structural tables follow `cascade_static`, and a change in the
  paint count only packs the paint arena again (SC-04). This removes the full rebuild on each
  keystroke that PERF.md records.
- Decide the scissors before the build, so no frame builds twice (RD-06). Walk the roots in
  order (SC-11).

### Batch 9. GPU frame plan as types (`wgpu_backend`, `gpu/frame`, `gpu/pipeline`, `gpu/raster`)

Root causes R4 and R11. Internal only.

- Make one `FrameTarget` enum (`Direct`, `Backbuffer`, `Canvas`) that carries its resources.
  Derive the pass plan from it. `RenderStream` fields become private, with a
  `mark_painted(route)` call (GA-05, GA-08, GA-21).
- `PipelineRecipe::build` returns `Pipeline { pipeline, layout }`. Store `QuadVariants` as an
  array indexed by `QuadDraw`. Derive the stencil label (GB-07, GB-08, GB-09).
- `RasterAtlas` exposes operations, not fields: `touch_slot` and `RasterPass::push` (GB-04,
  GB-27).
- Put the clear quad and the masks in the quad stream as spans, and delete
  `SingleQuadBuffer`, whose `is_uploaded` stays true past the first frame, so its
  `bind_clear` assert catches no missed upload (GB-05, GB-06).
- Merge or remove: `BoundSides`/`SidesLayout`, one sampler and one `texture_entry`,
  `ClipStencil`, static shader constants, `capture_blit` into `pipelines[fmt].blit`, one
  readback classifier, one 2D texture helper (GB-13, GB-14, GB-25, GB-26, GA-09, GA-10,
  GA-19).
- Remove the test seams: `shadow_grid`, the `#[cfg(test)]` stencil usage, the `pass_traffic`
  mirror (GB-20, GA-07, GA-11).
- Small fixes:
  - Bound `wait_until` by `WAIT_TIMEOUT` (GA-02).
  - Clear `MapState` on unmap (GA-12).
  - Use the full stencil mask (GB-18).
  - Make the UV-width check a release assert (GB-19).
  - Reset `painted` on resize (GB-24).
  - Tiler detection falls back to the adapter name (GA-01).
  - `create_filled_buffer` takes the label, size and usage (GA-13).
  - Add `From` for the errors (GA-16).
  - Fix the two false doc reasons (GA-15).
  - An `OwnedTarget` keeps its view, so an offscreen frame makes no view (GA-17).
- Add tests: the content after a `DynamicBuffer` grows, and a shared backend with two windows
  (GB-28).
- Move the module placement: the contexts go beside `GpuPaint` and `GpuCtx` goes to `frame`
  (GA-18). The device and queue get one access path (GA-22). `TextureBinding` is created with
  the device (GB-17).

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
| 5b | GB-01, GB-03, GB-11, GB-12, GB-16, GB-21, GB-22 |
| 6 | WG-01 ✔, WG-02, WG-03 ✔, WG-04, WG-05, WG-06, WG-07, WG-08, WG-19, WG-20, WG-21, WG-26 |
| 7 | GB-15, LT-14 |
| 8 | SC-04, SC-05, SC-11, LT-04, RD-03, RD-04, RD-06, RD-08, RD-09, RD-19, RD-20 |
| 9 | GA-01, GA-02, GA-05, GA-07 to GA-13, GA-15 to GA-22, GB-04, GB-05, GB-06 to GB-09, GB-13, GB-14, GB-17 to GB-20, GB-24 to GB-28 |
| 10 | UI-05 to UI-10, UI-14, UI-16, UI-17, UI-18, UI-19, UI-20, UI-23, UI-26 to UI-28, WG-09 to WG-18, WG-22 to WG-25 |
| 11 | HS-05 to HS-09, HS-23, HS-11 to HS-13, HS-15, HS-17 to HS-20, HS-24 to HS-26 |
| 12 | LT-02, LT-03, LT-06, LT-07, LT-08, LT-11 to LT-13, LT-15 to LT-20 |
| 13 | RD-01, RD-02, RD-05, RD-07, RD-10 to RD-18, RD-21, SC-06 to SC-10, SC-12 to SC-17 |
| 14 | PR-05 to PR-07, PR-09 to PR-23, GA-14, UI-25 |

These findings are kept as they are, with no change: SC-20 (the `SeenIds` size is justified
by a measurement) and RD-16 (the copies of the rounded-clip chain are accepted unless a
profile shows them). `.notes/ISSUES.md` already logs the `Staging::abort` race.
