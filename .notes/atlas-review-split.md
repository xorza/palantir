# Splitting `atlas-review` into readable commits

`atlas-review` is one commit on `master` (`7f38f3be9`): `9ed23212f` "Rework the atlas pool, its
tenants, bindings and gradient rows", 186 files, +6584 −5235. It is the squash of eleven local
commits that the reflog still holds, `9f7fda182..b10e4130f`; `git diff --quiet b10e4130f 9ed23212f`
exits 0, so their tip is the same tree. They are the review's batches, each a mix of moves, renames
and behavior:

| pre-squash | subject | files | ± |
| --- | --- | --- | --- |
| `9f7fda182` | Review: a proposal and a plan (`.notes/review-atlas.md`) | 1 | +543 |
| `cd8955d89` | Decay every candidate and every round | 12 | +752 −270 |
| `f3fd32fe0` | Give the atlas pool one surface | 36 | +943 −817 |
| `a00e4274b` | One tenant protocol | 32 | +1315 −1046 |
| `2948d198c` | Put the gradient row table under the atlas pool | 50 | +545 −584 |
| `7b1c7238d` | Age gradient rows on display frames | 27 | +771 −841 |
| `77c7a81bd` | Decay and purge the gradient rows with the pool, 16 rows | 48 | +866 −999 |
| `9bd1bf298` | Bind the mask and color pages through one atlas group | 19 | +374 −347 |
| `981d6d97f` | Build the spun raster pipelines only for a frame that turns a raster | 11 | +146 −72 |
| `06d240382` | Read the atlas pages through one shared include | 23 | +277 −282 |
| `dda3229c4` | Keep a glyph's template as its offset and its raster's handle | 8 | +85 −134 |
| `5bffb5ffb` | Give gpu's submodules an order, move the leaf types down | 88 | +524 −402 |
| `b10e4130f` | Say what the moved gpu modules hold | 4 | +10 −8 |

The pre-squash commits carry a `Claude-Session` trailer; the split's commits carry none.

## Proposal

- **Moves and renames first, as their own commits, with no behavior in them.** Today the largest
  diffs are inflated by them: `2948d198c` is 15 file renames plus a type rename under the ownership
  change, `5bffb5ffb` is 88 files of which about 75 only rewrite a `use` path, and the
  `gradient atlas → gradient rows` wording sits in `77c7a81bd`. Hoisted, each later commit shows
  only what it changes.
- **Every moved file is untouched by the batches before its move.** The blobs of `gpu_ctx.rs`,
  `map_state.rs`, `pipeline_recipe.rs`, `shader_body/`, `backbuffer_out.rs`,
  `wgpu_effect_store.rs`, `owned_target/mod.rs`, `render_target.rs` and `texel.rs` are the same at
  `7f38f3be9` and at every pre-squash commit up to `dda3229c4`, so hoisting their moves changes no
  later hunk but its `use` lines. Two moved items do change later, and their later edits land in
  the new file: `CompositeGroups`/`BackdropGroups` (edited in `a00e4274b` and `06d240382`) and
  `AtlasSlots` (beside `page_slots.rs`, whose tests `cd8955d89` extends).
- **Then the no-behavior preparation** (a test fixture, an indexing type, the decoupling a module
  order needs), **then behavior in the batches' order**, each batch split where its bullets are
  separate changes. A later commit that corrects an earlier one in the same series is folded into
  it: "a build moves the pool to its submit's frame only where it paints" (`77c7a81bd`) fixes the
  plumbing `7b1c7238d` adds.
- **The review document lands once, last**, at its final content. Its per-batch "Changed" and
  "Declined" notes churn in every pre-squash commit and add noise to each diff.
- **One fix rides along:** `src/gpu/wgpu_backend/mod.rs`'s `internals` module has `#[cfg(test)]`
  twice over the `AtlasCounts` import, from `7f38f3be9`; commit 4 edits that import block and drops
  the duplicate. So the split's tip differs from `9ed23212f` by that one line and nothing else.

## How to build it

1. **Keep the pre-squash commits reachable** before anything else: `git branch
   atlas-review-presquash b10e4130f`. They live only in the reflog, which `git gc` prunes.
2. **Work on a new branch** from `7f38f3be9`, `atlas-review-split`; `atlas-review` stays as it is
   until the split is reviewed.
