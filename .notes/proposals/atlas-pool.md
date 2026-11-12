# One atlas pool

Replace the GPU's four atlas systems with one pool that owns every atlas byte, so that memory
follows what is drawn and no frame pays for the shrinking: the decisions, then the work in steps.
Facts and sources: `.notes/research/2026-11-12-atlas-pools.md`,
`.notes/research/2026-10-28-atlas-growth.md`.

## Today

Seven textures across four systems, one set per device:

| atlas | system | format | first texture | soft / hard | shrinks |
|---|---|---|---|---|---|
| text masks | `RasterAtlas` over `PackedAtlas` | R8 | 1024² · 1 MiB, at startup | 4 / 16 MiB | never |
| text color | same | RGBA8 sRGB | 256² · 256 KiB, at startup | 4 / 16 MiB | never |
| icon masks | same, a second instance | R8 | 256² · 64 KiB, at startup | 4 / 16 MiB | never |
| icon color | same | RGBA8 sRGB | 512² · 1 MiB, at startup | 4 / 16 MiB | never |
| backdrop results | `Backdrops` over `PackedAtlas` | RGBA16F | 1024² · 8 MiB | 32 MiB / device edge | to 32 MiB, never below, never freed; never while a result passes 2048 |
| shadow cutouts | `CutoutTables` + `CutoutPlacements` | R32F | 1024² · 4 MiB fixed, at startup | — | never |
| gradient rows | `CpuGradientAtlas` + `GpuGradientAtlas` | RGBA16F | 256 rows · 512 KiB + a CPU mirror | 4096 rows · 8 + 8 MiB | never |

- **6.8 MiB before anything draws**, and up to about 108 MiB of GPU memory and 8 MiB of CPU memory
  held for the device's life after one busy moment (`.notes/ISSUES.md`).
- **Two tenants per format double the headroom**, each texture keeping its own empty half.
- **A grow copies the whole texture**, 296–914 µs measured beside a 248 µs text frame.
- **A full-window backdrop pays its page**: a 1920 × 1080 result takes a 2048² RGBA16F texture,
  32 MiB for 16, kept after it goes. Measured worse: a glass toolbar 2560 px wide at full
  resolution takes the atlas to 4096², 128 MiB for 10.4 MiB live, for good
  (`.notes/research/2026-11-12-atlas-pool-measurements.md`).
- **The one shrink there is moves everything in one frame**, a copy command a result, where a
  command costs about 22 µs on the AMD iGPU.

## Decisions

### The pool

- **One pool per device owns every atlas byte, through three allocators by item shape.** Packed
  pages for small 2D items, scratch textures for large ones, a row table for LUT rows; one
  accounting, one budget per format, one decay policy, one commit step, as WebRender's
  `ResourceCache` owns its texture cache, GPU cache and render-target pool. Refused: one allocator
  for every shape, which packs window-sized results into pages or splits gradient reads across
  them; an allocator per tenant, today's, which doubles headroom and code.
- **An allocation is a handle; a move rewrites its placement and nothing else.** Icons and backdrop
  slots read placements each frame; a cutout plan reads its codes in each build
  (`CutoutPlan::place`); the encoded-run cache repacks each glyph's `uv` from the placement
  its handle's stamp returns. Only eviction moves a generation.
- **The pool ages on display frames**, as the backdrops do; the text clock keeps its runs, which
  find a moved or evicted glyph through the slot either way.
- **Device loss rebuilds the pool through its constructor**, and each tenant fills it again as at
  startup (`src/gpu/AGENTS.md`, Device loss).

### Packed pages

- **One list per texel format, shared by every tenant of it.** R8 holds glyph and icon masks and
  the cutout tables, RGBA8 color glyphs and icons, RGBA16F small backdrop results; as WebRender's
  `SharedTextures` and Zed's atlas keep a list per format or kind.
- **Glyphs and icons share their lists (D1).** `raster_pass.rs` gives each its own budget "so a
  color-icon-heavy frame cannot evict the label's glyphs"; inside one list nothing drawn this
  frame or the last is evicted, so sharing lets them compete only for idle entries, and one bind
  group lets icons join the raster lane. Zed keeps glyphs and SVG icons in one `Monochrome` list
  (`AtlasKey::texture_kind`). Refused: a list per tenant, as WebRender splits glyphs from images
  that never share its batches, where here they share the pipeline. Cost: masks together cap at
  16 MiB, where each tenant had 16.
