# Perf journal

What nine optimization runs learned (2026-10-28 to 2026-11-05): how to measure on the hosts,
where the time goes, what was refused and why, and what is still open. What was kept is in the
commits, each with its numbers per host.

## Measuring

- **Every A/B is a per-case ABBA**: `--exact <case> --measurement-time 3 --warm-up-time 1`, legs
  A, B, B, A, medians compared, the binary pinned with `taskset -c N setarch -R`. A sequential
  A/A moved the local CPU cases by up to 7 % and the GPU cases by up to 31 %.
- **Hosts.**
  - local: high-end Intel CPU, high-end NVIDIA GPU (dGPU only), Vulkan; core 2. A bench build peaks at
    4.7 GB cold, 3 GB after.
  - fanless: a LAN host, Apple laptop, 8 GB, Metal, no pinning; 2.2 to 2.8 GB a build, so one at
    a time. Login shell fish: run scripts with `bash`.
  - handheld: handheld@a LAN host, handheld (an AMD APU, RADV), core 4,
    governor `powersave` without `sudo`; 3.0 to 4.2 GB a build. Fish login shell. It kills a
    user's processes when the SSH session that started them ends, so a run holds its session.
  - win: a LAN host, AMD CPU with a Radeon mobile AMD iGPU, 30 GB, Windows, Vulkan; pinned through
    `ProcessorAffinity` 4 in a PowerShell ABBA runner; 4.2 to 4.7 GB a build. The rustup
    `cargo.exe` and `rustc.exe` are symlink proxies that fail over SSH (os error 448), and
    `sccache` does not start there: a build runs the toolchain's binaries by full path with
    `rustc-wrapper = ""`. A push to its bare repo needs `--receive-pack="powershell git
    receive-pack"`; criterion writes CRLF, and cargo prints `benches\palantir.rs` with a
    backslash.
  - Earlier: board, a LAN host, VideoCore board (V3D, V3DV), core 3, governor `ondemand`; it
    builds nothing, its binaries are cross-built against a sysroot copied from it (glibc 2.41)
    with `clang` and `lld`, or on the mac with `zig cc -target aarch64-linux-gnu.2.41` behind a
    wrapper that drops `--fix-cortex-a53-843419` and `-Wl,-O1`, and run from worktrees at the
    builder's paths, which `CARGO_MANIFEST_DIR` names. mac, a LAN host, Apple laptop Mac, 24 GB; its
    `sccache` wants `/opt/homebrew/bin` on the `PATH`. A macOS Local Network permission belongs
    to the app that started the `tmux` server: a server started before it gets `No route to
    host` to every LAN address until it restarts.
- **Noise floors** (ABBA A/A):

  | host | CPU cases | GPU cases |
  | --- | --- | --- |
  | local | under 1 %, at most 2 % | up to 8.4 % (`frosted/scrolling`), 7.4 % (`blur/scrolling`), 6.3 % (`pair_near/scrolling`) |
  | fanless | under 0.6 % | under 0.9 % |
  | handheld | under 0.7 %; `frame/cpu/alternating` and `backdrop_frame/cpu/blur/cached` bimodal, 12 to 15 % | median 0.7 %, up to 11 % on `*/cached` |
  | win | under 0.7 % | median 1 %, up to 8 %, 23 % on `pair_near/scrolling` |
  | board | under 0.3 % | under 0.3 %, `glass/hover` 0.7 %, `record_pass` 5 % |

- **CPU A/B across binaries moves by code placement.** A change a case never runs moved it 4 %
  (`backdrop_frame/cpu/*/cached`), a commit that only added a GPU bench moved `layout/cached`
  +5 %, and a tight loop doubles when its branch crosses a 64-byte line
  (`half_simd/from_f32x4/runtime_simd`, 60 ↔ 114 ns). Judge a small CPU change inside one
  binary, switched at run time, or read the moved case's code before believing it.
- **The fanless has no fan and drifts**: one ABBA read a base binary 16 % slow on both A legs. Confirm
  a surprising fanless result with interleaved short runs. It writes no timestamps between passes, so
  `backdrop_blur`, `backdrop_capture` and `backdrop_composite` read wall clock there.
- **Apple GPUs compile with fast math**: shader variants equal in IEEE arithmetic differ by up to
  one f16 step at 1 (2⁻¹¹). A bit-exact check of a shader change compares frames on each host's
  own device (`f16diff`).
