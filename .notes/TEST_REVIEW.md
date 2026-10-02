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

## 1. Tests read the wrong record pass, or read one-frame edges between frames

The closure runs once per record pass, and pass B sees drained edges (harness rules 3–5). The
harness has `frame_value` and `response_in` for this, but nothing stops a test from reading the
last pass or reading between frames. In each case below, the bug that the test guards against
forces pass B, and pass B erases the evidence.

- [ ] Replace hand-counted passes (`passes += 1`) with `FrameReport::processing`: `combo_box/tests.rs:130`, `popup/tests/placement.rs:319`, `splitter/tests.rs:48,219`, `tooltip/tests.rs:72,205,215`, `scroll/tests/bars/presence.rs:47`, `cold_mount.rs:47`, `ui/tests/frames/settle.rs:59-66`, `frames/passes.rs:58,175,222,242`, `ui/tests/text.rs:689`, `starting.rs:200`. Keep a counter only where the warmup pass matters.
- [ ] Remove the reason for between-frame `response_for` reads. 34 widget sites use `h.ui.response_for` because `ui` is a `pub(crate)` field. `h.rect` and `h.layout_rect` cover geometry. Add `h.transform(id)` for the rest.

## 2. Asserts that cannot fail

Each test below passes when the behaviour it names is broken.

- [ ] `host/window_driver/tests.rs` `output_validity_tests`: the GPU completion that sets `output_valid` (`WindowDriver::gpu_frame`) is stood in for by an assignment. Exercise the real completion path through a GPU-backed frame.
- [ ] Encoder tests filter paint calls by kind in 13 hand-written closures. `PaintCapture::kinds()` now exists; assert sequences with it instead.
- [ ] `renderer/frontend/composer/tests/clipping.rs:121-197`: the cull tests count identical white quads. Use `draw_marked` and `survivor_calls` to name which survived, as the pruning tests now do.
- [ ] `gpu/shader_template.rs` tests keep their own list of shader sources. Share one array with production, so a new shader cannot skip the check.

## 4. Input aimed by literal coordinates, with no check that it lands

The `_on` helpers check that the pointer reaches the widget. Tests mostly bypass them.

- [ ] Counts: widgets 95 literal-position sites against 16 `_on` uses; input 72 (`click_at` 29, `press_at` 43) against 10. 18 widget sites compute `center_of` and then call `click_at`, which skips the occlusion check: `expander/tests.rs:99,119,138,202,241,256`, `tabs/tests.rs:158,179,334,381-382,448,450`, `dock/tests.rs:944,970,1029,1036`, `splitter/tests.rs:166`, `text_edit/tests/context_menu.rs:53`. Swap them to `click_on` / `press_on`.
- [ ] Negative tests where a miss passes: `checkbox/tests.rs:179`, `radio/tests.rs:111`, `drag_value/tests/scrub.rs:223`, `scroll/tests/panning.rs:33-46`, `input_state/tests/click.rs:226,258,454,486`, `keyboard.rs:521`. Give each a control row, and assert `h.hit_at(p) == Some(id)` (or `!=` for a deliberate miss) before the gesture.
- [ ] `context_menu/tests/interaction.rs:77-81` clicks `(90, 80)`, "well inside any plausible row layout". Use `menu_rows()` from `theming.rs:385`.
- [ ] Add `UiHarness::point_in(id, local) -> Vec2` (layout-local to screen, hit-checked) plus `click_in` / `press_in`. It replaces 5 hand-rolled transform copies (`splitter/tests.rs:167-171`, `slider/tests.rs:280-284`, `drag_value/tests/scrub.rs:81-88`, `scroll/tests/panning.rs:259-266`, `scroll/tests/pivot.rs:43-45`) and makes the 19 caret clicks in `text_edit/tests/click.rs` relative to the field.

## 7. The visual suite's tolerance and capture lose information

