# Public API changes

This file holds every fix from `.notes/REVIEW.md`, `.notes/TEST_REVIEW.md` and `.notes/ISSUES.md`
that adds, renames, removes or re-signs an exported item. Under AGENTS.md, each one needs a
go-ahead before work starts. `.notes/REDESIGN.md` holds the rest, and names an interim step where
one exists.

When an item is done or rejected, delete it.

Each item gives the findings it closes, the options, a recommendation, and what it touches.
The recommendations are starting positions. Before you settle a signature, read its neighbours
(the sibling setters, the trait it joins, the `Response` its peers return) and match their names,
argument shapes and order, as AGENTS.md requires.

Line numbers are against `50b34a49`.

---

## A1. Widget-authoring surface (largest item)

**Findings.** REVIEW "Big widgets reach `pub(crate)` internals" and "Small widgets reach
non-public API". Shipped widgets use items that an outside crate cannot reach, which breaks
"widgets use only the public API".

| Item | Used by |
|------|---------|
| `Widget::scroll`, `Widget::scrollbars`, `Widget::scrollbar_def`, `ScrollAxes`, `ScrollbarsDef`, `BarGeometry`, `Ui::scroll_content` | Scroll, TextEdit, TabStrip |
| `ScrollState`, `ScrollBounds` (`apply_wheel_pan`, `clamp_to_natural`, `transform`) | TextEdit |
| `OverlayScope`, `Backdrop` | Popup, Modal, Tooltip, ContextMenu |
| `Ui::gpu_view`, `GpuPaintRef` | GpuView |
| `DragNum::{read, commit_drag, commit_value, parse_from, edit_string}`, `Num`, `Limits` | Slider, DragValue |
| `Response::lazy` | DragValue |
| `Axis::{main, main_v, rows_cols, compose_spacing}`, `CursorIcon::resize_along` | Splitter, Scroll |
| `Arrow`, `check_polyline`, `chevron_pts`, `arrow_angle` | Checkbox, ComboBox, Expander |
| `ToggleChrome`, `checkerboard`, `ColorSurface` (`texel_size`, `checked_downsample`, `DOWNSAMPLE`), `axis_keys` | Checkbox, Radio, Switch, colour widgets |
| `TextStyle::metrics_valid`, `Background::border_inset`, `Align::place_in` | TextEdit |
| `TabStrip::insertion_slot`, `TabItemBuf`, `DockState::drag`, `DockState::drop_target` | TabbedView, DockView |

**Options.**

1. Make each item `pub` where it stands, with docs a stranger needs.
2. Group the scaffolding into one public `authoring` module (scroll viewport, overlay scope, numeric
   drag, toggle chrome, colour surface, glyph helpers), and keep the widgets' own types where
   they are.
3. Rewrite the widgets so they need less (for example, TextEdit stops using `ScrollState`, see
   REVIEW "`ViewState.scroll` carries `zoom` and `drag_anchor`").

**Recommendation.** Do 3 first where it removes the need: TextEdit gets a text-viewport state of
its own (offset and natural size only), and `TabStrip::insertion_slot` becomes a method on a
public type both callers already hold. Then 1 for the rest, item by item, in the module where each
item lives today. A new `authoring` module would be a second place to look for the same kind of
item. Split this item into one go-ahead per row of the table.

**Touches.** `src/widgets/**`, `src/layout/scrollbars`, `src/layout/types/scroll_axes.rs`,
`src/ui/mod.rs`, `src/renderer/gpu_paint`, `src/primitives/geometry` helpers, `lib.rs` exports.

## A2. Colour-only text override in a widget look

**Findings.** REVIEW "Theme looks freeze `TextStyle::default()` instead of inheriting
`Theme::text`". `WidgetLook.text: Option<TextStyle>` is all-or-nothing, so a recipe that only
wants a colour bakes a full 16 px SANS style. An app that sets `theme.text` to 13 px gets 16 px
disabled fields and tab chips that change size on press.

