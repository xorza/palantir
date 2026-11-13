# Review: the atlas pool, its tenants, bindings and gradient rows

Read at `7f38f3be9`: `src/gpu/atlas`, the four tenants (`gpu/raster`, the cutout tables under
`gpu/pipeline/quad_pipeline`, `gpu/backdrop/backdrops`), what binds or samples a page
(`gpu/pipeline`, `gpu/resource/texture_binding.rs`, the WGSL), the orchestration in
`gpu/wgpu_backend`, and the gradient rows (`renderer/cpu_gradient_atlas`, `gpu/atlas/row_table.rs`).
Nothing here was benched: every performance item names the bench that accepts it. The scope's
tests run in 0.02 to 0.66 s a module, none over a second.

## 1. Proposal

### Root causes

- **R1. The pool hands out less than it knows, and panics on a state it creates.** `allocate`
  returns a bare handle, so every tenant asks again where it lies; `release` and `pin` panic on a
  stale handle though an eviction makes one at any time, so every caller looks up first; a scratch
  id does not name its list. Tenants then keep copies of what the pool holds: a side, a size, a
  content, a table's code.
- **R2. There is no tenant protocol.** The same steps (find, allocate, upload, stamp, learn of an
  eviction, release) are written four ways: four drains with four signatures, three reverse
  lookups, three treatments of `AtlasError`, eviction counts in two tenants at two sites, three
  clocks of which two are stale where they are read, three shapes of "follow after a commit".
- **R3. A device constant and a consumer's key sit in the wrong half.** The move budget rides
  every `end_submit` and is mirrored in three two-case enums; the device edge sits on `PageList`
  to be handed to its scratch cache; the CPU half builds the raster pipelines' key; the per-format
  tables are positional, `3` written five times and `format as usize` 21.
- **R4. Decay plans one candidate behind one latch.** The merge considers the emptiest page
  alone, the stall is one field for the list, a purge stops at its first drain that moved, a
  list's last page never closes, and the frame's move budget goes to the lists in a fixed order.
- **R5. The mask pages have two consumers and each built its own binding.** Two bind groups over
  the same pages at one group index, five statements of which lists a draw reads, the page pick
  written twice in WGSL, and a pipeline layout a family, which wgpu-core answers at each family
  switch with an immediates clear and often a rebind.
- **R6. The gradient rows are a second pool, in the renderer.** Its own clock (a count of
  flushes), its own eviction rule (evict before grow, no grace), its own shrink (re-bake all), a
  CPU staging that only grows, and four points where a host stitches its two halves; no purge
  reaches it.
- **R7. Leaf types sit in high modules.** Eleven submodules of `gpu` form one strongly connected
  component; no order among them is written or tested.
- **R8. A cached glyph is a whole quad.** 28 of its 36 bytes are derivable, and a replay pays for
  each glyph what a run could pay once.

### Target design

**Data**

- **An allocation is `Allocation { handle, placement }`.** `allocate` returns both; `pin` returns
  the placement as `stamp` does; `release` of a stale handle is a no-op, as its generation rules
  out a wrong free; `unpin` stamps. Refused: the lookups kept, three `expect`s and a hash lookup a
  new item.
- **`ScratchId { format, idx, generation }`**, a sibling of `AtlasHandle`, so `release_scratch(id)`
  and `scratch_view(id)` cannot be handed the wrong list or a stale slot.
- **An item carries its tenant's tag.** `allocate(.., tag: u32)` stores it in `Entry`; the
  eviction queue yields `Evicted { handle, tag }`; a tenant checks `slots[tag].handle == handle`.
  `Entry` stays 32 bytes with `pins: u16`. Refused: a shared `HandleKeys<K>` map, which shares the
  code and keeps a hash insert a raster and 20 to 32 bytes a glyph. To bench: `cache_churn/prepare`.
- **A tenant stores a handle and what the pool does not hold.** `PlacedTable { handle, baked:
  Option<TexelSpan> }`, table-relative, so a move bakes nothing; `Kept::{Item(AtlasHandle),
  Scratch(ScratchId), Clear}`; `PlacedRaster` without `content`; `EncodedGlyph { offset, handle }`.