3. **Phase 1 commits are made by hand**: `git mv`, path and identifier rewrites, `cargo fmt`.
4. **Each later commit is rebuilt from a pre-squash tree, not cherry-picked.** For pre-squash
   commit `C`: `git checkout C -- .` (and remove the files `C` lacks), re-apply the Phase 1
   transform (the file moves, the path and name rewrites below, the item moves, fixing the import
   lists `cargo check` names), `cargo fmt`, then stage only the hunks of the commit being built
   (`git add -p`) when `C` is split into several. A cherry-pick conflicts on almost every `use`
   block, and a hand resolution there is where a slip hides.
5. **Each commit builds and its tests pass**: `cargo gate-lint` and a filtered `cargo gate-test`
   over the modules it touches. A Phase 2 split that does not build alone is merged with its
   neighbor rather than patched into building.
6. **At the tip, `git diff 9ed23212f` shows only the duplicate `#[cfg(test)]`**, then the full
   gate runs once, with `cargo visual` as the batches change shaders.
7. **Then** the branch replaces `atlas-review` (a force push, or a new remote branch, your call),
   and Darkroom's `palantir` pointer moves to the new tip in Darkroom's own commit.

The rewrites the Phase 1 transform applies, in this order (whole words; the first two in this
order, because `RowTable` names the texture before commit 1 and the CPU table after commit 2):

| from | to |
| --- | --- |
| `gpu::atlas::row_table`, `RowTable` (the texture) | `gpu::atlas::row_texture`, `RowTexture` |
| `renderer::cpu_gradient_atlas`, `CpuGradientAtlas`, `GradientAtlasCounters` | `renderer::row_table`, `RowTable`, `RowTableCounters` |
| `INITIAL_ATLAS_ROWS`, `DEFAULT_MAX_ATLAS_ROWS`, `MAX_ATLAS_ROWS` | `INITIAL_ROWS`, `DEFAULT_MAX_ROWS`, `MAX_ROWS` |
| `gpu::atlas::eviction_clock` | `common::eviction_clock` |
| `gpu::frame::gpu_ctx`, `gpu::surface::map_state` | `gpu::resource::gpu_ctx`, `gpu::resource::map_state` |
| `gpu::pipeline::pipeline_recipe`, `gpu::pipeline::shader_body` | `gpu::resource::…` (and `include_str!("prelude.wgsl")`) |
| `gpu::surface::backbuffer_out` | `gpu::device::backbuffer_out` |
| `gpu::resource::wgpu_effect_store` | `gpu::pipeline::effect_pipeline::wgpu_effect_store` |
| `TextureBinding::{frame, backdrops, layout, bind_group}` | `{frame_layout, backdrop_layout, texture_layout, texture_group}` |
| `ShelfPacker::{size, MAX_SIZE, shelf_height}`, `ShelfPacker::takes` | `{edge, MAX_EDGE, shelf}`, `Shelf::takes` |

## Commits

Sources name the pre-squash commit each part comes from.

### Phase 1: moves and renames

1. **Name the gradient row texture `RowTexture`.** `gpu/atlas/row_table.rs` → `row_texture.rs`,
   `RowTable` → `RowTexture`, its callers. Frees the name for commit 2. (`2948d198c`)
2. **Name the CPU gradient atlas the row table.** `renderer/cpu_gradient_atlas/` →
   `renderer/row_table/`, `CpuGradientAtlas` → `RowTable`, `GradientAtlasCounters` →
   `RowTableCounters`, the three `*_ATLAS_ROWS` constants, the bench registration in
   `bench/mod.rs`, and "gradient atlas" → "gradient rows" in the docs and WGSL comments
   (`effect/contract.rs`, `primitives/wgsl/{frame,gradient_lut}.wgsl`, `lut_row.rs`,
   `quad_draw_row.rs`, `gpu_fill.rs`, `texture_binding.rs`, `resource/mod.rs`, the curve and
   effect pipelines). Bench ids stay. (`2948d198c`, `77c7a81bd`)
3. **Move the eviction clock to `common`**, so the row table can turn one: `gpu/atlas/
   eviction_clock.rs` → `common/eviction_clock.rs`, `pub(super)` → `pub(crate)`, its module doc.
   `GRACE_FRAMES` stays for commit E2. (`7b1c7238d`)
