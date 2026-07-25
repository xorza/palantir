# Review — `paint-session` vs `master`

Range: `aperture` `master...paint-session` (`495ba7ba..210b4866`), one commit —
*Replace Command Buffer With Paint Sink Pipeline*. 29 files, +1856 / −2015.

The change deletes `renderer/frontend/cmd_buffer/` (a packed `Vec<u32>`
descriptor + arena command stream) and replaces it with a `PaintSink` trait the
encoder paints through. `ComposeSession` is the production sink; `RecordedPaint`
(`record_sink.rs`, `cfg(test | internals)`) is the test/bench sink.

## Verdict

The direction is right and the execution is mostly clean. Fusing encode and
compose removes a serialization layer that existed only to be immediately
decoded, and nothing in the branch trades that for new steady-state allocation.
No rendering regression reproduced; the full matrix is green.

Two API contracts are worth tightening before treating this as the durable
frontend boundary (findings 1 and 2), and three regressions are *silent* — they
degrade profiling, benchmarking, and test strictness without failing anything
(findings 3–5). Those are the ones that get expensive if they sit.

## Verification state

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets --features internals -- -D warnings` | clean |
| `cargo check` (production, no `internals`) | clean |
| `cargo test --lib` | 1188 passed, 0 failed |
| `cargo test --test alloc --features internals` | 24 passed |
| `cargo test --test visual --features internals` | 71 passed |

Caveat on the visual suite: goldens are gitignored and per-machine. They existed
before the run and a re-run emitted no `NEW GOLDEN` lines, so the comparisons
were real — but whether they were baselined before or after this change isn't
determinable from the branch, so treat it as a determinism check, not a
master-vs-branch pixel diff.

## What the change gets right

- **Deletes the hand-rolled packing layer wholesale.** 4-bit kind tags, 28-bit
  word offsets, `MAX_DATA_WORD_OFFSET` overflow asserts, `pod_read_unaligned`
  for the one payload with align > 4 — ~700 lines whose entire job was surviving
  a round trip through `Vec<u32>`. Fused, the round trip doesn't exist.
- **Drops two retained per-frame `Vec`s** (`descriptors`, `data`) plus the
  `gpu_view_paints` side channel, each sized to the whole frame's command
  stream. `Encoder` now retains only the gradient resolver.
- **Breaks up a 760-line `match`.** `Composer::compose` was one function with
  thirteen inline arms (master `composer/mod.rs:441–1204`); each is now a named
  method on `ComposeSession`.
- **Frees payloads from `Pod`.** `LineCap` / `LineJoin` / `ColorMode` ride as
  typed enums instead of `u8` plus three panicking `from_u8` decoders, all three
  now deleted.
- **The composer keeps its allocation-owning stacks and scratch**, so removing
  the arena doesn't relocate the cost. Payload sizes stay pinned in
  `hot_structs!`; the alloc audits pass.
- **`RecordedPaint` is a better test artifact than the arena it replaces** — a
  `Vec<PaintCall>` you can match on and count beats byte-comparing two opaque
  `Vec<u32>`s. (With one caveat: finding 4.)

---

## Findings

### 1. Dropping a `ComposeSession` silently leaves a half-built `RenderBuffer`

`composer/mod.rs:444–465` (`begin`), `:716–732` (`finish`),
`frontend/mod.rs:96–104`.

`Composer::compose` was a single operation that always closed the trailing text
batch and draw group before returning. That invariant is now split across `begin`
and a manually-required `finish`. `ComposeSession` is not `#[must_use]`, has no
`Drop` finalization, and `finish` is an ordinary consuming method — so a call
site can push quads and text, drop the session, and get a buffer that *looks*
populated but whose trailing group and batch were never emitted. The backend then
can't schedule the unflushed rows.

Every current call site finishes correctly, so this is a footgun, not an observed
failure. Ranked first because it is the one finding where a future edit lands as
silently-missing pixels.

Fix, in preference order:

1. Make the normal API scoped — `Composer::compose_with(display, payloads, out, |sink| …)`
   finalizing after the closure returns, with `begin` kept private if still
   useful internally. This restores the old "one operation, one invariant"
   shape without giving up the per-op method split.
2. Failing that, `#[must_use = "a ComposeSession must be finished — dropping it
   leaves the trailing batch and group unflushed"]`.

### 2. The "single canonical gate" is now convention, not structure — and the naming hides the boundary

`paint_sink.rs:20–22` claims the trait is *"the single canonical correctness
gate … a gate can't drift between the two paths because there is only one copy of
it."* The single-copy half holds. The unbypassable half no longer does.

