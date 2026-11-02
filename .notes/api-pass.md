# API pass

The public API changes palantir waits on, each needing a go-ahead (AGENTS.md, "Public API"). The
design is in Part 1. The batches that apply it are in Part 2. Whoever applies an item deletes it,
updates every caller in palantir and darkroom in the same batch, and regenerates `API_SURFACE.md`
with `cargo drive surface`.

Sources: a review of the whole exported surface in four parts (widgets; overlays, menus, tabs and
dock; `Ui`, input, hosts and players; primitives, paint, text, theme and GPU), with caller counts
taken across palantir and darkroom, and the items this file held before. Every earlier item is
either in a batch below or dropped under "Refused" with its reason.

## Part 1: Design

The surface is mostly sound. Its defects come from a few missing family rules: siblings that each
answer one question their own way, a second spelling kept beside a newer one, and plain types where
a checked or set type exists. Each rule below removes one cause. A violation of a rule after this
pass is a bug.

### Rules

- **One way to do one thing.** A second spelling stays only when it is shorter and clearer at the
  call site and has no behavior of its own (`Corners::from(4.0)`, `Shortcut: FromStr`). Refused: a
  builder beside a constructor that already takes every argument (`GradientBuilder`), a setter that
  repeats a `Configure` setter (`Separator::thickness`), a free function that repeats a variant
  (`Action::key`).