**Options.**

1. `WidgetLook { background, text: Option<TextStyle>, text_color: Option<RgbaF32> }`, applied
   over the inherited style.
2. `text: TextLook` where `TextLook` is `Inherit`, `Color(RgbaF32)` or `Style(TextStyle)`.
3. Recipes resolve against `Theme::text` when the theme is built. This needs no API, but it goes
   stale when the app sets `theme.text` after `from_palette`.

**Recommendation.** 2. One field, one meaning, and `Inherit` is the explicit default. Check
`WidgetLook::resolve` and the `AnimatedLook` lerp, which must lerp the colour of a `Color` arm.

**Touches.** `widgets/theme/widget_look`, every recipe that bakes `TextStyle::default()`
(`text_edit.rs:153`, `tabs.rs:149-150`, `button.rs:65`, `toggle.rs:158`, `expander.rs:109`,
`menu_item.rs:85`), the theme serde format.

## A3. Split `KeyClass::Motion`

**Findings.** REVIEW "Scopes claim key classes their owners never act on". A focused TextEdit or
TabStrip claims all of `Motion`, which includes Tab and PageUp/PageDown, and then drops the keys
it does not use. An app that polls Tab for focus traversal never sees it.

**Options.**

1. Split `Motion` into `Caret` (arrows, Home, End), `Page` (PageUp, PageDown) and `Focus` (Tab,
   Shift+Tab). `KeyFilter` gains the three flags.
2. Keep the classes and add a "consumed" report after handling, so an unhandled key walks on.
   This is the WPF `Handled` model, but here the grant is decided before any widget handles the
   key, so it needs a second dispatch round.

**Recommendation.** 1. The scope model is "declare what you take"; finer classes keep it
declarative. A multi-line TextEdit takes `Caret | Page`; a single-line one takes `Caret`; a
TabStrip takes `Caret`.

**Touches.** `input/key_class.rs`, `KeyFilter` presets (`TEXT_FIELD`), TextEdit, TabStrip, every
test that names `Motion`.

## A4. Fallible `UserScale` constructor

**Findings.** REVIEW "`UserScale::new` asserts on a persisted `nan`". The doc tells apps to read a
saved preference back through `new`, which asserts.

**Options.** `UserScale::try_new(f32) -> Option<UserScale>`, or `from_persisted(f32) -> UserScale`
that falls back to `ONE` for a non-finite or out-of-range value.

**Recommendation.** `try_new` returning `Option`, beside `new`. Check the other constructors in
`display/` for an established fallible name first. Change the doc to point at it.

## A5. Per-type settle tolerance for `Animatable`

**Findings.** REVIEW "Spring settle floor is in pixels but is applied to colours and mixed
compounds". REDESIGN D7 adds a unit-free interim.

**Options.**

1. An associated const with a default: `const SETTLE_EPS: f32 = 0.01;`. `RgbaF32` sets about
   `1.0 / 4096.0` (below one 8-bit sRGB step near black). The derive takes the minimum over
   fields.
2. A method `fn settle_distance_squared(self) -> f32` that each type scales into a common unit.

**Recommendation.** 1, with a default, so existing implementations still compile. The derive
change is in `anim-derive`. After this lands, remove D7's interim (one absolute `1e-4` floor for
every spring), which costs about 0.4 s of extra repaint per pixel spring.

## A6. `BatchKind` without strum in its public derives

**Findings.** REVIEW "Public API leaks third-party crate types"; TEST_REVIEW 2 (hand-listed
"exhaustive" tests missed `BatchKind::Icon`).

**Recommendation.** Inherent `BatchKind::iter()` and `BatchKind::COUNT`, as `PointerButton` does.
Remove the strum `EnumIter` / `EnumCount` derives from the public type.

## A7. `golden` module surface

**Findings.** REVIEW "the public `golden` API takes and returns `image::RgbaImage` without
re-exporting `image`". TEST_REVIEW 7: `Tolerance` caps the share of differing pixels but not how
far a pixel may differ, and `render*` drops the `FrameReport`.

