# Review: palantir's public API

Scope: every item in `.notes/API_SURFACE.md`, judged against the rules in `AGENTS.md`
("Public API design") and against how darkroom, the showcase and the tests call it.
Darkroom is one user; an item stays only if it is general.

## Proposal

The surface is mostly consistent: builders, values, flag sets, theme slots, state drivers and
responses each follow one shape. An item is judged by whether a general user needs it, not by
whether darkroom calls it. The defects fall into two root causes:

- **One kind of constructor, three names.** The sub-theme recipes are `from_parts`,
  `from_palette` or `from_metrics` by the inputs each reads today.
- **One kind of parameter, two spellings.** Setters name their one argument differently, and
  some generic arguments are named type parameters where their siblings take `impl Trait`.

Nothing in the surface is specialized for darkroom. The items added for it in the last plan
(`App::is_busy`, `Ui::waker`, `Harness::wait_for_wake` and `has_pending_wake`,
`Remote::settle`, `TextEdit::buffered`, `EditableLabel`, `Button::selected` and `show_with`,
`Metrics::text`, `BackdropFilter::with_max_luminance_for`, `OverlayResponse::opened`) each name
a general need, with its source in `AGENTS.md`.

## Plan

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
- **`TextEditTheme::corner_centering`**: it places a hugging single-line field so its glyphs
  land on a point, as an app that draws text on a canvas needs to edit it in place.
  `EditableLabel` covers the case inside a layout, not over drawn text.
- **`ColorPicker::color`**: a wrapper outside the crate reads the color a picker shows through
  it, as `ColorButton` does; the Wrappers rule keeps a wrapper on public API.
- **`Palette::popup_panel`, `DockView::tab_key`**: shared by recipes, and the key a caller
  needs for `TabStrip::chip_id` of a docked tab.

## Gaps with no workload

Each is a missing sibling, left until a caller needs it: `Scroll::set_zoom` beside
`set_offset`; `Clipboard::image` beside `set_image`; `EditableLabel::edit` and `is_editing`,
the state drivers `ContextMenu` and `ComboBox` have; `From<bool>` for `TabStripFocus` and
`MenuState`, as `Selection` and `CheckState` have.