- **State a caller drives from outside is reached by the widget's own id, in one of two
  vocabularies.** An overlay or a disclosure: `open(ui, id)`, `close(ui, id)`, `is_open(ui, id)`.
  A field with an editor of its own: `start_editing(ui, id)`, `stop_editing(ui, id, EditEnd)`,
  `is_editing(ui, id)`, as GTK4's `EditableLabel` names them
  ([GTK4 `EditableLabel`](https://docs.gtk.org/gtk4/class.EditableLabel.html)), with `EditEnd` in
  place of GTK's `commit: bool` by the crate's enum-over-flag rule. Every widget that holds such state
  has the full set, and the state row is keyed by the widget's own type, so a base type's function
  is not a second route to it. Refused: a `&mut bool` setter (`Expander::open`), the verb `edit`
  beside `is_editing`, and a row reachable through `PopupTrigger::open(ui, combo_id)`.
- **Every overlay can be closed from outside.** `close(ui, id)` gives back the input scope at once.
  An overlay that stops recording without it keeps its scope one frame, which drops the next key.
- **One menu type, two placements.** Every menu, whatever drops it, is one type with two
  constructors: at the pointer on a secondary click, and below a trigger that a click toggles, as
  egui's `Popup::context_menu` and `Popup::menu` are
  ([egui `Popup`](https://docs.rs/egui/latest/egui/containers/struct.Popup.html)). ComboBox, the
  tab overflow and MenuBar wrap it. A menu row closes the overlay it sits in through the scope, as
  egui's menus close on a click by default, so no row takes a `CloseHandle`. Its name is `Menu`
  (decided, 10). A menu no one widget raises, as darkroom's canvas menus, keeps its open state
  with the caller through `Menu::at(point)`, as egui's `Popup::open_bool` does (decided,
  2026-11-02). Where a keyboard open lands is `MenuLanding` (first, last, or a row), so a combo
  list opens on its pick as the APG's Down does; `TabEnd` stays crate-private. Refused:
  publishing `PopupTrigger::menu_look` and `layer`, which gives `PopupTrigger` two theme setters;
  darkroom's canvas menus on `Menu::below`, as they open at a point the canvas picks.
- **A trigger-driven overlay returns `Option<OverlayResponse<R>>`**, `None` while closed, as egui's
  `Popup::show` returns `Option<InnerResponse<R>>`, so `OverlayResponse::id` always names a node.
  How it closed is one field, `close: Option<OverlayClose>` with `Dismissed` and `Requested`, which
  no longer collides with the window's `Ui::close_requested`. `Menu::show` keeps the `Option` for a
  menu `Menu::at` a point too, which always answers `Some`, so one menu type has one `show`.
- **A part id has one word per overlay kind**, published as a `*_id` function: `popup_id` for a
  popup, `menu_id` for a menu, `bubble_id` for a tooltip. A combo box's list is a menu, so it is
  `ComboBox::menu_id`. Refused: one word for every overlay, as
  one trigger may hold a dropdown and a context menu at once.
- **A widget that drops a menu has `menu_style`**, the `<slot>_style` rule applied to the menu's
  theme: ComboBox, MenuBar, TabStrip, TabbedView, DockView.
- **A widget that shows one run of text carries Text's eight axis setters** (`color`, `font_size`,
  `line_height_factor`, `family`, `weight`, `slant`, `bold`, `italic`), stored as
  `TextStyleOverrides`.
- **A pick binds a value on every picking widget**, as RadioButton binds `&mut T` against a value
  and egui's `selectable_value` writes the picked value (decided, 1).
- **A bound number is any of the crate's scalars**: `BoundNum` takes `f32`, `f64`, `i32` and `i64`,
  as `f32` is the crate's own scalar. A closed set of scalars is an enum by the coding rules; egui
  reaches the same reach with its `Numeric` trait.
- **An edit a widget cannot perform itself is opt-in.** A tab is closable and draggable only when
  the caller says so, unless the widget applies the edit (the dock).
- **A one-word input verb is the whole gesture**; its halves are named, as Playwright's keyboard
  names them ([Playwright `Keyboard`](https://playwright.dev/docs/api/class-keyboard)): `key`
  presses and releases (Playwright's `press`), `key_down` and `key_up` are the halves, `type_text`
  presses a key per character (`type`), and `insert_text` commits text with no key event
  (`insertText`). The harness and `Action` carry the same four, under the same names
  (`Action::Key`, `KeyDown`, `KeyUp`, `Type`, `InsertText`), so a step means one thing on both
  players.
- **A fact the system reports is a host setter, seeded by the builder under the same name**: system
  scale, refresh rate, color scheme, reduced motion. App state, such as the user scale, is set
  through `Ui` alone. Every host exposes the same set with the same names.
- **`request_*` asks for this frame only; `set_*` is retained**: `request_repaint`,
  `request_screenshot`, `request_ime`, and `request_cursor` in place of `set_cursor`. egui spells a
  frame's cursor `set_cursor_icon` beside `request_repaint`; the crate's own pair is the stronger
  rule, as the name alone then says whether a call must repeat each frame.
- **A checked type where one exists, at every door.** A pinch takes `ZoomFactor`, so
  `PlayError::BadPinch` and the pinch assert go. A conversion that can fail is `TryFrom`, never a
  `From` that panics, as std's `From` docs require
  ([`From`](https://doc.rust-lang.org/std/convert/trait.From.html)). A wait is a `Duration`, never
  `f32` seconds or an `Instant`.
- **A set of flags is `flag_set!`**: `Modifiers`, `ShortcutMods`, `AllowedSplits`, as winit's
  `ModifiersState` is a bitflags set and the crate's own `Sense` and `KeyFilter` are. Refused: a
  struct of `bool`s with combined constants (`CTRL_SHIFT`), egui's shape, which needs `BitOr`
  written by hand and still has no `contains`.
- **A value's `with_*` setters cover every field its constructors do not take**
  (`Chrome::with_corners`, `with_fill`), and take the same `impl Into` its constructors take.
- **`Default` is a value's neutral.** A value whose neutral is `NONE` or `ZERO` defaults to it, so
  `Lens::default`, a window-glass recipe, becomes the named `Lens::WINDOW_GLASS`. A theme part has none
  (decided, 9).
- **One word, one meaning, across the crate.** A scrim is a `scrim`, as Material names the layer
  under a dialog, a blur a `BackdropFilter`; a
  pinch is a `Pinch`, the view's scale a zoom; a border is a `Stroke`; a split rule's states are
  `rule_hovered` and `rule_active`, as the scrollbar's are `thumb_*`.
- **No mirror of a wgpu type.** `DeviceFeatures`, `SurfaceUsages` and `PowerPreference` go: the
  signatures that take them name wgpu anyway, and `pub use wgpu` serves the caller.
- **Nothing public without a caller outside the crate's own tests**, unless a documented app use
  needs it. Plumbing a widget needs is `pub(crate)`.

### Decided

Each choice was taken on 2026-11-02 with the owner; the refused option and the field's practice
follow it.

1. **A pick binds a value**: ComboBox takes `&mut T` among `&[T]`, as RadioButton does, and
   darkroom's `offered_idx` goes. egui binds the value (`selectable_value`); iced's `pick_list` takes
   the selected value. Refused: an index on both, which maps every RadioButton value to a position;
   an index form beside the value form, as egui's `show_index`, a second way to pick.
2. **A box shape starts at its owner's rect and takes `.at(rect)`**: `Shape::rect(r)` becomes
   `Shape::rect().at(r)`, and `owner_rect` and `owner_windowed_rect` go. A shape records before
   layout, so the owner rect is the one place every box shape can default to, as a SwiftUI
   `Rectangle` fills the frame of its view and moves by modifier
   ([SwiftUI `Rectangle`](https://developer.apple.com/documentation/swiftui/rectangle)). Refused:
   the rect in every constructor with an `owner_*` twin each, as epaint's `RectShape::new` and
   Flutter's `drawRect` take it, which doubles the constructors.
3. **`Configure::pos` renamed `at`**, one placement word for nodes and shapes, as the naming rule
   asks. Refused: `pos` as an exception to the rule, egui's `*_pos` spelling.
4. **The system scale is a host setter on every host**: `OffscreenHost::set_system_scale` and
   `set_refresh_millihertz`, seeded by its builder, and `frame(target, app)`. Refused: the scale per
   frame, as egui's `RawInput` carries it, which leaves one host unlike the other two.
5. **Every gradient's `two_stop` takes its geometry** (`RadialGradient::two_stop(center, radius,
   c0, c1)`), as peniko's `Gradient::new_radial(center, radius)` takes it before the stops
   ([peniko](https://docs.rs/peniko/latest/peniko/struct.Gradient.html)). Refused: `centered(c0,
   c1)` for the kinds that hide a geometry, two names in one family.
6. **The glyph plumbing goes** (`TextShaper::glyphs`, `TextGlyphs`, `GlyphLine`, `PlacedGlyph`,
   `GlyphRasterKey`, `RasterImage`, `ContentType`): no caller anywhere, and a public item removed
   later breaks callers. Refused: keeping it with an example that labels a `GpuView`, until a view
   asks for a designed API.
7. **`IconTable::baked` and `IconDefinition` go**: no generator exists. Refused: shipping the baker
   for a path no app uses.
8. **`FontSource` takes bytes only**: `File`, the path `From`s and `FontLoadError::Io` go, as the
   workspace rule says a library takes bytes, readers and writers, never paths. The system fonts
   keep fontdb's memory-mapped path loading inside `FontScope::System`. Refused: keeping the path
   form for memory mapping, which fontdb's `load_font_file` offers.
9. **No theme part has `Default`**: every bundle, `Theme` and `TextStyle` name their scheme
   (`Theme::stock(scheme)`), as a part taken by default inside a light theme paints dark ink, and
   Rust's API guidelines ask for `Default` only where a reasonable default exists
   ([C-COMMON-TRAITS](https://rust-lang.github.io/api-guidelines/interoperability.html)). Refused:
   `Theme` alone keeping a dark `Default`, as egui's `Visuals` does.
10. **`ContextMenu` renamed `Menu`**, with `Menu::context(snapshot)` at the pointer and
    `Menu::below(snapshot)` from a trigger, `MenuTheme` and `Theme::menu`: a combo's list and a menu
    bar's menu are not context menus, and egui names the pair `Popup::menu` and
    `Popup::context_menu`. Refused: `ContextMenu::below`, which calls a dropdown a context menu.

### Refused

- **The harness reopens its own device after a loss.** It contradicts the caller-supplied `Gpu` and
  "`Gpu` owns the device's health". The harness takes `recover(gpu)`, as `OffscreenHost` does.
- **`TabStrip::drop_caret` made public.** The strip reports its own reorder (`TabMove`), so a caller
  needs no caret.
- **"The rest of `ThemeDefaults` a widget needs".** No widget needs another default; only
  `default_line_gap`, the sibling of `default_gap`, is added.
- **Renaming `Display` and `Waker`**, which shadow std names under a glob import. The crate names
  them for its domain, and no caller globs both.

## Part 2: Plan

Batches run most harmful first. A batch changes palantir and darkroom together and leaves both
gates green, then regenerates `API_SURFACE.md`. A rendering change also runs `cargo drive run
visual`. Counts are call sites outside the defining file: palantir / darkroom.

### Batch 5: Value widgets

- [ ] `DragNum` renamed `BoundNum`, with `Num` and `StoreRule` in its module, and `F32` and `I32`
  variants with their `From<&mut f32>` and `From<&mut i32>`; a stored `f32` rounds after the snap.
  About 30 `as f32` casts in the showcase go.
- [ ] ComboBox binds a value (decided, 1): `new(selected: &'a mut T, options: &'a [T]) where T:
  PartialEq + AsRef<str>` and `labeled(selected, options, label)`; a value not in `options` shows
  `label(selected)`. Callers: 10 / 2; darkroom's `offered_idx` goes.
- [ ] `DragValue::editable` defaults to `true`, as `EditableLabel::editable` does. Callers: 3 / 2
  drop `.editable(true)`.
- [ ] `ColorPicker::history(bool)` and `swatches(&[RgbaF32])` set one choice two ways. Target:
  `swatches(SwatchRow<'a>)` with `None`, `History`, `Fixed(&'a [RgbaF32])`.
- [ ] `ColorButton::style` overwrites the style of the picker handed in. Target: it styles the chip
  and the popup only.
- [ ] `EditableLabel`: Text's eight axis setters; `EditableLabelResponse` gains `committed` and
  `ended: Option<EditEnd>`, as `TextEditResponse` has. Darkroom's bold `FlatTextEdit` goes.
- [ ] One key map per role in `axis_keys`: a slider map for Slider and ColorStrip, a spin-button map
  with Page Up/Down and Home/End for DragValue; `DragValue::step` means what `Slider::step` means,
  a snap that is also the arrow unit.
- [ ] `ColorStrip::hue` and `alpha` in place of `for_hue` and `for_alpha`. Callers: 4 / 0.
- [ ] Disabled looks for Slider and the color widgets; hover and press looks for ColorButton and
  ColorSwatch through a button-shaped slot in `ColorPickerTheme`, applied by `wear`.
- [ ] `Splitter::along(ratio, axis)` and a public `Separator::along(axis)`, beside the `row`/`col`
  pairs, as `Panel::stack(axis)` is.
- [ ] `Separator::thickness` repeats `Configure::size` on the cross axis. Target: the long-axis
  stretch applies whenever that axis is `Hug`, and `thickness` goes (0 callers).
- [ ] `Splitter::min_ratio` public, with a release `assert!` and `# Panics`, as Dock needs it.
- [ ] `default_line_gap` in `ThemeDefaults`, with its `Configurator` twin.

### Batch 6: Input, players and hosts

- [ ] `PlayError`: `step` reads differently from the two players and darkroom overwrites it;
  `NoFrame` and `Busy` carry `f32` seconds. Target: drop `step` from `Unsettled` and `Busy`;
  `Duration` in `NoFrame` and `Busy`. Callers: 5 / 3.
- [ ] Pinch: `InputEvent::Zoom(f32)`, `PointerEvent::Zoom`, `Harness::pinch(at, f32)` and
  `Action::Pinch { factor: f32 }` become `InputEvent::Pinch(ZoomFactor)`, `PointerEvent::Pinch`,
  `pinch(at, ZoomFactor)` and `Action::Pinch { factor: ZoomFactor }`. Remove `PlayError::BadPinch`
  and the pinch assert; darkroom checks at its MCP boundary.
- [ ] `PointerEvent` names: `Down`, `Up`, `Move`, `Leave` become `Pressed`, `Released`, `Moved`,
  `Left`, as `InputEvent` names them. Callers: 7 / 1.
- [ ] `Modifiers` and `ShortcutMods` become `flag_set!`; `CTRL_SHIFT` and `Shortcut::ctrl_shift` go.
- [ ] `Action::Scroll` becomes `ScrollPixels` and `ScrollLines`, as the harness names its methods.
  Remove `Action::key`, the twin of the variant `Action::Key`, and keep `Action::click`, which
  fills the button and modifiers a variant needs.
- [ ] `Locator::offset(self, by)` and `nth(self, n)` in place of boxing by hand; `Found::rect` the
  constructor renamed `center_of`.
- [ ] Harness recovery: public `Harness::is_device_lost` and `recover(&mut self, gpu: Gpu)`, which
  rebuilds the target and asserts `Depth::Gpu`.
- [ ] `Remote::find(&self, at) -> Result<Found, PlayError>`, `with_wait(Duration)`, and `&mut self`
  on `play`, `settle` and `shot` in place of `Cell`s.
- [ ] Window token: public `Harness::WINDOW`, as `OffscreenHost::WINDOW`.
- [ ] `OffscreenHost` records window commands, with `window_commands(&self) -> &[WindowCommand]`,
  as the harness does, in place of a panic; fix the `Ui::open_window` and `close_window` docs.
- [ ] System facts: `HarnessBuilder::scale` renamed `system_scale`; `HarnessBuilder::user_scale`
  removed (2 test callers); `Harness::set_display` replaced by `set_system_scale(f32)` and
  `set_refresh_millihertz(Option<u32>)`, each returning `&mut Self`. The same setters land on
  `OffscreenHost`, seeded by its builder, and `frame` loses its scale (decided, 4).
- [ ] `HarnessBuilder::gpu(gpu)` repeats `depth(Depth::Gpu(gpu))`, and `format` matters only at
  that depth. Target: `Depth::Gpu { gpu, format }`, and `gpu()` and `format()` go. Callers: 21 / 1.
- [ ] Harness shapes: `state` returns `Option<&S>`, as `Ui::state`; `arranged` returns `Option`, as
  `rect`; `ui(&self) -> &Ui` beside `ui_mut`; every input method returns `InputDelta` and every
  setter `&mut Self`; `wait_for_wake(timeout: Duration)`.
- [ ] `Ui::gpu_pass_stats` public; `OffscreenHost::gpu_pass_stats` goes.
- [ ] `HostHandle::request_repaint` and `send_input`, with `OwnedInputEvent`, become crate-private:
  `Waker` is the one cross-thread wake, and a second input path makes a `Remote` settle early.
- [ ] `HostHandle::run_on_main` takes `FnOnce(&mut T)` and always repaints, in place of a `bool`.
- [ ] `WinitHostBuilder::title` writes into the `WindowConfig` that `window` sets, so their order
  no longer drops the title.
- [ ] `Clipboard::text() -> Result<Option<String>, ClipboardUnavailable>`, as `image()` answers an
  empty clipboard.
- [ ] `WindowGeometry` derives `PartialEq`, `Eq`; `WindowConfig::with_geometry` replaces
  `with_placement`.

### Batch 7: The Ui and animation

- [ ] `Ui::set_cursor` renamed `request_cursor`, as it lasts one frame. Callers: 10 / 1.
- [ ] One spelling of "no motion": `AnimationSpec` drops its `Option` everywhere
  (`animate(request, spec)`, `set_theme_transition(spec)`, `Metrics::transition`,
  `SlotDefaults::animation`), with `SNAP` and serde default `SNAP`; update AGENTS.md "Theme
  transition".
- [ ] An animate sibling returning `{ value, settled }`, replacing Expander's `stored.shown`.
- [ ] Reduced motion for paint animations: a marker on `PaintAnimation`, decorative or progress,
  that palantir honors as `animate` honors it.
- [ ] `Easing::curve(self) -> PaintCurve`; `PaintCurve`'s doc allows the overshoot of `OutBack`;
  `curves::linear` goes.
- [ ] `AnimationRequest::key(self, ..)` renamed `with_key`, as a value's setter.
- [ ] The `peek_*` rule in AGENTS.md covers pointer and modifier levels alone; the unused
  `pointer_local` pair goes.

### Batch 8: Paint values and shapes

- [ ] Gradients: remove `GradientBuilder`, its three aliases, the three `builder` functions and the
  four panicking `From`s; `new(<geometry>, stops)` then `with_spread` and `with_interpolation`.
  `two_stop` takes the geometry on every kind (decided, 5). Callers: about 13 builder sites,
  19 `two_stop` sites.
- [ ] `Chrome::rounded(impl Into<Brush>, impl Into<Corners>)`; `with_corners` and `with_fill`; the
  doc stops saying "solid".
- [ ] `Corners`: remove both tuple `From`s, which mean top and bottom where `Spacing`'s mean axes
  and CSS's mean diagonals (0 callers outside tests).
- [ ] `Spacing::from_array` becomes crate-private; `Spacing::xy`'s doc stops citing CSS, which puts
  vertical first.
- [ ] Shapes (decided, 2): `Shape::rect()` and `Shape::windowed_rect()` start at the owner rect
  and take `.at(rect)`; `owner_rect` and `owner_windowed_rect` go.
- [ ] `TextShape::family`, `weight` and `slant` go: the font goes through `Shape::text(text, font)`.
- [ ] `IconFit` moves beside `ImageFit` at the root, with the shared variants in one order.
- [ ] `RgbaF32: Display`, writing the form `FromStr` reads; `Serialize` goes through it.
- [ ] `ColorCoords`: `with_hue`, `with_saturation`, `with_value` in place of the `set_*` methods.
- [ ] `Lens::default` becomes `Lens::WINDOW_GLASS`; `Lens::is_refractive_index` moves into
  `domain`.
- [ ] `Track`'s `From<Sizing>` goes (0 callers); `HAlign` and `VAlign` derive `Hash`.
- [ ] `domain::f64::bounds` and `is_bounds` published beside `range`, with a row in the kinds
  table: neither end NaN, infinity allowed, ends ordered low to high where `range` keeps them.

### Batch 9: Theme

- [ ] `Default` (decided, 9): remove `Default` from every theme bundle, `Theme` and `TextStyle`;
  a caller names the scheme (`Theme::stock(scheme)`), and `SchemePreference::FALLBACK` stays the
  one named dark choice. Callers: about 150 test sites, 2 example sites.
- [ ] `Lerp` split from `Animatable`, with its own derive, then `#[animate(text)]` on the derive,
  which emits `for_each_text`, so the 15 hand-written walks go.
- [ ] `DragValueTheme` reshaped to `{ looks, defaults, caret, caret_width, selection, placeholder }`,
  the editor slot derived at `show`; the 8 copied looks and `from_chip` go. Darkroom's
  `const_editor_theme.rs` edits one copy.
- [ ] `Theme::frost_floats` without the `Palette`: the inks come from the theme; a bare popup's ink
  is `context_menu.item.shortcut`.
- [ ] Names: `ColorPickerTheme::border` and `border_width` become `border: Stroke`; `DockTheme`'s
  `preview_fill`, `preview_stroke`, `preview_radius` become `preview: Chrome`; `SplitterTheme`'s
  `hovered`, `active` become `rule_hovered`, `rule_active`; `SpinnerTheme::min_width` becomes
  `min_stroke_width`; `Palette::window_bg` and `Theme::window_clear` take one name;
  `SlotDefaults::animation` becomes `transition`, as `Metrics::transition`; slots list
  `unselected` before `selected` everywhere.
- [ ] `Palette::popup_panel` and `float_shadow` become crate-private.

### Batch 10: GPU

- [ ] Remove `DeviceFeatures`, `SurfaceUsages` and `PowerPreference`: the error variants and
  `RequestedGpu::headless` take the wgpu types, and `wgpu_mirror!` goes. Callers: 2 / 1.
- [ ] `Gpu::create_render_pipeline` and `create_compute_pipeline` public; the examples'
  `GpuPaint::init` use them, as views build on parallel test threads over one shared device;
  `Gpu::new` documents that its turn and lost callback are its own.
- [ ] `DeviceRequirements::met_by` becomes crate-private.

### Batch 11: Dead surface

Each item has no caller outside palantir's own definition and tests, or a newer item replaced it.

- [ ] Text (decided, 6 and 8): `TextShaper::glyphs`, `TextGlyphs`, `GlyphLine`, `PlacedGlyph`,
  `GlyphRasterKey`, `RasterImage`, `ContentType`; `FontSource::File` and its path `From`s;
  `TextProbe::text_hash` and `hash_of` crate-private.
- [ ] Icons (decided, 7): `IconTable::baked`, `IconDefinition`.
- [ ] Geometry: `MeshVertex` and `Mesh::content_hash` crate-private; `Mesh::filled_triangle` goes;
  `TranslateScale`'s `compose`, `anchored_at`, `is_identity` and the point and rect maps
  crate-private; `BatchKind::COUNT` goes.
- [ ] Colors: `HueSlice` and `ColorModel::slice` crate-private.
- [ ] Ui: `render_frame_id`, `pointer_actions` with `PointerAction` and `PointerEdge`, `has_font`
  and `font_families` (twins of `TextShaper`'s), `input_policy`, `set_input_policy` and
  `InputPolicy`; `Platform` and `PLATFORM`, whose methods are all crate-private.
- [ ] Widgets: `Widget::grid` and `grid_tracks` crate-private, as `scroll` is; `GridCell::along`
  crate-private and `GridCell`'s `NoUninit` derive replaced by a hand pack;
  `EditableLabel::label_id` goes; `Scroll::zoom_by` and `content_margin` go, as tests alone call
  them; `Scroll::hide_bars` renamed `hidden_bars`.

### Batch 12: Names and polish

- [ ] `Configure::pos` renamed `at` (decided, 3).
- [ ] `NodeIdx` renamed `DockNodeIdx` if it stays public after batch 4.
- [ ] `TextStyle::font_size` and `line_height_factor` against `GlyphFont::size` and `line_height`:
  one name per quantity; a factor is a `factor`, a length has no unit in its name.
- [ ] `ImageDataError::TooLarge` renamed `LengthOverflow`, apart from the GPU's `ImageTooLarge`.
- [ ] The unfilled remainder of a bar is `"track"` in Slider and `"rest"` in ProgressBar: one word.
- [ ] `CheckState` derives `Default`, as `Selection`; `ResponseState` and `ResponseSnapshot`
  derive `PartialEq`, with their docs stating what the crate produces, not invariants;
  `FrameReport` derives `Clone` and `PartialEq`.
- [ ] `Button::show_with` written into the lifecycle rule as the one `show` with a body for a widget
  that also shows alone.
- [ ] `Widget::wear`'s doc names no private type.
