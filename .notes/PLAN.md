# Plan: golden orphans, focus traversal, IME

Three independent features. Each step below is one commit with its tests, and a step a user can
see ends with a showcase section. Steps marked **API** add public surface; your decisions of
2026-10-04 approve their shape, and each still shows its final signatures before it lands.

## Status (2026-10-04)

All steps are implemented: golden orphans; F1–F10; I1–I5. Two checks remain, both by hand: the
showcase (`cargo run --example showcase`), and IME with a Japanese or Chinese input source on the
macOS laptop. Windows and X11 are not checked for a key typed twice with IME on.

Changed from the design below while implementing:

- **The ring is a chrome stroke, not a shape.** A clipping node (a `TextEdit`) clips its own shapes
  to its padded box, so a shape ring was cut away. The ring rides the node's chrome row (one flag
  in `ChromeRow`, the stroke once on `Tree::focus_ring`), drawn after the chrome and before the
  clip. `FocusRingTheme` is `{ color, width }`: the chrome edge is the owner rect, so there is no
  inset.
- **A dialog takes focus without a call.** `InputState::end_frame` moves focus into a `Modal` root
  on the frame it appears, beside the return on close. `Modal` has no focus code.
- **An overlay with a backdrop claims every class but `FOCUS`**, so Tab walks its own stops.
- **`OverlayResponse::id`** (new field) names the overlay, so `PopupTrigger` moves focus into the
  popup it opened, whatever id the caller gave it. **`Ui::is_focus_visible`** (new) tells a
  keyboard open from a click.
- **A click focuses a `Button`** (decided 2026-10-04): there is no press-focus setting.
- **The preedit cursor is a `Span`** in `InputEvent`, because `Range` is not `Copy`;
  `ImePreedit::cursor` is a `Range<usize>`.

Decided 2026-10-04 and implemented:

- **Key routing — `Widget::key_pressed(&mut self, ui, shortcut)`**, a read as the widget itself.
  The widgets with their own scope read through it: `Button`, `ColorButton`, `ComboBox`,
  `MenuItem`, `DragValue`, the `Splitter` divider, the toggles and `Expander`'s header.
  `Slider`, `ColorField` and `ColorStrip` declare no scope, so their reads were already right.
- **`DragValue` is a spin button.** Focus opens no editor; Up and Down step one unit of the last
  decimal (ten with Shift); Enter, a typed character or a click without a drag opens the editor;
  the chip keeps focus when the edit ends.
- **`Configure::arrow_focus(Axis)`** makes a node an arrow group; a scope strictly inside it may
  claim the arrows, and its own node's claim does not count. `ContextMenu` and `ComboBox`'s list
  set it.

---

## 1. Golden orphans

**Problem.** `Goldens::orphans(names)` exists, but no test calls it, because the suite has no list
of the names it draws. The 46 `assert_matches_golden("…")` calls each spell a string literal, so a
removed or renamed fixture leaves its PNG behind with nothing to say so.

**Design.** The names become a type, not a second list of strings to keep in step:

- `tests/visual/goldens.rs` gets `enum Golden { ButtonHello, … }`, one variant per golden, with
  `const fn name(self) -> &'static str` (an exhaustive `match`) and `const ALL: [Golden; N]`.
- `assert_matches_golden(golden: Golden, actual)` takes the enum. A literal can no longer drift.
- One test, `every_golden_file_belongs_to_a_fixture`, asserts `goldens().orphans(ALL.map(name))`
  is empty. Under `UPDATE_GOLDEN=1` it deletes the orphans instead, as an update run rewrites
  stale goldens.

**Why this closes both directions.** A fixture removed leaves its variant unused, and
`-D warnings` fails on the dead variant. A variant left out of `ALL` makes its PNG an orphan, and
the new test fails. A new fixture must add a variant to compile.

**Steps.** One commit: the enum, the 46 call sites, the test. No library change.

---

## 2. Focus traversal

### What exists

- Focus is `InputState::focused`. A left press focuses the topmost focusable row under the pointer
  (`Cascade::hit_test_press`), at input time, from the last frame's cascade.
- `NodeFlags` has `FOCUSABLE` (bit 9). Bits 18–31 are free.
- Keys route by input scope: `Scopes::grant(class)` gives a key to the innermost scope on the
  focused widget's path whose `KeyFilter` takes its class, else to the active layer's outermost
  scope. `KeyClass::Focus` is Tab and Shift+Tab; no code acts on it.
- Eight widgets are focusable: `TextEdit`, the toggles, `Slider`, `Expander`, `TabStrip`,
  `ColorField`, `ColorStrip`, the dock panes. None shows focus: `Palette::border_focused` is used
  only for the pressed look.

### Design

**Tab stops (data).**

- `NodeFlags` gets `NOT_TAB_STOP` (bit 18), so the default, zero, is a stop. **API:**
  `Configure::tab_stop(bool)` and its `ConfigureWidget` twin, and `Widget::authored_tab_stop()`.
