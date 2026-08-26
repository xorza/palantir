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

## Test modules past the split threshold

The rule turns an inline `mod tests` into `mod tests;` once the tests pass 40%
of the file or 150 lines. The crate splits 57 files and keeps 95 inline. These
39 are over the line. Each becomes `foo/{mod.rs, tests.rs}`.

- [ ] `src/shape/stroke_bounds.rs:34` — 54 of 87 lines (62%)
- [ ] `src/widgets/modal.rs:125` — 198 of 322 lines (61%)
- [ ] `src/primitives/approx.rs:145` — 226 of 370 lines (61%)
- [ ] `src/widgets/gpu_view.rs:111` — 160 of 270 lines (59%)
- [ ] `src/primitives/arc.rs:55` — 78 of 132 lines (59%)
- [ ] `src/primitives/corners.rs:138` — 196 of 333 lines (59%)
- [ ] `src/primitives/spacing.rs:132` — 175 of 306 lines (57%)
- [ ] `src/layout/types/limits.rs:24` — 30 of 53 lines (57%)
- [ ] `src/widgets/spinner.rs:140` — 180 of 319 lines (56%)
- [ ] `src/frame_fixture/mod.rs:198` — 245 of 442 lines (55%)
- [ ] `src/widgets/toggle_chrome.rs:98` — 118 of 215 lines (55%)
- [ ] `src/common/time.rs:38` — 45 of 82 lines (55%)
- [ ] `src/scene/record_store/mod.rs:79` — 93 of 171 lines (54%)
- [ ] `src/icons/icon_raster_key.rs:83` — 94 of 176 lines (53%)
- [ ] `src/host/shared.rs:59` — 63 of 121 lines (52%)
- [ ] `src/primitives/mesh.rs:289` — 302 of 590 lines (51%)
- [ ] `src/layout/types/track.rs:126` — 131 of 256 lines (51%)
- [ ] `src/widgets/separator.rs:105` — 108 of 212 lines (51%)
- [ ] `src/widgets/progress_bar.rs:63` — 64 of 126 lines (51%)
- [ ] `src/icons/icon_rasterizer.rs:220` — 221 of 440 lines (50%)
- [ ] `src/primitives/brush/gradient/stops/mod.rs:137` — 137 of 273 lines (50%)
- [ ] `src/primitives/num.rs:111` — 108 of 218 lines (50%)
- [ ] `src/ui/resources.rs:63` — 59 of 121 lines (49%)
- [ ] `src/common/hash.rs:142` — 133 of 274 lines (49%)
- [ ] `src/text/glyphs.rs:155` — 134 of 288 lines (47%)
- [ ] `src/primitives/bezier.rs:108` — 90 of 197 lines (46%)
- [ ] `src/common/expiry_wheel.rs:297` — 247 of 543 lines (45%)
- [ ] `src/scene/seen_ids.rs:275` — 224 of 498 lines (45%)
- [ ] `src/host/winit/gpu.rs:300` — 242 of 541 lines (45%)
- [ ] `src/primitives/widget_id.rs:200` — 160 of 359 lines (45%)
- [ ] `src/text/wrap.rs:190` — 151 of 340 lines (44%)
- [ ] `src/renderer/frontend/composer/text_grid/mod.rs:262` — 207 of 468 lines (44%)
- [ ] `src/bench/cli.rs:218` — 170 of 387 lines (44%)
- [ ] `src/animation/anim_spec.rs:203` — 154 of 356 lines (43%)
- [ ] `src/ui/frame_runtime.rs:233` — 172 of 404 lines (43%)
- [ ] `src/primitives/serde.rs:121` — 87 of 207 lines (42%)
- [ ] `src/renderer/frontend/paint_sink.rs:233` — 160 of 392 lines (41%)
- [ ] `src/scene/tree/paint_anims/mod.rs:359` — 239 of 597 lines (40%)
- [ ] `src/input/shortcut.rs:244` — 155 lines, over the 150-line half of the rule

---

## `impl` blocks away from their type's file

The rule puts every `impl` — inherent, local trait, foreign trait — in the file
that declares the type. Three modules hold impls for types declared elsewhere.

