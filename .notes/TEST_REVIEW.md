# Test and harness review

When you address an item, delete it. Delete a heading when its last item is gone.

Scope: every test in `src/`, `tests/alloc`, `tests/visual`, `src/ui/harness`, `src/gpu/test_gpu.rs`,
`src/text/mono.rs`, `src/frame_fixture`, `src/golden`, and the gated `internals` / `test_support` modules.
Line numbers are against `50b34a49`.

Measured on this tree: the unit suite passes 1737 tests in 1.8–3.0 s, and `alloc` passes 47 in 1.4 s.
One test is over the 1 s limit (see "Slow and environment-dependent tests").

An item marked **(API)** adds or changes an exported item. Under AGENTS.md it needs a go-ahead first.
`UiHarness` and the rest of `palantir::internals` are documented as "not a supported API", so
harness items are not marked.

Correction to three of the agent reports: libtest runs each test on a new thread
(`library/test/src/lib.rs:700-704`). Thus the thread-local shaper in `UiHarness::with_text` and in
`tests/visual/harness.rs:49` is private to one test. It does not leak state between tests.

---

## 10. Two input doors, and input state mirrored outside `InputState`

- [ ] 50 `Modifiers { ctrl: true, ..Modifiers::NONE }` literals. Add `Modifiers::CTRL` / `SHIFT` / `ALT`. **(API)**

## 12. Renderer and GPU test rigs are hand-rolled per test

- [ ] `RasterProgram::new(device)` is rebuilt in 13 GPU tests. One program on the shared device could serve them. (judgement)

## 14. Missing coverage for computable logic

- [ ] `anim-derive`'s two error arms (a non-struct input, an unknown `#[animate(..)]` option) have no `compile_fail` test. `trybuild` would cover them at a cost of seconds per run, past the 1 s test budget. (judgement)

## 15. Duplicated fixtures and setup

- [ ] Dock scene in `tests/alloc/fixtures/dock.rs:19-93` and `tests/visual/fixtures/tabs.rs:92-170` (plus the showcase and `widgets/dock/tests.rs:843`). Add a `DockFixture` beside `FrameFixture`.
- [ ] Std-hash helper spelled 7 ways (`color/tests.rs:6`, `size.rs:224`, `rect/tests.rs:7`, `track/tests.rs:8`, `sizing.rs:330`, `brush/tests.rs:20`, `approx/tests.rs:6`, plus `stops/tests.rs` closures). Signed-zero hash agreement is pinned 4 times, once per type; make it one table over every `FloatHash` type.
- [ ] Lane serde is tested three times (`serde/tests.rs`, `corners/tests.rs:161-198`, `spacing/tests.rs:310-343`) with identical `ser`/`de` helpers. Add a generic `ron_round_trip<T>`.
- [ ] HSV and Okhsv suites are parallel copies (`hsv.rs:136,154,161` vs `okhsv/tests.rs:96,133,163`, `color_coords.rs:150` vs `okhsv/tests.rs:52`). Sweep `ColorModel::ALL`.
- [ ] SVG fixtures copied (`icons/icon_table.rs:166-167` = `svg_facts.rs:113-114`; broken `"<svg"` ×3). `IconRef` built twice, `.icon.set` overridden by hand 3 times.
- [ ] Bare size pins outside `hot_struct_sizes.rs` `PINS`; `MeshVertex = 12` is pinned twice. (judgement)
- [ ] Two App-lifecycle counting fixtures (`host/winit/tests.rs:26`, `host/window_driver/tests.rs:~425`) pin one fact.
- [ ] Widgets: the explicit-size trio ×4 (`progress_bar`, `separator`, `spinner`, `slider`); scroll-over-`Block` ×37 (add a `ScrollFixture`); 31 `Option` out-vars; copy-in/copy-out of the bound value in `checkbox` and `radio`; `context_menu/tests/interaction.rs:173` dead parameter; `popup/tests/support.rs:18-20` `BODY_W/BODY_H` unused by the body it records; `record_at_secs` ×3; `ui_at_no_cosmic` is `UiHarness::new` under a second name (44 calls); 15 `response_for(id).rect.expect` that are `h.rect(id)`; 4 `fn harness()` wrappers.
- [ ] Input and ui: the 100×40 Button scene ×21 (move `response_state.rs:254` `build_button` to `input_state/tests/mod.rs`); `sample_layers` = `sample_pointer_layers`; the blink-text fixture copied in `ui/tests/text.rs:298-323,406-431`; `ui/tests/support.rs` `COLD` equals `SURFACE` and `cold_frame` only wraps `h.frame`; `starting.rs:222` builds a warm harness on `COLD`; `frame_runtime/tests.rs:30-140` spells all 10 fields per row.
- [ ] Layout and damage: 37 identical consecutive `h.frame(x)` pairs that `prime(2, x)` covers; the paragraph string ×12 with three consts; `text_wrap/support.rs` holds one const and a stale doc; two damage drivers (`support::frame` and inline), and `support::frame`'s doc describes behaviour it does not have; 12 raw `paints.slots[..]` reach-ins (add `prev_paint_rows`); 37 `region.iter_rects().collect()` for messages; 4 `compose_sizing` and 9 `stack(axis)` matches (add `Axis::compose_sizing`, and a test-support `stack(axis)` — a public `Panel::stack(axis)` is **(API)**).
- [ ] Animation: `Block::new().id(from_hash(salt)).show(ui)` ×21 while `AnimUi` holds the id; `eviction.rs` re-implements the row count a third time; 70 `map.tick(.., next_frame())`.
- [ ] `measure_calls(ui)` (`ui/tests/support.rs:17`) beside `ui.shaper().measure_calls()`.

