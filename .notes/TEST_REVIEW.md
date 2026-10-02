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
