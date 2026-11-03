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
- **No caller is no reason to remove** (AGENTS.md, "Public API"): an item goes when it is wrong
  for the surface. Plumbing a widget needs is `pub(crate)`.

### Decided

Each choice was taken on 2026-11-02 with the owner; the refused option and the field's practice
follow it.

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
- [ ] Animation (decided): `impl Animatable for TextStyleOverrides` goes. Overrides blend and
  never spring, as a theme does; `Lerp` stays.

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
