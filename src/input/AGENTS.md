# Input

How keys, chords and wheels mean the same on every OS. The top `AGENTS.md`'s "One behavior in
every build and on every platform" governs: a chord means the same everywhere, the command key
its only substitution.

- **One chord table**, Cmd standing in for Ctrl on macOS. Refused: a second chord for one action,
  as Ctrl+Y beside Ctrl+Shift+Z; each platform's own table, as
  [Flutter](https://codebrowser.dev/flutter/flutter/packages/flutter/lib/src/widgets/default_text_editing_shortcuts.dart.html)
  binds, which a person who moves between machines learns twice.
- **A shortcut label follows its platform's convention**, the one thing about a chord that does:
  symbols on macOS, as [Qt's `NativeText`](https://doc.qt.io/qt-6.5/qkeysequence.html) shows them,
  in [Apple's order](https://daringfireball.net/2026/03/modifier_key_order_for_keyboard_shortcuts),
  names elsewhere.
- **A person's scroll setting moves a scroll, not a zoom**: a wheel event carries its notches
  apart from the lines they scroll, and a zoom reads the notches. A host reads the system's
  setting where the system has one
  ([Microsoft's sample](https://devblogs.microsoft.com/oldnewthing/20030807-00/?p=42963)).
  Refused: one lane, multiplied, which makes a wheel zoom jump
  ([GTK #1408](https://gitlab.gnome.org/GNOME/gtk/issues/1408)).
- **Activation**: Space and Enter fold into one left click, a toggle takes Space alone, as
  WAI-ARIA leaves Enter to the form; a focused widget takes those keys and no letter, as a Qt or
  GTK button accepts only the keys it acts on.
