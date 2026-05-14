# Rollup-hash packing follow-up

`SubtreeRollups` hashes a handful of per-node structs every frame. Most of those structs are still authored as "hash each field individually" — N small `Hasher::write_*` calls per node where one `state.write(bytemuck::bytes_of(self))` would do the same work. With FxHash each `write_*` call costs a mul + xor on top of the byte fold; collapsing N writes into 1 saves N−1 muls per node, and at 1k+ nodes/frame that adds up.

Same recipe used for `Corners` / `Spacing` (one `write_u64` instead of four `write_u16`s) applied at the struct level.

## Candidates

| impl | location | writes / node today | shape |
|---|---|---|---|
| `LayoutCore::hash` | `src/forest/element/mod.rs:267` | 6 (mode, size, padding, margin, align, visibility) | per-field hash |
| `BoundsExtras::hash` | `src/forest/element/mod.rs:179` | 4 (position raw bytes, grid, min_size, max_size) | mixed raw + field |
| `PanelExtras::hash` | `src/forest/element/mod.rs:193` | 4 (gap u32, line_gap u32, child_align, justify) | small scalars |
| `ShapeRecord::hash` | `src/forest/shapes/record.rs:220` | varies per variant | per-shape, per-frame |

`LayoutCore` is the most consumed (every node, every frame, feeds the cross-frame `MeasureCache` key via the subtree rollup). Best ROI.

## Path

For each type:

1. Make it `#[repr(C)]` + `bytemuck::Pod` + `Zeroable`. Replace any enum field with a `#[repr(u8)]` enum (or a `u8` discriminant field). Use `#[padding_struct::padding_struct]` (already in deps, see `DrawPolylinePayload` etc.) to fill any internal/trailing padding so the no-padding-bytes Pod invariant holds.
2. Replace the manual `Hash` body with:

   ```rust
   #[inline]
   fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
       state.write(bytemuck::bytes_of(self));
   }
   ```

3. Mirror the changes the rollup hasher expects — if any field was historically *excluded* from the hash (e.g. `BoundsExtras::position` and `PanelExtras::transform` are intentionally omitted so a parent's translation doesn't dirty its own node hash, with the transform folded into the subtree hash separately by `Tree::compute_hashes`), split the type:
   - Hot, hashed-as-bytes struct holds the fields that *are* part of the per-node hash.
   - A sibling sparse column (or extra field on the existing one) holds the excluded fields.

   This is the bigger surgery — without it, byte hashing would over-invalidate. `BoundsExtras` is the textbook case (its `position` is in the struct but not the hash today).

## Order

1. **`LayoutCore`** — `mode`, `align`, `visibility` are all enum-shaped today; biggest win and the cleanest reshape since no field is currently excluded from the hash.
2. **`ShapeRecord`** — per-variant; inspect first, may already be mostly Pod-friendly (most variants are flat numeric payloads).
3. **`PanelExtras`** — small struct, four scalars, easy.
4. **`BoundsExtras`** — needs the field-split surgery (position is excluded from hash). Lowest ROI per unit work.

## Out of scope

- The full per-frame `subtree_hash` finalization itself (`SubtreeRollups::roll_up`) — separate concern, not about per-field write counts.
- `WidgetId`, `Sizes`, etc. — already packed and use single-word writes.
- `Spacing`, `Corners` — already done in the current pass (`src/primitives/spacing.rs`, `src/primitives/corners.rs`).

## Why now or not

Each item here is a 30–200 line change with one test-suite re-baseline. Worth doing when measure-pass profiling lands `LayoutCore::hash` in the top-N self-time, which the most recent frame bench did *not* show (the post-Spacing run came in within noise). Park as opportunistic — pick up the next time the hash path shows up in a profile, or roll into a broader "Pod-ify the per-node columns" pass.
