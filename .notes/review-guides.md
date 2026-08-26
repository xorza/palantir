# Palantir: coding-guide conformance

Findings from a mechanical sweep of `src/` against the Rust code-style rules.
Each item is a checklist entry: **when you address one, delete it.** The file
lists open findings only — no "done" markers, no resolved section.

Scope is conformance to the stated rules. Documentation accuracy, duplicated
predicates, and behaviour are covered in `.notes/review-palantir.md` and
`.notes/ISSUES.md`, and are not repeated here.

Clean: directory modules, intra-crate re-exports, nested collections,
`#[derive(Debug)]` coverage, `Result` discipline, comment discipline,
`reserve_exact` usage, and test runtime (1581 tests, 4.9s).

---

## Smaller items

- [ ] `src/ui/layer_scope.rs:39` — `LayerScope::anchored` returns
      `(Vec2, Option<Size>)`. The `Placement::Fixed { anchor, size }` variant
      beside it already names those two fields.

- [ ] `src/layout/grid/axis_scratch.rs:258`, `:267`, `:278` — three
      `fill_weight().unwrap()` calls whose `Some` is not obvious from the line;
      the rule wants `.expect("…")` there.

- [ ] `src/layout/stack/mod.rs:263` and `:327` — `frozen_alloc.unwrap()`, same.

- [ ] `src/scene/cascade/engine.rs:355` — `self.stack.pop().unwrap()`, same.

- [ ] `src/scene/forest.rs:331` — `open_frames.last_mut().unwrap()`, same.
