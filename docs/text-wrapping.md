# Text Wrapping & Intrinsic Sizing

Follow-up to `docs/text.md`. That plan covers single-line labels; this one covers the moment text gains a height-vs-width degree of freedom (wrapping paragraphs, multi-line labels).

## The problem

`measure(available) → desired` from WPF assumes the parent knows the width it can offer. Text breaks that:

- A `Hug`-width `HStack` with a `Fill` text child: parent's width depends on the child's desired width, which depends on the offered width. Circular.
- A `Grid` `Auto` column containing a paragraph: column wants the text's min/max intrinsic width *before* it can decide its own width.
- `VStack` distributing leftover height between a paragraph and a flex sibling: paragraph's height depends on the resolved width, which depends on sibling distribution.

Single-line text dodges this — the only natural size is one line, no width/height tradeoff.

Browsers solve it with `min-content` / `max-content` / `fit-content`. Flutter solves it with `getMinIntrinsicWidth` / `getMaxIntrinsicWidth` / `getMinIntrinsicHeight(width)` as a protocol separate from `layout`. WPF mostly dodges by being width-driven and trusting authors to set explicit widths on wrapping text.

## Plan: Option A now, Option B later

### Option A — eager shape at unbounded width, re-shape on constraint (now)

**Stages per text node, per frame:**

1. **Authoring** — `ui.text.shape(id, &str, font, size, max_w = None)`. Produces an unbounded-width shape. Returns `(measured_max, intrinsic_min)`:
   - `measured_max` = width of widest line, height = line_count × line_height (= max-content size).
   - `intrinsic_min` = width of the widest *unbreakable* run (longest word). This is the floor below which wrapping cannot reduce width.
   - Both written into `Shape::Text`.
