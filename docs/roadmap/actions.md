# Actions & keymap

Today input is routed through `Sense` flags + `HitIndex` (single-pass,
pointer-driven) and key events fall through to whoever calls
`Ui::input` to peek raw keys. There's no notion of a named action, no
chord support, no focus-scoped binding, no way for a host app to say
"`Ctrl+S` means save in this view, format in that one."

That's fine for buttons and panels. It falls apart the moment you
build anything editor-shaped — multi-key chords (`Ctrl+K Ctrl+S`),
modal keymaps (Vim normal vs insert), context-scoped overrides
(`Esc` closes a popup if open, otherwise blurs focus).

## Why this matters

Hardcoding `if key == Key::S && mods.ctrl { save() }` at the call site
sounds fine until you want:

- The same binding rebindable from a settings file.
- A different binding when a text field is focused.
- A two-key chord that doesn't fire the first key as a standalone.
- A help overlay that lists "what does `Ctrl+K` do here right now."

GPUI solves this with `actions!(my_app, [Save, Open])` + a `Keymap`
that maps `(context, key-sequence) → action`, plus capture/bubble
dispatch so a focused widget gets first refusal. The macro half is
optional sugar; the routing half is necessary.

## What we want

A small action-dispatch primitive on `Ui`, layered on top of the
existing focus stack:

- **Typed actions.** Plain `#[derive]`-able marker structs; no global
  registry, no string ids in user code. Internally keyed by
  `TypeId` for routing.
- **Keymap.** Ordered list of `(context-predicate, key-sequence,
  action)`. Context predicates are simple — "focused widget has tag
  X" or "popup layer Y is open." Sequences are 1–N keys with a
  configurable chord timeout.
- **Dispatch.** Capture pass (root → focused leaf) gives ancestors
  first refusal; bubble pass (focused leaf → root) is the default.
  An action handler returns "consumed" or "fall through." This is
  the one place we add capture/bubble — pointer events stay
  single-pass.
- **Action handlers.** Widgets register `ui.on_action::<Save>(id, |ui|
  …)` during record. Stored in a per-frame map, drained at
  end-of-frame like events.
- **Discoverability.** `ui.bindings_for::<Save>()` returns the live
  shortcut for a given action — drives "press `Ctrl+S` to save"
  hints and command palettes for free.

## What it solves

- **Rebindable shortcuts** without rewriting widgets.
- **Focus-aware bindings** — text field captures `Tab`, tree view
  captures arrows, root captures `Ctrl+S`.
- **Chord support** — first key buffers, second key resolves or
  times out.
- **Command palette** — iterate registered actions with their current
  bindings.
- **Modal keymaps** — push/pop keymap layers (Vim modes, "input
  field active," "modal open").

## What it explicitly is not

- Not a global event bus. Actions are typed and locally dispatched,
  not broadcast.
- Not pointer routing. Mouse stays on `HitIndex`/`Sense`. Capture/
  bubble is keyboard only.
- Not a settings system. The keymap is a value the host app builds
  and hands to `Ui`; loading it from JSON / TOML is the host's job.

Block on focus v1 (already shipped) and a real editor-shaped workload
in the showcase before designing further.
