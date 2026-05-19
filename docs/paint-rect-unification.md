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

1. ✅ **Cascade computes tight text rects.** Extracted `align_text_in` to `forest/shapes/record.rs`, added `text_paint_bbox_local` helper. Cascade threads a text ordinal through its shape loop, looks up `ShapedText.measured` from `LayerLayout`, writes the tight rect into `shape_rects` + per-node paint-rect union. Deleted `paint_extents_local`'s `(precise, extent)` tuple — the text fudge is gone.
2. ✅ **Unified `Paint` column.** Added `Paint { screen, hash }` and replaced `Cascades::shape_rects` + `chrome_rects` with `paints: [Vec<Paint>; COUNT]` + `node_paints: [Vec<Span>; COUNT]`. Rewrote `compute_paint_rect` to emit ordered Paint rows (chrome at span row 0 when present, then shapes). Added `shape_to_paint` translation column for `paint_anims`.
3. ✅ **Old columns deleted.** `shape_rects`, `chrome_rects`, and `Tree.rollups.paints` bitset removed. Per-node `node_paints[i].len > 0` answers the "paints?" predicate.
4. ✅ **Damage diff collapsed.** `NodeSnapshot` collapsed from `{chrome_rect, chrome_hash, shape_span}` to one `paint_span`. `ShapeSnap` deleted (replaced by `Paint`). Renamed `shape_snaps*` → `paint_snaps*`. Replaced `push_decomposed_paint` / `push_changed_chrome` / `append_curr_shape_snaps` / `diff_changed_shape_leg` / `refresh_shape_rects_in_arena` with `append_curr_paints` / `diff_changed_paint_leg` / `refresh_paint_rects_in_arena`. The `chrome_hash != NodeHash::default()` chromedness predicate is gone — chrome is just paint row 0.

## Possible follow-ups (not yet done)

- **Encoder reads cascade's text rect** instead of recomputing via `align_text_in` in `emit_one_shape`. The cascade now writes the same rect — encoder can read it from `paints[span.start + paint_idx].screen` rather than recomputing `owner_rect.deflated_by(padding)` + `align_text_in(...)`. Eliminates the last duplicate formula site.
- **AoSoA-split `paints`** into `screen / hash` parallel `Vec`s if the per-shape memory bump (16 → ~40 B per row) shows up in profiling.

Critical files: `src/forest/shapes/record.rs`, `src/ui/cascade.rs`, `src/ui/damage/mod.rs`, `src/forest/rollups.rs`, `src/forest/tree/paint_anims.rs`, `src/renderer/frontend/encoder/mod.rs`.
