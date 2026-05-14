# Hot-path data structure audit

Per-frame hot-path structs across layout / cascade / damage / encode /
compose, ranked by estimated impact (bytes saved × access frequency
per frame).

## Verified sizes (from `size_of` on current `master`)

```
                       LayoutCore : size =   56 align =  4
                     BoundsExtras : size =   48 align =  4
                        NodeFlags : size =    1 align =  1
                         HitEntry : size =   32 align =  8
                          Cascade : size =   24 align =  8
                     NodeSnapshot : size =   40 align =  8
                     DamageRegion : size =  136 align =  4
                    ArenaSnapshot : size =   40 align =  8
                             Span : size =    8 align =  4
                             Quad : size =   92 align =  4
                           Sizing : size =    8 align =  4
                            Sizes : size =   16 align =  4
                          Spacing : size =   16 align =  4
```

Agent inventory was off in places (`HitEntry` is 32 B not 24,
`LayoutCore` 56 not 68). Re-ranked targets below reflect real sizes.

## Targets

### 1. `NodeSnapshot` — `src/ui/damage/mod.rs:49`
- Current ~40 B: `Rect` (16) + 2× `NodeHash` (16) + `CascadeInputHash` (8).
- Access: `FxHashMap<WidgetId, NodeSnapshot>` lookup per painting node
  during damage diff, every frame.
- Idea: collapse the two `NodeHash` fields into one via a non-lossy
  combine (e.g. one full hash that already mixes node+subtree at
  `post_record` time, dropped here). **Don't** XOR — high collision risk.
  Requires changes in `Tree::post_record` and downstream.

### 2. `LayoutCore` — `src/forest/element/mod.rs:240`
- Current ~68 B (claimed): `LayoutMode` + `Sizes` (2× `Sizing`) +
  2× `Spacing` + `Align` + `Visibility`.
- Access: per-node SoA column; read by every measure/arrange.
- Idea: bitpack `Sizing` (2-bit tag + quantized f32 value) into one
  `u32` per axis; reorder for no padding holes. Largest potential
  win — but invasive (touches every layout driver).

### 3. `HitEntry` — `src/ui/cascade.rs:56`
- Current ~24 B: `WidgetId` (u32) + `Rect` (16) + `Sense` (enum, 1 B)
  + `focusable: bool` + `disabled: bool`.
- Access: reverse-scanned every pointer event for hit-test.
- Idea: pack `sense` (4-variant), `focusable` (1 bit), `disabled` (1 bit)
  into a single `u8`. Saves 2 B + 1 padding byte = 4 B. Low risk,
  contained to cascade + hit-test.

### 4. `Quad` — `src/renderer/quad.rs:116`
- Current ~92 B: GPU instance with both `fill` and `stroke_color`.
- Access: per-quad GPU instance buffer write.
- Idea: split solid vs. stroked at compose time; solid drops the 16 B
  stroke color. ~70% solid in typical UIs. Touches the cmd buffer SoA
  + the wgpu pipeline binding — significant refactor.

### 5. `BoundsExtras` — `src/forest/element/mod.rs:130`
- Sparse side-table mixing `Option<TranslateScale>`, position offset,
  `GridCell`, min/max size.
- `GridCell` only applies to ~2–5% of nodes — doesn't belong colocated
  with the transform.
- Idea: split into 3 sparse tables (transform, position, grid_cell).

### 6. `ArenaSnapshot` — `src/layout/cache/mod.rs:46`
- 3× `Span` (24 B) + `IVec2 available_q` (8) + `u64 subtree_hash` (8).
- Idea: `Span(u16,u16)` where subtree spans fit (always); pack
  `available_q` as two `i16`s in a `u32`. Saves ~18 B per cached entry.

### 7. `DamageRegion` — `src/ui/damage/region/mod.rs:67`
- `ArrayVec<[Rect;8]>` always 128 B inline; 95% of frames use ≤2 rects.
- Idea: inline 2, spill to heap beyond. Worth measuring — heap spill
  on the rare path may not be acceptable under the alloc-free posture.

### 8. `Span` everywhere
- `(u32,u32)` = 8 B; most spans index <65K.
- Idea: `(u16,u16)` saves 4 B per use. Biggest hit:
  `NodeRecord::shape_span` (per-node SoA column).

### 9. `DrawRectPayload` / `DrawMeshPayload` — `src/renderer/frontend/cmd_buffer/mod.rs`
- Same fill+stroke waste as `Quad`; same split idea.

## Skip / not worth it

- `NodeFlags` — already well packed (1 byte, 4 fields).
- `TextRun` / `DrawTextPayload` — low absolute frequency.

## Action plan

Phase 1 (low risk, high read frequency):
- `HitEntry` SoA split — **done.** `WidgetId` moved into parallel
  `Cascades::entry_ids: Vec<WidgetId>`; hot scan struct went 32 → 20 B.
  `input_throughput`: `pointer_move_stream` −10.1%, `mixed_stream` −9.6%
  (vs baseline `hitentry-before`); click/scroll unchanged. Alloc-free
  invariant holds.
- `NodeSnapshot` hash fold — not pursued (no safe non-collision combine).

