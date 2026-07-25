# Backend renderer review — simplification, consolidation, optimization

Reviewed 2026-07-25 at Aperture commit `86162a5a`, scope
`src/renderer/backend/**` (24 Rust files ≈ 7.3k lines, 5 WGSL files ≈ 1.1k
lines, including tests).

This is a static audit. No benchmark was run for it, and no finding below
should be credited with a speedup until its stated measurement moves.

## Relationship to the existing reviews

`src/renderer/REVIEW.md` (renderer-wide) and `SIMPLIFICATION_REVIEW.md`
(crate-wide) already cover this code. Four of that review's P1/P2 items have
shipped since (`e9de0060` image filtering, `9b0c06bf` schedule state
deduplication, `f2dc2bf2` text-cache maintenance, `86162a5a` text-grid spill
dedup). This document only reports findings that are **new**, plus a short
status line for the prior items that are still open. It does not restate the
prior review's reasoning.

## Conclusion

The backend is in good shape. Its structure — format-independent resources
plus a per-format pipeline set, one encoder per frame with every buffer upload
routed through a single staging belt, a pure `RenderStep` schedule shared by
production and tests — is sound and I found nothing that argues for a rewrite
or a new abstraction layer.

The remaining opportunities are narrow:

1. Per-step GPU debug markers are recorded unconditionally in release. This is
   the only cost in the backend that scales with UI complexity and buys
   nothing outside a GPU capture.
2. Image draws still bind and draw one instance at a time, with a hash probe
   per draw.
3. Bind-state deduplication tracks one enum where the pass really has three
   independent pieces of state, and text opts out of it entirely.
4. Several small consolidations that delete code without changing behaviour.

Priority summary:

| Priority | Change | Expected benefit | Risk | Code effect | Status |
| --- | --- | --- | --- | --- | --- |
| P1 | Feature-gate per-step debug markers | CPU command recording, scales with draw steps | Low | Small reduction | Shipped `bfe5c493` |
| P1 | Coalesce same-texture image runs; cache the last binding | Bind/draw count, per-draw hash probes | Low | Small increase | Coalescing shipped; last-binding cache dropped — see below |
| P2 | Split bind tracking into pipeline / group-0 / vertex buffer, include text | Redundant `set_*` commands | Low–medium | Increase (~40 lines) | **Withdrawn** — `record_pass` puts the ceiling below 1% of a frame |
| P2 | Let `ImageTextures` own its bind-group layout + sampler | Signature consolidation | Low | Reduction | Shipped `c37d8006` |
| P2 | Move `text/mod.rs`'s gated tests into `text/tests.rs` | Navigability; project convention | None | Neutral (move) | Shipped — 1032 → 401 lines |
| P3 | Small cleanups bundle (7 items) | Clarity, ~60 lines | Low | Reduction | 4 open; #1, #3, #5 shipped |

P1 here means "measure and do first", not "assume the result".

---

## P1 — Per-step debug markers are recorded in every release build

`WgpuBackend::render_groups` wraps **every** emitted draw step in a debug
group: `preclear`, `mask_stamp`, `mask_clear`, `quads`, `text`, `meshes`,
`images`, `curves` (`backend/mod.rs`, the eight `push_debug_group` call sites
between the `PreClear` and `CurveBatch` arms). `GlyphAtlas::flush_pending_uploads`
adds two more per frame, and `GpuViewTargets::paint` two per painted view.

None of them are gated. In wgpu 30 each pair costs, per step:

- a memcpy of the label bytes into the pass's `string_data`
  (`wgpu-core-30.0.0/src/command/render.rs`, `render_pass_push_debug_group`);
- two `ArcRenderCommand` pushes into the pass command vec;
- two extra iterations of the command-replay match at pass end;
- on the HAL side, a real `vkCmdBeginDebugUtilsLabelEXT` only when the
  `debug_utils` extension is present (`wgpu-hal-30.0.0/src/vulkan/command.rs`),
  so the HAL call is genuinely free in a normal release run — the wgpu-core
  cost is not.

The step count is exactly the thing that grows with UI complexity: a frame
with a few hundred groups emits a few hundred steps, so this adds several
hundred recorded commands per frame purely for tooling that is not attached.

Recommended change:

