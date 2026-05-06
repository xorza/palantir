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

Pass 2 adds another **~7 test cuts** plus **~5 fixture extractions** (LOC reduction without test reduction). Revised target: **~329 tests**, with shared scaffolding pulled into `support::testing`.

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

## Pass 2 additions — additional merges

Found on a second-pass re-read. Refines or extends pass-1 entries.

23. **`tree/tests.rs`** — refinement of #3
    - The 4 tests `changing_layout_size_changes_hash` (L184), `changing_padding_changes_hash` (L203), `changing_visibility_changes_hash` (L221), `changing_justify_changes_hash` (L241) have **identical structure** (`record_hash` twice, mutate one field, assert hashes differ) → 1 parametric over a local `Property` enum. This is the precise scope of the "6 → 1" estimate in #3.
    - Keep `changing_fill_color_changes_hash` (L94, asserts via per-child loop), `shape_order_matters_for_hash` (L261), `changing_text_content_changes_hash` (L292) as standalone — different assertion shape.

24. **`text/mod.rs`** — caret-position cluster — 3 → 1
    - `caret_x_zero_offset_or_empty_returns_zero`, `caret_x_mono_path_matches_prefix_glyph_widths`, `caret_x_width_independent_of_line_height` (≈L441–L465) all probe `caret_x()` with different `(text, byte_offset, font, expected)` → table-driven over those four columns.

25. **`input/tests.rs`** — focus-policy pair — 2 → 1
    - `focus_lands_on_press_over_focusable_widget_and_preserve_holds_it` and `clicking_non_focusable_widget_preserves_focus_under_preserve_policy` share scaffolding; merge over `(policy, click_target_focusable: bool, expected_focused_id)`. Conservative: counts toward the ≈9 already estimated for this file in #21.

26. **`widgets/tests/visibility.rs`** — alignment pair (was "leave alone") — 2 → 1
    - `hstack_child_align_y_centers_all_children_by_default` and `child_align_self_overrides_parent_default` (L152–L207) both build hstack + measure child y-positions. Parametric over `(parent_align, child_override, expected_y)`. Reverses pass-1 "leave alone" for *only* these two tests; the other 4 (collapsed/hidden semantics) stay separate.

27. **`primitives/corners.rs`** — already-parametric, but compactable (informational)
    - `serialize_then_parse_round_trips` (≈L362–L380) already loops over 3 fixtures. Adding labels would catch the case-id on failure.

Net pass-2 cuts: ≈7 tests (some absorbed into pass-1 estimates).

---

## Shared fixtures — extract without merging tests

Pass 2 found duplicated *setup* across tests whose *assertions* differ. Don't merge those tests; extract a fixture helper into `src/support/testing.rs` (or a per-driver `tests/common.rs`) and call it from each.

**F1. `fixture_grid_with_two_text_cols`** — `support::testing`

```rust
pub(crate) fn fixture_grid_with_two_text_cols(
    ui: &mut Ui,
    col_widths: (Sizing, Sizing),
    text: (&str, &str),
) -> GridFixture { /* (grid_node, left_node, right_node) */ }
```

Call sites: `cross_driver_tests/text_wrap.rs` L266, L361; `cross_driver_tests/fill_propagation.rs` L110.

**F2. `fixture_canvas_with_fill_child`** — `support::testing` or local to `layout/canvas/tests.rs`

```rust
pub(crate) fn fixture_canvas_with_fill_child(
    ui: &mut Ui,
    canvas_size: (Sizing, Sizing),
    pos: (f32, f32),
) -> Rect { /* child's arranged rect */ }
```

Call sites: `layout/canvas/tests.rs` L90, L120 (Fixed/Hug variants).

**F3. `build_scroll_with_content`** — local to `widgets/tests/scroll.rs`

```rust
fn build_scroll_with_content(
    ui: &mut Ui,
    id: &str,
    axes: ScrollAxes,
    viewport: (f32, f32),
    content: (f32, f32),
)
```

Replaces three near-identical inline closures (`build`, `build_h`, `build_xy` at L23, L124, L146) plus the body of `record_two_frames` callers (L435, L456).

**F4. `setup_focused_editor`** — local to `widgets/text_edit/tests.rs`

```rust
fn setup_focused_editor(ui: &mut Ui, size: (f32, f32)) -> WidgetId
```

Wraps "build TextEdit, click to focus, return id". Used by `escape_blurs_focus`, `caret_clamps_after_external_buffer_shrink`, and several integration tests (≈L148–L189).

**F5. `scroll_state(ui, id) -> ScrollState`** — `support::testing`

State-map read is open-coded as `ui.state::<ScrollState>(WidgetId::from_hash("scroll"))` in 4+ scroll tests (≈L323, L587). One-liner helper kills the boilerplate.

**Already centralized — no action:**
- `under_outer(...)` — used 14× across `layout/{zstack,canvas}/tests.rs`, already in `support::testing`.
- `ui_with_text(UVec2)` — used 16× across `cross_driver_tests/`, already in `support::testing`.

**Optional consts** (low value, only if it earns its place after F1–F5 land):
- `SURFACE_400_600`, `SURFACE_800_600`, etc. for repeated `UVec2::new(W, H)` literals. Skip unless `rg 'UVec2::new\(\d+, \d+\)' src/` shows a single value used in 5+ files.

---

## Pass 3 additions — third-pass re-read