- **A drain's stall is its page's**: `Page.stalled: Option<DrainCount>`, where one field for the
  list remembers one page.
- **Per-format tables are `PerFormat<T>` indexed by `AtlasFormat`**, a list's constants
  `PageListConfig::of(format)`, an exhaustive `match`; `Tenant::ALL` with `COUNT = ALL.len()`.
- **A row is `Row { ramp, hash, last_use }`**, held rows a prefix of `rows`, so "never claimed" is
  `len`, not an `Option`, a zero epoch and a place in a list.

**Ownership**

- **The pool owns the move budget and the row table.** `AtlasPool::new(PoolConfig { device_edge,
  move_budget })`, `end_submit()` with no argument; `MoveRoute` goes, `MoveBudget::of(PassTraffic)`
  and `Mover::of(gpu, PassTraffic)` read the one enum.
- **`ScratchCache` owns the device edge** and expires in `PageList::decay`, one housekeeping
  moment where `advance_to` held a second.
- **The backend owns one `AtlasGroup` and the cutout placements.** One bind group over
  `ATLAS_LISTS` at `ATLAS_GROUP`, its layout lent to the raster and the shadow pipelines; the
  cutout placements, a CPU map with no device, beside the pool, where six forwards reach it today.
- **`TextureBinding` owns every shared layout's group and the one sampler**: the backdrop
  composite's group is its method, where a free fn builds it over a second sampler and the blur
  holds a third.
- **The table hands the renderer rows through a lease.** `RowLease { table, frame }` beside the
  table in `renderer::row_table`, which the encoder takes in `BuildPlan` (batch 4 took the table
  itself; batch 5 adds the frame).
- **Each tenant has `drop_evicted(&mut self, pool)`**, called for all four at the end of a submit
  and after a purge; the pool counts evictions by tenant in `Evictions::push`.

**Module layout**

```text
gpu/device      + BackbufferOut
gpu/texel       + TargetSpec
gpu/resource    + GpuCtx, RowCopy, Staged, MapState      (beside upload_staging)
gpu/atlas/row_texture.rs                                 texture, view, generation
gpu/pipeline/atlas_binding.rs    ATLAS_GROUP, ATLAS_LISTS, AtlasGroup, AtlasSlots::of(&pool)
gpu/pipeline/effect_pipeline/    + wgpu_effect_store
gpu/frame/gpu_timings            + FrameTiming
primitives/wgsl/atlas_pages.wgsl page bindings, slot override, palantir_mask_taps
```

- **An order among `gpu`'s submodules, written and tested**: device → texel → resource → atlas →
  pipeline → {raster, backdrop} → frame → surface → wgpu_backend, as `internals::module_order`
  holds the top-level one.

### Decisions to take

Each changes a rule `src/gpu/AGENTS.md` states or a behavior a person can see; the pick is first.

- **Where the row table lives.** Under the pool behind `RampRows` (pick): purge, decay, device
  loss and the memory report reach it where they reach the lists, and the four stitch points go.
  Or in the `Encoder`, handed a frame number: the smallest diff, but the host still stitches and
  "one pool owns every atlas byte" stays figurative.
- **How the CPU half learns the display frame**, for the rows and for the cutout tables, which
  allocate before `advance_to`. A pure `DisplayFrames::next(owner)` passed through `BuildCtx`
  (pick), `advance` defined through it. Or the clock advanced at build, which counts frames that
  never paint, against "reclaim runs on painted frames only".
- **Rows grow before they evict, with the page lists' one-frame grace**, and shrink by moving at
  most 32 rows a frame into freed places. This rewrites "halves at the start of a frame that ends
  an idle expiry window ... its live rows renumbered and baked again", and gives the rows a soft
  budget of their own beside "one budget a format".
- **A list's last page closes** once empty, after an expiry window and at once in a purge: 64,
  256 and 512 KiB by list that an app which once drew one glass keeps today.
- **A held scratch result no frame drew goes in a purge**, as an item does: up to about 25 MiB for
  a full-window glass, where the purge today takes the page results and leaves the largest.
