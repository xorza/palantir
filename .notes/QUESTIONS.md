# Open questions from the API refactor

Each question blocks one item of `API_CHANGES.md`. The item stays in that file until you answer.

## Q2. A19: IME and focus traversal need a design each

**The problem.** A19 says IME text and Tab focus traversal are features with their own design,
out of scope for the defect work. Both add public surface, so both need your go-ahead first.

- **IME** needs `InputEvent` variants for the preedit string (with its cursor range) and the
  commit, winit's `Ime` events enabled per focused field, and `TextEdit` drawing the preedit
  inline. Open choices: whether a widget opts into IME through a `Configure` setter or
  `TextEdit` alone enables it, and where the candidate window is anchored (the caret rect each
  frame through a `Ui` call).
- **Focus traversal** needs a focus order. Open choices: document order of focusable nodes (the
  browser default, no new API), or an explicit `tab_index` setter beside `focusable(bool)`; and
  which layer scopes traversal (a modal traps it, as a browser `dialog` does).

**Recommendation.** Focus traversal first, in document order with no new setter, and a modal
or popup as the trap; then IME, enabled by `TextEdit` alone. A20's keyboard support on toggles
and ranges does not wait on either: those widgets take keys once focused by a click.
