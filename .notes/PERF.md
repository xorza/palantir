# Perf journal

What five optimization runs learned (2026-10-28 to 2026-10-29): how to measure on the two
hosts, what was rejected and why, and what is still open. What was kept is in the commits.

## Measuring

- **Hosts.** local: Ryzen 7 6800U with a Radeon 680M (RDNA2 iGPU), Vulkan (RADV), pinned with
  `taskset -c 2 setarch -R`, governor and EPP at `performance`. m2: 192.168.0.7, an M2 MacBook
  Air with 8 GB, Metal, no pinning; a bench build peaks at 2.8 GB there, so one build at a time.
- **Noise floors** (A/A over every case): CPU cases within 2 % local and 4 % m2. GPU cases within
  4 % on m2 but `gradient_upload` (13 %); local GPU cases up to 43 % (`gradient_upload`), 32 %
  (`record_pass/images`), 12 to 16 % (`text_atlas`, `raster_atlas`), and 8 % on the frames.
- **CPU A/B across binaries moves by code placement.** A change a case never runs can move it
  4 % (`backdrop_frame/cpu/*/cached`), and a tight loop doubles when its branch crosses a
  64-byte line (`half_simd/from_f32x4/runtime_simd`, 60 ↔ 114 ns). A commit that only added a
  GPU bench moved `layout/cached` +5 % and `hit_test/click_focus/topmost` +8 %. Judge a small
  CPU change inside one binary, switched at run time, or read the moved case's code before
  believing it.
- **The m2 drifts.** It has no fan: one ABBA read a base binary 16 % slow on both A runs. Confirm
  a surprising m2 result with interleaved short runs.
- **The m2 writes no timestamps between passes**, so `backdrop_blur`, `backdrop_capture` and
  `backdrop_composite` read wall clock there, submit latency included.
