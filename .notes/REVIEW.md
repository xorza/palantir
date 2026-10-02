# Crate review

Whoever addresses an item deletes it from this file. A group goes when its last item goes.

Groups run from the most severe to the least: panics on reachable input first, then wrong output, then per-frame cost, then design, docs and style. Items tagged **bug** were traced through the code. Items tagged **bug (plausible)** name the check that would confirm them.

## Theme and preference file data reach asserts
`F32Ext::themed_length` says theme scalars are hand-edited file data that "cannot assert". Most widgets screen them. These do not, and `SpinnerTheme`/`ToggleTheme`/… derive `Deserialize`:
- [ ] `src/display/user_scale.rs:27-29,71-77` **bug**: the doc tells apps to read a persisted preference back through `UserScale::new`, which `assert!`s on a non-finite value. A config file containing `nan` crashes the app. Public API: a fallible constructor needs a go-ahead.

## Fields and strips that cannot scroll still take the wheel
- [ ] `src/widgets/text_edit/mod.rs:140-141` **bug**: every TextEdit senses `SCROLL` unconditionally. The scroll target is the topmost `SCROLL` row with no chaining (`src/scene/cascade/mod.rs:308-313`). Scenario: a form of single-line fields inside `Scroll::vertical()`. With the pointer over any field, the wheel goes to the field, which turns it into a horizontal pan (`view_state.rs:105-109`). That pan is a no-op when the text fits, and the page does not scroll.
- [ ] `src/widgets/tabs/tab_strip.rs:243-246` **bug**: same root cause. The chip band is `Scroll::horizontal()`, so a vertical wheel over any tab strip is swallowed (`pan_y=false` discards it). The page around a TabbedView or dock pane does not scroll from over the strip, and mouse-only users cannot pan overflowing chips. TextEdit maps y onto x for this case; the band does not.

## Scopes claim key classes their owners never act on
- [ ] `src/widgets/text_edit/mod.rs:389` with `src/input/key_class.rs:91-100` **bug**: a focused field declares the whole `MOTION` class, which includes `Tab`, `PageUp`/`PageDown` and every modified arrow. `apply_key` ignores Tab and PgUp/PgDn, and Up/Down in single-line mode. Those keys are granted to the field and dropped. An app polling `ui.key_pressed(Shortcut::key(Key::Tab))` for its own focus traversal never sees Tab while a field is focused.
- [ ] `src/widgets/tabs/tab_strip.rs:145` **bug**: same cause. A focused strip claims `MOTION` but handles only Left/Right/Home/End and Ctrl(+Shift)+Tab, so plain Tab, Up/Down and PgUp/PgDn are swallowed.

## Theme looks freeze `TextStyle::default()` instead of inheriting `Theme::text`
- [ ] `src/widgets/theme/text_edit.rs:153`, `tabs.rs:149-150`, `button.rs:65`, `toggle.rs:158`, `expander.rs:109`, `context_menu/menu_item.rs:85` **bug**: `WidgetLook.text` is all-or-nothing. To change only the colour, these recipes bake a full 16 px SANS style. Any app that sets `theme.text` (13 px, mono…) gets mixed results:
  - A disabled TextEdit/Button switches to 16 px, and the single-line field's height changes (`paint_input.rs:51-53`).
  - Unselected tab chips render at 16 px while the selected chip uses 13 px.
  - Pressing an unselected chip flips it to 13 px while held, because `inactive.active` is `None`.
  - The close glyph resizes on hover.

## TabbedView reorder and identity
- [ ] `src/widgets/tabs/tabbed_view.rs:276`: chips are keyed `i as u64`, which TabItem's doc (`tab_item.rs:8-12`) says hands one chip's state to another. After Closed/Reordered, the look animation and hover state of slot i transfer to whatever page slid in.

## Smaller widget bugs

## Icon prewarm warms keys the frame never asks for
- [ ] `src/gpu/icon/mod.rs:124` **bug (plausible)**: prewarm keys on `def.view_box * display_scale`. The composer keys on the drawn box `phys_rect.size`, which includes ancestor transforms (`composer/session.rs:381`). So prewarm hits only icons drawn at exactly their view-box size. Any other size still takes the 10-20× filtered raster lazily.
  - Prewarm rasterizes every filtered icon of every loaded set in one frame on each DPI change or set load: a worst-case-frame spike. Those slots are stamped current-frame, so they cannot be evicted while that frame's real draws compete for space.
  - To confirm: compare showcase icon box sizes against their SVG view boxes.

## Big widgets reach `pub(crate)` internals ("widgets use only the public API")
- [ ] TextEdit: `src/widgets/text_edit/mod.rs:29,140` (`ScrollAxes`, `Widget::scroll`), `mod.rs:417` (`TextStyle::metrics_valid`), `mod.rs:431` (`Background::border_inset`), `text_geometry.rs:107` (`Align::place_in`), `view_state.rs:9` and `paint_input.rs:13` (`ScrollState`/`ScrollBounds` with `apply_wheel_pan`, `clamp_to_natural`, `transform`).
- [ ] Scroll: `src/widgets/scroll/mod.rs:16-17,139,213,425` (`ScrollbarsDef`/`BarGeometry`, `ScrollAxes`, `Widget::scroll`, `Ui::scroll_content`), `bars.rs:145-148` (`Widget::scrollbars`, `scrollbar_def`), `state.rs:277` and `bars.rs:134` (`Axis::main_v`/`main`).
- [ ] Tabs/dock: `src/widgets/tabs/tab_strip.rs:212` `TabStrip::insertion_slot` (used from `tabbed_view.rs:346` and `dock/pane_geometry.rs:71`), `tab_item.rs:83` `TabItemBuf` (used by TabbedView and DockView), `dock_view.rs:141,360` (`DockState::drag` / `drop_target`).
- [ ] Theme helpers that other widgets call: `combo_box.rs:42` `chevron_pts` (built on the `pub(crate)` `Arrow`), `toggle.rs:86` `check_polyline`, `expander.rs:85` `arrow_angle`.

