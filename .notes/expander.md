# Expander

A header that reveals or hides a body when its arrow is clicked — the
control HTML spells `<details>`, WPF and GTK spell `Expander`, egui spells
`CollapsingHeader`, and Apple spells a disclosure triangle.

Palantir has none. This is a plan, not a survey: it names the API, the
theme surface, the decisions and their costs, and the order to build in.

## 1. What Palantir already has

Every part, and no widget over them.

| Part | Where | What it gives |
|---|---|---|
| `Visibility::Collapsed` | `Configure::collapsed` | A node that records but takes zero size, and is skipped by its stack parent — no gap, no fill weight |
| Cross-frame flags | `Ui::state_mut` / `try_state` | Where the open bit lives. `ComboBox` keeps its own open flag exactly this way |
| `Ui::animate` | `f32` is `Animatable` | The `0..1` tween, and the repaint request while it is unsettled |
| Last frame's geometry | `Ui::response_for(id).layout_rect` | The body height a reveal has to clip against |
| Clipping | `Configure::clip_rect` + `max_size` | The reveal itself |
| The triangle | `ComboBoxTheme::chevron_pts` | A three-point polyline, font-independent, already drawn once |

So the widget is small. What is *not* small is the two questions in §4 and
§5, and both are Palantir's own rather than the field's.

## 2. What the field does

| Library | Name | Open state | Body while closed | Animation |
|---|---|---|---|---|
| egui | `CollapsingHeader`, over `CollapsingState` | Context state, `default_open`, `open(Option<bool>)` forces a frame | Not recorded | `openness` 0..1, body **clipped** to `openness × open_height` remembered from last frame |
| Dear ImGui | `CollapsingHeader` / `TreeNode` | Storage keyed on id; `SetNextItemOpen`; `p_visible` adds a close button that removes the section | Not recorded — `if CollapsingHeader(..) { … }` | None |
| WPF / Avalonia / GTK | `Expander` | `IsExpanded` / `expanded` property | Kept, hidden | Toolkit-dependent |
| SwiftUI | `DisclosureGroup` | Two initialisers: an `isExpanded` binding, or uncontrolled | Retained view tree | Built in |
| Flutter | `ExpansionTile` | `initiallyExpanded` + optional controller | `maintainState` decides | Built in, `expansionAnimationStyle` |
| HTML | `<details>` / `<summary>` | The `open` attribute | In the DOM, hidden | None by default |

Four conclusions.

**Everyone offers both bindings.** An uncontrolled default for the common
case, and an explicit one for the application that owns the state — a
saved layout, an "expand all" button. egui and SwiftUI both spell it as
two entry points rather than one; Flutter as a controller beside a flag.

**The open flag belongs off the widget by default.** ImGui and egui key it
on the id and mint nothing until it is touched. That is exactly the
argument `ComboBox` already makes in this crate: a control that spends
nearly every frame in its default state should keep no row at all.

**egui is the only one that solves the immediate-mode animation**, and it
solves it by clipping to a height it measured on a previous frame. That
transfers whole — Palantir's record pass has the same blindness.

**Nobody ships an accordion as a separate widget** except as a *list* type
(`ExpansionPanelList`, `QToolBox`). The one-open-at-a-time rule is a
policy over N of these, not a thing the control knows.

## 3. The surface

```rust
// The common case: the flag lives on the id, and an untouched
// expander mints no state row at all.
Expander::new("Advanced")
    .default_open(false)
    .show(ui, |ui| {
        advanced_settings(ui);
    });
```

```rust
// The application owns the flag: a saved layout, an "expand all".
Expander::new("Advanced")
    .open(&mut self.advanced_open)
    .show(ui, |ui| advanced_settings(ui));
```

```rust
pub struct ExpanderResponse<'a, R> {
    pub response: Response<'a>,
    /// `None` while collapsed — the body did not run.
    pub inner: Option<R>,
    /// The header was clicked, or Space was pressed on it, this frame.
    pub toggled: bool,
    /// `0.0` closed, `1.0` open, in between while animating.
    pub openness: f32,
}
```

`new` takes the same `impl Into<TextInput>` every other labelled widget
takes, and the label is the id source — `id_salt` overrides it when the
title is dynamic or repeats.

### The header is not always a label

The second entry point, for a header that carries a control of its own —
a checkbox, a count, a delete button:

```rust
Expander::new("Layer 3")
    .show_header(ui, |ui| {
        Checkbox::new(&mut layer.visible).show(ui);
        Text::new(fmt!(ui, "{} nodes", layer.len())).show(ui);
    })
    .show_body(ui, |ui| layer_rows(ui));
```

That is egui's `CollapsingState` split, under two methods rather than a
second type. Deferred to phase 5 — the plain form is what a settings page
wants, and shipping the split before a caller needs it is speculation.

## 4. The body is not recorded while closed, and that costs state