- **Local GPU benches pay RADV's round trip**: an empty submit and wait is 250 µs local, 19 µs
  on m2. Traced (`perf trace -e ioctl`; perf decodes amdgpu's CS ioctl as `DRM_I915_IRQ_EMIT`):
  a `scrolling` frame's submit takes 49 µs and its wait 590 µs (the 425 µs pass); a `partial` or
  `hovering` frame's submit takes 60 µs and 230 µs on alternate frames, asleep in the kernel, not
  on the CPU, and its wait 180 to 330 µs (the backbuffer's 179 µs copy out). Why the submit
  alternates is not found.
- **The 680M's GPU frames are the APU's.** With the iGPU busy the core runs at 2.0 GHz with an
  IPC of 1.8 (3.2 idle), so `frame/gpu/partial` spends 1.1 ms in record against 0.18 ms in the
  CPU harness. Not a C-state: a busy-poll wait changes nothing. Read CPU work off the CPU arms.
- **GPU tools.** `atomic/main_pass_batch` times each pipeline of a whole repaint from in-pass
  timestamps (not on Apple GPUs). RADV prints each pipeline's registers and occupancy under
  `RADV_DEBUG=shaderstats`. A diagnostic build with a shader that returns at once, or with
  blending off, splits raster from ALU. The m2 has no Xcode, so no Metal trace; RGP opens its
  captures only in its GUI, and the Arch RenderDoc ships no Python module.
- **Profiling.** DWARF call graphs do not unwind in the bench binaries; `perf record` flat
  profiles with `addr2line -i` over the sample addresses give the inlined frames. On the 680M,
  RADV's RGP capture (`MESA_VK_TRACE=rgp`) frames at a present, so capture from the showcase;
  whether an offscreen bench triggers one is not tried.

## Where the time is

- **CPU frame**: record dominates every arm (`frame/cpu/cached` is almost all record). The profile
  is flat at IPC 3.2: `open_node` 27 % (SoA push 4.6 %, chrome lowering 3.8 %), `widget_id`
  5.8 %, `Button::show` 6.8 %, `Text::show` 5.3 %, nothing else past 3 %. Only fewer
  instructions move it.
- **An action frame records twice** (`typing`: two passes, and a full cascade rebuild as `""`
  and `"x"` differ by one shape). That is the frame cycle's design.
- **Main pass GPU** of a whole fixture repaint on the 680M (`main_pass_batch`): quads 151 µs,
  shadows 115 µs, text 9 µs. The quads are raster-bound: a shader that returns its fill at
  once times the same, and blending off entirely saves 40 µs (27 %). They shade 4.1 M fragments
  over 1.12 screens; 0.4 M lie under a later opaque interior. The shadows are 79 µs of raster
  and 46 µs of coverage math (a trivial shader, before the edge cells were tightened).
- **Backdrop passes** (run 5, `.notes/research/2026-10-29-backdrop-passes.md`): a blur pass costs
  the M2 about 48 µs whatever it draws, five a pane, and the 680M about 6 µs; the panes of one
  break now blur in one set of passes. On the 680M a whole frame's backdrop work is the main
  pass's composite, then the blurs, then the captures (cards: 793, 610 and 248 µs).
- **The pane composite** (run 5, a temporary diagnostic override that cut one part at a time):
  on the m2's 4K `modal/scrolling`, 4.9 ms a frame, the composite costs 1.4 ms over a pane that
  returns at once: the B-spline read 0.29 ms, grain and saturation 0.21 ms, the luminance ceiling
  and tint 0.16 ms, the dither nothing. On the 680M at 2048² the B-spline is a third of it
  (605 against 408 µs with one fetch) and the grain 90 µs.
- **Shader statistics** (`RADV_DEBUG=shaderstats`): every fragment shader stays at 24 to 48 VGPRs
  and 20 to 32 waves a SIMD, with no spills; occupancy is not what bounds them.

## Rejected

- **Blur: `LoadOp::Clear` on the tent and first-axis passes.** +8 % to +15 % on the 680M's 512²
  cases, as a clear writes the whole mip; no change on Apple GPUs.
- **Blur: the residual as a compute pass over workgroup memory** (FidelityFX Blur's design).
  Both GPUs serve the residual's reads from texture cache as fast as from workgroup memory;
  the barrier and fill lose (680M +15 % on `1/2048`, m-series +86 %; wgpu also zero-fills
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
  `backdrop_blur/jobs/*` on the m2, but reads 25 fetches a texel from the larger level: m2
  `backdrop_blur/48/2048` +145 %, `backdrop_frame/gpu/*/scrolling` +17 %.
- **Mean and peak image taps** (`image_pipeline/minified_mean`, 39 ms on the 680M against 4 ms
  for one tap): the bench reads a 5120² texture whole 24 times a frame, the 680M's memory
  bandwidth. Only a mip chain reads less.
- **Clip per instance in the shader** (GPUI's content mask) to merge clipped groups:
  `record_pass/groups/per_item` pays 340 ns a group on Vulkan, but the frame fixture's main pass
  records in 23 µs of a 2.1 ms frame.
- **A hit-test rect column for SIMD**: a miss scans 0.5 ns a row local, 0.9 ns on m2; a few
  hundred rows cost under 0.3 µs.
- **A byte compare before `AnimatedLook::eq`**: the look holds padding.
- **No blending for opaque quad interiors**, by a depth-ordered opaque pass (Skia Graphite's
  design) or core-and-ring quads: the ceiling is blending off for every quad, −40 µs of a 436 µs
  pass on the 680M; Apple GPUs blend in tile memory. A depth attachment and a second draw order
  for under 2 % of the frame.
- **Discard a shadow pixel its source clips** instead of returning zero: −1.9 % on the shadows
  before the edge cells; the tighter cells leave only the corner cells' few such pixels.
- **A join billboard as one quad, not sixteen**: −11.5 % on `curve_pipeline/join`, but 8 pixels
  of `polyline_gradient` move by up to 235 levels. The clip planes that split a joint between its
  segments and its join test the interpolated `phys`, so the partition depends on the
  triangulation. Testing `@builtin(position)` would make it exact and free the triangulation, but
  it moves seam pixels against the goldens: a rendering change, not an optimization.
- **Every capturing frame through the backbuffer.** It saves each capture's copy, −6 % on the
  m2's `cards_apart`, but the copy onto the target costs more than two small footprints: the
  680M's `pair_near/scrolling` +12 %. Kept only where the captures add up to the surface.
- **Sample the swapchain image in place of a capture.** It needs a sampled usage, which turns
  off Metal's framebuffer-only path, and a group each frame, as each acquire is a new texture.
- **A cheaper grain hash.** The grain costs the 680M 90 µs at 2048²; a hash without integer
  multiplies changes the grain, so it is a rendering change, not an optimization.
- **A narrower results atlas** (`Rg11b10Ufloat`, `Rgb10a2Unorm`), for a B-spline read that is a
  third of the composite: a 6-bit mantissa or 10-bit linear bands under an 8-bit sRGB step.
- **`is_finite() && v >= 0.0` in a value kind.** LLVM folds it into one class test that x86
  lowers to 14 integer ops; the kinds bound by `f32::MAX` instead, two compares.

## Open

- **Skip the backdrop capture copy on a swapchain image.** A frame that paints the backbuffer
  copies nothing since run 5, and a frame whose captures would copy more than the surface paints
  there. A direct frame still copies: the swapchain image would need a sampled usage, which
  turns off Metal's framebuffer-only path, and a group each frame. Skipping every copy took the
  m2 5 % to 12 % off the frames that capture (`modal` 7.5 %).
- **A pane's break test reads one bounding box** of what was painted since the break, so in an
  L-shaped layout a pane breaks anew though no single draw touches its footprint. A short list
  of rects would see it; no bench has such a layout.
- **Shadow coverage per instance**: `filter_cdf_series` takes `1 / σ` and `h²/6` per pixel,
  where the vertex stage could hand them on flat. The coverage math is about 40 µs of the
  shadows' 115 µs on the 680M.
- **Shader costs with a profiler.** RGP on the 680M and Xcode's Metal System Trace on the m2 can
  now split the main pass (quads, shadows) and `backdrop_frame/gpu/*/under` on the m2, which
  spends 1.6 ms over `hover` on two captures.