- Add a `gpu-debug-markers` cargo feature, off by default, next to the existing
  `profile-with-tracy` (which already documents the "zero overhead by default"
  posture). Enable it implicitly under `debug_assertions` if desired.
- Funnel every call through one helper (`fn group(pass, label)` /
  `fn end_group(pass)`, or a `marker!` macro) that compiles to nothing when the
  feature is off, rather than sprinkling `#[cfg]` across eight arms. That also
  removes the eight `pop_debug_group()` lines from the arms.
- Keep the markers on for the visual suite and the showcase, where a capture is
  the point.

Do **not** simply delete them: RenderDoc / Xcode captures of this renderer are
worth more than the CPU they cost, and the labels are well chosen.

Verification:

- `frame/*_gpu` (`APERTURE_BENCH_MODE=gpu`) on the `cached` and `partial` arms,
  which already exercise the real submit path.
- Add a step-heavy fixture (many small clipped groups) — the current arms may
  not have enough draw steps for the effect to clear noise.
- Command count is the explanatory metric; wall time is the decision metric.

## P1 — Image batches: coalesce same-texture runs and stop probing per draw

**Shipped, with one of the two proposed wins withdrawn.** `ImagePipeline::draw`
is now `draw_batch`, walking maximal runs of adjacent equal `TextureId`s
(`image_runs`) and emitting one `set_bind_group` + one instanced `draw` per run.
Measured on the `record_pass` bench (min over 256 frames, 256 images):

| Arm | Before | After |
| --- | --- | --- |
| `images/shared` (256 draws, one texture) | 3.6–4.1 µs | **1.3 µs** |
| `images/distinct` (256 draws, 256 textures) | 6.9–7.4 µs | 7.2 µs — flat, as required |

`shared` now sits at the same floor as `groups/single`'s lone instanced quad
draw (1.2 µs): 256 image draws cost what one draw costs. `distinct` is the
control and did not move. (Criterion reports a change on `distinct` too, but its
interval on this bench is tens of percent wide — the min is the signal, and it
is flat.)

