# Review: palantir

Scope: the whole crate (`src/`), `palantir-derive`, `palantir-showcase`, `examples/`. Read from the
code and from `.notes/API_SURFACE.md`; nothing was edited. Paths are relative to the repository
root.

Marks used below:

- **API**: changes an exported item, so it waits for a go-ahead (`AGENTS.md` "Public API").
- **verified**: the defect was re-read in the source after it was found.
- **unrun**: reasoned from the code, never executed; the first step is the failing test.

Test timings, one run of the gate's test lane on an Apple-silicon laptop: `palantir` 2683 tests in
1.43 s, `palantir-showcase` 29 in 0.27 s, `tests/alloc` 44 in 0.54 s, `reel` 9 in 0.14 s,
`palantir-derive` 4 in 0.00 s. The slowest test,
`gpu::pipeline::quad_pipeline::cutout_tables::tests::a_baked_table_holds_the_cutout_it_tabulates`,
takes 0.79 s alone and 1.01 s inside the suite; the next is 0.46 s. `cargo visual` was not run.

## Part 1: Proposal

### Root causes

1. **Nobody reads the surface as one text.** Each name is right inside its module and wrong beside
   its neighbors: `Fit::Natural` pins top-left while `ImageFit::Natural` centers; `ButtonState` is a
   pointer button's beside the `Button` widget; `Reduction::Full` means "not reduced"; `slot`,
   `selected`, `scale`, `build`, `blur`, `reach` and `close` each mean two things. No rule says what
   belongs in `palantir::widget`, so app-level `Ui::add_shape` and `Ui::animate` take arguments
   that only `widget` exports.
2. **A convenience is added as a second route with its own state, not as sugar over the canonical
   one.** `WinitHostBuilder::title` (a second stored title and a precedence rule),
   `ZoomFactor: TryFrom<f32>` beside `new`, `TextShape::family/weight/slant` beside the `GlyphFont`
   the constructor took, `DockView::run` (drops every setting), `Menu::at` (a second owner of
   "open"), `Popup::close`/`Modal::close` (aliases of `Ui::withdraw_input_scope`),
   `Ui::watch_keyboard`, `Ui::load_icons`' pointer dedup, `default_chrome` on nine builders,
   `ColorPicker::color(&self)`.
3. **One concept is declared as two types.** `Fit`/`ImageFit`; `Axis`/`SplitDirection`/
   `AllowedSplits::{ROW, COL}`/a `bool` for `SplitHalf`; `Gradient<G>` generic over a sealed set of
   three; an effect's parameters as `*Input` and as `params`; a backdrop effect's read reach as
   `EffectSource::reach` and as `EffectPaint::max_sample_offset`; `MenuBarTheme` re-declaring a
   `ButtonTheme` with a selected pack; four `From<bool>` pick enums that disagree in name, order
   and derives.
4. **A closed set of cases is carried as independent `Option`s, `bool`s or placeholders, and each
   stage re-infers the case.** An effect's kind travels as `capture: Option` plus
   `outside: Option` through `EffectPaint` → `ErasedEffectPaint` → `ClosedPaint::Effect` →
   `ClosedEffect`; an overlay root is inferred from `layer != Main`; `Locator` nests `Nth` and
   `Offset` in boxes; `TexelOrder::Blit` is not an order; GPU timing is an `Option` threaded
   through four structs beside the enum that already models "does nothing"; `ShotError::Play
   { line: 0 }` means the launch; every payload carries `spin: None` until a gate fills it.
5. **A value owned by one layer is parked in another and read back.** `BackendSpec` is rebuilt from
   three backend accessors on every device attach; the device's texture cap is passed back into the
   `SurfaceManager` that opened the device; `RenderBuffer.live_targets` is written by `ui` from a
   `GpuViews` the consumer also receives; `Ui` looks up a `WidgetLocation` in `Input`'s own cascade
   and hands it back to `Input`; the effect registry pushes into an `Rc<dyn EffectStore>` whose
   only implementor already waits for the next `prepare`.
6. **A small rule is written again at each site, and the copies drift.** Reveal-pan three times
   (one contradicts its doc), the tab drag lifecycle twice (one leaks), `commit_travel` twice (the
   two disagree on `committed`), `ValueResponse` assembled by hand at six sites, the texture
   window's step-down in `StagingTarget` and three more times in the blur, a generational slab in
   three atlas types, a handle-to-key reverse map in three atlas tenants, the grid's two-round span
   distribution three times, the `Selection` → pack match three times, the float inks twice, the
   headless hosts' setter bodies twice, the thread handoff per platform file, the error-chain
   walker twice, the fixture theme twice.
7. **State lives in plain bags and the logic lives in the large owner.** `Capture`, `Press` and
   `Pointer` expose fields, so 22 transitions and both collations sit in the 1.1k-line
   `input/mod.rs`; `DamageEngine` fields are `pub(super)` for tests beside gated accessors for the
   same data. The reverse also occurs: `GpuPassStats` has nine setters that enforce nothing.
8. **Test and debug needs shape shipped code.** `Rect::intersect` answers differently in debug and
   release; `Harness::open` pins a font size under `cfg(test)`; the clipboard's fallback is typed
   as any backend so one test can make it refuse, which makes every `set_*` return an error no
   shipping build produces; `DrawStrokePayload: Default` exists for test literals;
   `internals::module_order` exempts a folder by the name `"sweep"`; `Widget::show` reads its
   response at a different time than `response()` + `record()`.
9. **Docs and names were left behind by a refactor.** The README names a method that does not
   exist; `backdrop/mod.rs` documents the refused atlas; `EncodedKey` keys on a color its template
   no longer bakes; `TextRun` cites `Shape::Text`; `subs` still names the watches.

### Target design

**Surface**

- **One rule for `palantir::widget`**: a type sits there only when no root-exported method takes
  or returns it. Under it the drawing vocabulary (`Shape`, the `*Shape` builders, `PaintedBy`,
  `AddedShape`, `ImagePaint`, `IconPaint`, `EffectPaint`, `Mesh`, the two beziers, the `Paint*`
  animation family, `AnimationRequest`/`AnimationSlot`/`AnimationKey`/`MotionRole`) moves to the
  root, and what only a widget author names (`AnimatedLook`, `Engagement`, the pick enums) moves in.
  `widget::domain` exports the kinds a public `# Panics` doc names, item by item, not the module.
- **One type per concept**: one `Fit`; `Axis` for every split; `enum GradientGeometry` inside one
  `Gradient`; `*Params` for an effect's parameters; `Backdrop*` for a backdrop effect's reach and
  fallback, with one reach mechanism in the registry.
- **A second route is sugar or it goes.** Each item of root cause 2 either only converts and calls
  the canonical form, with no state of its own, or is removed.
- **Rules amended where the code is right and the rule is narrow**, in `AGENTS.md`: a one-case
  error is a struct still named `*Error`; a bound alias (`DockTab`) and an extension trait
  (`ThemeDefaults`) are kinds; `xxs` … `xl` are abbreviations; `DEFAULT`, `FALLBACK`, `ROOT` and
  `EXACT` are sentinels; an overlay's `*Response` holds no node response.
- **`cargo drive surface` stops printing a blanket impl on every type** (`DockTab (blanket)` sits
  on about 70 unrelated entries).

**Data**

