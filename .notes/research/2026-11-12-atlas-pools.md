# Atlas pools in other renderers (2026-11-12)

Facts behind `.notes/proposals/atlas-pool.md`. `2026-10-28-atlas-growth.md` holds how an atlas
grows; this note holds how the field shares, binds, shrinks and compacts one.

## WebRender (texture cache)

Source: `webrender/src/texture_cache.rs`, `webrender/src/texture_pack/mod.rs`, servo/webrender
`main`.

- **One cache, a list of textures per format and filter.** `SharedTextures` holds six
  `AllocatorList<ShelfAllocator, _>`: `alpha8_linear` (R8, 1024), `alpha8_glyphs` (R8, 2048),
  `alpha16_linear` (R16, 512), `color8_linear` (2048), `color8_glyphs` (2048), `color8_nearest`
  (512). Glyphs and images of one format share the list machinery; the glyph lists are separate
  lists of it.
- **A list grows by a texture, never a bigger one.** `AllocatorList::allocate` tries each unit,
  then creates a square texture of the list's size.
- **Large items take a texture of their own.** `is_allowed_in_shared_cache`: a linear color item
  whose larger side passes half the list's size, anything wider or taller than 512
  (`TEXTURE_REGION_DIMENSIONS`), and nearest-filtered narrow formats go standalone; freeing one
  frees its texture at once.
- **Eviction by age, scaled by pressure.** In `begin_frame`, so a current-frame item is never
  evicted. The threshold is a third of a texture's bytes (two thirds for glyph lists); the age
  limit steps 400, 200, 100, 50, 25, 10, 5, 1 frames as `bytes_allocated / threshold` rises.
  `MAX_EVICTIONS_PER_FRAME = 32`, lifted past four times the threshold. Eviction stops once the
  list is under its threshold and holds one texture.
- **Compaction moves, bounded per frame.** `run_compaction` takes one list a call, round robin.
  `try_compaction` moves allocations from the last unit into the first, stopping when the first is
  full or past `max_pixels` (`512*512` moved pixels a frame); each move rewrites the handle and
  queues a texture-to-texture copy. The emptied unit is kept one frame (`delay_deallocation`),
  then `release_empty_textures` frees it at `end_frame`, so start-of-frame eviction can reuse it.

## Skia (Ganesh `GrDrawOpAtlas`)

Source: `src/gpu/ganesh/GrDrawOpAtlas.cpp`,
`src/gpu/ganesh/effects/GrAtlasedShaderHelpers.h`, skia `main`.

- **Up to `kMaxMultitexturePages` (4) pages bound together.** A new page activates only when no
  page fits the item; with all pages active the LRU plot is evicted.
- **The page rides in the vertex.** A 2-bit page index sits in bits 13–14 of the texel `x`;
  the fragment picks the sampler by a chain of `if (texIdx == i)`, the index a flat varying.
  So a draw never splits by page.
- **Compaction evicts, never copies.** `compact()` runs after a flush the atlas was used in, or
  past `kAtlasRecentlyUsedCount` (128) flushes. Plots of the last page unused for
  `kPlotRecentlyUsedCount` (32) flushes are evicted; when at most a quarter of the last page's
  plots are in use and an earlier page has a plot that idle, the used plot is evicted to make
  room there, and its glyphs upload again on their next draw. A page with no recent plot is
  deactivated (freed).

## WebRender (beyond the texture cache)

- **Compaction runs at the start of every frame**: `ResourceCache::begin_frame` calls
  `texture_cache.run_compaction` unconditionally (`webrender/src/resource_cache.rs`).
- **A batch binds a set of textures and breaks only on a clash.** `BatchTextures` is a
  `TextureSet` of three color inputs and a clip mask; `combine_textures` and `merge` fill empty
  slots, and a batch breaks when a slot holds another texture and no earlier batch takes the
  instance without crossing an overlap (`webrender/src/batch.rs`).
- **Render targets come from a pool apart from the texture cache**, which `gc_render_targets`
  trims at `end_frame`, oldest unused first, over a 64 MB threshold, a 320 MB red line and a
  60-frame window; `clear_render_target_pool` exists "e.g. on memory pressure"
  (`resource_cache.rs`).
- **Gradient stops are not texture-cache items**: `GradientGpuBlockBuilder` writes them as blocks
  of the GPU cache, now a GPU buffer, beside other per-primitive data
  (`webrender/src/prim_store/gradient/mod.rs`).
