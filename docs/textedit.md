# TextEdit — design

v1 of editable-text widget. Single-line, focusable, click-to-place-caret,
arrow/home/end/backspace/delete/insert via keyboard. Mouse-drag selection,
multi-line, IME, undo, copy/paste deferred. Goal: smallest slice that's
*actually editable* and pins the focus + keyboard plumbing every later
text/input widget will reuse.

## What's missing today

The codebase has no keyboard or focus story:

- `InputEvent` only carries pointer/scroll. No `KeyDown`, no `Text` (char
  commit), no modifiers.
- `InputState` tracks `hovered` / `active` / `scroll_target` but no
  `focused` widget.
- `Sense` has no focus participation — nothing distinguishes "click sets
  focus" from "click is a button press".
- `Shape` has no caret primitive (an `Overlay` rect can stand in fine).
- `Theme` has no text-edit slot.

So TextEdit lands in two halves: **input plumbing** (focus + keyboard)
then **the widget**. Both are small.

## Tree shape

One leaf node, no children.

```
TextEdit            (Leaf, sense = Click, sets focus on press)
  shapes:
    RoundedRect     (background)
    Overlay         (selection highlight, optional, behind text)
    Text            (the buffer — Single wrap; Wrap is multi-line v2)
    Overlay         (caret, painted last)
```

All four shapes are pushed during `show()` and live inside the leaf's
shape slice. Same contiguity model as `Button` (background + label).

## State

A `TextEditState` row in `Ui::state`, keyed by `WidgetId`:

```rust
pub(crate) struct TextEditState {
    pub buffer: String,
    pub caret: usize,        // byte offset into `buffer`
    pub selection: Option<usize>, // anchor byte; None = no selection
    pub blink_phase: f32,    // 0..1, advanced from frame dt; v1 may stub to 1
}
```

Why byte offsets, not chars: cosmic-text's hit-testing returns byte
cursors, and `&buffer[..caret]` is the natural way to measure
caret-from-line-start. Char/grapheme arithmetic happens at edit time
(`unicode-segmentation` for grapheme-aware backspace; defer if we want
to keep deps small in v1 — ASCII-correct backspace by byte is fine).

## Input plumbing changes

### 1. Extend `InputEvent`

```rust
pub enum InputEvent {
    PointerMoved(Vec2),
    PointerLeft,
    PointerPressed(PointerButton),
    PointerReleased(PointerButton),
    Scroll(Vec2),
    // NEW:
    KeyDown { key: Key, mods: Modifiers, repeat: bool },
    KeyUp   { key: Key, mods: Modifiers },
    Text(SmolStr),  // committed character(s); fired separately from KeyDown
}
```

`Key` is a palantir-native enum mapping the small set we care about
(arrows, backspace, delete, home, end, enter, tab, escape, plus letter
keys for shortcuts later). Translate from `winit::keyboard::Key` in
`InputEvent::from_winit`. `Modifiers` is bitflags (ctrl/alt/shift/super).

The `Text` event is separate from `KeyDown` because IME and dead-key
composition emit text without a corresponding physical keypress, and a
key like `Enter` produces a logical key but typically *not* a Text
commit. Match winit's `WindowEvent::Ime(Commit)` + `KeyboardInput` split.

### 2. Focus tracking on `InputState`

```rust
focused: Option<WidgetId>,
frame_keys: Vec<KeyDown>,    // drained by focused widget at record time
frame_text: SmolStr,         // committed text this frame, ditto
```