**The decision that has no counterpart in a retained toolkit.** Palantir
sweeps cross-frame state for any widget that stops being recorded —
`FrameCycle::finalize_frame` calls `sweep_removed` once per frame. So a
collapsed section that skips its closure drops every row inside it: a
`TextEdit`'s caret and unsent edit, a `Scroll`'s offset, a nested
expander's own flag.

Two answers, and the widget offers both:

- **Skip the closure** (the default). Nothing is recorded, nothing is
  measured, and the section costs one header. State inside it is gone
  when it reopens.
- **`.keep_body(true)`** records the body under `Visibility::Collapsed`.
  Its ids stay live so the sweep spares them, it takes zero space, it is
  neither painted nor hit-tested — and the application pays a full record
  of every collapsed section on every frame.

Flutter calls the second one `maintainState`. The name here says what it
does in *this* crate, because what it preserves is the state store rather
than a view tree.

**Default skip**, because a section that costs nothing while closed is the
reason the control exists, and because the caller who needs the other
answer knows it — they have a field in there.

## 5. Animation, and the height nobody knows yet

A record pass cannot see this frame's layout, so on the frame a section
opens, the body's height is not knowable. egui's answer is the one to
take: remember the height, clip against it.

```
openness = ui.animate(id, SLOT_OPEN, if open { 1.0 } else { 0.0 }, spec)

full     = ui.response_for(body_id).layout_rect.map(|r| r.size.h)   // last frame
body     .max_size((INF, openness * full))
         .clip_rect()
```

The body records whole; the clip is what reveals it. Scaling it instead
would squash the glyphs, and re-laying it out at a fraction of its width
would reflow the text on every frame of the animation.

**The first open is not animated, and that is deliberate.** There is no
measured height the first time a section opens, and egui's answer — guess
10 px — shows a wrong height for one frame. Palantir snaps `openness` to
`1.0` instead whenever `full` is `None`, so the first open is instant and
every one after it animates. One un-animated reveal per section per
session, against never drawing a height that is a guess.

**Naming the cost, not hiding it.** An animating body changes its own
extent every frame, so its parent column re-measures every frame of the
reveal, and everything below it moves. Inside a `Scroll` that is a
scrolling content height too. `AnimSpec` is `None` by default for exactly
this reason — the reveal snaps, and an application opts into the motion
where it can afford it, the way `ToggleTheme` leaves the checkbox
un-animated and turns the switch on.

## 6. One theme bundle

```rust
pub struct ExpanderTheme {
    /// Four-state look for the header row — the `ButtonTheme` shape.
    pub header: StatefulLook,
    /// Chevron bounding box, drawn as a polyline so it stays
    /// font-independent.
    pub arrow_size: Vec2,
    pub arrow_stroke: f32,
    /// Gutter between the arrow and the label.
    pub gap: f32,
    /// How far the body is inset from the header's leading edge.
    pub indent: f32,
    /// Inset between the body's edges and its content.
    pub body_padding: Spacing,
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}
```

The arrow takes its colour from the picked look's `text`, the way
`ComboBox`'s chevron does, so it is never a field of its own.

**`body_padding`, not `padding`.** `SlotDefaults` is flattened and carries
a `padding` of its own; two fields of that name collide on the wire and
fail RON at load. `TabsTheme` learned this the hard way and renamed its
own to `chip_padding`.

**The chevron shape is lifted, not copied.** `ComboBoxTheme::chevron_pts`
already builds the three points; the two bundles differ in the *size* they
draw it at, not in the shape. Move the geometry to `shape/chevron.rs` as a
`Chevron { size }` with a `points()` method, and have both bundles call
it — one definition, and each keeps its own size field.

## 7. Decisions, and what each one costs

### The arrow rotates right → down

Right when closed, down when open, rotating through `openness`. That is
what egui, GTK, and every file tree do, and it is the shape that says
"nested content sits under this".

The accordion literature prefers down-when-closed rotating to
up-when-open, which is the better read for a *list of sibling sections*.
The two conventions describe different things, and a theme can express
neither with a colour — so the rotation range is a builder axis on the
widget rather than a second bundle field, and the default is the
disclosure triangle.

**Cost:** one `sin_cos` per header per frame while animating, and none
when settled. Lerping between two point triples would avoid the trig and
be wrong in the middle — the chevron shortens as it turns.

### The flag lives on the id, and a binding is the escape hatch

`.default_open(bool)` for the initial state; `.open(&mut bool)` when the
application owns it. An untouched expander mints no state row, which is
the same property `ComboBox` documents for its own open flag.

**Cost:** two ways to say one thing, which is what every toolkit surveyed
concluded is worth it. The binding wins when both are set, and the widget
says so.

### No `Accordion` type

One-open-at-a-time is a policy over N expanders, and a caller expresses it
with one `usize` and N `.open(&mut …)` calls. The showcase demonstrates
it; the crate does not ship it.

**Cost:** the caller writes the four lines. A type would have to own the
option list, the labels, and the bodies — which is `TabbedView` with a
different arrangement, and that already exists.

### The header is a button, not a toggle