- **One pipeline layout `[frame, atlas]`** for fills, shadows, rasters, meshes and curves. To
  bench before taking: layout changes counted in `PassCounts`, `main_pass/*` and `frame/*` on the
  three hosts, the fill's instruction count unchanged with an unused group.

### Shared ownership

| site | holds | verdict |
| --- | --- | --- |
| `Rc<WgpuImageStore>`, `RefCell` inside (`wgpu_backend/mod.rs:146`, `resource/wgpu_image_store.rs:31`) | image textures and groups | Stands between the registry and the device: the registry writes at any time and outlives a device. The backend's own clone only carries it across recovery; the host could own it and lend it at `submit`. |
| `Rc<WgpuEffectStore>`, `RefCell` inside (`wgpu_backend/mod.rs:148`, `resource/wgpu_effect_store.rs:41`) | modules and effect pipelines | The same. |
| `EffectDevice.gpu: Gpu` (`pipeline/effect_pipeline/mod.rs:39`) | a device clone | Becomes a parameter: only the backend builds. |
| `AttachedStore.{gpu, binding}` (`resource/wgpu_image_store.rs:40`) | device and layouts | Stands: `ImageStore::write` runs with no backend on the stack. |
| `GpuPaintRef(Rc<RefCell<dyn GpuPaint>>)` (`gpu_paint/gpu_paint_ref.rs:10`) | the app's callback | Stands: owned by the app, lent across the frame boundary. |
| `DeviceHealth(Arc<AtomicBool>)`, `MapState(Arc<AtomicU32>)`, the tracers' `done` flags | a flag a wgpu callback sets | Stand: wgpu's callbacks are `'static` on any thread. |
| `Arc<dyn InstanceDisplay>`, `&Arc<W>` (`surface/surface_manager/mod.rs:48`) | display, window | Stand: wgpu takes owned handles. |
| `TestOnly<Cell<u32>>` in the composer | test counters | Stand: zero-sized outside tests. |
| `TextureBinding` cloned into `FrameGroup` and `GpuViewTargets` | seven layouts, a sampler | Lent by the backend at `upload` and `paint_gpu_views`; the image store keeps its own. |
| the backdrop composite layout (`backdrops/mod.rs:233`), the raster atlas layout (`raster_pipeline/mod.rs:78`), three samplers | wgpu handles in two owners | One owner each: `TextureBinding`, `AtlasGroup`, one sampler. |
| `TargetSource` cloned into `BackdropBlur.target` each frame (`backdrops/mod.rs:539`) | the stream's group | Lent: `prepare` and `encode` take `Option<&TargetSource>`. |
| `Pipeline.layout` in each built pipeline | the layout's identity | Stands: the recorder compares it at each bind. |

No `static mut`, `thread_local`, `Weak`, `OnceCell`, `RwLock` or shipped `Mutex` in `src/gpu` or
`src/renderer`.

## 2. Plan

Paths under `src/` unless they start with a dot. Each batch leaves the build and the tests green.

### Weighed and left

- **`frame` and `evictions` held by each list**: the frame would be stored three more times, and
  tests step it a call.
- **A generation each for pages and scratch**: no mask or color scratch exists, and a ladder step
  rebuilds five groups on the frame it lands.
- **`MoveDraw`'s bind group and buffer write a run**: at most nine runs on a drain frame.
- **`RasterVariants` as a sparse list**: a few tens of KiB a format for an O(1) lookup.
- **`PendingCopy` resolved by handle at the flush**: no decay or purge runs between a submit's
  prepare and its flush.
- **A smaller `Entry`**: 32 bytes buys no cache line at 24.
- **`Budget`, `ShelfUse`, `PageSlots`, `DirtyRows`, `MERGE_GAP_ROWS`, `Tenant` as an enum**: each
  names a real case or a measured choice.
- **The bake by segment** (`color_ramp.rs:157-180`): a new row's copy command, 45 µs on the board,
  outweighs its bake of 5 to 12; take it only at 5 % or more on `gap_row/bake/*` there.
- **The flat raster fragment's color and four-tap arms compiled out**: a pipeline key each, worth
  one only if the instruction count shows a thread-count step as the page pick did.

## Decisions

