# Widget authoring on the public API

Research notes on the rule in `CLAUDE.md`:

> A widget in this crate is written the way a widget outside it would be. It
> reaches nothing an outside crate could not.

The test is reimplementation: could another person write `Scroll`, `TextEdit`
or `Popup` outside this crate, line for line, against the published surface?
Today the answer is no for five of the bundled widgets. This note says exactly
what stops them, what the framework would have to publish, and what it would
cost.

This is a survey plus a set of proposals, not a plan. Nothing here is
committed.

## Scope

A widget's own private modules do not count. An outside author writing a widget
set can have private helpers too, so `ToggleChrome`, `ColorSurface`,
`OverlayScope`, `ThemeSlot` and the rest are in bounds even though they are
`pub(crate)` — an outsider would simply write their own. What counts is a
widget reaching *out of* `src/widgets` into `ui`, `scene`, `layout`, `text`,
`renderer`, `input`, `primitives` or `common`, because nothing an outsider
writes can reach there at all.

Test and bench code is out of scope. It reaches in on purpose, through the
`internals` feature, and that arrangement is documented and deliberate.

## Where it stands

Production widget code reaches outside `src/widgets` at **22 sites across 14
private items**, plus **9 `pub(crate)` methods on the published `Ui`**. Every
one of them belongs to one of the gaps below.

It started at 32 sites across 21 items. Six went to a straight rewrite (see
*Already migrated*), and closing G3 and G2 took the rest of the difference.
What is left is capability gaps: no combination of published calls produces
the same frame.

## Already migrated

| was | is now | why it was possible |
|---|---|---|
| `Ui::chrome_leaf` (slider ×3, progress bar ×2) | `Widget::leaf().id().size().record()` | the method body was already public API, one line of it |
| `Ui::each_keyboard_event` (text edit ×2) | a walk over public `Ui::keyboard_events()` | the method body was a `for i in 0..len` over a public slice |
| `widget.node.padding` (text edit) | `Widget::authored_padding()` | the public accessor returns that exact field |
| `primitives::arrow::Arrow` | `widgets::arrow::Arrow` | only the expander and the combo box theme ever used it |
| `primitives::limits::Limits` | `widgets::drag_num::limits::Limits` | only `DragNum` ever used it |
| `common::platform::{PLATFORM, Platform}` | published at the crate root | see below |

`Platform` is the one public addition made here. Keyboard convention is a
widget's business — which modifier starts word navigation, which chord
submits — so an outside widget branches on the same three cases `TextEdit`
does. The alternative was `cfg!(target_os = "macos")` at each site, which
scatters the spelling and loses the `const`.

Deleting `chrome_leaf` and `each_keyboard_event` also dropped `Ui`'s imports of
`Widget`, `Configure` and `Sizes`, so the `ui` module no longer names anything
from `widgets`.

## The gap list

| # | gap | what is missing | blocks | size |
|---|---|---|---|---|
| G1 | scroll viewport and scrollbar layout mode | `LayoutMode::Scroll`, `ScrollSpec`, `ScrollbarsDef`, `Ui::scroll_content`, `Ui::push_scrollbars_def`, `Ui::current_node`, `Widget::scroll`, `Widget::scrollbars` | Scroll | large |
| ~~G2~~ | ~~anchored, flip-to-fit overlay placement~~ | **closed** — `Anchor` and `LayerScope::anchored` are published | — | — |
| ~~G3~~ | ~~clipboard read and write~~ | **closed** — `Clipboard`, `ClipboardUnavailable` and `Ui::clipboard` are published | — | — |
| G4 | paint-time shape animation | `PaintAnim`, `Ui::add_shape_animated` | Spinner, TextEdit caret | small |
| G5 | GPU view registration | `GpuPaintRef`, `Ui::gpu_view` | GpuView | small |
| G6 | text content identity without shaping | `TextShapeKey::content_hash`, `hash::hash_str` | TextEdit | trivial |
| G7 | zoom factor arithmetic | `input::zoom::{is_valid, combine, from_wheel}` | nothing — correctness sharing only | trivial |

Each gap is worked through below: what the bundled widget does today, why the
published surface cannot express it, the proposals, and a recommendation.

## The gaps, one by one

### G1 — Scroll viewport and scrollbars

**Blocks:** `Scroll`, and `TextEdit` indirectly (it reuses `ScrollState`).

`Scroll` is not a widget that clips and translates its child. It is a widget
that installs a *layout mode* the engine understands:

```rust
inner.node.set_mode(LayoutMode::Scroll(spec.with_fit(fit)));   // scroll/mod.rs:422
let def_id = ui.push_scrollbars_def(ScrollbarsDef { … });      // scroll/bars.rs:276
let content = ui.scroll_content(scroll_id);                    // scroll/mod.rs:340
```

