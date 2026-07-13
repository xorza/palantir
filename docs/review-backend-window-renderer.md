# Review: `renderer/backend/mod.rs` + `window_renderer.rs`

> **Status:** A1, A2, S1, S3 and the smaller items were applied (asserts replace
> the three degrade paths; `PresentMode` is computed once in `cpu_frame` and
> threaded through `CpuFrame`; stencil ensure + `frame_submitted` ack moved into
> the painting arms; `post_record` self-gates; `Clock: Debug` supertrait +
> derives). Not applied: S2 (`WgpuBackendConfig` — deliberate future-proofing
> per its doc), `WindowRendererBuilder` Debug (blocked on `HostContext` not
> being `Debug`), and the open questions (need a call).

Scope: `src/renderer/backend/mod.rs` (the `WgpuBackend` frame function and the
`Backbuffer`/`Stencil` ensure API) and `src/window_renderer.rs` (`WindowRenderer`,
`PresentStrategy`/`PresentMode`, the frame/present flow). Cross-referenced against
`offscreen_host.rs`, `winit_host/`, `ui/mod.rs` (frame_submitted / classify_frame),
`ui/damage/region/mod.rs`, and `renderer/frontend/mod.rs`.

Overall: the two files are in good shape — responsibilities split cleanly
(backend = window-agnostic GPU translation, window renderer = per-window policy),
the `present_mode` classifier is a genuinely nice piece of design with the right
tests, and the documentation density is exceptional. The findings below are mostly
about **invariants enforced by comment + silent degrade paths** rather than by
construction, which conflicts with the project's own crash-on-logic-errors rule.

## Architectural issues

### A1. Three "self-healing" degrade paths mask logic errors instead of asserting

All three are unreachable under current invariants, and — crucially — **none of
them can produce a correct frame if they ever fire**, because by the time they
run, the draw list has already been built (and damage-culled) for the original
plan in `cpu_frame`. Silently escalating the *plan* without rebuilding the *list*
clears the target and then draws only the Partial-culled leaves — undamaged
content is erased for a frame. Per the project rule (release asserts on
invariants, crash on logic errors), these should assert.

1. **Recreate-escalation in `render_to_texture`** — `window_renderer.rs:495-496`
   (`let plan = if recreated { plan.to_full() } else { plan };`).
   A Partial plan reaches `ViaBackbuffer` un-escalated only when
   `backbuffer_fresh` is true, which means last frame rendered into the
   backbuffer at this size/format — so `ensure_backbuffer` must return `false`.
   Every path where `recreated` can be `true` (first resync after Direct frames,
   BackbufferCopy first paint, size/format change) already carries a Full plan,
   escalated *before* `Frontend::build` (in `present_mode` or by force_full
   upstream). The escalation is dead code; if it ever fired live it would submit
   a Partial-culled list under `LoadOp::Clear`.
   **Change:** replace with
   `assert!(!recreated || matches!(plan.kind, RenderKind::Full), "...")`.

2. **`copy_backbuffer_to_surface` silently creates + copies an undefined
   texture** — `backend/mod.rs:1049-1065`. The doc comment (1044-1048) claims
   "`ensure_backbuffer` forces the next painting frame to `Full` via the same
   signal, so the one-frame glitch self-heals" — this is **false**: the returned
   bool is dropped here, and the next painting frame's `ensure_backbuffer` finds
   a size/format-matching texture and returns `false`, so nothing escalates. The
   actual protection is upstream: `SkipCopy` requires `Damage::Skip`, which
   requires the previous frame to have been submitted at the same size
   (`classify_frame` forces Full on display change / non-submit) and format
   (`note_format` clears `frame_submitted`). So the backbuffer must already
   exist and match when this runs.
   **Change:** drop the internal `ensure_backbuffer`; take the backbuffer by
   `.expect("SkipCopy implies a prior submitted paint")`, assert size+format
   match the target, and fix the doc. Signature then tightens to
   `(&self, &Backbuffer, &wgpu::Texture)` (it doesn't need `&mut self` even
   today).

3. **Empty-scissor degrade in `submit`** — `backend/mod.rs:397-399` claims a
   Partial region whose rects all clamp to zero "degrades to a single `Full`
   pass — correct, just wasteful". Not correct: the empty `damage_scissors`
   list routes into the Full branch (`mod.rs:614`) — `LoadOp::Clear` over the
   whole target with a Partial-culled draw list. It's also unreachable:
   `DamageRegion::collapse_from` clips every rect to the surface and drops
   paint-empty ones (`ui/damage/region/mod.rs:137-140`), `Damage::new` returns
   `Skip` for an empty region (`ui/damage/mod.rs:356-358`), and
   `logical_rect_to_phys_scissor`'s ±2 px AA padding means any surviving rect
   yields a nonzero scissor.
   **Change:** after `build_damage_scissors`, assert
   `!(plan is Partial && damage_scissors.is_empty())`; delete the misleading
   paragraph. `damage_scissors.is_empty()` then cleanly means "Full plan".

### A2. Draw-list/submit-plan agreement is enforced by comment — compute `PresentMode` once

