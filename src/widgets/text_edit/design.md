# TextEdit — design

Focusable, click/drag-to-place-caret editable text leaf. Single-line
by default; flip to multi-line with `.multiline(true)`. Typing via
`KeyDown` printable chars or IME `Text` commits; backspace/delete,
left/right arrows (up/down too in multi-line), home/end, escape-to-blur.
Selection: shift+arrow / shift+home/end extend, plain arrows collapse,
ctrl/cmd+A select-all, click+drag selects, edits replace the range,
two-stage Escape (first collapse, second blur), painted highlight wash
behind the glyphs. Cut/copy/paste via Cmd/Ctrl+X/C/V and a default
right-click menu; paste sanitizes `\n`/`\r` to spaces in single-line
mode. Undo/redo via Cmd/Ctrl+Z and Cmd/Ctrl+Shift+Z with snapshot
coalescing — consecutive typing or consecutive deletes group into one
undo unit; any caret-only motion ends the group. IME preedit and
caret blink are still deferred — see "Out of scope" at the end.

Code lives in `src/widgets/text_edit/{mod.rs,tests.rs}`.

## Tree shape

One leaf node, no children.

```
TextEdit            (Leaf, sense = Click, focusable = true)
  shapes:
    [Background]    (state.background; theme-driven, omitted when None)
    Text            (the buffer or the placeholder — TextWrap::Single)
    Overlay         (caret, painted last, only when focused)
```

Empty buffer + unfocused renders the placeholder; focused (even with an
empty buffer) renders the buffer so the caret has flush-left position
to anchor against. `Background::default()` (transparent / no stroke)
collapses to a no-op via `Ui::add_shape`'s `is_noop` filter, so themes
that omit a background don't pay an extra shape.

## State

Per-widget cross-frame row in `Ui::state` keyed by `WidgetId`:

```rust
pub(crate) struct TextEditState {
    pub(crate) caret: usize,                       // byte offset (active end)
    pub(crate) selection: Option<usize>,           // anchor byte; None == no sel
    pub(crate) drag_anchor: Option<usize>,         // latched on press rising edge
    pub(crate) prev_pressed: bool,                 // edge detection for `pressed`
    pub(crate) undo: VecDeque<EditSnapshot>,       // capped at UNDO_LIMIT (128)
    pub(crate) redo: Vec<EditSnapshot>,            // cleared on every fresh edit
    pub(crate) last_edit_kind: Option<EditKind>,   // Typing / Delete / Other; None ⇒ new group
}
```

`EditSnapshot` is `{ text, caret, selection }` — the *pre-edit* buffer
+ caret state captured by `record_edit` before each mutation. Snapshot
coalescing rule: consecutive same-kind edits (`Typing` runs,
`Delete` runs) skip the push because the group's existing top already
captures the pre-state. `Other` (paste, cut, clear, multi-line
`Enter`) never coalesces. Any caret-only motion calls `move_caret`,
which clears `last_edit_kind` and opens a fresh group on the next
edit. Undo limit is a constant 128; older entries fall off the front
of the deque.

Byte offsets, not chars: cosmic-text's hit-testing returns byte cursors
and `&buffer[..caret]` is the natural prefix-measure path.
`selection`/`caret` form a two-cursor anchor/active model. Invariant:
empty selection always collapses to `None` (every mutation site clears
`Some(caret)` immediately) so "is there a selection" is one
`is_some()` check, not `anchor != caret`. Sorted range comes from
`TextEditState::sel_range()` (no tuple return — style rule).

Eviction rides on the same `removed` sweep that drives `MeasureCache` /
`TextMeasurer`: a `WidgetId` that vanishes from this frame's tree gets
its row dropped in `post_record`.

The buffer itself isn't in state — `TextEdit<'a>` borrows
`&'a mut String` from the host (egui-style). Host-side mutations
between frames are visible immediately; the widget clamps
`caret <= text.len()` at the top of every `show()`.

## Input plumbing (shipped)

### `InputEvent`