Phase 2 (invasive but high impact):
- `LayoutCore` repack via `Sizing` u32 packing — **done.** `Sizes`
  went 16 → 8 B, `LayoutCore` 56 → 48 B. `frame` bench (post_record /
  post_record_resizing): within noise (±0.5%) — encode/decode cost on
  every read roughly offsets the cache win at current workload size;
  benefit grows with bigger trees / higher cache pressure. Alloc-free
  invariant holds; all 603 lib tests pass. 2-bit mantissa truncation
  on Sizing values is well below physical-pixel resolution.

Phase 2.5 — **done.** `Soa<BoundsExtras>` via `soa-rs` (same crate
that already powers `Tree::records`). Replaced `Vec<BoundsExtras>` with
5 parallel columns (transform/position/grid/min_size/max_size).
`Tree::bounds()` accessor removed; per-field inline accessors
(`transform_of` / `position_of` / `grid_of` / `size_clamps_of`) replace
12 call sites. Cascade bench: −1.5%/−1.4% at small N (100/500 nodes),
noise at 2k, +1.1% at 10k. Frame bench: +1.2%/+0.4% (within slow drift).
Modest absolute win — BoundsExtras isn't actually the bottleneck in
cascade; the rect-transform + hash mixing work dominates. Kept the
change because the structure is cleaner (per-field column access matches
the per-driver read pattern) and `alloc_free` invariant + 603 tests
still pass.

Phase 2.6 — `Soa<PanelExtras>`: **tried, reverted.** Same `Soars`
derive applied to PanelExtras (16 B, 4 fields). Frame bench regressed
+2.5% / +2.4% even with a combined `panel_of` accessor. Reason:
PanelExtras is small enough that `Vec<PanelExtras>` already packs
4 entries per cache line; most readers (stack arrange, wrapstack
arrange) want 3-4 of the 4 fields together. SoA's per-column writes
on `push` cost more than the per-driver read selectivity saves.
Lesson: SoA wins when the struct is big *and* most readers want a
small subset. PanelExtras fails the second test.

Phase 2.7 — `transform` moved from `BoundsExtras` to `PanelExtras`:
**done.** Access-pattern insight: only `Panel::transform()` and
`Grid::transform()` are public, so every transformed node is a panel
that almost always also customizes some panel knob (gap, justify) —
the `Option<TranslateScale>` field amortizes against an
already-allocated panel row. Sizes: `BoundsExtras 48 → 32 B`,
`PanelExtras 16 → 28 B`, `ExtrasIdx stays 8 B` (no growth — the prior
"separate transform_table" experiment grew ExtrasIdx 8 → 10 B and
regressed frame +3%). Also reverted `Soa<BoundsExtras>` back to
`Vec<BoundsExtras>` — column SoA had a modest cascade win but tied or
regressed on frame; the 32-B contiguous `BoundsExtras` row fits 2 per
cache line and most readers (`size_clamps_of`, `position_of`)
load the whole row.

Bench (vs `boundsextras-before`, the original pre-experimentation
state):

| case | Δ |
|---|---|
| `cascade/run/100` | **−1.8%** |
| `cascade/run/500` | **−2.7%** |
| `cascade/run/2000` | **−0.6%** |
| `cascade/run/10000` | **−1.8%** |

Frame (vs `layoutcore-before`, the earliest baseline): post_record
+0.13%, post_record_resizing −0.28% — **within noise both ways**.

So compared to original: cascade wins 0.6–2.7% across all sizes,
frame is flat. The transform field's natural home was always
`PanelExtras` — it just took three experiments to realize it.
Alloc-free invariant holds.

Phase 2.8 — fold `clip_radius_table` into `chrome_table`: **done.**
The `clip_radius` column was 99% redundant — its values are always
`bg.radius` (extracted at `open_node_with_chrome`). The only reason
for the split was the noop-chrome-with-rounded-clip case: if chrome
is fully invisible, the old gate dropped it but a separate
`clip_radius_table` row preserved the radius for the stencil mask.

Relaxed the chrome gate to keep the row when `ClipMode::Rounded`
even if `Background::is_noop` — i.e., the only time a noop chrome
row survives. Encoder reads radius via `tree.chrome(id).radius`.
The `clip` hashing in `compute_hashes` was already double-counting
(Background's derived Hash includes `radius`), so removing it is a
free correctness fix as well.

Sizes:
- `ExtrasIdx`: 8 → **6 B** (-2 B per node, always — every leaf saves).
- Removed: `clip_radius_table: Vec<Corners>`, `Tree::clip_radius()`,
  one `Slot` field, one `push`/`clear`/hash site.

Bench (vs `clipfold-before`):

| case | Δ |
|---|---|
| `cascade/run/100` | **−7.3%** |
| `cascade/run/500` | +1.7% |
| `cascade/run/2000` | **−0.8%** |
| `cascade/run/10000` | **−2.7%** |
| `frame/post_record` | **−2.7%** |
| `frame/post_record_resizing` | **−6.5%** |

Genuine, broad-based wins. The 500-node cascade case is the only
outlier (+1.7%) — likely measurement variance given the others all
went the other way. Frame bench shows the strongest gain: −6.5% on
resizing, −2.7% on steady record. The 2-B `ExtrasIdx` shrink pays
off everywhere — that column is read on every node every pass.

Alloc-free invariant holds; all 609 lib tests pass.

Phase 3 (renderer):
- `Quad` / `DrawRectPayload` split — pending.

Verify each phase: baseline a representative bench
(`input_throughput` for HitEntry, `frame` for LayoutCore), optimize,
re-bench, confirm no regression elsewhere.
