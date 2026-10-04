audit core data structs NodeRecord
Widget-authoring surface

Visual suite: assert `Goldens::orphans` — needs one test that names every golden the suite draws

## Focus traversal and IME — decided 2026-10-04, focus traversal first

Focus traversal:

- **Order.** Record order, changed by two new `Configure` settings, as WPF has `IsTabStop` and
  `TabIndex`: `tab_stop(bool)` removes the Tab stop and keeps click focus, and `tab_index(i16)`
  sorts the stops ascending from a default of 0, with ties in record order.
- **Scope.** The topmost open overlay (`Modal`, `Popup`, `Menu`) traps Tab, which cycles from its
  last stop to its first. With no overlay open, Tab cycles through the `Main` layer.
- **Indicator.** The framework draws one ring around the focused widget, from a new theme value,
  only when focus came from the keyboard (CSS `:focus-visible`). No widget draws its own.
- **Tab stops.** Every interactive widget, each with the keys the ARIA practices give its role:
  `Button`, `PopupTrigger` and `ColorButton` on Space and Enter; `ComboBox` opens on Space, Enter
  and Alt+Down, and steps with the arrows; `DragValue` enters its edit field as a spin box;
  `Splitter` moves on the arrows; `MenuItem` activates on Enter.

IME:

- **Enable.** A focused widget calls `ui.request_ime(caret_rect)` on each frame it wants IME
  text. The call turns IME on (winit `set_ime_allowed`) and places the candidate list
  (`set_ime_cursor_area`). A frame with no call turns IME off. `TextEdit` uses only this public
  call.
- **Events.** `InputEvent<'a>` gets `ImePreedit { text: &'a str, cursor: Option<Range<usize>> }`
  and `ImeCommit(&'a str)`, and stays `Copy`. The queue copies the text into a retained buffer.
- **Read.** A commit enters `keyboard_events` in its exact place as text-only presses
  (`Key::Other`), split at character boundaries into `KeyText`-sized pieces, so its order against
  Backspace is exact. The preedit is a level, read with `ui.ime_preedit()`, which `TextEdit`
  draws underlined at the caret. The `KeyText` doc that says it carries no IME commit changes.