Six private items are involved: `LayoutMode::Scroll`, `ScrollSpec` (a `u16` of
pan and fit bits), `ScrollbarsDef`, `ScrollbarsDefId`, `BarDomain`, and the
`scrollbars::{viewport, bar_geometry}` free functions. Three `Ui` methods carry
them: `push_scrollbars_def`, `current_node`, `scroll_content`. Two `Widget`
constructors mint the nodes: `Widget::scroll`, `Widget::scrollbars`.

Two of those are doing work no widget can do for itself:

- `Ui::scroll_content` answers *how big was the content last frame*, and only
  `Ui` can answer it — the extent is keyed by `(layer, node)` in `Layout`
  while the caller holds a `WidgetId`, and `Cascade` is the map between them.
- The `Scrollbars` layout mode lets the bar rects be assigned **after
  measure**, which is what removed `Scroll`'s old `request_relayout`. A widget
  that placed its own thumbs at record time would be one frame late again.

**Proposals**

1. *Publish the mode.* `Sizing`-style: a public `ScrollAxes` (pan mask + fit
   bits) on `Configure`, plus `Ui::scroll_content(id) -> Size`. The scrollbar
   half stays private and `Scroll` keeps painting its own bars. An outside
   author gets a scrolling viewport but must place bars at record time, one
   frame late.
2. *Publish the post-measure hook.* Generalise `ScrollbarsDef` into a public
   "arrange callback" — a widget registers a node whose children's rects it
   computes from measured geometry. This is the honest fix and by far the
   largest: it puts a user callback inside the arrange pass, which today is a
   closed loop with no re-entrancy.
3. *Publish nothing; make `Scroll` composable.* Give `Scroll` a body slot and
   a bar-theme slot good enough that an outside widget wraps it instead of
   rebuilding it. Cheapest, and it does not satisfy the rule — it only makes
   the rule not matter for this case.

**Recommendation:** (1) now, (2) only if a second widget wants post-measure
placement. (1) is roughly a day and covers the case people actually ask for —
a scrollable custom container. (2) is a frame-model change and belongs beside
the record-time-geometry work in `record-time-geometry.md`.

### G2 — Anchored overlay placement — **closed**

Published: `Anchor`, with `at_point`, `above`, `below`, `left_of`, `right_of`
and a `gap` builder, plus `LayerScope::anchored`. An overlay written outside
the crate now gets the same post-measure resolve the bundled ones do — the
preferred side when the body fits there, the opposite side when it does not,
and a shift back inside the surface when neither has room.

`AnchorSide` and `AxisAlign` stayed private. Nothing needs to name a side once
there are four constructors, and no caller in the crate has ever varied the
cross-axis alignment. Adding `Anchor::align` later is additive, and that is
when the alignment enum earns publishing.

The side names are relational — `Above`, `Below`, `LeftOf`, `RightOf` — because
the body sits *outside* the rect. `Top` and `Bottom` already mean an edge of a
box or a position inside one everywhere else in the crate (`SplitSide`,
`VAlign`), so reusing them here would have said the wrong thing.

Two private items went with it. `LayerScope::placement` is deleted — `anchored`
replaced its only caller. `Placement::fixed` went too, dead once nothing but
`Default` called it. `Placement` no longer reaches the widgets at all:
`OverlayScope` holds an `Option<Anchor>`, where `None` takes the layer's
documented default of the surface origin with the whole surface available.
`Modal` passes `None`, which is what it was spelling out as
`Placement::fixed(Vec2::ZERO, Some(surface.size))` — the same origin and the
same available extent, since the surface rect starts at the origin.

#### The naming pass that followed

The cluster read as word soup — `Placement`, `PlacementOrigin`,
`OverlayPosition` — and carried one outright collision: `LayerScope::at` never
moved the body while `OverlayPosition::at_point` flipped and shifted it.

| was | is |
|---|---|
| `OverlayPosition` (pub) | `Anchor` |
| `OverlaySide` | `AnchorSide`, with `Left`/`Right` → `LeftOf`/`RightOf` |
| `PlacementOrigin` | `Origin` |
| `PlacementOrigin::Anchor(Vec2)` | `Origin::Fixed(Vec2)` |
| `PlacementOrigin::Overlay(..)` | `Origin::Anchored(Anchor)` |
| `LayerScope::at` (pub) | `LayerScope::fixed_at` |
| `Placement::with_anchor` / `with_overlay` / `with_size` | `with_fixed` / `with_anchored` / `with_max_size` |
| `Popup::anchored_at` (pub(crate), took a `Vec2`) | `Popup::anchor` (takes an `Anchor`) |
| module `layout::types::overlay` | `layout::types::anchor` |

