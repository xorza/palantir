# Rendering

How palantir draws, from the renderer's instance streams through wgpu: the decisions any change to
`renderer`, `gpu` or the shared WGSL in `primitives::wgsl` keeps. Measurements behind them are in
`.notes/PERF.md` and `.notes/research/`.

## Module order

- **`gpu`'s submodules import only those to their left**, as the crate's top-level modules do
  (`internals::module_order` holds both):

  ```text
  {debug_marker, flag_names, render_owner_id, spectrum_tables, atlas_memory} → error → device
    → {texel, gpu_paint} → resource → atlas → pipeline → {raster, backdrop} → frame → surface
    → wgpu_backend
  ```

  A type two layers share sits in the lower: `GpuCtx` and the pipeline recipe in `resource`, a
  target's `TargetSpec` and `TargetFormat` in `texel`, what a backdrop's composite binds in
  `pipeline`, and a layer hands a lower one what it reads, a view and a size, not its own type.
  Refused: one cycle through eleven submodules, which no part of `gpu` could be read or tested
  apart from.

## Shapes

- **A closed shape draws as a template the vertex stage builds**: a fill, a shaped image, a
  material, an app effect and a shadow alike. Per corner a ring, per edge a band and an interior,
  each with a closed form of the shape's distance, between two rounded outlines; a halo and a
  shadow draw the rim alone, so no fragment runs in the hole. As Skia's `FillRRectOp` and
  Graphite's `AnalyticRRectRenderStep`, whose stroke leaves the center collapsed
  ([source](https://skia.googlesource.com/skia/+/main/src/gpu/graphite/render/AnalyticRRectRenderStep.cpp)).
  The one-quad form stays as the reference the visual suite compares with. Refused: a grid of
  cells around a hole, which shades its corners' squares; arcs cut into chords, as Impeller cuts
  circles, which err by a tolerance that grows with the radius; MSAA, which changes every
  pipeline's antialiasing; mesh shaders, experimental in wgpu and on no GL device.
- **Every closed shape is one kind on the GPU**: a sharp convex polygon and a radius at each
  corner, worked out on the CPU, so the vertex stage takes one path. Refused: a kind tag the vertex
  stage branches on.
- **Write for both GPU families**: an Apple GPU charges a fragment for each varying it reads, so a
  fragment reads only its form's; a tiler such as V3D runs the vertex stage in binning and again
  per tile, so the template draws indexed and takes no trigonometry.
- **Coverage is geometry, paint a function the fragment calls.** A pipeline is one geometry and
  one paint, pinned per form, as Skia Graphite pairs a RenderStep with a paint key and WebRender a
  quad with a pattern. Refused: every paint behind one dynamic branch, whose registers VideoCore
  cannot afford; a pipeline per shape kind.
- **Antialiasing is an interpolated distance**, or one `length` in a corner's ring. Refused:
  coverage as a vertex attribute, exact on straight edges only; MSAA.
- **Shared WGSL has one copy, under the `palantir_` prefix** the effect contract reserves.
  Refused: `naga_oil` and WESL, a dependency each for an include order a list of files states.
- **A draw call joins what it can**: a text run clips its glyphs on the CPU, so it batches with
  any other, and a closing group's quads join one run per form, looking back past quads they do
  not overlap, as WebRender's `batch_lookback_count`. Refused: sorting by pipeline with a depth
  buffer, as Graphite does, a depth attachment in every pipeline for a small ceiling.

## Rounded clip

- **A rounded clip is a coverage term each draw multiplies in**, the rounded rect's edge
  coverage, so a child that fills its clip draws its parent's edge antialiased; the scissor keeps
  the rect part. As Chromium's fast rounded corners
  ([`effect_node.h`](https://chromium.googlesource.com/chromium/src/+/HEAD/cc/trees/effect_node.h))
  and Skia's analytic clip
  ([Graphite `ClipStack.h`](https://skia.googlesource.com/skia/+/main/src/gpu/graphite/ClipStack.h)).
  Refused: geometry cut to the clip, which cannot cut an arc antialiased; a mask texture or an offscreen
  layer, a pass and a texture a clip. Cost: where two draws cross one edge pixel, the lower shows
  through by `c(1 − c)`, as in Skia and Chromium.
- **An instance names its chain of clips**, and nested clips multiply. Refused: the clip inline
  in each instance, as GPUI's `content_mask`; a chain per draw call, a batch per clip; a rect clip
  in the chain, whose code costs every frame more than the draws it saves.
- **A clip that cannot cut a draw leaves its chain**, tested on the CPU, as Skia's `ClipStack`
  drops an element another contains; an occluder covers only where its clip covers whole.
- **A frame with no rounded clip binds pipelines with the clip compiled out**, as its code costs
  a frame even unrun. Refused: one pipeline that branches on the chain.
- **The frame group is bind group 0**, the one that changes least, bound once a pass, as wgpu
  rebinds from the first slot whose layout differs. Refused: a fifth bind group, past
  `downlevel_defaults`'s four.
- **An effect takes the clip outside the app's code.**

## Atlases

Facts and sources: `.notes/research/2026-11-12-atlas-pools.md` and
`.notes/research/2026-11-12-atlas-pool-measurements.md`.

- **One pool a device owns every atlas byte, through three allocators by item shape** (`gpu::atlas`):
  packed pages for small 2D items, scratch textures for large ones, a row table for gradient rows;
  one budget a format, one decay, one commit, as WebRender's `ResourceCache` owns its texture
  cache, GPU cache and render-target pool. Refused: one allocator for every shape, which packs
  window-sized results into pages or splits gradient reads across them; an allocator a tenant,
  which doubled the headroom and the code.
- **A list a texel format, shared by every tenant of it**: R8 holds glyph and icon masks and the
  shadow cutout tables, RGBA8 color glyphs and icons, RGBA16F small backdrop results, as
  WebRender's `SharedTextures` and Zed's atlas keep a list a format. Glyphs and icons share their
  lists and one bind group, as Zed keeps glyphs and SVG icons in one `Monochrome` list; nothing a
  frame drew this frame or the last is evicted, so they compete for idle items alone. Refused: a
  budget a tenant, which kept a color-icon-heavy frame from evicting glyphs at the cost of each
  tenant's own headroom.
- **An allocation is a handle; a move rewrites its placement and nothing else.** Every tenant
  reads its placement at the lookup that draws it, so a drain needs no tenant's help; only an
  eviction moves a handle's generation. Each tenant drops what the pool evicted at the end of the
  submit; the backdrops after a purge too, as a window decides what its backdrops hold before its
  next submit, where a raster or a table reads as stale at its lookup.
- **A list binds up to four pages together, the page in the instance**, as Skia's
  `kMaxMultitexturePages`, so a draw never splits by page; a closed page binds a 1 × 1 stand-in.
  The first page doubles from its first edge to the page edge; past it the list adds pages.
  Refused: a texture array, which gains no layer without copying every layer; binding arrays,
  native only; fixed pages from the start, which charge a light app a whole page.
- **A pipeline that reads a list's pages is built for its open slots** (`PageSlots`): the shader
  picks a page by a chain of branches an override decides wherever it can
  (`palantir_reads_slot`), so over one open page it loads and never branches, in any slot, as
  Skia keys its text program on the atlas's active pages (`GrBitmapTextGeoProc`'s
  `numTextureSamplers`). Every pipeline whose shader can reach a page is one, the general shadow
  among them, which reads the tables its shadow's other corners hold. A choice among four compiled in full on V3D: the raster fragment 196 →
  943 instructions at half the threads, the board's whole repaints +2.7 to +3.1 %. Refused: a
  `switch` over the four; the slots through the last open one, which a shrink defeats, as it
  leaves a list's one page in slot 1; a draw a page, which splits a run of glyphs on two pages
  into a draw at each change. Cost: a page opening or closing builds the pipelines over the new
  set at the frame that changes it, once for each set a session meets.
- **Color pages store `Rgba8Unorm` and the raster shader decodes sRGB**, so a move copies their
  bytes exactly on every backend, as WebRender keeps color as plain RGBA. Refused: sRGB pages,
  which GL and WebGL give no linear view of. Cost: a color pixel may move by one step.
- **Shadow cutout tables are R8 in the mask pages**, as Skia keeps its blur profiles `kAlpha_8`: a
  quarter of `R32Float`'s memory, and a format every backend renders, so no device falls back to
  shading. Cost: a table texel errs half an 8-bit step more, which the bake's test bounds.
- **An item past half a page takes a scratch texture of its own, on Skia's `GetApproxSize`
  ladder, reused within twice its area as Chromium's `ResourcePool`, expired after an expiry
  window.** Only per-draw tenants take one. Refused: exact sizes, which make a texture each
  frame of a growing animation.
- **A backdrop's composite binds the one texture its result lies in**, a page or a scratch
  texture, so a draw splits where that texture changes, as WebRender binds each batch's
  textures: the composite reads one texture, as before the pool. Refused: the pages and a scratch
  texture in one group and a choice among five in the shader, the B-spline five times, which V3D
  compiled in full (619 → 1340 instructions, the board's `modal/scrolling` +6.9 %).
- **Gradient rows are a row table, not atlas items**, per-paint data every color pipeline reads
  branch-free through the frame group, as WebRender keeps gradient stops in its GPU cache. A row
  keeps its id while it lives; the table doubles when full and halves at the start of a frame that
  ends an idle expiry window under a quarter used, its live rows renumbered and baked again from
  their ramps. Refused: a CPU mirror of the texture, 2 KiB a row, kept only to fill a run's gaps,
  which a bake from the ramp fills with the same bytes.
- **Eviction is a clock with a one-frame grace and pins.** Nothing drawn this display frame or the
  last goes, which keeps a working set a little past budget from evicting what the next frame
  draws again, WebRender's least age at full pressure; a pin, a cutout table a plan holds, keeps
  an item from every eviction. Under the soft budget a list grows; over it, it evicts; with nothing
  evictable it grows to four pages; past them it refuses.
- **Decay is WebRender's: age out, then drain, then free, once a display frame** at the end of its
  first submit, the GPU's moves first in the next commit, ahead of every upload, bake and draw.
  The age shortens with the list's pressure as WebRender's `should_continue_evicting` table, at
  most 32 evictions a frame below four times the budget; the pressure is the bytes the items fill,
  as WebRender's `bytes_allocated`, and nothing ages under the budget on one page, grown or not,
  as WebRender ages under its threshold only across several textures (refused: a grown page aged
  under the budget, which evicted every glyph of a 512-scale zoom just before its scale came back,
  `cache_churn` +40 to +83 %); of several pages, the emptiest whose items fit half the others'
  free texels drains into them, a page whose drain found no room passed while what it counts
  holds, until a frame has moved 512² texels as WebRender's compaction, whose last move may pass
  them, so an item of more still moves; the frame's budget goes to the lists round robin, a list
  later each frame, as WebRender's compaction takes one list a call. One page a smaller one holds
  twice over for an expiry window, an eighth full at most, drains into the smallest page that holds
  twice its items' texels and packs them, landing at half or under as a `Vec` halves; one that
  holds nothing closes, so a list once drawn holds no page for good (refused: a first page kept,
  512 KiB of half-float an app that once showed a glass holds for its life). One page drains at a
  time, a source or a destination in one frame and never both, and no drain starts while moves
  wait for the commit, so no page closes under a move; the commit lands the moves in the order they
  were made, as a purge may land one where an item it evicted had just moved. Refused: Skia's
  compaction by eviction, which rasterizes a page's visible items again in one frame; a repack in
  one frame, which moves the whole live set at once.
- **Moves go by one instanced draw a destination page, or by copy commands where a pass moves its
  whole target** (`MoveBudget::of` and the GPU half's mover, from `PassTraffic`), WebRender's two
  routes. Where a pass moves only what it draws, a draw wins at every count measured: 16 glyphs
  107 µs against 222 on the handheld, and ties at 16 and wins past 64 on the Apple laptop. Where
  it moves the whole target, a draw loads and stores the whole destination page, 2.2 to 5.1 ms on
  V3D at 2048², so copies win below about 64 there, and a frame takes 16 copies, about 0.7 ms at
  V3D's 43 µs a copy, where 64 took 2.4 ms. Both stop at 512² texels a frame.
- **The system's memory warnings purge the pool on every platform**, between frames: everything no
  one pins and the last frame did not draw goes, idle scratch goes, and the pages drain and shrink
  with no budget, committed at once, round after round as each drain's moves land before the next
  is planned, so the memory returns with no frame to wait for; a stutter traded for
  the process not being killed, as Chromium's memory-pressure listeners and GLib's
  `GMemoryMonitor` advice to free caches. A watch a platform, no crate past the platform's: macOS's
  libdispatch memory-pressure source, Linux's portal `MemoryMonitor`, Windows's
  `CreateMemoryResourceNotification`.
- **Reclaim otherwise runs on painted frames only**, as WebRender and Flutter's raster cache
  reclaim; an idle app never wakes for housekeeping. Refused: a maintenance wake, a new wake kind
  in every host. Cost: an app idle after a glass closes keeps its pages until its next frame.

## GPU memory

- **Scratch decays.** Per-frame GPU scratch grows to a frame's need and steps down to the peak of
  a window of frames; a cache shrinks to its budget once its live set fits. Refused: a high-water
  mark for the window's life, which one 4K glass material holds for good; an image streamed in
  bands through one buffer, a GPU wait per band to bound a peak that lasts one submit.

## Swapchain

Facts and sources: `.notes/research/2026-10-29-swapchain.md`.

- **No frame reads the last frame from the swapchain**, as wgpu hands each image out
  uninitialized: a frame that keeps pixels paints through the backbuffer.
- **Never sample a swapchain image**, as Metal and GL offer no `TEXTURE_BINDING`.
- **Ask only for usages the surface offers**; a path that needs an optional one checks it.
- **Nothing is keyed by an acquired image**, as wgpu does not stop a stale view after a
  reconfigure.
- **`configure` runs only on a change**, as it waits for the device to idle.

## Device loss

Facts and sources: `.notes/research/2026-10-30-device-loss.md`.

- **Recover, do not panic**: a loss opens a new device and rebuilds every GPU object; the UI state
  stays. Refused: a panic, which a TDR turns into a crash (MDN `GPUDevice.lost`, Zed PR #50898).
- **A recovery runs the first attach again** through the calls startup makes, and device-bound
  state is made again by its constructors. Refused: a recovery path of its own, or a `reset` on
  each type, which drift (Zed PR #54349, DirectX DeviceResources).
- **The app uploads an image again** when `ImageHandle::is_resident()` is false, one check for
  every window. Refused: a CPU copy of every image, which doubles the RAM of large images; a
  per-window signal, answered twice for one image.
- **`GpuPaint::init` runs again.** Refused: a `device_lost` hook, as Rust drops the old handles.
- **`Gpu` owns the device's health.** Refused: callbacks set in each host; polling on the frame
  path. Cost: a loss in a blocking wait ends the process, as in Zed.
- **The texture limit is shared state**, as a new adapter's can be smaller.
- **Tests lose a real device** through `Gpu::lose`, as wgpu's tests do, and agents through
  `AgentWindow::lose_device`. By hand, `cat /sys/kernel/debug/dri/0/amdgpu_gpu_recover` as root
  resets the GPU for every app.
- **A harness's owner replaces its device**, as it handed the harness the first.