## 16. Table merges and misplaced tests

- [ ] `wrapstack/tests/bounds.rs:98,142,186,236`: one copied fixture, four tests.
- [ ] Collapsed child per driver (`stack/tests.rs:342`, `visibility/tests.rs:127` (a near-duplicate), `zstack/tests.rs:179`, `canvas/tests.rs:236`, `wrapstack/tests/packing.rs:62`; none for grid): fold into the cross-driver sweep.
- [ ] `stack/tests.rs:586`, `scroll/tests.rs:238` vs `:187,218`; `record_hash` / `record_cascade_static` / `record_subtree_hash` are one function; `text_wrap/wrapping.rs:44-55` vs `:80-88`.
- [ ] `common/expiry_wheel/tests.rs:87`; `brush/tests.rs:146,294,419` and `:230,361,372`, `:427`; `common/time/tests.rs:76`; `translate_scale.rs:251-275`; `half_simd/tests.rs:104,121`; `display/mod.rs:229,243`.
- [ ] `composer/tests/clipping.rs:121-197` cull tests: one sweep over draw kinds. `gradient_atlas/tests/residency.rs:35,92` are covered by `:105`; `upload.rs:14` by `:43`.
- [ ] Misplaced: `encoder/tests/emission.rs:398-418` tests `Align::place_in`; `composer/tests/pruning.rs:271-307,578-590` test `Rect`; `input_state/tests/zoom.rs:62-80` tests `ZoomFactor` and duplicates `zoom_factor.rs`; `host/winit/window.rs:528,600` test only `WindowDriver::drain_window_output`.

## 17. Structure-rule violations

- [ ] Inline `mod tests` over the limits: `scene/shapes/lower.rs:430` (212 lines), `host/winit/window.rs:471` (182 lines), `primitives/color/hsv.rs:93` (45 %), `input/zoom_factor.rs:121` (42 %), `primitives/color/srgb_transfer.rs:145` (41.7 %), `widgets/color_picker/history.rs:73` (40.8 %).
- [ ] `foo.rs` beside `foo/`: `tests/alloc/fixtures.rs` and `tests/visual/fixtures.rs`.
- [ ] `tests/alloc/harness_tests.rs` is an aggregator; its tests belong at the end of `allocator.rs` and in `harness/tests.rs`. `harness/format.rs:121` has `mod tests` without `#[cfg(test)]`.
- [ ] Mid-file gated items: `renderer/frontend/capture.rs:146-181` (`count`, `assert_same_capture`), with an orphan comment at `:19-20`.
- [ ] Function-local re-imports of names already imported: `drag_value/tests/layout.rs:16-22,73-80,225-231`, `keyboard.rs:396-401,445-449,482-484,525-528`, `repainting.rs:598`, `stack/tests.rs:543-544`; `FrameProcessing` imported in 7 functions. `text_edit/tests/mod.rs:1-51` splits imports around helpers.
- [ ] Tuple-returning test helpers: `record_two_frames`, `placement`, `shape_origins`, `recorded`, `settle.rs:48` `warm`, `click.rs:583` `probe`. (judgement: whether the rule binds test code)
- [ ] `TestShape`'s `cfg(test)` fields (`text/request.rs:117-120`) force `#[cfg(test)]` inside a const literal in `text/bench.rs:66-69`. (judgement)
- [ ] Missing `//!` lines on `raster_atlas/tests.rs`, `surface_manager/tests.rs`, `paint_sink/tests.rs`, `text_grid/tests.rs`, `text/encode/cache/tests.rs`.

## 18. Stale and wrong comments in test code

- [ ] Dead paths: `tests/alloc/gates.rs` (now `gates/mod.rs`) in `bench/mod.rs:16`, `frame_fixture/mod.rs:27`, `tests/alloc/harness/offscreen.rs:3`; `tests/alloc/dock.rs` in `frame_fixture/tests.rs:110`; a removed "alloc bench" in `ui/bench.rs:7-8`, `common/counters.rs:23`, `frame_fixture/specimen.rs:219`, `layout/counters.rs:7`, `renderer/gradient_atlas/counters.rs:10`; "~20 small scenes" (there are 34) in `tests/alloc/main.rs:5`, `gates/mod.rs:78`.
- [ ] Tests or modules that do not exist: `canvas/tests.rs:73`, `stretch_semantics.rs:106`, `cross_driver_tests/mod.rs:6` (`crate::support::testing`), `icon_rasterizer/tests.rs:21,137` (`leak_from_svgs`; "three fixtures" for two).
- [ ] Wrong derivations: `stack/tests.rs:162` ("80 between gap"; it is 40 per gap), `encoder/tests/fit.rs:16-18` (wrong crop axis), `encoder/tests/transforms.rs:232-244` (blames the composer in code that never composes), `convergence.rs:154` (font load in a mono test).
- [ ] `scene/shapes/tests.rs:100-104` calls the polyline colour check a `debug_assert`. It is a release `assert_eq!` by design (`shape/polyline.rs:98-102`).
- [ ] `ui/tests/repainting.rs:231-234` narrates history, `:620-627` says the tests build a bare `Ui` (they do not), `:596` is a half-sentence; `input_state/tests/drag.rs:281-285` names `resp` and `post_record`; `damage/tests/support.rs:33-36` puts `one_frame`'s doc on `BLUE`; `color_button/tests.rs:24-30` gives a wrong reason for its press/release frame.
- [ ] Duplicate asserts: `input_state/tests/drag.rs:240`, `:320`.
- [ ] `common/expiry_wheel/mod.rs:288-289`: a doubled empty `///`.
