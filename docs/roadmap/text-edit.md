# TextEdit

v1 ships single-line + multi-line typing, caret (multi-line aware via
`cursor_xy`), codepoint backspace/delete + arrows + home/end + up/down,
click-to-place via `Buffer::hit`, drag-to-select, focus + `FocusPolicy`,
escape-to-blur (two-stage), selection (visible wash via
`selection_rects` + shift/arrow extension + ctrl/cmd+A + edits replace
the range), undo/redo (128-entry ring with edit-kind coalescing),
clipboard (ctrl/cmd+c/x/v + right-click context menu via `arboard`),
IME `Commit`, placeholder, theme per-state, disabled, overflow
handling (`ClipMode::Rect` + scroll-to-caret: single-line x,
multi-line y), **caret blink (500 ms half-period, resets on any caret
/ selection / text change, host wake via `request_repaint_after`)**.
See `src/widgets/text_edit/design.md`.

## Next — tier 1, perceived-quality bar

- **Word navigation.** Ctrl/Cmd+ArrowLeft/Right,
  Ctrl/Cmd+Shift+ArrowLeft/Right, double-click selects word,
  triple-click selects line / all. Universal expectation.
- **Grapheme-aware boundary walks.** `unicode-segmentation` so
  shift+arrow / backspace step whole graphemes (emoji + ZWJ, accent
  combiners) instead of bare codepoints.

## Next — tier 2, non-English / a11y correctness

- **IME preedit.** Currently dropped at translation. Plumb
  `InputEvent::ImePreedit { text, cursor }`, render underlined under
  caret, commit on `Ime(Commit)`. Non-Latin input is unusable today.
- **`Ui::wants_ime()`.** Host gates `set_ime_allowed(true)` on
  whether a focused widget actually wants IME, instead of
  unconditional.
- **Drag-select auto-scroll.** When dragging past the editor's edge in
  multi-line mode, the viewport should follow. Builds on overflow
  handling.
- **PageUp / PageDown.** Trivial once viewport height + scroll exist.

## Next — tier 3, builder knobs every text editor exposes

- **`.read_only(bool)`** — accept focus + selection + copy, refuse
  edits. "Display a selectable string" use case.
- **`.password(char)`** — render masked glyphs (cosmic shapes the mask
  char). Blocks clipboard-history leaks of paste content.
- **`.max_length(n)`** — reject inserts past the limit (also gates
  `paste_at_caret`).
- **`.on_change(impl FnMut(&str))`** — sugar over the "diff every
  frame" pattern callers do today.

## Later — deferred until a workload asks

- RTL / bidi correctness (cosmic supports it; widget doesn't query).
- Screen-reader / accessibility hooks (focus announcement,
  selection-change events, aria labels).
- Find / replace dialog (orthogonal to the widget itself).
- Syntax-tinted spans (per-byte color runs).
- Soft-wrap visual marker.
- Drag-and-drop text reorder.
- Paste rich content (image / styled text) fallback.

## Known gotchas

- **First-frame multi-line wrap:** `wrap_target` reads from
  `response.rect`, which is `None` until cascade runs — multi-line
  editors lay out unwrapped on their first recorded frame. The
  `request_discard` slice in `docs/roadmap/invalidation.md` would fix
  this generically.
- **Selection invariant** (`Some(a)` always implies `a != caret`) is
  enforced at mutation sites only; no assertion guards it, so future
  nav code could leak empty-`Some` selections.
- **`last_edit_kind` undo coalescing** rides on every caret-only
  motion clearing the field — fragile if a new motion handler forgets
  to route through `move_caret`.
