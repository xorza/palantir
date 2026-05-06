# TextEdit

v1 ships single-line typing, caret, codepoint backspace/delete +
arrows + home/end, click/drag-to-place, focus + `FocusPolicy`,
escape-to-blur, IME `Commit`. See `src/widgets/text_edit/design.md`.

## Next

- **Selection (visible + edits).** Selection-fill `Overlay` under text;
  shift+arrow / home/end / drag extends, plain arrow collapses,
  ctrl+a all-select; edits replace selected range. State + theme
  slots already there.
- **Glyph hit-test via `Buffer::hit`.** Replace O(n) `caret_from_x`
  scan with one shaped lookup. Same upgrade gives multi-line
  `byte_to_xy`.
- **Grapheme-aware boundary walks.** `unicode-segmentation` once
  selection lands.
- **Multi-line.** Enter inserts `\n`, PageUp/Down live, caret y from
  `Buffer::hit`, `TextWrap::Wrap` when builder sets `multiline`.
- **Clipboard.** `arboard` behind `Clipboard` trait on `Ui`; route
  ctrl/cmd+c/x/v from `frame_keys`.
- **IME preedit.** Currently dropped at translation. Plumb
  `InputEvent::ImePreedit { text, cursor }`, render underlined under
  caret, commit on `Ime(Commit)`.
- **`Ui::wants_ime()`.** Host gates `set_ime_allowed(true)` instead of
  unconditional.
- **Undo / redo.** Bounded ring buffer per `TextEditState`, coalesce by
  edit-kind + timestamp. Needs shortcut routing.
- **Caret blink.** Tick alpha off `dt` once an animation-tick infra
  consumer exists.
