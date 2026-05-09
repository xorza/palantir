# Multi-rect damage

**Status:** implemented. This file holds (1) the durable design
rationale + survey, (2) the remaining open follow-ups in priority
order. Broader damage roadmap in `damage.md`; live code is canonical
for *what* the system does.

## Shipped design

- `DamageRegion` is `tinyvec::ArrayVec<[Rect; 8]>` with the LVGL-style
  merge cascade + Slint min-growth fallback at the cap
  (`src/ui/damage/region/mod.rs`).
- `DamagePaint::{Full, Partial(DamageRegion), Skip}`
  (`src/ui/damage/mod.rs`).
- Encoder filter: `region.any_intersects(rect)` per leaf
  (`src/renderer/frontend/encoder/mod.rs:170`).
- Backend: one render pass per rect; first pass uses `Clear` when
  `force_clear`, else `Load`; subsequent passes always `Load`
  (`src/renderer/backend/mod.rs::run_one_pass`).
- Coverage threshold: `0.7` of surface area
  (`src/ui/damage/mod.rs::FULL_REPAINT_THRESHOLD`).
- Debug overlay: instanced draw, one stroke per damage rect
  (`QuadPipeline::upload_overlays` / `draw_overlays`).

## Problem (what motivated this)

Old `Damage::compute` unioned every dirty `(rect, hash)` diff plus
every removed widget's prev rect into a single `Option<Rect>`. Two
unrelated tiny dirty corners (top-left FPS counter + bottom-right
indicator) unioned to a near-fullscreen rect and tripped the
50 %-area `Full`-repaint heuristic even though < 1 % of pixels
actually changed.

## Survey: what other systems do

### Slint — fixed-N array + best-merge (closest model copied)

`tmp/slint/internal/core/partial_renderer.rs:213-274`. Stores
`[Box2D; 3]` + `count`. `add_box`:

1. Drop existing rects contained by `new`; if `new` is contained by
   any existing, return.
2. If under cap, append.
3. **If full, merge `new` into the existing rect that minimises
   `union(existing, new).area() - existing.area()`** (greedy
   minimum-growth).

Buffer-age aware via `RepaintBufferType::{New, Reused, Swapped}`
(`:336-351`); `Swapped` unions previous + pre-previous frame.

### Iced — unbounded Vec + sort-by-distance + merge-on-area-excess

`tmp/iced/graphics/src/damage.rs:5-78`. Diff produces
`Vec<Rectangle>`; sort by `center.distance(ORIGIN)`, merge `current`
and `next` if `union.area() - current.area() - next.area() <= 20_000`
px² (`AREA_THRESHOLD`). Used only by `iced-tiny-skia`; iced-wgpu
doesn't apply damage filtering at all.

### LVGL — `inv_areas[LV_INV_BUF_SIZE = 32]`

Merge only when `area(A ∪ B) ≤ area(A) + area(B)` — i.e. overlapping
or adjacent. Distant corners *never* merge. Overflow → full-screen
invalidate.

### Pixman / X11 / Skia — banded RLE

`SkRegion` and pixman's region store an ordered set of horizontal
bands; bands coalesce only when adjacent bands' x-walls match.
Boolean ops via sweepline. Better than free-form rect lists when N is
large (10s–100s of rects), worse than fixed-array under low pressure.

### Chromium `cc::DamageTracker`

Single-rect union; no threshold. Higher layers (Blink invalidation)
decide what becomes a damage event.

### Egui / Xilem / Masonry / Vello / Quirky / Makepad

No per-rect damage. Egui uses `request_repaint_after(Duration)` to
gate *whether* to repaint, not where. Xilem/Masonry: `needs_paint:
bool` flag. Vello pays full scene cost. Quirky/Makepad: per-widget /
per-DrawList re-record.

### WPF

Public API delivers single rect per `InvalidateRect`; a closed-source
MIL compositor accumulates them. Ships a kill switch
(`MIL_RT_DISABLE_DIRTY_RECTANGLES`) — implies the dirty-rect path has
known correctness gotchas.

### Patterns that emerge

| Pattern | Examples | Pros | Cons |
| --- | --- | --- | --- |
| Fixed-N array + best-merge | Slint (N=3), Palantir (N=8) | alloc-free, O(N) insert, never explodes | overpaints under pressure |
| Unbounded Vec + post-sort-merge | Iced | precise; merges by spatial locality | unbounded; per-frame alloc unless retained |
| Banded RLE (region) | Pixman, Skia | scales to 100s of rects | overkill for desktop UI |
| Single union | Chromium | dead simple | corner-pair pathology |
| Coarser granularity | Egui, Xilem | trivial | no partial-paint savings |