- **An APU's GPU frames are not its CPU's**: on the mobile AMD iGPU under RADV an empty submit and wait took
  250 µs (19 µs on the fanless), and with the iGPU busy the core ran at 2.0 GHz with an IPC of 1.8,
  so `frame/gpu/partial` spent 1.1 ms in record against 0.18 ms in the CPU harness. Read CPU
  work off the CPU arms.
- **Tools.**
  - CPU: DWARF call graphs do not unwind in the bench binaries; `perf record --call-graph lbr`
    gives callers on Intel, and `perf annotate -l` ranks source lines. A diagnostic build that
    stubs one call out (no probe, no check) gives its ceiling in one ABBA.
  - GPU: `atomic/main_pass_batch` times each pipeline from in-pass timestamps (not on Apple
    GPUs); `RADV_DEBUG=shaderstats` prints registers and occupancy; a shader that returns at
    once, or blending off, splits raster from ALU. V3D: `V3D_DEBUG=shaderdb` with
    `MESA_SHADER_CACHE_DISABLE=true` prints instructions, threads and spills, `nir` names each
    entry point.
  - Missing: no Xcode on the fanless, so no Metal trace; RGP opens captures only in its GUI; the
    the distribution's RenderDoc ships no Python module.

## Where the time is

- **The CPU frame** (local `frame/cpu/hovering` 116 µs, LBR call stacks): record is nearly all
  of it. `open_node` 21.6 %, `Ui::response_for` 10.1 %, `post_record` 9.7 %, `Text::show`
  5.4 %, `Button::show` 4.9 %, the text shape's push 4.7 %, `widget_id` 3.8 %, the fixture's own
  `format!` and id hashing about 5 %, and before it was made one test, the chrome check
  `Widget::record` runs on every chromed widget, 4.4 %. Flat at an IPC above 4: only fewer
  instructions move it. A frame probes 1216 responses over 1245 nodes, 7.5 ns each; no probe in
  `Widget::show` is a ceiling of −2 to −7.8 %.
- **The scrolling frame**: `CascadeEngine::refresh` 13 %, `compute_paint_rect` 9.3 % and
  `DamageEngine::compute` 3.7 % of local `frame/cpu/scrolling`. A scroll moves every paint rect
  under the viewport by one translation, and the refresh computes each again in screen space,
  spread over the tree-item walk and the rect math.
- **An action frame records twice** (`typing`: two passes, and a full cascade rebuild as `""`
  and `"x"` differ by one shape). That is the frame cycle's design.
- **The theming frame is a resize frame** since the shape store: `cpu/theming` 288 µs against
  `cpu/resizing` 261 µs local; layout 40 % and record 35 % on the fanless. Before, shaping was 37 %:
  a 60 s transition moves a font size one 1/64 px step about every 6 frames, and a run off
  screen expired after probation and shaped again.
- **Current CPU numbers** (ABBA medians at `5ab2bdeef`):

  | case | local | fanless | handheld | win |
  | --- | --- | --- | --- | --- |
  | `frame/cpu/cached` | 97.9 µs | 120.0 µs | 238.3 µs | 160.6 µs |
  | `frame/cpu/hovering` | 112.2 µs | 133.6 µs | 271.8 µs | 182.5 µs |
  | `frame/cpu/scrolling` | 166.6 µs | 194.8 µs | 379.8 µs | 246.4 µs |
  | `frame/cpu/typing` | 276.6 µs | 322.9 µs | 634.8 µs | 422.8 µs |
  | `frame/cpu/theming` | 286.1 µs | 307.4 µs | 713.2 µs | 411.9 µs |
  | `backdrop_frame/cpu/glass/hover` | 116.7 µs | 138.6 µs | 275.1 µs | 187.0 µs |

- **The main pass on the mobile AMD iGPU** (`main_pass_batch`, a whole fixture repaint): quads 151 µs,
  shadows 115 µs, text 9 µs. The quads are raster-bound: a shader that returns its fill at once
  times the same, and blending off saves 40 µs (27 %); 4.1 M fragments over 1.12 screens, 0.4 M
  under a later opaque interior. The shadows: 79 µs of raster, 46 µs of coverage math. Every
  fragment shader sits at 24 to 48 VGPRs and 20 to 32 waves a SIMD, with no spills.
- **Backdrop passes** (`.notes/research/2026-10-29-backdrop-passes.md`): a blur pass costs the
  Apple laptop about 48 µs whatever it draws and the mobile AMD iGPU about 6 µs; on the mobile AMD iGPU a frame's backdrop work
  is the composite, then the blurs, then the captures (cards: 793, 610 and 248 µs). The
  composite on the fanless's 4K `modal/scrolling` costs 1.4 ms over a pane that returns at once: the
  B-spline read 0.29 ms, grain and saturation 0.21, the luminance ceiling and tint 0.16, the
  dither nothing; on the mobile AMD iGPU at 2048² the B-spline is a third of it and the grain 90 µs.