- [ ] `Tolerance` caps the share of differing pixels but not how far a pixel may differ (`golden/mod.rs:17-36`). The default lets 6 px of `spinner` and 2400 px of `dashboard_hidpi` be completely wrong. The goldens are local and from one adapter, so an unchanged tree diffs at zero. Use WPT-style `{ max_delta, max_pixels }`, default to exact, and require a derivation for each loosening. `format_change.rs:126-129` compares the same format on the same device at 1 %; make it exact.
- [ ] `render*` drops the `FrameReport` (`tests/visual/harness.rs:122`). A repeat render of an identical scene skips and copies the backbuffer, so `scroll.rs:265-284` and the replay half of `main.rs:41-43` test the copy, not encoder replay. Return `Capture { image, paint }` and assert the paint mode. Then pin `damage.rs:153-182` and the overlay `red > 0` checks (`damage.rs:209,353-354,414`) exactly.
- [ ] `fixtures/text.rs:108-120` documents that it cannot catch the bug it is named for, and its row bands are hand constants. Assert unchanged bands bit for bit with `paint == Partial`, or delete it.
- [ ] A failed adapter request is not cached: `OnceLock::get_or_init` stays empty after a panic, so each GPU test runs the 2 s retry again (about 3 minutes for 87 visual tests). Store `OnceLock<Result<ProcessGpu, String>>` (`gpu/test_gpu.rs:98-100`).
- [ ] Golden bookkeeping: `golden/frame_filled_with_stroke.png` is an orphan, and `frame_filled_with_border` (`fixtures/widgets.rs:56`) has no golden here, so it fails now. Nothing reports orphans. `UPDATE_GOLDEN=1` rewrites passing goldens too (`golden/mod.rs:174-179`). A pass leaves an earlier failure's `output/<name>/` in place (`:197-200`). The adapter and driver are not recorded, so a driver update shows as many pixel diffs. Rewrite only missing or failing goldens, delete `output/<name>/` on pass, write an adapter sidecar and compare it on load, and report orphans.
- [ ] Four drivers (`render`, `render_to_format`, `render_after_settle`, `render_with_overlay`) take 4–6 positional args, and 65 of 109 calls pass `1.0, DARK_BG`. `render_after_settle` requires `F: Copy`, and `render_with_overlay` resets the overlay to default. Hold scale, clear, format and overlay on the harness, with `frame(scene) -> Capture` and `prime(n, scene)`. Add `#[derive(Debug)]` to `Harness`.
- [ ] Image comparison has four idioms (`Goldens::assert_matches`, `Tolerance{0,0}.diff`, `shadow.rs:41-63`, raw `==`), and only one writes artifacts. Add `golden::assert_same(label, actual, expected, tol, region)`.
- [ ] 8 hand-rolled per-pixel probes use four bounds (±1, ±2, ±3, ±4); `fixtures.rs:39` `close` returns `bool` and its doc says one step but allows two. Add `assert_px(img, (x, y), want, tol)` with a named constant per derivation.
- [ ] `widgets.rs:500-525` `rounded_clip_survives_surface_resize` discards both images. Assert the corner and centre pixels.
- [ ] `fixtures/widgets.rs` (1226 lines) holds shape and gradient tests; move them to `gradient.rs` and a new `shapes.rs`. `showcase_gradients_tab_matches_golden` (`:192-296`) is a hand copy of the showcase page and already differs from it.
- [ ] No golden renders `FrameFixture`, the one tree that covers every public widget. Add one at small scale. (judgement)
- [ ] `damage.rs:26-35` `SAVE_DAMAGE_PNGS` is a second artifact channel beside `Goldens`.

## 8. The `internals` feature is on in every test build

