# Replaying `atlas-review` onto master

The full plan, with each commit's scope and source, is on the `atlas-review` branch:
`git show atlas-review:.notes/atlas-review-split.md`. Each change is applied on master, reviewed,
gated, then committed and pushed on request. 10–14 landed as one commit, and the order test
passed there, so K drops: 39 commits, 10 applied and 29 left, fewer where a Phase 2 split cannot
build alone and merges with its neighbor.

## Done

| # | commit | change |
| --- | --- | --- |
| 1 | `0d7070f23` | Name the gradient row texture `RowTexture` |
| 2 | `d5b9f290f` | Name the CPU gradient atlas the row table |
| 3 | `ec96ee454` | Move the eviction clock to `common` |
| 4 | `77edaa4d7` | Move gpu leaf files to the modules that use them |
| 5 | `961feb6db` | Move `TargetSpec`/`TargetFormat` into `texel`, `FrameTiming` into `gpu_timings` |
| 6 | `6700319d2` | Move the backdrop group types to the effect pipeline (no `CompositeBound` yet: it comes with I) |
| 7 | `da06a5308` | Move `AtlasSlots` to `pipeline::atlas_binding`, `AtlasSlots::of(&pool)` |
| 8 | `9390184b3` | Name the texture binding's layouts and groups alike |
| 9 | `df48dc781` | Name the shelf packer's side its edge |
| 10–14 | uncommitted | The memory report's columns, one seeded RNG, `PerFormat<T>` and `Tenant::ALL`, views handed down, the gpu module-order test |

## Left

Phase 2, behavior:

- A1–A6. Decay every candidate and every round
- B1–B3. Give the atlas pool one surface
- C1–C7. One tenant protocol
- D. Put the gradient row table under the atlas pool
- E1–E2. Age gradient rows on display frames
- F1–F4. Decay and purge the gradient rows with the pool
- G. Bind the mask and color pages through one atlas group
- H. Build the spun raster pipelines only for a frame that turns a raster
- I1–I2. One shared page read and one sampler
- J. Keep a glyph's template as its offset and its raster's handle
- L. The review document

Then Darkroom's `palantir` pointer moves to the new tip in Darkroom's own commit.
