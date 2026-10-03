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

## A7. `golden` module surface

**Findings.** REVIEW "the public `golden` API takes and returns `image::RgbaImage` without
re-exporting `image`". TEST_REVIEW 7: `Tolerance` caps the share of differing pixels but not how
far a pixel may differ, and `render*` drops the `FrameReport`.

**Recommendation.**

- `pub use image` from `golden` (the same reason `lib.rs:225-238` re-exports `wgpu`).
- The `render*` functions return the `FrameReport` beside the image, so a suite can assert on
  repaint and paint mode without a second frame.
- The tolerance is A25 and the adapter sidecar and orphan report are A26; the three items land
  together (phase 5 of the plan).

## A8. Remove `WinitHostError::Gpu`

**Findings.** REVIEW "Input and host structure". `WinitHostError::Gpu` and
`From<GpuRequestError>` are never constructed in production. Device failures already surface as
`Surface { source: SurfaceError::Device }`.

**Recommendation.** Remove the variant and the `From` impl. Tests that build it move to the
`Surface` variant.

## A9. Test-driven additions

**Findings.** TEST_REVIEW 10 (50 `Modifiers { ctrl: true, ..Modifiers::NONE }` literals) and 15
(9 `stack(axis)` matches).

**Recommendation.** `Panel::stack(axis: Axis)` beside `Panel::hstack` / `vstack` (`Axis` is
public already), and `Widget::stack(axis)` beside `Widget::hstack` / `vstack` to match. The
`Modifiers` constants moved to A33, which owns both modifier types.

## A10. Super modifier

**Findings.** REVIEW "Platform key events": `Modifiers` has no super bit, so Super+L arrives as
bare `l`. REDESIGN D2 clears the text in the host, which fixes the typing without this item.

**Recommendation.** Add a `meta` field to `Modifiers` and `ShortcutMods` alike, after A33 has made
the two types convert, so apps can bind Super chords. `super` is a keyword, so the field takes the
W3C `KeyboardEvent.metaKey` name; winit reads it from `ModifiersState::super_key()`. On macOS
Command already lands in `ctrl`, so `meta` is the Windows / Super key elsewhere. `Shortcut`'s
display gets the platform glyph. Low priority.

## A11. Dock model and view split

**Findings.** REVIEW "Big widgets design": `DockState` is documented as pure data, but `scan`,
`drag`, `set_drag`, `drop_target` and `content_size` take `Ui`. `content_size` returns
`Option<Vec2>` where `Size` exists.

**Recommendation.** Move `scan` and `content_size` to `DockView` (the private ones follow);
`content_size` returns `Option<Size>`. Check `dock_tabs.rs:200` for the same `Vec2`. The id
derivations (`dock_id`, `pane_id`, `content_id`, `strip_id`, `splitter_id` and the static
`tab_key`) are view facts too, and go with them — `TabStrip::chip_id` / `close_id` already live on
the view.

## A12. TabbedView page identity

**Findings.** REVIEW "TabbedView reorder and identity": chips are keyed by index, so after a close
or reorder, slot `i`'s hover and look animation move to the page that slid into it.

**Options.** A key function, `TabbedView::keyed(|page: &S| -> u64)`, or a bound `S: Hash`.

**Decided 2026-10-04.** A builder setter `TabbedView::keyed(|page: &S| key)` whose key is any
`impl Hash`, hashed the way `DockState::tab_key` hashes a tab, so the two widgets derive chip
identity one way. Without it, index keys as today, so a static page list needs nothing. A bare
name, because `TabbedView` is a builder (A47).

## A13. One way to pick a look

**Findings.** REVIEW "Big widgets design": `ButtonTheme`, `TextEditTheme`, `TabsTheme`,
`ToggleTheme` and `MenuItemTheme` each have a public `pick` that duplicates `ThemeSlot::look`;
`ExpanderTheme` has none.

**Recommendation.** Remove the five `pick` methods; callers use `ThemeSlot::look`.

## A14. Attach-to-trigger naming

**Findings.** REVIEW "Small widgets design": `Tooltip::on(&snapshot)` and
`ContextMenu::attach(ui, &snapshot)` name one idea twice, with different argument shapes.