```rust
pub enum InputEvent {
    PointerMoved(Vec2),
    PointerLeft,
    PointerPressed(PointerButton),
    PointerReleased(PointerButton),
    Scroll(Vec2),
    KeyDown { key: Key, mods: Modifiers, repeat: bool },
    KeyUp   { key: Key, mods: Modifiers },
    Text(TextChunk),                       // committed character(s)
    ModifiersChanged(Modifiers),
}
```

`Key` is a small palantir-native enum — arrows, Backspace/Delete,
Home/End, PageUp/PageDown, Enter, Tab, Escape, `Char(char)` for
printables (post-layout; shift+'a' arrives as `Char('A')`), and a
catch-all `Other`. `Modifiers` carries `shift/ctrl/alt/meta` (meta =
Cmd on macOS / Super on Linux / Win on Windows). The `Text` event
carries a `TextChunk` (inline UTF-8 buffer, ≤ 15 bytes — sized for one
grapheme cluster) so `InputEvent` stays `Copy`; longer IME commits
split across multiple events at the translation boundary.

`InputEvent::from_winit` translates from `winit::event::WindowEvent`
(KeyboardInput / Ime(Commit) / ModifiersChanged / pointer / scroll).

### `InputState` queues

```rust
focused: Option<WidgetId>,
focus_policy: FocusPolicy,
modifiers: Modifiers,
frame_keys: Vec<KeyPress>,    // drained by the focused widget at show()
frame_text: String,           // committed text this frame, ditto
```

- `PointerPressed(Left)` runs *two* hit-tests on the cascade: the usual
  `Sense::click()` test (drives `active`/`clicked`) and a separate
  `hit_test_focusable` test for the focusable bit. A focusable hit sets
  `focused`; a miss defers to `FocusPolicy` (`ClearOnMiss` default,
  `PreserveOnMiss` opt-in).
- `KeyDown` becomes a `KeyPress { key, mods, repeat }` and pushes onto
  `frame_keys`. `KeyUp` is dropped — editors care about presses, no
  consumer needs releases yet. `Text` events append onto `frame_text`.
  Modifiers are snapshotted at *push* time, not drain time, so
  interleaved chord input doesn't mis-attribute.
- Eviction: if `focused`'s `WidgetId` isn't in the new cascade,
  `post_record` clears it.
- `Ui::focused_id()` / `Ui::request_focus(Option<WidgetId>)` /
  `Ui::set_focus_policy` / `Ui::focus_policy` are public.

### Focusable flag

`Element::focusable: bool` (default `false`); `TextEdit::new` flips it
to `true`. The cascade carries it into `CascadeResult` so
`hit_test_focusable` is a flat scan over the same arena `hit_test`
walks. Buttons stay focus-inert: clicking one doesn't steal the caret
from a TextEdit. No `Sense::Focus` variant — focus is orthogonal to
pointer participation (programmatic focus, future tab-cycling) and
bolting it onto `Sense` would conflate two concepts that diverge.

### winit wiring

The example apps translate `WindowEvent::KeyboardInput`,
`WindowEvent::Ime(Commit)`, and `WindowEvent::ModifiersChanged` via
`InputEvent::from_winit`. IME is enabled unconditionally for now; a
`Ui::wants_ime()` query gating `window.set_ime_allowed(true)` is open
work.

## Widget surface

```rust
pub struct TextEdit<'a> {
    element: Element,
    text: &'a mut String,
    style: Option<TextEditTheme>,
    placeholder: Cow<'static, str>,
}

impl<'a> TextEdit<'a> {
    #[track_caller]
    pub fn new(text: &'a mut String) -> Self;
    pub fn placeholder(mut self, s: impl Into<Cow<'static, str>>) -> Self;
    pub fn style(mut self, s: TextEditTheme) -> Self;
    pub fn show(self, ui: &mut Ui) -> Response;
}

impl Configure for TextEdit<'_> { ... }   // .padding / .margin / .with_id / .disabled
```

`show()` runs in three phases (the ordering is load-bearing for
borrow choreography):

