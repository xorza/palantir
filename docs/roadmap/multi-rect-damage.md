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
  (`src/renderer/frontend/encoder/mod.rs:172`).
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
6. **Skipped frames.** `Ui::begin_frame`'s auto-rewind (via the
   shared `FrameState` set by `WgpuBackend::submit`) rewinds the
   prev snapshot when the previous `FrameOutput` didn't reach
   submit. No host-facing "invalidate" call needed.
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

# Open follow-ups

Both items are blocked on a workload that doesn't exist yet — neither
is action-ready today.

### Symmetric scissor-boundary leakage

The scissor-padding/encoder-filter asymmetry. Backend pads each
scissor by `DAMAGE_AA_PADDING = 2` px (`backend/mod.rs:22`); the
encoder filter (`encoder/mod.rs:172`) tests
`region.any_intersects(rect)` against the un-padded rect.

The padding solves the **outgoing-fringe** problem: a *changed* leaf
inside the damage rect whose AA / stroke / glyph metrics extend past
the rect's edge — the padded scissor accepts those pixels. Without
it, the scissor would clip the fringe and leave a 1-px-hard edge.

But the same padding *creates* the **incoming-fringe** problem. The
2-px strip just outside the damage rect overlaps the rendered bounds
of *adjacent unchanged* leaves (strokes, italic descenders, glyph
fringes). `PreClear` / `LoadOp::Clear` paints clear color across
that strip; the encoder skipped the unchanged leaf, so its fringe
is never re-emitted — a slice of the leaf's overhang gets wiped.

| | Without padding | With padding (today) |
|---|---|---|
| Outgoing fringe (changed leaf inside damage) | clipped → 1-px hard edge | painted correctly |
| Incoming fringe (unchanged leaf adjacent to damage) | preserved | overwritten → missing slice |

**Fix:** pad the *encoder filter* by the same amount. Mechanical —
add a `Region::any_intersects_padded(rect, pad)` and thread
`DAMAGE_AA_PADDING` (in logical px) to the filter call site.

**Why invisible today:** production scenes use filled rects + plain
text — neither overhangs. Trigger is "stroked element next to a
frequently-changing widget", which no fixture or app currently has.

**Side knob:** `DAMAGE_AA_PADDING = 2` is defensive; most AA bleeds
< 1 px. Halving shrinks the leakage zone. Worth evaluating once a
fixture exists.

### `DAMAGE_RECT_CAP = 8` tuning

Slint ships 3, LVGL 32. Eight was a guess. Re-decide once a real
workload bench exists; until then `8` is fine.

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
  `src/renderer/frontend/encoder/mod.rs:172`,
  `src/renderer/backend/mod.rs::{submit, run_one_pass}`,
  `src/renderer/backend/quad_pipeline.rs::{upload_overlays, draw_overlays}`.
