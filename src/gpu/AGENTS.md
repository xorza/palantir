# Rendering

How palantir draws, from the renderer's instance streams through wgpu: the decisions any change to
`renderer`, `gpu` or the shared WGSL in `primitives::wgsl` keeps. Measurements behind them are in
`.notes/PERF.md` and `.notes/research/`.

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
  Refused: a stencil, which keeps or drops a whole pixel and breaks batches at each clip;
  geometry cut to the clip, which cannot cut an arc antialiased; a mask texture or an offscreen
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
