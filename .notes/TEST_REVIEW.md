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

- [ ] Bare size pins outside `hot_struct_sizes.rs` `PINS`; `MeshVertex = 12` is pinned twice. (judgement)
- [ ] Widgets: the explicit-size trio ×4 (`progress_bar`, `separator`, `spinner`, `slider`); scroll-over-`Block` ×37 (add a `ScrollFixture`); 31 `Option` out-vars; copy-in/copy-out of the bound value in `radio/tests.rs` `frame_rows`.
- [ ] Input and ui: the 100×40 Button scene ×21 (move `response_state.rs:254` `build_button` to `input_state/tests/mod.rs`); `sample_layers` = `sample_pointer_layers`; the blink-text fixture copied in `ui/tests/text.rs:298-323,406-431`; `ui/tests/support.rs` `COLD` equals `SURFACE` and `cold_frame` only wraps `h.frame`; `starting.rs:222` builds a warm harness on `COLD`; `frame_runtime/tests.rs:30-140` spells all 10 fields per row.
- [ ] Layout and damage: 37 identical consecutive `h.frame(x)` pairs that `prime(2, x)` covers; the paragraph string ×12 with three consts; `text_wrap/support.rs` holds one const and a stale doc; two damage drivers (`support::frame` and inline), and `support::frame`'s doc describes behaviour it does not have; 12 raw `paints.slots[..]` reach-ins (add `prev_paint_rows`); 37 `region.iter_rects().collect()` for messages; 4 `compose_sizing` and 9 `stack(axis)` matches (add `Axis::compose_sizing`, and a test-support `stack(axis)` — a public `Panel::stack(axis)` is **(API)**).
- [ ] Animation: `Block::new().id(from_hash(salt)).show(ui)` ×21 while `AnimUi` holds the id; `eviction.rs` re-implements the row count a third time; 70 `map.tick(.., next_frame())`.