- [ ] `src/primitives/brush/gradient/stops/serde.rs:9`, `:18`, `:34`, `:40` —
      `Serialize` and `Deserialize` for `Stop` and `GradientStops`, both
      declared in `stops/mod.rs`. `src/primitives/serde.rs:8` states the
      opposite policy in its own module doc: "the `Serialize` /
      `Deserialize` impls that drive it live there too". `Corners` and
      `Spacing` follow that; `Stop` and `GradientStops` do not.

- [ ] `src/widgets/theme/serde.rs:15` — `TryFrom<UncheckedTextStyle> for
      TextStyle`. `TextStyle` is declared in `text_style.rs`, and
      `UncheckedTextStyle` (`:7`) is a satellite of it, so both belong there.

- [ ] `src/renderer/backend/schedule/mod.rs:457`, `:462` — `PerGroupBatch` for
      `TextBatch` and `GroupBatch`. Both types are declared in
      `src/renderer/render_buffer/{text_batch,group_batch}.rs`.

---

## Gated modules that are not the last item

The rule makes the test module the last item in the file it reaches into.

- [ ] `src/scene/node/mod.rs:764` — `mod tests;` sits above the production
      `checked_spacing` at `:766`.

- [ ] `src/scene/shapes/mod.rs:7` — `mod tests;` is declared with the submodule
      list at the top of the file, ahead of every `use` and every item.

- [ ] `src/widgets/theme/mod.rs:38` — same shape.

- [ ] `src/layout/grid/mod.rs:160` and `src/layout/stack/mod.rs:413` — `mod
      tests;` precedes `mod test_support`, so the test module is second-last.

---

## Inline `crate::…` paths in expressions

The rule forbids them: add a `use`, and keep a free function
namespace-qualified through its module rather than its name.

- [ ] `src/text/shaper.rs:139` and `:154` — `crate::text::mono::root(…)` and
      `crate::text::mono::resolve(…)`.

- [ ] `src/text/probe/mod.rs:113` and `:133` —
      `crate::text::mono::single_line_caret_x(…)` and
      `crate::text::mono::nearest_byte(…)`.

- [ ] `src/text/cosmic/mod.rs:130` — `crate::text::RENDERED_RUN_KEEP_FRAMES` in
      a `const` initializer.

- [ ] `src/renderer/frontend/composer/geometry.rs:137` —
      `crate::text::TEXT_SCALE_STEP`.

- [ ] `src/renderer/backend/text/encode/cache.rs:272` —
      `crate::text::RENDERED_RUN_KEEP_FRAMES` inside a `const _` assertion.

- [ ] `src/host/winit/mod.rs:166` — the `window` builder spells
      `crate::window::window_config::WindowConfig` in its parameter list.

`src/bench/driver.rs:57`–`:100` holds a further 20, and they read as a
deliberate table: one `use` per bench module would cost 20 imports to name each
path once. Worth a decision either way rather than an accident.

---

## `#[cfg]`-gated `use` in a production file

The rule allows a mid-file gate only where it cannot move — a struct field, or
an inline statement in a production function, which takes a full path instead
of a gated import.

- [ ] `src/primitives/half_simd/mod.rs:136` and `:138` — `use half::f16;` and
      `use half::slice::HalfFloatSliceExt;`, each behind a target gate.
      `src/primitives/corners.rs:96` already shows the alternative:
      `half::f16::from_f32_const(…)` spelled inline.

- [ ] `src/renderer/backend/texture_region.rs:26` — `use
      std::sync::atomic::{AtomicU64, Ordering::Relaxed};` behind `feature =
      "bench"`, at the top of the file.

---

## Release `assert!` on a per-frame path

The rule reserves release `assert!` for public-API misuse **outside** hot
paths, and puts `debug_assert!` on everything per-frame, per-widget, or
per-glyph.

- [ ] `src/scene/record_store/text_store.rs:78` — `TextStore::record` bills one
      comparison per interned string per frame. The doc at `:69` argues the
      case for release, so this is a stated exception rather than an oversight;
      it still contradicts the rule and should be settled one way.

- [ ] `src/primitives/mesh.rs:158` — `Mesh::triangle` bills a `max` of three
      indices and a bounds comparison per triangle. `src/bin/showcase/pages/
      shapes.rs:172` rebuilds a `SIDE²`-vertex mesh every frame through it.

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