- **On the board the GPU is the frame**: `frame/gpu/scrolling` 18 ms, its CPU half 0.55 ms; a whole
  2560 × 1440 main pass 17.1 ms, of which the tabled shadows 7.7 (coverage 5.8: cutout lookups
  2.1, source clip 1.4), the quads 4.6, text 0.16, and every fragment entry trivial 4.8. In
  `backdrop_frame/gpu/blur/scrolling` (52 ms) the blur passes take 8 ms, the composite 4.6, the
  shadows 7.2, the quads 4.7, the rest the pass breaks, each a load and a store of the whole
  target. A blur residual at 2048² takes 70 ms at σ 1 and 153 ms at σ 4, each tap a
  `textureLoad` of `Rgba16Float`.

## Rejected

- **Shadows: the blur's reciprocals per instance**, handed on flat from `vs_shadow`: bit-exact,
  nothing gained (board 17.11 → 17.08 ms, fanless +2.0 %). The SFU does not bound the tabled shadows.
- **Shadows: fewer `vs_shadow` invocations** through an index buffer over the cells' 24 corners:
  the vertex stage costs nothing measurable on V3D.
- **Shadows: discard a pixel its source clips**: −1.9 % before the edge cells were tightened,
  which left only the corner cells' few such pixels.
- **Blur: one residual loop for every device**: the mirrored loop takes V3D's σ 4 at 2048² from
  232 to 153 ms and the Apple laptop's from 6.2 to 14.3; the windowed one wins the Apple laptop 20 % and loses V3D
  2 to 3 %. Each device takes its own (Decisions).
- **Blur: `LoadOp::Clear` on the tent and first-axis passes**: +8 to +15 % on the mobile AMD iGPU at 512², as
  a clear writes the whole mip.
- **Blur: the residual as a compute pass over workgroup memory** (FidelityFX Blur): both GPUs
  serve the reads from texture cache as fast; the barrier and fill lose (mobile AMD iGPU +15 %, m-series
  +86 %, and wgpu zero-fills workgroup memory per dispatch on Metal).
- **Blur: halve above σ 2 texels, or cut the residual at 3σ**: the response leaves the Gaussian
  by 3.5 % and 0.83 % of its peak, past the 0.4 % `blur_plan` promises.
- **Blur: an unsigned-compare mirror in the tents' edge fallback**: +3.1 % on `160/2048`; it
  stays in the residual only.
- **Blur: two tents in one pass**: −11 % on `backdrop_blur/jobs/*` on
  the fanless, but 25 fetches a texel from the larger level, `backdrop_blur/48/2048` +145 %.
