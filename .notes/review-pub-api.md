# Review: palantir's public API

Scope: every item in `.notes/API_SURFACE.md`, judged against the rules in `AGENTS.md`
("Public API design") and against how darkroom, the showcase and the tests call it.
Darkroom is one user; an item stays only if it is general.

## Proposal

The surface is mostly consistent: builders, values, flag sets, theme slots, state drivers and
responses each follow one shape. The defects fall into four root causes:

- **Surface that lost its caller.** `TextEditTheme::corner_centering` placed darkroom's old
  inline rename; `EditableLabel` replaced it. `ColorPicker::color` is a builder getter for one
  in-crate caller.
- **One concept, two names.** The two-pack themes name the resting pack `looks` (button) and
  `unselected` (tabs). The same tab gesture is `draggable` (dock, `TabItem`) and `reorderable`
  (`TabbedView`). A byte color is `srgba` on `RgbaF32` and `srgba8` on `Image`, and `srgba` on
  `RgbaF32` also names its float constructor.
- **One kind of event, two shapes.** `TabStripResponse` reports participles, as the Responses
  rule asks; `TabbedViewResponse` reports an action enum.
- **Signatures that ask for what they already have.** `TextStyle::line_height_for(font_size)`
  is always called with the style's own size. `Remote::widgets` takes `Option<String>` where
  `Harness::widgets` takes `Option<&str>`.

Nothing in the surface is specialized for darkroom. The items added for it in the last plan
(`App::is_busy`, `Ui::waker`, `Harness::wait_for_wake` and `has_pending_wake`,
`Remote::settle`, `TextEdit::buffered`, `EditableLabel`, `Button::selected` and `show_with`,
`Metrics::text`, `BackdropFilter::with_max_luminance_for`, `OverlayResponse::opened`) each name
a general need, with its source in `AGENTS.md`.

## Plan

### Batch 1: dead surface and signatures

Files: `src/theme/text_edit.rs`, `src/widgets/text_edit/tests/align.rs`,
`src/primitives/paint/chrome.rs`, `src/widgets/color_picker/mod.rs`,
`src/theme/text_style.rs` and its callers, `src/host/winit/remote/mod.rs`,
darkroom `driver/window_backend.rs` and `widgets/zoom_range/mod.rs`.

- [ ] `TextEditTheme::corner_centering`: no caller outside its own test. Remove it, its test,
  and the mention in `Chrome`'s doc.
- [ ] `ColorPicker::color(&self)`: a getter on a builder, read only by `ColorButton` in the
  crate. Make it `pub(crate)`.
- [ ] `TextStyle::line_height_for(font_size)`: every caller passes `self.font_size`. Replace it
  with `line_height(&self)`.
- [ ] `Remote::widgets(filter: Option<String>)`: take `Option<&str>`, as `Harness::widgets`
  does, and own the text inside for the event loop's thread.

### Batch 2: one name per concept

Files: `src/theme/button.rs` and every reader of `ButtonTheme::looks` (palantir and darkroom),
`src/theme/tabs.rs`, `src/widgets/tabs/`, `src/primitives/paint/color/mod.rs` and callers.

- [ ] `ButtonTheme::looks` → `unselected`, beside `selected`, as `TabsTheme` names its two
  packs. Keep `#[serde(flatten)]`, so a theme file keeps `[button.normal]`. A single-pack theme
  keeps `looks`.
- [ ] `TabbedView::reorderable` → `draggable`, as `TabItem::draggable` and
  `DockTabs::draggable` name the same gesture.
- [ ] `TabsTheme::close_glyph` and `close_stroke` (both `f32`): the Lines rule names a line's
  width `*_width`. Rename them `close_glyph_span` and `close_glyph_width`.
- [ ] `RgbaF32::from_srgba(SrgbaU8)` and `to_srgba() -> SrgbaU8` → `from_srgba8` and
  `to_srgba8`, as `Image::from_srgba8` names bytes, so `RgbaF32::srgba(r, g, b, a)` is the one
  float constructor with that word.

### Batch 3: one response shape

Files: `src/widgets/tabs/tabbed_view.rs`, `src/widgets/tabs/tabs_action.rs`, the showcase.

- [ ] `TabbedViewResponse { action: Option<TabsAction> }` → the participle fields
  `TabStripResponse` uses: `activated: Option<usize>`, `closed: Option<usize>`,
  `reordered: Option<Reorder>` with `Reorder { from, to }`. Remove `TabsAction`.

### Batch 4: theme recipe constructors

Files: `src/theme/{progress_bar,slider,spinner,combo_box}.rs`, `src/theme/mod.rs`.

- [ ] `ProgressBarTheme`, `SliderTheme` and `SpinnerTheme` take `from_palette(p)`, and
  `ComboBoxTheme` takes `from_metrics(m)`; every other sub-theme takes `from_parts(p, m)`. Give
  all of them `from_parts(p, m)`, so an app that rebuilds one slot calls the same constructor
  for each, and a recipe that later reads a metric changes no signature. Cost: three recipes
  ignore `m` and one ignores `p`.

### Batch 5: spelling

Files: the items named.

- [ ] Parameter names of one-argument setters: `MenuItem::shortcut(s)` beside
  `shortcut_hint(shortcut)`, `Tooltip::when_disabled(yes)` beside `on` elsewhere,
  `Popup::click_outside(m)`, `Text::color(color)` beside `Separator::color(c)`, and
  `EditableLabel::label_id(id)` beside `Expander::header_id(expander)`. Take one form each.
- [ ] Generic spelling: `Chrome::fill<I: Into<Brush>>` and `rounded<I: …>` → `impl Into<Brush>`;
  `OffscreenHost::frame<T: App>(…, app: &mut T)` → `&mut impl App`, as `Harness` takes it.

## Kept

- **`style(impl Into<Option<&Theme>>)`**: `None` is the theme's slot, and a wrapper forwards its
  own optional style through it.
- **`TabStrip::selected(impl Into<Option<usize>>)`**: `None` is a state, a strip with no
  current tab, not the absence of a setting, as it was for `MenuItem::shortcut`.
- **`Modifiers` beside `ShortcutMods`**: held keys, with `mac_ctrl`, against a chord.
- **`Palette::popup_panel`, `DockView::tab_key`**: shared by recipes, and the key a caller
  needs for `TabStrip::chip_id` of a docked tab.

## Gaps with no workload

Each is a missing sibling, left until a caller needs it: `Scroll::set_zoom` beside
`set_offset`; `Clipboard::image` beside `set_image`; `EditableLabel::edit` and `is_editing`,
the state drivers `ContextMenu` and `ComboBox` have; `From<bool>` for `TabStripFocus` and
`MenuState`, as `Selection` and `CheckState` have.
