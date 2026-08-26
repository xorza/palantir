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

## Tests that assert a threshold rather than a value

The rule asks for hand-computed exact outputs. These accept a range where the
expected number is derivable.

- [ ] `src/text/tests/reuse.rs:324` — `assert!(counts.hits > 0);`, no message.
- [ ] `src/renderer/frontend/composer/tests/brushes.rs:186` —
      `assert!(rows[0].0 >= 1);`, no message.
- [ ] `src/renderer/backend/text/tests.rs:399` — `assert!(n > 0, "'File' must
      emit glyphs")`. "File" shapes to a known glyph count.
- [ ] `src/renderer/backend/raster_atlas/tests.rs:378` — `assert!(placed > 0)`
      for a 128² side whose tile capacity is fixed.
- [ ] `src/scene/damage/tests/clipping.rs:287` and `:294` — `>= 1` and `>= 2`
      against a fixture that builds a known number of rows.
- [ ] `src/renderer/frontend/composer/tests/curves.rs:334` —
      `assert!(batch.items.len >= 2 && …is_multiple_of(2))`.
- [ ] `src/animation/tests/retarget.rs:18` — `assert!(mid > 0.4 && mid < 0.6)`.
      The easing at the midpoint has a closed form.
- [ ] `src/text/tests/geometry.rs:254` and `:255` — `> 0.0` on both extents.
- [ ] `src/text/tests/wrap.rs:94` — `assert!(sans > 0.0 && sans.is_finite());`
- [ ] `src/text/tests/truncate.rs:37` — `assert!(truncated.size.w <= 20.0);`
- [ ] `src/text/tests/truncate.rs:392` — `assert!(wrapped.size.h > 16.0)`.
- [ ] `src/widgets/drag_value/tests/layout.rs:63` — `assert!(display_w >= 40.0)`
      against a `min_size` floor the test itself sets.
- [ ] `src/primitives/rect/tests.rs:99` — `>= 0.0` on both extents of a
      computed intersection.
- [ ] `src/renderer/gradient_atlas/tests/bake.rs:119` — `assert!(q.b <= 0.02)`
      with no stated derivation for the tolerance.

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