- **Blur only the texels the composite reads**: board −0.7 to −5 %,
  handheld −0.5 to −4.4 % but +1 % on the blur frames. A held capture serves a pane inside the
  snapped region, so the cut needs the footprint snapped everywhere it is read (damage, the
  encoder's filter, the capture), for a few percent.
- **Composite: dither by the sRGB slope**: no change; the composite does not wait on its `pow`s.
- **Composite: a cheaper grain hash**, **a narrower results atlas** (`Rg11b10Ufloat`,
  `Rgb10a2Unorm`): each changes pixels, a rendering change and not an optimization.
- **The spectrum loop's state**, its integrals read again or the lens surface computed once:
  board −2.5 to +3.3 % on the glass composite, its frames −0.4 % at best; handheld +1.2 %.
- **Captures: every capturing frame through the backbuffer**: −6 % on the fanless's `cards_apart`,
  +12 % on the mobile AMD iGPU's `pair_near/scrolling`. Kept only where the captures add up to the surface.
- **Captures: sample the swapchain image**: wgpu 30 offers a sampled surface usage only on
  Vulkan (wgpu-hal 30.0.1 `metal/adapter.rs:474`, `dx12/adapter.rs:1360`,
  `gles/adapter.rs:1379`), and there it costs a bind group a frame, as each acquire is a new
  texture.
- **`LoadOp::DontCare` on the blit**: wgpu-core refuses it under
  `InstanceFlags::STRICT_WEBGPU_COMPLIANCE`, which a device the app opened can carry unseen.
- **No blending for opaque quad interiors** (Skia Graphite's depth-ordered opaque pass, or
  core-and-ring quads): a ceiling of −40 µs of a 436 µs pass on the mobile AMD iGPU; Apple GPUs blend in
  tile memory.
- **A join billboard as one quad, not sixteen**: −11.5 % on `curve_pipeline/join`, but the
  joint's clip planes test the interpolated `phys`, so 8 pixels of `polyline_gradient` move by
  up to 235 levels; testing `@builtin(position)` moves seam pixels against the goldens.
- **Mean and peak image taps**: the bench reads a 5120² texture 24 times a frame, the mobile AMD iGPU's
  memory bandwidth; only a mip chain reads less.
- **Clip per instance in the shader** (GPUI's content mask): 340 ns a group on Vulkan, but the
  fixture's main pass records in 23 µs of a 2.1 ms frame.
- **A hit-test rect column for SIMD**: a miss scans 0.5 ns a row; a few hundred rows cost under
  0.3 µs.
- **A byte compare before `AnimatedLook::eq`**: the look holds padding.
- **`is_finite() && v >= 0.0` in a value kind**: LLVM folds it into a class test x86 lowers to 14
  integer ops; the kinds bound by `f32::MAX`, two compares.
- **cosmic-text's `shape-run-cache`**: −28.7 % on `cpu/theming`, but its key builds a `String`
  and clones the attributes per lookup, and its `trim` walks the map every frame; the shape
  store beats it by 8 %.
- **Shapes from resident buffers alone**: −11 % on the fanless's `cpu/theming`; 62 % of the misses
  found no resident buffer, as runs never rendered expire after probation.
- **Kept shapes on the entries' own expiry wheel**: within the floor, `resize_drag` +2.4 % on the
  fanless.
- **A memo of the theme blend's Oklab conversions**: a ceiling of about 3 % of `cpu/theming`,
  for a second form of the public `Lerp` through every derive.
- **A probe that skips its interaction half** for a widget the pointer does not engage: within
  each host's floor, handheld +1.7 %. A disengaged widget's interaction half is a few compares.
- **A probe by the widget's resolve position**, which skips the `by_id` map for a container
  probed after its children: within ±1 %.

## Decisions

- **A chrome is checked in one branch-free test** (`5ab2bdeef`). Each part's lane checks
  gather into one array compare and the f16 sign test is SWAR; the asserts that name the broken
  rule run only on the cold path, so a panic keeps its message and caller. Refused: dropping the
  per-frame check for `debug_assert!`, against "a wrong value never draws quietly"
  (`AGENTS.md`, Input validation); a check that is only branch-free (−1 %, its cost unchanged).
  CPU frames: local −0.8 to −2.7 %, handheld −1.2 to −3.2 %, fanless −0.1 to −1.1 %, win −1.0 to
  +0.4 %; the ceiling is −1.6 to −5.2 %.
- **A shape outlives its size.** Shaping runs unhinted, so glyphs are in em units: a miss lays
  out the shaped lines of a resident buffer of its `ShapeIdentity` at another size or leading,
  else the buffer an expired entry left, as cosmic-text's `Buffer::set_metrics` lays out again
  and shapes nothing; another width shapes again into a recycled buffer. Refused:
  `shape-run-cache` and resident buffers alone (Rejected). `cpu/theming` −34.9 % local, −34.3 %
  fanless. Sources: cosmic-text 0.19 `shape.rs` (`x_advance / font_scale`), `buffer.rs`.
- **A theme transition blends in place**: `Lerp::lerp_into`, which the derive overrides field by
  field; a frame had cloned both 28 KiB themes for the by-value `lerp`. `lerp` and every
  `Animatable` method borrow their operands, as Bevy's `Animatable::interpolate` does.
  `cpu/theming` −2.6 % local, −2.0 % fanless.
- **The blur residual's loop follows the device**: `ResidualLoop::One` on Broadcom's VideoCore,
  `Windowed` elsewhere, by an `override` constant, both bit for bit (a test). Refused: one form
  for all (Rejected); a vendor list nobody measured. Pi σ 4 −34 %, fanless −20 %.
- **The cutout atlas keeps what it baked**: `CutoutTables` holds earlier frames' tables and bakes
  only what a frame lacks. Refused: a cache per window's plan, which another window's bake would
  void unseen; baking more tables, which changes which shadows read one. Pi frames −13 to −21 %,
  fanless −9 to −12 %.
- **A partial frame paints a damage canvas where a pass moves its whole target**: on Arm,
  Imagination, Qualcomm and Broadcom GPUs (`PassTraffic::WholeTarget`, ANGLE's tile-based list
  less Apple) a partial frame clears a `DamageCanvas` over its scissors' bounds and copies each
  scissor into the backbuffer, every pixel the backbuffer path's (a test). Refused: Vulkan's
  render area, which wgpu does not expose; the canvas on Apple GPUs, where it gains nothing.
  Pi `frame/gpu/partial` 7.37 → 3.73 ms. Sources:
  [Vulkan Guide, tile-based rendering](https://docs.vulkan.org/guide/latest/tile_based_rendering_best_practices.html),
  [Dawn render-pass-render-area](https://dawn.googlesource.com/dawn.git/+/refs/heads/main/docs/dawn/features/render_pass_render_area.md),
  ANGLE `vk_renderer.cpp` (`isTileBasedRenderer`), Mesa V3DV `v3dv_cmd_buffer.c`.
- **Glass draws through pipelines of its own, and its spectrum's tables are a buffer**:
  `QuadForm::BackdropLens` and `BackdropSpectrum`, which the `QUAD_FORM` constant prunes.
  Refused: the lens behind a branch, whose registers V3D spilled on every pane; the tables as
  WGSL constant arrays, which V3D indexes by a select over every entry. Pi glass frames −26 to
  −41 %. The module doc of `gpu::backdrop` holds both.
- **A glass pane's footprint holds what its lens reads, not its reach**: `LensOptics::margin`,
  from the pane's narrowest side where the cascade knows it. Glass frames −5.6 to −10.6 % on the
  board, handheld and mac; one texel of 54 handheld frames moves by one f16 step at 1.

## Open

- **A scroll as a translation**: paint rects kept in the space of the nearest scroll viewport,
  so a scroll changes one offset and not every row under it, as Chromium's property trees hold
  a scroll as one transform node and paint chunks in its local space. Damage diffs screen rects
  row by row and the encoder culls on them, so both would read through the offset. A ceiling of
  about a quarter of `frame/cpu/scrolling`; not spiked.
- **The response probe**: 5 to 8 % of a CPU frame at 7.5 ns a probe. What is left is the
  geometry half's two table reads and the 152-byte copies.
- **`FamilyKeys::key` and `TextSystem::refresh`** compare a 64-byte family name and each reused
  run's text every frame (`memcmp`, 1.2 % and 1.3 % on the fanless).
- **The glass pipeline spills on V3D**: 1410 instructions, 32:74 spills:fills, two threads.
- **Glass's σ is the plan's most costly**: σ 4 physical runs the residual at level 0, 25 taps an
  axis; the bilinear pair at the 8 sub-texel bits the board and handheld report moves a pair's weight by
  up to 0.39 % of the peak.
- **Glass chrome keeps the lens's whole reach**: the cascade keeps no chrome lift, so
  `LensOptics::margin` reads no narrowest side for it.
- **A direct frame still copies its captures**: no sampled swapchain on Metal or DX12
  (Rejected). Skipping every copy took the fanless 5 to 12 % off the frames that capture.
- **A pane's break test reads one bounding box** of what was painted since the break, so an
  L-shaped layout breaks anew though no draw touches the footprint; no bench has one.
- **Shadow coverage per instance**: `filter_cdf_series` takes `1 / σ` and `h²/6` per pixel; the
  coverage math is about 40 µs of the shadows' 115 µs on the mobile AMD iGPU.
- **A shadow corner stream per shadow, not per quad**: in a frame with a shadow every quad
  uploads 16 B of `CornerTables` that only shadow instances read. No bench shows the upload.
- **`Ordering::order` inlined into the curve lowering**: x86 needs it inlined (as a call,
  `curves/alone` +20 %), the fanless's `curves/alone` runs 21 % slower so and its `same_tier_mesh`
  23 % faster; no profile says why.
- **The board's tabled shadows** run at 2 threads where V3D runs 4 at 16 registers; a lean entry
  for the edge cells could run on 4, at twice the shadow draws. Ceiling: 15.0 against 17.1 ms.
- **The board's pass breaks**: each pane capture ends the main pass, 3.8 ms on V3D. Painting what
  lies under the panes into capture canvases first trades the breaks for drawing each footprint
  twice.
- **The board's vertex floor**: the main pass with every fragment entry trivial takes 4.8 ms.
- **The cutout bake's cost model** assumes a bake a frame; with held tables more keys would pay,
  but which shadows read a table is a rendering decision.
- **Shader costs with a profiler**: RGP on the mobile AMD iGPU and Xcode's Metal System Trace on the fanless
  would split the main pass and `backdrop_frame/gpu/*/under`, which spends 1.6 ms over `hover`
  on the fanless.