**Recommendation.**

- `pub use image` from `golden` (the same reason `lib.rs:225-238` re-exports `wgpu`).
- `Tolerance { max_delta: u8, max_pixels: u32 }` (the WPT reftest fuzzy model), default exact.
- `Goldens::assert_matches` writes an adapter sidecar and reports orphans (TEST_REVIEW 7);
  those parts are behaviour, but they belong with this change.

## A8. Remove `WinitHostError::Gpu`

**Findings.** REVIEW "Input and host structure". `WinitHostError::Gpu` and
`From<GpuRequestError>` are never constructed in production. Device failures already surface as
`Surface { source: SurfaceError::Device }`.

**Recommendation.** Remove the variant and the `From` impl. Tests that build it move to the
`Surface` variant.

## A9. Test-driven additions

**Findings.** TEST_REVIEW 10 (50 `Modifiers { ctrl: true, ..Modifiers::NONE }` literals) and 15
(9 `stack(axis)` matches).

**Recommendation.**

- `Modifiers::CTRL`, `Modifiers::SHIFT`, `Modifiers::ALT`, beside `Modifiers::NONE`.
- `Panel::stack(axis: Axis)` beside `Panel::hstack` / `vstack`. `Axis` is `pub(crate)` today, so
  this also needs `Axis` public, which overlaps A1. If `Axis` stays private, keep a test-support
  helper instead and drop this half.

## A10. Super modifier

**Findings.** REVIEW "Platform key events": `Modifiers` has no super bit, so Super+L arrives as
bare `l`. REDESIGN D2 clears the text in the host, which fixes the typing without this item.

**Recommendation.** Add `Modifiers::sup` (or `meta`; check what `Shortcut` and `ShortcutMods`
call it) so apps can bind Super chords. Low priority.

## A11. Dock model and view split

**Findings.** REVIEW "Big widgets design": `DockState` is documented as pure data, but `scan`,
`drag`, `set_drag`, `drop_target` and `content_size` take `Ui`. `content_size` returns
`Option<Vec2>` where `Size` exists.

**Recommendation.** Move `scan` and `content_size` to `DockView` (the private ones follow);
`content_size` returns `Option<Size>`. Check `dock_tabs.rs:200` for the same `Vec2`.

## A12. TabbedView page identity

**Findings.** REVIEW "TabbedView reorder and identity": chips are keyed by index, so after a close
or reorder, slot `i`'s hover and look animation move to the page that slid into it.

**Options.** A key function, `TabbedView::keyed(|page: &S| -> u64)`, or a bound `S: Hash`.

**Recommendation.** An optional key function, with index keys as the default when none is given.
Compare with how `TabItem` and `DockState` key tabs.

## A13. One way to pick a look

**Findings.** REVIEW "Big widgets design": `ButtonTheme`, `TextEditTheme`, `TabsTheme`,
`ToggleTheme` and `MenuItemTheme` each have a public `pick` that duplicates `ThemeSlot::look`;
`ExpanderTheme` has none.

**Recommendation.** Remove the five `pick` methods; callers use `ThemeSlot::look`.

## A14. Attach-to-trigger naming

**Findings.** REVIEW "Small widgets design": `Tooltip::on(&snapshot)` and
`ContextMenu::attach(ui, &snapshot)` name one idea twice, with different argument shapes.

**Recommendation.** One verb and one argument order for both. Read `Popup`'s constructors first;
whichever of the three reads most like prose wins.

## A15. ColorButton parity with ColorPicker

**Findings.** REVIEW "Small widgets design": ColorButton lacks `swatches(&[RgbaF32])` and
`downsample(n)`, and its `history` default and type differ from ColorPicker's.

**Recommendation.** Add the two setters with ColorPicker's signatures, and make `history` take the
same type with the same default.

## A16. Fallible render target conversion