- **Cutout tables are R8 in the mask pages (D2)**, as Skia stores its blur profiles and round-rect
  blur masks as `kAlpha_8`. A quarter of R32F's memory; R8 renders everywhere, so
  `CutoutMode::Shade`, its probe and the GL fallback go. Cost: quantizing to 1/255 moves a shadow
  pixel by at most one 8-bit step, so snapshots change. Refused: R16F or R32F, a list of their own
  that keeps the fallback.
- **A list binds up to four pages together, the page index in the instance**, as Skia's
  `kMaxMultitexturePages` and its 2-bit page in the texel `x`, so a draw never splits by page.
  Skia's default for multiple glyph pages is its own logic, which can weigh the driver version
  (`fAllowMultipleGlyphCacheTextures`), so M3 measures the branch on V3D and Apple: a cost past
  noise on any bench host binds one page per draw instead, as WebRender and Zed bind, splitting a
  batch only where its items cross pages. Refused: a texture array, which gains no layer without
  copying every layer; binding arrays, native only.
- **A list's first page starts at 256² and doubles to the page edge; past it the list adds
  pages.** A light app holds a few hundred KiB, and no grow past one page copies anything.
  Refused: fixed pages from the start, which charge a light app a whole page; one texture
  doubling without end, today's.
- **An item larger than half a page is scratch, which only per-draw tenants take.** Large backdrop
  results bind their texture in their composite's group; raster items stay within a page edge,
  as glyphs past it are refused today (`oversized`) and an icon rasters at most 512 px
  (`MAX_RASTER_PX`).
- **One eviction clock per list across its tenants.** It skips what was drawn this display frame or
  the last, and what is pinned (a cutout table a plan holds). Under the soft budget a list grows;
  over it, it evicts; with nothing evictable it grows to the hard cap; at the cap the insert is
  refused, as today. The one-frame grace is WebRender's least age at full pressure, and keeps a
  working set a little over budget from evicting and rasterizing the same items every frame,
  which today's "not drawn this frame" allows.
- **Moves go by copy commands or by one instanced draw per destination page, whichever M2 finds
  faster, per device if hosts disagree.** WebRender keeps both, `copy_texture_sub_region` a copy
  and `ps_copy` instanced draws sorted by destination "where submitting hundreds of small blit
  commands" costs too much, as palantir picks `BackbufferOut` per device. Both are exact, so the
  difference is speed alone. A draw needs `RENDER_ATTACHMENT` on the pages, and on a tiler loads
  and stores the whole destination page. Refused: a compute copy, as R8 is no storage format in
  WebGPU.
- **Color pages store `Rgba8Unorm`, and the raster shader decodes sRGB (D3)**, so either mechanism
  moves color bytes exactly on every backend and later batched uploads can draw too; WebRender
  keeps color as plain RGBA. Refused: sRGB pages moved by copies alone, a second move path, as GL
  and WebGL give no linear view of an sRGB texture (`DownlevelFlags::VIEW_FORMATS`) and a round
  trip through an sRGB attachment does not promise the same byte. Cost: a color glyph or icon
  pixel may move by one step.

### Scratch textures

- **Sizes round, are reused, and expire.** A side rounds to the next power of two up to 1024 and
  to the midpoint between powers above it (Skia `GetApproxSize`); a free texture serves a rounded
  request it covers within twice the area (Chromium `kReuseThreshold`); an unused one goes after
  an expiry window. The ladder bounds how many sizes a resizing glass makes. Cost: a full-window
  result of 1936 × 1088 takes 2048 × 1536, 1.5 times its bytes. Refused: exact sizes reused as
  Chromium reuses them, which make a texture each frame of a growing animation.

### The row table

- **Gradient rows are a row table, not atlas items.** They are per-paint data every color pipeline
  reads through the frame group, as WebRender keeps gradient stops in its GPU cache and not its
  texture cache; the read stays branch-free.
- **A row keeps its id while it lives; the table doubles when full and halves at a quarter.**
  Halving renumbers live rows at the frontend's next frame and bakes them again from their ramps,
  which the registry holds.
- **No CPU mirror of the texture.** A flush stages each dirty row's bake, and the clean rows a run
  folds in (`MERGE_GAP_ROWS`) bake again from their ramps, the same bytes, as the bake is
  deterministic. M5 checks the flush cost. Refused: the mirror, 2 KiB of CPU memory a row, up to
  8 MiB, kept only to fill those gaps.

