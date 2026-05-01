# Intrinsic-dimensions protocol (Option B)

On-demand `intrinsic(node, axis, req: LenReq) -> f32` query plus an
intra-frame cache, used by `Grid` and `Stack` drivers when a parent's
available width can't be derived from a child's measure pass alone.

Two motivating user-facing patterns drive the scope:

- **Property-grid layout** — `Grid` with `Track::hug()` for the label
  column and `Track::fill()` for the value column, where the value
  column contains wrapping text. The Hug column should hug to the
  longest label; the Fill column should get the rest of the width and
  wrap text inside. Today `sum_spanned_known` returns `INFINITY` for the
  Hug track during measure and the value column's wrapping text shapes
  at its natural unbroken width. **Step B target.**
- **Chat-message layout** — `HStack { Avatar (Sizing::Fixed),
  Message (Sizing::Fill, wrapping text) }`. Avatar takes its fixed
  width; message Fill claims the leftover. Today Stack measures the
  message with `available_w = INFINITY` (the WPF intrinsic trick), then
  arrange clamps the slot to leftover — the cached shape is at natural
  width, the rendered slot is narrower, text overflows. **Step C target.**

Pinned by `wrapping_text_in_grid_auto_column_does_not_wrap_today` and the
"BUG (Option B gap)" card in `examples/showcase/text.rs`. When this lands
both flip; the showcase grows two new cards demonstrating the fixed
property-grid and chat patterns.

## Conceptual model

Two explicit kinds of intrinsic query, named by CSS Grid spec:

```rust
pub enum LenReq {
    /// Smallest size the node can occupy without breaking. For text:
    /// longest unbreakable run.
    MinContent,
    /// Size the node "wants" with unlimited room. For text: natural
    /// unbroken width.
    MaxContent,
}
```

Each query is **per-axis**. Callers ask `engine.intrinsic(node, axis, req)`
on the axis they care about; the orthogonal axis isn't computed.

Splitting "infinity-as-intrinsic" into two named cases is the Masonry /
Yoga / Taffy convergence point — `references/xilem.md` §3 calls this
"the most refined version of WPF's `availableSize`". (Masonry has a
third `FitContent(N)` variant; we don't need it because our `Track`
type already carries `min`/`max` clamps that express CSS
`fit-content(N)` at the track level rather than the query level. If a
non-grid widget ever needs an at-most-N intrinsic, revisit.)

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

After Option B, simplified CSS Grid §11.5 algorithm. Per-track ranges
fold together two sources: (a) the user's `Track.min`/`Track.max` clamps,
(b) intrinsic queries to span-1 cells.

1. **Per-track range.**
   - `Hug` track: `track.min = max(t.min, max over cells of intrinsic.MinContent)`,
     `track.max = min(t.max, max over cells of intrinsic.MaxContent)` — i.e. `t.max`
     gives a CSS `fit-content(N)`-style ceiling.
   - `Fixed(v)` track: `track.min = track.max = v.clamp(t.min, t.max)`.
   - `Fill(_)` track: `track.min = t.min`, `track.max = t.max` (final size resolves
     in step 2 from leftover).

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
   resolved to. Avoids the WPF cyclic-iteration trap. **User-visible
   gotcha:** a span-N cell with no span-1 cells in its tracks has zero
   width. Workaround: add explicit `Track.min` or use span-1 cells.
   Document in `docs/grid.md` and add a regression test pinning this
   behavior so the limit is intentional rather than a forgotten edge.

### Step B design commitments

Three semantic decisions baked into the algorithm above; calling them
out explicitly so they don't get re-litigated in code review:

- **`Sizing::Fill` element in intrinsic context returns its content's
  intrinsic, ignoring the Fill weight.** A `Fill` child reports the same
  `MinContent` / `MaxContent` as a `Hug` child would; the weight only
  matters at distribution time. Matches CSS Grid `1fr` semantics (track
  contributes content's `min-content` to base size, `max-content` to
  growth). The two alternatives — Fill = ∞ or Fill = 0 — both produce
  surprising layouts in mixed Hug+Fill grids.
- **No iterative re-measure.** Step B is forward-only: query intrinsics
  → resolve track sizes → measure children with resolved widths. We do
  not retry if results "don't converge" (WPF's `c_layoutLoopMaxCount`
  pattern). One-shot decision, accept the result.
- **Existing grid tests are the canary.** Run them under the new
  algorithm before merging; any test that shifts is either a bug we
  introduced or a deliberate behavior change worth pinning. Don't update
  expected values blindly.

4. **Mixed Auto + Fill.** Resolve Fixed first, Auto next (as above) using
   the available width *minus* Fixed total. Fill tracks consume any final
   leftover by weight, as today.

This is a real algorithm. ~80 lines of change in `grid/mod.rs`, including
the existing `resolve_axis` becoming aware of intrinsic ranges per track.

## Stack `Fill` flex with min-content floor

Today (in `src/layout/stack/mod.rs`): Stack measure passes `available.main
= INFINITY` to children (WPF intrinsic trick). Children measure at their
natural unbroken size. Stack arrange then computes Fill widths from
leftover and slots each child into the resolved width — but the child's
*measured* size and any cached layout (e.g. text shape) are at natural
width, not the Fill-resolved width. Result: chat-message HStack ships a
shape cached at 700 px into a 160 px slot, text overflows visually.

