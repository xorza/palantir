# Paint-rect / damage unification

## Diagnosis: where the complexity lives

A shape's paint rect is computed **four times** in three coordinate systems, with text and chrome carved out as special cases each time:

1. **Lowering** (`src/forest/shapes/record.rs`): `Polyline`/`Curve`/`Mesh` stash an owner-local `bbox`. `RoundedRect`/`Image`/`Mesh`/`Shadow` don't — they rely on `local_rect` or implicit "owner full". `Text` can't (not shaped yet).
2. **Cascade per-shape**: `ShapeRecord::paint_extents_local` → `paint_bbox_local` returns a `(precise, extent)` tuple. The tuple exists **purely so `Text` can lie** — `Text { local_origin: Some(_) }` returns `Size::ZERO` for `precise` then secretly inflates `extent` to the full owner rect (`record.rs:464-471`) so the union isn't empty.
3. **Cascade per-node**: `compute_paint_rect` (`src/ui/cascade.rs:507-630`) runs **two parallel transform pipelines** — chrome through `parent_transform`, shapes through `parent.compose(self_anchored)` — and unions in screen space. Writes per-shape `shape_rects` + per-node `chrome_rects` as two separate columns.
4. **Encoder per-shape**: `Text { local_origin: None }`'s actual paint rect is computed *nowhere except the encoder* — `owner.deflated_by(padding)` then `align_text_in(...)`. Damage's text rect is always wider than the painted glyphs.

The downstream damage code then has to bridge per-shape and per-node columns via a third arena (`ShapeSnap` + `compact_shape_snaps`), with separate diff arms (`push_decomposed_paint`, `push_changed_chrome`, `diff_changed_shape_leg`, `refresh_shape_rects_in_arena`) and a special `chrome_hash != default` predicate that has to stay in sync with `chrome_rect.area() > 0` and `rollups.paints` bitset — three columns answering the same conceptual question. **This three-leg weave is where most of the recent code, and most of the small bugs, live.**

## Proposal: one `Paint` column per layer

```rust
struct Paint {
    owner_local: Rect,
    screen:      Rect,
    content_hash: NodeHash,
}

struct Cascades {
    paints: [Vec<Paint>; Layer::COUNT],
    node_paint_spans: [Vec<Span>; Layer::COUNT], // empty span = paints nothing
    subtree_paint_rects: [Vec<Rect>; Layer::COUNT], // unioned over span
    // ...
}
```

Key moves:

- **Chrome becomes row 0** of a node's paint span when present — same data shape as shapes, just positioned. The two-transforms split (chrome in parent space, body in self-anchored) is preserved by computing each row's `screen` with the appropriate matrix, not by a separate column.
- **`local_rect` is mandatory** — `None` resolved to owner-full in the cascade's one already-existing per-shape loop.
- **`Text` becomes symmetric**: cascade reads `shaped.measured` (already at `layout.text_shapes[span.start+ord]`) and writes the tight `align_text_in(...)` rect into `paints` once. Encoder reads that rect instead of recomputing. The `(precise, extent)` tuple dies.
- Damage collapses to one loop: `diff_paint_spans(prev: &[Paint], curr: &[Paint]) -> Vec<Rect>`. `ShapeSnap` ≡ `Paint`. The chrome predicate, the `paints` bitset, and `refresh_shape_rects_in_arena` all delete.

## Tradeoffs

|                                  | Today                                                          | Proposal                       |
| -------------------------------- | -------------------------------------------------------------- | ------------------------------ |
| Ways to ask "what's the paint rect" | 4 sites, 3 coord systems                                    | 1 column                       |
| Damage diff arms                 | per-node chrome + per-shape + snap-arena bridge                | one slice diff                 |
| Text damage accuracy             | over-covers by `padding + slack`                               | matches glyphs                 |
| Encoder                          | chrome-before-clip exception + `align_text_in` inside paint loop | ordered paint span; chrome is row 0 |
| Memory                           | per-shape `shape_rects` (16B) + per-node `chrome_rects` (16B) + `ShapeSnap` (24B) | `Paint` (~40B) per row; mitigate with AoSoA split |

**Preserved load-bearing properties:** shape-layout decoupling (cascade still the producer, layout untouched), two-transform body/chrome split, per-shape damage decomposition (now natural, not special-cased), SoA + per-frame arena posture.

**Real downside:** the unified row is wider per painted shape than today's slot; mitigate by SoA-splitting `paints` into `screen / local / hash` parallel `Vec`s (same argument as `Soa<EntryRow>`).

## Migration steps

1. **Cascade gains a text-ordinal counter; reads `shaped.measured` from `LayerLayout` to compute text's true rect once.** Pure refactor — same data flowing into the same columns, no observable behavior change yet. Establishes that cascade can compute the tight text rect.
2. Rewrite `compute_paint_rect` to emit `Vec<Paint>` per-node span; chrome = row 0; drop the `(precise, extent)` tuple.
3. Replace `shape_rects` + `chrome_rects` with `paints[layer]` + `node_paint_spans[layer]`. Re-key `tree.paint_anims.by_shape` to paint-idx (or thread a translation table for one PR).
4. Collapse the four damage diff functions into one `diff_paint_spans`.
5. Encoder reads `paints[span.start].screen` for text instead of recomputing via `align_text_in`.
6. Delete `paint_extents_local`, `chrome_rects`, `shape_rects`, `chrome_hash` snap field, `rollups.paints` bitset, chrome predicate logic.

Critical files: `src/forest/shapes/record.rs`, `src/ui/cascade.rs`, `src/ui/damage/mod.rs`, `src/renderer/frontend/encoder/mod.rs`, `src/layout/mod.rs`.

Each step compiles standalone, so this can land in 2–3 PRs with the old and new columns coexisting briefly.