- [ ] The self dev-dependency (`Cargo.toml:101`, `features = ["internals"]`) merges `internals` into every test target. Seven comments say a plain `cargo test` is GPU-free, and that is false: `lib.rs:209-211`, `text/mod.rs:179-183`, `text/shaper.rs:469-473`, `gpu/icon/tests.rs:4-6`, `gpu/raster_atlas/tests.rs:258-259`, `gpu/text/tests.rs:7-9`, `gpu/text/mod.rs:162-165`. `text/cosmic/mod.rs:921-923` gives the wrong reason for its gate. Choose: accept GPU tests in every run and delete the claims, or add a dedicated `gpu-tests` feature that the dev-dependency does not request. Then the AGENTS.md test line's `--features internals` is redundant or needs a reason.
- [ ] As a result, `all(test, feature = "internals")` equals `test` (`gpu/text/encode/encoder.rs:168`, `gpu/image_store.rs:241`, `gpu/text/encode/cache/mod.rs:384,388`, `text/shaper.rs:528`).
- [ ] GPU tests are gated three ways (file `#![cfg]`, nested `mod gpu`, `all(test, internals)`). The last hides the CPU-only `image_store.rs:257` premultiply test. Use one idiom and move that test out.
- [ ] `internals` modules that hold only test-only items belong in `test_support`: `scene/tree/mod.rs:711-728` (empty under `--features internals`), `text/shaper.rs:396-531` (also a doubled `#[cfg(test)]` at `:399-400`), `ui/mod.rs:1593-1779` (14 cfg'd imports).
- [ ] AGENTS.md has no rule for an in-crate consumer that is itself `internals`-gated (the harness, `OffscreenHost` peepholes), and the code names it both ways (`scene/cascade/mod.rs:361`, `gpu/image_store.rs:227` vs `ui/resources/mod.rs:120`, `gpu/mod.rs:1169`). Add the rule and rename one pair.
- [ ] The facade doc (`lib.rs:196-205`) claims to be the whole list, but `internals` also adds `pub` methods to public types: `Ui::theme_mut`, `TextShaper::test_mono` (no outside caller; make it `pub(crate)`), `OffscreenHost::has_format_pipelines` / `gpu_image_cache_len` (a mid-file gated `impl` at `host/offscreen.rs:337-355` with an inline path at `:342`).
- [ ] Redundant gates on constants in `frame_fixture/mod.rs:59,63,73`, a module that exists only under `internals`.
- [ ] Six behaviour tests run only under `bench`: `layout/cache/bench.rs:540+` (its `cold_frame` builds a warm harness) and `ui/bench.rs:824-929`. Move them to `cfg(test)`.

## 9. Stale text-determinism advice, and tolerances with no reason

- [ ] Harness rule 11 (`ui/harness/mod.rs:96-102`) and the `with_text` doc (`:229-232`) say real shaping loads platform fonts, so tests must "assert relations, not exact widths". `TextShaper::new()` is `FontScope::Bundled`: four faces, deterministic, about 6 µs (`text/font_scope.rs:34-41`). Rewrite both. The same false claim justifies headroom in `tests/alloc/gates/on_gpu.rs:114-117`.
- [ ] The thread-local in `with_text` saves nothing (one shaper per test thread anyway, at 6 µs) and makes two harnesses in one test share a cache. Build a fresh `TextShaper::new()` per harness; `over_shaper` then folds into `from_resources`. Same in `tests/visual/harness.rs:44-50`.
- [ ] Exact values available under mono or bundled faces. Layout: `wrapstack/tests/bounds.rs:128,170,269` (`> 0` → 62), `packing.rs:43,48,123`, `scroll/tests.rs:96` (`> 200` → 436), `stretch_semantics.rs:142` (→ 50), `stack/tests.rs:318` (→ 42), `no_overlap.rs:111-112,201-207`, `cascade/tests/transforms.rs:57-63,188-197,263-270`, `damage/tests/shapes.rs:796,807`, `intrinsic/tests.rs:50,175,182` (`!is_nan()`), and the text-height thresholds in `wrapping.rs:40,57,97,123,178`, `fill_propagation.rs:26,147,151`.
- [ ] `wrapstack/tests/bounds.rs:222`: the bound accepts both 1 and 2 cells per line, and the comment "fits 2" at `:211` ignores the 15 px header. Decide the intended forwarding and pin it.
- [ ] Widgets: `context_menu/tests/interaction.rs:165-170`, `tooltip/tests.rs:89-92,244-246,403-406,549-552`, `tooltip/tests.rs:336-410` (pin the first visible frame against 300 ms), `dock/tests.rs:888-915`, `expander/tests.rs:75-82,145-148,263-266`, `text_edit/tests/blink.rs:242-245,280,313`, `drag_value/tests/edit.rs:69-72`, `bars/presence.rs:98-101`, `gutter.rs:100`, `color_button/tests.rs:48`, `drag_value/tests/layout.rs:63`, `block/tests.rs:13-49` (never checks rounding or fill).
- [ ] Renderer: `composer/tests/batching.rs:241,413-435`, `brushes.rs:153`, `encoder/tests/emission.rs:212-219,445-455`, `encoder/tests/transforms.rs:232-244,282-291`, `fit.rs:42-45`, `paint_sink/tests.rs:220-224`, `gradient_atlas/tests/bake.rs:25-29,53-59,98-102,219-221`, `composer/tests/scaling.rs:254-255`, `gradient_atlas/tests/residency.rs:35-41,92-99`, `gpu/device_requirements.rs:184-186`. `encoder/tests/damage_cull.rs:293-296` tests gaps 2 and 10 around a margin of 3; test 3 and 4.
- [ ] Input, ui, text: `ui/tests/text.rs:78,114,515`, `text/tests/geometry.rs:79,259-273,315,317,348,363`, `text/tests/wrap.rs:303,314,325`, `text/glyphs/tests.rs:82`, `ui/tests/repainting.rs:65,287,671` (`:671` should assert `DoubleLayout`), `input_state/tests/drag.rs:128-133,638,643`, `keyboard.rs:498`, `paint_anim.rs:53`, `zoom.rs:48,90`, `scroll_routing.rs:125,282`, `frames/passes.rs` dt, `frame_runtime/tests.rs:192,208`.
- [ ] Animation and primitives: seven settle loops assert only `is_some()` (`duration.rs:196`, `spring.rs:47,334,357,413`, `snap.rs:187`, `ui_animate.rs:55`); FAST at 16 ms settles at `Some(6)`. Also `duration.rs:53,131,172,222`, `ui_animate.rs:105,194`, `spring.rs:216,250`, `primitives/bezier/tests.rs:38` (the comment's 38.5 is wrong; the extremum is `50/√3 ≈ 28.87`), `brush/tests.rs:396-398`, `color/tests.rs:187`, `diagnostics/gpu_pass_stats.rs:192-216`. `ui_animate.rs:83,115` use `assert!(x == 0)`, which hides the value.
- [ ] Tolerances with no stated reason: 112 inline in widgets with six epsilons, 32 × 0.5 px in layout, 26 hand-written rect checks in the renderer. Add `assert_close(actual, expected, tol, why)` and `assert_rect_near(.., why)` with a mandatory reason, and one `LAYOUT_SNAP_TOLERANCE` only where pixel snap is on. Use `assert_eq!` everywhere else.
- [ ] `text/mono.rs:66,89,118-126` measures `text.len()` (bytes) while its docs say characters, so non-ASCII text measures wide.

## 10. Two input doors, and input state mirrored outside `InputState`

- [ ] Seven sites write `input_mut().focused =` directly (`keyboard.rs:356`, `input_delta.rs:143,151,185,191,237`, `repainting.rs:602`), plus `popup/tests/dismissal.rs:184,196`. Use `h.set_focus` / `h.clear_focus`, with one named made-up id.
- [ ] The key-to-text match has 8 copies (`ui/harness/mod.rs:614-617`, `input/key_class.rs:216`, `input/shortcut/tests.rs:11`, `drag_value/tests/edit.rs:283-286`, `text_edit/tests/word_nav.rs:94-97`, `text_edit/tests/mod.rs:88-91,119-122,139-142`), and 17 + 6 full `KeyPress` / `KeyDown` literals. Add a crate-private `KeyText::of_key`, plus test-support `KeyPress::with(key, mods)`. Expose the word-nav modifier set from `text_edit/input_pass.rs:322` so tests cannot drift from it.
- [ ] 24 raw `state.on_input(ev, &cascade, Duration::ZERO)` calls with 9 throwaway `Cascade::default()` (`keyboard.rs`, `scroll.rs`, `zoom.rs`). Add a test-support `InputState::feed(event)`.
- [ ] 50 `Modifiers { ctrl: true, ..Modifiers::NONE }` literals. Add `Modifiers::CTRL` / `SHIFT` / `ALT`. **(API)**

## 11. Harness API: hazards, gaps, and dead surface

- [ ] `UiHarness::pointer_pos` and `escape_pressed` (`ui/harness/mod.rs:726,731`) forward to record-time APIs that register wake watches, which contradicts their "no protocol hazard" doc. Neither has a caller. Delete them.
- [ ] No callers: `hover_within` (`:721`), `right_click_on` (`:527`), the `pixel_snap` builder (`:282`), `try_frame_value`. `collisions` (`:745`) is used only by the harness's own test and returns a tuple vec. The doc's promise that dead tier-1 methods get reported (`:145-154`) cannot hold for `pub` items. Replace it with "prune at zero callers".
- [ ] `user_scale` has two homes. Production derives it from the setting (`host/window_driver/mod.rs:392`); the harness stamps `self.display` and never reads it. A `ui.set_user_scale` inside a frame has no effect, and `set_display` leaves `Ui::user_scale()` disagreeing. Derive it in `drive` as the driver does.
- [ ] `UiHarness::cold(s).scale(2.0)` is silently warm: every builder calls `mark_warm`. Re-warm only when already warm. `try_frame_value` keeps the warmup pass's value on a cold harness; skip the warmup.
- [ ] `advance_frames` returns nothing, so every animation settle loop hand-rolls `now += 16ms; h.at(now)` (`spring.rs:317`, `snap.rs:171`, `ui_animate.rs:41`, `duration.rs`). Add `frames_until_idle(max, dt, record) -> Option<u32>`.
- [ ] `node_for_widget_id` is Main-only, so 10 tests hand-roll an `id → node` scan (`color_button/tests.rs:99-104`, `modal/tests.rs:32-37`, `tooltip/tests.rs:124-129`, `text_edit/tests/context_menu.rs:38-43`, `context_menu/tests/theming.rs:373-381,413-418`, `drag_value/tests/layout.rs:165-171`, `scroll/tests/bars/support.rs:26-33`, `presence.rs:280-285`, `lifecycle.rs:55-60`, `ui/tests/ids.rs:205-213`) and 4 use magic `roots[i]` (`popup/tests/placement.rs:66,109,154`, `modal/tests.rs:57`). `Cascade::endpoint(id)` already answers this. Add `node_of(id)` and `child_rects(id)` for any layer.
- [ ] Six ways to read an arranged rect: `ui.arranged_rect` (43), `layout_rect().expect` (24), `main_child_rects` (38), wrapstack's local `rect_of` (39), raw `.rect[idx]` (16 + 19), and `response_for(id).rect`. The last is the visible rect, which the harness doc forbids for layout, yet `cross_driver_tests/arrange_axis.rs:127` and `stretch_semantics.rs` (×10) use it. Promote `rect_of` to `UiHarness::arranged(key) -> Rect` and move the rest to it.
- [ ] 61 sites smuggle a `NodeId` out of the record closure; 10 of them are never read (`canvas/tests.rs:273`, `fill_propagation.rs:214,276`, `grid/tests/degenerate.rs:18,121`, `grid/tests/hug_grid.rs:16,53,92`, `grid/tests/tracks.rs:418`, `stack/tests.rs:285`). 11 of 29 `under_outer` callers discard its `NodeId`. Make `under_outer` generic over its return, like `frame_value`.
- [ ] Intrinsic queries take a 5-arg call plus an interning dance at 14 sites (`intrinsic/tests.rs`, `text_wrap/wrapping.rs`, `hug_cols.rs`), and the NaN-fill cache reset is copied 4 times. Add `UiHarness::intrinsic(node, axis, req)` and `LayoutEngine::forget_intrinsics`.
- [ ] Stale harness doc lines: `:219-221` vs `:151` (lint allows), `:376` "No caller yet" and `:397` "No caller outside this module" (both have callers), `:136-140` "the one knob" (there are two).
- [ ] Mid-file gates in the harness itself: `:173-176` (cfg'd import at the top) and `:847-860`. Move them to an end-of-file tier module.

## 12. Renderer and GPU test rigs are hand-rolled per test

- [ ] Composer tests never compose with pixel snapping on, but production does. `composer/tests/support.rs:87-95` `params()` sets `pixel_snap: false` for about 112 calls, and the snap branch of `Rect::scaled_by` (`primitives/rect/mod.rs:407-410`) has no unit test anywhere. Build from `Display::from_physical`, opt out only where needed, and add a snap on/off sweep.
- [ ] 8 hand-written compose loops (`brushes.rs:44-55,141-150,184-193`, `curves.rs:108-117`, `batching.rs:397-406`, `pruning.rs:564-575,819-844`, `clipping.rs:47-61`); `pruning.rs:570` makes a new `RenderBuffer` each frame, so buffer reuse is never tested. The texture cap is `16_384` twice in `support.rs` against `TEST_MAX_TEXTURE_DIM = 8192`. Add a `ComposeRig`.
- [ ] 22 hand-built `DrawQuadPayload`, 9 `DrawImagePayload`, 6 gpu-view draws in `brushes.rs`. Add a quad builder and `gpu_view(b, rect, handle)`. Drop the `rect()` and `render_buffer()` aliases.
- [ ] No-op `GpuPaint` defined 4 times (`composer/tests/support.rs:120-129`, `paint_sink/tests.rs:111-118`, `renderer/gpu_paint/gpu_views.rs:129-138`, `widgets/gpu_view/tests.rs:23`). Add `GpuPaintRef::noop()` in `test_support`.
- [ ] `gpu/tests`: 51 `DrawGroup` literals, 20 `collect(.., MaskPlan::default(), false)`, `buf_with_mesh_anchors` equals `buf_with_image_anchors`. Add `group()`, `plain_steps()`, `buf_with_tier_anchors()`, and fold `text_batches.rs` into one table.
- [ ] `gpu/text/tests.rs`: `TestGpu` re-clones `lease.queue`; the 4-line setup is copied 6 times; `make_inner_run` takes 9 args with fixed `viewport` and `scale`; `run_one_frame` takes 6 and handles one batch, so two tests rebuild the submit by hand. Add a `TextRig`. `:682-706` derives its frames from 512 and 120, but the constants are 120 and 30; loop to `unallocated_dies_at(0) + 1`.
- [ ] `gradient_atlas/tests/support.rs:20-36` `distinct_grad(f32)` is distinct only by hash luck. Take `i: u32` and write its bytes. Add `fill_rows()` for the 10 copied loops; drop `register_for`.
- [ ] Encoder suite: `rect_with_fill()` for the colour lookups in `visibility.rs:88-115` and `transforms.rs:218-230,271-277`. Move `as_shadow` beside `as_rect`. Merge `spun_polyline_*` and `spun_arc_*`.
- [ ] The render-target descriptor is copied 3 times (`tests/alloc/harness/offscreen.rs:36`, `tests/visual/harness.rs:194`, `gpu/bench_gpu.rs:92`) and the `poll(Wait)` drain 4 times. Add `lease.target(..)` and `lease.wait()`.
- [ ] `RasterProgram::new(device)` is rebuilt in 13 GPU tests. One program on the shared device could serve them. (judgement)

## 13. Slow and environment-dependent tests

- [ ] `host/winit/input/tests.rs:91` branches on `PLATFORM`, so Linux never checks the macOS Cmd→ctrl table. Let `normalize_modifiers` take the `Platform`. (judgement)
- [ ] Multi-click tests depend on the frozen harness clock without saying so (`input_state/tests/click.rs:356,454,486,580`), and `:383-384` says "real time … 400ms window" (it is a frozen clock and 500 ms). Advance a stated in-window gap.
- [ ] `ui/tests/frames/settle.rs:59-66` calls `h.at(16ms)` every frame, which parks the clock while the code reads as a 16 ms cadence.

## 14. Missing coverage for computable logic

- [ ] Widgets: `Switch` has no click, disabled or knob-position test; `ColorSwatch` has no test; no test calls `Modal::backdrop`, `Separator::thickness`, `Spinner::thickness`, `Scroll::zoomable_with`, `ComboBox::button_style`, `ColorPicker::history`, `ColorButton::history`, `GpuView::repaint`, `Popup::default_background`, `MenuItem::shortcut_hint`. `TextEdit::max_chars` is not checked through `show` or paste. Extend `toggle_chrome/tests.rs` into a click / disabled / `clicked()==changed` sweep over Checkbox, RadioButton and Switch.
- [ ] Layout: no driver is tested with zero children; `grid/tests/degenerate.rs:16` builds zero rows but not zero cols; `arrange_axis` lacks `Fixed` × `min > fixed`, `Hug` × `max < content`, and `min == max` rows, and its margin sweep never affects an asserted value.
- [ ] Host and window: `WindowRequests::drain` has no direct test, and `ui/tests/frames/window_output.rs:115,128` re-implement its formula. `WindowDirectory::add` / `remove` panics, `sanitize_system_scale`, the `scale_factor_is_valid` boundary, `UserScale` stepping near a rung, and the `raster_eq` axes (`physical`, `pixel_snap`, the `refresh_millihertz` exclusion) have no test. `winit/tests.rs:53` never calls `.vsync()` or `.fonts()`.
- [ ] `runtime.rs:196` `schedule`, `native.rs:125` `position_on_monitor`, and the scale-change resync in `winit/mod.rs` are pure folds behind `ActiveEventLoop`. Extract them as functions over slices and table-test them.
- [ ] Diagnostics: `GpuSegment`'s `Display` and `clear_kinds` keeping the other fields.
- [ ] `anim-derive` has no tests: `#[animate(skip)]`, the error arms, an all-snap struct, generics, `zero()` of a snap field. Add a `cfg(test)` probe struct with hand-computed results. `compile_fail` coverage would need `trybuild`, a new dependency.
- [ ] `primitives/rect/aabb.rs` `Aabb::of` / `of_iter` NaN contract; `approx::share_of` and `vec2_approx_eq`.
- [ ] Mesh cache: whether `clear` and `append` invalidate `cached_hash` is unpinned; a `mutation × {hash, bbox}` table in `mesh/tests.rs` would show it.

## 15. Duplicated fixtures and setup

- [ ] `UiResources::isolated_mono()` written out by hand 15 times (`host/window_driver/tests.rs:145,172,188,218,323,388,413,516,549,658,684`, `host/winit/window.rs:538,607`, `ui/tests/repainting.rs:447`, `ui/tests/text.rs:725`). `UiResources::new(TextShaper::new(), ..)` 5 times; add `isolated_text()`. `Frontend::new(8192, ..)` 3 times in `window_driver/tests.rs`.
- [ ] Dock scene in `tests/alloc/fixtures/dock.rs:19-93` and `tests/visual/fixtures/tabs.rs:92-170` (plus the showcase and `widgets/dock/tests.rs:843`). Add a `DockFixture` beside `FrameFixture`.
- [ ] `Background { fill, ..Default::default() }` written 64 times (49 scene/layout, 15 encoder) while `Background::fill` has 0 uses there. The "id + fixed size + fill" leaf is re-declared 7+ times (`wrapstack/tests/support.rs:24`, `damage/tests/support.rs:39`, `cache/tests/reuse.rs:70`, `cascade/tests/incremental.rs:65`, `tree/tests/node_hash.rs:80`, `tree/tests/subtree_hash.rs:47`, `damage/tests/tree.rs` ×5). 60 `.style(&TextStyle::default().with_font_size(x))` beside `Text::font_size`.
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

- [ ] `cross_driver_tests/arrange_axis.rs:131,152,173`: three identical triple loops; make one `CASES` table.
- [ ] `wrapstack/tests/bounds.rs:98,142,186,236`: one copied fixture, four tests.
- [ ] Collapsed child per driver (`stack/tests.rs:342`, `visibility/tests.rs:127` (a near-duplicate), `zstack/tests.rs:179`, `canvas/tests.rs:236`, `wrapstack/tests/packing.rs:62`; none for grid): fold into the cross-driver sweep.
- [ ] `stack/tests.rs:586`, `scroll/tests.rs:238` vs `:187,218`; `record_hash` / `record_cascade_static` / `record_subtree_hash` are one function; `text_wrap/wrapping.rs:44-55` vs `:80-88`.
- [ ] `mesh/tests.rs` nine cache tests; `common/expiry_wheel/tests.rs:87`; `brush/tests.rs:146,294,419` and `:230,361,372`, `:427`; `common/time/tests.rs:76`; `translate_scale.rs:251-275`; `half_simd/tests.rs:104,121`; `display/mod.rs:229,243`.
- [ ] `composer/tests/clipping.rs:121-197` cull tests: one sweep over draw kinds. `gradient_atlas/tests/residency.rs:35,92` are covered by `:105`; `upload.rs:14` by `:43`.
- [ ] Misplaced: `encoder/tests/emission.rs:398-418` tests `Align::place_in`; `composer/tests/pruning.rs:271-307,578-590` test `Rect`; `input_state/tests/zoom.rs:62-80` tests `ZoomFactor` and duplicates `zoom_factor.rs`; `host/winit/window.rs:528,600` test only `WindowDriver::drain_window_output`.

## 17. Structure-rule violations

- [ ] Inline `mod tests` over the limits: `scene/shapes/lower.rs:430` (212 lines), `host/winit/window.rs:471` (182 lines), `gpu/shader_template.rs` (59 %), `primitives/color/hsv.rs:93` (45 %), `input/zoom_factor.rs:121` (42 %), `primitives/color/srgb_transfer.rs:145` (41.7 %), `widgets/color_picker/history.rs:73` (40.8 %).
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
- [ ] Wrong derivations: `stack/tests.rs:162` ("80 between gap"; it is 40 per gap), `encoder/tests/fit.rs:16-18` (wrong crop axis), `encoder/tests/transforms.rs:232-244` (blames the composer in code that never composes), `display/user_scale.rs:156` (says 1.0, asserts 1.1), `convergence.rs:154` (font load in a mono test).
- [ ] `scene/shapes/tests.rs:100-104` calls the polyline colour check a `debug_assert`. It is a release `assert_eq!` by design (`shape/polyline.rs:98-102`).
- [ ] `ui/tests/repainting.rs:231-234` narrates history, `:620-627` says the tests build a bare `Ui` (they do not), `:596` is a half-sentence; `input_state/tests/drag.rs:281-285` names `resp` and `post_record`; `damage/tests/support.rs:33-36` puts `one_frame`'s doc on `BLUE`; `color_button/tests.rs:24-30` gives a wrong reason for its press/release frame.
- [ ] Duplicate asserts: `input_state/tests/drag.rs:240`, `:320`.
- [ ] `common/expiry_wheel/mod.rs:288-289`: a doubled empty `///`.