**Decided 2026-10-04.** `on(&snapshot)` for every overlay that attaches to a trigger, with the
snapshot first and required text second (A51), and no constructor that takes `ui`:
`Tooltip::on(&snapshot, text)`, `ContextMenu::on(&snapshot)`, and A23's `PopupTrigger::on`.
`ContextMenu::attach` goes; its auto-open on a right-click moves into `show(ui)`, which has the
`ui` it needs. `ContextMenu::for_id` stays for a menu that code opens with `ContextMenu::open`.

## A15. ColorButton parity with ColorPicker

**Findings.** REVIEW "Small widgets design": ColorButton lacks `swatches(&[RgbaF32])` and the
resolution setter (`downsample(n)` today, `texel_size(n)` after A28). Both take
`history(on: bool)`; the defaults differ on purpose (on for the button, whose doc says why), so
that half of the REVIEW finding is closed.

**Recommendation.** Add `swatches` and `texel_size` to `ColorButton` with `ColorPicker`'s
signatures.

## A16. A checked render target

**Findings.** REVIEW "Render target colour encoding is not enforced". `From<&wgpu::Texture> for
RenderTarget` accepts any texture, and a wrong format panics on the first frame, inside the
renderer, far from the line that chose the texture. `From<wgpu::TextureFormat> for TargetFormat` is
the same gap for a format named before a texture exists.

**Decided 2026-10-04.** `RenderTarget::new(&texture)` asserts the format (sRGB or float) and the
usage (`RENDER_ATTACHMENT`, plus `COPY_DST` where the host presents through its backbuffer), with
`#[track_caller]` and a `# Panics` section, as A50 rule 3 says for a caller contract.
`TargetFormat::new(format)` asserts the same format rule. Both `From` impls go, because `From` must
not fail; the entry points take `RenderTarget` itself instead of `impl Into<RenderTarget>`.

## A17. Cargo features for tests

**Findings.** TEST_REVIEW 8: the self dev-dependency enables `internals` in every test build, so
seven comments that say a plain `cargo test` is GPU-free are false, and `all(test, internals)`
equals `test`. REVIEW "Support-module docs": `lib.rs:209-212`.

**Options.**

1. Accept GPU tests in every run; delete the claims; drop the redundant gates and the
   `required-features` on `[[test]] alloc`.
2. Add a `gpu-tests` feature that the dev-dependency does not request.

**Decided 2026-10-04.** Option 1: every test run needs an adapter. A GPU test panics without
one, so the headless test server needs a software Vulkan driver (Mesa lavapipe, in
`mesa-vulkan-drivers`) installed once — an install for you to run, not part of this item. Delete
the seven false "GPU-free" claims, drop the `all(test, internals)` gates that equal `test`, and
drop `required-features` on `[[test]] alloc`; then update the AGENTS.md test line.

## A18. Icon set limits and names

**Findings.** REVIEW "`IconId` is u16 but icon sets are unbounded" and "`IconDef::name:
&'static str` forces `Box::leak`".

**Recommendation.**

- `IconDef::name` becomes an owned or interned string, so a set built from files does not leak.
- `from_svgs` returns `Result<Self, IconTableError>` (A50 rule 3: icon files are data), with
  three variants: an SVG it cannot read (today it drops it silently), more than `u16::MAX`
  icons, and a duplicate name.

## A19. Features the docs imply: IME and focus traversal

**Findings.** REVIEW "Stale input docs": docs mention IME text, but winit `Ime` is never enabled
or translated, and there is no Tab focus traversal. REDESIGN D16 fixes the docs.

**Recommendation.** Treat each as a feature with its own design: IME needs `InputEvent` variants
for preedit and commit; focus traversal needs a focus order (the `KeyClass::Focus` class it reads
exists since the key-class split). Out of scope for the defect work.

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

**Recommendation.** A public `PopupTrigger` beside `Popup`, spelled as A14 decided:
`PopupTrigger::on(&snapshot)` attaches, and `show(ui, ..)` probes the open flag, toggles it on a
click, closes it when the trigger is disabled, records the `Popup` below the trigger, and writes
the flag back only on a flip; `open()` answers and `close()` closes. Read `Popup` and
`ContextMenu` first and match their shape. Touches `ColorButton`, `ComboBox` and any app that
drops its own panel from a button.

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

---

# Surface review (2026-10-04)

The items below come from a review of the whole exported surface, listed in `API_SURFACE.md`
(rustdoc JSON of `59b93e30`). Each item names what reads inconsistent, asymmetric, or
non-canonical, and what to do about it. Where a finding extends an earlier item, the earlier item
took it: A7, A9, A10, A11, A15, A18 and A19. The plan at the end of the file orders all
items, old and new.