- `tab_index: i16` is sparse, as `bounds_table` is: a tree column only for nodes that set a
  non-zero value. **API:** `Configure::tab_index(i16)`, its twin, `Widget::authored_tab_index()`.
- The cascade walk, which already emits `HitRow` and `ScopeRow` in pre-order, emits a
  `TabStopRow { layer, id, index }` for each node that is focusable, a stop, and neither disabled
  nor invisible: the rule `HitRow::focusable` already applies. Same lifecycle as `scopes`.

**Traversal (behaviour).**

- **Where.** In `InputState::pre_record`, after `Scopes::resolve`: for each `KeyClass::Focus` press
  in the frame, if **no scope on the focused widget's path takes `KeyFilter::FOCUS`**, the press is
  the framework's. It moves focus and is removed from the frame's key stream. A scope that takes
  `FOCUS` (a code editor that indents on Tab, or an app root that wants Tab itself) opts out for
  its subtree. Then scopes resolve again for the new focus. The trickle queue already admits one
  command key per frame, so a burst of Tabs walks one stop per frame.
- **Order.** The stops in the domain (below), sorted by `tab_index` ascending; a stable sort keeps
  record order for ties. Tab goes to the next, Shift+Tab to the previous, and both wrap. With
  nothing focused, Tab goes to the first stop and Shift+Tab to the last.
- **Domain.** **This refines decision 2 (please confirm).** "The topmost overlay traps" fails one
  common case: an autocomplete popup open under a focused `TextEdit` in `Main` would take the next
  Tab into the popup, where ARIA's combobox moves on to the next field. The rule becomes:
  1. An open `Modal` always traps: the domain is the topmost `Modal`-layer root. A focus behind it
     is pulled in on the next Tab.
  2. Otherwise the domain is the layer root that holds the focus: a `Popup` or `Menu` root traps
     while focus is inside it, and `Main` is the domain otherwise.
  3. `Tooltip` and `Debug` never hold stops.
- **Into and out of overlays.** So that rule 2 traps a keyboard user at all, an overlay opened by
  the keyboard moves focus to its first stop, and every overlay gives focus back on close to the
  widget that held it when it opened (ARIA dialog and menu-button practice). A `Modal` moves focus
  in on open whatever opened it, as `<dialog>.showModal()` does. **API:**
  `Ui::focus_first_within(ancestor: WidgetId)`, the twin of `is_focus_within`, deferred to the next
  pre-record because the overlay is not in the cascade on the frame it opens. Focus restore is
  state on the overlay's own row, through the public `state_or_default`.

**Indicator.**

- `InputState` gets `focus_visible: bool`: set by a Tab move, cleared by a pointer press that moves
  focus, unchanged by `set_focus` (a programmatic focus follows the last input's modality, as
  `:focus-visible` does).
- `Widget::record` draws the ring after the body when its id is focused and `focus_visible` is
  set, so it paints above the children and every widget gets it, inside and outside the crate. The
  ring is inside the node's rect (an outset ring is cut by an ancestor's clip), and follows the
  chrome's `corners`.
- **API:** `Theme::focus_ring: FocusRingTheme { color, width, inset }`, plain data, from
  `Palette::border_focused`. A theme file checks it on load, as every other slot.

**Tab stops (widgets).** Each takes the keys the ARIA practices give its role, and reports through
the response it already returns, so a caller adds nothing:

| Widget | Keys | Reported as |
|---|---|---|
| `Button` (so `PopupTrigger`, `ColorButton`) | Space, Enter | `clicked()`: `show` sets `left.phase` to a click, through public `ResponseState` fields |
| `ComboBox` | Space, Enter, Alt+Down open; arrows step | `ValueResponse`, `committed == changed` |
| `DragValue` | focus enters the edit field, as a spin box; arrows step | `ValueResponse` |
| `Splitter` | arrows move by a step, Home/End to the ends | `ValueResponse` |
| `MenuItem` | Enter; arrows move between items | `clicked()` |

**Behaviour that changes.** `text_edit::tests::response::a_focused_field_yields_the_keys_it_does_not_act_on`
expects Tab to reach an app root that declares `ACCEL`. Under this design that Tab moves focus,
and a root must take `FOCUS` to read Tab. The test changes with step F2.

### Steps

| Step | Content | API |
|---|---|---|
| F1 | `NOT_TAB_STOP`, the sparse `tab_index`, `TabStopRow` in the cascade; tests on row order, disabled and invisible rows, the sort | **API** setters |
| F2 | Traversal in `pre_record`, the domain rule, the `FOCUS` opt-out, the changed test; a table test over order, `tab_index`, ties, Shift+Tab, wrap, modal trap, popup trap, pull-in from behind a modal | — |
| F3 | `focus_visible`, the ring in `Widget::record`, `FocusRingTheme`; a golden, a damage test that the ring repaints only its node | **API** theme |
| F4 | `focus_first_within`, overlay focus-in on keyboard open and restore on close, for `Modal`, `Popup`, `PopupTrigger`, `ContextMenu` | **API** `Ui` |
| F5 | `Button` keys (with `PopupTrigger`, `ColorButton`) | — |
| F6 | `ComboBox` keys | — |
| F7 | `DragValue` as a spin box | — |
| F8 | `Splitter` keys | — |
| F9 | `MenuItem` keys and arrow movement in a menu | — |
| F10 | Docs: README's "Tab-key focus traversal" line, `Key::Tab`'s "binds no focus traversal", showcase page | — |