- **Glyphs and images keep separate lists of one format** because they never share a batch, which
  cut glyph textures and batch breaks; WebRender splits a 2048 atlas into two columns of shelves,
  which cut vertical fragmentation (<https://nical.github.io/posts/etagere.html>).

## Rectangle allocators (Nicolas Silva, étagère)

Source: <https://nical.github.io/posts/etagere.html>.

- Guillotine packs best but is slowest at thousands of items; WebRender keeps it for atlases of
  one frame. "Overall the best compromise was the simple shelf allocator", used in Firefox for
  glyph and image caches.
- A bucketed shelf frees a bucket only when all its items go, cheap for glyphs and poor for images;
  a simple shelf frees each item and merges or splits empty shelves anywhere, better over long
  sessions.

## Scratch textures

- **Skia rounds an approximate-fit texture up**: `skgpu::GetApproxSize` takes a side to the next
  power of two up to `kMagicTol` (1024), and above it to the midpoint between powers of two, at
  least 16; `createApproxTexture` looks for a scratch texture of that size before it makes one
  (`src/gpu/SkBackingFit.cpp`, `src/gpu/ganesh/GrResourceProvider.cpp`).
- **Chromium reuses within twice the area and expires unused resources**:
  `cc::ResourcePool::ResourceMeetsSizeRequirements` takes a resource at least as wide and tall
  whose area is at most `kReuseThreshold` (2.0) times the request; unused resources expire after
  `kDefaultExpirationDelay` (5 s) through a delayed task, and `ReduceResourceUsage` drops least
  recently used ones over the byte and count limits (`cc/resources/resource_pool.cc`).

## Impeller (glyph atlas)

- The atlas is 4096 wide (`kAtlasWidth`), at least 1024 high, and doubles its height, copying the
  old texture in; it never shrinks. Full at the maximum, it rebuilds with only the current frame's
  glyphs, a "GC" with no copy (`impeller/typographer/backends/skia/typographer_context_skia.cc`).

## Memory pressure

- macOS reports it through `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE`, levels `NORMAL`, `WARN` and
  `CRITICAL`; Chromium's `MemoryPressureMonitor` on the Mac listens to it
  (`base/memory/memory_pressure_monitor_mac.cc`); WebRender's `clear_render_target_pool` names
  memory pressure as its use, and Firefox delivers pressure events to WebRender (bug 1617083).

## Answers to the open decisions

- **Zed keeps glyphs and SVG icons in one list.** `AtlasKey::texture_kind` maps a glyph to
  `Monochrome` (`Polychrome` for emoji, `Subpixel` for subpixel text) and `AtlasKey::Svg` to
  `Monochrome` (`crates/gpui/src/platform.rs`).
- **Skia allows several glyph pages by default and gates them by driver.**
  `GrContextOptions::fAllowMultipleGlyphCacheTextures` defaults to `Enable::kDefault`, Skia's
  logic, which "can consider runtime properties such as the driver version";
  `fGlyphCacheTextureMaximumBytes` defaults to 8 MiB.
- **Skia's blur tables are 8-bit.** Its rect blur profile and round-rect blur mask are `kAlpha_8`
  textures, cached under "Rect Blur Mask" and "RoundRect Blur Mask" keys
  (`GrRectBlurEffect`, `GrRRectBlurEffect`, older revisions).
- **WebRender moves texture-cache bytes two ways.** `copy_from_staging_to_cache` issues a blit
  command a copy; `copy_from_staging_to_cache_using_draw_calls` draws `ps_copy` instances
  (`src_rect`, `dst_rect`), sorted by destination texture, chosen by
  `use_draw_calls_for_texture_copy` for drivers where "submitting hundreds of small blit commands"
  costs too much CPU (`webrender/src/renderer/upload.rs`, 0.69).
- **Memory-pressure sources.** GLib's `GMemoryMonitor` (Low Memory Monitor, a Flatpak portal) warns
  at `LOW` (50, free caches), `MEDIUM` (100, try harder) and `CRITICAL` (255, the system will soon
  kill processes) (docs.gtk.org, `MemoryMonitorWarningLevel`). Windows's
  `CreateMemoryResourceNotification` signals `LowMemoryResourceNotification` system-wide when
  physical memory runs low, a handle to wait on (learn.microsoft.com). Chromium's levels are
  `NONE`, `MODERATE` and `CRITICAL`, at 100 %, 50 % and 0 % of a consumer's limit
  (`base/memory/memory_pressure_listener.h`).

## Zed (`gpui_wgpu` `wgpu_atlas.rs`)

- A list of textures per kind: `Monochrome` (R8), `Subpixel` and `Polychrome` (the swapchain's
  color format), each texture at least 1024² (`DEFAULT_ATLAS_SIZE`), an
  `etagere::BucketedAtlasAllocator` each.
- A full list adds a texture; an emptied texture is dropped and its slot reused. No compaction.
- A tile is `(texture_id, bounds)`; a draw binds its tile's texture.

## wgpu

- **No sRGB view of a linear texture, or back, on GL.** `DownlevelFlags::VIEW_FORMATS`: "The
  WebGL and GLES backends doesn't support this" (wgpu-types 30.0.1 `limits.rs`); only the Vulkan
  adapter sets it in wgpu-hal 30.0.1.
- A texture cannot be resized and an array texture cannot gain layers
  (`2026-10-28-atlas-growth.md`).

## Palantir's own measurements

- **A copy command costs about 22 µs** on the AMD iGPU under RADV, a staged gradient row about
  0.42 µs (`2026-10-27-gpu-upload-and-binder.md`, "A copy against a staged gradient row").
- **A grow copies the whole texture**: 296 µs (`mask/1024`), 625 µs (`color/1024`), 914 µs
  (`mask/2048`) on an Apple laptop, against a 248 µs warm text frame
  (`2026-10-28-atlas-growth.md`).
