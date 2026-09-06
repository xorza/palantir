# Public API review — palantir 0.5.0

> **Status: applied.** Every item below is in the tree, except the three marked
> **[withdrawn]** — findings that did not survive a closer read. Two corrections
> to the original review are marked **[correction]**.

Method: `cargo public-api --simplified` gave the full surface. The dump has
8782 lines. After the removal of derive noise, 3217 public functions remain
across 176 structs, 47 enums, 11 traits and 55 type aliases.

Reference points used: the Rust API Guidelines checklist (C-GETTER, C-CONV,
C-BUILDER), the Rust Design Patterns builder chapter, and egui's response
surface.

---

## 1. What is already correct

Say this first, because most of the surface is good.

- `Ui::on_input` is `pub(crate)`. The old defect is gone. `Ui::frame`,
  `Ui::set_window_facts`, `Ui::seed_vsync` and `Ui::drain_window_output` are
  all crate-private, and the file states the rule.
- Every `Ui` field is private. The doc says why, and it binds inside the
  module too.
- `Configure` and `ThemeDefaults` give all 30+ widgets the same 36 layout
  setters. No widget deviates.
- Every widget takes `style(impl Into<Option<&XTheme>>)`. Every theme has
  `from_palette(&Palette)`. Both are uniform.
- The `peek_*` versus plain-read watch contract is a strong design. A plain
  read declares a wake. A `peek_*` read does not. The file states the rule
  before the methods.
- `RgbaF32` is linear everywhere. The sRGB constructors say so.
- The `internals`, `bench`, `showcase` and `golden` gates keep test reach-ins
  out of the shipped surface.

---

## 2. Structural — the root serves two audiences at once

The crate root publishes the app-author API and the widget-author API in one
flat list. A person who wants a button reads 176 structs to find it.

The widget-author items are these, and there are more:

| item | what it is for |
| --- | --- |
| `Widget` (25 methods, 13 `authored_*` getters) | the node a custom widget records |
| `ConfigureWidget` | the `&mut` half of `Configure` |
| `Response::eager(WidgetId, &Ui, ResponseState)` | how a widget closes its own response |
| `Lower`, `Shape`, and 9 `*Shape` types | paint recording |
| `TabStrip::chip_id`, `TabStrip::close_id` | child id derivation |
| `DockState::content_id` / `pane_id` / `splitter_id` / `strip_id` / `tab_key` | child id derivation |
| `TextEditTheme::corner_centring` | in-place edit placement |
| `Display::raster_eq` | swapchain cache identity |
| `Mesh::content_hash`, `TextProbe::hash_of`, `TextProbe::text_hash` | cache identity |
| `Rect::scaled_by(f32, bool)` | the renderer's logical→physical step |
| `approx::EPS` / `approx_zero` / `noop_f32` / `ratio` / `vec2_approx_eq` | float tolerance |
| `F32Ext` | theme and pointer scalars |
| `GpuInitCtx`, `GpuFrameCtx`, `GpuPaint` | custom GPU passes |

The project rule "widgets use the public API" forces each of these to be
public. That rule is right. The result is that the published surface mixes
plumbing with the API an application types.

**Suggestion.** Move the authoring surface into one module — `palantir::widget`
— and keep the crate root for the application surface. This is a move, not a
re-export, so one canonical path per item still holds. The application root
then holds about 60 items. The widget module holds the rest, with a module doc
that says who it is for.

The alternative is a rustdoc section grouping. That helps the docs page only.
It does not help a person who reads the `use` list at the top of a file.

---

## 3. The click predicate

`ResponseState` gives these derived predicates:

| method | derivation |
| --- | --- |
| `hovered()` | `pointer_over && !disabled` |
| `pressed()` | `left.held() && hovered()` |
| `press_fraction(band)` | the primary button's gesture |
| `any_clicked()` | any of the three buttons |

There is no `clicked()`. The canonical spelling is `resp.left.clicked()`.

This is the most common call an application makes. egui spells it
`if ui.button("click me").clicked()`. Palantir spells it
`if Button::new().label("click me").show(ui).left.clicked()`.

The family is also inconsistent with itself. `pressed()` means the left
button. Its click peer does not exist, but the any-button click peer does.

**Suggestion.** Add to `ResponseState`:

```rust
pub fn clicked(&self) -> bool { self.left.clicked() }
pub fn double_clicked(&self) -> bool { self.left.double_clicked() }
```