F1 → F2 → F3 in order; F4 after F2; F5–F9 after F2 in any order.

---

## 3. IME

### What exists

- winit 0.30 sends IME text as `WindowEvent::Ime(Preedit | Commit | Enabled | Disabled)` only after
  `Window::set_ime_allowed(true)`, and places its candidate list from `set_ime_cursor_area`. The
  host handles neither.
- `InputEvent` is `Copy`. The trickle queue (`InputQueue::pending`) holds events across frames.
- `KeyPress::text` is a `KeyText`: 14 bytes inline, control characters dropped.
- Per-window settings a frame asks for (`cursor`, `vsync`) travel as `WindowOutput` levels, which
  the winit window applies in `finish`.

### Design

**Events. API.** `InputEvent<'a>` gains:

- `ImePreedit { text: &'a str, cursor: Option<Range<usize>> }`: the uncommitted text, and the
  byte range winit reports for its cursor. An empty `text` ends the composition.
- `ImeCommit(&'a str)`.

It stays `Copy`. `OffscreenHost::on_input` takes `InputEvent<'_>`. The trickle queue cannot hold a
borrow, so a held event keeps its text in a `String` the queue retains and clears when it drains:
a span, not an allocation per event.

**Commits join the key stream.** `InputState` turns a commit into text-only presses, in place:
`KeyPress { key: Key::Other, mods: Modifiers::NONE, repeat: false, physical: Key::Other, text }`,
one per `KeyText`-sized piece, split at character boundaries. `mods` is `NONE` because a commit is
text whatever keys are held, and `KeyPress::types_text` reads the modifiers. Control characters
drop, as `KeyText::new` already does. A commit never waits in the trickle queue: like a typed
key, it is not a command key.

**Preedit is a level. API.** `Ui::ime_preedit() -> Option<ImePreedit<'_>>`, with
`ImePreedit<'a> { text: &'a str, cursor: Option<Range<usize>> }`. Held in `InputState`, replaced
by each preedit event, cleared by an empty one, by a commit, by `Ime::Disabled`, and by a focus
move.

**Enable and caret. API.** `Ui::request_ime(caret: Rect)`, with `caret` in the frame's screen
space, as `ResponseState::rect` is. It is a per-frame level on `WindowOutput` (`ime: Option<Rect>`,
reset at each frame start, so a frame with no call turns IME off). The winit window applies a
change in `finish`: `set_ime_allowed`, then `set_ime_cursor_area` in physical pixels.
`FrameReport` gains `ime_area: Option<Rect>` (**API**) for a host that embeds `OffscreenHost`.

**`TextEdit`.**

- While focused, it calls `ui.request_ime` with its caret rect, mapped to screen space by its
  response's `rect` and `transform`.
- While a preedit is live, it shapes a display string — the buffer with the preedit spliced in at
  the caret, in a retained scratch `String` — draws the preedit underlined with
  `TextProbe::selection_rects` over its range, and puts the caret at the preedit's cursor. The
  bound `String` changes only on commit.
- A composition that starts over a selection deletes the selection first, as browsers do.

### Steps

| Step | Content | API |
|---|---|---|
| I1 | `InputEvent<'a>`, the two variants, the queue's text buffer, commits as text presses, the preedit level; tests: `a`, Backspace, `b` in one frame gives `b`; a 40-byte commit of 3-byte characters splits on boundaries; control characters drop; held modifiers do not make a commit a chord | **API** event, `ime_preedit` |
| I2 | winit translation of `Ime` events; tests in `host::winit::input::tests` | — |
| I3 | `request_ime`, the `WindowOutput` level, the winit apply, `FrameReport::ime_area`; tests: the level resets each frame, the rect converts to physical pixels | **API** |
| I4 | `TextEdit`: request, preedit display, underline, selection delete; tests and a golden | — |
| I5 | Docs: `KeyText`'s "carries no IME commit", the `TextEdit` docs, the `ISSUES.md` entry deleted; a check on the macOS laptop with a Japanese or Chinese input source | — |

I1 → I2 → I3 → I4 → I5.

### Risk to check first

Platforms differ in whether a plain key press with IME on arrives as `KeyboardInput` with text, as
`Ime::Commit`, or as both. If both, a character types twice. I2 starts with a probe on macOS (the
laptop, through tmux) that logs both events for plain typing with IME on, before the translation
is written.

---

## Order across the three

Independent. Suggested: orphans (one small commit), then F1–F4, then I1–I5, then F5–F10. F4 and
I4 both touch `TextEdit`'s focus path, so they are best not in flight together.
