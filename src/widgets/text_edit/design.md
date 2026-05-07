# TextEdit — design

Single-line, focusable, click/drag-to-place-caret editable text leaf.
Typing via `KeyDown` printable chars or IME `Text` commits;
backspace/delete, left/right arrows, home/end, escape-to-blur. Selection
(visible + edits), shift+arrow / drag-to-select, multi-line, IME
preedit, undo, copy/paste are deferred — see "Out of scope" at the end.

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
    pub(crate) caret: usize,             // byte offset into the buffer
    pub(crate) selection: Option<usize>, // anchor byte; unused in v1
}
```

Byte offsets, not chars: cosmic-text's hit-testing returns byte cursors
and `&buffer[..caret]` is the natural prefix-measure path. `selection`
is reserved so the v1.1 selection branch doesn't need a state migration.

Eviction rides on the same `removed` sweep that drives `MeasureCache` /
`TextMeasurer`: a `WidgetId` that vanishes from this frame's tree gets
its row dropped in `end_frame`.

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
  `end_frame` clears it.
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

| Key                 | Effect                                          |
| ------------------- | ----------------------------------------------- |
| `Char(c)` w/o cmd   | insert UTF-8 of `c` at caret, advance           |
| `Backspace`         | remove the codepoint before caret               |
| `Delete`            | remove the codepoint after caret                |
| `ArrowLeft/Right`   | walk caret one codepoint                        |
| `Home`/`End`        | caret to 0 / `text.len()`                       |
| `Escape`            | request blur (clear focus)                      |
| anything else       | ignored                                         |

`Modifiers::any_command()` (any of ctrl/alt/meta) suppresses the
`Char` insert path so future shortcut routing (ctrl+a / cmd+c) doesn't
double-fire as text. `Char` also handles space — there is no `Space`
variant; winit's `NamedKey::Space` is translated to `Char(' ')`.

Codepoint-granular boundary walks (`prev_char_boundary` /
`next_char_boundary`) on the `&str`, not graphemes — multi-codepoint
graphemes (emoji + ZWJ, accent combiners) split apart on backspace.
Acceptable v1; grapheme awareness lands with `unicode-segmentation` on
the v1.1 selection branch where it already pays its way.

### Click-to-place-caret

`caret_from_x` linearly scans char boundaries, calling `caret_x` at
each one and picking the closest to `target_x`. O(n) measure calls
per pressed-frame — acceptable for short single-line strings, swappable
for a `byte_to_x` API on `MeasureResult` once cosmic-text's
`Buffer::layout_runs` is wired through `TextMeasurer`.

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

## Caret rendering

Caret is a thin `Shape::RoundedRect { local_rect: Some(..), .. }` at owner-local coords, painted
last so it sits *over* the text inside the widget's clip. Position
comes from `TextMeasurer::caret_x` (re-measures the prefix; cache miss
amortizes once cosmic exposes per-glyph x). Blink is stubbed to
"always on" — a `request_repaint`-on-timer pass is future work.

## Tests

- `widgets::text_edit::tests` — state mutations: insert / backspace at
  end / mid / start, delete, left/right past boundaries, home/end on
  empty buffer, escape-to-blur, click-to-place, drag-to-place, typing
  while unfocused is a no-op, focus eviction on tree removal,
  `FocusPolicy` variants, IME `Text` commit path.
- Showcase tab — `examples/showcase/text_edit.rs` echoes the live
  buffer and exercises focus, placeholder, and disabled visuals.

## Out of scope (future slices)

- **Selection (visible + edits)** — render the `Overlay` highlight
  behind the text shape; shift+arrow extension; drag-select via the
  existing `pressed` + pointer-pos plumbing. `selection: Option<usize>`
  is already on the state row; `theme.selection` already exists.
- **Multi-line** — re-uses caret-x via a future
  `MeasureResult::byte_to_x` plus per-line vertical offset; Enter
  inserts `\n`; PageUp/PageDown become live.
- **Grapheme-aware backspace/delete** — `unicode-segmentation` on the
  selection branch.
- **Copy/paste** — clipboard crate + ctrl+c/x/v shortcut routing
  (needs the `any_command()` branch to fan out instead of suppress).
- **Undo/redo** — ring buffer per `TextEditState`.
- **IME composition preview** — winit `Preedit` events + an extra
  underlined `Text` shape under the caret. cosmic-text supports it.
- **`Ui::wants_ime()`** — gate `window.set_ime_allowed(true)` on
  whether focus is on a text-input widget; today it's
  unconditionally enabled.
- **Tab focus cycling** — needs a focus-order traversal over the
  cascade; eats Tab keypresses from focused editors when not
  multi-line.
