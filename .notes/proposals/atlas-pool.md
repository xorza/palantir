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
| backdrop results | `Backdrops` over `PackedAtlas` | RGBA16F | 1024² · 8 MiB | 32 MiB / device edge | to 32 MiB, never below, never freed |
| shadow cutouts | `CutoutTables` + `CutoutPlacements` | R32F | 1024² · 4 MiB fixed, at startup | — | never |
| gradient rows | `CpuGradientAtlas` + `GpuGradientAtlas` | RGBA16F | 256 rows · 512 KiB + a CPU mirror | 4096 rows · 8 + 8 MiB | never |

- **6.8 MiB before anything draws**, and up to about 108 MiB of GPU memory and 8 MiB of CPU memory
  held for the device's life after one busy moment (`.notes/ISSUES.md`).
- **Two tenants per format double the headroom**, each texture keeping its own empty half.
- **A grow copies the whole texture**, 296–914 µs measured beside a 248 µs text frame.
- **A full-window backdrop pays its page**: a 1920 × 1080 result takes a 2048² RGBA16F texture,
  32 MiB for 16, kept after it goes.
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
  `touch_slot` already returns. Only eviction moves a generation.
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
changes; a step that finds it must stops and asks.

### Exit criteria

- `integration/frame/gpu/*`, `text_atlas/*` and `backdrop_frame/*` within noise of the base on
  every bench host, worst frame included.
- No frame records more than one page made per list, one move batch per list within budget, and
  one eviction batch; a test holds each bound.
- The scripted scenarios of step 7.6 end at the pages their live sets need, to the byte.
- The two atlas entries of `.notes/ISSUES.md` and the `TODO.md` line "consolidate gpu atlas" are
  gone, and this file is deleted, its decisions in `src/gpu/AGENTS.md`.

### 0. Measure first

Nothing past step 0 builds on a number it has not measured.

- **0.1 Memory counters.** A `BenchOnly` counter set per atlas texture of today: bytes made, live
  area, texture count, grows. Read in a bench-side report over every showcase page and the
  scrolling, theming and glass scenes. The pool keeps them as its own in step 1.
- **0.2 M2, moving rectangles.** `atomic/atlas_move/gpu/<copy|draw>/<r8|rgba8|rgba16f>/<n>` moves
  `n` ∈ {16, 64, 256} rectangles of 24 × 24 and of 128 × 128 between two 2048² pages, submitted and
  waited for, as `atomic/gradient_upload` measures. Settles the move mechanism per device and the
  budget's command count.
- **0.3 M3, the page branch.** A bench variant of the raster shader with four bound pages per
  format and the 2-bit switch, every instance on page 0, against today's on
  `integration/frame/gpu/*` and `text_atlas/*`, on V3D and Apple first. Settles one page per draw
  or four.
- **0.4 M4, making a page.** `atomic/atlas_page/gpu/<format>/<edge>`: create, first clear and one
  upload, per format at 256, 1024 and 2048.
- **0.5 M5, the mirrorless flush.** `atomic/gradient_upload` with gap rows baked from ramps against
  read from the mirror, on the gradient churn scene.
- **0.6 Record.** `.notes/research/<date>-atlas-pool-measurements.md`: the numbers, the hosts, and
  what each settles; then this file's constants and move rule.

### 1. The pool's CPU half (`src/gpu/atlas/`)

Pure bookkeeping, no wgpu type, so every rule is testable without a device.

- **1.1 Layout.** `atlas/mod.rs` (`AtlasPool`, the owner), `atlas_format.rs` (`AtlasFormat`:
  `R8`, `Rgba8`, `Rgba16F`, with bytes per texel), `atlas_handle.rs` (slab index and generation),
  `page_list.rs`, `scratch_cache.rs`, `eviction_clock.rs` (today's `ClockSweep`, moved),
  `decay.rs`, `counters.rs`.
- **1.2 Handles and the slab.** An entry holds its placement (page, origin, size), last use in
  display frames, a pin count and a tenant tag. `allocate`, `release`, `pin`, `unpin`, `stamp`.
  Generations move on release alone. Asserts: no release of a free handle, no unpin below zero, no
  stamp of a stale generation.
- **1.3 Page lists.** A list's pages, each a `ShelfPacker` and an edge; the first page doubles from
  256 to the page edge as `PackedAtlas` grows today (one texture at the final edge per frame); past
  it the list adds pages up to four. A page is `Open`, `Draining` or `Filling`; only `Open` and
  `Filling` take allocations. Items wider or taller than half a page are refused here and go to
  scratch.
- **1.4 Eviction.** One clock per list over its tenants, skipping stamped-this-frame,
  stamped-last-frame and pinned entries. The insert path: allocate; under the soft budget grow;
  over it evict; nothing evictable, grow to the hard cap; at it refuse. Evicted handles queue per
  tenant, which each tenant drains to drop its keys.
- **1.5 Scratch.** A request rounds to Skia's ladder (powers of two to 1024, midpoints above, at
  least 16); `acquire` then reuses a free texture that covers the rounded size within twice its
  area, most recently freed first; `release` files it free with its frame; textures free past an
  expiry window.