### Decay

- **Age-out, then drain, then free**, as WebRender's texture cache. An item idle past an age that
  shortens as the list fills goes, at most 32 a frame below four times the threshold. A page
  whose live items fit elsewhere drains into the fullest pages, the emptiest page first, at most
  512² texels a frame (WebRender's `area_threshold`) and M2's command count. An emptied page and
  an expired scratch texture go at the end of the submit. Refused: Skia's compaction by eviction,
  which rasterizes a page's visible items again in one frame.
- **Shrinking a list's only page is a drain into a smaller page.** Below a quarter full for an
  expiry window, the list opens a page in a free slot at the smallest power-of-two edge holding
  twice the live area and drains into it. Shrinking at a quarter and landing at a half keeps a list
  from shrinking and growing in turn, as a `Vec`'s capacity halves. Refused: a repack into a
  smaller texture in one frame, which moves the whole live set at once.
- **In one frame a page is a source or a destination, never both, and a draining page takes no new
  allocation**, so moves need no order among themselves and no pass samples its own target. A list
  that must grow while draining cancels the drain.
- **The plan runs at the end of the first submit of each display frame, where `end_frame` sweeps
  today, and its GPU work records first in the next submit**: page commits, moves, then frees,
  ahead of every upload, bake and draw. The next build of any window reads the new placements. As
  WebRender runs `run_compaction` at `begin_frame`.
- **A freed texture leaves every bind group and view that names it in the same commit**, as a bind
  group keeps its textures alive; its memory returns at wgpu's next maintain, which each submit
  runs.
- **The OS's memory pressure purges, on every platform (D4).** At a warning the pool evicts
  everything not drawn this display frame, frees idle scratch and empty pages and drains without
  its frame budget, a stutter traded for not being killed, as Chromium's memory-pressure
  listeners, WebRender's `clear_render_target_pool` and GLib's `GMemoryMonitor` advice to free
  caches. A watch per platform beside `PowerSaverWatch`, no new crate: macOS's
  `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE` through libdispatch's C API, Linux's portal
  `MemoryMonitor` over `zbus` (warning levels 50, 100, 255), Windows's
  `CreateMemoryResourceNotification` through `windows`. A Linux desktop with no portal reports
  nothing, which the watch documents.
- **Reclaim runs on painted frames only (D5)**, as `ExpiryWindow` decides today and as WebRender
  and Flutter's raster cache reclaim; an idle app never wakes for housekeeping, and D4 covers
  memory that is short. Refused: a maintenance wake, as Chromium's `ResourcePool` expires on a
  delayed task, a new wake kind in every host. Cost: an app idle after a glass closes keeps its
  pages until its next frame.

## Tenants

| tenant | allocator | read | a lost item costs | notes |
|---|---|---|---|---|
| glyphs | R8, RGBA8 pages | `textureLoad` | a swash raster | the run cache repacks `uv` on replay |
| icons | R8, RGBA8 pages | `textureLoad` | a resvg raster | one bind group with glyphs, which lets icons join the raster lane (`PERF.md`, To measure) |
| cutout tables | R8 pages | `textureLoad`, bilinear by hand | a bake pass | pinned while a plan holds one; code is page (2) · x (11) · y (11) · side − 1 (8), never all ones on a 2048 page |
| small backdrop results | RGBA16F pages | filtered inside the apron | a blur, or `Kept::Clear` while held | the page takes `BackdropSlot::pad` |
| large backdrop results | scratch | the same | the same | the composite's group binds the texture; draws split per large backdrop, a few a frame |
| gradient rows | row table | filtered at a row's center | a bake | ids stable while live; halving renumbers |

## Starting constants (M1 tunes them)

| allocator | first texture | page edge | pages | soft | hard |
|---|---|---|---|---|---|
| R8 pages | 256² · 64 KiB | 2048² · 4 MiB | 4 | 8 MiB | 16 MiB |
| RGBA8 pages | 256² · 256 KiB | 1024² · 4 MiB | 4 | 8 MiB | 16 MiB |
| RGBA16F pages + scratch | 256² · 512 KiB | 1024² · 8 MiB | 4 | 32 MiB together | a scratch side to the device edge |
| row table | 64 rows · 128 KiB | 256 wide | 1 | — | 4096 rows · 8 MiB |

Soft budgets are the sum of today's tenants'; a hard cap is four pages, one tenant's cap today.

## Expected memory (from the code, not measured)

| case | today | pool |
|---|---|---|
| before the first frame | 6.8 MiB GPU + 0.5 MiB CPU | 0 |
| after one busy moment | up to ~108 MiB GPU + 8 MiB CPU, for the device's life | the live set's pages, an expiry window plus the drain later |
| a 1920 × 1080 glass at full resolution | a 2048² texture, 32 MiB | scratch rounded to 2048 × 1536, 24 MiB |
| after that glass closes | 8–32 MiB, for the device's life | freed an expiry window later |

## Plan

Steps that each end green: the palantir gate, `cargo visual` where pixels can move, and the
benches a step names, A/B in ABBA order (`palantir-showcase/benches/AGENTS.md`). No public item
changes; a step that finds it must stops and asks. A step's heading says what of it is **done**.

### Exit criteria

- `integration/frame/gpu/*`, `text_atlas/*` and `backdrop_frame/*` within noise of the base on
  every bench host, worst frame included.
- No frame records more than one page made per list, one move batch per list within budget, and
  one eviction batch; a test holds each bound.
- The scripted scenarios of step 7.6 end at the pages their live sets need, to the byte.
- The two atlas entries of `.notes/ISSUES.md` and the `TODO.md` line "consolidate gpu atlas" are
  gone, and this file is deleted, its decisions in `src/gpu/AGENTS.md`.

### 0. Measure first — built; the memory measured, the timings to run

Nothing past step 0 builds on a number it has not measured.

- **0.1 Memory report — done.** `AtlasMemory`: per atlas its textures, their bytes and the bytes its
  live items fill, read from the textures and the slots when asked rather than counted, so a
  shipping build holds none of it. `cargo atlas-memory` prints it over each arm of
  `integration/frame` on the showcase's first page and every backdrop look and scene; measured on an
  Apple laptop. The pool reports the same once it holds textures, in step 2.
- **0.2 M2, moving rectangles — built.**
  `atomic/atlas_move/gpu/<copy|draw>/<r8|rgba8|rgba16f>/<side>/<n>` moves `n` ∈ {16, 64, 256}
  rectangles of 24² or 128² between two 2048² pages, submitted and waited for, as
  `atomic/gradient_upload` measures; each case first checks every texel lands. Settles the move
  mechanism per device and the budget's command count.
- **0.3 M3, the page branch — built.** `atomic/atlas_pages/gpu/<one|four>`: the device time of one
  pass drawing the steady text scene eight times over, the atlas bound as today or through the
  raster shader rewritten to four pages of each kind and a branch on the page, every quad on page 0.
  On V3D and Apple first. The scene draws masks alone, so the color branch goes unmeasured. Settles
  one page per draw or four. Removed in step 3, which binds four pages; V3D's verdict comes from
  step 3.6's A/B against the base instead.
- **0.4 M4, making a page — built.** `atomic/atlas_page/gpu/<r8|rgba8|rgba16f>/<edge>`: create, the
  first write of a 64² raster and the clear wgpu runs before it, at 256, 1024 and 2048.
- **0.5 M5, the mirrorless flush — built.**
  `atomic/gradient_atlas/cpu/gap_row/<mirror|bake>/<oklab|linear>`: one clean row staged from the
  mirror against baked again from its ramp; each case first checks that both give the same bytes.
- **0.6 Record — the memory and Apple's timings done.**
  `.notes/research/2026-11-12-atlas-pool-measurements.md`: on an Apple laptop the draw moves past a
  few dozen items for the cost of one, and the page branch costs nothing. To add: V3D, the AMD
  iGPU and `gap_row` in the bench profile, which settle the move rule per device and the branch.

### 1. The pool's CPU half (`src/gpu/atlas/`) — done

Pure bookkeeping, no wgpu type, so every rule is testable without a device. No tenant reaches it
until step 3, so its root expects `dead_code` outside tests, with that reason.

- **1.1 Layout.** `atlas/mod.rs` (`AtlasPool`, the owner, and the eviction queues),
  `atlas_format.rs` (`AtlasFormat`: `R8`, `Rgba8`, `Rgba16F`, with bytes per texel),
  `atlas_handle.rs` (handle and placement), `tenant.rs`, `error.rs`, `page_list/` (the list, its
  pages and slab), `scratch_cache.rs`, `eviction_clock.rs` (a clock its caller judges, which the
  raster atlas's `ClockSweep` folds into in step 3). `decay.rs` waits for step 7; the 0.1 report
  waits for step 2's textures.
- **1.2 Handles and the slab.** An entry holds its placement (page, origin, size), last use in
  display frames, a pin count and a tenant tag. `allocate`, `release`, `pin`, `unpin`, `stamp`.
  Generations move on release alone. Asserts: no release of a free handle, no unpin below zero, no
  stamp of a stale generation.
- **1.3 Page lists.** A list's pages, each a `ShelfPacker` and an edge; the first page doubles from
  256 to the page edge as `PackedAtlas` grows today (one texture at the final edge per frame); past
  it the list adds pages up to four, the hard cap. A page is `Open`, `Draining` or `Filling`;
  only `Open` and `Filling` take allocations. An item past the page edge is refused; one past half
  a page the list says `prefers_scratch`, which a per-draw tenant routes to scratch.
- **1.4 Eviction.** One clock per list over its tenants, skipping stamped-this-frame,
  stamped-last-frame and pinned entries. The insert path: allocate; under the soft budget grow;
  over it evict; nothing evictable, grow to the hard cap; at it refuse. Evicted handles queue per
  tenant, which each tenant drains to drop its keys.
- **1.5 Scratch.** A request rounds to Skia's ladder (powers of two to 1024, midpoints above, at
  least 16); `acquire` then reuses a free texture that covers the rounded size within twice its
  area, most recently freed first; `release` files it free with its frame; textures free past an
  expiry window.
- **1.6 Accounting.** Bytes per list across pages and scratch, live texels per page.
- **1.7 Tests.** Exact placements for a hand-worked sequence: a 64 first page doubles to the 128
  page edge keeping every origin, and a second page opens only at the edge. The insert path at
  each budget edge: evict over the soft budget, grow past it when nothing is evictable, refuse at
  four full pages. The clock never takes stamped, last-frame or pinned items, and queues each for
  its own tenant; a tall item empties bottom shelves until rows hold it. The scratch ladder on 1,
  16, 17, 1024, 1025, 1536, 1537 and 1936 × 1088; reuse within twice the area; expiry with the
  pool's frames. A seeded run of 4000 random inserts, releases, draws, pins and frames against a
  model, checking after each step that no two live rectangles overlap and that the accounting
  matches it.

### 2. The pool's GPU half (`src/gpu/atlas/atlas_textures/`) — done but 2.4, which waits for M2

- **2.1 Textures — done.** `AtlasTextures`: each page and scratch texture made through
  `texel::texture_2d`, every one `TEXTURE_BINDING | RENDER_ATTACHMENT | COPY_SRC | COPY_DST`, as a
  bake, a blur or a move by draw renders into it; color pages `Rgba8Unorm`.
- **2.2 Commit — done.** `commit(gpu, encoder, pool)`: a page opened makes its texture, a page
  grown in its slot makes a larger one and copies the old texels into its top-left, a page closed
  drops its texture; a scratch slot makes or drops its texture as its size comes and goes. Each
  page carries the epoch of its slot's opening, so a page opened again in a slot starts empty
  rather than copying the closed one's. Each list's generation moves when a view changes, which
  its consumers compare, as `FrameGroup` follows the gradient's. The backend calls it at the top
  of each submit's upload phase once a tenant moves onto the pool, in step 3.
- **2.3 Views and frees — done.** A list hands out four views, a closed page's the 1 × 1
  stand-in; a closed page's texture drops at the commit, and a consumer that binds the views makes
  its group again as the generation moves, which step 3's raster group is the first to do.
  `PageList::close_page`, which step 7's drain ends in, closes an empty page.
- **2.4 Moves — waits for M2.** Recorded by the mechanism M2 picks for the device, after the commit
  and before any upload, bake or draw. Test per format and mechanism: every texel of a drained page
  reads back byte for byte at its new place. Nothing moves before step 7.
- **2.5 Device loss — done.** Textures made again by their constructor take every open page at its
  edge at the first commit, empty for the tenants to fill again; a test holds it. The loss itself
  is the backend's, tested once the backend holds the pool in step 3.

### 3. Glyphs and icons — done but 3.6's A/B on the bench hosts

- **3.1 — done.** `RasterAtlas` is a tenant: its key map, its slab of rasters by pool handle with
  their bearings, the entries that draw nothing, and its uploads; the pool places, grows and evicts
  across both tenants, each draining its queue at the end of the submit, a lookup that finds its
  raster gone dropping the key first. Deleted: `Side`, `BoundSides`, `ClockSweep`, `FreeSlots`,
  `RasterAtlasConfig`, the raster atlas's `PackedAtlas` use and its grow bench.
- **3.2 — done.** `RasterQuad::uv_and_kind`: `u` in 12 bits, then 2 bits of page, the desaturate
  and kind flags, `v` from bit 16; a const assert holds the raster lists' page edges inside `u`.
- **3.3 — done.** The raster shader binds four pages of each kind and picks by the page bits, the
  spun path's four taps alike, and decodes the color pages' sRGB; M3 on Apple found the branch
  free, and V3D's run decides whether it stays. The page-branch bench went with the one-page
  binding it compared against. One visual golden, a turned color icon, moved by one step in 11
  pixels (D3) and was written again.
- **3.4 — done but its test.** An encoded glyph holds its raster's pool handle, so a replay stamps
  it in the pool and packs its `uv` from where it sits now, with no tenant slot between; the test
  of a glyph moved between two frames waits for step 7, as nothing moves before it.
- **3.5 — done.** Text and icons draw through one `RasterGroup`, which follows the textures'
  generations. The icons-in-the-raster-lane batching stays the separate `PERF.md` item.
- **3.6 Verify.** The gate and `cargo visual` pass. An indicative A/B in the test profile on an
  Apple laptop, ABBA: `integration/frame/gpu/{cached,scrolling,theming,typing}` and
  `text_atlas/gpu/{steady_warm,zoom_smooth}/frame` within noise; `steady_warm/prepare` 10.1 →
  12.3 µs, the pool's stamp and the repack a glyph, debug asserts on in that profile;
  `cache_churn/prepare` 61 → 20 µs. To run: the bench profile on every bench host, V3D first.

### 4. Shadow cutout tables — done but 4.5's A/B on the bench hosts

- **4.1 — done.** `CutoutPlacements` allocates from the R8 list, a pin per plan that holds a key,
  a released key unpinned, stamped and left to the clock; an eviction drops its entry through the
  queue, or at the next acquire that finds its handle stale. Each entry holds the span of its table
  baked, which replaced `HeldTables`: an evicted table's bake goes with it. Deleted: its packer,
  the cell grid, the fixed texture.
- **4.2 — done.** The table code is x (11) · y (11) · side − 1 (8) · page (2), low bits first;
  `CutoutPlan::code` and `shadow.wgsl`'s `cutout_origin`, `cutout_side` and page follow, their
  shifts substituted from Rust. A const assert holds that `NONE`, read as a table, runs past every
  mask page.
- **4.3 — done.** The bake renders a pass into each page that holds a table it bakes,
  `LoadOp::Load`, each instance carrying its page's edge. No scissor: wgpu has no render area, so
  a scissor would not narrow the load, and the draws cover only their spans already.
- **4.4 — done.** `renders_atlas`, its probe and the device's choice of mode are gone;
  `CutoutMode::Shade` stays as the plan's reference, which the visual suite holds the tables to
  (`CutoutPlan::shading`, internals). A test holds two baked tables, the series and the
  difference forms, to the integral within 3e-4 for the nodes and 0.6/255 for the 8-bit store,
  D3D's float-to-UNORM rule as Dawn's tests apply it; measured on Apple 2.2e-3 against 2.65e-3.
- **4.5 Verify.** The gate and `cargo visual` pass; one golden, `modal_dialog`, moved by one step
  in 510 pixels and was rewritten. To run: Pi and fanless shadow frames A/B.

### 5. Backdrop results — done but 5.5's A/B on the bench hosts

- **5.1 — done.** A result within half a page takes an item of the RGBA16F list, a larger one a
  scratch texture; `BackdropSlot::texture` names the page, or the scratch the draw's group binds.
  Each result read this frame is stamped before any allocation, so making room for one never
  evicts another; the drain lists the pool's evictions for each window's ledger.
- **5.2 — done.** A `BlurJob` writes into its `ResultTexture` at its origin, the down pass a pass
  a destination. The composite's group binds the four pages and one scratch texture, a group a
  scratch texture, made after the commit and kept while the views and the slot table hold; an
  effect run splits where a backdrop's scratch changes. The pool's texture generation now moves on
  a scratch texture's change too.
- **5.3 — done.** Deleted: the results `PackedAtlas`, `shrink_atlas`, `moving`, `moving_slots`,
  `past_budget`, `RESULTS_BUDGET`, and with its last user `PackedAtlas` itself (9.1's deletion,
  early). The memory study's results row is the half-float list, pages and held scratch.
- **5.4 — done.** Tests: a 400 × 300 result lands on a page; a 1936 × 1088 one takes a 2048 × 1536
  scratch texture, a 1900 × 1000 one reuses it once the first goes, and it frees an expiry window
  after; past the soft budget the pool evicts another window's result before it grows; a held
  result the pool evicted shows the clear; a scratch result reads through a group of its own.
- **5.5 Verify.** The gate and `cargo visual` pass, no golden moved. To run: `backdrop_frame/*` A/B
  on the board, the handheld and the mac.

### 6. Gradient rows

- **6.1** `GpuGradientAtlas` becomes the pool's row table: the texture and its copies through the
  commit. The frame group follows its generation as today.
- **6.2** `CpuGradientAtlas` halves below a quarter for an expiry window: live rows renumbered
  low, baked again from their ramps, at the start of the frontend's next frame. Ids stay while no
  halving runs. `EXPIRY_FRAMES` moves where `renderer` reaches it.
- **6.3** The CPU mirror goes; a flush bakes each run's rows, gaps included, into the staging.
  Tests: a halving keeps every live ramp's texels exact at its new row; a flushed run with a gap
  uploads the same bytes as the mirror did.

### 7. Decay

- **7.1 Age-out.** WebRender's age table over a list's pressure, at most 32 evictions a frame below
  four times the threshold; stamped, last-frame and pinned entries never.
- **7.2 Drain.** The emptiest page whose live items fit the others drains into the fullest pages
  first, within the move budget; a grow cancels it.
- **7.3 Shrink.** One page below a quarter for an expiry window opens a page at the smallest
  power-of-two edge holding twice its live area and drains into it.
- **7.4 Free.** An emptied page and an expired scratch texture go in the next commit.
- **7.5 When.** The plan runs at the end of the first submit of each display frame; its GPU half
  records first in the next submit.
- **7.6 Tests.** Exact plans: which page drains, which entries move and where, how many texels a
  frame. No page is source and destination in one frame. A pinned entry moves. A shrink lands at
  half. A list above a quarter never shrinks. Scripted on the offscreen host: a text burst then 300
  small frames ends at the first page's edge; a glass opened and closed frees its scratch an
  expiry window later; each frame in between within the bounds.
- **7.7 Verify:** the full A/B set of the exit criteria, worst frame included.

### 8. Memory pressure

- **8.1** `MemoryPressureWatch` beside `PowerSaverWatch` in `host/winit/platform`: macOS through
  libdispatch's C API, Linux through the portal's `MemoryMonitor` over `zbus`, Windows through
  `CreateMemoryResourceNotification`, `other.rs` a watch that reports nothing; each reports a
  warning to the host.
- **8.2** The host forwards it to the backend; the pool purges.
- **8.3** Test: a purge on a pool with stamped, idle and pinned entries leaves exactly the stamped
  and pinned ones.

### 9. Cleanup and docs

- **9.1** Delete `PackedAtlas`, its tests and `ExpiryWindow`'s results use; nothing outside the
  pool makes an atlas texture (`rg 'texture_2d|create_texture' src/gpu` lists only owned targets).
- **9.2** An "Atlases" section in `src/gpu/AGENTS.md` with this file's lasting decisions, each with
  its source; `raster_pass.rs`'s module doc follows D1.
- **9.3** Remove the two atlas entries of `.notes/ISSUES.md` and the `TODO.md` line; delete this
  file.
- **9.4** In Darkroom: move the submodule pointer, run its gate, and look at a glass-heavy project
  through `darkroom_open` and `darkroom_shot`.

Out of scope: the blur's capture, levels and scratch, which `Scratch decays` already governs and
which the scratch cache can take over later; the backbuffer and view targets; registered images,
textures of their own.