**The last-binding cache was not implemented, because it cannot work alongside
coalescing.** The review claimed the two were "cheap and independent". They are
not independent: once adjacent runs are merged, no two *consecutive* lookups can
share an id — if they did they'd be the same run. A one-entry `(last_id,
&BindGroup)` cache therefore has a structurally guaranteed 0% hit rate, including
on the `A B A` case the review offered as its motivation, where the cached entry
is always `B` by the time the second `A` is probed. Catching non-adjacent repeats
needs a full memo, which is what the `FxHashMap` already is. Adding the cache
would have been pure dead state.

P3 cleanup #3 (hand-rolled span arithmetic) folded in while the arms were open —
see that item.

Original finding follows.

---

Still open from the renderer review, and worth restating because a second cost
sits next to it.

The `ImageBatch` arm walks the batch's ids one at a time
(`backend/mod.rs`, `RenderStep::ImageBatch`), and `ImagePipeline::draw`
(`image_pipeline/mod.rs:211-222`) does an `FxHashMap<TextureId, BindGroup>`
lookup plus `set_bind_group` plus a single-instance `draw` for each one.

Two separate wins:

- **Run coalescing.** Instance indices inside a batch are contiguous by
  construction (`(start + offset)`), so a run of adjacent equal `TextureId`s
  collapses to one `set_bind_group` + `draw(0..4, first..last+1)`. Paint order
  is preserved because only *adjacent* equal ids merge — no sorting.
  Repeated icons and repeated `GpuView` composites are the target; alternating
  ids are the required control and must not regress.
- **Last-binding cache.** Even where ids alternate, the hash probe repeats for
  ids that were just looked up. Holding `(last_id, &BindGroup)` across the arm
  removes the probe on any repeat, including the non-adjacent case that
  coalescing cannot catch. Cheap and independent of the above.

A missing id (dropped `ImageHandle`) simply draws nothing today; a run-length
pass must keep that behaviour, which is easiest if the skip check happens
before a run is opened.

Verification:

- Assert identical draw order and identical composited output for repeated,
  alternating, and all-unique id patterns.
- Count binds/draws through a test seam.
- Extend `image_pipeline` bench (`src/bench/renderer/backend/image.rs`) with a
  many-distinct-icons workload — its current shape is one texture and
  fragment-bound, so it cannot see this at all.

## P2 — Bind tracking models one state where the pass has three — **withdrawn**

The `record_pass` bench (built for the coverage gap below) measured the ceiling
and it is not worth taking. Deriving from its arms: the `groups` pair gives a
generic cost of **~11 ns per recorded step**; `text/per_group`'s 768 steps
should cost ~8.8 µs at that rate and cost ~17.5 µs, so a text batch carries a
**~29 ns premium** over a generic step. Removing 3 of the 5–6 unconditional
commands recovers maybe 4 µs — on a fixture deliberately engineered to produce
**256 consecutive text batches** via strict-bounds clipping. Real frames do not:
text batches span groups by design, so they only split when clipped text is cut
in X. The `PreClear → Quads` seam is 2 commands × ≤8 damage rects ≈ **0.2 µs**.

Whole main-pass recording is single-digit µs against a ~146 µs CPU frame, so
this item's ceiling is a fraction of a percent, on frames that are already
unusual. The original finding's hedge was right for the wrong reason: the
limiter is not the `Quads → Text → Quads` alternation, it is that recording is
not a large enough slice of the frame to matter.

Original finding follows.

---


`render_groups` tracks a single `Bound` enum and re-issues the whole
pipeline + bind group + vertex buffer triple whenever it changes. But the pass
holds three independent pieces of state, and they do not change together:

- **Group 0 is the same handle** (`self.gradient.bg`) for the quad, curve,
  mask-stamp, mask-clear and pre-clear paths. Every transition among those
  kinds re-sets an identical bind group.
- **`PreClear` shares the quad pipeline** with the `Quads` arm — `bind_clear`
  and `bind` both call `pipelines.select(use_stencil)` on `fmt.quad`. Only the
  vertex buffer differs (`clear_buffer` vs `instance_buffer`), yet `PreClear`
  resets `bound` to `None`, so the following `Quads` step re-sets pipeline and
  bind group as well. That is two redundant commands per damage rect, up to
  `DAMAGE_RECT_CAP` = 8 per Partial frame.
- **Text opts out entirely.** `TextBackend::render_batch` (`text/mod.rs:290-316`)
  issues `set_pipeline`, `set_bind_group`, two `set_immediates`,
  `set_vertex_buffer` and the draw unconditionally, and the arm then sets
  `bound = None`. Consecutive text batches therefore repeat five commands each.
  The atlas-size immediate in particular only changes when an atlas grows.

Recommended change, staged:

1. Split `render_batch` into a `bind`-shaped half and a `draw`-shaped half so
   text participates in the same tracking as the other five kinds, and push the
   atlas-size immediate only when `atlas_px` actually changed.
2. If a recording benchmark justifies it, replace `Bound` with three small
   tracked values (bound pipeline, bound group 0, bound vertex buffer) so a kind
   switch re-issues only what genuinely changed.

Be honest about the ceiling: the common `Quads → Text → Quads` alternation
changes all three pieces of state, so it gains nothing. The wins are
transitions among the gradient-sharing kinds, consecutive text batches, and the
`PreClear → Quads` seam. Step 1 is small and self-contained; step 2 adds state
and should not be done on faith.

Verification:

- The `schedule` bench measures `for_each_step` only, not the wgpu dispatch, so
  it cannot see this. A command-recording benchmark is missing (see below).
- Visual suite, since a dropped bind is a silent corruption, not a panic.

## P2 — `ImageTextures` should own its bind-group layout and sampler

`image_bgl` and `sampler` live on `ImagePipeline` but are used almost entirely
by the texture store, so they are threaded by hand through five signatures:

- `ImagePipeline::drain_registry` → `ImageTextures::drain_registry(ctx, images, layout, sampler)`
- `ImagePipeline::paint_gpu_views` → `GpuViewTargets::paint(ctx, targets, owner, now, textures, layout, sampler)`
- `GpuViewTargets::ensure(device, id, size, owner, epoch, textures, layout, sampler)`
- `render_target::allocate(device, layout, sampler, size)`
- `GpuViewTargets::retire_owner(owner, textures)`

Both `paint` and `ensure` carry `#[allow(clippy::too_many_arguments)]` as a
direct result.

Moving `bgl` + `sampler` onto `ImageTextures` (which already owns `bindings`)
removes roughly ten threaded parameters and both `allow`s. `ImagePipeline::build_variants`
then reads `&self.textures.bgl`, which is the only other consumer. This is pure
consolidation — no behaviour change, no new type.

