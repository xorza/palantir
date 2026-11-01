# Perf journal

What seven optimization runs learned (2026-10-28 to 2026-11-01): how to measure on the hosts,
what was rejected and why, and what is still open. What was kept is in the commits.

## Measuring

- **Hosts.** local: AMD CPU with a Radeon mobile AMD iGPU (RDNA2 iGPU), Vulkan (RADV), pinned with
  `taskset -c 2 setarch -R`, governor and EPP at `performance`. fanless: a LAN host, an Apple laptop
  Air with 8 GB, Metal, no pinning; a bench build peaks at 2.8 GB there, so one build at a time.
  mac: a LAN host, an Apple laptop Mac with 24 GB, Metal, no pinning, which replaced the fanless during run 6;
  a bench build peaks at 4.1 GB there, through its `sccache`, which wants `/opt/homebrew/bin` on
  the `PATH`. Its CPU cases move up to 10 % between A/A runs at `schedule/cpu/walk`, under 1 %
  elsewhere.
  board: a LAN host, a VideoCore board (V3D through V3DV
  Mesa), Linux, pinned with `taskset -c 3 setarch -R`; its governor stays `ondemand`, as
  the user has no password-free `sudo` there. It builds nothing: the bench and test binaries are
  cross-built on local against a sysroot copied from it (glibc 2.41, `clang` and `lld`; a fat-LTO
  bench build peaks at 4.7 GB), and run there from worktrees at local's paths, which
  `CARGO_MANIFEST_DIR` names.