The fix is structural: **Fill resolution moves into Stack's measure
pass.** Single forward pass:

1. **Query intrinsics** for each child on main axis: `MinContent` and
   `MaxContent`.
2. **Resolve Fill widths** using leftover-after-non-Fill, weight share,
   clamped to each child's `[MinContent, MaxContent]`. If total floors
   exceed available, clamp at floors and overflow.
3. **Measure each child** with its resolved width as `available.main`.
   For Fill children that's the Fill share; for non-Fill children it's
   still the WPF infinity. Wrapping text reshapes correctly because
   `shape_text` now sees the right width.
4. **Arrange** uses already-resolved widths, no recomputation.

The non-flex (no-Fill) case stays the same — `justify` distribution with
sum-of-desired; nothing changes there.

This restructures Stack to do "decide widths during measure" instead of
"decide widths during arrange". Cleaner long-term — measure becomes the
single decision point — but the change is bigger than just adding an
intrinsic call.

### Step C scope commitment

Stack `Fill` distribution stops at:

- **Min floor** — each child clamped to its `MinContent` on the main
  axis.
- **Weight** — `Sizing::Fill(w)` weights split leftover.
- **Max-size clamp** — `Element.max_size` honored as a per-child
  ceiling.

Anything richer (`flex-basis`, `flex-shrink` distinct from `flex-grow`,
`align-items: baseline`, `flex-wrap`, `align-content`) is **out of
scope** for in-tree extension.

### Future direction: native vs Taffy

For now, the native panel set — **`HStack`, `VStack`, `ZStack`,
`Canvas`, and `Grid`** — is the committed layout vocabulary. Step C
extends `HStack`/`VStack` Fill distribution to be intrinsic-aware (the
chat-message use case); Step B extends `Grid` Auto track sizing the
same way (the property-grid use case). After Steps A/B/C land, the
native set is "done" for the foreseeable future.

**Open future decision (deferred):** whether richer flex/grid features
arrive via:

- (α) Taffy as an opt-in feature flag (`palantir/taffy`), with new
  widgets `ui.flex(|ui| …)` / extended grid backed by Taffy alongside
  the native panels. `references/taffy.md` §7 has the integration
  sketch.
- (β) Taffy replacing the native Grid entirely (full CSS Grid is a
  strict superset of our model — gains `minmax`, `repeat`, named
  areas, etc.).
- (γ) Taffy replacing both Stack flex and Grid (eliminates flex-creep
  pressure entirely; native code keeps only Leaf/ZStack/Canvas).
- (δ) Hand-grow flex/grid in-tree if Taffy proves to have unacceptable
  cost (binary size, mental overhead, integration debt).

We'll pick a direction when the first user demand for a feature beyond
the Step C scope arrives — not now. Until then, native panels are the
authoring surface and the stop-rule on flex creep holds. The corpus's
preferred path is (α); it's the cheapest opt-in if/when needed.

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

### Step A — intrinsic API + cache, no behavior change — **done**

`LenReq { MinContent, MaxContent }`, `IntrinsicQuery`, and `Axis` (promoted
from `stack::Axis` to `layout::axis::Axis`). `LayoutEngine.intrinsics:
HashMap<IntrinsicQuery, f32>` cleared per `run`. Per-driver `intrinsic()`
free functions live next to `measure`/`arrange` in each driver module
(`stack`, `zstack`, `canvas`, `grid`); the central `intrinsic.rs` keeps
the dispatch + leaf path + helpers + types. `LayoutEngine::intrinsic`
memoizes via the cache. Pinned by
`intrinsic_query_on_wrapping_text_leaf_returns_sensible_values`.

### Step B — Grid Auto under constraint — **done**

`AxisScratch` carries `hug_max` (from desired) + `hug_min` (from intrinsic
queries); `GridHugStore` carries both pools. `grid::measure` now takes
`inner_avail`, queries intrinsics for span-1 cells in Hug columns, runs
the constraint solver to resolve column widths against the parent's
available width, then measures cells with their resolved widths so wrap
text shapes correctly. Row heights resolve from cell desired heights
(unchanged). `resolve_axis` rewritten as a three-phase algorithm: Fixed →
Hug constraint solve → Fill constraint-by-exclusion. Pinned by
`wrapping_text_in_grid_auto_column_wraps_under_constrained_width`. New
showcase cards: "two Hug columns" (paragraph wraps, label keeps natural)
and "property-grid" (Hug label column + Fill value column with three
wrapping rows).

### Step C — Stack Fill resolved during measure — **done**

`stack::measure` is now two-pass: first measures all children at INF on
main (the WPF intrinsic trick, as before), then — if the stack itself
has a finite main-axis size and Fill children exist — re-measures each
Fill child at its resolved Fill share clamped to `[intrinsic_min,
max_size]`. Wrap text in Fill children reshapes via the existing
`shape_text` reshape branch because `committed_w < natural_w`. Hug
stacks (`inner.main = INF`) skip the second pass — Fill children stay
at natural width as before, matching the existing "Hug stack hugs to
children's natural widths" rule. Pinned by
`hstack_fill_wrap_text_reshapes_at_resolved_share` and
`hstack_fill_wrap_text_floors_at_min_content`. New showcase card:
"chat-message" — `HStack { Fixed avatar + Fill wrapping message }`
across three rows.

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