Keep `any_clicked()`. Then `Button::new().show(ui).clicked()` works through the
`Response` deref, and `pressed()` / `clicked()` name the same button.

### 3.1 Redundant disabled guards inside the crate

`ResponseState::merge_disabled` already clears `left`, `right`, `middle` and
`scroll`. Both paths that build a response call it — `Ui::response_for` folds
the ancestor flag, and `Widget::response` folds the node's own.

So `left.clicked()` is already false on a disabled widget. Yet six in-crate
sites still guard it by hand:

- `src/widgets/drag_value/mod.rs:277`
- `src/widgets/color_button/mod.rs:116`
- `src/widgets/radio/mod.rs:84`
- `src/widgets/toggle_chrome/mod.rs:70`
- `src/widgets/combo_box/mod.rs:169`
- `src/widgets/expander/mod.rs:179`

Five other sites omit the guard. The split shows that the contract is not
obvious from the API. `examples/custom_widget.rs` also writes the redundant
guard, so the example teaches it.

**Suggestion.** Remove the redundant guards. State on `ButtonState` and on
`ResponseState::disabled` that the interaction half is already empty when
`disabled` is true.

---

## 4. Constructors that read as setters

### 4.1 `ColorStrip`

```rust
ColorStrip::hue(&'a mut ColorCoords) -> Self      // constructor
ColorStrip::alpha(&'a mut RgbaF32) -> Self        // constructor
ColorPicker::alpha(self, bool) -> Self            // setter
ColorButton::alpha(self, bool) -> Self            // setter
```

`alpha` is a constructor on one colour widget and a setter on two neighbours.

**Suggestion.** `ColorStrip::for_hue(...)` and `ColorStrip::for_alpha(...)`.

### 4.2 `ComboBox` and `TabbedView` have no `new` — **[withdrawn]**

**[correction]** Both *do* have `new`. They sit in an
`impl<'a, S: AsRef<str>>` block that my first extraction dropped, because the
regex could not balance the `>` inside the bound. `labeled` is a second
constructor for rows that carry a label rather than being one — a good design.
No change made.

### 4.3 `MenuSeparator` has no constructor

The only way to build one is `MenuItem::separator()`, a factory on a different
type.

**Suggestion.** Add `MenuSeparator::new()`. Keep `MenuItem::separator()` as
sugar, or remove it.

### 4.4 Two verbs for one idea

```rust
Tooltip::on(&'a ResponseSnapshot) -> Self
ContextMenu::attach(&mut Ui, &ResponseSnapshot) -> Self
```

Both hang an overlay on a trigger response. Pick one verb.

---

## 5. `Anchor` is public, but `Popup` refuses one

`Popup::new(Anchor)` is private (`src/widgets/popup/mod.rs:115`). The five
public constructors are thin wrappers:

```rust
Popup::anchored_to(p) -> Self::new(Anchor::at_point(p))
Popup::below(r)       -> Self::new(Anchor::below(r))
Popup::above(r)       -> Self::new(Anchor::above(r))
Popup::left_of(r)     -> Self::new(Anchor::left_of(r))
Popup::right_of(r)    -> Self::new(Anchor::right_of(r))
```

`Popup::gap(f32)` duplicates `Anchor::gap(f32)`. An application that holds an
`Anchor` — from `LayerScope::anchored`, say — cannot build a `Popup` from it.

Three names also exist for one idea:

| name | owner |
| --- | --- |
| `Anchor::at_point(Vec2)` | `Anchor` |
| `Popup::anchored_to(Vec2)` | `Popup` |
| `LayerScope::fixed_at(Vec2)` | `LayerScope` |

**Suggestion.** Make `Popup::new(Anchor)` public. Keep the four rect wrappers
as sugar. Remove `Popup::anchored_to` and `Popup::gap`, because
`Popup::new(Anchor::at_point(p).gap(4.0))` says the same thing once. Rename
`LayerScope::fixed_at` to `at_point`.

---

## 6. Axis vocabulary — **[withdrawn]**

**[correction]** On a closer read this is principled, not accidental. `Panel`'s
constructors name *layout modes* and come with `zstack` and `canvas`, which have
no axis at all — `hstack`/`vstack` belong to that family. `Scroll`, `Separator`
and `Splitter` name an *axis*, and all three already agree on
`horizontal`/`vertical`. No change made.

The one residue is that `Axis` is public and no widget constructor takes one.
Left as is.

<details><summary>Original finding</summary>


