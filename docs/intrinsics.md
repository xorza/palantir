# Intrinsic-dimensions protocol (Option B)

Plan for a layout pre-pass that computes each node's `(min, max)` intrinsic
size on each axis, and lets `Grid` and `Stack` consume those bounds when
the parent's available width can't be derived from a child's measure pass
alone.

Closes the two known gaps from `docs/text.md` §4:

- **Grid `Auto` column wrapping a paragraph** — column needs to know the
  child's intrinsic max-content width to pick its own width *before*
  committing measure. Today `sum_spanned_known` returns `INFINITY` and the
  paragraph shapes at full natural width.
- **Stack `Fill` siblings respecting min-content** — `Fill` distribution
  should clamp each child to at least its intrinsic_min, not let a flex
  sibling steal width past the longest unbreakable run. Today
  distribution uses max-content as the only signal.

Pinned by `wrapping_text_in_grid_auto_column_does_not_wrap_today` and the
"BUG (Option B gap)" card in `examples/showcase/text.rs`. When this lands,
both flip.

## Conceptual model

For every node, layout computes two scalars per axis:

- **`min`** — the smallest size the node can occupy without breaking. Text:
  longest unbreakable run (`intrinsic_min` from cosmic). Stack on its main
  axis: sum of children's mins. Stack on cross axis: max of children's
  mins.
- **`max`** — the size the node "wants" if given infinite room (max-content
  width, single-line height). Text: natural unbroken width / single-line
  height. Stack: sum/max of children's maxes (matching min/max axis split).

These are **width-and-height-independent of each other**: the intrinsic
phase doesn't compute "height given a particular width" (Flutter's
`getMinIntrinsicHeight(width)`). That dependency is handled later by
`shape_text` during measure, once the resolved width is known. Skipping it
keeps the pre-pass O(nodes), not O(nodes × widths).

For the leaf-text case this means:

- `min.w` = longest word width.
- `max.w` = natural unbroken width.
- `min.h = max.h` = single-line height. The actual multi-line height after
  wrap is decided in measure, not the intrinsic phase.

## Data layout

```rust
// src/layout/mod.rs (or result.rs depending on visibility)
#[derive(Clone, Copy, Default, Debug)]
pub struct Intrinsics {
    pub min: Size,
    pub max: Size,
}
```

Stored as `Vec<Intrinsics>` indexed by `NodeId.0` on `LayoutEngine` —
sibling field next to `desired` and `grid`. Same lifecycle: `clear()` +
`resize(n, default)` at the start of each `run`. Capacity retained across
frames.

Not on `LayoutResult` — intrinsics are intra-layout intermediate state, no
external consumer planned. Symmetric with `desired` after the recent
strict-output split.

## Pass sequence

`LayoutEngine::run` becomes three logical passes:

1. **`intrinsic`** — bottom-up walk, populates `engine.intrinsics`.
2. **`measure`** — bottom-up, as today. Drivers that need intrinsic data
   (Grid Auto, Stack Fill) consult `engine.intrinsics(c)`. Available widths
   passed down already factor in resolved track / weight widths.
3. **`arrange`** — top-down, as today.

Intrinsic walk mirrors measure's structure (same `is_collapsed` skip, same
child cursor traversal). Cost: roughly +30–50% over current measure (no
shaping in the intrinsic walk for non-text leaves; one cosmic shape per
text leaf, which is cached and amortizes after the first frame). Doubles
allocation only on first frame; steady-state is unchanged.

### Per-driver intrinsic functions

Free functions per layout driver, dispatched from the engine the same way
`measure` is:

```rust
fn intrinsic_node(engine: &mut LayoutEngine, tree: &Tree, node: NodeId, text: &mut TextMeasurer) -> Intrinsics;
```

- **Leaf.** Walk shapes; for `Shape::Text` shape unbounded once via cosmic,
  produce `(intrinsic_min, natural.w)` for width and `(line_h, line_h)` for
  height. Other shapes contribute zero. Result includes padding + margin
  (so the node's intrinsics describe its *outer* size, matching how
  `desired` works).
