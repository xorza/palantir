# Multi `Shape::Text` per leaf

Status: **shipped**. Custom widgets can push multiple `Shape::Text`
runs to a single node, each positioned via `local_rect`. Pinned by
`layout::cross_driver_tests::text_wrap::multi_shape_text_per_leaf_shapes_each_run_independently`.
Implementation matches the design below; doc retained as the
architectural rationale.

## Why it's a footgun

A custom widget that opens one node and pushes two `Shape::Text` hits
a hard assert. Normal widget composition can't trigger it (each `Text`
widget opens its own node) — it only bites authors of low-level
widgets.

## What's coupled to "one text per node"

- `Tree::add_shape` (`src/tree/mod.rs`) — `tip.has_text` flag, asserts.
- `LayoutResult.text_shapes: Vec<Option<ShapedText>>`
  (`src/layout/result.rs`) — one slot per node.
- `LayoutEngine::shape_text` (`src/layout/mod.rs`) — writes
  `text_shapes[node.index()] = Some(...)`. Last write wins.
- `MeasureCache.text` + `SubtreeArenas.text_shapes`
  (`src/layout/cache/mod.rs`) — parallel-to-`desired` slice.
- `emit_one_shape` `Shape::Text` arm
  (`src/renderer/frontend/encoder/mod.rs`) — reads
  `layout.text_shapes[id.index()]`, emits one `DrawText` at owner
  inner rect with `align`.
- `TextMeasurer` cache key — `WidgetId`-keyed; two text shapes on the
  same node would collide.
- `intrinsic.rs`, `support.rs::leaf_text_shapes` — already iterates,
  no change needed.

## Design

Mirror the `tree.shapes` flat-buffer + per-node `Span` pattern.

### Storage

```rust
// src/layout/result.rs
pub(crate) struct LayoutResult {
    pub(crate) rect: Vec<Rect>,
    pub(crate) text_shapes: Vec<ShapedText>, // flat
    pub(crate) text_spans: Vec<Span>,        // parallel to `rect`
    pub(crate) available_q: Vec<AvailableKey>,
    pub(crate) scroll_content: Vec<Size>,
}
```

Empty `Span` for nodes with no text → zero overhead. No wasted `Option`
slots.

### Layout write path

`leaf_content_size` already iterates `leaf_text_shapes`. Push each
shaped result onto `text_shapes` and record the `Span` per node:

```rust
let start = self.result.text_shapes.len() as u32;
for ts in leaf_text_shapes(tree, node) {
    let m = self.shape_text(...); // shape_text now pushes
    s = s.max(m);
}
let len = self.result.text_shapes.len() as u32 - start;
self.result.text_spans[node.index()] = Span { start, len };
```

### `TextMeasurer` cache key

The renderer-side `TextCacheKey` (`src/text/mod.rs`) is content-derived
(`text_hash`, `size_q`, `max_w_q`, `lh_q`) — already discriminates
different texts. **No change.**

The collision is in `TextMeasurer.reuse: HashMap<WidgetId, TextReuseEntry>`
(`src/text/mod.rs`) — two text shapes on the same `WidgetId` would
overwrite each other's `unbounded`/`wrap` slots. Fix: rekey to
`HashMap<(WidgetId, u8), TextReuseEntry>` (`u8` = within-node text
ordinal). `shape_text`'s caller already iterates with an index — just
thread it through to `shape_unbounded` / `shape_wrap`.

### Encoder

`emit_one_shape` is called by an outer loop that walks the node's
shapes in order. Track a within-node text counter and index the span:

```rust
let text_span = layout.text_spans[id.index()];
let mut text_i: u32 = 0;
// in the Text arm:
let shaped = layout.text_shapes[(text_span.start + text_i) as usize];
text_i += 1;
let base = match local_rect {
    None => owner_rect.deflated_by(padding),
    Some(lr) => Rect { min: owner_rect.min + lr.min, size: lr.size },
};
out.draw_text(align_text_in(base, shaped.measured, *align), *color, shaped.key);
```

### Add `local_rect: Option<Rect>` to `Shape::Text`