## A27. One response type for a value a widget writes

**Findings.** Widgets that write a bound value report it three ways. `Slider`, `DragValue`,
`Splitter`, `ColorButton`, `ColorField`, `ColorPicker` and `ColorStrip` return `ValueResponse
{ changed, committed }`. `RadioButton` and `ComboBox` return `SelectResponse { changed }`.
`Checkbox` and `Switch` toggle a `&mut bool` and return a plain `Response`, so a caller cannot
tell a toggle from a click on a disabled box without comparing the bool itself. `ExpanderResponse`
names the same flag `toggled`.

**Recommendation.** Every value-writing widget returns `ValueResponse`. A discrete pick commits at
once, so `RadioButton`, `ComboBox`, `Checkbox` and `Switch` set `committed == changed`. Remove
`SelectResponse`. Rename `ExpanderResponse::toggled` to `changed`.

**Touches.** `Checkbox`, `Switch`, `RadioButton`, `ComboBox`, `Expander`, `SelectResponse`, prelude.

## A31. Text arguments take one type

**Findings.** Every widget label and caption takes `impl Into<TextInput<'a>>`, so a caller passes
`&str`, `String`, an interned string or `fmt!` output alike. (Window titles take
`impl Into<String>`, correctly: they outlive the frame.) `TextEdit::placeholder(&str)` and
`DragValue::suffix(&str)` take only `&str`.

**Recommendation.** Both take `impl Into<TextInput<'a>>`. `TabItem::new(key, label: InternedStr)`
stays as it is: a `TabItem` is `Copy` data in a slice, so it cannot own a borrow.

## A33. Two modifier types that do not convert

**Findings.** `Modifiers { shift, ctrl, alt, mac_ctrl }` is event state; `ShortcutMods { ctrl,
shift, alt }` is binding vocabulary. The field order differs, and the conversion is
`ShortcutMods::from_event(m)` rather than `From<Modifiers>`. A9 adds `Modifiers` constants.

**Findings, continued.** TEST_REVIEW 10 (moved here from A9): 50 test literals spell
`Modifiers { ctrl: true, ..Modifiers::NONE }` because `Modifiers` has only `NONE`, while
`ShortcutMods` has `NONE`, `SHIFT`, `CTRL` and `CTRL_SHIFT`.

