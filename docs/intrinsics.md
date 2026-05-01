# Intrinsic-dimensions protocol (Option B)

On-demand `intrinsic(node, axis, req: LenReq) -> f32` query plus an
intra-frame cache, used by `Grid` and `Stack` drivers when a parent's
available width can't be derived from a child's measure pass alone.

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

Three explicit kinds of intrinsic query, named by CSS Grid spec:

```rust
pub enum LenReq {
    /// Smallest size the node can occupy without breaking. For text:
    /// longest unbreakable run.
    MinContent,
    /// Size the node "wants" with unlimited room. For text: natural
    /// unbroken width.
    MaxContent,
    /// Max-content capped at a finite width. Reserved for future Grid
    /// `fit-content(N)` track sizing — not consumed by Step A/B.
    FitContent(f32),
}
```

Each query is **per-axis**. Callers ask `engine.intrinsic(node, axis, req)`
on the axis they care about; the orthogonal axis isn't computed.

Splitting "infinity-as-intrinsic" into three named cases is the Masonry /
Yoga / Taffy convergence point — `references/xilem.md` §3 calls
`LenReq` "the most refined version of WPF's `availableSize`".

For the leaf-text case:
- `intrinsic(text_node, X, MaxContent)` = natural unbroken width.
- `intrinsic(text_node, X, MinContent)` = longest-word width.
- `intrinsic(text_node, Y, _)` = single-line height (height-given-width is
  intentionally deferred — see "Explicitly deferred").

## API

```rust
impl LayoutEngine {
    pub(super) fn intrinsic(
        &mut self,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        req: LenReq,
        text: &mut TextMeasurer,
    ) -> f32;
}
```

Dispatched per-driver, the same shape as `measure`. Each driver:

- **Leaf.** Walk shapes. For `Shape::Text`, query `text.measure(...)` with
  `max_w = None` (gives both `intrinsic_min` and natural width from one
  shape, cached in cosmic); pick the right field per `req`. For
  `Shape::RoundedRect`/`Shape::Line`, contribute zero. Add padding +
  margin. Apply `Sizing` override (Fixed → fixed value, Hug →
  content-based, Fill → 0 for MinContent / very large for MaxContent).
  Apply `min_size` / `max_size` clamps.
- **HStack on its main axis (X).** Sum of children's intrinsic on X (same
  `req`) + `(n-1) * gap`.
- **HStack on cross axis (Y).** Max of children's intrinsic on Y.
- **VStack.** Symmetric.
- **ZStack / Canvas.** Max of children on both axes (Canvas adds child
  positions, like its `measure`).
- **Grid.** Sum of resolved track sizes + gaps, where each track's range
  comes from `MinContent`/`MaxContent` queries to spanning children.
  Step B specifies the track-resolution algorithm.

## Cache

Intra-frame: `HashMap<(NodeId, Axis, LenReq), f32>` on `LayoutEngine`,
`clear()`'d at the top of `run` (capacity retained — same pattern as
`desired`).

`intrinsic()` checks the cache first, recurses on miss, stores the result.
Justification: intrinsic answers are pure functions of the subtree —
they don't depend on the parent's available width or the arranged rect —
so caching them within a frame is sound.

Cross-frame caching is **deferred**. Cosmic already caches text shapes
across frames keyed on content hash, which covers the expensive part of
leaf intrinsics for free. Container intrinsics are cheap arithmetic;
re-running them per frame is fine until profiles say otherwise. When the
persistent `Id → Any` state map lands (CLAUDE.md §Status), revisit:
keying on `WidgetId` plus a content/topology hash would let us skip
intrinsic recomputation for unchanged subtrees, which is the model
Yoga/Taffy use.

## Text shape interplay

Cosmic shapes once per `(text, size, max_w)` triple. A `MaxContent` query
uses `max_w = None`; cosmic's hashmap then holds the unbounded buffer.
A `MinContent` query reads `intrinsic_min` from the same buffer (already
returned by `MeasureResult.intrinsic_min`) — no second shape needed.

Later, when measure resolves a constrained width and shapes again with
`max_w = Some(N)`, that's a separate cache entry. Same cost as today's
`shape_text`.

`shape_text` in measure simplifies once Grid + Stack consume intrinsics:
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

1. **Per-track range.** For each `Auto` track, query each span-1 cell:
   - `track.min = max over cells of engine.intrinsic(cell, axis, MinContent)`.
   - `track.max = max over cells of engine.intrinsic(cell, axis, MaxContent)`.

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