Mirror `Shape::RoundedRect`'s existing field (`src/shape.rs`).
Without it, multiple text shapes paint to the same owner inner rect
and overlap — useless. This is the change that makes the feature
useful, not just the assert removal.

Semantics (parallel to `RoundedRect`):

- **`None`** — today's behavior. Owner's arranged rect, deflated by
  the node's `padding`, with `align` positioning the glyph bbox
  inside.
- **`Some(lr)`** — owner-relative rect (`lr.min = (0, 0)` is owner
  top-left). Node `padding` is **skipped** (the user gave an explicit
  rect). `align` still positions glyphs *inside `lr`* (so
  `align: Center` centers the run within the user's rect, same as
  `align` does for `None`). Painted under owner clip but outside pan
  transform — identical to `RoundedRect { local_rect: Some, .. }`.

`Shape::is_noop` (`src/shape.rs:156`) gains a zero-area check for
`local_rect` symmetric with `RoundedRect`'s. Hash adds a
`Some/None` tag + `lr.hash` (mirror lines 90-96).

### MeasureCache snapshot

Two new columns in `MeasureCache`, paralleling existing patterns
(`src/layout/cache/mod.rs`):

- `text_spans: Vec<Span>` — parallel to `desired`, per-node, spans
  stored **subtree-local** (start relative to the snapshot's
  text-shape range start).
- `text_shapes_arena: LiveArena<ShapedText>` — variable-length,
  same pattern as `hugs`. Holds the snapshot's flat text-shape
  payload.

`SubtreeArenas` gains `text_spans: &[Span]` (parallel to `desired`)
and `text_shapes: &[ShapedText]` (variable-length, paralleling `hugs`).
`ArenaSnapshot` gains a `Span` for the text-shapes range
(paralleling `hugs`).

`LayoutResult.text_shapes` becomes a grow-during-measure Vec —
`resize_for` clears it (no pre-size). `text_spans` is sized to
`tree.records.len()` and zeroed.

Cache hit blit (in `LayoutEngine::measure`'s cache-hit arm,
`src/layout/mod.rs`):

```rust
let dest_start = self.result.text_shapes.len() as u32;
self.result.text_shapes.extend_from_slice(hit.arenas.text_shapes);
for i in 0..n {
    let s = hit.arenas.text_spans[i];
    self.result.text_spans[curr_start + i] = Span {
        start: dest_start + s.start,
        len: s.len,
    };
}
```

Cache write (in the bracket close): copy the subtree's
`text_spans[start..end]` straight (already subtree-local because the
measure pass writes spans relative to the per-frame `text_shapes`
buffer — re-base to subtree-local at write time by subtracting the
subtree's first text-shape index).

Compaction (`MeasureCache::compact`) gains the same `extend_from_slice`
loop the `hugs` arena already runs.

### Authoring guardrail

Drop `tip.has_text`. The new invariant — "spans align with shape
order" — is structurally enforced because layout writes in the same
iteration order the encoder reads.

## Cost

- Layout: unchanged for one-text (one push vs one index assignment).
- Encoder: one `u32` counter per node during the shape loop.
- Cache: one extra parallel column (`text_spans`) plus
  `LiveArena<ShapedText>`. Same memcpy pattern, slightly more
  bookkeeping.
- API: `Shape::Text` gains `local_rect`.
- Tests: 8 sites in `cross_driver_tests/{text_wrap,fill_propagation}.rs`
  read `text_shapes[node.index()]` directly. Add a
  `LayoutResult::first_text(node) -> Option<ShapedText>` helper and
  migrate all 8 in one place.
- `Shape::Text` constructors: 4 production sites
  (`widgets/{button,text,text_edit/mod}.rs` — one each) + 1 synthetic
  in `widgets/text_edit/tests.rs`. Mechanical `local_rect: None` add.

## Leaf-only invariant (unchanged)

Multi-text doesn't lift the existing leaf-only constraint
(`leaf_text_shapes` asserts `tree.records.end()[i] == i + 1`).
`Shape::Text` on a non-leaf is still UB — the `tip.has_text` assert's
replacement message should say *"on a leaf"* explicitly.

## When to ship

Trigger: the first custom widget that genuinely needs multiple text
shapes in one leaf. Until then, "open a child node per text" is the
workaround and the assert message points there.