Previously `RenderCmdBuffer`'s raw writers (`record_start`, `write_pod`) were
private module functions, so `draw_*` was the only door. Now both halves are
`pub(crate)` trait methods, so `sink.rect(payload)` compiles anywhere in the crate
and skips the gate. `RecordedPaint::replay` already does exactly that.

Two concrete consequences:

**a. `replay`'s doc states the opposite of what it does.** `record_sink.rs:79–82`
says calls *"re-enter through the provided half, so `sink` re-applies its own
no-op gates."* The arms below call `sink.rect`, `sink.text`, `sink.polyline` — the
required half, no gate. Behaviour is fine (a recorded call passed the gate at
record time); the doc is the problem, because it is exactly what a reader
consults before reusing `replay` for un-vetted input.

**b. You cannot tell from a call site which half you're in.**

| gated (provided) | ungated (required) |
|---|---|
| `push_clip`, `push_clip_rounded` | `clip` |
| `draw_rect`, `draw_rect_window` | `rect` |
| `draw_text`, `draw_mesh`, `draw_curve`, … | `text`, `mesh`, `curve`, … |
| — | `pop_clip`, `push_transform`, `pop_transform` |

The last row is what breaks the rule: three required methods are *also* the
intended call-site API, so "required means don't call it" isn't statable.

Fix: rename the raw draw half `raw_*` (or `emit_*`) and leave the clip/transform
ops as they are, since they have no gated counterpart — a skipped gate then reads
as `raw_` at the call site. Correct `replay`'s doc to say it re-enters *below* the
gate deliberately, and why that's sound. If more sinks are expected later, a
private raw supertrait plus the gated wrapper is the structural version.

### 3. Profiling lost the encode/compose split

master had three spans: `Frontend::build`, `Encoder::encode`, and
`Composer::compose` (master `composer/mod.rs:440`). The branch has two —
`frontend/mod.rs:96` and `encoder/mod.rs:153` — and because sink calls compose
inline, `Encoder::encode` now transitively contains **all** composer work.

So the profile now reads: `build` ≈ `encode` ≈ 100%, compose invisible. Anyone
comparing a before/after capture will conclude the encoder regressed massively.
Given the branch's whole premise is a pipeline restructure whose payoff is
measured in frame time, this is the instrumentation you most need to be honest.

Fix: rename the span to `encode_compose`, or push per-stage spans down to where
they're still separable.

### 4. `assert_same_paint` compares floats with `==`; the check it replaced was bitwise

master's `assert_same_stream` (`cmd_buffer/mod.rs:477–488`) did
`assert_eq!(left.data, right.data)` on `Vec<u32>` — **bitwise**. The replacement
(`record_sink.rs:168–184`) compares `PaintCall`s through derived `PartialEq` on
payloads full of `f32`. That differs in both directions:

- **More brittle:** `NaN != NaN`. The codebase deliberately lets NaN stroke
  widths through — `paint_sink.rs:369–373`: *"NaN width is NOT a `ShapeStroke`
  noop … it passes through on both paths identically."* So
  `encoded_buffer_stable_across_cache_hit_boundary`
  (`layout/cache/integration_tests.rs:451`) can fail on two byte-identical
  frames. Latent — no current fixture produces NaN — but it turns a documented
  pass-through into a spurious red.
- **Weaker:** `-0.0 == 0.0`. A sign-of-zero drift between cold and warm frames
  now passes where the byte comparison caught it.

Fix: compare bitwise in `assert_same_paint` (per-payload `to_bits()` helper, or
hash the byte view) rather than leaning on derived `PartialEq`.

### 5. The composer benchmark no longer measures compose alone

`bench/renderer/frontend/composer.rs:55–60` times
`ComposeSession::replay_from`, i.e. a `Vec<PaintCall>` walk plus an enum match
plus a second dispatch — none of which production executes. `record_sink.rs:5–8`
still advertises replay as *"what lets the compose bench measure compose alone."*

It remains valid for relative composer-algorithm regressions, but the absolute
number now includes test-only overhead. Rename the group so the replay cost is
explicit, and lean on the full-frame / `Frontend::build` bench for the production
figure.

### 6. Image/GPU-view identity is stored twice and the two can disagree

`payload.rs:255–279, 324`; `paint_sink.rs:75–77, 221–230`;
`encoder/mod.rs:529–541`; `composer/mod.rs:1060–1110`.