4. **Move gpu's leaf files down to the layers that share them.** `GpuCtx` (`frame` → `resource`),
   `MapState` (`surface` → `resource`), the pipeline recipe and the shader body with
   `prelude.wgsl` (`pipeline` → `resource`), `BackbufferOut` (`surface` → `device`), the effect
   store (`resource` → `pipeline::effect_pipeline`); the module docs of `device`, `frame`,
   `pipeline` and `resource` say what they hold; the duplicate `#[cfg(test)]`. Only paths change.
   (`5bffb5ffb`, `b10e4130f`)
5. **Move `TargetSpec` and `TargetFormat` into `texel`, `FrameTiming` into `frame::gpu_timings`.**
   Cut and paste, the `lib.rs` and `internals` re-exports' paths, and the orphaned doc line above
   `FrameTiming` goes. The public name `TargetFormat` is unchanged, so `.notes/API_SURFACE.md`
   holds. (`5bffb5ffb`)
6. **Move what a backdrop's composite binds beside the effect pipeline that reads it.**
   `ResultTexture` (from `backdrop_blur`) and `CompositeGroups`, `CompositeBound`,
   `BackdropGroups` (from `backdrops`) into `pipeline/effect_pipeline/backdrop_groups.rs`, their
   fields `pub(crate)` as they leave the module. `CompositeBound` arrives with `06d240382`; until
   then the file holds the rest. (`5bffb5ffb`)
7. **Move `AtlasSlots` to `pipeline::atlas_binding`, beside the pipelines that are built for it**:
   `AtlasPool::slots()` becomes `AtlasSlots::of(&pool)`, the one adaptation a move out of `atlas`
   needs. The file gains `AtlasGroup` in G. (`9bd1bf298`)
8. **Name the texture binding's layouts and groups alike**: `frame_layout`, `backdrop_layout`,
   `texture_layout`, `texture_group`. The new `backdrop_group`, `sampler` and `storage_entry` stay
   in I2. (`06d240382`)
9. **Name the shelf packer's side its edge**: `size` → `edge`, `MAX_SIZE` → `MAX_EDGE`,
   `shelf_height(y)` → `shelf(y)` returning the `Shelf`, `takes` a `Shelf` method. (`f3fd32fe0`)

### Phase 1b: preparation with no behavior change

10. **Line the memory report's rows up under its heading**: the names' column as wide as its widest
    name, with its test. A fix of its own: the half-float row sat six columns right of its heading.
    (`f3fd32fe0`)
11. **Draw the atlas tests' randomness from one seeded RNG**: `internals/seeded_rng.rs` and the
    tests that draw from it. (`f3fd32fe0`)
12. **Index a value a format by `AtlasFormat`**: `PerFormat<T>`, `AtlasFormat::COUNT`,
    `Tenant::ALL`, with their tests, and the positional `[_; 3]` tables and `format as usize`
    turned over to it. (`f3fd32fe0`)
13. **Hand a lower gpu layer a view, not its owner's type**: `FrameGroup` takes a `RowView`,
    `FrameBackbuffer` a view and a size, `EffectDevice` its `Gpu` at each build rather than a
    stored clone. (`5bffb5ffb`)
14. **Give gpu's submodules an order, held by the order test**: `internals::module_order` and
    `src/gpu/AGENTS.md`, "Module order". Only if the test passes here; Phase 2 may still add an
    import against the order, and then this commit goes last, as it was. (`5bffb5ffb`)

### Phase 2: behavior

**A. Decay every candidate and every round** (`cd8955d89`). The six bullets share
`page_list/mod.rs`; split by hunk, merging neighbors that do not stand alone.

- A1. A `Page` owns its packer and live texels; a shrink tries its packing in a packer kept on the
  list; `Pace` and `grow`'s size go. No behavior.
- A2. `evict_shelf` tallies the slab once over every page and latches a dry shelf for the frame
  (`ShelfDry`), cleared wherever a shelf can turn evictable.
- A3. A merge walks the pages by live texels and passes a stalled one, the stall now its page's;
  a drain places before it spends the budget.
- A4. A list's last page closes once empty, after an expiry window and at once in a purge.
- A5. A purge returns whether moves wait; the backend commits, submits and purges again, at most
  `MAX_PAGES + 1` rounds.
- A6. The frame's move budget goes to the lists round robin, starting at `frame % 3`.

**B. Give the atlas pool one surface** (`f3fd32fe0`, less commits 9–12).

- B1. `allocate` returns an `Allocation` and takes a `UVec2`; `pin` returns the placement; `unpin`
  stamps; a stale `release` frees nothing, and the tenants' guards go; `live` tests the
  generation alone.
