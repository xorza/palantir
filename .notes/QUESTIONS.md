# Open questions from the API refactor

Each question blocks one item of `API_CHANGES.md`. The item stays in that file until you answer.

## Q1. A31: how `TextEdit::placeholder` and `DragValue::suffix` read a `TextInput`

**The problem.** A31 says both setters take `impl Into<TextInput<'a>>`. A `TextInput` can be
`Interned(InternedStr)`, and both widgets need the *characters* at record time:

- `TextEdit` measures the placeholder with `ui.probe_text(TextRun { text: &str, .. })` to place
  the caret and the block (`text_geometry.rs:96`).
- `DragValue` formats the suffix into its label with `ui.fmt(format_args!("{v}{suffix}"))`
  (`drag_value/mod.rs:308`).

No public API turns an `InternedStr` back into a `&str`, and AGENTS.md lets a widget reach only the
public API. So the plan cannot land as written without new public surface.

**Options.**

1. **Add a public read and a probe over interned text.** `Ui::probe_text` gets a twin that takes an
   `InternedStr` (it can borrow the record store and the shaper as disjoint fields), and
   `Ui::fmt` users read the suffix through a public `Ui::text(InternedStr) -> &str`. Both setters
   then take `TextInput` as A31 says. Cost: two new public `Ui` methods.
2. **Take `impl Into<Cow<'a, str>>`.** Borrowed and owned text work, interned text does not. Small,
   but the setters then read unlike every other text setter.
3. **Keep `&'a str`** and close A31 as rejected. A `String` caller writes `&s`; a `fmt!` caller
   cannot pass its handle.

**Recommendation.** Option 1, because it is the only one that keeps the rule "every text argument
takes `TextInput`", and the probe twin also lets an outside widget measure interned text.