- **An effect's kind is one enum from erasure to compose**: `EffectRead::{Color { outside },
  Backdrop { capture }, Material { capture, stage }}`, built once where the paint is erased,
  matched once in the composer.
- **A pending overlay root is one `Option<PendingRoot { placement, layer, owner_disabled }>`**, and
  `RootSlot` stores `placement: Option<Placement>` instead of testing `layer == Main`.
- **`Locator` is a flat value**: `{ target, nth: Option<usize>, offset: Vec2 }`.
- **GPU timing is `enum GpuTimings { Off, Queries, WallClock }`**, held by value, never optional.
- **The value-write protocol has one home**: `ValueResponse::pick`, `ValueResponse::gesture`, and
  `BoundNum::commit_travel`.
- **Shared rules are owned once**: `reveal_pan`, `TabCarry`, `DecayingTexture`, `GenSlab`,
  `Handoff<T>`, `ErrorChain`, a tenant tag on atlas evictions, `Grid::spans`.

**Ownership**

Every shared-ownership case, with its verdict.

Replaced:

| where | what it shares | replacement |
| --- | --- | --- |
| `src/effect/effect_registry.rs:33` `Option<Rc<dyn EffectStore>>`; `src/gpu/pipeline/effect_pipeline/wgpu_effect_store.rs:41` `RefCell<StoreState>`; `Rc<WgpuEffectStore>` in `host/host_core.rs:33`, `host/host_device.rs:36`, `gpu/wgpu_backend/mod.rs:157` | the backend's pipeline store, written by the registry | the registry owns the modules and a bounded change list; the backend's `prepare` takes `&EffectRegistry` and drains it. Trait, `Rc` and `RefCell` go |
| `src/gpu/wgpu_backend/mod.rs:155,157` `Rc<WgpuImageStore>`, `Rc<WgpuEffectStore>` in their second role | the stores carried to the next device | `HostDevice { backend, spec: BackendSpec }` keeps them; the backend's read during draw stays |
| `src/icons/icon_registry/mod.rs:51` `Weak<IconSetToken>`; `Ui::load_icons(impl Into<Rc<IconTable>>)` | a pointer dedup of tables | `load_icons(table: IconTable) -> IconSet`; the registry makes the one `Rc`; `IconSet::clone` shares |
| `src/ui/resources/app_settings.rs:22-42` two system `Cell`s; `src/ui/resources/shared_device.rs:11-12` two `Cell`s | values always written together | one `Cell` of a `Copy` struct each |
| `src/bench/case_group.rs:12` `static BETWEEN_CASES: AtomicBool` | a flag between bench cases | a `Cell<bool>` on `Backends`, passed to `Kind::group` |
| `src/gpu/test_gpu.rs:58` `AtomicU64` (`cfg(test)`) | a frame counter through `&self` | `Cell<u64>`; no test shares it across threads |
| `palantir-showcase/examples/reel/deck.rs:518` `Rc::clone(&self.cube)` | nothing | `GpuView::new(&self.cube)` borrows |

Kept, as no single owner can exist:

- **Thread boundary**: `host/wake_signal.rs:12` `Arc<WakeSignal>`; `host/winit/frame_log.rs:16`
  `Arc<FrameLog>`; `host/winit/wake_latch.rs:11` and `host/winit/remote/mod.rs:236,352`
  `Arc<AtomicBool>`; `host/winit/platform/{linux.rs:112, windows.rs:132}` `Arc<Mutex<_>>` (one
  shared `Handoff<T>`, batch 13); `host/winit/platform/macos.rs:50,66,216`; `window/waker.rs:16`
  `Arc<dyn Fn>`; `gpu/device/device_health.rs:11`; `gpu/resource/map_state.rs:23`;
  `gpu/frame/gpu_timings/{query_tracer/mod.rs:98, wall_clock_tracer.rs:25}`;
  `palantir-showcase/src/job.rs:15`.
- **Forced by a dependency**: `host/winit/window/mod.rs:99` and `gpu/surface/surface_manager/mod.rs:65`
  `Arc<Window>`, `:48` `Arc<dyn InstanceDisplay>` (wgpu takes owned handles);
  `gpu/device/device_health.rs:29` (wgpu's error handler); `text/cosmic/mod.rs:266` and
  `text/font_scope.rs:33` `Arc` (fontdb's `Source::Binary`); wgpu's own ref-counted handles cloned
  across owners; `Gpu: Clone`.
- **Handles an app holds with overlapping lifetimes**: `text/shaper.rs:29,37,38` `Rc<Shared>`,
  `RefCell`, `Cell` in `TextShaper`; `text/family_keys.rs:33` `Rc<RefCell<Table>>`;
  `image_registry/image_handle.rs:17,25` `Rc<ImageToken>`, `Cell<u32>`;
  `image_registry/mod.rs:34` `Rc<dyn ImageStore>`; `gpu/resource/wgpu_image_store.rs:31`;
  `icons/icon_set.rs:22`, `icons/icon_registry/mod.rs:13,14,49,71`;
  `gpu/gpu_paint/gpu_paint_ref.rs:10` `Rc<RefCell<dyn GpuPaint>>` and `GpuView::new(&Rc<RefCell<T>>)`
  (the app mutates between frames, the backend at submit); `ui/theme_pair.rs:11`,
  `ui/theme_transition.rs:19,28-30` `Rc<Theme>` and every `Rc::clone(ui.theme())` in the widgets
  (a 28 KiB immutable value read across `&mut Ui`).
- **One owner reached through `&Ui`**: `ui/resources/mod.rs:33` `Rc<SharedResources>` (recorded:
  `Ui<'res>` refused) and what hangs from it: `effect_registry.rs:34,37`,
  `window/window_directory/mod.rs:11`, the remaining `AppSettings` cells,
  `clipboard/mod.rs:329` `Rc<RefCell<ClipboardState>>` (recorded at `ui/resources/mod.rs:26-31`;
  it could become a `RefCell` inside `SharedResources` behind `Ui::clipboard() -> &Clipboard` once
  `widgets/text_edit/editor.rs:95-106` stops holding the clone).