**Recommendation.** `impl From<Modifiers> for ShortcutMods` in place of `from_event`. Both types
order their fields `ctrl, shift, alt` (then `mac_ctrl` on `Modifiers`, and A10's `meta` on both),
and both carry the same constants: `NONE`, `SHIFT`, `CTRL`, `ALT`, `CTRL_SHIFT`.

## A34. Remove `Sums`

**Findings.** `Spacing::sums()` returns `widget::Sums { horizontal, vertical }`, which is a `Size`
under other names, beside `horizontal_sum()` and `vertical_sum()` that answer the same thing. Two
call sites use `sums()`.

**Recommendation.** Remove `Sums` and `Spacing::sums()`; the two call sites read the two methods.

## A35. Remove `From<Vec2>` and `From<Size>` for `Corners`

**Findings.** `Corners: From<Vec2>` reads `x` as both top radii and `y` as both bottom radii, and
`From<Size>` reads `w` and `h` the same way. Neither type means "top and bottom" anywhere else, so
the conversion is a guess a reader cannot make. `From<(top, bottom)>` and `Corners::top_bottom`
already state the pairing.

**Recommendation.** Remove both impls.

## A37. Seal `GradientGeometry`

**Findings.** `GradientGeometry` is a public, implementable trait whose items are renderer
internals: `DEFAULT_INTERP`, `axis_lanes` (the four shader lanes), `hash_geometry` (a cache key)
and `has_nan`. The renderer draws exactly three kinds, so an outside implementation cannot work,
and the four items are not something a user calls.

**Recommendation.** Seal it the way `widget::Lower` is sealed: the items move to a private
supertrait, and the public trait stays only as the bound on `Gradient<G>`.

## A38. Hide `UserEvent`

**Findings.** `UserEvent` is exported, and its doc says it is public only as the type parameter of
`EventLoopProxy`. But the proxy is a `pub(super)` field of `HostHandle`, so no public signature
names `UserEvent`, and a user can do nothing with it.

**Recommendation.** `pub(crate)`, and remove it from `lib.rs`.

## A39. Wrapper hooks on single widgets

**Findings.** Four public methods exist for in-crate wrappers, and the wrapped widgets' peers lack
them:

| Hook | For |
|------|-----|
| `TextEdit::adopt_placement(&Widget)` | `DragValue`'s inline editor; `Widget::adopt_placement` is the same thing one level down |
| `Separator::from_widget(Widget, Axis)` | `MenuSeparator` |
| `Popup::default_background(&Background)` | `ContextMenu` and `ComboBox` theming a popup's chrome after the caller's own settings |
| `Popup::anchored(Anchor)` | `ContextMenu`, which learns its anchor only in `show` |

**Recommendation.** One wrapper rule, written in AGENTS.md: *a wrapper holds the widget it wraps,
forwards `Configure` to it, and finishes it through that widget's public setters.* Under it:

- `adopt_placement` moves to `ConfigureWidget`, so every widget has it through `configure()`;
  `TextEdit::adopt_placement` and `Widget::adopt_placement` go.
- `MenuSeparator` holds a `Separator`, built horizontal in `MenuSeparator::new`;
  `Separator::from_widget` goes.
- `Popup::anchored` stays: it is an ordinary setter a holding wrapper needs, and it already
  mirrors `LayerScope::anchored`.
- `default_background` stays as the chrome peer of `ThemeDefaults::default_padding`, but on every
  widget that has `background(bg)`, not on `Popup` alone. **Decided 2026-10-04:** an inherent
  `default_background(bg)` beside the inherent `background(bg)` on `Block`, `Panel`, `Grid`,
  `Scroll`, `Popup`, `Modal`, `Tooltip` and `ContextMenu` — no trait. Because no trait keeps the
  eight in step, one table-driven test records each of them with only a default, with only a
  background, and with both, and asserts the painted fill each time.

## A40. Remove `MenuItem::separator`

**Findings.** `MenuItem::separator()` returns a `MenuSeparator`, which `MenuSeparator::new()`
already builds.

**Recommendation.** Remove `MenuItem::separator`.

## A41. `Popup` mirrors `Anchor` but not all of it

**Findings.** `Popup::below`, `above`, `left_of` and `right_of(rect)` are `Popup::new(Anchor::…)`
spelled shorter, but `Anchor::at_point` has no `Popup` twin, so the one placement a menu at the
pointer needs is the one that has to be spelled long.

**Recommendation.** Add `Popup::at_point(point)`, so the shorthand covers every `Anchor`
constructor.

## A43. One way to configure the windowed host

**Findings.** `WinitHostBuilder` has a setter per setting, a `config(WinitHostConfig)` that takes
the same settings as one struct, and `title` beside `window(WindowConfig)`, which also carries the
title. `OffscreenHostBuilder` has setters only.

**Recommendation.** Remove `WinitHostConfig` and `WinitHostBuilder::config`, so both builders
configure the same way. Keep `title` as the documented shorthand for the bootstrap window.

## A48. `Mesh::with_known_bbox` trusts its caller silently

**Findings.** `Mesh::with_known_bbox(bbox)` skips the lazy bounding-box computation, and its doc
says a wrong box "silently breaks scissor culling". It is the one public setter in the crate that
can make paint wrong without a check, and it saves only the lazy computation the mesh does anyway.

**Recommendation.** Remove it. If a measured workload needs it, keep it with a `debug_assert!` that
every vertex lies inside the box.

## A49. `Ui::escape_pressed`

**Findings.** `Ui::escape_pressed()` is `key_pressed(Shortcut::key(Key::Escape))` and nothing more,
the only key with its own method.

**Recommendation.** Remove it; `Modal` and the other callers spell the shortcut. Low priority.

## A50. One validation model for every public input

**Findings.** An audit of every public function that takes a number, a range, an index or a value
type (about 260, from the rustdoc JSON of `59b93e30`) finds seven different behaviours for an
invalid input. The same kind of value gets different ones: a negative length panics in
`Sizing::fixed`, is a debug-only check in `padding`, is silently clamped in `Splitter::min_pane`,
paints nothing as a `Stroke::width`, and is a deserialization error in a theme file.

| Behaviour today | Examples |
|---|---|
| Release panic on a per-frame authoring call (the guide allows release asserts only on cold paths) | `Sizing::{fixed, fill, share}`, `Track::{min, max}`, `gap` / `line_gap`, `min_size` / `max_size` bounds, `TranslateScale::new` and `from_*`, `Slider::new` (finite range), `Slider::step`, `Stop::new` / `GradientBuilder::stop`, `Scroll::zoom_by`, `PaintAnim::steps`, `ColorField` / `ColorStrip` / `ColorPicker::downsample`, `FontWeight::new`, `AnimSpec::{duration, spring}`, `ImageHandle::update` (wrong size) |
| Release panic on *data* (the guide: untrusted data is never an assert) | `ComboBox` and `TabbedView` panic when the bound index is out of range — a stale index after the app removed an option, and every `TabbedView` with zero pages; `Image::from_srgba8` asserts the byte length of pixels that usually come from a decoder; `FontFamily::named` panics when the family table is full, for a name that usually comes from configuration (the fallible `try_named` is crate-only) |
| Debug-only check, nothing in release | `padding` / `margin` (`debug_assert!` on NaN, so a release NaN enters layout), `Rect::from_min_max` |
| No check (one of them corrupts the bound value) | `DragValue::speed(NaN)`: the first drag stores `-inf` in an unbounded `f64` (NaN offset → the clamp's `max(-inf)`); `Configure::position` NaN; negative `padding`; `Display::from_physical` with a zero or NaN scale; `Hsv::new` / `Okhsv::new` out of range |
| Silent coercion, no report in debug either | `GridCell::span(0, _)` → 1; `Splitter::min_pane` and `DockView::min_pane` clamp; `Spinner::diameter`, `Separator::thickness` and every theme length through `themed_length`; reversed `DragValue` ranges ordered by `Limits`; `IconTable::from_svgs` drops an SVG it cannot read, so a malformed icon file vanishes without an error |
| Fallible | `UserScale::new`, `ZoomFactor::new` (`Option`); `Ui::load_image`, `Ui::load_font` (`Result`); theme files through `checked::*`, `TextStyle` / `TextStyleOverrides` `try_from`, `AnimSpec` and `DockState` deserializers |
| Public fields that skip every constructor check | `Rect`, `Size`, `RgbaF32`, `Stroke`, `Shadow`, `GridCell`, `TextStyle`, `Display`, `DockSplit`, `TabItem`, `WindowConfig`, every `Theme` field (a theme file is checked; a theme built in code is not) |

No rule says which behaviour a new setter gets, so each author picked one, and some cases are
caller bugs that are hidden (`GridCell::span(0, _)`) while others are ordinary data that crashes
(`ComboBox` with a stale index).

**Established practice.**

- WPF, whose layout contract this crate follows, asks two separate questions of every property
  value: a `ValidateValueCallback` (is the value valid at all — `Width` must not be negative; an
  invalid value throws) and a `CoerceValueCallback` (does a valid value fit its context —
  `Slider.Value` is pulled into `[Minimum, Maximum]` silently).
- Rust API Guidelines C-VALIDATE: prefer static enforcement through types; otherwise document a
  `# Panics` section or return `Result`; use `debug_assert!` where a check is too costly for
  release.
- Flutter checks constructor arguments with debug-only asserts, because widgets are built every
  frame.
- This repository's guide: `debug_assert!` on per-frame paths, release `assert!` only for public
  misuse outside hot paths and for cold configuration, `Result` for untrusted data, never an
  assert on data.

**Recommendation.** One model, in four rules, and one mechanism that implements it.

1. **Two questions, as WPF asks them.** *Validation* is about the value alone: finite,
   not negative, a power of two. *Coercion* is about the value against its context: inside a
   range, an index that exists, a `min` below its `max`. Each input names which question each of
   its rules belongs to.
2. **Coercion is total and silent, always.** It never asserts and never panics, because the
   context is data: an option list shrinks, a saved ratio comes from an older layout, a range
   comes from a settings file. Its result is documented on the API. Concretely: `ComboBox` and
   `TabbedView` show a stale index as the last option, without writing the clamped index back
   (**decided 2026-10-04**: the bound `usize` changes only when the user picks, and
   `ValueResponse::changed` stays `false`); with no options, `ComboBox` shows an empty chip and
   `TabbedView` records its strip with no page;
   reversed ranges are ordered (`Limits`, crate-wide); a fraction is clamped to `0..=1` with NaN
   as `0`. A zero `GridCell` span is not context but a caller bug, so it is a `count` under
   rule 3, not a coercion.
3. **Validation depends on where the value comes from.**
   - *Per-frame authoring* — builder setters and the value constructors a record pass calls
     (`Sizing`, `Track`, `Corners`, `Spacing`, `Stroke`, `TranslateScale`, `AnimSpec`,
     `Shape::*`): a release `assert!` with `#[track_caller]`, whose message names the value's
     kind and its rule, documented under `# Panics`. **Decided 2026-10-04:** a release panic,
     not a neutral value — never quietly wrong. This is a deliberate exception to the global
     guide's "`debug_assert!` on hot paths" for one case, public input validation, and AGENTS.md
     states it. The cost is one comparison per value per frame. The consequence for apps: a
     value *computed* at run time (a `0 / 0` thickness) must go through a coercing kind or an
     `is_*` predicate before it reaches a validating setter, or it crashes the app — which is why
     every validating kind also exports its predicate.
   - *Cold configuration* — host builders, `Theme::scale_text`, dock configuration, icon tables:
     a release `assert!` documented under `# Panics`.
   - *Data from outside the program* — files, persisted settings, decoded images, numbers a user
     typed: fallible. `Option` when one rule can fail (`UserScale::new`), `Result` with an error
     enum when several can (`Image::from_srgba8` and `IconTable::from_svgs` become `Result`).
     `FontFamily::named` returns `Option`, the public form of today's `try_named`. Serde goes through
     the same predicates.
4. **Plain data stays plain.** `Rect`, `Size`, `RgbaF32`, `Stroke`, `Shadow` and `Spacing` keep
   public fields: arithmetic passes through invalid intermediate values (a negative width out of
   a subtraction) on its way to a valid one. They are checked where they *enter* a widget, a
   shape or a node, by the same kind functions. Types whose consumers rely on an invariant keep
   or get private fields with a checked constructor — today's `Sizing`, `Track`,
   `TranslateScale`, `UserScale`, `ZoomFactor`, `FontWeight`; and also `GridCell` (spans) and
   `DockSplit` (ratio), which have public fields now. Theme fields stay public and are read
   through the kind functions (what `themed_length` does today, for lengths only).

**The mechanism (landed).** The public `widget::domain` module, which replaced `widget::approx`
and `F32Ext`: one public home for every scalar rule. Each validating kind is two `const fn`s — an `is_*` predicate and an asserting
checker — with one message; each coercing kind is one total `const fn`. Public, because AGENTS.md
lets a widget reach only the public API, and a widget outside the crate validates its own setters
and reads theme values through the same functions:

```rust
/// A distance: finite and not negative.
pub const fn is_length(v: f32) -> bool {
    v.is_finite() && v >= 0.0
}

/// `v`, which must be a length.
///
/// # Panics
///
/// Panics unless [`is_length`]`(v)`.
#[track_caller]
pub const fn length(v: f32) -> f32 {
    assert!(is_length(v), "a length must be finite and not negative");
    v
}
```

Every setter calls its kind: `Spinner::diameter(px)` stores `domain::length(px)`, `gap` stores
`domain::gap(g)`. The serde validators in `primitives::packed::serde::checked` become thin
wrappers over the same `is_*` predicates, so a file and a call site cannot disagree.
`#[track_caller]` puts the panic on the caller's line, and the message stays a constant so the
function stays `const`.

| Kind | Rule | On a value outside it | Used by |
|---|---|---|---|
| `offset` | finite | panic | margin, position, translation, shadow offset |
| `length` | finite, ≥ 0 | panic | padding, thickness, diameter, stroke width, radius, font size (`0` shapes nothing, as a sub-epsilon size does today), `Sizing::fixed`, `Sizing::share` |
| `extent` | ≥ 0, `+inf` allowed | panic | `max_size` |
| `gap` | length ≤ 65 504 (the f16 lane) | panic | `gap`, `line_gap` |
| `positive` | finite, > 0 | panic | scales, zoom factors, slider step, drag speed, fill weights |
| `angle` | finite | panic | gradient angles, arc angles |
| `color` | every channel finite (HDR values above `1` stay valid: tween outputs reach them) | panic | every `RgbaF32` that enters a shape, a look or a widget |
| `count` | ≥ 1, or a power of two in a range | panic | paint steps, `GridCell` spans, `texel_size` (A28) |
| `range` | both ends finite | panic; the *order* is coerced (`Limits`) | `Slider::new`, `DragValue::range`, `ZoomConfig::new` |
| `fraction` | `0..=1` | coerced: clamped, NaN → `0` | progress, split ratio, `Hsv` / `Okhsv` saturation and value |
| `turn` | `0..1` | coerced: wrapped, NaN → `0` | `Hsv` / `Okhsv` hue, `ColorCoords` fallback hue |
| `index` | `0..len` | coerced for display: clamped, never written back; no index when `len == 0` | `ComboBox`, `TabbedView`, `TabStrip::selected` |

Every numeric parameter's doc names its kind ("`px`: a *length*"), and the crate docs carry the
table. `ImageHandle::update` with a wrong size keeps its release panic: it is a `count`-like
contract on a whole image rather than a scalar, and the rule is the same.

**Tests.** The theme suite's `file_values` walk already proves one property for files: every
number is rejected on load or safe to render. The same property for code: one table per kind feeds
`NaN`, `±inf`, `-1`, `0` and the boundary values through every setter of that kind, and asserts
either the kind's panic message (`panic_probe::assert_panics_with`) or the coerced value. One more
test records a frame from every coercing input at its worst (NaN fractions, stale indices, reversed
ranges) and asserts that no NaN reaches layout or paint.

**Decided 2026-10-04.** A per-frame contract violation panics in release (rule 3), and a stale
selection shows as the last option without being written back (rule 2).

**Touches.** Every builder setter and value constructor listed above; `primitives::packed::serde::checked`;
`ComboBox`, `TabbedView`, `Image`, `IconTable` (with A18, whose two new rejections become this
`Result`'s variants), `GridCell`, `DockSplit`; the crate docs. Split into one go-ahead
for the `domain` module plus the rules, and then one per area (layout, paint, widgets, host).

## A51. Where a widget's text goes

**Findings.** Required text goes in the constructor (`Text::new(text)`, `Expander::new(label)`,
`MenuItem::new(label)`); optional text goes through `.label(..)` (`Button`, `Checkbox`, `Switch`,
`RadioButton`). `Tooltip` breaks the pattern: its text is required — an empty one records no
bubble — but it arrives through `Tooltip::on(&snapshot).label(..)`.

**Recommendation.** One rule, written in AGENTS.md: required text in the constructor, optional
text through `.label`. `Tooltip` takes its text in the constructor:
`Tooltip::on(&snapshot, text)`, as A14 decided; `Tooltip::label` goes.

---

# Implementation plan

Every step below is one go-ahead and one commit. A step:

- follows the rules this file adds to AGENTS.md (step 1.5) and the existing ones (read the
  neighbours before settling a signature; no shims, no compat aliases);
- updates every caller in `src/`, `tests/`, `benches/` and the showcase, and the docs that name
  the old item;
- adds or extends the tests the item names, with hand-derived expected values;
- runs the AGENTS.md verification chain, plus the visual suite when it moves pixels or layout, and
  ends with a look at the showcase when a user can see the change;
- regenerates `API_SURFACE.md` and deletes the items it closes from this file.

## Phase 0 — decisions before any code

All seven are decided. Each item named here carries its decision in its own text.

| Decision | Item | Recommendation | Decided (2026-10-04) |
|---|---|---|---|
| Release behaviour of a per-frame contract violation | A50 | the kind's neutral, with a debug panic | **a release panic**, with an `is_*` predicate per kind |
| A stale selection index | A50 | clamp to the last option | **clamp to the last option for display; the bound index is not written back** |
| The attach-to-trigger verb and argument order | A14, A51, A23 | one verb for `Tooltip` and `ContextMenu`, the snapshot first and the text second | **`on(&snapshot)`**, text second, no `ui` in a constructor |
| Page identity in `TabbedView` | A12 | an optional key function, index keys by default | **`.keyed(\|page\| impl Hash)`**, index keys by default |
| Fallible render target conversion | A16 | reject: no application chooses a target format at run time | **a checked `RenderTarget::new` / `TargetFormat::new`**; the `From` impls go |
| GPU tests in every `cargo test` | A17 | option 1 | **option 1**; the test server needs lavapipe installed |
| How the chrome setters are shared | A39 | a `Chrome` trait | **no trait**: inherent `default_background` on all eight, kept in step by one test |

## Phase 1 — foundations the later phases build on

1. Done: the `domain` module (A50 mechanism, A36).
2. Done: the `const` sweep (A24).
3. Done: flag sets (A32).
4. Done: no strum on public types (A6).
5. Done: the chainer, wrapper, text and validation rules are in AGENTS.md, and
   `scripts/api_surface.py` regenerates `API_SURFACE.md`.

## Phase 2 — renames and removals

Each line is one commit; none depends on another inside the phase.

1. Done: names (A28, A29, A30, A42, A45, A46).
2. Done: chainers (A47, A44).
3. **Removals.** A8 (`WinitHostError::Gpu`), A13 (the five `pick` methods), A34 (`Sums`), A35
   (`Corners` from `Vec2` / `Size`), A38 (`UserEvent`), A40 (`MenuItem::separator`), A43
   (`WinitHostConfig`), A48 (`Mesh::with_known_bbox`), A49 (`Ui::escape_pressed`).
4. **Argument types.** A31 (`placeholder` and `suffix` take `TextInput`), A33 (`From<Modifiers>`,
   one field order, one constant set) with A9 (`Panel::stack`, `Widget::stack`), A37 (seal
   `GradientGeometry`).
5. **Test features** (A17): the false claims, the redundant gates and the `alloc`
   `required-features` go.

## Phase 3 — structural API

1. **One value response** (A27): `Checkbox`, `Switch`, `RadioButton` and `ComboBox` return
   `ValueResponse`; `SelectResponse` goes; `ExpanderResponse::changed`.
2. **Wrappers** (A39): `ConfigureWidget::adopt_placement`; `MenuSeparator` holds a `Separator`;
   `default_background` on the eight chrome-bearing widgets, with the test that keeps them in
   step.
3. **Overlays**, in this order: A14 with A51 (the attach verb; `Tooltip` takes its text in the
   constructor), A41 (`Popup::at_point`), then A23 (`PopupTrigger`, in A14's argument order;
   `ColorButton` and `ComboBox` move onto it).
4. **Dock and tabs**: A11 (model and view split, ids move to `DockView`), then A12 (`TabbedView`
   keys).
5. **Colour button** (A15), after phase 2 step 1 renamed `downsample`.

## Phase 4 — the validation rollout (A50 rules 2–4)

One area per commit. Each adds its setters to the per-kind input tables of phase 1 step 1.

1. **Coercion** (rule 2), first, because it removes release panics on data: `ComboBox`,
   `TabbedView` (and its zero-page case) and `TabStrip::selected` coerce their index; `Limits`
   orders every range; fractions and turns coerce.
2. **Layout**: `Sizing`, `Track`, `gap`, `line_gap`, `min_size`, `max_size`, `padding`, `margin`,
   `position`, `TranslateScale`; `GridCell` gets private fields with `with_span`. Every check in
   the area panics with its kind's message; the debug-only ones (`padding`, `margin`) become
   release asserts, and the unchecked ones (`position`) gain one.
3. **Paint**: shape constructors, `Stroke`, `Corners`, `Shadow`, colours where they enter a shape
   or a look, `Stop` / `GradientBuilder::stop`, `PaintAnim`, `ImageHandle::update`.
4. **Widgets**: every remaining widget setter (`Spinner`, `Separator`, `DragValue::speed`,
   `Slider`, `Scroll::zoom_by`, `ZoomConfig::new`, `texel_size`, text sizes) and `AnimSpec`.
   Theme values read through `domain` where they are used.
5. **Data and host**: `Image::from_srgba8` and `IconTable::from_svgs` (with A18) return `Result`;
   `FontFamily::named` returns `Option`; A21's `FontLoadError::FamilyTableFull`; `DockSplit` gets
   a private, checked ratio; `Display::from_physical` validates its scale; A16's checked
   `RenderTarget::new` and `TargetFormat::new`.
6. **The frame property**: a test records one frame from every coercing input at its worst and
   asserts that no NaN reaches layout or paint.

## Phase 5 — goldens

A7, A25 and A26 together: `golden` re-exports `image`, `render*` returns the `FrameReport`,
`Tolerance { max_delta, max_pixels }` with `EXACT` as default, the adapter sidecar and the orphan
report. Downstream suites change once.

## Phase 6 — features

1. **A22** wheel sense per axis, after phase 1 step 3.
2. **A20** keyboard on toggles and ranges, after phase 3 step 1.
3. **A19** IME and focus traversal, each its own design.
4. **A10** the `meta` modifier, after phase 2 step 4.

## Order at a glance

Phase 0 decides; phase 1 must land first; phases 2 and 3 may interleave, except where a step names
a predecessor; phase 4 needs phase 1 step 1 and phase 3 step 1 (the index coercion touches
`ComboBox`, which A27 changes); phase 5 and phase 6 are independent of each other.