It now reads `Placement { origin: Origin, max_size }`, `Origin::{Fixed,
Anchored}`, `Anchor { rect, side, align, gap }`. `fixed_at` and `anchored`
carry the distinction that `at` and `at_point` hid, and every setter name
matches the variant or the field it writes.

**Four types is the floor.** Folding `Origin` into `Placement` as an enum puts
`max_size` in both arms. Folding `Anchor` into `Origin::Anchored { .. }` leaves
the anchored rule with no name a caller can store on a builder, which is what
`Popup` does and what makes `gap` a builder method. Publishing `Placement`
instead of `Anchor` puts `gap` on a type where four of six constructors make it
meaningless, and duplicates `LayerScope`, which is already `Placement`'s
builder. Moving `max_size` out onto `RootSlot` does remove a type, but splits a
pair that always travels together into two fields in three places.

The four other `*Placement` types in the crate — `SlotPlacement` (raster
atlas), `RunPlacement` (text render), `AxisPlacement` (layout axis) and the
public `WindowPlacement` (a window on the desktop) — are in unrelated
subsystems and each carries a prefix that says which. They were left alone.

### G3 — Clipboard — **closed**

`Clipboard`, `ClipboardUnavailable` and `Ui::clipboard` are published. The
handle was published rather than a `clipboard_text` / `set_clipboard_text` pair
on `Ui`, because a widget has to be able to hand the capability to a helper
without handing over the whole `Ui` — `EditAction::execute(editor, clipboard)`
is that shape — and because eleven `Ui`-free tests drive it. `TextEdit` and
`DragValue` are no longer blocked on this.

**One half stays open.** `OffscreenHost` builds an in-process clipboard
unconditionally, so an app that renders offscreen has no OS clipboard even in a
`winit` build. The `winit` feature comment says the arrangement was deliberate:
"the clipboard only ever had a system backend to reach for when a windowed host
was already in the build". An offscreen host with a system clipboard is exactly
the case that reasoning did not cover.

Closing it means splitting `arboard` out of `winit` into its own default-on
`system-clipboard` feature, implied by `winit`, then adding

```rust
#[cfg(feature = "system-clipboard")]
pub fn OffscreenHostBuilder::system_clipboard(self, on: bool) -> Self;
```

which mirrors `collect_gpu_stats(bool)` and `pixel_snap(bool)`. Default `false`,
so no existing offscreen build changes behaviour. A later
`OffscreenHostBuilder::clipboard(Clipboard)` would let several hosts share one,
mirroring the existing `shaper(TextShaper)` knob — worth adding only when
someone asks.

### G4 — Paint-time shape animation

**Blocks:** `Spinner`, `TextEdit` (caret blink).

`Ui::add_shape_animated(shape, PaintAnim)` registers an animation the *encoder*
samples, not the record pass. The recorded subtree is byte-identical every
frame, so it stays cache-stable and the widget never re-records. `PaintAnim`
ships two variants: `BlinkOpacity` and `Spin`.

The public substitute is `Ui::animate` plus `Ui::request_repaint`, which
re-records the widget every frame and invalidates its layout cache entry. It
produces the right pixels and the wrong frame cost.

**Proposals**

1. *Publish `PaintAnim` and `Ui::add_shape_animated`.* The enum is closed on
   purpose — the encoder can only fold an alpha multiplier and a rotation
   today — so publishing it publishes a promise the renderer cannot yet keep
   for a third variant. Mark it `#[non_exhaustive]`.
2. *Publish two shape modifiers instead.* `Shape::blink(half_period, stop)`
   and `Shape::spin(speed)` on the `Lower` builders, with `PaintAnim` staying
   private. This reads better at the call site and hides the enum entirely.

**Recommendation:** (2). `Shape` is already the vocabulary a widget paints in,
and a builder method there costs one line per variant with no new public type.

### G5 — GPU view registration

**Blocks:** `GpuView` only.

`Ui::gpu_view(id, GpuPaintRef, repaint)` mints the epoch and appends the image
shape. `GpuPaintRef` is a private `Rc<RefCell<dyn GpuPaint>>` wrapper.

This gap is the least urgent, because the capability *is* published — as the
`GpuView` widget. What an outsider cannot do is give GPU painting a different
node structure: a view that also paints an overlay inside its own node, say.

**Proposal:** publish `Ui::gpu_view` taking `Rc<RefCell<dyn GpuPaint>>`
directly and drop the wrapper from the signature. Half a day. Do it when
someone asks.

### G6 — Text content identity

**Blocks:** `TextEdit`.

`EditState::text_hash` mints the same `NonZeroU64` the shaping probe reports
through the public `TextProbe::text_hash()`, without shaping:

```rust
TextShapeKey::content_hash(hash::hash_str(text))   // text_edit/edit_state.rs:232
```

