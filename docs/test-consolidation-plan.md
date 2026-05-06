# Test Consolidation Plan

Audit covers all `#[test]`s in the crate (~17k LOC across ~75 files). No edits — plan only. Per-module proposals respect CLAUDE.md: alloc-budget tests, visual goldens, regression pins, and cross-module boundaries are deliberately preserved.

## Headline numbers

| Area | Before | After | Δ |
|---|---|---|---|
| Layout + primitives + tree + common | 133 | 106 | −27 |
| UI | 47 | 41 | −6 |
| Widgets | 86 | 67 | −19 |
| Renderer | 69 | 52 | −17 |
| Input + text + support | 60 | 48 | −12 |
| Integration (alloc + visual) | 22 | 22 | 0 |
| **Total** | **~417** | **~336** | **~−81 (≈19%)** |

(Counts approximate; some test mods weren't fully enumerated. Numbers are upper bounds — pre-merge re-read is mandatory before each cut.)

---

## Tier 1 — High-confidence, mechanical (≈30 tests cut)

These are literal "same setup, same call, varying input/expected" merges. Each becomes a `for (label, input, expected)` loop with `assert_eq!(…, "case: {label}")`.

1. **`primitives/rect/tests.rs`** — 11 → 4
   - 5 `intersects_*` (overlap / disjoint / touching / self / zero) → 1 parametric.
   - 4 `union_*` → 1 parametric.
   - Drop 2 trivial `area_*` (arithmetic; no contract).

2. **`primitives/urect/tests.rs`** — 8 → 2
   - 5 `intersect_*` → 1 parametric.
   - 3 `clamp_to_*` → 1 parametric.

3. **`tree/tests.rs`** — 21 → 16
   - 6 hash-changes-when-property-changes tests (fill, size, padding, visibility, justify, shape_order) → 1 parametric over `Property` enum. Drop 1 meta-guard `changing_text_content_changes_hash` if it just re-proves the test fixture.

4. **`tree/element/tests.rs`** — 5 → 2
   - 4 PaintAttrs round-trip variants → 1 parametric over flag combos.

5. **`widgets/tests/track_caller.rs`** — 11 → 3
   - 8 constructor-track_caller tests (`Button::new`, `Frame::new`, `Panel::{hstack,vstack,zstack,wrap_hstack,wrap_vstack}`, …) → 1 parametric loop calling each constructor twice via a `&dyn Fn(&mut Ui)` table and `assert_distinct`. Keep the 3 explicit `with_id` / closure-helper cases — they pin different mechanisms.

6. **`widgets/text_edit/tests.rs`** — 32 → 26
   - 10 `apply_key_*` unit tests over `(input_str, caret_in, key, str_out, caret_out)` → 1 parametric. Cases: printable / cmd-modifier-skipped / space / backspace mid / backspace at start / delete mid / delete at end / arrows step / arrows clamp / home-end. Keep `boundary_helpers_jump_full_codepoints` (documents UTF-8 boundary contract) and the 21 integration tests untouched.

7. **`layout/stack/tests.rs`** — 15 → 12
   - 4 justify variants (Center/End/SpaceBetween/SpaceAround) on identical setup → 1 parametric.

8. **`layout/wrapstack/tests.rs`** — 14 → 11
   - 3 per-line justify variants → 1 parametric.
   - 2 wrap-threshold variants → 1 parametric.
   - 1 subsumed test.

9. **`layout/canvas/tests.rs`** — 7 → 6: 2 Fill-child variants (Fixed canvas / Hug canvas) → 1 parametric.

10. **`layout/grid/tests.rs`** — 13 → 12: `col_span` + `row_span` → 1 parametric over axis.

11. **`layout/cross_driver_tests/no_overlap.rs`** — 5 → 4: 2 overlap-check variants → 1.

---

## Tier 2 — High value, requires care (~30 tests cut)

Same shape, but one or more cases carry a regression-pin annotation; verify each input survives the merge with its own labeled case.

12. **`ui/damage/tests.rs`** — 24 → 19
    - **(a)** Heuristic threshold cluster (`small_damage_stays_partial`, `large_damage_falls_back_to_full`, `at_threshold_stays_partial`, `zero_area_surface_forces_full`): 4 calls to `Damage::filter(surface)` → 1 parametric over `(damage_area, surface_area, expected_variant)`.
    - **(b)** Full-repaint events (`surface_resize_forces_full_repaint`, `scale_factor_change_forces_full_repaint`): 2 → 1 parametric over `mutate_fn`.
    - **(c)** Drop `first_frame_marks_every_node_dirty` — subsumed by `first_frame_filter_is_full` (latter is more direct).
    - Keep fill-change / transform tests separate — they look similar but assert different invariants (screen-space vs layout-space).

13. **`widgets/tests/scroll.rs`** — 26 → 18
    - **(a)** Bars submodule: 6 `bar_geometry(...)` thumb-size/offset tests → 1 parametric over `(viewport, content, offset, track, expected_thumb_size)`.
    - **(b)** Scroll-state cluster (3): wheel delta, clamp-at-max, no-overflow-stays-zero → 1 parametric over `(content_h, wheel_input, expected_offset)`.
    - **(c)** Measure-side `scroll_content` (4): V/H/XY/empty → 1 parametric over axis.
    - **Keep** the warm-cache + nested-clipped-warm-cache pair separate — distinct regression pins.

14. **`renderer/frontend/encoder/tests.rs`** — 19 → 16
    - 3 baseline encode tests (empty / fill / invisible) → 1 parametric over `(scene_kind, expected_draw_count)`.

15. **`renderer/frontend/encoder/cache/tests.rs`** — 13 → 9
    - Round-trip same-origin + shifted-origin → 1 parametric (2 cases).
    - 3 mismatch-misses (hash / wid / avail) → 1 parametric over `mismatch_field`.

16. **`renderer/frontend/composer/tests.rs`** — 24 → 20
    - 3 scissor-grouping tests (no-clip / single / nested) → 1 parametric over clip topology. Keep specialised `intersects_nested_clips` and `cull_drops_outside` standalone.

17. **`renderer/frontend/composer/cache/tests.rs`** — 11 → 8 (mirror of encoder/cache).

18. **`layout/cache/integration_tests.rs`** — 5 → 3: 3 grid-topology variants → 1 parametric. **Verify each topology after merge** — these protect `MeasureCache` partial-invalidation.

19. **`widgets/tests/zstack.rs`** — 3 → 2: 2 alignment cases → 1 parametric over `(halign, valign)`.

20. **`text/mod.rs`** — 13 → 11: shaping/wrapping clusters (sample only, requires re-read).

---

## Tier 3 — Conditional / requires re-read first

21. **`input/tests.rs`** — 41 → ≈32
    - Likely 9–12 button-response lifecycle tests with parallel shape (build → on_input → assert response). Splitting by axis: (a) basic press/release/click, (b) hover variants, (c) disabled suppression, (d) sense-pass-through. **Action:** before merging, list every `assert_eq!` per test in a 4-column table; only merge groups whose assertion shape is identical. Some "input" tests are integration pins for the focus state machine — keep those.

22. **`ui/tests.rs`** — 19 → 18
    - Only candidate: the `text_reshape_skipped_when_unchanged_*` cluster (~3 tests with the same shape, different mutation). Confirm assertion count parity before merging.

---

## Leave alone (explicit non-targets)

- **`tests/alloc/fixtures/widgets.rs`** (7) — each pins a distinct per-frame allocation budget. Merging two would average their budgets and lose the "this fixture regressed" signal. **Non-negotiable per CLAUDE.md.**
- **`tests/alloc/harness_tests.rs`** (8) — meta-tests of the auditor itself; each pins a different harness invariant.
- **`tests/visual/**`** — golden-image harness. Per skill rules, never merge snapshot tests; you lose which input regressed.
- **`layout/cross_driver_tests/{convergence,fill_propagation,text_wrap}.rs`** — cross-driver behaviour, module boundary == coverage boundary.
- **`layout/cache/tests.rs`** (15) — each pins a distinct cache invariant (lifecycle / partial / compaction / reappearance).
- **`layout/intrinsic.rs`** (3) — each is a distinct cache transition.
- **All `bug_*` / `regression_*` / issue-referencing tests** — document why behaviour exists.
- **`primitives/color.rs`, `primitives/corners.rs`** — small inline mods, clusters look distinct on inspection.
- **`widgets/tests/visibility.rs`** (6) — collapsed vs hidden assertions are orthogonal (layout vs render output).
- **`widgets/tests/{panel,frame,canvas}.rs`** — each test covers an orthogonal contract.
- **`renderer/backend/tests.rs`** (2) — distinct render-schedule invariants.
- **`input/keyboard.rs`** (6) — distinct key-handling contracts.
- **Doctests** — they double as docs.

---

## Execution rules

When this plan is applied:

1. **Always include a per-case label** in the assertion message (`assert_eq!(got, want, "case: {label}")`). Without labels, refuse the merge — failures become undebuggable.
2. **Never introduce new test deps** (`rstest`, `test-case`). Plain `for` over a `&[(…)]` slice is the target shape.
3. **One file at a time.** After each file: `cargo nextest run -p palantir`, then `cargo test --doc`. Revert that file alone if anything fails.
4. **Re-verify subsumption literally** before any cut — read both tests' assertions side-by-side and confirm the surviving test asserts ≥ what's removed under ≥-strict input. If unsure, keep the test.
5. **Order of attack:** Tier 1 first (mechanical), then Tier 2 (with care), then re-evaluate Tier 3 with a fresh re-read. Tier 3 should not be auto-merged — surface the proposed merges first.
6. **Stop conditions:** if a file's count would drop > 50%, or any cut crosses a module boundary, pause and surface for review.

---

## Suggested commit slicing

To keep blame readable and bisection cheap:

- **C1** primitives (rect + urect): −13
- **C2** tree + tree/element: −8
- **C3** layout drivers (stack, wrapstack, canvas, grid, no_overlap): −9
- **C4** layout cache integration: −2
- **C5** ui/damage: −5
- **C6** widgets/track_caller: −8
- **C7** widgets/scroll + zstack: −9
- **C8** widgets/text_edit: −6
- **C9** renderer/encoder + encoder/cache: −7
- **C10** renderer/composer + composer/cache: −7
- **C11** ui + text (small): −3
- **C12 (gated)** input/tests parametric pass: −≈9 — only after re-read

Each commit: a single file (or tightly-related pair), one test pass, fmt + clippy clean.
