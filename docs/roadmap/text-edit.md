# TextEdit

v1 ships single-line typing, caret, codepoint backspace/delete +
arrows + home/end, click/drag-to-place, focus + `FocusPolicy`,
escape-to-blur, IME `Commit`, selection (visible wash + shift/arrow
extension + drag-select + ctrl/cmd+A + two-stage Escape, edits replace
the range), and clipboard (ctrl/cmd+c/x/v via `arboard`). See
`src/widgets/text_edit/design.md`.

## Next

- **Glyph hit-test via `Buffer::hit`.** Replace O(n) `caret_from_x`
  scan with one shaped lookup. Same upgrade gives multi-line
  `byte_to_xy` and a cheaper selection-rect width computation
  (two `caret_x` calls become one shaped lookup pair).
- **Grapheme-aware boundary walks.** `unicode-segmentation` so
  shift+arrow / backspace step whole graphemes (emoji + ZWJ, accent
  combiners) instead of bare codepoints.
- **Word navigation.** Ctrl+ArrowLeft/Right, Ctrl+Shift+ArrowLeft/Right,
  double-click selects word, triple-click selects all.
- **Multi-line.** Enter inserts `\n`, PageUp/Down live, caret y from
  `Buffer::hit`, `TextWrap::Wrap` when builder sets `multiline`.
- **IME preedit.** Currently dropped at translation. Plumb
  `InputEvent::ImePreedit { text, cursor }`, render underlined under
  caret, commit on `Ime(Commit)`.
- **`Ui::wants_ime()`.** Host gates `set_ime_allowed(true)` instead of
  unconditional.
- **Undo / redo.** Bounded ring buffer per `TextEditState`, coalesce by
  edit-kind + timestamp. Needs shortcut routing.
- **Caret blink.** Tick alpha off `dt` once an animation-tick infra
  consumer exists.