Whether a draw is a GpuView is represented in two independent places: the private
`gpu_view: bool` inside `DrawImagePayload`, and the `Option<&GpuPaintRef>`
argument to `PaintSink::image`. The raw trait surface and `PaintCall::Image` can
represent both invalid pairings — a GpuView payload with `None` (bypasses the
null-handle rule, schedules no target) and an ordinary-image payload with `Some`
(schedules an off-screen target for a registered image). The provided wrappers
construct consistent pairs today; nothing enforces it.

The boolean is also **unreachable in practice**. Its only reader is
`is_noop`: `handle.0 == 0 && !self.gpu_view`. `TextureIdSource` documents that ids
start at 1 and `TextureId(0)` is never handed out (`texture_id.rs:21–22, 33`), so
a GpuView's handle is never 0 and `!gpu_view` cannot change the outcome. For that
dead guard the branch pays one private field on an otherwise `pub(crate)` struct,
which forces both `image()` and `gpu_view()` constructors plus a six-line doc
paragraph explaining the privacy.

Two smaller things ride along here:

- **An avoidable `Rc` clone per GpuView draw.** `encoder/mod.rs:540` clones
  `view.paint` to pass by value into `draw_gpu_view`, which passes `Some(&paint)`
  down; `composer/mod.rs:1108` clones it *again* into `frame_targets` and the
  encoder's temporary is dropped immediately. `view` is a `&GpuViewEntry`, so
  `draw_gpu_view` can just take `&GpuPaintRef`. Free.
- **Lost coverage.** `gpu_view_records_payload_and_paint_atomically` (deleted with
  `cmd_buffer/tests.rs`) asserted both that a zero-extent GpuView rect emits
  nothing and that a live one records payload + paint together. The atomicity half
  is now structural (`PaintCall::Image { payload, paint }` is one variant); the
  **noop half has no replacement** — the surviving `compose_gpu_view_*` tests
  (`composer/tests.rs:1667`, `:1714`) only exercise live views.

Fix: separate raw `image` / `gpu_view` sink methods (or one typed source enum),
drop the payload boolean, pass `&GpuPaintRef`, and restore the gate test against
`RecordedPaint` in `paint_sink.rs`'s test mod — it already holds the sibling rect
and triangle gate tests.

### 7. Seventeen stale command-buffer references, including a self-contradicting `AGENTS.md`

The rename left the deleted abstraction's name in comments describing code that
no longer has one. This matters more than usual here because the same commit
*did* update the architecture doc to say there is no intermediate stream — so the
tree now documents both stories.

Actively misleading:

- **`AGENTS.md:47`** — *"The retained `Encoder` owns both its command buffer and a
  dense resolver…"*, directly contradicting `AGENTS.md:35` as rewritten by this
  same commit.
- **`encoder/mod.rs:44`** — *"Retained encoder state and its command output."* The
  `Encoder` has no output.
- **`encoder/mod.rs:645–648`** — *"Clip culling intentionally does NOT live in the
  encoder: cmd shape would depend on screen position, complicating downstream
  walks."* There is no downstream walk. The decision may still be right (the
  composer holds the scissor) but this no longer supports it.
- **`encoder/mod.rs:654–656`** — *"wastes two cmd slots"*. No slots; two virtual
  calls and a stack push/pop.
- **`composer/mod.rs:1095`** — *"from the cmd buffer's side channel"*. The callback
  rides the call; there is no side channel.
- **`payload.rs:106`** — *"`Pod` invariant: `repr(C)` + no padding"*, contradicting
  the module doc four lines of code above it (`payload.rs:5–9`).
- **`record_store.rs:117–119`** — *"spans recorded on tree shape records and
  cmd-buffer payloads."* Notable because this commit updated the same file's
  header doc and missed this one 110 lines down.

Plain stale: `encoder/mod.rs:494`, `:749`, `:759`; `payload.rs:32`, `:168`,
`:206`, `:217`; `paint_sink.rs:31`; `text/key.rs:198–199`;
`widgets/scroll/tests/mod.rs:1083`.

A case-insensitive sweep for `cmd buffer`, `cmd-buffer`, `command buffer`,
`command stream`, and `per-cmd` catches all of them.

### 8. The display preamble is now duplicated across seven methods

The one place the new shape reads worse than the old. master hoisted `scale` /
`snap` / `viewport_phys` once at the top of `compose` (master
`composer/mod.rs:448–450`). Splitting into trait methods re-destructures them in
each, and four repeat the same `apply_rect` → `scaled_by` → `urect_from_phys` →
cull sequence verbatim: `composer/mod.rs:820–828` (`rect`), `:922–928` (`shadow`),
`:1060–1066` (`image`), `:1444–1459` (`text`).