1. **Per-child target.** Non-Fill child: `target = engine.intrinsic(c,
   main, MaxContent)`. Fill child: `target = available × weight /
   total_weight` (after subtracting non-Fill targets and gaps).
2. **Min floor.** Each child also has `floor = engine.intrinsic(c, main,
   MinContent)`.
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

### Step A — intrinsic API + cache, no behavior change

- Define `LenReq` enum and `Axis` (likely promote `stack::Axis` to `crate::primitives` or `layout::Axis`).
- Add `intrinsic_cache: HashMap<(NodeId, Axis, LenReq), f32>` on
  `LayoutEngine`, `clear()` in `run`.
- New per-driver `intrinsic()` free function; new `LayoutEngine::intrinsic`
  method that dispatches and memoizes via the cache.
- Drivers don't *consume* anything yet — the API exists, nothing calls it
  in production code paths.
- New test: directly call `engine.intrinsic(...)` on the BUG-card grid's
  text cell and assert sane values (paragraph's `MinContent` ≈ longest
  word width, `MaxContent` ≈ natural width).

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

- **Query cost.** On-demand, only the subtrees that drivers actually
  query. Cosmic caches text shapes across frames; the intra-frame cache
  removes "queried twice" duplication. Realistic cost: a few HashMap
  lookups + recursion through the relevant subtrees per Grid/Stack
  resolution.
- **Memory.** Intra-frame HashMap, capacity proportional to distinct
  `(NodeId, Axis, LenReq)` triples queried per frame — typically <100.
  Negligible.
- **Risk.** Grid test fallout — Auto track sizing semantics shift when
  available width matters. Most tests pin pixel-exact rects, so any
  semantic change shows up. Plan: pre-audit existing tests, decide which
  shifts are deliberate, update those.
- **Risk.** Stack with Fill children + Wrap text isn't currently tested
  end-to-end. New territory; lock down with tests in step C.

## Controversial / open

The reference corpus (`references/SUMMARY.md` and per-framework notes)
has strong opinions, several of which this plan contradicts. Calling them
out before code lands so we agree on the trade-offs.

### 1. The corpus's stated position is *no separate intrinsic API*

`references/SUMMARY.md` lists "Intrinsic-size API" as a decided design
fork:

> **Yes, separate (Flutter) / no, just measure (WPF, Palantir)** → **No,
> just measure**. *Two-pass already runs intrinsic queries. Flutter's
> separate `IntrinsicWidth`/`Height` is its O(n²) slow path.*

This plan **adds a separate intrinsic pre-pass**. That's a reversal.

The argument for the reversal is that "just measure with `INFINITY` as
available" *already happens* in our measure pass today, and it doesn't
solve Grid `Auto` (which is why we have the BUG card): the column resolves
its own size from children's `desired`, but `desired` for a wrapping text
child with `available_w = ∞` is the natural unbroken width, not anything
the column can use to share 200 px between two siblings.

The corpus's recommended *alternative* (`references/wpf.md` §7,
`references/SUMMARY.md` "WPF Grid cyclic pathology") is bluntly: **don't
do Grid `Auto` under constrained width at all** — restrict to "Fixed +
Auto + Star without `*`↔`Auto` cross-axis cycles", or avoid Grid for this
case. Acknowledging that route exists.

### 2. ~~Pre-pass vs on-demand~~ — **resolved: on-demand `LenReq`**

We picked the on-demand model:

- `engine.intrinsic(node, axis, req: LenReq) -> f32`, called by drivers
  during measure on the subtrees they need.
- Intra-frame `HashMap<(NodeId, Axis, LenReq), f32>` cache on the engine,
  cleared in `run`.
- No pre-pass, no per-node `Vec`. Cosmic's existing cache handles the
  expensive (text-shape) part of leaf intrinsics across frames; the
  intra-frame cache handles "queried twice during one resolution".
- Cross-frame caching deferred until persistent state map (CLAUDE.md
  §Status) lands and a profile says we need it.

Matches Masonry's `LenReq` precisely; matches Yoga/Taffy's "on-demand
queries with cache" shape modulo the simpler key (no `(known_dim, available)`
tuple — we only need the kind).

### 3. WPF's `c_layoutLoopMaxCount` warning

