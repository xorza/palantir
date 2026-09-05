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
one of them belongs to one of the gaps below, and every one is a capability
gap: no combination of published calls produces the same frame.

## The gap list

Ordered by what to do next.

| # | gap | what is missing | blocks | size |
|---|---|---|---|---|
| G1 | text content identity without shaping | `TextShapeKey::content_hash`, `hash::hash_str` | TextEdit | trivial |
| G2 | paint-time shape animation | `PaintAnim`, `Ui::add_shape_animated` | Spinner, TextEdit caret | small |
| G3 | scroll viewport and scrollbar layout mode | `LayoutMode::Scroll`, `ScrollSpec`, `ScrollbarsDef`, `Ui::scroll_content`, `Ui::push_scrollbars_def`, `Ui::current_node`, `Widget::scroll`, `Widget::scrollbars` | Scroll | large |
| G4 | GPU view registration | `GpuPaintRef`, `Ui::gpu_view` | GpuView | small |
| G5 | zoom factor arithmetic | `input::zoom::{is_valid, combine, from_wheel}` | nothing — correctness sharing only | trivial |

Each gap is worked through below: what the bundled widget does today, why the
published surface cannot express it, the proposals, and a recommendation.

## The gaps, one by one

### G1 — Text content identity

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

### G2 — Paint-time shape animation

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

### G3 — Scroll viewport and scrollbars

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

### G4 — GPU view registration

**Blocks:** `GpuView` only.

`Ui::gpu_view(id, GpuPaintRef, repaint)` mints the epoch and appends the image
shape. `GpuPaintRef` is a private `Rc<RefCell<dyn GpuPaint>>` wrapper.

This gap is the least urgent, because the capability *is* published — as the
`GpuView` widget. What an outsider cannot do is give GPU painting a different
node structure: a view that also paints an overlay inside its own node, say.

**Proposal:** publish `Ui::gpu_view` taking `Rc<RefCell<dyn GpuPaint>>`
directly and drop the wrapper from the signature. Half a day. Do it when
someone asks.

### G5 — Zoom factor arithmetic

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
| TextEdit | no | G1, G2 |
| DragValue | no | G1, G2 (via TextEdit) |
| Spinner | no | G2 |
| Scroll | no | G3, G5 |
| GpuView | no | G4 |

Twenty-seven of the bundled widgets are reimplementable outside the crate
today. The five that are not need the text content hash, paint-time animation,
`Scroll`'s layout mode, or GPU view registration.

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
- **Walking the keyboard queue by index.** Iterating `Ui::keyboard_events()`
  holds a borrow on `Ui` across the whole loop, so a handler that needs a text
  probe cannot iterate. The index form is the answer and it is not obvious.
- **`Popup::on(Layer)`.** Deliberately crate-private: layer rank is a fact
  about the kind of overlay, and a caller free to pick is free to invert the
  ranks. An outside menu therefore cannot sit on `Layer::Menu`. This is a
  decision to revisit, not an oversight.
- **Theme geometry helpers.** `ComboBoxTheme::chevron_pts`,
  `ExpanderTheme::arrow_angle`, `ToggleTheme::check_polyline`,
  `TextStyle::metrics_valid` — all `pub(crate)` methods on published theme
  types. An outside widget that re-skins a combo box wants the chevron the
  theme describes.