- **HStack.** `min.w = sum(child.min.w) + (n-1)*gap`, `max.w = sum(child.max.w) + (n-1)*gap`.
  `min.h = max(child.min.h)`, `max.h = max(child.max.h)`. Plus padding + margin.
- **VStack.** Symmetric.
- **ZStack / Canvas.** Both axes: max of children. (Canvas adds child position offsets to max, like measure.)
- **Grid.** Per-track min/max from spanning children, then sum + gaps. Mirrors how `Auto` track sizing works in measure today, but emits track ranges instead of resolving them.

`Sizing` interaction:
- `Sizing::Fixed(v)` overrides both min and max to `v` (plus margin) on that axis.
- `Sizing::Hug` uses content-based `(min, max)` from children/shapes.
- `Sizing::Fill(_)` — node has no preferred size; report `(0, ∞)` on that axis. Parent decides.

`min_size` / `max_size` extras clamp the result.

### Text shape interplay

Text shapes once during the intrinsic pass (unbounded) — that's what
gives us `intrinsic_min` and `max.w`. The result is cached in cosmic.
Measure may shape *again* with a constrained `max_w` if the parent committed
less than `max.w`; that's a HashMap hit for the unbounded entry plus one
new entry for the bounded one — same cost as today's `shape_text`.

`shape_text` in measure simplifies:
- No more `available_w = INFINITY` early-return — measure now always gets a
  finite resolved width when wrapping matters (Grid/Stack drivers compute
  widths from intrinsics first).
- Reshape branch becomes the standard path for `Wrap` shapes when
  `committed_w < natural_w`.

## Grid `Auto` track sizing under constraint

Today (in `src/layout/grid/mod.rs`): Auto tracks resolve in measure as
`max(span-1 children's desired sizes)`. No awareness of grid container's
available width. Sum can exceed available → grid overflows.

After Option B, simplified CSS Grid §11.5 algorithm:

1. **Per-track range.** For each `Auto` track, gather:
   - `track.min = max over span-1 children's intrinsic.min` on that axis.
   - `track.max = max over span-1 children's intrinsic.max` on that axis.

   `Fixed(v)` tracks: `min = max = v`. `Fill(_)` tracks: `min = 0, max = ∞`
   (or actual base size if any explicit `Track.min`/`max` was set).

2. **Distribute available space.**
   - Compute `total_min = sum(track.min) + gaps`, `total_max = sum(track.max) + gaps`.
   - If `available >= total_max`: each track gets `track.max`. Leftover
     goes to `Fill` tracks proportionally to weight (existing star
     distribution).
   - If `available <= total_min`: each track gets `track.min`. Grid
     overflows the slot. (Same behavior as today's overflow case, just
     clamped at min instead of max.)
   - Else (the interesting case): each track starts at `track.min`. Grow
     each toward `track.max` proportionally to its `(track.max - track.min)`
     until the sum equals available.

3. **Span > 1.** Same exclusion as today — span-1 only contributes to track
   sizing. Span > 1 children consume whatever the spanned tracks already
   resolved to. Avoids the WPF cyclic-iteration trap.

4. **Mixed Auto + Fill.** Resolve Fixed first, Auto next (as above) using
   the available width *minus* Fixed total. Fill tracks consume any final
   leftover by weight, as today.

This is a real algorithm. ~80 lines of change in `grid/mod.rs`, including
the existing `resolve_axis` becoming aware of intrinsic ranges per track.

## Stack `Fill` flex with min-content floor

Today (in `src/layout/stack/mod.rs`): `Fill` distribution treats leftover
as `available - sum(non_Fill children's desired)`. Distributes leftover
to Fill children proportionally to weight. No min-content awareness.

Result: a `Wrap` text in an HStack with a `Fill` sibling can be squeezed
below its longest-word width, breaking inside a word visually (or
overflowing the panel rect, depending on rect-vs-arranged behavior).

After Option B, proper flex on the main axis:

1. **Per-child target.** Non-Fill child: `target = desired = intrinsic.max`
   on the main axis. Fill child: `target = available × weight / total_weight`
   (after subtracting non-Fill targets and gaps).