1. **Input.** `handle_input` (free fn in the same module) holds the
   `TextEditState` row, clamps `caret`, applies click-to-place-caret
   from this frame's pointer pos when `response_state.pressed` (drag-
   to-place falls out for free since the press tracks pointer x while
   held), then — when focused — drains `frame_text` first (insert at
   caret) and `frame_keys` second (navigation/edits). Returns the caret
   byte to render.
2. **Record.** `ui.node(...)` opens the leaf and pushes background →
   text/placeholder → caret overlay. Caret position uses
   `TextMeasurer::caret_x(text, caret_byte, font_size, line_height_px)`,
   which re-measures the prefix; height = `font_size × line_height_mult`
   so the rect spans the same y-range the shaped glyphs occupy.
3. **Side effects.** If a key handler set `blur_after = true` (Escape),
   `ui.request_focus(None)` runs after the node closes so we don't
   mutate during recording.

### Edit set (`apply_key`)

| Key                       | Effect (no selection)               | Effect (selection live)                  |
| ------------------------- | ----------------------------------- | ---------------------------------------- |
| `Char(c)` w/o cmd         | insert UTF-8 at caret               | replace range with `c`, collapse         |
| `Backspace`               | remove prev codepoint               | delete range, collapse                   |
| `Delete`                  | remove next codepoint               | delete range, collapse                   |
| `ArrowLeft` / `ArrowRight`| walk caret one codepoint            | collapse to range start / end            |
| `Shift+ArrowLeft/Right`   | extend by one codepoint             | extend by one codepoint                  |
| `Home` / `End`            | caret to 0 / `text.len()`           | collapse + jump                          |
| `Shift+Home` / `Shift+End`| extend to 0 / `text.len()`          | extend to 0 / `text.len()`               |
| `Ctrl+A` / `Cmd+A`        | select all (anchor=0, caret=len)    | select all                               |
| `Ctrl/Cmd+C`              | copy (no-op)                        | clipboard ← range                        |
| `Ctrl/Cmd+X`              | cut (no-op)                         | clipboard ← range, delete range          |
| `Ctrl/Cmd+V`              | insert clipboard at caret           | replace range with clipboard             |
| `Ctrl/Cmd+Z`              | pop undo group                      | pop undo group                           |
| `Ctrl/Cmd+Shift+Z`        | pop redo group                      | pop redo group                           |
| `Enter` (multi-line only) | insert `\n`                         | replace range with `\n`                  |
| `Up` / `Down` (multi-line)| caret one visual line up / down     | extend with shift                        |
| `Escape`                  | request blur                        | collapse selection, *don't* blur         |
| anything else             | ignored                             | ignored                                  |

`Modifiers::any_command()` (any of ctrl/alt/meta) suppresses the
`Char` insert path so shortcut routing doesn't double-fire as text;
`Ctrl/Cmd+A` is matched ahead of that suppression so select-all still
fires. Selection mutations go through one `move_caret(state, n, extend)`
helper that latches the anchor on the first extending move and
collapses on plain navigation — the "never store empty selection"
invariant lives there. `Char` also handles space — there is no `Space`
variant; winit's `NamedKey::Space` is translated to `Char(' ')`.

Codepoint-granular boundary walks (`prev_char_boundary` /
`next_char_boundary`) on the `&str`, not graphemes — multi-codepoint
graphemes (emoji + ZWJ, accent combiners) split apart on backspace.
Acceptable v1; grapheme awareness lands with `unicode-segmentation` on
the v1.1 selection branch where it already pays its way.

### Click-to-place-caret + drag-select

`TextShaper::byte_at_xy` is one shaped lookup via `Buffer::hit` — O(1)
in caret count, multi-line aware. Mono / empty-text falls back to a 1D
`(x ÷ 0.5·font_size)` scan over char boundaries.

Drag-select rides on the same pressed-frame loop: on the *rising edge*
of `ResponseState::pressed` (detected via `state.prev_pressed`) the
hit caret is latched into `state.drag_anchor` and any prior selection
clears. On subsequent pressed frames `state.caret` follows the pointer
and `state.selection` flips to `Some(drag_anchor)` once the active end
diverges from the anchor (collapses back to `None` if they coincide,
matching the invariant). The release edge clears `drag_anchor`.