## P2 — `text/mod.rs` is 61% gated test code — **shipped**

`mod.rs` is now 401 lines of production; the `internals` fixture, the wire-layout
pins, and the `gpu_regression` suite moved to `text/tests.rs` (641 lines) behind
one `#[cfg(test)] mod tests;`. The GPU suite keeps its `internals` gate, nested
inside the file rather than repeated per mod, so the default headless
`cargo test` stays GPU-free. All 14 text tests still pass.

Original finding follows.

---


`text/mod.rs` is 1032 lines, of which lines 400-1032 are the
`#[cfg(all(test, feature = "internals"))] mod internals` fixture and the
`#[cfg(feature = "internals")] #[cfg(test)] mod gpu_regression` suite — 633
lines against ~400 of production.

The project's own rule is to split at >150 lines or >40% of the file, and
`backend/mod.rs` already follows it (`#[cfg(test)] mod tests;` →
`backend/tests.rs`). Move the GPU regression suite to `text/tests.rs` and keep
`make_inner_run` beside it as the shared fixture. Mechanical, no risk.

While there: `backend/tests.rs` is 1431 lines for 25 tests. I did not find
redundancy worth deleting — the cases pin distinct schedule invariants — so
this is a note, not a finding. The prior review's "do not shrink the renderer
test corpus by deleting cases" still holds.

## P3 — Small cleanups

Each of these is independently mergeable and removes code.

1. ~~**`PartialScissors` splits a head off a small array for no reason.**~~
   **Done.** Now a plain `ArrayVec` with `len()` / `iter()`; the two
   `rects.iter().count()` call sites in `mod.rs` became `len()`, and the O(n)
   `remove(0)` and the `once().chain()` iterator are gone. The non-emptiness
   invariant stays where it always actually lived — the constructor's
   `assert!` — and the type doc now says so, so nobody re-derives the split.
   Original finding:
   `viewport.rs:23-41` stores `first: URect` plus `rest: ArrayVec`, built with
   `rects.remove(0)` (an O(n) shift) and iterated as `once(first).chain(rest)`.
   The only thing the split buys is a non-empty guarantee that the existing
   `assert!` already provides. A plain `ArrayVec` field gives the same
   guarantee, turns `iter()` into `self.rects.iter().copied()`, and turns the
   two `rects.iter().count()` calls in `mod.rs` into `len()`. ≈15 lines.

2. **`Backbuffer::size` caches a value that is already a field read.**
   The field's doc (`mod.rs:81-86`) justifies itself with "the Arc traversal
   that call walks is ~15 µs/frame … 14% of trace time". In wgpu 30
   `Texture::size()` is `self.descriptor.size` — an inline field on the
   `Texture` struct, no dispatch (`wgpu-30.0.0/src/api/texture.rs:134`). The
   measurement predates that. Either drop the field (`ensure_backbuffer` and the
   `copy_backbuffer_to_surface` assert read `tex.size()` directly) or rewrite
   the comment so it stops asserting a cost that no longer exists. Note that
   `copy_backbuffer_into` already calls `bb.size()` rather than the cached
   field, so the two paths disagree about which is authoritative today.
   `Stencil::size` is *not* redundant — it keeps no texture handle.

3. ~~**Hand-rolled span arithmetic in three draw arms.**~~ **Done**, alongside
   the P1 image work — leaving one arm converted and its two neighbours not
   would have been worse than either state. All three now go through
   `Span::range()` / `From<Span> for Range<u32>`, the latter being what
   `QuadPipeline::draw_range` already used. `ImagePipeline::draw_batch` also
   takes the batch `Span` itself rather than a pre-sliced slice plus a start,
   matching `draw_range`'s shape and making it structurally impossible to
   slice by one batch and index instances by another.

4. **Two hand-built full-viewport quads.** `QuadPipeline::upload_clear`
   (`quad_pipeline.rs:286-313`) and `DebugOverlay::upload_dim`
   (`overlay_pass.rs:89-105`) construct the same shape: viewport rect, solid
   fill, no corners, no stroke. One constructor (`Quad::viewport_fill(size,
   color)` next to `Quad`) covers both. Worth noting while doing it: the dim
   quad omits `FillKind::SOLID.with_fast()` even though it satisfies every
   fast-path precondition the clear quad does, so it runs the full rounded-rect
   SDF over the whole viewport. Debug-only, but free to fix in the same edit.

