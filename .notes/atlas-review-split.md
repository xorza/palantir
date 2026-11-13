# Replaying `atlas-review` onto master

The full plan, with each commit's scope and source, is on the `atlas-review` branch:
`git show atlas-review:.notes/atlas-review-split.md`. Each change is applied on master, reviewed,
gated, then committed and pushed on request. 43 commits (the order test is either 14 or K); 8
are applied and 35 are left, fewer where a Phase 2 split cannot build alone and merges with its
neighbor.

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
| 8 | uncommitted | Name the texture binding's layouts and groups alike |

## Left

Phase 1, moves and renames:

- 9. Name the shelf packer's side its edge

Phase 1b, preparation with no behavior change:

- 10. Line the memory report's rows up under its heading
- 11. Draw the atlas tests' randomness from one seeded RNG
- 12. `PerFormat<T>`, `AtlasFormat::COUNT`, `Tenant::ALL`
- 13. Hand a lower gpu layer a view, not its owner's type
- 14. The gpu module-order test, if it passes this early (else K)

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
- K. The order test, if 14 could not go early
- L. The review document

Then Darkroom's `palantir` pointer moves to the new tip in Darkroom's own commit.