## Heuristics with concrete numbers

| Source | Heuristic | Value |
| --- | --- | --- |
| LVGL | invalidation buffer cap | `LV_INV_BUF_SIZE = 32` |
| LVGL | merge rule | `area(A∪B) ≤ area(A) + area(B)` |
| Iced | merge rule | `union_excess ≤ 20_000` px² |
| Slint | rect cap | 3 |
| Palantir | rect cap | `DAMAGE_RECT_CAP = 8` |
| Palantir | full-repaint threshold | `0.7` of surface area |

## Pitfalls (pin in tests when relevant)

1. **AA bleed at scissor edges.** Backend pads each scissor by
   `DAMAGE_AA_PADDING` px. Encoder filter uses *unpadded* rect — see
   open follow-up H1.
2. **Sub-pixel snapping.** Round outward (`floor(min)`, `ceil(max)`).
   The `URect` conversion path does this.
3. **Subpixel-AA / LCD text.** Glyph filtering reads neighbours; a
   tight scissor over part of a glyph cell produces fringing. Text
   path is alpha-AA only today, so theoretical.
4. **Scroll / transform animation.** A subtree's whole screen-space
   rect moves; the damage diff emits prev + curr (covered by
   `animated_parent_transform_unions_old_and_new_positions`).
5. **Z-order changes.** Sibling reorder damages the union of
   affected siblings via `subtree_hash` rolling up child order.
6. **Skipped frames.** `Ui::invalidate_prev_frame` rewinds the prev
   snapshot. Not enforced; see open follow-up H2.
7. **TBDR mobile.** Multi-pass damage can be net-negative on tilers.
   Desktop-first.