5. ~~**`bind_clear` contradicts the schedule's documented invariant.**~~
   **Done — dropped the call**, rather than amending the doc, because the
   redundancy is provable rather than probable: a render pass opens with the
   stencil reference at 0 (WebGPU spec), `for_each_step`'s tail
   `clear_active()` returns every walk to 0 (a chain-less `establish` ends at
   `stencil_ref(0)`; a stamped one is closed by the tail clear, whose own
   comment is "never let a stamped chain survive the walk"), and `PreClear` is
   a walk's first step. So the schedule's invariant is now true as written
   instead of carrying an exception. `bind_clear` gained a comment explaining
   the *absence* of the call, so it does not get re-added defensively.
   Original finding:
   `schedule.rs:318-321` states that deduplication is "only sound because
   `SetScissor` / `SetStencilRef` are the *only* steps that touch either piece
   of state — no draw arm … sets a scissor or stencil reference of its own".
   `QuadPipeline::bind_clear` calls `pass.set_stencil_reference(0)` when
   `use_stencil` (`quad_pipeline.rs:338-340`). It is harmless today — a walk
   always begins with `cur_ref == 0` and always exits at 0, and WebGPU
   initialises the reference to 0 at pass open — but the invariant as written is
   false, and a future change to either side would desync silently. Either drop
   the call (the value is already 0 at every point `PreClear` is emitted) or
   amend the doc to record the one sanctioned exception and why it is safe.

6. **Release asserts on per-frame and per-glyph paths.** The crate's assert
   policy reserves release `assert!` for public-API misuse outside hot paths and
   names per-frame / per-batch / per-glyph checks as exactly what must not pay:
   - `mesh_pipeline.rs:132-139` `mesh_upload_required` — twice per frame.
   - `text/atlas.rs:278` the 256-alignment check — once per rasterized glyph.
   - `text/atlas.rs:251` "glyph inserted over a live cache entry" — once per
     inserted glyph.
   All three are cheap, so this is a policy nit rather than a measured cost;
   `debug_assert!` is the conforming form. `PartialScissors::new`'s assert is a
   deliberate exception and should stay: its failure mode is silent visual
   corruption, and the module doc already argues for crashing there.

7. **`specialize` allocates one `String` per constant.**
   `shader_template.rs:26-42` calls `String::replace` per constant, so the quad
   shader's 13 constants produce 13 full copies of a ~15 KB source at startup. A
   single scanning pass is both simpler and cheaper, and keeps the
   "exactly once" validation. Startup-only, lowest value in this list — include
   it only if touching the file anyway.

Not worth changing but worth knowing: the `profiling::scope!` in `submit`'s
text-prepare block takes a `format!("count={}", …)`. `profiling`'s no-op macro
body is empty (`profiling-1.0.18/src/empty_impl.rs`), so the argument is never
evaluated in a default build — the allocation exists only under
`profile-with-tracy`, where a profiler is attached anyway.

---

## Examined and deliberately not recommended

Recorded so the same ground is not re-walked.

- **Carrying bind state across damage rects.** `render_groups` is called once
  per damage rect and rebuilds `bound` from `None` each time, which looks like a
  free win since the wgpu pass state persists across walks. It is not: every
  Partial walk opens with `PreClear`, which binds the clear vertex buffer and
  resets the tracking anyway. Carrying the state across walks would save
  nothing. (Deduplicating the `PreClear → Quads` seam, P2 above, is the version
  of this that does pay.)

- **Per-rect schedule re-walk cost.** A Partial frame runs up to
  `DAMAGE_RECT_CAP` = 8 walks, each O(groups + batches) — every group's scissor
  is intersect-tested once per rect and all four batch cursors restart. This is
  a real O(rects × groups) term, but the per-group work is a handful of integer
  comparisons and the alternative (precomputing per-group rect membership) adds
  a data structure to save arithmetic. Leave it unless a wide-UI profile says
  otherwise.