`references/wpf.md` §5, §7, `references/SUMMARY.md` "WPF Grid": the
real-world WPF Grid hits a cyclic-measure trap any time `*` columns
contain `Auto` rows depending on the column's resolved width. WPF caps the
iteration count at a constant; Telerik / DevExpress ship workarounds.
Microsoft's own Visual Studio 2010 perf retro highlights Grid as a top
offender.

**This plan's algorithm avoids the actual cycle** (no `Auto`↔`Star`
cross-axis dependency — track sizing reads `Intrinsics`, never re-runs
measure). But the warning is broader than the cycle itself: complex Grid
algorithms produce surprising layouts and slow re-measure paths. Adding
constraint-aware Auto sizing puts us closer to that territory.

Mitigation: keep the algorithm strictly forward (intrinsic → resolve →
measure-with-resolved-width — no iteration). Add a regression-pin test
suite for the existing Grid cases before changing semantics.

### 4. No persistent cache

Yoga / Taffy / Masonry all cache intrinsics across frames. Their
profiles say it's load-bearing.

We don't have a persistent state map yet (CLAUDE.md §Status — pending).
Cosmic caches text shapes by `(text, size, max_w)`, so the leaf side rides
on existing infrastructure for free. Container intrinsics get
recomputed per frame.

For our scales this is fine. If a profile ever shows the pre-pass
dominating, the fix is the same as Yoga/Taffy: per-node cache keyed on
content hash, invalidated on tree topology change. Defer until measured.

### 5. Per-axis vs symmetric `Intrinsics` struct

The plan picks `Intrinsics { min: Size, max: Size }` — symmetric, both
axes computed together. The corpus consistently chooses **per-axis
queries** (Masonry, Yoga, Taffy) because in practice each driver wants
intrinsics on one specific axis and computing the orthogonal axis is
wasted work.

For text specifically, the `min.h` / `max.h` produced by the pre-pass
(both equal to single-line height) are *wrong* in the useful sense:
height-after-wrap depends on width, which the intrinsic pass can't know.
We document this as "deferred", but a per-axis API makes the asymmetry
explicit instead of hiding behind a struct that pretends symmetry.

If we adopt the on-demand `LenReq` redesign (§2), per-axis falls out
naturally.

### 6. Stack `Fill` flex with min-content — re-implementing flexbox

`references/SUMMARY.md` and `references/yoga.md` §6 are explicit: real
flexbox is a complex spec. Yoga's main-axis distribution under
min/max-content sizing is hundreds of lines (`CalculateLayout.cpp`),
backed by the cache.

This plan's Stack `Fill` algorithm is a simplified version: `(min,
target)` per child, distribute leftover proportionally to slack. Fine for
the cases we're targeting. But it's a step toward owning a flex
implementation. If we end up needing percentage flex-basis, wrap, or
align-content, the cleanest path is to depend on Taffy
(`references/taffy.md` §7) rather than grow our own.

Mark this in the doc and the code: the Stack flex stops at "min floor +
weight distribution". Anything richer triggers a "should we depend on
Taffy?" conversation, not "let's add another case to our algorithm".

### 7. Three passes vs two

`DESIGN.md` and `CLAUDE.md` both pin the model as **two-pass measure +
arrange**. This plan adds a third (intrinsic) pass.

`references/SUMMARY.md` "Pass shape": **"WPF two-pass + height-prop DFS
for text wrap"** — a third pass *is* anticipated, but specifically as a
height-propagation DFS for text wrap, not a generic intrinsic pre-pass.
`references/clay.md` §4, §9 walk through Clay's third-pass design for
text height.

So adding a third pass isn't violating the design — but it's worth
specifying *which* third pass: ours is broader than what the corpus
expected. Update `DESIGN.md` to reflect the actual model when the code
lands.

### 8. Open question: should we just stop at Step A?

Step A (intrinsic infrastructure, no behavior change) is genuinely cheap
and unlocks cleaner future paths. Steps B and C are real algorithm work
that the corpus warns about (Grid cycles, flex spec complexity) and
that we don't currently have a *user-facing* need for — the BUG card is
artificial; no shipped widget hits it.

Honest scoping question: **maybe we land Step A, leave the BUG card
labeled as a known gap, and revisit B/C only when an actual widget needs
them.** That matches the corpus's general stance ("Flutter intrinsic O(n²)"
warnings, "avoid Grid in Palantir's prototype") more closely than the full
three-step plan.

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