Found on a third pass focused on files less covered by passes 1–2.

28. **`layout/zstack/tests.rs`** — alignment pair — 2 → 1 *(distinct from #19, which targets `widgets/tests/zstack.rs`)*
    - `zstack_aligns_per_axis_from_child_override` (L48–76) and `zstack_child_align_cascades_to_auto_axes` (L78–101) both build a `Fixed(100×100)` zstack under a 200×200 surface and assert child-rect offsets given an `(parent_child_align, child_align_override)` pair. Same `under_outer` scaffold, same `panel_rect.min` math.
    - Merge as parametric over `(parent_child_align: Option<Align>, child_overrides: &[(WidgetId, Option<Align>)], expected: &[(f32, f32)])`. Keep `zstack_lays_children_at_inner_top_left_by_default` (L25–46), `zstack_fill_child_stretches_to_inner` (L103–125), and `zstack_hugs_to_largest_child_per_axis_independently` (L8–23) standalone — different invariants (default placement / Fill stretch / Hug rollup).

29. **`ui/tests.rs`** — `prev_frame_captures_*` cluster (informational, gated)
    - `prev_frame_captures_arranged_rect` (L144) and `prev_frame_captures_authoring_hash` (L163) share fixture: build one frame, end_frame, read `ui.prev_frame()` field. Possibly mergeable as parametric over `(extractor_fn, expected)`. **Conservative:** assertion shapes likely diverge (rect vs hash); read both before merging. Don't auto-merge — surface for review. Counts as a pass-3 candidate, not a commit.
    - Plan #22's text-reshape cluster (L229/L263/L292) re-checked: assertions are *orthogonal* (skip path / change path / wrap path), not parametric. Downgrade #22 to: leave alone, or cut **at most** the wrapping variant if its skip-path assertion duplicates L229. Net: probably 19 → 19, not 19 → 18.

30. **`widgets/tests/{panel,frame,canvas}.rs`** — fixture re-check (no merge)
    - All three already use the `ui_at(UVec2)` helper (re-confirmed F1–F5's "already centralized" note). The proposed `under_panel` fixture from a third-pass scan does **not** earn its place — call sites differ in panel kind (hstack/zstack), surface config, and child build closure. Skip.

31. **`primitives/corners.rs`** — internal parametric tests already labeled
    - Re-read confirms `serialize_then_parse_round_trips` (#27) is the only candidate. Other tests (radius arithmetic, hit-test) probe distinct contracts. No further cuts.

32. **`common/hash.rs`, `common/cache_arena.rs`, `tree/node_hash.rs`, `renderer/gpu/quad.rs`, `ui/state.rs`, `widgets/theme.rs`** — small modules
    - Re-checked: each test pins a distinct contract (Hasher byte-equivalence variants, arena alloc/grow/clear, node-hash determinism, Quad shader layout, StateMap eviction/type-mismatch, Theme token resolution). No parametric merges identified.

33. **`input/keyboard.rs`** — confirmed "leave alone" from §123
    - Re-read: 6 tests cover distinct key-handling contracts (modifier mapping / repeat / IME / focus traversal / text submission / cancel). Not parametric.

Net pass-3 cuts: **−1 test** (#28), **+1 informational gate** (#29), **−1 to plan estimate** (#22 downgrade).

---

## Revised totals (post-pass-3)

| | Before | After (pass 1+2+3) |
|---|---|---|
| Hard cuts | ~417 | ~329 → **~328** (#28 −1, #22 likely +1 vs prior estimate) |
| Fixture extractions | — | F1–F5 (no count change) |
| Gated/conditional | — | #21 (input), #22 (ui text-reshape, downgraded), #29 (prev_frame_captures pair) |

Net: third pass mostly *confirms* the existing plan and tightens estimates rather than uncovering new mass merges. Pass 1 + pass 2 captured the bulk of the available consolidation.

---

## Re-confirmed "leave alone" (pass 2)

Re-read flagged these as candidates and confirmed they should stay separate:

- **`layout/cross_driver_tests/convergence.rs`** — both tests already parametric via `for outer_w in (260..=600).step_by(10)` and `(480..=900).step_by(1)`. Each width probes a distinct flex-shrink solver state. Don't split.
- **`layout/cache/tests.rs`** (15 tests) — re-checked: each pins an orthogonal cache transition (insertion / hash stability / invalidation / eviction / compaction). Merging would obscure which transition broke.
- **`primitives/color.rs::hex_round_trip_stable_over_all_bytes`** — already a 256-iteration property test. Leave.
- **`tests/alloc/**`, `tests/visual/**`** — non-negotiable per CLAUDE.md.

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
- **C12 (gated)** input/tests parametric pass (incl. focus-policy pair from #25): −≈10 — only after re-read
- **C13** fixture extractions F1–F5 into `support::testing` (no test count change, ~120 LOC removed from test bodies)
- **C14** caret-position parametric (#24) + visibility alignment pair (#26): −3
- **C15** corners round-trip labels (#27): no count change, label-add only
- **C16** layout/zstack alignment pair (#28): −1
- **C17 (gated, optional)** ui prev_frame_captures pair (#29): −1 only after side-by-side assertion check

Each commit: a single file (or tightly-related pair), one test pass, fmt + clippy clean. **Run F1–F5 (C13) AFTER all parametric merges** so the helper signature reflects the post-merge call sites, not the pre-merge ones.