- **Process-wide by nature**: `common/id_counter.rs:9` with its statics at
  `primitives/identity/texture_id.rs:22` and `primitives/text/text_epoch.rs:16`;
  `common/hash/mod.rs:27` (foldhash's seed); `effect/stock/mod.rs:66` `LazyLock<StockEffects>`
  (immutable, refusals recorded); `common/tracy.rs:73` (a feature-gated diagnostic);
  `gpu/test_gpu.rs:150` `static PROCESS`; `internals/panic_probe.rs:13-20`;
  `bench/backends.rs:19` `OnceCell`; the showcase's `tests/visual/goldens.rs:20` and the alloc
  test's allocator cells.
- **Counters retained under `cfg(test)` alone**: `common/counters.rs:103` `TestOnly<Cell<u32>>`,
  used at `renderer/frontend/composer/{occlusion_pruner.rs:40, rect_grid/mod.rs:39}`.

`layout`, `cascade`, `input`, `damage`, `renderer` (beyond the two counters), `shape`, `scene`,
`animation`, `display`, `diagnostics` and the widgets hold none.

**Module layout**

- `src/lib.rs`: the root / `widget` split above; no `pub use naga`.
- `src/primitives`: `effect/` holds every stock effect's name and params; `material/` leaves
  `paint/`; `math/domain/file` moves to `primitives/file`; `FillKind` moves to `paint`;
  `TypedStores` moves to `common`. This removes the `primitives` → `effect` edge that an inherent
  const hides from `internals::module_order`, and the `paint` ↔ `effect`, `math` ↔ `geometry` and
  `packed` ↔ `paint` cycles.
- `src/shape`: `lower/` dissolves into `RecordStore` methods; `BlurPlan` moves to its leftmost
  user.
- `src/renderer`: `GpuFill` moves into `render_buffer`, which then imports nothing from
  `frontend`.
- `src/damage/sweep` becomes `src/damage/bench/sweep`, and the module-order test treats a `bench`
  directory as it treats `bench.rs`.
- `src/gpu/atlas`: `FormatPool { pages, scratch }` per format; `page_list/{page.rs, move_budget.rs}`.
- `src/host/winit`: `user_event.rs` holds the loop's protocol, cutting `handle` ↔ `remote`;
  `handoff.rs` serves both platform files.
- `palantir-derive`: one file per derive.
- Sibling cycles cut: `layout::measure_columns` ↔ `cache`, `layout::plane_layout` ↔ `drivers`,
  `scene::scrollbars_def` ↔ `tree`, `gpu::surface::window_surface` ↔ `surface_manager`,
  `gpu::frame::schedule` ↔ `frame_passes`.

### Where the code and a recorded decision disagree

- **`Response: Deref<Target = ResponseState>`** (`primitives/input/interaction/response.rs:12`,
  documented, so `r.hovered()` reads like the state) is what the Rust API Guidelines rule out:
  "Only smart pointers implement `Deref` and `DerefMut`"
  ([C-DEREF](https://rust-lang.github.io/api-guidelines/predictability.html#c-deref)). The same
  file refuses the deref on every wrapper because it "would blur which is read". **Decided: one
  struct.** `Response` holds the state's fields and `id`; `ResponseState` and the `Deref` go
  (batch 4).
- **`AGENTS.md` module order** omits `effect`, which `internals/module_order.rs:11-20` places in
  the third tier.
- **`src/gpu/backdrop/mod.rs:25-36,95-100`** documents one results atlas and refuses a standalone
  texture per result; `backdrops/mod.rs:6-16,627-629` and `src/gpu/AGENTS.md:164-172` do the
  opposite.
- **`ui/ui_preferences.rs:12-13`** refuses a getter per setting on `Ui`; `Ui` has nine.

### Judged and left

- `Modifiers` beside `ShortcutMods`, `Size`/`Sizing`/
  `SizeSpec`/`Track`, `Paint`/`Brush`/`Chrome`/`MaterialPaint`, the `look` family, `Ui`'s flat
  surface, `Arranged` owned by `Input`: each carries a recorded reason.
- `Ui::focus_first_within`, `Harness::new`, `Chrome::rounded`, `Track::fixed`, `Rect::inflated`:
  sugar that only converts and calls.
- `TabbedViewResponse` repeating `TabStripResponse`'s fields: flat by the Responses rule.
- `ParamType` as twelve variants: the flat form mirrors WGSL's types, as `wgpu::VertexFormat` does.
- `FontSource` as a named type: it leaves room for a face index.
- `Popup::click_outside(impl Into<Option<ClickOutside>>)`: `None` is "no barrier", which a modal
  cannot be.
- `Vsync { On, Off }` as an enum: presentation has more than two modes wherever it is modeled
  ([`wgpu::PresentMode`](https://docs.rs/wgpu/latest/wgpu/enum.PresentMode.html) has six), so a
  `bool` would close the type. Record that reason beside the type, as the Setters rule reads
  otherwise.

### Checked against established practice

Each entry settled or changed an item of the plan. Every linked page was read for this review,
and quoted words are the page's own, except the thiserror, C-GOOD-ERR and C-CUSTOM-TYPE links
and the std items (`NonZero`, `OpenOptions`, `ParseIntError`), which are cited from the
manuals as known.

- **A gradient's kind is an enum inside one struct, each variant holding its position struct.**
  As [peniko](https://docs.rs/peniko/latest/peniko/struct.Gradient.html):
  `Gradient { kind: GradientKind, extend, stops, .. }` with
  [`GradientKind::{Linear(LinearGradientPosition), Radial(..), Sweep(..)}`](https://docs.rs/peniko/latest/peniko/enum.GradientKind.html).
  So `LinearGeometry`, `RadialGeometry` and `ConicGeometry` stay as the variants' payloads, with
  their validation, and only the sealed trait, the type parameter and the three aliases go.
  Refused: struct-like variants, which re-declare the fields those three structs hold.
- **`Natural` centers, and tiling is not a fit.** CSS `object-fit: none` is "not resized to fit
  inside the element's content box" and `object-position` starts at `50% 50%`
  ([css-images-3](https://drafts.csswg.org/css-images-3/#the-object-fit)), so `ImageFit::Natural`
  is the conforming one and `Fit::Natural` cites the same property for a top-left pin. Flutter's
  [`BoxFit.none`](https://api.flutter.dev/flutter/painting/BoxFit.html) agrees: "Align the source
  within the target box (by default, centering)". None of `BoxFit`'s seven values tiles, nor does
  any `object-fit` value, which is why batch 2 gives tiling its own setter.
- **A pick binds a value.** egui's
  [`selectable_value(&mut cur, value, label)`](https://docs.rs/egui/latest/egui/containers/struct.ComboBox.html),
  iced's [`pick_list(options, selected: Option<V>, on_selected)`](https://docs.rs/iced/latest/iced/widget/fn.pick_list.html)
  with `T: PartialEq + Clone`, and Flutter's
  [`DropdownButton<T>.value`](https://api.flutter.dev/flutter/material/DropdownButton-class.html),
  "the value of the currently selected DropdownMenuItem", all bind the value; egui adds
  `show_index` for an index. None of the three binds a reference into the options, and that
  form's cost shows at eight showcase call sites. With `&mut T` and
  `T: PartialEq + Clone` the widget compares each frame and clones once, on the pick, which
  answers the clone-each-frame reason `src/widgets/AGENTS.md:42-45` gives for refusing it.
- **A stepped value commits a change, never a key press.** HTML: the `input` event "fires
  whenever the user has modified the data of the control", and `change` is queued "any time the
  user commits the change"; a slider's `change` fires "when the user let go of the knob,
  committing to a specific value"
  ([HTML, common input element events](https://html.spec.whatwg.org/multipage/input.html#common-input-element-events)).
  An arrow key at the bound changes nothing, so it commits nothing: `Slider`'s rule stands and
  `DragValue` changes. A text commit keeps Qt's `editingFinished`, which fires unchanged, as
  `src/widgets/AGENTS.md` records.
- **"Pan the least that shows it" is CSSOM's `nearest`.** The
  [scroll-into-view algorithm](https://drafts.csswg.org/cssom-view/#scroll-an-element-into-view):
  both edges outside on both sides, "Do nothing"; start edge outside and the element smaller, or
  end edge outside and the element larger, align the start; the two mirrored cases align the end.
  Palantir's documented rule, "a span longer than the view shows its start", matches one of the
  three oversized cases, and `ScrollState::reveal` matches none of them where it shows the end of
  a span that starts in view. Adopting `nearest` gives the shared `reveal_pan` a specification
  and an oracle
  ([WPT `scrollIntoView-nearest-oversized-element`](https://searchfox.org/firefox-main/source/testing/web-platform/tests/css/cssom-view/scrollIntoView-nearest-oversized-element.html));
  Firefox was changed to it from "always the start"
  ([bug 1416391](https://bugzilla.mozilla.org/show_bug.cgi?id=1416391)).
- **The dock's one-call form is the configured builder's terminal.**
  [egui_dock](https://docs.rs/egui_dock/): `DockArea::new(&mut dock_state).style(style)
  .show_inside(ui, &mut tab_viewer)` draws and applies in one call, after the options. So the
  one-call form stays on the surface as a configured builder: `DockView::new(dock, &mut state)`,
  beside `DockView::emitting(..)` for a caller that applies the operations itself. Refused:
  moving `run` to the tests, which leaves the surface with the four-step protocol alone; the
  state at the terminal, which takes required input out of the constructor.
- **GPU timer queries: a bounded queue of pending frames that drops the newest.**
  [wgpu-profiler](https://docs.rs/wgpu-profiler/latest/wgpu_profiler/struct.GpuProfilerSettings.html)
  "queues up to `max_num_pending_frames`", suggests 2 to 4, and drops the newest frame so older
  results still come back. Palantir's skipped frame is that overflow rule; the defect is that the
  count is one below the lag of a frame path that polls nowhere. The count is derived from the
  lag, not typed in.
- **An eviction hands back the tenant's own data.** WebRender's texture cache entry carries
  `user_data` and an optional `eviction_notice`
  ([`CacheEntry`](https://doc.servo.org/webrender/texture_cache/struct.CacheEntry.html)), so a
  tenant keeps no reverse map.
- **A generational slab promises what [`slotmap`](https://docs.rs/slotmap) promises**: a removed
  key never resolves again, and a slot's reuse bumps its version. The crate's handle packs a
  format beside index and generation, which a `slotmap` key has no room for, so a wrapper is
  needed either way; the in-crate `GenSlab` takes `slotmap`'s method names (`insert`, `remove`,
  `get`, `get_mut`, `contains_key`) and its guarantee, and adds no dependency.
- **The owner keeps the data and a change list; the renderer extracts at one point of the
  frame.** Bevy's [`RenderAsset`](https://docs.rs/bevy/latest/bevy/render/render_asset/trait.RenderAsset.html)
  "describes how an asset gets extracted and prepared for rendering". That is the registry and
  `prepare` of batch 8; a trait object the registry pushes into is the inverse.
- **A wheel event carries one delta and its unit; lanes are the frame's sum.** The DOM's
  [`WheelEvent.deltaMode`](https://developer.mozilla.org/en-US/docs/Web/API/WheelEvent/deltaMode),
  "the unit of the delta values" (pixel, line, page), and winit's
  [`MouseScrollDelta::{LineDelta, PixelDelta}`](https://docs.rs/winit/latest/winit/event/enum.MouseScrollDelta.html).
  So the event is `InputEvent::Scroll { delta: Vec2, unit: WheelUnit }`, which cannot hold two
  units at once, and the four-lane `WheelDelta` is what a frame accumulates.
- **`nth` belongs to the locator, an offset to the action.** Playwright:
  [`locator.nth(index)`](https://playwright.dev/docs/api/class-locator), and `click` takes
  `position`, "a point to use relative to the top-left corner of element padding box". Six
  `Action` variants take a locator, so the offset stays one field on the locator instead of six
  on the actions; both fields are order-free, as Playwright's are.
- **A chain printer is `std::error::Report`**, unstable under `error_reporter`
  ([rust-lang/rust#90172](https://github.com/rust-lang/rust/issues/90172)): a wrapper whose
  `Display` prints the error, then each `source`. `ErrorChain` takes that shape, so it is
  replaced by the std type when it lands.
- **A one-case error is a struct named for what failed**, as std's `ParseIntError`,
  `TryFromIntError` and `RecvError`
  ([C-GOOD-ERR](https://rust-lang.github.io/api-guidelines/interoperability.html#c-good-err)).
- **A wrapper either adds a message or is transparent.** Both are sound: its own `Display` with
  the cause in `source`, or the cause's `Display` forwarded with `source` skipping one level
  ([`#[error(transparent)]`](https://docs.rs/thiserror/latest/thiserror/#details)). The rule to
  write down is which applies when: transparent when the wrapper adds no context.
- **A comparison is written so a NaN fails it.** euclid's `Box2D::is_empty` is
  `!(max.x > min.x && max.y > min.y)`, true for "zero, negative or NaN", and `intersection`
  returns `None` on it ([euclid](https://docs.rs/euclid/latest/euclid/struct.Box2D.html)).
  `Rect::intersects` already has that form; `intersect` loses it to `f32::max`.
- **A boolean setter named for its setting is std's form** (`OpenOptions::create(true)`);
  [C-CUSTOM-TYPE](https://rust-lang.github.io/api-guidelines/type-safety.html#c-custom-type) is
  about positional arguments. So `EffectSource::prebuild(bool)` stands.
- **`TryFrom` beside `new` is not wrong by std's lights**: `NonZero<u32>` has both. The reason to
  drop `ZoomFactor: TryFrom<f32>` is this crate's one-route rule and its "an `Option` for one
  rule", not a general idiom.
- **An authoring module beside the app surface has precedent**: iced's
  [`iced::advanced`](https://docs.rs/iced/latest/iced/advanced/index.html), "Leverage advanced
  concepts like custom widgets", holds `Widget`, `Layout`, `Shell`, `Renderer`. What an app draws
  with is not in it. `palantir::widget` keeps that role once the drawing vocabulary leaves.
- **A clipboard write that can fail says so, or cannot fail.** arboard's
  [`set_text`](https://docs.rs/arboard/latest/arboard/struct.Clipboard.html) returns a `Result`;
  egui's `Context::copy_text` returns nothing and queues. Palantir always lands the text in its
  in-process buffer, so a paste inside the app cannot fail; what can fail is reaching other
  apps. Either is canonical; today's `Result` is neither, as it is never `Err`.

## Part 2: Plan

Each batch leaves the build and the gate green. Whoever addresses an item deletes it. A batch
marked **API** ends with `cargo drive surface`.

**Go-ahead.** An item marked "(decided)" has its design settled by the owner. No exported item
changes on that alone: when an API batch (2 to 7, and the API items inside 9, 12, 13 and 18) is
taken up, its exported changes are listed as a before/after table, decided ones marked, and the
batch waits for a yes before any code moves.

**Order.** Batch 1 first: its fixes are local and several later batches rewrite the same code
(11 after the reveal and carry fixes, 19 after the `Layer::Main` assert). Batches 2 to 7 are the
surface and go in that order, as 3 and 4 rename what 2 merges and 7 moves what they leave.
Batch 8 needs 2 (the `Backdrop*` vocabulary). Batch 22 needs 5 (`Image::texel`). Batches 9 to 21
are independent of each other.

**Verification beyond the gate.**

- A batch that touches painting (2, 8, 15, 16, 18) runs `cargo visual` on the unchanged tree
  first, then after.
- A batch that changes a hot struct or a per-frame path (2 for `Brush`, 8, 9, 15, 16, 17) runs
  `internals::hot_struct_sizes` and the frame bench before and after.
- An item marked **unrun** starts with its failing test; if the test passes on the unchanged
  tree, the item is deleted as not a bug.
- A serialized shape that changes (theme files in 2, 4 and 12; dock layouts and operations in 2
  and 4) rewrites the showcase's and the consumer's files in the same change.

**The consumer.** Darkroom builds against this crate, so each API batch lands with its matching
change there. Its uses of what the plan touches: `Locator::{App, Point, Text}` variants 42,
`Align::h`/`Align::v` 27, `palantir::widget` imports 15, `ImageFit` 9, `ButtonState`/`ButtonPhase`
5, `Ui::set_theme` 4, `Depth` 3, `ComboBox` 2, `max_texture_dim` 2, `window_commands` 2,
`Menu::at` 1, `FrameReport` 1.

### Batch 1: bugs with a local fix

Files: as listed per item. No exported signature changes.

- [ ] `src/widgets/scroll/scroll_state.rs:260-272`: `ScrollState::reveal` shows the end of a span
  longer than the view that starts inside it, where its doc, `scroll/mod.rs:710-719` and CSSOM's
  `nearest` all show the start. Not local: fixed by the shared `reveal_pan` of batch 11, which
  carries this item's test of the three oversized cases.

### Batch 5: additions the call sites ask for (API, additive)

Files: per item.

- [ ] `host/offscreen/mod.rs:219-381`: a `Waker` wake is invisible on `OffscreenHost`. Add
  `has_pending_wake()` and `wait_for_wake(timeout)`, as `Harness` names them; test it.
- [ ] `theme/look`: `impl From<Chrome> for WidgetLook`, `WidgetLook::new(chrome)`,
  `with_text(overrides)`; 36 literals carry `text: TextStyleOverrides::NONE`.
- [ ] `primitives/layout/sizing.rs`: `SizeSpec::FILL` and `SizeSpec::FILL_W`; 77 of the showcase's
  220 `.size(` calls are these two.
- [ ] `primitives/geometry/{spacing, corners}`: lane accessors (`left()` … `bottom_left()`); the
  only read is `to_array()[i]`.
- [ ] `primitives/geometry/translate_scale/mod.rs:158-189`: add `apply_vector`; the `apply_point`
  doc sends a forward caller to `inverse_vector`.
- [ ] `ui/mod.rs:1248-1264`: a state row built from `ui` needs `Option<S>` plus
  `get_or_insert_with` at six showcase sites. Add `with_state_or_insert_with(id, init, body)`,
  named as std's `Entry::or_insert_with`, with `init: impl FnOnce(&mut Ui) -> S`; add
  `set_singleton`/`remove_singleton` or say why a singleton only lends.
- [ ] `primitives/paint/{brush/mod.rs:111-144, paint.rs:143-181, chrome.rs:156-185}`: the `From`
  ladders differ per level (`Brush: From<SrgbaU8>` but not `Paint` or `Chrome`). One set at each
  level, each delegating down.
- [ ] `primitives/input/shortcut`: `ShortcutMods: FromStr`; the shot runner parses
  `"{word}+a"` and takes `.mods`.
- [ ] `primitives/paint/image`: `Image::texel(at: UVec2)`, `Image::magnified(factor)`, and a
  fallible PNG pair under `golden` in place of the two private copies (`golden/mod.rs:351-372`,
  `palantir-showcase/src/shot/mod.rs:370-389`).
- [ ] `theme/color_scheme.rs`: `ColorScheme::ALL`, as `ColorModel::ALL`.
- [ ] `input/capture.rs`: the double-click interval and radius, which
  `palantir-showcase/examples/reel/script.rs:53-57` copies.
- [ ] `host/harness/harness_builder.rs`: `collect_gpu_stats` is public on the other two builders;
  make it public here or record why.

### Batch 6: errors (API)

Files: `src/driver/error.rs`, `src/clipboard/mod.rs`, `src/image_registry/error.rs`,
`src/gpu/error.rs`, `src/host/winit/error`, `src/effect/error.rs`, `src/lib.rs`, `src/common`,
`palantir-showcase/src/{lib.rs, shot/mod.rs}`.

- [ ] (decided) Amend `AGENTS.md` "Error": a one-case error is a struct, named `*Error` for what
  failed, as std's `ParseIntError` and `RecvError`. Rename `ClipboardUnavailable` →
  `ClipboardError`, `ImageTooLarge` → `ImageSizeError`, `HostDisconnected` → `HostHandleError`,
  `UnmetRequirements` → `DeviceRequirementsError`; the last is `#[non_exhaustive]` as its
  neighbors are.
- [ ] `driver/error.rs:106-162`: `PlayError::Unsettled.frames` is the bound on one host and the
  frames recorded on the other; `NoPixels` also means "no device to lose". Target:
  `Unsettled { frames_recorded }` on both; a `NoDevice` case; `#[non_exhaustive]` on `PlayError`
  and `LocateError`; `StepError { step, source }`.
- [ ] Wrappers take two forms: their own message with the cause in `source`
  (`GpuRequestError::Requirements`), and transparent (`PlayError::Locate`, tested at
  `driver/error.rs:258-264`). Both are sound; write into `AGENTS.md` "Error" which applies when
  (transparent where the wrapper adds no context), and hold each exported wrapper to it in one
  table test that walks every chain and asserts no message appears twice.
- [ ] `lib.rs:159-160`, `effect/error.rs`: `pub use naga` for two error sources, where
  `icons/error.rs:64-66` wraps `usvg` so no parser type reaches the surface. Target (decided):
  one opaque `WgslError` holding naga's error and the text it parsed, as the source of
  `EffectError::Parse(WgslError)` and `Validation(WgslError)`; `Display` is naga's message,
  `diagnostic() -> String` what `source.emit_to_string(&text)` prints today, `span() ->
  Option<Range<usize>>` the byte range in the app's text (`None` where it falls in palantir's
  prelude); delete the re-export. Test `diagnostic()` and `span()` on a parse error and a
  validation error.
- [ ] `gpu/test_gpu.rs:33-48`, `damage/sweep/mod.rs:203-211`: two private chain walkers and none
  exported, so the showcase prints "the graphics adapter cannot run Palantir" without the cause
  (`palantir-showcase/src/lib.rs:53-67`, `shot/mod.rs:135`, `examples/reel/main.rs:162`). Target:
  one exported `ErrorChain<'a>(&'a dyn Error): Display` in `common`, in the shape of the unstable
  `std::error::Report`: the error, then each `source` on its own line. The table test above is
  its oracle.
- [ ] `palantir-showcase/src/lib.rs:53-80`, `shot/mod.rs:78-137`: results are `Result<_, String>`
  and a test matches message prefixes. Target: `enum RunError { Usage, UnknownPage,
  AgainstNeedsGolden, Gpu, Window, Shot }` with `From` impls, printed once through `ErrorChain`.
- [ ] `palantir-showcase/src/shot/mod.rs:160-215`: `ShotError` prints its cause in `Display` and
  returns it from `source`; `Play { line: 0 }` means the launch. `Display` names the case alone;
  add `Launch(PlayError)`.

### Batch 7: the root and `palantir::widget` (API)

Files: `src/lib.rs`, `AGENTS.md`, `src/primitives/math/domain`, the showcase's imports.

- [ ] (decided) Write the rule into `AGENTS.md` and the `widget` module doc, then move the items
  listed under "Surface" above. Paths change, nothing else. A trait that only bounds a root
  method's parameter (`Animatable`, `Lerp` on `Ui::animate`) stays in `widget`: an app names it
  only to implement it.
- [ ] `lib.rs:200`: `pub use …::domain` publishes about 55 items, among them `MAX_FILE_LENGTH`,
  `is_lane`, `is_refractive_index`, and submodules named `f64` and `vec2`. Export the kinds a
  public doc names; the rest is `pub(crate)`.
- [ ] `widget_core/configure.rs:389-395`: `ThemeDefaults` carries `default_id` and `default_size`
  → `ConfigureDefaults`.
- [ ] `cascade/rows/tab_stop_row.rs:19-27`: the exported `TabEnd` sits in a cascade row file →
  `primitives/input/tab_end.rs`.
- [ ] `animation/easing.rs:56`: `Easing::curve()` returns the type the surface names
  `widget::PaintCurve`; move the alias left into `animation` and return it.

### Batch 8: an effect's kind, from paint to pipeline

Files: `src/shape/{effect_paint.rs, paint/closed_paint.rs, record_store/mod.rs}`,
`src/renderer/frontend/{payload/draw_closed_payload, composer/compose_session.rs,
encoder/plane_ctx.rs}`, `src/effect/{effect_store.rs, effect_registry.rs}`,
`src/gpu/pipeline/effect_pipeline`, `src/host/{host_core.rs, host_device.rs}`. After batch 2.

- [ ] `shape/effect_paint.rs:50,58,256-261`, `paint/closed_paint.rs:50-55`,
  `record_store/mod.rs:455`, `renderer/…/closed_effect.rs:24-40,69-91`,
  `compose_session.rs:428-463`: `capture`, `outside` and `stage` are three independent `Option`s;
  `stage: Some` with `backdrop: None` would compose against backdrop row 0. Target: the
  `EffectRead` enum of the proposal, built where the paint is erased and matched once in the
  composer; on `EffectPaint` the two fields fold into the per-kind associated type.
- [ ] `closed_effect.rs:56-64,80-109`: `MaterialStage` repeats `ClosedFill` field for field →
  `MaterialStage { paint: ClosedFill, max_luminance, min_luminance }`.
- [ ] `effect/effect_store.rs:22-31`, `effect_registry.rs:33,174-191,285-331`,
  `wgpu_effect_store.rs:225-273`: a one-implementor trait behind `Rc<dyn>`, a `RefCell` over the
  store, and every module's WGSL cloned into it. Target: the registry owns
  `LoadedEffect.module` and bounded `changed` / `wanted` lists; `prepare` takes
  `&EffectRegistry` and extracts, as Bevy's render world does from the main world's assets.
  Trace device recovery and `ensure_format` first, as both rebuild from stored modules: with the
  modules in the registry a new device rebuilds from the registry's full list, not from a change
  list, so `prepare` needs a "rebuild all" entry beside the drain. Test a load, a device loss
  and a recovery in that order.
- [ ] `gpu/pipeline/effect_pipeline/mod.rs:243-248`: `create_shader_module` runs for every form,
  format and clip half of one effect's text. Measure the naga parse against pipeline creation;
  if it shows, cache the module per effect id and clear it on reload.
- [ ] `effect/effect_registry.rs:247-249`: the doc says a late alternative is held to "types and
  images" where it paints; `painted` checks types alone.

### Batch 9: input

Files: `src/input`, `src/cascade/mod.rs`, `src/ui/mod.rs:1152-1167`,
`src/primitives/input/{input_event.rs, pointer.rs, interaction/scroll_delta.rs, scroll_targets}`,
`src/host/winit/input`.

- [ ] `input/capture.rs:34-43,116-138` with 22 outside field writes in `input/mod.rs` and
  `input/pointer.rs`: private fields on `Capture` and `Press`, and methods `break_run`,
  `travel_to`, `end_frame`, `is_pressed`, `pressed_on`, `has_edge`, `button_state(id, transform)`,
  `edges(button)`; `Input::pointer_actions` and `response_state` loop over `PointerButton::ALL`.
  `input/tests/response_state.rs:144-201` drives `begin_press`/`end_press` or feeds events.
- [ ] `input/mod.rs:971-980`, `ui/mod.rs:1152-1167`, `cascade/mod.rs:72-78,148-191`: `Ui` computes
  a `WidgetLocation` from `Input`'s cascade and hands it back. Target: delete `WidgetLocation`,
  `Cascade::locate` and `location`; `Cascade::entry(at: Endpoint)`, `endpoint_at(id, resolved_at)`;
  `Input::response_state(id, resolved_at)`.
- [ ] `input/mod.rs:596,880,896`: `next_frame` ("End the frame for input"),
  `drain_per_frame_queues` (touches no queue) and `end_frame`. Target: `admit_held`,
  `clear_edges`, `settle`.
- [ ] `input/mod.rs:555-592,861-866`: `ready(&mut self) -> bool` abandons a capture inside a
  predicate, and the lost-release cut is written twice more. Target: a pure `lost_release(&event)
  -> Option<PointerButton>` and `cut_gesture(button)`.
- [ ] `cascade/mod.rs:270-310,354-366`: `tab_stop_within` is `next_tab_stop`'s entry case again.
  Target: `TabDomain::Group` → `Within`; a private `stops_in(domain)`;
  `tab_stop_at(domain, end)`.
- [ ] One wheel turn's four lanes are spelled in `ScrollDelta`, `PointerEvent::Scroll` and
  `InputEvent::ScrollLines`, and enumerated by hand at four more sites. Target (API), as W3C's
  `deltaMode` and winit's `MouseScrollDelta`: an event carries one delta and its unit,
  `InputEvent::Scroll { delta: Vec2, unit: WheelUnit }` with `WheelUnit::{Pixels, Lines, Notches,
  Pages}`, replacing `ScrollPixels` and `ScrollLines`; the frame's sum is
  `WheelDelta { pixels, lines, notches, pages }` with `add(delta, unit)`, held by
  `PointerEvent::Scroll { pos, delta: WheelDelta }` and `ScrollDelta { wheel, zoom }`. The queue
  merges two events of one unit and keeps two units apart, as it does today.
- [ ] `host/winit/input/wheel_steps.rs:8-45`, `translator/mod.rs:52-53,155`: `vertical_lines:
  Option<f32>` uses `None` for "a page", and `Translator` reads the real platform for it. Target:
  `enum NotchStep { Lines(f32), Page }`, passed into `translate`.
- [ ] `input/input_queue/mod.rs:39-40,178-183`: `text` is reclaimed only when `pending` empties,
  with no stated bound. Compact past half the length, or state the bound.
- [ ] Names: `subs` and `*_subbed` → `watches`, `*_watched` (`input/mod.rs:96,496,690-801`);
  `Pointer::hovered` (the raw hit) → `under_pointer` and `hover_target()` → `hovered()`;
  `input::Arranged` beside `layout::cache::Arranged` → the latter `ArrangedRows`.

### Batch 10: value widgets

Files: `src/widget_core/value_response.rs`, `src/widgets/{bound_num, slider, drag_value,
value_area.rs, toggle_row, radio_group, combo_box, color_picker, expander, editable_label}`.

- [ ] `ValueResponse` is assembled by hand: the pick form at `toggle_row/mod.rs:145-149`,
  `radio_group/mod.rs:169-173,344-348`, `combo_box/mod.rs:225-229`; the gesture form at
  `slider/mod.rs:117-154`, `value_area.rs:88-109`. Target: `ValueResponse::pick(response,
  changed)` and `ValueResponse::gesture(response, pressed, keyed)`.
- [ ] `drag_value/mod.rs:285-291,427-441`, `slider/mod.rs:113-174`: two `commit_travel` fns with
  two return shapes; an arrow key at the bound gives `committed: true` on `DragValue` and `false`
  on `Slider`, `ColorField` and `ColorStrip`. Target: one `BoundNum::commit_travel(travel,
  keys: TravelRule, rule: StoreRule) -> bool` that returns whether the value moved, and one rule
  for `committed`, HTML's `change` (decided): a commit is of a change, so a key that moves
  nothing commits nothing. By the same rule a press and release that leave the value where it
  was commit nothing; `slider/mod.rs:117-120` and `value_area.rs:88` commit on any release today,
  so confirm that case with the owner before changing it. State the rule on
  `ValueResponse::committed` and in `src/widgets/AGENTS.md` beside the text rule, which differs
  on purpose. Test a key at a bound across all four widgets in one table.
- [ ] `expander/mod.rs:124-133,201-207,267-274`, `color_picker/mod.rs:158-177`: results leave
  record closures through pre-filled locals; return the closure's value. In `ColorPicker`: drop
  the `Inputs` re-bundle (`:152-199`), compute `gap` once (`:141,313,430,590`), fold the three
  gesture blocks (`:338-383`) into `Writes::gestured`.
- [ ] `editable_label/mod.rs:241,315-343`: `with_overrides` unpacks `TextStyleOverrides` into six
  setters that write the same struct. Target: `pub(crate) TextEdit::overrides(o)`.
- [ ] `widget_core/response.rs:29-49`, `widget_core/widget/tests.rs:31-33`: `widget_core`'s tests
  import the widgets to its right; move both groups to `src/widgets/tests/`. The file holds
  `InnerResponse` → `inner_response.rs`.

### Batch 11: overlays, tabs, dock, scroll, text edit

Files: `src/widgets/{popup, menu, menu_bar, tabs, dock, scroll, splitter, text_edit}`.

- [ ] `popup/trigger_state.rs:29-210`: `TriggerState<K>` carries a state-map key in the value type,
  at four hand-written impls and a `K` on `Settled` and `settle`; `MenuBar` holds one whose `K`
  means nothing. Target: plain `TriggerState { open, shown }` with derives; a private
  `TriggerRow<K>` owns the map access.
- [ ] `scroll/mod.rs:532-553,710-719`, `scroll/scroll_state.rs:260-272`,
  `tabs/tab_strip.rs:458-476`: reveal-pan three times, under two rules for a span longer than the
  view. Target: one `reveal_pan(span, offset, extent)` in `scroll_state.rs` that implements
  CSSOM's `nearest` (a span covering the view does not move; otherwise the edge already in view
  is aligned), with a table test of its six cases; `TabStrip` deletes `reveal_delta` and passes
  `.reveal(chip_id)` to its band. Check the strip against a golden. Decided: `nearest`, not
  "always the start"; the docs at `scroll/mod.rs:710-719` and `scroll_state.rs:260` are
  rewritten to it, and `src/widgets/AGENTS.md` records the rule with the CSSOM link.
- [ ] `tabs/tab_strip.rs:252-353`, `dock/dock_view.rs:263-305`, `dock/tab_drag.rs`: the drag
  lifecycle twice. Target: `TabCarry<K>` in `tabs/` with `begin`, `cancel`, `end_if_gone`,
  `release`.
- [ ] `tabs/tabbed_view.rs:218-291`, `tabs/tab_strip.rs:316,352,431-436`: `StripHit`, `ViewPass`
  and `TabbedViewResponse` copy the same four fields from a `Copy` `TabStripResponse`. Target: the
  closure returns the response; `ViewPass { strip, bound, inner }`; `StripHit` and `StripHits` go.
- [ ] `text_edit/shape_ctx.rs:17-35`, `text_layout.rs:13-103`, `view_state.rs:36,84-128`:
  `ShapeCtx` exists only inside a `TextLayout` and stores what the layout derives;
  `ViewState::wheel_axes` is read straight after it is written. Target: delete `shape_ctx.rs`;
  `TextLayout::run(text)`; `ViewState::update` returns `ViewUpdate { caret_animation,
  wheel_axes }`.
- [ ] `splitter/mod.rs:189-229,358-365`, `widgets/axis_keys.rs:122-129`: `Seam` built twice from
  the same four values; `KEYS_X` is `AxisKeys::SCROLL_X_ALONE`. Build once; one constant.
- [ ] `combo_box/mod.rs:78-79`: the comment says the activation keys add `TEXT`; they add `SPACE`
  and `ENTER`. `:268`: `i` → `idx`.
- [ ] Tests: `dock/tests.rs` builds corrupt trees through 27 reach-ins where the contract is
  `DockState::load(DockLayout)`; `Splitter::along` has no test.

### Batch 12: theme and `Ui`

Files: `src/theme`, `src/ui`, `palantir-derive/src/lib.rs`.

- [ ] `ui/frame_report.rs:42-57`: `FrameReport` has public fields, two `pub(crate)` fields and a
  fact as a method, and its derived `PartialEq` compares the private damage plan. Target (API):
  plain data `FrameReport { repaint_requested, repaint_at, earliest_repaint_at, paint }`;
  `WindowFrames::record` returns a crate-private `RecordedFrame { report, plan, processing }`.
- [ ] `ui/window_frames/mod.rs:256-258`, `frame_runtime/mod.rs:124-133`,
  `resources/app_settings.rs:31-34,113-150`: one frame-start snapshot is three fields filled from
  three resolvers that each recompute `power()`. Target: `AppSettings::resolve() ->
  ResolvedPreferences { scheme, reductions, transitions, frame_rate }`, one `FrameRuntime` field;
  drop `UiResources::color_scheme`.
- [ ] `ui/mod.rs:268-306`: thirty lines of theme and effect validation inside `set_theme`. Target:
  `Theme::check_effects(&self, effects) -> Result<(), ThemeEffectError>`.
- [ ] `theme/mod.rs:414-466` against `menu/mod.rs:62-76`, `popup.rs:42-50`, `tooltip.rs:57-62`,
  `modal.rs:57-67`: each float's ink is derived twice, from the palette and from the theme.
  Target: a private `try_dress(&mut self, float, ambient)` on each float bundle;
  `Theme::dress_floats` loops them.
- [ ] `theme/mod.rs:209-236,390-393,508-514`: `for_each_text(other: Option<&mut Self>, f)` is two
  operations with an `unreachable!`. Target: `each_text`, `each_text_mut`, `zip_text_mut`; the
  derive emits the third.
- [ ] `ui/window_frames/present/screenshot_lane.rs:53-55`: `requests` and `pending` state no
  bound, and k > 1 regions asked each frame grow `pending` by k − 1 readbacks a frame. Target: a
  stated `MAX_PENDING`; past it `request_screenshot` returns `None`.
- [ ] `ui/window_frames/present/mod.rs:253-256`: `finish_unpainted` appends the screenshot
  requests, then clears the destination. Clear both.
- [ ] `ui/theme_transition.rs:96-125,190-201`: the copy-on-write branches a held
  `ui.theme().clone()` drives have no test. One test holding the blend handle across a step, a
  third-theme set and the run's end.
- [ ] Stale docs: `theme/expander.rs:56`, `theme/text_edit.rs:26-28`, `theme/button.rs:48`,
  `theme/look/stateful_look.rs:7`.

### Batch 13: hosts, driver, derive

Files: `src/host`, `src/driver/locator.rs`, `palantir-derive/src`.

- [ ] `host/offscreen/mod.rs:91-185,274-358`, `host/harness/{harness_builder.rs:106-169,
  mod.rs:353-399}`, `host/headless_window.rs:26-30,196-201`: both headless hosts hold the same
  four closures and both builders the same nine setter bodies. Target: typed setters on
  `HeadlessWindow` and const setters on `HeadlessSettings`; each host forwards one line.
- [ ] `driver/locator.rs:19-34,123-133,233` (API): `Nth(Box<Self>, usize)` is valid around `Text`
  or `App` alone, `Id(x).nth(0)` reports a false count, and `offset` then `nth` fails where the
  reverse works. Target: the flat `Locator` of the proposal, with constructors `Locator::{text,
  id, point, app}` beside the `From` impls; `nth` on a target that names one thing (`Id`,
  `Point`) is `0` or an error that says so, and `NoNth` means out of range alone. The consumer's
  42 variant constructions become constructor calls. Test each message in one table.
- [ ] `host/winit/handle.rs:14`, `remote/mod.rs:23`: `handle` ↔ `remote`. Target:
  `winit/user_event.rs` with `UserEvent`, `MainTask`, `QueryTask`, `ReadTask`, `LoseTask`,
  `ShotAsk`.
- [ ] `host/winit/mod.rs:140-145,252-262,291-313,413`: `WinitHost` stores a `HostHandle`'s two
  fields apart and rebuilds one per call. Hold the handle.
- [ ] `host/winit/remote/remote_end.rs:21,35-82`: a shot ask a remote gave up on stays in
  `pending`, and each retry adds one. `ShotAsk` carries the `given_up` flag queries already use;
  state the bound.
- [ ] `host/winit/platform/{windows.rs:131-249, linux.rs:110-163}`: the open / closed handoff
  protocol twice. Target: `winit/handoff.rs` with `Handoff<T>::{file, close}`.
- [ ] `common/time/mod.rs:12-38`, `display/mod.rs:42,91`: a refresh rate of `Some(0)` also means
  unknown, handled in two places and documented in neither public setter. Normalize where it
  enters.
- [ ] `palantir-derive/src/lib.rs:119-602`: three of four derives expand inside a 736-line
  `lib.rs`; the field loops stop at the first error. Target: `lerp.rs`, `animatable.rs`,
  `theme_walk.rs`, `options.rs`; fold errors with `syn::Error::combine`.
- [ ] Tests: `host/host_core.rs:60-354` is 83 % test → `host_core/{mod.rs, tests.rs}`;
  `host/winit/platform/macos.rs:284-287` polls 1 ms up to 1 s where a channel would signal.

### Batch 14: GPU backend, device, surface

Files: `src/gpu/{wgpu_backend, frame, surface, resource, device}`, `src/host/host_device.rs`,
`src/host/winit/{mod.rs, runtime.rs}`.

- [ ] `host/host_device.rs:32-61`, `gpu/wgpu_backend/mod.rs:433-450`: `BackendSpec` is rebuilt
  from three backend accessors with one caller each. Target: `HostDevice { backend, spec }`;
  delete `WgpuBackend::frame_timing`; the store accessors become `internals`-gated.
- [ ] `gpu/wgpu_backend/render_stream.rs:42-118`, `wgpu_backend/mod.rs:301-342,1397-1401`,
  `frame/gpu_timings/wall_clock_tracer.rs:53-80`: the `GpuTimings` enum of the proposal; the six
  `if let Some(t)`, `encoder_mark` and six empty methods go.
- [ ] `gpu/surface/surface_manager/mod.rs:105,179-226`, `window_surface.rs:53-60`,
  `host/winit/runtime.rs:262-266,318-320,404,451`: the device's cap is passed back in at four
  sites, and pre-clamping is a doc contract that ends in a wgpu validation panic. Target:
  `SurfaceManager` keeps the cap; `WindowSurface::resize` clamps; `clamp_extent` is private.
- [ ] `gpu/frame/gpu_timings/query_tracer/mod.rs:78-119,371-392,512-575`,
  `resource/{map_state.rs:15-33, readback_staging.rs:13-95}`: four `Option`s set together, a
  buffer and its map state paired by hand, and four frame-record fields mirrored in `Cursor` and
  `Slot`. Target: `ReadBuffer { buffer, map }`; `Slot.stats: Option<SlotStats>`; one
  `FrameRecord` swapped at `finish`.
- [ ] `gpu/wgpu_backend/mod.rs:276-284,334-343,617-624`: `MainPass` stores three values of the
  `FrameTarget` beside it → `MainPass { target, clear, scissors }`.
- [ ] `gpu/surface/capture.rs:27-63,132-171`: `enum ChannelOrder { Rgba, Bgra }` with
  `of(format) -> Option<Self>` and a total `to_rgba`.
- [ ] Cycles: move `swapchain_mode` and `vsync_of` (`surface_manager/mod.rs:243-265`) into
  `window_surface.rs`; move `ColorLoad` (`frame/frame_passes/mod.rs:29-36`) into `schedule`.
- [ ] `gpu/resource/effect_image_groups.rs:42-102`: past `MAX_GROUPS` a new set evicts at most one,
  so the map never returns to its bound, against `src/gpu/AGENTS.md:251-252`. Evict two not drawn
  this frame; test the shrink.
- [ ] Hand-offs: `Gpu::requirements_met` (`device/gpu.rs:90-92`); `WgpuBackend::poll` (`:1304`);
  `OwnedTarget::new` rebuilding its `TargetSpec` (`surface/owned_target/mod.rs:35-54`);
  `ViewTarget` copying `AllocatedTarget` (`resource/gpu_view_targets/mod.rs:146-159`); the
  hand-written `Origin3d` (`resource/gpu_ctx.rs:106-110`); `GpuPaintRef.0` reached at
  `gpu_view_targets/mod.rs:84`.
- [ ] Free fns over a crate type: `texel::texture_2d` → `TargetSpec::texture`;
  `pipeline_recipe::pipeline_layout` → `Gpu::pipeline_layout`; `submission::frame_clear` →
  `RenderBuffer::frame_clear`.
- [ ] Tests: `resource/gpu_view_targets/view_target.rs:59-90` re-implements the `retain` loop it
  tests; the clear-a-texture fixture is written at `texel_readback.rs:113-132` and
  `owned_target/tests.rs:101-128`.

### Batch 15: GPU atlas, raster, pipelines, backdrop

Files: `src/gpu/{atlas, raster, pipeline, backdrop, resource/staging_target.rs}`, `src/common`.

- [ ] `gpu/raster/text_backend/encode/mod.rs:18-63`, `encode/cache/mod.rs:49-137`: `EncodedKey`
  keys on a color the template no longer bakes, so a color transition takes the full miss path
  each frame and leaves a dead row per run per frame for 30 frames. Target: `EncodedKey { text,
  scale_q, bins }`; the color rides `EncodedRunKey`. Fix the docs at `mod.rs:18-21` and
  `cache/mod.rs:304`. Confirm first that no glyph raster key depends on the run's color.
- [ ] `gpu/backdrop/backdrop_blur/mod.rs:426-466,517-534,630-690,1165-1174`,
  `resource/staging_target.rs:12,55-62,130-168`: the window step-down is written once for
  `StagingTarget` and three more times in the blur; `GROWTH_GRANULE` is declared twice. Target:
  `resource::decaying_texture::DecayingTexture` with `reserve` and `decay`.
- [ ] `gpu/atlas/mod.rs:53-66,215-226`, `raster/raster_atlas/mod.rs:79`,
  `pipeline/quad_pipeline/cutout_placements.rs:26`, `backdrop/backdrops/mod.rs:254-257,561-588`:
  evictions come back as bare handles, so each tenant keeps a reverse map. Target, as WebRender's
  `CacheEntry.user_data`: `allocate(format, tenant, size, tag: u32)` and
  `Evicted { handle, tag }`; the same for `acquire_scratch`. Check `Entry` stays 32 bytes, or the
  tag moves to a parallel column.
- [ ] `gpu/atlas/page_list/mod.rs` (1170 lines, twelve types), `atlas/scratch_cache.rs:55-59,
  163-242`: the generational slab is hand-written in `PageList`, `ScratchCache` and `RasterAtlas`.
  Target: `common::gen_slab::GenSlab<T>` with `slotmap`'s names and guarantee (`insert`,
  `remove`, `get`, `get_mut`, `contains_key`; a removed key never resolves again), tested against
  a reuse-after-remove case; `Entry` becomes the held payload alone, so a free slot holds no
  stale `last_use`, `pins` or `tenant`; `page_list/{page.rs, move_budget.rs}`. Refused: the
  `slotmap` dependency, whose key has no room for the handle's format.
- [ ] `gpu/atlas/page_list/mod.rs:306,420-423,820-874`, `atlas/mod.rs:229-256`: `ScratchCache`
  sits inside `PageList` and is driven from `AtlasPool` → `FormatPool { pages, scratch }`.
- [ ] `gpu/atlas/scratch_cache.rs:88-142`: scratch textures have no byte bound; each backdrop past
  half a page takes its own `Rgba16F` texture (about 24 MiB at 1936 × 1088) against a 32 MiB soft
  budget. State the bound in the doc and `src/gpu/AGENTS.md`, or return `AtlasError::Full` past a
  hard multiple.
- [ ] `gpu/atlas/atlas_textures/move_draw.rs:98-102,195-215`: a hand-rolled buffer kept at its
  high-water mark, against `src/gpu/AGENTS.md:251-254` → `DynamicBuffer<MoveRect>`.
- [ ] `gpu/pipeline/instanced_pipeline.rs:14-59`: a wrapper three of six families use, with
  `FrameVariant` and `VariantTarget` in its file. Drop it; `pipeline/frame_variant.rs`;
  `VariantTarget { format, variant }`.
- [ ] `gpu/backdrop/backdrop_blur/mod.rs:174-565`: five `filters: bool` where `Filtering` exists;
  the format test twice, the capture lookup four times beside `capture_of`.
- [ ] `backdrop_blur/mod.rs:96-123,590-610`: `BlurJob.layout` is an output inside an input →
  `BackdropBlur.layouts` and `prepare(&[BlurJob])`.
- [ ] `raster/raster_atlas/atlas_slot.rs:12-67`: `SlotPlacement` flattens `Allocation` and
  rebuilds a `Placement` → `{ raster, placement }`. `raster_atlas/mod.rs:85-288`: "unplaced" and
  "unallocated" name one entry; `drop_evicted` calls `drop_slot`.
- [ ] `pipeline/quad_pipeline/cutout_tables/mod.rs:322-328`: location `9` is
  `SHADOW_INSTANCE_ATTRS.len()`; derive it. `raster_pipeline/raster_quad.rs:176-238`: the spun
  attributes retype five of the flat ones (run the visual suite).
- [ ] `pipeline/quad_pipeline/cutout_plan/mod.rs:55-65`, `quad_draw/mod.rs:19-41`: `ShadowEntry`
  has two orders and a typed-in `COUNT` → `ALL` and `COUNT = ALL.len()` in `quad_draw`.
- [ ] `raster/raster_pass.rs:38-47`: `RasterPassConfig` carries five fields the tenant decides →
  `RasterPass::new(gpu, Tenant)`.
- [ ] `backdrop/backdrops/mod.rs:244-655`: the tuple `(RenderOwnerId, BackdropKey)` six times →
  `ResultKey`; `slot_for` returns `Kept`.
- [ ] `gpu/backdrop/mod.rs:25-36,95-100`: delete the superseded atlas wording.
- [ ] Small: `atlas/mod.rs:122-125` passes the raw frame; `backdrop_blur/mod.rs:390-394` builds a
  second identical pipeline layout; `shape_template.rs:89` and `page_list/mod.rs:622-623` use
  tuples.
- [ ] Test mods past the split rule: `pipeline/shape_template.rs`,
  `effect_pipeline/wgpu_effect_store.rs`, `quad_pipeline/cutout_placements.rs`,
  `atlas/scratch_cache.rs`, `quad_pipeline/mod.rs`.
- [ ] `pipeline/quad_pipeline/cutout_tables/tests`:
  `a_baked_table_holds_the_cutout_it_tabulates` runs 1.01 s inside the suite (0.79 s alone), on
  the 1 s limit. Tabulate a smaller table or fewer samples for the same property.

### Batch 16: renderer and damage

Files: `src/renderer`, `src/damage`, `src/internals/module_order.rs`,
`src/ui/window_frames/present/mod.rs:474-477`, `src/gpu/resource/gpu_view_targets`.

- [ ] `renderer/render_buffer/backdrop_row.rs:10`: the GPU contract imports the frontend's
  `GpuFill` → `render_buffer/gpu_fill.rs`.
- [ ] `renderer/render_buffer/mod.rs:62,132-138`, `ui/window_frames/present/mod.rs:474-477`:
  `live_targets` is written by `ui` from a `GpuViews` the consumer also receives. Target:
  `GpuViewTargets` builds the roster from `callbacks`; `Frontend.buffer` becomes private.
- [ ] `renderer/frontend/composer/paint_order.rs:42-48,282-387`: `Admitted.scissor` is read at two
  of eight sites, where it equals `clip.scissor()`. `order` and `run_scissor` return `()`;
  `admit` returns `Option<URect>`. Check the inlined codegen at `:312-314`.
- [ ] `renderer/frontend/paint_sink/mod.rs:112-169` and the six `faded` impls: every payload
  carries `spin: None`, and six early-outs test `by == 1.0`. The gate tests the fade once;
  constructors own the `None`; `draw(payload, animation: DrawAnimation)` with `STILL` at both
  sites.
- [ ] `renderer/backdrop_captures/mod.rs:75-79`: `BackdropTicket { capture, captures: bool }` →
  `enum TicketResult { Held, New }`.
- [ ] `renderer/frontend/encoder/plane_ctx.rs:73-92`: thirteen fields, three of them slices of one
  `PlaneCascade`, under a doc that says "fixed" → `cascade: &PlaneCascade` and one `EncodeFrame`.
- [ ] `compose_session.rs:734-748`: inline the one-caller `list_backdrop`.
  `composer/curve_run.rs:28-41`: `CurveRun::turned` → `CurveBasis::turned`.
  `render_buffer/image_flags.rs`: use `flag_set!`.
- [ ] `payload/draw_stroke_payload.rs:20,55-61`: shipped `Default` impls for test literals → a
  gated `fixture()`.
- [ ] `damage/engine.rs:28-47,243-280`: fields `pub(super)` for tests beside gated accessors →
  private fields, one reach-in each. `:37,39` with `root_order.rs:18-23`: one `InversionScratch`.
- [ ] `damage/sweep` (about 4k lines with tests), `internals/module_order.rs:143-150`: a bench
  study exempted from the module order by the literal `"sweep"`, importing `ui`, `host` and
  `widgets`. Target (decided): `src/damage/bench/{mod.rs, sweep/}`, with `ships()` treating a
  `bench` directory as `bench.rs` and the `"sweep"` literal gone; `the_scan_finds_an_edge…`
  gains a case that a shipped folder named `sweep` is held to the order. Refused: under
  `ui/window_frames/present/bench`, which takes the damage study out of `damage`.
- [ ] `damage/sweep/policy.rs:230-257`: a second strip `union_area` beside
  `damage/region/mod.rs:95-130` → one in `primitives::geometry`.
- [ ] Tests: `PlaneBackdrop` fixtures built by hand in three places → one gated
  `PlaneBackdrop::fixture`; `damage/tests/arena.rs:44-52,150-163,242` asserts arena layout.

### Batch 17: layout and cascade

Files: `src/layout`, `src/cascade`.

- [ ] `layout/drivers/grid/mod.rs:191-235`, `measure.rs:17,248,279`, `arrange.rs:12`: track
  predicates are free fns over `&[Track]`, and `GridAxis`'s impl is spread over three files →
  `grid/grid_axis.rs` with one `impl`.
- [ ] `grid/mod.rs:149-167`, `measure.rs:66-77,148-189`: the two-round span distribution three
  times, and a grid with no span still walks its children up to eight times for it. Target:
  `Grid::spans(tree, axis)` and a `has_spans` bit decided once. Measure the miss path.
- [ ] `cascade/engine.rs:627-747`: 120 lines of backdrop listing on `TreeSource` →
  `cascade/plane_backdrop.rs` as `PlaneCascade::list_backdrops`.
- [ ] Cycles: `UNMEASURED` and `same_offer` move into `measure_columns.rs`;
  `PlaneLayout::outside_input` (`plane_layout.rs:37-40`) and the inline dispatch at
  `pass.rs:300-305` become one fn in `drivers/mod.rs`.
- [ ] `layout/pass.rs:127-132`: `grid_mut` hands out a bag where all fifteen uses take
  `.track_state` → `grid_tracks_mut`; drop the stale sentence.
- [ ] Names: the grid's `AxisSlot` → `TrackSlot`; `ArenaSnapshot` → `SubtreeDesc`, `snapshots` →
  `desc_by_id` (`layout/cache/measure_snapshot.rs`).
- [ ] Small: `layout/engine.rs:150-155` builds a `PlacedChild` `pass.placed(root)` returns;
  `drivers/scroll/mod.rs:88-99` rebuilds `content`; `pass.rs:209-264` writes three columns field
  by field; `pass.rs:381-386` repeats `zero_subtree`; `grid/measure.rs:117,170-174` spell one
  loop two ways; `input/scope.rs:64` → `CascadeKey::same_structure`.

### Batch 18: shape and text

Files: `src/shape`, `src/text`, `src/scene/forest.rs:252-261`.

- [ ] `shape/lower/mod.rs:56-147`: four free fns on `&mut RecordStore` and a misplaced
  `LoweredPaint`, closing three cycles with `record_store` and `paint`. Target:
  `RecordStore::{lower_brush, lower_ramp, lower_paint, lower_chrome}`;
  `shape/paint/lowered_paint.rs`; `LoweredMaterial::lower(material, resolved, params: Span)`.
- [ ] `shape/{rect.rs:81-133, circle.rs:58-110, triangle.rs:85-146, image_paint.rs:90-130,
  effect_paint.rs:164-206}`: fifteen `DrawWith` impls differ in the geometry alone. Target: a
  crate-private `ClosedShape` and five blanket impls; the public `PaintedBy` impls stay one per
  pair. Compile the coherence first.
- [ ] `shape/arc.rs:15-72`, `triangle.rs:26-48`, `rect.rs:60`, `circle.rs:35,143`: each builder
  restates its geometry struct → the builder holds its geometry.
- [ ] `shape/mod.rs:183` (API): `Shape::text` takes an `InternedStr`, so 9 of 11 callers intern on
  the line before → `impl Into<TextInput<'a>>`.
- [ ] `shape/{triangle.rs:24, polyline.rs:20, mesh.rs:16, text.rs:19, effect_paint.rs:45}` (API):
  four shapes of `Copy` fields are `Clone` alone and `EffectPaint` is neither → `Copy` on the
  shapes, `Clone` on `EffectPaint`.
- [ ] `shape/shapes/mod.rs:69-86`: `add_gpu_view` is a second place a record is made →
  `ShapeRecord::gpu_view(id, epoch)` through `tree.push_shape`.
- [ ] `shape/paint/lowered_shadow.rs:70`: `paint_rect_local` takes an `Option` each caller
  resolved; the fallback is spelled at three more sites → `paint_rect(source)` and one helper.
- [ ] `shape/geometry/triangle_geometry/mod.rs:103-127`: `CornerCenters` is `PlacedTriangle` →
  `PlacedTriangle::rounded`.
- [ ] `shape/paint/blur_plan`: nothing in `shape` uses it → its leftmost user, `cascade`.
- [ ] Stale docs: `text/run.rs:2-24`, `text/shaped_ref.rs:25,31`, `widgets/text.rs:168`,
  `widgets/text_edit/mod.rs:406`, `shape/record/mod.rs:314`.
- [ ] Tests: the "record this pair" fixture is rebuilt at six sites → one helper in
  `shape::internals`; `shapes/tests.rs:261-527` tests builders, not `Shapes`.

### Batch 19: scene and the leaf modules

Files: `src/scene`, `src/diagnostics`, `src/display`, `src/icons`, `src/image_registry`.

- [ ] `scene/tree/recording_scratch.rs:24-46`, `forest.rs:277-311`, `tree/mod.rs:469-480`,
  `root_slot.rs:14-18`: the `PendingRoot` and `RootSlot.placement` of the proposal; one
  `if parent_frame.is_none()` block in `open_node`.
- [ ] `scene/forest.rs:190-259`: `assert_node_open` duplicates the `expect` after it and names the
  wrong function → the `expect` alone, with its own message.
- [ ] `scene/scrollbars_def.rs:12,92-96`: `ResolvedScrollbarsDef` →
  `scene/tree/resolved_scrollbars_def.rs`.
- [ ] `diagnostics/gpu_pass_stats.rs:207-314`: nine assign-only setters and four tests of
  assignment → `pub(crate)` fields; keep the public `last_*` getters.
- [ ] `display/mod.rs:25-28`: `scale_factor_is_valid` re-checks what the two scale types
  guarantee → a const assertion beside the types.
- [ ] `icons/icon_table.rs:124-150`: an intermediate `Vec` filled in one loop and drained in the
  next → one loop.
- [ ] `image_registry/error.rs:1`: stale module doc.

### Batch 20: primitives and common

Files: `src/primitives`, `src/common`, `src/effect/stock`, `AGENTS.md`.

- [ ] `primitives/paint/material/mod.rs:191-210`, `effect/stock/mod.rs:25-50`: the module moves
  under "Module layout" above; add `effect` to the order line in `AGENTS.md`.
- [ ] `primitives/geometry/corners/mod.rs:36-38,147-152`, `layout/sizing.rs:76-77`,
  `math/domain/mod.rs:184-231`: a multiplier is validated as a length (`Sizing::fill(-1.0)` panics
  with "a length must be finite"), and "a length that is a lane" has four names. Target:
  `domain::factor` at both sites; one `domain::is_length_lane`.
- [ ] `primitives/layout/align.rs:194-230`, `scene/node/mod.rs:369-379`: "`Auto` falls back" twice,
  and twin helpers on `HAlign`/`VAlign` → `Align::or(fallback)` and `Align::axis(axis)`.
- [ ] `primitives/paint/material/mod.rs:364-481,619-670`: 200 of 673 lines are a contrast solver
  `theme` imports on its own → `material/ink_bound.rs`.
- [ ] `primitives/paint/material/glass_input.rs:47-65`: field offsets 0 to 24 as literals over
  `Lens`'s layout → `core::mem::offset_of!`.
- [ ] `primitives/paint/brush/gradient/stops/mod.rs:116-177`: `GradientStopsBuilder` has one
  caller and a stale doc → fold into `GradientStops::new`; one `collect_capped` for it and
  `ColorRamp::evenly`.
- [ ] `primitives/layout/layout_mode.rs:21-85`: `GridDefId` and `ScrollbarsDefId` are the same 25
  lines → one `TableIdx<Tag>` or a private macro.
- [ ] Dead: `common/sync.rs:30-46` `try_lock_unpoisoned`; `geometry/rect/aabb/mod.rs:21-36`
  `Aabb::of_iter`.
- [ ] Placement: `identity/typed_stores.rs` → `common`; `layout/visibility/mod.rs` →
  `visibility.rs`; `common/eviction_clock.rs` named for `ClockSweep`; `common/non_max.rs:40`
  `index(idx)` → `from_idx`; `input/policy.rs`, `paint/image/mod.rs`, `effect/effect_kind.rs`
  each hold unrelated types.
- [ ] Tests: `turned_cover`, `spun_cover` and `spun_radius` (`geometry/rect/mod.rs:215-241`, six
  production callers) have no direct test; the material tests in `primitives`
  (`paint/material/tests.rs:32-340`) drive `shape`'s lowering; `effect/tests.rs` (2371 lines)
  mixes GPU render tests with contract tests; four `Shortcut::matches` tests are one table.

### Batch 21: showcase

Files: `palantir-showcase`, `src/bench/scene.rs`.

- [ ] `tests/visual/theme/{dark.rs, light.rs}` (981 and 975 lines): with the hex literals
  normalized the two differ in nine hunks. Target (decided): one `fn theme(p: &Palette, motion)
  -> Theme` in `tests/visual/theme/mod.rs`, a literal in structure, over two suite-owned
  palettes, `FIXTURE_PALETTE` and a light twin; the sentence at `visual_rig.rs:21-22` is
  rewritten, as the palette is now the theme's source. Land it with no golden changed: run
  `cargo visual` before and after, and a moved golden means the function is not the literal.
- [ ] `src/pages/controls.rs:36-48,459-469`, `src/bench.rs:14-41`, `src/bench/scene.rs:39-40`: the
  first page holds bench code, against `AGENTS.md` "the page holds no bench code". Drop
  `Scene::field_len`; the typing arm confirms its key inside palantir; open the bench page by
  name.
- [ ] `src/lib.rs:17,23`: `pages` and `support` are `pub` for two visual fixtures, which rebuild
  the shell's page wrapper and a kit by hand. Target: one gated `internals::page(ui, page,
  palette)` on `Shell::card`'s code path.
- [ ] `src/shot/script.rs:124-268`: the instruction set is spelled as strings in four places →
  `enum Instruction` with `ALL`, `FromStr`, `is_header()`; one spelling for consuming a peeked
  word (`:252,256,481,496`).
- [ ] `src/shot/mod.rs:6-7,113-115,343-375`: `--against` behind `golden` gives a twin function,
  an `unreachable!` and a run-time refusal, though the showcase depends on `image` anyway. Depend
  on `golden` outright; the cost is that no lane builds `winit` without `golden`.
- [ ] `src/support.rs` (543 lines, eleven types) beside `src/scaffold/` → one scaffold module, a
  file per type; `shot_tests.rs` → `shot/tests.rs`; `Kits` holds two `Swatches`.
- [ ] `clippy.toml` at the root applies palantir's internal layering to the consumer crate, at
  thirteen `#[expect]`s → a `palantir-showcase/clippy.toml` with what binds a consumer.
- [ ] `src/shell.rs:418-432,904`: `Shell::open(page)` and `ui.set_preferences(Shell::PREFERENCES)`
  are one act; the bench and alloc scenes skip the second. The shell applies its preferences on
  its first record; `PAGES[20]` by name.
- [ ] `src/scaffold/rail.rs:261-287`: a hand loop where `StatefulLook::each_mut()` exists; a
  six-parameter `Segmented::row` → `PreferenceRow`.
- [ ] `src/support.rs:57-80`, `src/appearance.rs:33-93`: three per-scheme pairs, each with its own
  `match` → one `PerScheme<T>`.
- [ ] `src/pages/custom_widget.rs:333-340` picks a look by hand and never shows the disabled one
  where `StatefulLook::pick` exists.
- [ ] Tuples: `examples/reel/{script.rs:110-143, main.rs:240,275, downscale.rs:83}`,
  `src/cube_renderer.rs:80`.
- [ ] Tests: `tests/visual/fixtures/{clip.rs:23-34, image.rs:38-54, icon.rs:81-108,
  shadow.rs:27-33}` copy `cell` and `render`; `VisualRig.physical: Option<UVec2>` with an
  `expect` → `VisualRig::new(size)`; `MadeImage`'s refill is tested twice through pages
  (`pages/motion.rs:516-543`, `pages/images.rs:366-392`) and never in `made_image.rs`;
  `tests/alloc/harness/mod.rs:38-40,165` and `tests/alloc/gates/{mod.rs:37-52,
  backdrop.rs:63-81}` repeat their setup.
- [ ] Wording: `src/pages/custom_widget.rs:3`; `src/shell.rs:1` ("chrome", which `AGENTS.md`
  refuses for scaffolding).

### Batch 22: internal read-back image type

Files: `src/gpu/surface/owned_target/mod.rs:144`, `src/host/harness/mod.rs:1086`,
`palantir-showcase/tests/visual/goldens.rs`. After batch 5.

- [ ] Read-backs return `image::RgbaImage` while `golden` and `Screenshot` use `palantir::Image`,
  so `goldens.rs:30-37` clones 4·w·h bytes per assertion. Read-backs return `Image`; `to_image`
  goes.