- **1.6 Accounting.** Bytes per format across pages and scratch, live area per page, the counters
  of 0.1.
- **1.7 Tests.** Exact placements for a hand-worked sequence on a 256 page: grow to 512 keeps
  every origin, a second page opens only at the page edge, a half-page item is refused to scratch.
  The insert path at each budget edge. The clock never takes stamped, last-frame or pinned
  entries. The scratch ladder on 1, 16, 17, 1024, 1025, 1536, 1537 and 1936 × 1088. A request
  rounds, then reuses: 1300 × 900 rounds to 1536 × 1024 and takes a free 2048 × 1536, twice its
  area, but not a free 2048², past twice. A seeded sweep of a few thousand random allocate,
  release, pin and stamp steps against a reference model, checking after each step that no two
  live rectangles overlap and that the accounting matches the model.

### 2. The pool's GPU half

- **2.1 Textures.** Each page and scratch texture made through `texel::texture_2d` with the usage
  its format's tenants need and `COPY_SRC | COPY_DST`; `RENDER_ATTACHMENT` where a tenant renders
  (R8, RGBA16F) or M2 picked the draw. Color pages are `Rgba8Unorm`.
- **2.2 Commit.** One `commit(ctx)` at the top of each submit's upload phase: grow the first page
  (one copy into the new texture), make new pages, then frees. Each list carries a generation that
  moves when its views change, which its consumers compare, as `FrameGroup` follows the gradient's
  today.
- **2.3 Views and frees.** A list hands out four views, an absent page bound as a 1 × 1 texture of
  its format. A freed texture's views and every bind group naming it drop in the same commit.
  Test: after a free, no consumer's group names the texture.
- **2.4 Moves.** Recorded by the mechanism M2 picked for the device, after the commit and before
  any upload, bake or draw. Test per format and mechanism: every texel of a drained page reads
  back byte for byte at its new place.
- **2.5 Device loss.** A test loses the device with a pool holding pages and checks that the next
  frame fills it again.

### 3. Glyphs and icons

- **3.1** `RasterAtlas` becomes a tenant: its key map, bearing and content in a side table by
  handle; allocation, eviction and growth through the R8 and RGBA8 lists. Delete `Side`,
  `BoundSides`, the per-tenant `RasterAtlasConfig` budgets and its `PackedAtlas` use.
- **3.2** `RasterQuad::uv_and_kind`: `u` in 12 bits, then 2 bits of page, the desaturate and kind
  flags, `v` from bit 16; `U_BITS` and the shader constants follow. Test: `pack_uv` and `clipped`
  round-trip the page and never carry into it.
- **3.3** The raster shader binds four pages per format and picks by the page bits, or one page
  per draw by M3; the spun path's four taps pick alike; it decodes the color pages' sRGB.
- **3.4** The encoded-run replay repacks each glyph's `uv` from `touch_slot`'s placement. Test: a
  glyph moved between two frames replays at its new origin, the exact packed value.
- **3.5** Text and icons bind one group. The icons-in-the-raster-lane batching stays the separate
  `PERF.md` item.
- **3.6** Verify: `cargo gate-test raster`, `cargo visual`, `text_atlas/*` and
  `integration/frame/gpu/*` A/B.

### 4. Shadow cutout tables

- **4.1** `CutoutPlacements` allocates from the R8 list, a pin per plan that holds a key, released
  keys unpinned and left to the clock. Delete its packer, the cell grid and the fixed texture.
- **4.2** The table code is page (2) · x (11) · y (11) · side − 1 (8); `CutoutPlan::code`,
  `cells` and `shadow.wgsl`'s `cutout_origin` and `cutout_side` follow. Test: the code of the
  largest table at the farthest origin of a 2048 page is not `NONE`, and every field round-trips.
- **4.3** The bake renders into each page that holds a table it bakes, `LoadOp::Load`, scissored
  to its tables.
- **4.4** Delete `CutoutMode::Shade`, `renders_atlas` and the probe. A test holds the
  baked table to the exact integral within the stated tolerance, which states its new 1/510 term.
- **4.5** Verify: `cargo visual` with the goldens rewritten on purpose, Pi and fanless shadow
  frames A/B.

### 5. Backdrop results

- **5.1** A result within half a page takes the RGBA16F list, a larger one scratch.
  `BackdropSlot::pad` names a page or the draw's scratch texture, which the composite picks.
- **5.2** A `BlurJob` writes into its item's texture and origin. The composite's group binds the
  four pages and one scratch texture; a draw splits at each large backdrop.
- **5.3** Delete the results `PackedAtlas`, `shrink_atlas`, `moving`, `moving_slots`, `past_budget`
  and `RESULTS_BUDGET`'s own use; the list's soft budget is 32 MiB across pages and scratch.
- **5.4** Tests: a 1936 × 1088 result takes a 2048 × 1536 scratch texture; one of 1900 × 1000 the
  frame after reuses it; it frees an expiry window after its backdrop goes; a 400 × 300 result
  lands on a page.
- **5.5** Verify: `cargo visual`, `backdrop_frame/*` A/B on the board, the handheld and the mac.

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