- `PointerPressed(Left)` hit-tests for `Sense::click()` and *separately*
  for the focusable bit (see §3). What happens on a press that misses a
  focusable widget is governed by `Ui::focus_policy`:

  ```rust
  pub enum FocusPolicy {
      /// Press on non-focusable widget or empty surface preserves the
      /// current focus. The caret stays put; keystrokes keep flowing
      /// to the focused TextEdit. Friendlier for sketches and tooling
      /// UIs where every other widget is a Button.
      PreserveOnMiss,
      /// Press anywhere that isn't a focusable widget clears focus.
      /// Native-app convention on most platforms (click-outside-to-
      /// blur). Default.
      ClearOnMiss,
  }
  ```

  Default is `ClearOnMiss` — matches what users expect from native
  GUIs. Apps that want sticky focus (sketches / tooling where
  clicking a Button shouldn't kill an in-progress edit) set
  `ui.set_focus_policy(PreserveOnMiss)` once at startup. Programmatic
  `Ui::request_focus(None)` works under either policy for explicit
  dismissal (Escape-to-blur, modal close, etc.).
- `KeyDown`/`Text` events accumulate into the frame queues. They're
  dispatched by lookup, not routed: only the widget whose
  `WidgetId == focused` reads them at record time. Same model as
  `scroll_delta_for(id)`.
- Eviction: if `focused` widget vanishes from the tree (same diff
  driving `state.sweep_removed`), clear it.

Add public `Ui::focused_id() -> Option<WidgetId>` and
`Ui::request_focus(id)` for programmatic focus (e.g. autofocus on
mount).

### 3. Focusable flag

Buttons are clickable but **not focusable** — clicking one shouldn't
steal the caret from a TextEdit. So `Sense::Click` alone can't decide
focus. Add a `focusable: bool` on `Element` (default `false`); only
TextEdit flips it to `true`. Cascade can copy it into a per-node bit
the hit-test path reads, alongside the existing sense byte.

`PointerPressed(Left)` then does:

1. Hit-test for `Sense::click()` as today (drives `active` /
   `clicked`).
2. *Separately* hit-test for `focusable`. If it returns `Some(id)`,
   `focused = Some(id)`. Otherwise leave `focused` untouched.

Don't add a `Sense::Focus` variant — focus is orthogonal to pointer
participation (programmatic focus, tab-cycling later) and bolting it
onto `Sense` would conflate two concepts that diverge fast.

### 4. winit wiring (example side, not in `Ui`)

`helloworld.rs` / `showcase` translate `WindowEvent::KeyboardInput` and
`WindowEvent::Ime(Commit)` via `InputEvent::from_winit`. Same pattern as
the existing pointer translation. Enable IME on the window
(`window.set_ime_allowed(true)`) once focus lands on a TextEdit — that's
a `Ui::wants_ime()` query for the host to poll, deferred to v1.1.

## Widget surface

```rust
pub struct TextEdit<'a> {
    element: Element,
    text: &'a mut String,        // borrowed mutable ref, like egui
    style: Option<TextEditTheme>,
    placeholder: Cow<'static, str>,
    multiline: bool,             // forced false in v1
}

impl<'a> TextEdit<'a> {
    pub fn new(text: &'a mut String) -> Self { ... }
    pub fn placeholder(mut self, s: impl Into<Cow<'static, str>>) -> Self { ... }
    pub fn show(self, ui: &mut Ui) -> Response { ... }
}
```

Borrow model: caller owns the buffer, widget mutates via `&mut String`.
Matches egui and avoids the "two sources of truth" problem of caching
in `TextEditState` and reconciling. The state row only stores
`caret`/`selection`/`blink_phase`. On every `show()`:

1. Read `state_mut::<TextEditState>(id)` for caret/selection.
2. If `*self.text != state.buffer` (host mutated externally), reset
   caret to clamped end. Drop the cached buffer mirror — just always
   trust `*self.text` and clamp `caret <= self.text.len()`.
3. If focused, drain `ui.input.frame_keys` + `frame_text` and apply
   edits to `*self.text`, updating `caret`.
4. Hit-test the press position (when `response.pressed` and the press
   landed *this frame*) against the shaped buffer to set caret. v1
   approximation: linear scan of `Text` shape's measured glyph
   positions; once cosmic exposes per-glyph x for us, swap in.
5. Push background → selection overlays → text → caret overlay.

Edits in v1:

- `Text(s)`: insert at `caret`, advance.
- `Backspace`: remove the byte/grapheme before `caret`.
- `Delete`: remove after `caret`.
- `Left`/`Right`: move caret by one grapheme (or one byte for v1
  ASCII).
- `Home`/`End`: caret to 0 / `text.len()`.
- `Enter`: ignored in single-line v1 (multi-line: insert `\n`).

No selection edits in v1 (no shift+arrow, no drag select). Land them
once the keyboard plumbing is proven.

## Theme

```rust
pub struct TextEditTheme {
    pub background: Color,
    pub background_focused: Color,
    pub stroke: Option<Stroke>,
    pub stroke_focused: Option<Stroke>,
    pub radius: Corners,
    pub padding: Spacing,
    pub text: Color,
    pub placeholder: Color,
    pub caret: Color,
    pub selection: Color,
}
```

Slot it into `Theme::text_edit`. Defaults: dark slate background,
1px focused stroke in the existing button-blue, white text, 50% white
placeholder, white caret, 30% blue selection.

## Caret rendering

Caret is a 1px-wide `Overlay` rect at owner-local coords. Position
needs the byte-offset → x mapping, which means asking the shaped
buffer where `caret` byte falls. Two options:

- **Easy v1**: re-measure `&buffer[..caret]` at the same font/size;
  `MeasureResult.size.w` is the caret x. One extra cache miss per
  frame the caret moves; fine.
- **Better**: extend `MeasureResult` (or a sibling lookup) with a
  `byte_to_x(offset) -> f32` API backed by the cached cosmic buffer.
  Defer until we want IME / multi-line / drag-select; same lookup
  serves all three.

Blink: stub to "always on" in v1. A `request_repaint()` schedule on a
0.5 s timer can come with the first animation pass.

## Tests

- Unit: `TextEditState` mutations — insert, backspace at end / mid /
  start, delete, left/right past boundaries, home/end on empty buffer.
  All host-side, no `Ui` needed.
- Integration: drive `Ui::on_input(KeyDown(...))` + `Text(...)` against
  a focused TextEdit, assert `*text` matches expected after each
  sequence.
- Pin: clicking outside a focused TextEdit clears focus; pressing keys
  with no focus is a no-op (no panic, no buffer mutation).
- Showcase tab: a TextEdit with a label echoing its current buffer.

## Steps

Order matters — each step compiles and runs the existing showcase.

1. **Keyboard event types.** Add `Key`, `Modifiers`, `KeyDown`,
   `KeyUp`, `Text` to `InputEvent`. Update
   `InputEvent::from_winit` to translate. No `InputState` consumer
   yet — events fall on the floor. Verify nothing regresses.
2. **Frame queues.** Add `frame_keys`/`frame_text` to `InputState`,
   push into them from `on_input`, drain in `end_frame`. Still no
   reader.
3. **Focus.** Add `focused: Option<WidgetId>` plus
   `Ui::focused_id()` / `Ui::request_focus(id)`. Set on
   `PointerPressed(Left)` to whatever click hit-test returns.
   Evict on cascade-removal in `end_frame`. Add a focus test.
4. **Caret-x lookup.** Pick the v1 "remeasure prefix" path — no
   `MeasureResult` change yet. Add a tiny helper next to `Text` that,
   given `(text, caret_byte, font_size)`, returns the prefix width.
5. **TextEdit widget.** New file `src/widgets/text_edit.rs`. Leaf
   `Element`, `Sense::CLICK`. State row + theme slot + the four shapes.
   Drain frame queues only when `id == focused`. Apply edits, clamp
   caret. Add `pub use widgets::text_edit::{TextEdit, TextEditTheme}`
   to `lib.rs`.
6. **Tests + showcase tab.** Unit tests for state mutations.
   Integration test feeding events through `Ui`. New
   `examples/showcase/text_edit.rs` tab.
7. **winit wiring in examples.** Translate `KeyboardInput` and
   `Ime(Commit)` in both example apps' event paths. Enable IME
   unconditionally for now (v1.1: gate on `Ui::wants_ime()`).

Each step ends with `cargo fmt && cargo clippy --all-targets -D
warnings && cargo test`.

## Out of scope (future slices)

- Multi-line (re-uses caret-x via `byte_to_x` on the shaped buffer plus
  per-line vertical offset).
- Mouse-drag selection (needs `Sense::ClickAndDrag` and drag-delta
  consumption — we already have `drag_delta` plumbing for scrollbars).
- Shift+arrow selection extension.
- Copy/paste (clipboard crate, ctrl+c/x/v shortcut routing).
- Undo/redo (a ring buffer per `TextEditState`; `request_repaint` on
  edit so a 60Hz host never misses a frame).
- IME composition preview (cosmic-text supports it; needs winit IME
  events `Preedit`/`Commit` differentiation and an extra `Text`
  shape under the caret with a different color/underline).
- Auto-focus / `Ui::wants_ime()` for the host to call
  `window.set_ime_allowed(true)` only when needed.