- **Unifying the four pipeline modules.** `quad` / `mesh` / `image` / `curve`
  look alike at the `new` / `build_variants` / `upload` / `bind` level, and the
  `upload*` methods are one-line delegations to `DynamicBuffer`. They are not
  worth merging: the shared parts already live in `pipeline_utils` and
  `dynamic_buffer`, and the differences (bind groups, index buffers, instance
  formats, draw shapes) are exactly at the boundaries a merge would have to
  paper over. The thin `upload` wrappers do earn their place — each carries a
  `#[profiling::function]` scope that direct field access would lose.

- **`GlyphAtlas::evict_one`'s linear scan** and **`EncodedCache::sweep` running
  every frame.** Both are documented, deliberate, and measured (the sweep was
  made uniform on purpose in `f2dc2bf2`; the scan is bounded by distinct
  in-view rasterizations). No change.

- **`GpuTimings`' `Cell`/`RefCell` interior mutability.** Required: the pass
  walk holds `&self` on the whole backend for the pass lifetime. Justified as
  written.

- **Dropping the quad shader's `inv_size` varying.** Location 9 exists purely
  to avoid a per-fragment divide on the gradient path, at the cost of one flat
  `vec2` interpolator slot on *every* quad including solid ones. Trading it back
  is defensible on paper; on desktop GPUs at this varying count it is almost
  certainly immeasurable, and the current shape is the one that was tuned.

- **Splitting `backend/mod.rs` by line count.** At 1122 lines it is long, but
  the crate rule keeps inherent impls with their type, so only `Backbuffer`,
  `Stencil`, `Submission*` and `begin_load_pass` could move — a cosmetic split
  that separates the target types from the `ensure_*` methods that own them.

## Prior renderer-review items still open in this scope

| Item | Status |
| --- | --- |
| Coalesce adjacent same-texture image draws | Shipped — see P1 above; the added hash-probe finding was withdrawn as unimplementable alongside it |
| Partition retained render targets by owner (P3) | Open, still correctly P3; the two `retain` scans are unchanged |
| Avoid uploading unused mesh payload ranges (P3) | Open, still correctly P3 |

## Benchmark coverage gaps — closed

This gap is filled. `src/bench/renderer/backend/record.rs` (target
`record_pass`) measures wgpu command recording directly, which is what findings
1, 2 and 3 all turn on. The three workloads this section asked for exist as
three **pairs**, each holding painted content fixed and moving only bind / draw
/ state-set count:

| Arm | Control | Covers |
| --- | --- | --- |
| `groups/per_item` — N clipped cells, one scissor + one draw each | `groups/single` — same N rects in one group, one instanced draw | Per-step cost; the fixture for bind-tracking work |
| `images/distinct` — N images on N textures | `images/shared` — N images on one texture | Run coalescing must collapse `shared` and leave `distinct` alone |
| `text/per_group` — N strict-bounds text batches | `text/single` — N runs in one batch | The five unconditional commands per text batch |

Metric plumbing: `WgpuBackend::run_main_pass` publishes its own host CPU time —
pass open, every recorded step, and the end-of-pass command replay — to
`GpuPassStats::last_main_pass_cpu_ms`. Unconditional (two `Instant::now()` per
frame), because gating it behind `collect_gpu_stats` would mean the only way to
read the number is to also enable the in-pass timestamp writes that perturb it.
Each arm reports min / median over 256 sampled frames plus exact step counts
replayed from the composed buffer.

Two things learned while building it, both recorded in the module doc:

- **Whole-frame wall time is not a usable proxy** and is deliberately not
  reported. A frame is ~70x the recording it contains, and the frontend costs
  dominating it move the *opposite* way across the `groups` pair — one big
  group prunes occlusion over 256 quads at once, 256 small ones don't. It ranks
  the arms backwards.
- **The device must be drained per frame** (`PollType::Wait`). Under `Poll` the
  `images` pair inverts: queued submissions contend with the recording being
  timed. The wait lands outside the measured window.

Read the text pair with one correction. A plain scissor change does not split a
text batch — batches deliberately span groups — so the only way to *get*
consecutive batches also churns scissors, and `text/per_group`'s gap bundles
generic per-step cost with per-batch text cost. Net the first out using the
per-step rate the `groups` pair measures.

The marker on/off control the original table named is now a compile-time
choice, not a runtime one (`bfe5c493`), so it is a two-build comparison rather
than a bench arm.

Measure release builds. Command counts explain a result; they do not replace
elapsed time.