**Findings.** REVIEW "Render target colour encoding is not enforced". REDESIGN D6 adds a release
assert at `OffscreenHost` construction, which needs no API change.

**Option.** Replace `From<&wgpu::Texture> for RenderTarget` with `TryFrom`, with an error that
names the format. Only worth it if apps pick target formats at run time.

## A17. Cargo features for tests

**Findings.** TEST_REVIEW 8: the self dev-dependency enables `internals` in every test build, so
seven comments that say a plain `cargo test` is GPU-free are false, and `all(test, internals)`
equals `test`. REVIEW "Support-module docs": `lib.rs:209-212`.

**Options.**

1. Accept GPU tests in every run; delete the claims; drop the redundant gates and the
   `required-features` on `[[test]] alloc`.
2. Add a `gpu-tests` feature that the dev-dependency does not request.

**Recommendation.** 1, unless a GPU-free `cargo test` is a real need on the headless test server.
Features are part of the published surface, which is why this is here. Then update the AGENTS.md
test line.

## A18. Icon set limits and names

**Findings.** REVIEW "`IconId` is u16 but icon sets are unbounded" and "`IconDef::name:
&'static str` forces `Box::leak`".

**Recommendation.**

- `IconDef::name` becomes an owned or interned string, so a set built from files does not leak.
- `from_svgs` rejects more than `u16::MAX` icons and duplicate names. If it already returns a
  `Result`, add the two errors; if not, REDESIGN phase 10 adds release asserts as the interim.

## A19. Features the docs imply: IME and focus traversal

**Findings.** REVIEW "Stale input docs": docs mention IME text, but winit `Ime` is never enabled
or translated, and there is no Tab focus traversal. REDESIGN D16 fixes the docs.

**Recommendation.** Treat each as a feature with its own design: IME needs `InputEvent` variants
for preedit and commit; focus traversal needs a focus order and a `Focus` key class (A3). Out of
scope for the defect work.

## A20. Keyboard support on the toggle and range widgets

**Findings.** REVIEW "Small widgets design": ColorField, ColorStrip and Expander are focusable and
key-driven; Slider, Checkbox, RadioButton and Switch are not.

**Recommendation.** Make them focusable with Space/arrow handling. If this needs a public setter
(for example `focusable(bool)`), match Expander's. Pairs with A19's focus traversal.

## A21. A font load that fills the family table

**Findings.** REVIEW "Release builds lack screens the primitive docs promise": `load_font`
interned family names with the panicking `FontFamily::named`. The interim (REDESIGN D11) uses
`try_named`, skips a name that does not fit, and returns `FontLoadError::NoFaces` when none fits —
true about the result, but it names the wrong cause.

**Recommendation.** Add `FontLoadError::FamilyTableFull` (check whether the enum is
`#[non_exhaustive]` first) and return it when no loaded family could be interned.


## A22. Wheel sense per axis

**Findings.** REVIEW "Wheel routing" / REDESIGN D9: `hit_test_targets` sends the whole wheel delta
to the topmost `Sense::SCROLL` row. Every `TextEdit` senses `SCROLL` and a tab strip band is a
horizontal scroll, so a vertical wheel over either is swallowed and the page under it does not
scroll. Routing each axis to the nearest row that can pan along it (browser scroll chaining, CSS
Overscroll Behavior §2) needs each row to say which axes it pans *this frame* — a `Scroll` only
where its content overflows its viewport or its zoom is above 1, a single-line `TextEdit` only
along x and only when its text overflows. The cascade cannot infer that: the row that senses the
wheel is a `Scroll`'s outer frame, not the viewport node layout knows the extent of, and a
`TextEdit` pans inside one leaf. Widgets reach only the public API, so the declaration has to be
public.

**Options.**

1. `Sense::SCROLL_X` and `Sense::SCROLL_Y`, with `SCROLL = SCROLL_X | SCROLL_Y`. A widget senses the
   axes it can pan this frame; `hit_test_targets` keeps the topmost row per axis, and the wheel
   delta splits by axis (after the Shift swap) before delivery.