| widget | words |
| --- | --- |
| `Panel` | `hstack`, `vstack`, `zstack`, `wrap_hstack`, `wrap_vstack`, `canvas` |
| `Widget` | the same six |
| `Scroll` | `horizontal`, `vertical`, `both` |
| `Separator` | `horizontal`, `vertical` |
| `Splitter` | `horizontal`, `vertical` |
| `GridCell` | `along(Axis, u16)` |

`Axis` is a public enum. No widget constructor takes one.

</details>

---

## 7. Colour — **[withdrawn]** on the `hex` half

**[correction]** `RgbaU8::hex` already decodes sRGB and stores the linear
bytes, exactly as `RgbaF32::hex` decodes to linear floats. All three `hex`
constructors agree on what a literal means. There was no trap, and nothing was
removed.

What was real is below.

### 7.1 Gradient stops spoke a different colour type — **applied**

```rust
Stop::new(f32, impl Into<RgbaU8>) -> Self
Stop::color(self) -> RgbaU8
GradientBuilder::stop(self, f32, impl Into<RgbaU8>) -> Self
Gradient::two_stop(f32, impl Into<RgbaU8>, impl Into<RgbaU8>) -> Self
Mesh::vertex(&mut self, Vec2, impl Into<RgbaU8>) -> u32
Mesh::filled_polygon(&[Vec2], impl Into<RgbaU8>) -> Self
```

Everything else on the surface takes `RgbaF32`. A person who builds a gradient
from theme colours meets a second colour type at that one boundary.

**Applied.** `Stop::new`, `Stop::color`, `GradientBuilder::stop` and the three
`two_stop` constructors now take and return `RgbaF32`. The `RgbaU8` stays as
`Stop`'s private storage — which is the rule the type's own doc already stated
for `offset`, now held on both fields.

`Mesh` and `MeshVertex` keep `impl Into<RgbaU8>`: there the byte form *is* the
public type (`MeshVertex.color` is a `Pod` vertex field), so the line is
"private storage takes `RgbaF32`, a published byte layout takes `RgbaU8`".

**Wire format break.** A stop's `color` now serializes as the sRGB hex string
every other theme colour uses (`color: "#22ccdd"`) instead of a linear-byte
struct. A theme file with gradients needs updating. This went beyond the
original item, and is worth calling out: hand-authored linear bytes were a trap
of their own, since an author would write the sRGB bytes they know.

### 7.2 Gradient constructor names did not match — **applied**

```rust
Gradient<LinearGeometry>::two_stop(f32, a, b)
Gradient<ConicGeometry>::two_stop_centered(a, b)
Gradient<RadialGeometry>::two_stop_centered(a, b)
```

The linear one takes an angle. The other two take nothing. All three are now
`two_stop`: the geometry already says what is centred.

---

## 8. "style" carries two meanings

`.style()` on every widget means "theme override":

```rust
Button::new().style(&theme.button)
Text::new("x").style(&theme.text)     // takes a &TextStyle
```

`style` elsewhere means slant:

```rust
TextShape::style(impl Into<FontStyle>)     // Normal | Italic
TextStyle::with_style(FontStyle)
TextStyle { style: FontStyle, .. }         // public field
```

So `Text::style(&text_style)` takes a `TextStyle` whose `style` field is
italic-or-not.

**Suggestion.** Rename the slant. `TextShape::slant`, `TextStyle::with_slant`,
field `slant`. Or use the plain word: `TextShape::italic(bool)`. Keep `.style()`
for theme overrides only.

---

## 9. Response wrappers

`Response` and `ResponseSnapshot` deref to `ResponseState`. Seven wrappers do
not, and a pinning test in `src/widgets/response.rs:165` denies it on three of
them.

| type | payload field | carries a `Response` |
| --- | --- | --- |
| `InnerResponse<R>` | `inner` | yes |
| `ValueResponse` | — | yes |
| `SelectResponse` | — | yes |
| `TextEditResponse` | — | yes |
| `ExpanderResponse<R>` | `inner: Option<R>` | yes |
| `TabStripResponse` | — | yes |
| `TabbedViewResponse` | — | yes |
| `OverlayResponse<R>` | `inner` | **no** |

So `Button::show(ui).hovered()` compiles but `Slider::show(ui).hovered()` does
not. The second needs `.response.hovered()`.

The non-deref is a stated decision, and the reason given — keeping the body
result distinguishable — is sound for `InnerResponse`. It is weaker for
`ValueResponse`, `SelectResponse` and `TextEditResponse`, which carry no body
result at all, only flags.