2. **Measure** — `leaf_content_size` returns `measured_max` (unchanged from v1 behavior). `Hug` containers pick up max-content as today.
3. **Arrange** — when the parent commits a width `w` to a wrapping text child:
   - If `w >= measured_max.width`: no reshape, height stays at `measured_max.height`.
   - If `w < intrinsic_min`: reshape with `max_w = intrinsic_min` (don't break unbreakable runs); accept the overflow.
   - Otherwise: **re-shape** with `max_w = w`, get new height. Update the arranged rect's height.

**TextKey already supports this.** `max_w_q` is part of the cache key, so a widget shapes at most twice per resize transition (`None` + a specific `max_w`) and zero times in steady state.

**Where the re-shape hook lives:**

- New `Shape::Text` field: `wrap: TextWrap` enum — `Single` (no wrap, current behavior) | `Wrap { intrinsic_min: f32 }`.
- New layout entry point: `LayoutEngine::reshape_text(node, width)` called from `arrange` in `src/layout/mod.rs` for `Wrap` nodes whose final width differs from `measured.width`.
- This is a localized addition to the arrange pass, not a new stage. The two-pass model stands.

**Cost analysis:**

- Steady state (no resize, no content change): 0 reshapes, 0 allocations — same as v1.
- Resize frame: 1 reshape per visible wrapping text node. Cosmic-text shaping is fast; budget allows hundreds per frame at 60fps.
- Cache pollution: bounded — `BufferEntry` GC at `end_frame` retires stale entries (see `docs/text.md` §Allocation strategy).

**What A can't do:**

- A `Grid` `Auto` column containing wrapping text needs to know intrinsic width *during measure*, before arrange runs. A reshapes too late — the column has already committed.
- Min-width resolution for `Fill` siblings: distributing leftover space ideally factors in text's `intrinsic_min` floor; A approximates by using `measured_max` (the max-content width).
- Mixed-direction flex (a child whose preferred axis depends on parent's offered cross-axis): doesn't come up yet.

These are real limits. They're also limits we don't hit in v1 — Grid `Auto` columns currently size from `RoundedRect` shapes (single-line labels), not from wrapping paragraphs. We accept the gap until a concrete widget needs it.

**Acceptance for A:**

- New test: `VStack { Frame.fixed_w(200) { Text("long paragraph...") } }` — text wraps to 200px, height grows accordingly.
- New test: `VStack { Frame.fixed_w(20) { Text("supercalifragilistic") } }` — text overflows at `intrinsic_min`, doesn't break the word.
- No regression in single-line button label tests.
- Steady-state frame profile: 0 reshapes after warmup.

### Option B — intrinsic-dimensions protocol (later)

When a real use case demands it (Grid `Auto` column wrapping a paragraph; `Fill` distribution that respects text's min-content floor; nested wrapping inside flex), promote intrinsic sizing to a first-class protocol.

**Sketch:**

- New trait method (or layout-driver function) `intrinsic(node, axis) -> (min, max)`:
  - For text: `(intrinsic_min, measured_max.width)`.
  - For containers: recurse + combine per-axis (sum for main-axis stacks, max for cross-axis).
  - Cache results on `LayoutResult` so a single measure/arrange pass can call `intrinsic` cheaply.
- Layout becomes three logical stages:
  1. **Intrinsic** — bottom-up, fills `(min_w, max_w)` per node.
  2. **Resolve** — top-down constraint propagation: parents pick widths for `Fill`/`Auto` children using intrinsic ranges as bounds.
  3. **Measure-with-width + arrange** — current two passes, but `available.width` is now the resolved width from stage 2.
- Text reshape happens once, in stage 3, with the final width. No "reshape during arrange" hack.

**What B costs:**

- A pre-pass over the tree, `O(nodes)`. Cheap structurally; needs careful caching to avoid re-walking subtrees during resolve.
- Refactor of `LayoutEngine`: drivers (`stack`, `grid`, `canvas`, `zstack`) all need an `intrinsic` implementation. Mostly mechanical.
- New `LayoutResult` columns: `min_w`, `max_w`, `min_h(w)` (callable, since intrinsic height depends on width — keep as a function, not stored).
- `Shape` extras column needs intrinsic ranges so the leaf path can answer without re-shaping.

**Why defer:**

- B is a layout-engine refactor, not a text-system change. Doing it before we have a wrapping-text widget is speculation.
- A handles the cases v1/v2 will actually hit (button labels, fixed-width paragraphs, simple wrapping inside fixed-width containers).
- The `TextKey` cache and `Shape::Text.wrap` design from A carry forward to B unchanged — A is not throwaway work.
- B's stage-1 intrinsic pass is the right place to add other features later (baseline alignment, aspect-ratio constraints, content-driven `Fit` modes). Worth doing once we have multiple drivers for it, not just text.

**Trigger to revisit B:**

- First widget added that needs intrinsic-aware sizing in a flex/grid context (likely a paragraph widget inside a `Grid` `Auto` column, or a multi-line label inside a flexible toolbar).
- Or: a profiling result showing A's reshape-during-arrange is causing visible layout instability (one-frame width thrash on resize).

## Migration path A → B

When B lands:

1. Add `intrinsic` driver methods, populate `min_w`/`max_w` columns. No behavior change yet — measure/arrange still drive sizing.
2. Switch `Hug` and `Fill` resolution in stack/grid to consult intrinsics. Behavior changes for wrapping-text-in-flex cases; existing tests stay green if they don't exercise those.
3. Remove the arrange-time reshape hook from A — stage 3 reshapes with the final width directly.
4. `Shape::Text.wrap` and `TextKey.max_w_q` are unchanged.

A is a strict subset of B's behavior in the cases A handles correctly. No data loss in the migration.

## Summary

| Concern | A (now) | B (later) |
|---|---|---|
| Single-line labels | works | works |
| Fixed-width wrapping paragraph | works | works |
| Wrapping text in `Grid` `Auto` column | wrong width, accepted | correct |
| Wrapping text in `Fill` flex slot | uses max-content as intrinsic | uses true min-content |
| Reshape cost | up to 1 per resize frame | up to 1 per resize frame |
| Steady-state alloc | 0 | 0 |
| Engine refactor | localized arrange hook | three-stage pipeline |
| Trigger to build | now, with text | first real victim of A's gaps |