## Small widgets reach non-public API ("widgets use only the public API")
- [ ] `popup/mod.rs:214`, `modal/mod.rs:111`, `tooltip/mod.rs:238`: `OverlayScope`/`Backdrop` are `pub(super)`. Popup, Modal and Tooltip cannot be rebuilt outside the crate.
- [ ] `gpu_view/mod.rs:112`: `Ui::gpu_view` and `GpuPaintRef` are `pub(crate)`.
- [ ] `slider/mod.rs:130,138`, `drag_value/mod.rs:233,253,271,365,382`: `DragNum::read/commit_drag/commit_value/parse_from/edit_string`, `Num` and `Limits` are `pub(crate)`. Only the `DragNum` enum is public.
- [ ] `drag_value/mod.rs:392`: `Response::lazy` is `pub(super)`.
- [ ] `splitter/mod.rs:169,213,224,247`: `Axis::main/main_v/rows_cols/compose_spacing` and `CursorIcon::resize_along` are `pub(crate)`.
- [ ] `checkbox/mod.rs:89`, `combo_box/mod.rs:169`, `expander/mod.rs:196`: theme helpers `check_polyline`, `chevron_pts` and `arrow_angle` are `pub(crate)`, as is `Arrow` (`arrow.rs`).
- [ ] `toggle_chrome/mod.rs` (whole file), `checkerboard.rs`, `color_surface.rs` (`ColorSurface`, `texel_size`, `checked_downsample`, `DOWNSAMPLE`), `axis_keys.rs`: crate-only scaffolding the shipped widgets depend on.
- [ ] `widget/mod.rs:128,148,435`: `Widget::scroll`, `scrollbars` and `scrollbar_def` are `pub(crate)`, yet documented as authoring surface ("as `crate::Scroll` does").

## Public API leaks third-party crate types
- [ ] `src/diagnostics/gpu_pass_stats.rs:32`: public `BatchKind` derives strum's `EnumIter`/`EnumCount`, so iterating it requires the caller to depend on the same strum major. `PointerButton` hides this behind an inherent `iter()`, and `flag_set` removed bitflags for exactly this reason.
- [ ] `src/golden/mod.rs:14,45,108,172`: the public `golden` API takes and returns `image::RgbaImage` without re-exporting `image`. A consumer has to keep a semver-identical `image` dependency by hand — the hazard `lib.rs:225-238` cites for re-exporting `wgpu`.

## Dependencies that could go

## Big widgets design and consolidation
- [ ] `src/widgets/dock/mod.rs:5` vs `dock_state.rs:716-838`: the module doc says the model is "pure data with no `Ui` in sight", but `DockState` carries `scan`, `drag`/`set_drag`, `drop_target` and `content_size`, all of which take `Ui`. That is view code on the model.
- [ ] `dock_state.rs:835` and `dock_tabs.rs:200`: a size is passed as `Option<Vec2>` where `Size` exists.
- [ ] ButtonTheme/TextEditTheme/TabsTheme/ToggleTheme/MenuItemTheme each have a public `pick` that duplicates `ThemeSlot::look`, while ExpanderTheme has none: two entry points with an asymmetric set.
- [ ] `src/widgets/scroll/state.rs:21-27`: TextEdit's `ViewState.scroll` carries `zoom` and the thumb `drag_anchor`, which a text viewport never uses. The shared type is wider than TextEdit needs, and that width forces the `pub(crate)` reach-in.
- [ ] Stale docs: `text_edit/mod.rs:86,98-99` and `unicode.rs:13` claim an "IME/text commit" insertion path that does not exist (`KeyText` caps at 14 bytes and carries no IME commit). `view_state.rs:35-38` speaks of a host that "assigns `EditState::caret`", which the host cannot do (the field is private). `scrollbars_def.rs:17,25` link `Widget::scrollbars` / `scrollbar_def` as public API.
- [ ] `src/widgets/text_edit/mod.rs:351`: `pass` re-resolves the id that `show` already resolved.

## Small widgets design and consolidation
- [ ] `color_button/mod.rs:46` vs `combo_box/mod.rs:28`: `ChipState`/`ComboState` are identical `{open}` structs. The "probe flag → toggle on click → `Popup::below(rect)` → close on `closed()` → write back on flip" block is duplicated verbatim.
- [ ] Keyboard support differs across siblings. ColorField/ColorStrip and Expander are focusable and key-driven. Slider, Checkbox, Radio and Switch are not focusable and have no key path.
- [ ] `color_button/mod.rs`: lacks ColorPicker's `swatches(&[RgbaF32])` and `downsample(n)`. Its `history` default (true) also differs from ColorPicker's (Hidden).
- [ ] Naming: `Tooltip::on(&snapshot)` vs `ContextMenu::attach(ui, &snapshot)` are two names for "attach to a trigger snapshot".
- [ ] File layout: `TooltipResponse`, `ExpanderResponse`, `ClickOutside` (`popup/mod.rs`) and `SplitHalf` (`splitter/mod.rs`) are standalone public types inside a widget's file, while `ValueResponse`/`SelectResponse`/`OverlayResponse` each get their own file.