It has to agree exactly. A disagreement reads as *the host replaced the
buffer* and wipes the undo stack under the user. Both halves — `hash_str` and
the zero-maps-to-one rule in `content_hash` — are private.

**Proposal:** one associated function,
`TextProbe::hash_of(text: &str) -> NonZeroU64`, documented as the twin of
`TextProbe::text_hash`. An hour. There is no design question here, only a
missing accessor.

### G7 — Zoom factor arithmetic

**Blocks:** nothing. `Scroll` uses it, but an outside author can rewrite it.

`input::zoom` holds `is_valid`, `combine` and `from_wheel`. Zoom is
multiplicative, so a long gesture accumulates a running product, and every
product goes through a clamp that keeps it invertible — including a NaN arm
that resolves to identity. `ScrollDelta::zoom` crosses the public API as a
plain `f32`, so an outside widget receives the value and gets to rediscover
that discipline.

**Proposal:** publish the three functions as a `zoom` module, or as inherent
methods on a published `ZoomFactor` newtype. Half a day. This is correctness
sharing, not a capability gap — file it under nice-to-have.

## Per-widget verdict

| widget | reimplementable outside today | blocked by |
|---|---|---|
| Button, Text, Frame, Panel, Grid, Separator | yes | — |
| Switch, Checkbox, Radio, ColorSwatch | yes | — |
| Slider, ProgressBar, Splitter, Expander | yes | — |
| ColorPicker, ColorField, ColorStrip | yes | — |
| Modal | yes | — |
| Popup, ContextMenu, MenuItem, Tooltip | yes | — |
| ComboBox, ColorButton | yes | — |
| TabStrip, TabbedView, Dock | yes | — |
| Scroll | no | G1, G7 |
| TextEdit | no | G4, G6 |
| DragValue | no | G4, G6 (via TextEdit) |
| Spinner | no | G4 |
| GpuView | no | G5 |

Twenty-seven of the bundled widgets are reimplementable outside the crate
today. The five that are not need `Scroll`'s layout mode, paint-time
animation, the text content hash, or GPU view registration.

## Not gaps

These look like reach-ins and are not.

- **`Widget` and `Configure` over `Node`, `Ident`, `NodeMode`.** `widget/mod.rs`
  and `configure.rs` name private scene types because they *are* the published
  façade over them. `Ui::open_node`, `close_node`, `resolve_ident` and
  `push_grid_def` are that façade's implementation, with no second caller. An
  outside widget uses `Widget` and `Configure` and never meets any of them.
- **Widget-private helpers.** `ThemeSlot`, `LookPlan`, `ToggleChrome`,
  `OverlayScope`, `Checkerboard`, `ColorSurface`, `AxisKeys`, `TabItemBuf`,
  `ScrollState`, the `text_edit` submodules. All inside `src/widgets`. An
  outsider writes their own.
- **Test reach-ins.** 214 sites across 106 items, all behind `internals`.

## Convenience, not capability

An outside author can write these, but would rewrite them from scratch. They
are the argument for a second tier of public API — a widget-author toolkit —
if one is ever wanted.

- **`ThemeSlot` + `LookPlan`.** The whole route from a theme bundle to a
  painted look: pick the per-state look from a response, apply the bundle's
  padding and margin defaults, animate toward it. Fifteen widget files depend
  on it. An outside widget re-derives per-state precedence and transitions by
  hand, and will get the precedence subtly different.
- **`ToggleChrome`.** Box, gap, label row and the toggled-value rule shared by
  switch, checkbox and radio.
- **The keyboard index walk.** Now inlined twice in `text_edit`. Iterating
  `Ui::keyboard_events()` holds a borrow on `Ui` across the whole loop, so a
  handler that needs a text probe cannot iterate. The index form is the
  answer and it is not obvious.
- **`Popup::on(Layer)`.** Deliberately crate-private: layer rank is a fact
  about the kind of overlay, and a caller free to pick is free to invert the
  ranks. An outside menu therefore cannot sit on `Layer::Menu`. This is a
  decision to revisit, not an oversight.
- **Theme geometry helpers.** `ComboBoxTheme::chevron_pts`,
  `ExpanderTheme::arrow_angle`, `ToggleTheme::check_polyline`,
  `TextStyle::metrics_valid` — all `pub(crate)` methods on published theme
  types. An outside widget that re-skins a combo box wants the chevron the
  theme describes.

## Suggested order

1. ~~**G3** clipboard~~ — done.
2. ~~**G2** overlay anchoring~~ — done.
3. **G6** text content hash — an hour, no design question.
4. **G4** paint animation, as `Shape` builder methods.
5. **G1** scroll, proposal (1) only.
6. **G5**, **G7** on demand.