## Theme

```rust
pub struct TextEditStateStyle {
    pub background: Option<Background>,
    pub text: Option<TextStyle>,
}

pub struct TextEditTheme {
    pub normal:      TextEditStateStyle,
    pub focused:     TextEditStateStyle,
    pub disabled:    TextEditStateStyle,
    pub placeholder: Color,
    pub caret:       Color,
    pub caret_width: f32,            // 1.5 px default — hairline reads at 1, i-beam at 2
    pub selection:   Color,          // unused in v1, slot reserved
    pub padding:     Spacing,
    pub margin:      Spacing,
}
```

Slotted as `Theme::text_edit`. State picked by
`if disabled → disabled else if focused → focused else normal` —
disabled wins over focus, mirroring Button's per-state precedence.

State-style fields are `Option`s with theme-level fallback:

- `background = None` ⇒ `Background::default()` (transparent / no
  stroke; the no-op shape filter drops it).
- `text = None` ⇒ inherit `Theme::text` (so apps tweaking
  `theme.text.color` move every editor's buffer text alongside every
  button label).

`padding`/`margin` apply when the builder hasn't called
`.padding(...)` / `.margin(...)` — the sentinel is `Spacing::ZERO`
("user didn't override"). To get a TextEdit with truly zero padding
under a padded theme, build a custom theme rather than passing zero.

## Caret + selection rendering

Caret is a thin `Shape::RoundedRect { local_rect: Some(..), .. }` at owner-local coords, painted
last so it sits *over* the text inside the widget's clip. Position
comes from `TextMeasurer::caret_x` (re-measures the prefix; cache miss
amortizes once cosmic exposes per-glyph x). Blink is stubbed to
"always on" — a `request_repaint`-on-timer pass is future work.

Selection highlight is a `Shape::RoundedRect` pushed *before* the
text shape (record order is paint order within a node) so glyphs sit
on top of the wash. Width = `caret_x(end) - caret_x(start)`, height =
the caret rect's height. Painted only when focused and `sel_range()`
is `Some` — collapsed selections never store as `Some` so a `None`
check suffices. Fill is `theme.selection` (linear-RGB premultiplied,
already authored at ~25% alpha so it doesn't obscure the glyphs).

## Tests

- `widgets::text_edit::tests` — state mutations: insert / backspace at
  end / mid / start, delete, left/right past boundaries, home/end on
  empty buffer, escape-to-blur, click-to-place, drag-to-place, typing
  while unfocused is a no-op, focus eviction on tree removal,
  `FocusPolicy` variants, IME `Text` commit path. Selection axes:
  table-driven `selection_state_transitions` (shift+nav, plain-nav
  collapse, edit-replaces-range, ctrl+A, two-stage Escape);
  `drag_select_extends_selection` (press+drag → range, then typed key
  replaces); `shift_end_paints_selection_highlight` (wash precedes
  caret in the shape buffer); `no_selection_paints_no_highlight_rect`;
  `click_without_drag_clears_prior_selection`.
- Showcase tab — `examples/showcase/text_edit.rs` echoes the live
  buffer and exercises focus, placeholder, and disabled visuals.

## Out of scope (future slices)

- **Grapheme-aware backspace/delete** — `unicode-segmentation` on the
  selection branch.
- **IME composition preview** — winit `Preedit` events + an extra
  underlined `Text` shape under the caret. cosmic-text supports it.
- **`Ui::wants_ime()`** — gate `window.set_ime_allowed(true)` on
  whether focus is on a text-input widget; today it's
  unconditionally enabled.
- **Tab focus cycling** — needs a focus-order traversal over the
  cascade; eats Tab keypresses from focused editors when not
  multi-line.
- **Caret blink** — needs `request_repaint`-on-timer; today the
  caret is always-on.
- **Op-log undo** — current undo stores full-buffer snapshots, which
  is fine for small fields but linear in buffer size per edit-group
  boundary. An Insert/Delete op-log would amortize better if buffer
  sizes ever grow.