`OverlayResponse` is the one real gap. It is the only wrapper with no
`Response`, so a popup body's own hover and click state is unreachable.

**Suggestion.** Add `response: Response<'a>` to `OverlayResponse`, or say in
its doc why an overlay has none. Consider a deref for the three flag-only
wrappers.

---

## 10. `Tooltip::show` returns nothing

Every other `show` returns a response type. `Tooltip::show(self, &mut Ui)`
returns `()`.

**Suggestion.** Return `Response<'_>`, or a small `TooltipResponse` that
reports `visible`.

---

## 11. Two ways to set one thing

### 11.1 Presentation pacing

```rust
WinitHostBuilder::vsync(Vsync)
WinitHostBuilder::present_mode(wgpu::PresentMode)
```

Both write the same slot. The last call wins, silently. The docs on each point
at the other, but neither states the precedence.

**Suggestion.** State the precedence on both, or make `present_mode` an escape
hatch that `vsync` cannot overwrite.

### 11.2 Scrollbar mode

```rust
Scroll::bar_mode(BarMode)
Scroll::hide_bars()
Scroll::overlay_bars()
```

The last two are shorthands for `BarMode` variants.

**Suggestion.** Keep `bar_mode` only. `BarMode::Hidden` is as short to type and
is one thing to learn.

---

## 12. `with_` prefix is inconsistent on widgets

Value types use `with_`:

`Background::with_shadow`, `Background::with_stroke`, `Shadow::with_spread`,
`RgbaF32::with_alpha`, `Gradient::with_interp`, `Gradient::with_spread`,
`TextStyle::with_color`, `Mesh::with_known_bbox`, `ColorCoords::with_model`,
`ContextMenuTheme::with_radius`.

Widget builders use bare names: `Button::label`, `Scroll::content_margin`,
`Slider::step`, `DragValue::speed`.

Two widget methods break that: `Scroll::with_zoom()` and
`Scroll::with_zoom_config(ZoomConfig)`.

**Suggestion.** Rename to `Scroll::zoom()` and `Scroll::zoom_config(...)`.

---

## 13. Smaller items

| item | problem | suggestion |
| --- | --- | --- |
| `Rect::scaled_by(self, f32, bool)` | a bare `bool` at a call site says nothing. Its doc says it serves the renderer | split into `scaled_by(f32)` and `scaled_snapped_by(f32)`, or move it to the widget module |
| `Size::scaled(f32)` vs `Corners::scaled_by(f32)` vs `Rect::scaled_by(...)` | two names for one operation | pick `scaled_by` |
| `Spacing::horiz()`, `Spacing::vert()`, `Sums::horiz`, `Sums::vert` | abbreviations, against `horizontal` / `vertical` elsewhere | spell them out |
| `Image::from_rgba8(u32, u32, Vec<u8>)` vs `Image::blank(UVec2)` and `Image::size() -> UVec2` | two spellings of one size | take `UVec2` |
| `approx::noop_f32(f32) -> bool` | "noop" tells a reader nothing. It answers "does this paint" | rename `paints_nothing` |
| `approx::ratio(f32, f32) -> f32` | the name does not say that a collapsed divisor yields zero | rename `share_of` or keep and lead the doc with the zero rule |
| `Ui::state_mut` | there is no `state`, only `try_state`. Rust reads `try_x` as the fallible `x` | rename `state_or_default` / `try_state` / `try_state_mut` |
| `Ui::set_cursor` | no `cursor()` reader. Every other level pair on `Ui` has both | add `Ui::cursor()`, or say in the doc why not |
| `Shape` | a unit struct used as a namespace. `Shape::rect` returns `RectShape`, never a `Shape` | keep it, but say in the doc that it is a namespace |
| `PaintAnim::alpha` / `with_alpha`, `turn` / `with_turn` | one word is a constructor, the other a setter | acceptable, but name the pair in the doc |
| `DockView::run` beside `DockView::new(...).show(...)` | two paths to one frame | documented and justified. No change |
| `ContextMenu` owns its open state in `Ui`. `Popup` and `Modal` make the caller own it | three overlays, two state models | defensible, because a right-click menu is a gesture. State the split in the module doc |

---

## 14. Bitflags publishes more than the crate chose

`Sense`, `KeyFilter`, `PointerWake` and `KeyboardWake` each publish the whole
`bitflags` surface. Three parts of it are not wanted:

