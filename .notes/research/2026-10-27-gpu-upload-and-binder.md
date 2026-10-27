# GPU upload copies and the pass binder (2026-10-27)

Rig: AMD Radeon Graphics (RADV REMBRANDT), Vulkan, Mesa 26.2.5. Bench
profile (fat LTO), `taskset -c 2 setarch -R`, governor `performance`.

## A copy against a staged gradient row

`atomic/gradient_upload/gpu/<split|merged>/gap<g>`: 32 dirty LUT rows (2 KiB
each) `g` clean rows apart, staged in the belt and copied into an
`Rgba16Float` texture, submitted and waited for.

| gap | split (32 copies) | merged (1 copy) |
| --: | ----------------: | --------------: |
|   1 |           1.11 ms |          0.43 ms |
|   8 |           1.12 ms |          0.52 ms |
|  16 |           1.09 ms |          0.63 ms |
|  32 |           1.11 ms |          0.87 ms |
|  64 |           1.08 ms |          1.29 ms |
| 128 |           1.11 ms |          1.86 ms |

- A copy costs about 22 µs ((1.10 − 0.40) ms over 31 copies); a staged gap
  row about 0.42 µs ((1.29 − 0.87) ms over 31 × 32 rows).
- Merged and split cross at a gap of about 52 rows.

## The pass binder

`atomic/record_pass/gpu/*`, HEAD (`Bound` enum, every bind re-sets the
pipeline's groups and buffers) against the binder that sets only changed
state. ABBA order; a change counts only where both comparisons mirror it.

| case             | B against A | A against B | verdict      |
| ---------------- | ----------: | ----------: | ------------ |
| text/per_group   |     −19.1 % |     +21.7 % | −19 %, real  |
| groups/per_item  |      −9.3 % |      −3.2 % | not mirrored |
| groups/single    |      +2.8 % |      +1.6 % | no change    |
| images/distinct  |      −0.5 % |      +7.0 % | no change    |
| images/shared    |      −4.0 % |      +8.1 % | no change    |
| text/single      |      −5.2 % |      −6.4 % | no change    |

- `text/per_group` gains because each text batch re-bound the atlas group and
  vertex buffer; the binder sets them once.
- No workload switches quad forms, the case the binder was written for, so
  that gain is unmeasured.

## One tile index

`RectGrid` and `OcclusionPruner` on one `TileIndex` (inline rows plus
overflow chains), against each keeping its own: the pruner's chains were
newest-first and stopped at the first cover beneath a quad, the shared index
skips such covers instead. ABBA, mirrored changes only.

| case                                     | B against A | A against B |
| ---------------------------------------- | ----------: | ----------: |
| rect_grid/realistic/64                   |      +0.7 % |      −1.2 % |
| rect_grid/realistic/200                  |      +1.2 % |      −0.9 % |
| higher_kind_overlap/same_tier_image/256  |      +2.1 % |      −1.0 % |
| higher_kind_overlap/text_between_mesh_over/256 | +1.4 % |   −1.6 % |
| frontend/repeated_chrome/gradient        |      −2.6 % |      +5.0 % |

- The first shared build ran `rect_grid/realistic` 32 % slower: the walk went
  through `Iterator::any` closures that did not inline. Plain loops and
  `#[inline(always)]` on `span`, `insert` and `any_over` closed it.
- What stays is about 1 % on the grid paths, within the several-percent drift
  `benches/AGENTS.md` notes; the pruner's frame gains about 3 %.