2. A separate `pan_axes(ScrollAxes)` configure setter beside `sense`. More explicit, but a second
   knob that has to agree with the `SCROLL` bit.

**Recommendation.** Option 1: the axes are what the sense means, and existing `Sense::SCROLL`
callers keep their behaviour. `Scroll` senses its declared axes intersected with the axes it can
pan (from last frame's `ScrollGeometry`), and `TextEdit` senses `SCROLL_X` while its text
overflows; the y→x wheel mapping then moves into routing — a horizontal-only row takes a pure-y
delta only when no row under the pointer pans y. Touches `Sense`, `Scroll`, `TextEdit`,
`TabStrip`, `Cascade::hit_test_targets` and `InputState::on_scroll`. Tests: wheel y over a field in
a `Scroll::vertical()` scrolls the page; an overflowing tab strip pans on wheel x and Shift+wheel y
(Linux) and passes wheel y to the page; a lone field with overflowing text pans on wheel y.

## A23. A shared popup trigger

**Findings.** REVIEW "Small widgets design": `ColorButton`'s `ChipState` and `ComboBox`'s
`ComboState` are the same `{open}` row, and the block around them is the same in both: probe the
flag, toggle on click, close when disabled, show `Popup::below(rect)`, close on `closed()`, write
back only on a flip. REDESIGN D12 proposed a crate-internal `PopupTrigger`, but AGENTS.md lets a
widget reach only the public API, so a shared helper has to be public.

**Recommendation.** A public `PopupTrigger` beside `Popup`: `PopupTrigger::new(id, &response)`
probes and toggles, `open()` answers, `close()` closes, and `finish(ui)` writes back on a flip. Read `Popup`, `OverlayScope` and `ContextMenu::attach` first, and match their argument
order. Touches `ColorButton`, `ComboBox` and any app that drops its own panel from a button.

## A24. `const` on public functions that can take it

**Findings.** REVIEW "Text and primitives style-rule violations" and "Renderer design and
duplication": `Rect::deflated`, `Display::from_physical` and `Display::scale_factor` are public
`fn`s that can be `const`. Adding `const` re-signs an exported item, so it waits here; the
crate-internal ones are done.

**Recommendation.** Make it `const`, and sweep the rest of the public surface for the same in
one pass.

## A25. Golden tolerance that bounds how far a pixel may differ

**Findings.** TEST_REVIEW "The visual suite's tolerance and capture lose information":
`golden::Tolerance { per_channel, max_ratio }` caps the share of pixels past `per_channel`, but not
how far those pixels may move, so a loosened golden lets a few pixels be wholly wrong. Its
`Default` (2 per channel, 0.1 %) hid stale goldens in this repo's own suite: switching the suite to
exact comparison turned up five goldens 1–3 steps off on a large share of their pixels. The suite
now compares exactly through its own wrapper (`tests/visual/goldens.rs`); the public type and its
default are unchanged.

**Recommendation.** The WPT fuzzy shape: `Tolerance { max_delta, max_pixels }` — at most
`max_pixels` pixels may differ, and none by more than `max_delta` on any channel — with
`Tolerance::EXACT` as the `Default`. A loosening then names two numbers a reader can derive. Touches
`golden::Tolerance`, `DiffReport::passes`, `Goldens::tolerance` and any downstream suite.

## A26. Golden bookkeeping a suite cannot do from outside

**Findings.** TEST_REVIEW "The visual suite's tolerance and capture lose information": `Goldens`
does not know which adapter wrote a golden, so a driver update reads as pixel diffs across the
suite; and nothing reports a golden no test compares against any more.

**Recommendation.** `Goldens::adapter(info)` writes an adapter sidecar beside the goldens and
fails a comparison against a sidecar from another adapter with that reason instead of a pixel
diff; `Goldens::orphans(names)` lists golden files not among `names`, for a suite to assert empty
from one test that names them all. Touches `golden::Goldens`.