`present_mode()` is evaluated twice per frame with identical inputs:
`window_renderer.rs:423` (in `cpu_frame`, to pick the build plan) and
`window_renderer.rs:470` (in `render_to_texture`, to pick the GPU path). The
comment at 423 argues "same plan, strategy, and backbuffer freshness — none
mutated between the two calls", i.e. correctness rests on nobody ever touching
`backbuffer_fresh` (or the strategy) between the two call sites. Computing twice
is exactly how the two phases *could* disagree; computing once is how they
can't.

**Change:** have `cpu_frame` return the mode alongside the report (named struct
per house style, e.g. `struct CpuFrame { report: FrameReport, mode: PresentMode }`)
and thread it through `present()` into `render_to_texture(gpu, target, mode)`.
`render_to_texture` uses `report` *only* to recompute the mode (line 470), so it
stops needing the report entirely; the big invariant comment in `cpu_frame` and
the `PresentMode` doc ("computed identically ... so the two phases can't
disagree") both dissolve. This also concentrates all plan escalation in one
place, which is what makes the A1-1 assert obviously right.

## Simplifications

### S1. Move the stencil block inside the painting arms of `render_to_texture`

`window_renderer.rs:464-469` runs before the mode match, so the Skip arms read
**stale** frontend state: on a skip frame `Frontend::build` didn't run, so
`buffer.rounded_clips` is last painted frame's, and `ensure_stencil` may
allocate/resize a stencil no pass will consume. Harmless today only because a
skip frame implies an unchanged target size. Compute
`use_stencil`/`ensure_stencil`/`stencil_view` only in the `Direct`/
`ViaBackbuffer` arms (or behind a `mode.plan().is_some()` gate once A2 lands).

### S2. `WgpuBackendConfig` is a one-field struct

`backend/mod.rs:66-77`. The doc says it exists so call sites "don't grow a long
positional signature" — but there is exactly one knob and two construction
sites (`offscreen_host.rs:80-83`, winit's gpu setup). Passing
`collect_gpu_stats: bool` directly deletes the type; reintroduce a config
struct when a second knob actually appears. (Deliberate future-proofing per its
doc — cheap to keep, cheaper to delete.)

### S3. `submit` reaches into `text.instances`

`backend/mod.rs:687-689`:
`if !self.text.instances.is_empty() { self.text.post_record(); }` — the
emptiness gate belongs inside `TextBackend::post_record`; the frame function
shouldn't know which field encodes "had text this frame".

## Smaller improvements

- `backend/mod.rs:1026`: on the direct-present path `submit` already created a
  surface view (`mod.rs:610`); `run_overlay_pass` creates a second one. Only
  costs when the damage-rect overlay is on — pass the view in if it's ever
  restructured, not worth its own change.
- `window_renderer.rs:102`, `:37`, `:208`: `PresentStrategy`, `WindowRenderer`,
  `WindowRendererBuilder` lack `#[derive(Debug)]` while sibling `PresentMode` /
  `FrameTarget` have it (house rule: always derive Debug). Needs `Clock: Debug`
  or a manual impl for the two structs.
- `window_renderer.rs:508`: `render_to_texture` sets `frame_submitted = true`
  even on the Skip arms, where `Ui::frame` already acked it
  (`ui/mod.rs:451-453`). Harmless, but the ack currently has two owners; moving
  the store into the painting arms makes ownership single again.
- `backend/mod.rs:1049`: `copy_backbuffer_to_surface` takes `&mut self` but
  needs `&self` (falls out of A1-2 anyway).

## Open questions

1. **Device encapsulation posture.** The backend hides `device`/`queue` behind
   four thin methods (`configure_surface` `mod.rs:173`, `ensure_backbuffer`
   `:296`, `ensure_stencil` `:333`, `max_texture_dim` `:356`, `present` `:363`).
   Two of these are one-hop wrappers the house style would normally delete in
   favor of `pub(crate)` fields — and with a `pub(crate)` device,
   `Backbuffer::ensure` / `Stencil::ensure` could live on their own types
   (behavior next to data, and they're WindowRenderer-owned resources anyway).
   Is "nothing outside `backend/` issues raw wgpu calls" an intentional hard
   boundary worth the wrappers, or an accident of growth? Both are defensible;
   pick one and note it in the module doc.
2. **`configured` coalescing keys on size only** (`window_renderer.rs:80-87`,
   `:362-365`); format changes reach it via `note_format` resetting it to
   `None`. If a host ever mutates other `SurfaceConfiguration` fields mid-run
   (present_mode, alpha_mode), no reconfigure fires. Fine for today's hosts —
   is that contract worth a line on `FrameTarget::config`?

## Prioritized shortlist

1. **A1** — replace the three silent degrade paths with asserts and fix the two
   false doc claims (`window_renderer.rs:495`, `backend/mod.rs:1044-1065`,
   `backend/mod.rs:397-399`). Small diff, removes real present-garbage risk.
2. **A2** — compute `PresentMode` once in `cpu_frame` and thread it through;
   delete the recompute + invariant comments.
3. **S1** — move the stencil ensure into the painting arms.
4. **S3** — fold the `text.instances` check into `post_record`.
5. Debug derives + `&self` on `copy_backbuffer_to_surface` as a drive-by.