- **Run 7 hosts** (backdrops, 2026-11-01). mac is the build machine: it builds its own benches
  and cross-builds the board's with zig as the C compiler and linker (`zig cc -target
  aarch64-linux-gnu.2.41`, a wrapper that drops rustc's `--fix-cortex-a53-843419` and `-Wl,-O1`,
  which zig's linker refuses or ignores), in 53 s for the fat-LTO bench binary. handheld:
  a LAN host, a handheld (an AMD APU, RADV), pinned with `taskset -c 4 setarch -R`, governor `powersave` without `sudo`; it
  builds its own benches, peaking at 3.0 GB, and kills a user's processes when the SSH session
  that started them ends, so a run holds its session. win (a LAN host, the mobile AMD CPU under Windows
  11) was dropped: `cargo` over SSH hangs at 0 % CPU before it resolves the dependencies, and
  its `cargo.exe` proxy is 0 bytes, so `rustup run stable cargo` stands in. The Mac's macOS
  Local Network permission belongs to the app that started the `tmux` server: a server started
  before the permission gets `No route to host` to every LAN address until it restarts.
- **Run 7 noise floors** (A/A, ABBA): handheld frames ±1.5 % (`many/under` 2.4 %), composite and
  blur ±0.8 %; board under 0.3 % but `glass/hover` 0.7 %; mac frames ±1 %.
- **A shader change moves the last f16 bit on Apple GPUs.** Variants of the spectrum loop that
  are equal in IEEE arithmetic differ by up to 2⁻¹¹ (one f16 step at 1) on a few hundred texels:
  Metal compiles with fast math. A bit-exact check of a shader change compares frames on each
  host's own device (`f16diff`, a frame dump per look and pane shape).
- **Noise floors** (A/A over every case): CPU cases within 2 % local and 4 % fanless. GPU cases within
  4 % on fanless but `gradient_upload` (13 %); local GPU cases up to 43 % (`gradient_upload`), 32 %
  (`record_pass/images`), 12 to 16 % (`text_atlas`, `raster_atlas`), and 8 % on the frames.
- **CPU A/B across binaries moves by code placement.** A change a case never runs can move it
  4 % (`backdrop_frame/cpu/*/cached`), and a tight loop doubles when its branch crosses a
  64-byte line (`half_simd/from_f32x4/runtime_simd`, 60 ↔ 114 ns). A commit that only added a
  GPU bench moved `layout/cached` +5 % and `hit_test/click_focus/topmost` +8 %. Judge a small
  CPU change inside one binary, switched at run time, or read the moved case's code before
  believing it.
- **The board barely drifts**: an A/A of the frame and main-pass cases moves 0.0 to 0.3 %, and
  `record_pass` up to 5 %.
- **The fanless drifts.** It has no fan: one ABBA read a base binary 16 % slow on both A runs. Confirm
  a surprising fanless result with interleaved short runs.
- **The fanless writes no timestamps between passes**, so `backdrop_blur`, `backdrop_capture` and
  `backdrop_composite` read wall clock there, submit latency included.
- **Local GPU benches pay RADV's round trip**: an empty submit and wait is 250 µs local, 19 µs
  on fanless. Traced (`perf trace -e ioctl`; perf decodes amdgpu's CS ioctl as `DRM_I915_IRQ_EMIT`):
  a `scrolling` frame's submit takes 49 µs and its wait 590 µs (the 425 µs pass); a `partial` or
  `hovering` frame's submit takes 60 µs and 230 µs on alternate frames, asleep in the kernel, not
  on the CPU, and its wait 180 to 330 µs (the backbuffer's 179 µs copy out). Why the submit
  alternates is not found.
- **The mobile AMD iGPU's GPU frames are the APU's.** With the iGPU busy the core runs at 2.0 GHz with an
  IPC of 1.8 (3.2 idle), so `frame/gpu/partial` spends 1.1 ms in record against 0.18 ms in the
  CPU harness. Not a C-state: a busy-poll wait changes nothing. Read CPU work off the CPU arms.
- **V3D tools** (run 6). `V3D_DEBUG=shaderdb` prints each pipeline's instructions, threads,
  uniforms and spills, and `nir` the entry point of each (`decl_function … (entrypoint)`), which
  names a stats line; `perf` names V3DV's slow paths, such as a render area off the tile
  granularity, and `fs` the QPU code. A diagnostic build that swaps a shader module's body for a
  file at run time (`PALANTIR_DIAG_BODY=blur:/path.wgsl`, local branch `diag-body`) times shader
  variants on every host from one build each.
- **GPU tools.** `atomic/main_pass_batch` times each pipeline of a whole repaint from in-pass
  timestamps (not on Apple GPUs). RADV prints each pipeline's registers and occupancy under
  `RADV_DEBUG=shaderstats`. A diagnostic build with a shader that returns at once, or with
  blending off, splits raster from ALU. The fanless has no Xcode, so no Metal trace; RGP opens its
  captures only in its GUI, and the the distribution's RenderDoc ships no Python module.
- **Profiling.** DWARF call graphs do not unwind in the bench binaries; `perf record` flat
  profiles with `addr2line -i` over the sample addresses give the inlined frames. On the mobile AMD iGPU,
  RADV's RGP capture (`MESA_VK_TRACE=rgp`) frames at a present, so capture from the showcase;
  whether an offscreen bench triggers one is not tried.

## Where the time is

- **Run 6, the branch tip against its start**, medians (board one leg each, its A/A 0.0 to 0.3 %;
  mac and local ABBA):

  | case | board | mac | local |
  | --- | --- | --- | --- |
  | `frame/gpu/scrolling` | 21.1 → 18.2 ms | 712 → 663 µs | 2.29 → 2.33 ms (floor) |
  | `frame/gpu/partial` | 7.42 → 3.19 ms | 387 → 349 µs | 1.69 → 1.52 ms (floor) |
  | `frame/gpu/hovering` | 7.25 → 2.87 ms | 369 → 330 µs | 1.77 → 1.69 ms |
  | `frame/cpu/scrolling` | 586 → 545 µs | 131 → 131 µs | 224 → 225 µs |
  | `backdrop_frame/gpu/glass/scrolling` | 174 → 142 ms | 2.35 → 1.93 ms | 4.29 → 4.15 ms |
  | `backdrop_blur/gpu/4/2048` | 232 → 153 ms | 3.68 → 2.86 ms | 3.24 → 3.23 ms |
  | `main_pass/gpu/partial` | 3.84 ms → 161 µs | 13.2 → 11.4 µs | 5.42 → 6.06 µs |

  The partial main pass alone grows on local and fanless since the atlas holds its tables: with no
  bake pass before it, it opens the frame's work; the frames it belongs to shrink.

- **Run 7, the glass composite on V3D.** `fs_backdrop` with the spectrum loop compiled to 2869
  instructions, 1288 uniforms and 37 spills: the three 48-entry tables, constant arrays indexed
  by the cell, ran as a select over every entry, and the lens's registers spilled on every pane,
  glass or not (`V3D_DEBUG=shaderdb` with `MESA_SHADER_CACHE_DISABLE=true`, as a cached shader
  prints nothing). A blurred pane's composite: 56.5 ms at 2048² against 25.9 with no lens in its
  shader; glass at 512²: 13.8 ms against 5.4 with the tables in a buffer. The glass pipeline
  still spills (1410 instructions, 32:74 spills:fills); the plain backdrop pipeline does not
  (413 instructions).
- **Run 7, the glass frame's blur.** Glass blurs at σ 2 logical, 4 physical at 2×: the residual
  runs at level 0 with 25 taps an axis, the plan's most costly σ (`backdrop_blur/gpu/4/2048`:
  board 153 ms, handheld 4.7 ms, mac 2.9 ms, against 22, 1.3 and 0.5 ms at σ 16). A lens's reach used
  to grow its footprint by up to 41 px a side at 2×, which the residual blurred and the capture
  copied.
- **On the board the GPU is the frame** (run 6): `frame/gpu/scrolling` 21 ms of which the CPU half
  0.59 ms; the main pass of a whole 2560 × 1440 repaint 17.1 ms. Cutting one fragment entry at a
  time (`PALANTIR_DIAG`, a diagnostic build that makes an entry return at once): the tabled
  shadows 7.7 ms (their coverage 5.8, of it the cutout lookups 2.1 and the source clip 1.4), the
  quads 4.6 ms, text 0.16 ms, meshes and curves 0.06 ms, and the pass with every fragment entry
  trivial 4.8 ms. `V3D_DEBUG=shaderdb` gives `fs_shadow_tables` 564 to 698 instructions at 2
  threads, `fs_shadow` 9612 with 12 loops, `vs_shadow` 461 and 420 (binning) at 2 threads.
- **On the board a partial pass moved the whole target**: `main_pass/gpu/partial` was 3.84 ms, a
  load and a store of the 14.7 MB backbuffer, as V3DV runs the render area wgpu always sets to
  the whole attachment. See Decisions.
- **The board's backdrop blur**: a full-resolution residual at 2048² took 98 ms at σ 1 and 232 ms
  at σ 4 (the mobile AMD iGPU 2.6 and 3.3 ms), as each tap is a `textureLoad` of `Rgba16Float`; 70 and
  153 ms since the loop per device. In `backdrop_frame/gpu/blur/scrolling` (52 ms) the blur
  passes take 8 ms, the composite 4.6, the shadows 7.2, the quads 4.7; the rest is the pass
  breaks, each a load and a store of the whole target.
- **The board's cutout bake** took 2.5 ms of a 21 ms scrolling frame until the atlas held its tables.

- **CPU frame**: record dominates every arm (`frame/cpu/cached` is almost all record). The profile
  is flat at IPC 3.2: `open_node` 27 % (SoA push 4.6 %, chrome lowering 3.8 %), `widget_id`
  5.8 %, `Button::show` 6.8 %, `Text::show` 5.3 %, nothing else past 3 %. Only fewer
  instructions move it.
- **An action frame records twice** (`typing`: two passes, and a full cascade rebuild as `""`
  and `"x"` differ by one shape). That is the frame cycle's design.
- **Main pass GPU** of a whole fixture repaint on the mobile AMD iGPU (`main_pass_batch`): quads 151 µs,
  shadows 115 µs, text 9 µs. The quads are raster-bound: a shader that returns its fill at
  once times the same, and blending off entirely saves 40 µs (27 %). They shade 4.1 M fragments
  over 1.12 screens; 0.4 M lie under a later opaque interior. The shadows are 79 µs of raster
  and 46 µs of coverage math (a trivial shader, before the edge cells were tightened).
- **Backdrop passes** (run 5, `.notes/research/2026-10-29-backdrop-passes.md`): a blur pass costs
  the Apple laptop about 48 µs whatever it draws, five a pane, and the mobile AMD iGPU about 6 µs; the panes of one
  break now blur in one set of passes. On the mobile AMD iGPU a whole frame's backdrop work is the main
  pass's composite, then the blurs, then the captures (cards: 793, 610 and 248 µs).
- **The pane composite** (run 5, a temporary diagnostic override that cut one part at a time):
  on the fanless's 4K `modal/scrolling`, 4.9 ms a frame, the composite costs 1.4 ms over a pane that
  returns at once: the B-spline read 0.29 ms, grain and saturation 0.21 ms, the luminance ceiling
  and tint 0.16 ms, the dither nothing. On the mobile AMD iGPU at 2048² the B-spline is a third of it
  (605 against 408 µs with one fetch) and the grain 90 µs.
- **Shader statistics** (`RADV_DEBUG=shaderstats`): every fragment shader stays at 24 to 48 VGPRs
  and 20 to 32 waves a SIMD, with no spills; occupancy is not what bounds them.

## Rejected

- **Shadows: the blur's reciprocals per instance** (run 6): `1 / σ`, the reach and the cutout
  scale handed on flat from `vs_shadow` instead of worked out per pixel. Bit-exact on all three
  hosts and nothing gained: the board's main pass 17.11 → 17.08 ms, the fanless's +2.0 %, local within
  its floor. The SFU is not what bounds the tabled shadows on V3D.
- **Blur residual: one loop that mirrors each read, everywhere** (run 6). V3D: σ 4 at 2048²
  232 → 153 ms; the Apple laptop: 6.2 → 14.3 ms. **Two loops with the weights read four at a time,
  everywhere**: the Apple laptop −20 %, V3D +2 to +3 % at 512². So each device takes its own (Decisions).
- **Shadows: fewer `vs_shadow` invocations** through an index buffer over the cells' 24 distinct
  corners (run 6). The vertex stage costs nothing measurable on V3D: a main pass whose shadows
  compute every output and land off screen times as one with no shadow draws.
- **Blur: `LoadOp::Clear` on the tent and first-axis passes.** +8 % to +15 % on the mobile AMD iGPU's 512²
  cases, as a clear writes the whole mip; no change on Apple GPUs.
- **Blur: the residual as a compute pass over workgroup memory** (FidelityFX Blur's design).
  Both GPUs serve the residual's reads from texture cache as fast as from workgroup memory;
  the barrier and fill lose (mobile AMD iGPU +15 % on `1/2048`, m-series +86 %; wgpu also zero-fills
  workgroup memory per dispatch on Metal). Wins only on 512² high-σ, where it saves a render
  pass's fixed cost.
- **Blur: halve at σ above 2 texels, or cut the residual at 3σ.** The impulse response leaves the
  Gaussian by 3.5 % and 0.83 % of its peak, past the 0.4 % `blur_plan` promises.
- **Blur: an unsigned-compare mirror in the tents' edge fallback.** A test per read costs more
  than the remainders it skips (+3.1 % on `160/2048` local); it stays in the residual only.
- **Composite: dither by the sRGB slope.** No change on either host; the composite does not wait
  on its `pow`s.
- **Blur: two tents in one pass** (branch `spike-two-tents`), `[1 3 6 10 12 12 10 6 3 1] / 64`
  as five bilinear fetches an axis, exact at 1/16. It saves a pass, −11 % on
  `backdrop_blur/jobs/*` on the fanless, but reads 25 fetches a texel from the larger level: fanless
  `backdrop_blur/48/2048` +145 %, `backdrop_frame/gpu/*/scrolling` +17 %.
- **Mean and peak image taps** (`image_pipeline/minified_mean`, 39 ms on the mobile AMD iGPU against 4 ms
  for one tap): the bench reads a 5120² texture whole 24 times a frame, the mobile AMD iGPU's memory
  bandwidth. Only a mip chain reads less.
- **Clip per instance in the shader** (GPUI's content mask) to merge clipped groups:
  `record_pass/groups/per_item` pays 340 ns a group on Vulkan, but the frame fixture's main pass
  records in 23 µs of a 2.1 ms frame.
- **A hit-test rect column for SIMD**: a miss scans 0.5 ns a row local, 0.9 ns on fanless; a few
  hundred rows cost under 0.3 µs.
- **A byte compare before `AnimatedLook::eq`**: the look holds padding.
- **No blending for opaque quad interiors**, by a depth-ordered opaque pass (Skia Graphite's
  design) or core-and-ring quads: the ceiling is blending off for every quad, −40 µs of a 436 µs
  pass on the mobile AMD iGPU; Apple GPUs blend in tile memory. A depth attachment and a second draw order
  for under 2 % of the frame.
- **Discard a shadow pixel its source clips** instead of returning zero: −1.9 % on the shadows
  before the edge cells; the tighter cells leave only the corner cells' few such pixels.
- **A join billboard as one quad, not sixteen**: −11.5 % on `curve_pipeline/join`, but 8 pixels
  of `polyline_gradient` move by up to 235 levels. The clip planes that split a joint between its
  segments and its join test the interpolated `phys`, so the partition depends on the
  triangulation. Testing `@builtin(position)` would make it exact and free the triangulation, but
  it moves seam pixels against the goldens: a rendering change, not an optimization.
- **Every capturing frame through the backbuffer.** It saves each capture's copy, −6 % on the
  fanless's `cards_apart`, but the copy onto the target costs more than two small footprints: the
  mobile AMD iGPU's `pair_near/scrolling` +12 %. Kept only where the captures add up to the surface.
- **Sample the swapchain image in place of a capture.** wgpu 30 offers a sampled usage on a
  surface only on Vulkan, where the driver lists it: Metal's surfaces take render, copy and
  storage usages, DX12's render and copy, GL's render alone (wgpu-hal 30.0.1
  `metal/adapter.rs:474`, `dx12/adapter.rs:1360`, `gles/adapter.rs:1379`), so the fanless cannot
  take it. Where it is offered it costs a group each frame, as each acquire is a new texture.
  Metal's framebuffer-only path is not the obstacle: wgpu keeps it only for a usage of
  `COLOR_TARGET` alone (`metal/surface.rs:260`), and the surface already takes the copies.
- **A cheaper grain hash.** The grain costs the mobile AMD iGPU 90 µs at 2048²; a hash without integer
  multiplies changes the grain, so it is a rendering change, not an optimization.
- **A narrower results atlas** (`Rg11b10Ufloat`, `Rgb10a2Unorm`), for a B-spline read that is a
  third of the composite: a 6-bit mantissa or 10-bit linear bands under an 8-bit sRGB step.
- **`LoadOp::DontCare` on the blit**, which writes every texel with blending off. wgpu-core
  refuses it under `InstanceFlags::STRICT_WEBGPU_COMPLIANCE`, which a device the app opened can
  carry and not report, so the blit would fail validation there.
- **`is_finite() && v >= 0.0` in a value kind.** LLVM folds it into one class test that x86
  lowers to 14 integer ops; the kinds bound by `f32::MAX` instead, two compares.

- **Blur only the texels the composite reads** (run 7, branch `spike-tight-blur`). Each pass
  writes the region the footprint snaps to, though the texels within the blur's reach of its
  edge are past their support and nothing reads them; scissors cut to what the composite reads
  and, stage by stage, what that needs. Pi −0.7 to −5 % (`cards` −5 %), handheld −0.5 to −4.4 % but
  +0.6 to +1 % on the blur frames, mac −0.6 to −2 %. Refused: a held capture serves a pane whose
  footprint lies inside the snapped region, not inside the copied footprint, so cutting the
  texels past the support needs the footprint snapped to the lattice everywhere it is read
  (damage, the encoder's filter, the capture), for a few percent.
- **The spectrum loop's state** (run 7): its running integrals read again from the buffer each
  step rather than carried; the lens surface computed once for every wavelength; both. Pi: the
  glass composite −2.5, +3.3 and −2.4 %, its frames −0.4 % at best; handheld: the first +1.2 % on
  the glass composite at 512² and +0.4 to +0.7 % on its frames; mac within its floor. A V3D path
  for 0.4 % of a frame is not worth its branch.
## Decisions

- **The blur residual's loop follows the device** (run 6). `ResidualLoop::One` on Broadcom's
  VideoCore, one loop that mirrors each read where the window crosses the level's edge;
  `Windowed` elsewhere, a loop for a window inside the level and one that mirrors every read;
  both read the weights four at a time, and write the same texels bit for bit (a test). Chosen
  by an `override` constant at pipeline creation. Refused: one form for all, as each loses 2 to
  130 % on the other host (Rejected); a vendor list for Mali, Adreno and PowerVR, which nobody
  measured. Pi: σ 4 at 2048² −34 %, `glass/scrolling` −17 %; fanless: σ 4 −20 %, `glass` −13 %;
  local within ±1.3 %.
- **The cutout atlas keeps what it baked** (run 6). `CutoutTables` holds the tables of earlier
  frames, pairwise apart, and bakes of a frame's tables only what it lacks; the plan still
  places each frame's tables afresh, so a table the frames keep needing stays at its place.
  Refused: a cache per window's plan, which another window's bake into the shared atlas would
  void unseen; baking more tables now that a bake lasts, which changes which shadows read a
  table and so their pixels. Pi frames −13 to −21 %, fanless −9 to −12 %.
- **A partial frame paints a damage canvas where a pass moves its whole target** (run 6). On
  Arm, Imagination, Qualcomm and Broadcom GPUs (`PassTraffic::WholeTarget`, ANGLE's tile-based
  list less Apple) the main pass of a partial frame clears a `DamageCanvas` the size of its
  scissors' bounds out to 64 px tiles, in power-of-two size classes of which the stream keeps
  two, under the surface's viewport moved by the canvas's origin (wgpu 30 takes a viewport past
  the target), and copies each scissor into the backbuffer. The fragment entries that read their
  position add the origin (`frag_px`), so every pixel is the backbuffer path's: a test paints
  both on one device and compares. Refused: Vulkan's render area, which wgpu does not expose
  (Dawn has `render-pass-render-area` for it); the canvas on Apple GPUs, whose pass over the same
  target takes the fanless 32 µs, so it gains nothing there and costs 45 ns of encoding; shifting the
  varyings by the origin in the vertex stage, which saves the add but is not exact for a center
  left of half the origin; a canvas over half the surface, whose store and copies cost what the
  load did. Pi: `frame/gpu/partial` 7.37 → 3.73 ms, `hovering` 7.24 → 3.62 ms,
  `backdrop_frame/gpu/blur/hover` 6.38 → 2.71 ms; whole frames +0.2 to +0.3 %, the add in the
  shadows' fragments. fanless and local within their floors. Spike branch `spike-damage-canvas`.
  Sources: [Vulkan Guide, tile-based rendering](https://docs.vulkan.org/guide/latest/tile_based_rendering_best_practices.html),
  [Dawn render-pass-render-area](https://dawn.googlesource.com/dawn.git/+/refs/heads/main/docs/dawn/features/render_pass_render_area.md),
  ANGLE `vk_renderer.cpp` (`isTileBasedRenderer`), Mesa V3DV `v3dv_cmd_buffer.c`.

- **Glass draws through pipelines of its own, and its spectrum's tables are a buffer** (run 7).
  `QuadForm::BackdropLens` and `BackdropSpectrum` beside `Backdrop`, each a pipeline that the
  `QUAD_FORM` constant prunes; the spectrum tables a storage buffer the composite group binds.
  Refused: one shader with the lens behind a branch, whose registers V3D spilled on every pane;
  the tables as WGSL constant arrays, which V3D indexes by a select over every entry. Pi: a
  blurred pane's composite −54 %, `modal/scrolling` −34 %, glass composite −61 % at 512² and
  glass frames −26 to −41 %, `glass_plain` −14 % on the handheld; frames elsewhere within each
  host's floor. The module doc of `gpu::backdrop` holds both.
- **A glass pane's footprint holds what its lens reads, not its reach** (run 7).
  `LensOptics::margin`: a sample `d` px in moves inward along the edge's normal, and a rounded
  rect's chord along an inward normal is no shorter than its narrowest side, so a pane wider than
  its bezel, lean and a pixel together reads a pixel past itself; the pixel is for the normal the
  composite takes from screen differences, which can turn a pixel from the ridge. The cascade
  knows a pane's narrowest side only for a shape no clip cuts and no paint animation turns;
  chrome and the rest keep the reach. Glass frames: mac −8.8 to −10.6 %, handheld −7.2 to −8.3 %, board −5.6 to −6.6 %; `stacked`
  −4.5 to −6.5 %. Frames of every look and three pane shapes match the reach's bit for bit on
  the mac; on the handheld one texel of 54 frames moves by one f16 step at 1 (2⁻¹¹), where a smaller
  capture moves the tent's normalized fetch coordinates.

## Open

- **The glass pipeline spills on V3D** (run 7): 1410 instructions, 32:74 spills:fills, two
  threads. The loop carries the last read, the running integrals and the sum; the variants
  tried (Rejected) moved nothing.
- **Glass's σ is the plan's most costly**: σ 4 physical runs the residual at level 0, 25 taps an
  axis, where σ 4.01 halves first. Every exact cut tried is in Rejected (a halving at 2 texels,
  pairs of linear taps); the bilinear pair at the 8 sub-texel bits the board and the handheld report
  still moves a pair's weight by up to 0.39 % of the peak.
- **Glass chrome keeps the lens's whole reach**: the cascade keeps no chrome lift, so
  `LensOptics::margin` reads no narrowest side for it.
- **The Windows host's `cargo`** hangs over SSH before it resolves dependencies (run 7).

- **Skip the backdrop capture copy on a swapchain image.** A frame that paints the backbuffer
  copies nothing since run 5, and a frame whose captures would copy more than the surface paints
  there. A direct frame still copies: wgpu offers no sampled usage on a Metal or DX12
  swapchain (see Rejected). Skipping every copy took the fanless 5 % to 12 % off the frames that
  capture (`modal` 7.5 %).
- **A pane's break test reads one bounding box** of what was painted since the break, so in an
  L-shaped layout a pane breaks anew though no single draw touches its footprint. A short list
  of rects would see it; no bench has such a layout.
- **Shadow coverage per instance**: `filter_cdf_series` takes `1 / σ` and `h²/6` per pixel,
  where the vertex stage could hand them on flat. The coverage math is about 40 µs of the
  shadows' 115 µs on the mobile AMD iGPU.
- **A shadow corner stream per shadow, not per quad.** In a frame with a shadow every quad
  uploads 16 B of `CornerTables` (25 % over its 64 B), which only shadow instances read; in
  `CutoutMode::Shade` all of them are `NONE`. A stream per shadow needs the instance buffer bound
  at an offset for each shadow run, and the run's shadow ordinal. No bench shows the upload.
- **`Ordering::order` inlined into the curve lowering on the fanless.** Always inlined, as x86 needs
  (as a call, `curves/alone` +20 % and `same_tier_mesh` +25 % on the local host), the fanless's
  `curves/alone` runs 21 % slower than with `order` a call, while its `same_tier_mesh` runs 23 %
  faster. The curve function grows from 362 to 432 instructions; no profile says why.
- **The board's tabled shadows** (7.7 ms of a 17.1 ms main pass): the coverage math, not its
  reciprocals (Rejected), and 2 threads where V3D runs 4 at 16 registers. A lean entry for the
  edge cells, which take one span and no cutout, could run on 4; drawn per shadow, edge cells
  then corner cells, it keeps the blending order but doubles the shadow draws. Its ceiling: the
  pass without the cutout lookups takes 15.0 ms against 17.1.
- **The board's pass breaks**: each pane capture ends the main pass, a load and a store of the
  whole target, 3.8 ms on V3D. Painting what lies under the panes into capture canvases first,
  then the frame in one pass, trades the breaks for drawing each footprint twice, on a device
  whose pass moves its whole target only.
- **The cutout bake's cost model** assumes a bake a frame; with held tables more keys would pay,
  but which shadows read a table is a rendering decision (Decisions).
- **The board's vertex floor**: the main pass with every fragment entry trivial still takes 4.8 ms;
  `vs_shadow` runs once per vertex of 48 per shadow, binned and rendered.
- **The board's blur residual**: 98 to 232 ms at 2048². A pass's taps read a dynamically indexed
  uniform array and a 64-bit texel each; the bilinear pair, which would halve the reads, is
  refused for its 1/16 sub-texel weights (`blur.wgsl`).
- **Shader costs with a profiler.** RGP on the mobile AMD iGPU and Xcode's Metal System Trace on the fanless can
  now split the main pass (quads, shadows) and `backdrop_frame/gpu/*/under` on the fanless, which
  spends 1.6 ms over `hover` on two captures.