`curve` and `arc` go further and open with:

```rust
let scale = self.display.scale_factor;
let display = self.display;
```

where the first is derivable from the second, then pass both onward.

One small `ComposeSession` helper returning a named struct (`{ world, phys, urect }`
— no tuple returns) collapses all four. Worth doing because these rects feed cull
*and* overlap tracking, where a false negative reorders paint.

### 9. `ComposeSession::replay_from` is a bench-only method on a production type, outside `test_support`

`composer/mod.rs:727–731`, plus the `cfg`-gated import of the test-only
`record_sink` into the production composer at `composer/mod.rs:16–17`. The repo
convention — and the example two files over, `Frontend::for_test` at
`frontend/mod.rs:107–123` — is a gated `test_support` mod.

It's also barely used: the bench calls it; every composer test open-codes the
identical two lines instead (`recorded.replay(&mut session); session.finish();`,
`composer/tests.rs:94–96`). Either move it into `test_support` and use it in both
places, or delete it and let the bench open-code the two lines like the tests do.

### 10. `payload.rs`'s module doc narrates history instead of the present

`payload.rs:5–9`: *"they used to be `bytemuck::Pod` so a packed command arena
could store them, and carried `#[repr(C)]` plus injected trailing padding to
satisfy that."* The comment policy is non-obvious *why*, not history. Two
sentences — "plain value types; layout is unconstrained, so fields can be ordinary
enums rather than `u8` newtypes" — carry everything a reader needs, and the removed
constraint is visible in the derives.

### 11. Three production `PartialEq` impls exist to serve a `cfg(test)` derive

`PaintCall` derives `PartialEq`, which pulled it into `GpuPaintRef`
(`gpu_view.rs:120–127`), `ShapedTextRef` (`text/key.rs:204`), and `TextSource`
(`interned_str.rs:57`). Each is defensible and `GpuPaintRef`'s is documented, but
note what `GpuPaintRef: PartialEq` now offers every caller: an equality meaning
"same handle", easy to misread as "same painter behaviour". Worth knowing a test
module's derive is what created it.

---

## Minor

- **Two sources for one value.** `Composer::begin` resets scratch from
  `display.physical` (`composer/mod.rs:452`); `discard_composed` resets from
  `out.viewport_phys` (`:476`). `RenderBuffer::start_frame` makes them equal, so
  it's correct today — but these are precisely the pair whose stated purpose
  (`:479–485`) is that a new scratch field resets identically on both paths.
- **`enter_higher_kind(tier, scissor, out)`** (`composer/mod.rs:340`) — the
  parameter is the draw's bounds, not a scissor. Callers pass `mesh_urect`,
  `image_urect`, `bbox_scissor`.
- **`text` open-codes its reject** (`composer/mod.rs:1464`) where every other draw
  calls `cull_bounds`, whose doc (`:315–316`) says centralising it "keeps each
  handler from growing its own variant". Text genuinely needs the clamped rect
  that `cull_bounds` doesn't return, so the clamp stays local; the trailing
  `bounds.w == 0 || bounds.h == 0` is still the shared one.
- **Blanket `#![allow(dead_code)]`** at `record_sink.rs:17`. Justified by the
  `test`/`internals` matrix, but a genuinely dead helper here will never be
  reported. Both current ones are live (`kind` in `assert_same_paint`, `count` in
  `encoder/tests.rs:932`).
- **`#[inline]` asymmetry** inherited from `cmd_buffer`: present on `push_clip`
  through `draw_text`, absent on `draw_mesh` through `draw_polyline`; the required
  half carries none in either impl. Same-crate monomorphization makes it
  near-irrelevant now — which is also why the old excuse for the split is gone.

---

## Not a problem

- **Generic `encode<S: PaintSink>` over `dyn`.** Static dispatch is right for a
  per-shape hot path, and `record_sink` is `cfg`-gated, so production
  monomorphizes exactly one instantiation.
- **Composer tests going record → replay → compose** rather than the production
  encoder → session path. Payload values are identical either way; the extra hop
  buys the assertable artifact.
- **`Composer` holding `transform_stack` while `ComposeSession` holds
  `current_transform`.** Asymmetric, but the reason is stated
  (`composer/mod.rs:703–706`) and correct: the stack is the allocation worth
  retaining across frames, the live product isn't.
- **The broad encoder/composer test migration.** Ordering, clipping, gradients,
  text, damage, and GPU-view coverage all survive; the one focused gap is in
  finding 6.