8. **`VK_KHR_incremental_present`.** Not exposed by wgpu (gfx-rs/wgpu
   #2869). Out of scope.

## Why the chosen GPU plumbing

Three options were on the table:

1. **Replay-pass-per-rect** *(picked)*. Backend wraps the
   `render_groups` loop in `for rect in region.iter()`. Quad upload
   + text prepare happen once. Cost: N render-pass setups. Benefit:
   zero composer changes, composes with rounded-clip stencil cleanly.
2. **Single pass, scissor-per-draw.** Composer expands so each group
   carries a damage-intersected scissor; groups duplicate when they
   touch multiple damage rects. Composer surgery.
3. **Stencil-mask damage.** Write 1s into stencil for the union;
   draw with `EQUAL 1`. Conflicts with rounded-clip stencil
   semantics.

Option 1 was correct for the bound (`N ≤ 8`) and lack of profile-
driven motivation. Graduate to (3) if profiling shows pass overhead
dominates *or* we ship LCD subpixel text (per-rect scissor wraps
glyph cells incorrectly; stencil over union doesn't).

---

# Open follow-ups (priority order)

## High — quick wins

### 1. Pin multi-rect threshold escalation in the test sweep

`damage_filter_threshold_cases` (`src/ui/damage/tests.rs`) is all
single-rect cases. The new threshold (0.7) is applied to the *sum*
of per-rect areas — that's the actual escalation logic the multi-
rect change introduced, and it isn't pinned. Add a case with two
non-overlapping rects whose `total_area` sum sits just below
threshold (stays Partial) + another just above (escalates Full).

### 2. Inline the `damage_region` test helper

`support/testing.rs:123` is a two-line helper called from two sites,
introduced as a CLAUDE.md-compliance workaround. The doc comment
justifying its existence is itself a smell — inline at the call
sites, delete the helper.

### 3. `partial_cmp(...).unwrap_or(Equal)` → `total_cmp` in min-growth

`src/ui/damage/region/mod.rs:99-105`. Defensive against NaN that
can't occur (`Rect::area()` is `w * h` from internally-non-NaN
fields). One-line cleanup.

## Medium — real value, more thought

### 4. Rename `damage` shadow in `WgpuBackend::submit`

`backend/mod.rs:267, :284-288`. `frame.damage` is shadowed by
`damage` after the `backbuffer_recreated` escalation. Rename to
`requested` / `effective` so the divergence between "what the host
asked for" and "what we rendered" is obvious — especially in the
debug-overlay call (`damage` shadow is what the overlay sees).

### 5. `Region::any_intersects` strictness vs. `add` symmetry

`region/mod.rs:51` calls `Rect::intersects` (strict `<`); two damage
rects sharing an edge merge in `add` (LVGL rule fires via
`area`-equality) but a leaf touching the edge of a damage rect
reports false in `any_intersects`. Asymmetric. Either document the
strictness or add an `intersects_or_touches` variant.

### 6. `iter` → `iter_rects` rename

Once `DamageRegion` has `is_empty`, `total_area`, `any_intersects`,
the bare `iter` reads ambiguously. Trivial rename.

## Lower — defer / debug-only / data-driven

### 7. Assert `upload_clear` ↔ per-pass `PreClear` correlation

`backend/mod.rs:365-368` uploads the clear-quad buffer iff
`damage_scissors` is non-empty; the schedule emits `PreClear` on
every pass with `damage_scissor.is_some()` (`schedule.rs:77-80`).
Correlated by construction; nothing pins it. A `debug_assert!`
("partial pass with empty `clear_buffer` is a bug") would catch a
future decorrelation.

### 8. Skip `PreClear` when first pass `LoadOp::Clear` already ran

Force-clear-first-pass case: `LoadOp::Clear` paints clear color over
the whole backbuffer, *then* `PreClear` paints clear color a second
time inside the damage rect. Wasted draw. The fix would thread the
load op into the schedule, coupling two modules currently kept
apart. Defer; document as known debug-only inefficiency.

### 9. `force_clear` semantic for trail-style demos

`force_clear` applies to the first pass only. A bouncing-cursor demo
+ `clear_damage` would show the cursor's current rect flash but the
trail rect *not* flash (pass 1 loaded over pass 0's magenta). The
existing fixture works because both rects are first-time damage.
If user-visible: move the conditional inside the loop.

### 10. `DAMAGE_RECT_CAP = 8` tuning

Slint ships 3, LVGL 32. Eight was a guess. The cost of `8` vs `4`
is mostly: how often the min-growth merge fires, and how badly it
degrades quality when it does. Profile against a real workload.

## Hazards (pre-existing; cross-listed in `damage.md`)

### H1. AA fringe leakage at scissor boundaries

Backend pads each *scissor* by `DAMAGE_AA_PADDING`
(`backend/mod.rs:22`); encoder filter (`encoder/mod.rs:170`) tests
against the unpadded rect. A leaf adjacent to a damage rect — its
nominal bounds touch but don't cross — gets *skipped* by the
encoder, but its AA fringe (1–2 px) falls inside the padded scissor.
If that leaf's authoring changed, fringe stays as last-frame's
pixels — visually a 1-px-hard line at the damage boundary.
Pre-existing for single-rect; multi-rect makes it more likely (more
boundaries). Fix: pad the rect in the encoder filter by the same
1–2 logical px, or expose a `Region::any_intersects_padded(r, pad)`.
No fixture catches this today.

### H2. `frame.damage` is a snapshot from a possibly-stale frame

If the host batches `Ui::end_frame` outputs and submits them
out-of-order, or skips a `submit` after `end_frame`, the
`Damage.prev` map is rolled forward but the backbuffer isn't
painted — next frame's diff is wrong. `Ui::invalidate_prev_frame`
covers the documented case (surface lost / outdated), but the
contract that "every `end_frame` is followed by exactly one
`submit`" isn't enforced. A debug-assert in `submit` ("we haven't
seen `end_frame` since last submit") would catch host-loop bugs.

## References

- `tmp/slint/internal/core/partial_renderer.rs:213-324` — Slint
  DirtyRegion (the structural model).
- `tmp/iced/graphics/src/damage.rs:5-78` — Iced grouping (alternative
  if we ever go unbounded).
- `tmp/iced/tiny_skia/src/window/compositor.rs:147-198` — buffer-age
  framing.
- LVGL drawing pipeline (`LV_INV_BUF_SIZE = 32`, merge rule).
- Skia `SkRegion`, pixman regions — RLE/banded reference.
- Chromium `cc::DamageTracker` — single-rect baseline.
- Live code: `src/ui/damage/{mod.rs, region/mod.rs}`,
  `src/renderer/frontend/encoder/mod.rs:170`,
  `src/renderer/backend/mod.rs::{submit, run_one_pass}`,
  `src/renderer/backend/quad_pipeline.rs::{upload_overlays, draw_overlays}`.