- B2. `ScratchId` carries its format; `ScratchCache` owns its format and the device edge and
  expires in the list's decay; `PageList` forwards to it no more.
- B3. `PoolConfig`: `PageListConfig::of(format)`, the device edge, `MoveBudget::of(PassTraffic)`
  spent at `end_submit`; `MoveRoute` goes, `Mover` reads `PassTraffic`, `MoveDraw` keeps its
  run's rectangles.

**C. One tenant protocol** (`a00e4274b`).

- C1. The backend owns the cutout placements; the quad pipeline's three forwards go.
- C2. `WgpuBackend::drop_evicted` hands every tenant the pool's evictions at a submit's end and
  after a purge; frame teardown is its own call.
- C3. `RasterAtlas::find` gives a placement or nothing in one lookup; `RasterPass::file` decides
  what draws nothing and owns the upload queue; a slot is a 16-byte enum; `TextEncoder::
  encode_batch` owns the run loop.
- C4. The encoded runs stamp the shaper's clock as the batch prepares, where the mirror cut each
  keep window a frame short; the raster atlas's empty entries age on the pool's display frames.
  A fix of its own.
- C5. A cutout table keeps its baked span across a drain's move and codes from its placement.
- C6. A backdrop keeps an item, a scratch texture or a clear; a result past the device's edge is
  kept as its clear and logged.
- C7. A purge takes held scratch its frame did not draw and queues it to its tenant.

**D. Put the gradient row table under the atlas pool** (`2948d198c`, less commits 1–2). One
commit: the pool owns the table, the encoder borrows it through `BuildPlan`, and the host's four
stitch points go. With the renames hoisted it falls from 50 files to the ownership change alone.

**E. Age gradient rows on display frames** (`7b1c7238d`, less commit 3).

- E1. A build moves the pool and its table to the display frame its submit belongs to
  (`DisplayFrames::next` through `BuildCtx`), only where it paints (folded in from `77c7a81bd`);
  the cutout plan places on its submit's frame; a host with no device counts a compose a frame.
- E2. Rows grow under their soft rows and evict past them by the clock with the lists' grace:
  `GRACE_FRAMES` moves into `common::eviction_clock`, the MRU list goes, held rows a prefix.

If E1 does not build against the MRU table, E1 and E2 are one commit.

**F. Decay and purge the gradient rows with the pool** (`77c7a81bd`, less commit 2 and E1).

- F1. A flush bakes each row into the belt through one row on the stack; the table's staging goes.
- F2. Rows idle past an expiry window leave above the soft rows; a table a quarter full drops a
  rung; a purge takes every row the frame did not draw.
- F3. The index keeps each row's hash and room for two entries a row.
- F4. A table starts at 16 rows.

**G. Bind the mask and color pages through one atlas group** (`9bd1bf298`, less commit 7):
`AtlasGroup` at `ATLAS_GROUP` for the rasters and the shadows, `page_group.rs` goes,
`FrameVariant` made once a submit.

**H. Build the spun raster pipelines only for a frame that turns a raster** (`981d6d97f`), as is.

**I. One shared page read and one sampler** (`06d240382`, less commit 8).

- I1. `primitives/wgsl/atlas_pages.wgsl` replaces `atlas_slots.wgsl`: the page bindings and one
  texel or four taps of a page, for the raster and shadow shaders.
- I2. `TextureBinding::backdrop_group` over the binding's one sampler, which the blur takes too;
  the storage entry shared.

**J. Keep a glyph's template as its offset and its raster's handle** (`dda3229c4`), as is.

**K. The order test**, if commit 14 could not go early.

**L. The review document**, `.notes/review-atlas.md` at its final content. The `.notes/ISSUES.md`
and `src/gpu/AGENTS.md` edits do not wait for it: each travels with the commit whose change it
records, an issue deleted in the commit that fixes it.

## Your decisions

- **The review document**: commit L as above (pick), or leave it out. Your rule for a proposal is
  that its lasting decisions fold into the system's document and it is deleted when the work
  lands; some of its decisions say "Folded into `src/gpu/AGENTS.md`", the rest do not yet.
- **How the split replaces `atlas-review`**: a force push of `atlas-review`, which `origin` holds
  at `9ed23212f` (pick, as the branch is a review branch), or a new remote branch beside it.

Unverified so far: whether each pre-squash commit built and passed its tests on its own, and which
Phase 2 splits stand alone; the steps above check both as each commit is made.