ARIA's Disclosure pattern is explicit: `role="button"` plus
`aria-expanded`, and Enter or Space activates. Palantir has no
accessibility layer yet, so what transfers is the *interaction*: the
header senses a click over its whole row — arrow, label and the gap
between them — rather than only the triangle.

**Cost, and an open question.** `KeyClass::of` puts a bare `Enter` and a
bare `Space` in `KeyClass::Text`, so a focused header that wants them must
declare a `TEXT` scope — the same claim a text field makes. Phase 4
settles whether that is acceptable, whether the toggle should be Space
only, or whether `KeyClass` needs a rung for "activation" that is neither
typing nor an accelerator. **Do not guess this in phase 1.**

## 8. The plan

Five phases. Each one compiles, tests, and shows something in the showcase
before the next starts.

### 1. ExpanderTheme and the chevron

The bundle, and the shared chevron geometry lifted out of
`ComboBoxTheme`. No widget yet, so `ComboBox`'s own goldens are the proof
that lifting it changed nothing.

- `shape/chevron.rs`
- `widgets/theme/expander.rs`
- `widgets/theme/combo_box.rs` — calls the lifted geometry

Verify: the chain scoped to `-p palantir`, plus the visual suite.

### 2. Expander, snapped

Header row, rotating arrow, click to toggle, body skipped while closed.
`openness` is `0.0` or `1.0` and nothing tweens, so the reveal is exact
and the phase has no height problem to solve. `.default_open`,
`.open(&mut bool)`, `.keep_body`.

- `widgets/expander/{mod,expander_response,tests}.rs`
- `bin/showcase/pages/controls.rs` — a section on the page that already
  hosts the form controls

Verify: the chain, plus a golden of one open and one closed section.

### 3. The animated reveal

`Ui::animate` over `openness`, the body clipped to
`openness × last frame's height`, and the snap-on-first-open rule. A test
that pins the snap: a section that has never opened reaches full height on
the frame it opens, with no intermediate.

- `widgets/expander/mod.rs`

Verify: the chain, plus an allocation gate — an animating reveal must not
allocate, and a settled one must not repaint.

### 4. Keyboard

Whatever §7's open question resolves to, plus focus: the header is
focusable, and a focused header shows it. Settle the `KeyClass` question
first, in one paragraph, before writing the code.

- `widgets/expander/mod.rs`
- possibly `input/key_class.rs`

Verify: the chain, plus a harness test that a focused header toggles on
the chosen key and that a focused `TextEdit` beside it still types.

### 5. `show_header` / `show_body`

The split form, for a header carrying its own controls. Two methods on the
same builder rather than a second type — the state and the arrow are
already resolved by then.

- `widgets/expander/mod.rs`
- `bin/showcase/pages/controls.rs`

Verify: the chain, plus a golden.

## 9. Risks

- **The state sweep is the sharp edge.** A caller who puts a `TextEdit`
  inside a default-skip expander loses the edit on collapse, and the loss
  looks like a bug in `TextEdit`. §4's `keep_body` must be documented on
  the *widget*, not only in a note, and the showcase should demonstrate
  the failure rather than avoid it.
- **`serde(flatten)` collides on `padding`.** Named in §6; it fails at
  load rather than at compile, and only for a theme that round-trips.
  `widgets/theme/tests/serialization.rs` catches it.
- **Animating inside `Scroll`.** A reveal changes the content height every
  frame, which moves the thumb. Worth one showcase section, because a
  caller will do it and the interaction is not obvious.
- **The first-open snap is visible in a golden.** A visual fixture that
  opens a section has to prime past the snap or it captures the un-animated
  frame. Note it beside the fixture.

## Sources

- [WAI-ARIA APG — Disclosure pattern](https://www.w3.org/WAI/ARIA/apg/patterns/disclosure/)
- [WAI-ARIA APG — Accordion pattern](https://www.w3.org/WAI/ARIA/apg/patterns/accordion/)
- [egui — `CollapsingHeader`](https://docs.rs/egui/latest/egui/containers/collapsing_header/struct.CollapsingHeader.html)
- [egui — `CollapsingState`](https://docs.rs/egui/latest/egui/containers/collapsing_header/struct.CollapsingState.html)
- [egui — `collapsing_header.rs`](https://github.com/emilk/egui/blob/master/crates/egui/src/containers/collapsing_header.rs)
- [GTK 4 — `GtkExpander`](https://docs.gtk.org/gtk4/class.Expander.html)
- [SwiftUI — `DisclosureGroup`](https://developer.apple.com/documentation/swiftui/disclosuregroup)
- [Flutter — `ExpansionTile`](https://api.flutter.dev/flutter/material/ExpansionTile-class.html)
- [Dear ImGui — `imgui.h`](https://github.com/ocornut/imgui/blob/master/imgui.h)
- [GitLab Design System — Accordion](https://design.gitlab.com/components/accordion)
- [UX Movement — Where to place accordion menu icons](https://uxmovement.com/navigation/where-to-place-your-accordion-menu-icons/)