- `from_bits_retain(u8)` builds a value with undefined bits. Nothing rejects it.
- `iter()` returns `bitflags::iter::Iter<Sense>`. `bitflags` is not
  re-exported, so a caller cannot name the return type.
- The associated types `Internal`, `Primitive` and `Bits` leak. `Internal`
  names a `bitflags`-private type.

This is the normal cost of `bitflags` in a public API, and it is low severity.

**Suggestion.** Keep `bitflags`, and say in the module doc that only the named
constants and the set operations are supported. Or wrap: keep the generated
type private and hand-write the four public newtypes. The second costs about
40 lines per type and buys an exact surface.

---

## 15. What shipped

| # | change | breaking |
| --- | --- | --- |
| 1 | `ResponseState::clicked()` + `double_clicked()`, primary-button, beside `pressed()` | no |
| 2 | Six dead `!disabled` guards removed; the contract stated on `ResponseState::disabled` | no |
| 3 | `palantir::widget` module — 36 authoring items moved out of the root | yes |
| 4 | Gradient stops take and return `RgbaF32`; `two_stop` on all three geometries; stop colour serializes as sRGB hex | yes |
| 5 | `ColorStrip::for_hue` / `for_alpha`; `MenuSeparator::new` made public | yes |
| 6 | `Popup::new(Anchor)` public; `Popup::anchored_to` and `Popup::gap` removed | yes |
| 7 | `FontStyle` → `FontSlant`; `TextShape::slant`, `TextStyle::with_slant`, fields `slant` | yes |
| 9 | `Scroll::zoom()` / `zoom_config()`; `Tooltip::show` returns `TooltipResponse` | yes |
| 10 | `Rect::scaled_by` crate-private; `Size::scaled_by`; `horiz`/`vert` spelled out; `Image::from_rgba8(UVec2, ..)`; `approx::paints_nothing` / `share_of`; `Ui::state` / `state_mut` / `state_or_default`; `Ui::cursor()` | yes |

Withdrawn: 4.2, 6 (axis), the `hex` half of 7.

`OverlayResponse` keeps no `Response`: carrying one would put a `Ui` borrow on
the type and cost the `Copy` and `Default` that let a trigger widget hold a
closed overlay's result without a branch. The reason is now in its doc.

`palantir::widget` holds: `AnimSlot`, `Animatable` (trait and derive), `approx`,
`ContentType`, `Mesh`, `MeshVertex`, `F32Ext`, `RasterImage`, `Span`, `Sums`,
`curves`, the `Paint*` family, `Lower`, `Shape` and the nine `*Shape` types,
`IconFit`, `LineCap`, `LineJoin`, `PolylineColors`, `GlyphFont`, `TextGlyphs`,
`Caret`, `TextProbe`, `GlyphRasterKey`, `PlacedGlyph`, `TextRun`,
`ConfigureWidget`, `ThemeDefaults`, `Widget`.

`palantir-anim-derive` emits `::palantir::widget::Animatable` to match.

### Still open

- **Bitflags** (section 14) — untouched. `from_bits_retain`, the unnameable
  `bitflags::iter::Iter`, and the `Internal` associated type are still
  published on `Sense`, `KeyFilter`, `PointerWake` and `KeyboardWake`.
- **`InputEvent` / `InputDelta`** — reachable from the root, but the only
  public consumer is `internals::UiHarness`, which is gated. They are dead in a
  default build. Candidates for the `internals` module.
- **`WinitHostBuilder::vsync` vs `present_mode`** (section 11.1) — still two
  setters on one slot with no stated precedence.
- **`Scroll::hide_bars` / `overlay_bars`** (section 11.2) — kept beside
  `bar_mode`.
- **Response wrapper derefs** (section 9) — `ValueResponse`, `SelectResponse`
  and `TextEditResponse` still need `.response.` to reach interaction state.
  The pinning test that denies the deref is a stated decision, left standing.

---

## Sources

- [Rust API Guidelines — Checklist](https://rust-lang.github.io/api-guidelines/checklist.html)
- [Rust API Guidelines — Naming](https://rust-lang.github.io/api-guidelines/naming.html)
- [Rust Design Patterns — Builder](https://rust-unofficial.github.io/patterns/patterns/creational/builder.html)
- [egui documentation](https://docs.rs/egui/latest/egui/)
- [bitflags — Flags trait](https://docs.rs/bitflags/latest/bitflags/trait.Flags.html)
- [bitflags — CHANGELOG, 2.0 internal/public split](https://github.com/bitflags/bitflags/blob/main/CHANGELOG.md)