- **A list's last page closes once empty**, after an expiry window in a decay and at once in a
  purge, so a list a frame once drew holds no page for good: up to 512 KiB of half-float for an
  app that once showed a glass. As WebRender drops an emptied texture of its shared cache
  ([`texture_cache.rs`](https://github.com/servo/webrender/blob/main/webrender/src/texture_cache.rs),
  `TextureCache::update`'s free of empty units). Refused: a first page kept, which a light app pays
  for its life. Folded into `src/gpu/AGENTS.md`, Atlases.
- **A purge goes on round after round**, each round's moves committed and submitted before the
  next drain is planned, at most `MAX_PAGES + 1` rounds, as the move draw writes each commit's
  rectangles from the start of one buffer. Refused: one round, which the decision's "the pages
  drain with no budget" did not hold to; several commits in one submission, which overwrite the
  rectangles of the first.
- **The frame's move budget goes to the lists round robin**, the first list `frame % 3`, as
  WebRender's `run_compaction` takes one allocator list a call round robin
  ([`texture_cache.rs`](https://github.com/servo/webrender/blob/main/webrender/src/texture_cache.rs)).
  Refused: one fixed order, under which a draining mask list held every other list's moves.
- **An unpin stamps its item as drawn now**, so the clock takes the items let go longest ago
  first, and so nothing turns evictable within a frame: the clock's and the shelf's latches,
  keyed by the frame, hold without an unpin opening them. Refused: an unpin that left the stamp,
  after which each caller stamped by hand and the latches had to open.
- **A stale `release` gives nothing back**, as `HashMap::remove` of an absent key: an eviction can
  stale a handle any frame before its tenant drains, and the generation keeps it from freeing
  another item. Refused: a panic every caller guarded with a lookup. A scratch release keeps its
  panic, as a scratch texture is never evicted, so a stale id there is a slip.
- **Each list's constants are `PageListConfig::of(format)`, an exhaustive `match`**, and a value a
  format is `PerFormat<T>`, indexed by `AtlasFormat`, as `enum_map`'s `EnumMap` keys an array by
  an enum ([docs.rs/enum-map](https://docs.rs/enum-map)): written here, a 30-line type, rather than
  a dependency for three formats.
- **The move budget is the pool's, from `PassTraffic` once**: `PoolConfig::of_device(edge,
  traffic)`, `MoveBudget::of(traffic)` beside the GPU half's `Mover::of(gpu, traffic)`, both over
  the one enum. Refused: `MoveRoute`, a third enum of the same two cases, and the budget passed at
  every `end_submit`.
- **Scratch expires in the list's decay**, at the end of the display frame's first submit, with
  everything else the pool returns; `advance_to` only moves the frame.
- **A tenant's entries that draw nothing age on the pool's display frames**, an expiry window, as
  the pool's own items do; the encoded runs stay on the shaper's clock, which their shaped buffers
  age on, read as the batch prepares, the clock the frame's record pass ticked. Refused: the
  shaper's clock mirrored into the raster atlas at the last submit's end, a frame behind each
  stamp, which cut every keep window a frame short.
- **Every tenant drops the pool's evictions in one place**, `WgpuBackend::drop_evicted`, at the end
  of a submit and after a purge, each through its own `drop_evicted`; frame teardown is its own
  call.
- **A held scratch result no frame drew goes in a purge**, queued to its tenant as an item is; a
  purge left the largest results, tens of MiB for a full-window glass. Folded into
  `src/gpu/AGENTS.md`, Atlases.
- **A backdrop result past the device's texture edge is kept as its clear**, logged, so its ledger
  holds it and no frame captures it again for nothing; a page's refusal for room still lists it
  evicted, as room may come back.
- **The pool owns the row table, a concrete type the encoder borrows**: `AtlasPool.rows`, made
  with the pool and lost with it, reached by a painted frame's build through the pool it plans its
  cutouts in, and flushed by the submit that paints it. The table stays `renderer::row_table`, as
  the encoder, below `gpu`, names it, so no trait stands between them. As WebRender's
  `ResourceCache` owns its GPU cache beside its texture cache
  ([`resource_cache.rs`](https://github.com/servo/webrender/blob/main/webrender/src/resource_cache.rs)).
  Refused: a `RampRows` trait with one implementor; the table in the `Encoder` handed a frame
  number, which keeps the host stitching texture and table together at four points.
- **A build that paints moves the pool to the display frame its submit belongs to**:
  `DisplayFrames::next` before the submit's `advance`, handed to the build in `BuildCtx` and
  applied at the start of `build_painted`; the pool and its row table move only on, so a build
  whose submit was dropped leaves them a frame ahead, which holds what the next window draws a
  frame longer. As WebRender stamps its cache with the frame being built (`FrameStamp` in
  [`resource_cache.rs`](https://github.com/servo/webrender/blob/main/webrender/src/resource_cache.rs)).
  Refused: the pool left on the last submit's frame, where an insert met that frame's dry latch
  and a release stamped a frame early; the pool moved before every build, which a still frame left
  a frame ahead of the last it drew, so a purge took what that frame drew.
- **Rows grow under their soft rows and evict past them, on the lists' clock and grace**:
  `RowTableConfig { first_rows, soft_rows, max_rows }`, the clock over the held rows with its dry
  latch, `GRACE_FRAMES` one constant in `common::eviction_clock` for the lists and the rows. As
  the page lists grow under their soft budget and evict past it. Refused: the exact LRU list,
  which relinked on every hit for an order the stamps hold (register hit −16 %, miss unchanged on
  the Apple laptop); an epoch a flush ends.
- **A host with no device counts a display frame a compose**, its table its own.
- **Rows decay as the lists do, above the soft rows**: at each display frame's first submit, rows
  idle past an expiry window leave, the last row moving into each place, at most
  `AGE_EVICTIONS_MAX` moves and `AGE_SCAN_MAX` rows walked, the bounds the lists' age-out takes
  from WebRender's `MAX_EVICTIONS_PER_FRAME`; at a quarter or less held the capacity drops to the
  smallest rung of the first rows that holds twice the rows, as a `Vec` halves, the kept rows
  keeping their ids, so the texture copies them. A purge takes every row the frame did not draw.
  Refused: the halving, which renumbered and baked every live row in one frame.
- **A device's table starts at 16 rows**, 32 KiB, as the lists' first pages start small and grow:
  `cargo atlas-memory` reads 32 KiB of gradient rows over the bench scenes, where 256 first rows
  held 512 KiB beside a first mask page of 256 KiB. Refused: 32, which no scene needs.
- **A flush bakes each row into the belt through one row on the stack**, staged by
  `GpuCtx::stage` and copied a run at a time, as `GpuCtx::copy_rows` stages packed rows. Refused:
  a staging `Vec` the table kept, the size of its largest flush for good, copied once more into
  the belt.
- **The index stores each row's hash and room for two entries a row**, so it never resizes inside
  a registration: at half full or under hashbrown rehashes in place when tombstones fill it
  ([`RawTable::reserve_rehash`](https://docs.rs/hashbrown/latest/src/hashbrown/raw/mod.rs.html)).
- **A glyph's template is its offset and its raster's handle**, 20 bytes, the replay building the
  quad with the miss path's constructor (`RasterQuad::placed`) from the raster's placement and
  list and the run's color: `stable_keys/prepare` 31.5–32.1 → 25.4–26.0 µs and
  `steady_warm/prepare` 5.49–5.63 → 4.65–4.69 µs on the Apple laptop, with a `reserve` of the
  run's glyphs before the loop, past the pool's base (`1851f60e2`, 26.6–27.1 and 4.86–4.96 µs). As Skia's `GlyphRun` keeps glyph ids and positions, not quads.
- **`gpu`'s submodules take an order the order test holds**, leaves first and the backend last, as
  the crate's top-level order does (`internals::module_order`); a type two layers share sits in
  the lower. Folded into `src/gpu/AGENTS.md`, Module order. Refused: the one cycle through eleven
  submodules.
- **A raster with no image is filed as drawing nothing**, as one of no pixels: glyph keys carry a
  font id fontdb never reuses, and an icon's failed parse is not retried either.

## Changed

- Batch 1, tests nothing pins: every case is pinned but a page grown in the commit that moves onto
  it. It arises only for a shrink's target below the page edge beside a draining page an insert
  cannot use, a shelf state no plain scenario reaches; the commit's order, a grow's copy before the
  moves, is the code's (`ListTextures::commit`).
- Batch 1, `plan_shrink`: named `shrink`, as it acts rather than plans: it closes an empty page or
  opens the target and begins the drain; `shrink_target` is the plan `decay` reads for its window.
- Batch 2, `scratch_view(id)`: the id carries its format, but the GPU half keeps no generations,
  so a stale id still reads whatever its slot holds now; a caller holds its id live, as the
  backdrops hold this frame's reads.
- Batch 2, private state in tests: `draining()` stays beside `is_stalled()`, as the drain's state
  shows in no move where a stall follows; `evictions.queues` stays, the list's output channel a
  test reads what an insert or a decay evicted from. `begin_drain` and `pages` are no longer read.
- Batch 3, find and file: `RasterAtlas::find` gives a `Found` and `RasterPass::file` files an image,
  as the pass owns the upload queue (`RasterUploads`) the plan put on it; `Filed::{Found, Full}`
  in place of `Rasterized`.
- Batch 3, forwards: `QuadPipeline::prepare_cutouts` still forwards to the tables, which own the
  bake's GPU objects, and `follow_cutouts` waits for batch 7's one atlas group; the placements'
  three forwards are gone.
- Batch 3, the cutout plan's frame: moved to batch 5, which brings the display frame to the build
  for the rows; the fix needs the same plumbing.
- Batch 3, one driver for the rigs: each rig now runs `WgpuBackend::submit`'s order, the decay, the
  drain, the frame end and the next frame; one driver over the four tenants would hold every
  tenant's fixture for tests that drive one. Of the gaps, a moved cutout table, a moved held
  backdrop and a hand-packed oracle are pinned; an icon reads the pool's placement at every find,
  as a glyph's miss does, which the text test pins.

- Batch 4, where the table sits: `renderer::row_table::RowTable`, owned by `AtlasPool`, the
  encoder taking `&mut RowTable` in `BuildPlan`; the plan's `gpu/atlas/row_table/` behind a
  `RampRows` trait would add a trait of one implementor, as the encoder sits below `gpu`. The
  texture is `gpu/atlas/row_texture.rs`, a type of its own beside `AtlasTextures`, not inside it.
- Batch 4, a host with no device: a harness at `Depth::Compose` holds its own table; one at
  `Depth::Record` holds none, so `paint_full` and `rebuild` there panic, as that depth builds no
  draw list; every caller composes.

- Batch 5, the lease: no `RowLease`; the table holds its display frame, which the pool moves at
  `begin_build` and a host with no device at each compose, so the encoder keeps `&mut RowTable`.
- Batch 5, a stamp's `max(last_use, frame)`: the table's frame only moves on, so every stamp is the
  later already.
- Batch 5, the script table: on configs of 4 rows growing to 4, 8 and 16, which shows each rule at
  the smallest table that holds it.

- Batch 6, `FlushedRows`: kept as the flush's view, its runs, the held rows it bakes from and
  the capacity, as the texture needs the dirty runs and the ramps at once; the staged rows and
  `copies` went with the staging.
- Batch 6, the benches: `atomic/gradient_upload` times copies alone, which this change leaves as
  they were, so it measures nothing of it; the 16-against-32 grow frame was not run, the first size
  decided by memory.

- Batch 7, the raster pair's atlas: `RasterPipelinePair` still carries the group, from the one
  `AtlasGroup` through `PassPipelines::atlas`, as an effect's images take group 1 between two
  raster draws, so each draw binds it and the recorder elides the repeats.
- Batch 7, which lists a draw reads: `ATLAS_LISTS` and `AtlasSlots::of(&pool)` in
  `pipeline::atlas_binding`, the overrides one name each (`MASK_SLOTS`, `COLOR_SLOTS`), the shadow
  shader taking `ATLAS_GROUP` by substitution; `backdrop.wgsl`'s group is the composite's, not the
  atlas's, and stays.
- Batch 7, `commit_atlases`: the backend's commit and the atlas group's follow stay two lines at
  the frame's upload and the purge, as the upload's `GpuCtx` holds the belt through them; the
  cutouts follow nothing, and the backdrops follow at the upload and drop their groups at a purge.
- Batch 7, `FrameVariant`: made once a submit, the clipping beside it read once; `VariantTarget`
  still hands the slots to the families that read no page, as their `ensure` takes one target.

- Batch 7, the shared page reads: `primitives/wgsl/atlas_pages.wgsl` declares the bindings, the
  overrides as `PALANTIR_MASK_SLOTS` and `PALANTIR_COLOR_SLOTS`, under the prefix every shared
  name takes, and one texel and four taps of a page each picked once; the instruction counts
  were not taken, as no tool here reads them, and the change reads three fewer chains a resampled
  fragment.
- Batch 7, the composite's group: `TextureBinding::backdrop_group` over the binding's one sampler,
  which the blur takes too; `Backdrops` keeps a clone of the binding in place of the layout's,
  as `FrameGroup` and `GpuViewTargets` keep theirs.
- Batch 7, one field `bound`: the composite groups' and the effect group's; the blur's
  `params_bound` keeps its name, beside the blur's other buffers.

- Batch 10, the moves: `GpuCtx`, `RowCopy`, `Staged`, `MapState`, the pipeline recipe and the
  shader body into `resource`; `TargetSpec` and `TargetFormat` into `texel`; `BackbufferOut` into
  `device`; `FrameTiming` into `frame::gpu_timings`; the effect store under
  `pipeline::effect_pipeline`, with what a backdrop's composite binds (`backdrop_groups`), which
  an effect reads; `FrameGroup` takes a `RowView`, `FrameBackbuffer` a view and a size. The order
  names the leaves `error` and `gpu_paint` sit on.

## Declined

- Batch 2, `Move { size, from, to }`: the two sizes are one placement's, checked by two
  `debug_assert`s on a list of at most a frame's moves; a page-and-origin type costs a new type and
  every move site for 4 bytes a move.
- Batch 2, the age-out on `eviction_clock`: the age-out evicts every idle slot within a scan cap,
  the clock stops at its first victim; a shared sweep would take a cap and an evicting callback,
  more than the two lines of wrap it would replace.
- Batch 3, tags in place of the reverse maps: a tag a tenant gives at `allocate` would serve the
  raster tenants alone, the cutout tables and backdrops keying on values no `u32` names, and the
  rasters' map costs a hash insert on a miss whose rasterization costs microseconds.
- Batch 3, evictions counted by the pool: the raster tenants' counts serve the text bench's churn
  arms, no reader asks for the others', and the two raster sites stay exclusive by construction,
  a handle leaving the reverse map at the first.
- Batch 7, one `[frame, atlas]` pipeline layout: WebGPU validates every group of a draw's
  pipeline layout, used or not, so each fill after an effect's images at group 1 would bind the
  atlas group again; the gain is unmeasured, the bench asks for three hosts.
- Batch 7, the effect runs' `group_once`: its own condition, other effect work, has not come.
- Batch 7, the blur's target as a parameter: four steps between `prepare` and `encode` read it;
  threading it through each costs more than the reference count it saves a frame.
- Batch 8, pages no pass loads whole: the route pays only on a device whose passes move their
  whole target, which no host here has, so neither its cost nor its gain can be read; taken
  blind it trades a load the board measured for texture churn and draw splits no one measured.
  It waits for the board; the issue stays in `.notes/ISSUES.md`.
- Batch 9, the replay's other cuts (a run's one format, `stamp_run`, the clip inside the loop):
  the template took the replay 15 to 18 % faster on this host, and `reserve` one or two more; the
  rest wait for a profile of the replay that shows them, as each needs its own A/B on every host.
- Batch 10, `TextureBinding` lent to `FrameGroup` and `GpuViewTargets`: an immutable bundle of
  wgpu handles, cloned by reference count and never changed, so a clone holds no hidden state;
  lending it adds a parameter at every call and takes `FrameGroup::upload` past four.