2. **Min floor.** Each child also has `floor = intrinsic.min` on the main
   axis.
3. **Resolve.** If `sum(floors) + gaps > available`: every child clamped to
   floor; stack overflows. Else: starting from floors, grow each child
   toward its target proportionally to slack until sum = available.

The non-flex (no-Fill) case stays the same — `justify` distribution with
sum-of-desired; nothing changes there.

## What stays unchanged

- `Sizing::Fixed(v)`-parent → child's `inner_avail` propagation (added
  during §4). Still useful: a fixed-width parent can short-circuit
  propagation without needing the intrinsic pass to resolve. Keep.
- Cosmic `MeasureResult.intrinsic_min` and the bounded-vs-unbounded reshape
  logic in `shape_text`. Still the leaf's source of truth for the
  `intrinsic` function.
- `TextWrap::Single` vs `Wrap`. Still distinguishes "never reshape" from
  "reshape under constraint".

## Implementation steps

Three independently mergeable steps. fmt/clippy/test green at each.

### Step A — intrinsic infrastructure, no behavior change

- Define `Intrinsics`. Add `intrinsics: Vec<Intrinsics>` on `LayoutEngine`,
  alloc in `run`.
- New `intrinsic()` method per driver, dispatched from a new
  `LayoutEngine::intrinsic` method.
- Pre-pass before measure populates the table.
- Drivers consume nothing from intrinsics yet. No semantic change.
- New test: pin sane intrinsic values for the BUG-card grid (paragraph's
  min ≈ longest word, max ≈ natural; "right column"'s min == max).

**Acceptance:** `cargo test` green, all 87 existing tests pass plus the
new pin. Showcase visually unchanged.

### Step B — Grid Auto under constraint

- Modify `resolve_axis` (or refactor into a new function) in
  `grid/mod.rs` to use `Intrinsics` for Auto track sizing under
  constrained available width.
- Update existing grid tests where deliberate. Audit each that uses
  `Auto` or `Hug` tracks.
- Flip `wrapping_text_in_grid_auto_column_does_not_wrap_today` →
  `_wraps`. Update the BUG card label in `examples/showcase/text.rs` (or
  remove the BUG marker; keep the card as a working demo).

**Acceptance:** `cargo test` green; showcase BUG card no longer
overflows; grid tests deliberately updated where the algorithm change
shifts results.

### Step C — Stack Fill min-content floor

- Modify `stack::arrange` Fill distribution to clamp at child's
  `intrinsic.min` on the main axis.
- New test: HStack with `Fill` Wrap text + `Fill` Frame, narrow
  available — text gets at least longest-word width.

**Acceptance:** `cargo test` green; new test pins the new behavior.

## Cost & risk

- **Pre-pass cost.** O(nodes), one extra recursive walk. Cosmic shapes
  cache; the only first-frame extra work is the unbounded shape per text
  leaf, which we already do today during measure — net zero on the cosmic
  side, just relocated. Probably +5–15% on layout time per frame; layout
  is a tiny share of frame time.
- **Memory.** `Intrinsics` is 16 bytes (4 × f32). 16 bytes × node_count.
  At 1k nodes that's 16 KB per frame, retained across frames. Negligible.
- **Risk.** Grid test fallout — Auto track sizing semantics shift when
  available width matters. Most tests pin pixel-exact rects, so any
  semantic change shows up. Plan: pre-audit existing tests, decide which
  shifts are deliberate, update those.
- **Risk.** Stack with Fill children + Wrap text isn't currently tested
  end-to-end. New territory; lock down with tests in step C.

## Explicitly deferred

- **Height-given-width intrinsic** (Flutter-style). Skipped because the
  two gaps we're solving don't need it. If we later add a layout that
  sizes a parent based on a wrapped child's height (rare in flex /
  grid), revisit.
- **Baseline alignment.** Not part of intrinsics; would attach to
  `LayoutResult` if needed.
- **Aspect-ratio constraints.** Same — separate concern.
- **Caching intrinsics across frames.** First version recomputes every
  frame. If a profile shows the pre-pass dominates, memoize per node by
  comparing Element hash + tree topology. Premature.
