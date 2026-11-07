# Perf journal

What the optimization runs learned (2026-10-28 to 2026-11-07): how to measure on the hosts,
where the time goes, what was refused and why, and what is still open. What was kept is in the
commits, each with its numbers per host. Runs 1 to 10 measured the frame fixture, 1,245 nodes, which
the showcase's first page, 246 nodes, has since replaced, so only run 11's numbers compare with a
new run.

## Measuring

- **Every A/B is a per-case ABBA**: `--exact <case> --measurement-time 3 --warm-up-time 1`, legs
  A, B, B, A, medians compared, the binary pinned with `taskset -c N setarch -R`. A sequential
  A/A moved the local CPU cases by up to 7 % and the GPU cases by up to 31 %.
- **Hosts.**
  - local: a high-end laptop, its NVIDIA GPU alone, Vulkan; one pinned core. A bench build peaks
    at 4.7 GB cold, 3 GB after.
  - fanless: a fanless Apple laptop with little RAM, Metal, no pinning; 2.2 to 2.8 GB a build, so
    one at a time.
  - handheld: a handheld AMD APU, RADV; one pinned core, governor `powersave` without `sudo`; 3.0
    to 4.2 GB a build. It kills a user's processes when the SSH session that started them ends, so
    a run holds its session.
  - windows: a mobile AMD iGPU, Windows, Vulkan; pinned through `ProcessorAffinity` in a PowerShell
    ABBA runner; 4.2 to 4.7 GB a build. The rustup `cargo.exe` and `rustc.exe` are symlink proxies
    that fail over SSH (os error 448), and `sccache` does not start there: a build runs the
    toolchain's binaries by full path with `rustc-wrapper = ""`. A push to its bare repo needs
    `--receive-pack="powershell git receive-pack"`; criterion writes CRLF, and cargo prints
    `benches\palantir.rs` with a backslash.
  - intel: a laptop's Intel integrated GPU, Vulkan through Mesa's anv; one pinned performance core,
    governor `performance`. A bench build takes 6.0 GiB of system memory cold and 5.8 GiB after,
    read off `MemAvailable`, as `rustc` runs under the `sccache` daemon outside cargo's process
    tree.
  - Earlier: board, a VideoCore single-board computer; one core, governor `ondemand`; it builds
    nothing, its binaries are cross-built against a sysroot copied from it (glibc 2.41) with
    `clang` and `lld`, or on the mac with `zig cc -target aarch64-linux-gnu.2.41` behind a wrapper
    that drops `--fix-cortex-a53-843419` and `-Wl,-O1`, and run from worktrees at the builder's
    paths, which `CARGO_MANIFEST_DIR` names. mac, a larger Apple laptop; its `sccache` wants
    the package manager's `bin` on the `PATH`. A macOS Local Network permission belongs to the app that
    started the `tmux` server: a server started before it gets `No route to host` to every LAN
    address until it restarts.
- **Noise floors** (ABBA A/A):

  | host | CPU cases | GPU cases |
  | --- | --- | --- |
  | local | under 1 %, at most 2 % | up to 8.4 % (`frosted/scrolling`), 7.4 % (`blur/scrolling`), 6.3 % (`pair_near/scrolling`) |
  | fanless | under 0.6 % | under 0.9 % |
  | handheld | under 0.7 %; `frame/cpu/alternating` and `backdrop_frame/cpu/blur/cached` bimodal, 12 to 15 % | median 0.7 %, up to 11 % on `*/cached` |
  | windows | under 0.7 % | median 1 %, up to 8 %, 23 % on `pair_near/scrolling` |
  | board | under 0.3 % | under 0.3 %, `glass/hover` 0.7 %, `record_pass` 5 % |
  | intel | under 1 %, `frame/cpu/partial` 1.9 %, `typing` 1.7 % | under 1.5 %, `frame/gpu/alternating` 6.3 % |

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
- **An APU's GPU frames are not its CPU's**: on the AMD iGPU under RADV an empty submit and wait took
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
  - intel: `INTEL_DEBUG=wm` with `MESA_SHADER_CACHE_DISABLE=true` prints each fragment shader's
    SIMD widths, instructions and spills; `/sys/class/drm/card*/gt_act_freq_mhz` reads the GPU's
    clock during a run. `perf report --children` over an LBR capture does not finish; a script that
    sums `perf script`'s call chains does. A frame case runs 1 s of warm-up and 3 s of samples
    whatever `--measurement-time` says, as its `Scale` sets both, so a capture of its frames alone
    starts after the setup and ends before the analysis (`perf record -D`).
  - Missing: no Xcode on the fanless, so no Metal trace; RGP opens captures only in its GUI; the
    distribution's RenderDoc ships no Python module.

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
  and `"x"` differ by one shape), unless `App::update` answers `Handled`.
- **The theming frame is a resize frame** since the shape store: `cpu/theming` 288 µs against
  `cpu/resizing` 261 µs local; layout 40 % and record 35 % on the fanless. Before, shaping was 37 %:
  a 60 s transition moves a font size one 1/64 px step about every 6 frames, and a run off
  screen expired after probation and shaped again.
- **Current CPU numbers** (ABBA medians at `5ab2bdeef`):

  | case | local | fanless | handheld | windows |
  | --- | --- | --- | --- | --- |
  | `frame/cpu/cached` | 97.9 µs | 120.0 µs | 238.3 µs | 160.6 µs |
  | `frame/cpu/hovering` | 112.2 µs | 133.6 µs | 271.8 µs | 182.5 µs |
  | `frame/cpu/scrolling` | 166.6 µs | 194.8 µs | 379.8 µs | 246.4 µs |
  | `frame/cpu/typing` | 276.6 µs | 322.9 µs | 634.8 µs | 422.8 µs |
  | `frame/cpu/theming` | 286.1 µs | 307.4 µs | 713.2 µs | 411.9 µs |
  | `backdrop_frame/cpu/glass/hover` | 116.7 µs | 138.6 µs | 275.1 µs | 187.0 µs |

- **The main pass on the AMD iGPU** (`main_pass_batch`, a whole fixture repaint): quads 151 µs,
  shadows 115 µs, text 9 µs. The quads are raster-bound: a shader that returns its fill at once
  times the same, and blending off saves 40 µs (27 %); 4.1 M fragments over 1.12 screens, 0.4 M
  under a later opaque interior. The shadows: 79 µs of raster, 46 µs of coverage math. Every
  fragment shader sits at 24 to 48 VGPRs and 20 to 32 waves a SIMD, with no spills.
- **Backdrop passes** (`.notes/research/2026-10-28-backdrops.md`): a blur pass costs the
  Apple laptop about 48 µs whatever it draws and the AMD iGPU about 6 µs; on the AMD iGPU a frame's backdrop work
  is the composite, then the blurs, then the captures (cards: 793, 610 and 248 µs). The
  composite on the fanless's 4K `modal/scrolling` costs 1.4 ms over a pane that returns at once: the
  B-spline read 0.29 ms, grain and saturation 0.21, the luminance ceiling and tint 0.16, the
  dither nothing; on the AMD iGPU at 2048² the B-spline is a third of it and the grain 90 µs.
- **On the board the GPU is the frame**: `frame/gpu/scrolling` 18 ms, its CPU half 0.55 ms; a whole
  2560 × 1440 main pass 17.1 ms, of which the tabled shadows 7.7 (coverage 5.8: cutout lookups
  2.1, source clip 1.4), the quads 4.6, text 0.16, and every fragment entry trivial 4.8. In
  `backdrop_frame/gpu/blur/scrolling` (52 ms) the blur passes take 8 ms, the composite 4.6, the
  shadows 7.2, the quads 4.7, the rest the pass breaks, each a load and a store of the whole
  target. A blur residual at 2048² takes 70 ms at σ 1 and 153 ms at σ 4, each tap a
  `textureLoad` of `Rgba16Float`.

- **The GPU frames** (run 10): a GPU frame waits for its device each frame, so the CPU and the
  GPU never overlap in the bench. On the APUs the CPU half is most of it: on a handheld the CPU is
  busy 77 % of `frame/gpu/hovering`, record at an IPC of 1.6 against 2.6 in the CPU frame at the
  same 2.4 GHz, and the wgpu, RADV and kernel calls about 1.4 M instructions a frame more than
  the CPU harness. The passes of a partial frame are small (`main_pass/gpu/partial` 7 µs local,
  37 µs fanless, 13 µs handheld, 17 µs windows); the copy of the whole backbuffer onto the target is not:
  without it `frame/gpu/hovering` and `partial` run 16 to 33 % faster on every host
  (diagnostic). A whole repaint's main pass takes 0.68 ms on the fanless, 1.0 ms on the handheld and
  1.04 ms on windows, of it the quads 426 and 553 µs and the shadows 356 and 297 µs on the handheld and
  windows; windows's `frame/gpu/scrolling` spends 1.6 ms of its 2.9 outside the passes and the CPU
  half, in its submit and fence wait.

- **The GPU frames on the intel** (run 11): the frame waits on the device for 1.3 to 1.9 ms of its
  1.6 to 2.3 ms, and the GPU runs at 300 of its 1400 MHz all run long, as the kernel's frequency
  control picks for a device busy part of each frame. A pass writes about 8 pixels a GPU clock, so
  pixels decide: the copy of a 2560 × 1440 backbuffer onto the target takes 1.1 ms, 75 % of
  `frame/gpu/partial` (1,640 µs with it, 412 µs without, diagnostic); the whole repaint of
  `scrolling` spends 0.73 ms on 2.3 MP of quads, 0.21 ms on text and 0.28 ms on the clear, the
  submit and the wake. Blending off takes 8.6 % off that wait, and a mask-only text shader 3.6 %
  off the text batch. The same copy takes 1,085 µs under a sleeping wait and 536 µs under a
  spinning one: with its core asleep the package lowers the clocks the GPU's memory traffic rides
  on, as a desktop frame that waits for vsync does too. The CPU half is 210 to 380 µs against 31 to
  150 µs in the CPU harness: wgpu-core and anv record the commands in `CommandEncoder::finish`
  (56 to 84 µs), `Queue::submit` takes 27 to 44 µs, and palantir's own upload and draw loop 3 %.
- **Current numbers** (run 11, ABBA medians at `880111750`, the intel):

  | arm | `frame/cpu` | `frame/gpu` |
  | --- | --- | --- |
  | `cached` | 31.3 µs | 32.1 µs |
  | `hovering` | 38.4 µs | 1,572 µs |
  | `partial` | 49.2 µs | 1,653 µs |
  | `alternating` | 57.6 µs | 2,272 µs |
  | `scrolling` | 60.4 µs | 1,845 µs |
  | `resizing` | 76.3 µs | 1,989 µs |
  | `typing` | 91.3 µs | 1,681 µs |
  | `theming` | 148.0 µs | 2,216 µs |

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
- **Blur: `LoadOp::Clear` on the tent and first-axis passes**: +8 to +15 % on the AMD iGPU at 512², as
  a clear writes the whole mip.
- **Blur: the residual as a compute pass over workgroup memory** (FidelityFX Blur): both GPUs
  serve the reads from texture cache as fast; the barrier and fill lose (AMD iGPU +15 %, m-series
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
  +12 % on the AMD iGPU's `pair_near/scrolling`. Kept only where the captures add up to the surface.
- **Captures: sample the swapchain image**: wgpu 30 offers a sampled surface usage only on
  Vulkan (wgpu-hal 30.0.1 `metal/adapter.rs:474`, `dx12/adapter.rs:1360`,
  `gles/adapter.rs:1379`), and there it costs a bind group a frame, as each acquire is a new
  texture.
- **`LoadOp::DontCare` on the blit**: wgpu-core refuses it under
  `InstanceFlags::STRICT_WEBGPU_COMPLIANCE`, which a device the app opened can carry unseen.
- **No blending for opaque quad interiors** (Skia Graphite's depth-ordered opaque pass, or
  core-and-ring quads): a ceiling of −40 µs of a 436 µs pass on the AMD iGPU under RADV; on Windows'
  AMD iGPU and the handheld turning quad blending off moves no frame (run 10). The fanless reads otherwise (Open).
- **A join billboard as one quad, not sixteen**: −11.5 % on `curve_pipeline/join`, but the
  joint's clip planes test the interpolated `phys`, so 8 pixels of `polyline_gradient` move by
  up to 235 levels; testing `@builtin(position)` moves seam pixels against the goldens.
- **Mean and peak image taps**: the bench reads a 5120² texture 24 times a frame, the AMD iGPU's
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

- **The backbuffer drawn out on every device** (run 10): against the copy, Windows' AMD iGPU +8 to
  +10 % on the partial frames, the NVIDIA GPU +4 to +5 %, the handheld within its floor, and in
  run 11 the intel host's iGPU +37 % on the wait of `frame/gpu/partial` (1,366 to 1,865 µs). Kept on Apple
  GPUs alone (Decisions).
- **Opaque fast quads without blending** (run 10): a solid, sharp, pixel-aligned quad of full
  alpha through a pipeline with no blend, the same pixels. Every case within each host's floor:
  such quads are rare, and the blended quads hold the fanless's ceiling (Open).

- **A mask-only text shader** (run 11), the color and resampled paths pruned as `QUAD_FORM` prunes
  the quad's: `main_pass_batch/gpu/text` −3.6 % on the intel, every frame within its floor. Its
  SIMD8 and SIMD16 forms spill nothing, and one texel load serves a mask glyph either way.

## Decisions

- **A chrome is checked in one branch-free test** (`5ab2bdeef`). Each part's lane checks
  gather into one array compare and the f16 sign test is SWAR; the asserts that name the broken
  rule run only on the cold path, so a panic keeps its message and caller. Refused: dropping the
  per-frame check for `debug_assert!`, against "a wrong value never draws quietly"
  (`AGENTS.md`, Input validation); a check that is only branch-free (−1 %, its cost unchanged).
  CPU frames: local −0.8 to −2.7 %, handheld −1.2 to −3.2 %, fanless −0.1 to −1.1 %, windows −1.0 to
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
  for all (Rejected); a vendor list nobody measured. board σ 4 −34 %, fanless −20 %.
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

- **The backbuffer reaches the target by a draw on Apple GPUs** (run 10). `BackbufferOut::Draw`
  on Metal with memoryless storage, a full-screen blit in a pass that clears the target, where
  the copy reads it through another engine; `Copy` elsewhere, as the draw loses on the immediate
  GPUs (Rejected). The blit reads each texel by index into the same format, so both paint the
  same pixels (`a_canvas_paints_what_the_backbuffer_paints`). Refused: the draw on every device
  whose transient attachments save memory, the other tilers, which nobody measured. fanless:
  `backdrop_frame/gpu/blur/sliding` 699 → 656 µs (−6.2 %), `glass/under` −4.5 %,
  `modal/scrolling` −3.4 %; the rest within 1 %.

- **A frame's repaint follows its scene and its rects' waste** (`cargo damage-sweep`, which holds
  the catalog, the grid and how to read it). A pair of rects merges by the pixels its bounds add
  past what the two cover, 5,000 px² at most, overlapping or not. A frame repaints whole past a
  share of `d / (d + 400)`, of the density `d`: the rows that draw on the surface per million
  physical pixels, and 300 for all of the surface under pane footprints. Sum of CPU and GPU, each
  scenario's regret past its cheapest decision as a percent of that frame, over six stages of
  content, scale and surface; `scene_glass`, added after, is without and with the pane term:

  | host | mean, one share 0.4 and a 20,000 px² budget | mean, now | worst, then | worst, now | `scene_glass`, no pane term | with it |
  | --- | --- | --- | --- | --- | --- | --- |
  | mac | 3.6 | 0.7 | 37 | 17 | 10.8 | 0.6 |
  | handheld | 17.8 | 2.2 | 137 | 78 | 15.1 | 1.3 |
  | windows | 3.7 | 0.4 | 105 | 30 | 12.3 | 0.0 |
  | board | 15.8 | 2.8 | 243 | 42 | 14.6 | 0.6 |

  Without the bare `plain` stage, the mean goes from 4.0 to 1.1. The worst left is a caret on a
  screen of two rows, which repaints whole. Rows count as they draw: a child marker draws nothing,
  and a frame culls a row off the surface however it repaints, so a long list or a zoomed-out graph
  is no dense screen. The pane term's 200 scored 0.06 better than its 300, and reads a share of
  about 0.26 on the screen where a toolbar's and a sidebar's stale rects cover 24 %, a cliff of 65
  to 104 % a step past it. Refused, each from the best policy of its host group, read before rows
  counted as they draw:
  - always merging overlapping rects: a 4 % crosshair repainted whole, 19 to 105 % past its best
    frame, mean 2.8 against 0.7 on mac, handheld and windows;
  - the bounds as the measure, or whole past half the surface in bounds, as a tiler holds no canvas
    there: 8.7 and 9.1 against 0.7, and on the board too, 4.7 and 4.6 against 1.1, as far small
    rects read as the whole surface;
  - a lower share on a stale backbuffer: at half, the board 2.7 → 1.1, the handheld 1.8 → 5.0;
  - a first whole frame through the backbuffer, a second route and its state: 0.86 → 0.74, the
    board 1.5 → 1.1;
  - a floor under the share: 0.1 read 3.2 against 0.8 on mac, handheld and windows;
  - a rect cap of 4: within 0.1 of 8, whose storage exists.

## Open

- **A depth-ordered opaque pass on Apple GPUs** (run 10): with no quad blending the fanless's
  `frame/gpu/scrolling` ran 12.4 % faster and its main pass 19.6 % (diagnostic); the handheld, windows
  and local moved within their floors. Realizing it exactly takes each opaque quad's core drawn
  first with a depth value, so the later draws, the quad's own blended edge among them, skip the
  pixels it covers (Skia Graphite's opaque pass): a depth attachment beside the rounded-clip
  stencil, a depth state in every pipeline and a value per instance. A core-and-ring split with
  no depth puts a pipeline switch after each such quad. Not spiked. On the intel, blending off takes
  8.6 % off `frame/gpu/scrolling`'s device wait, of which the opaque interiors are most, as the
  showcase's sidebar and its two large cards hold 1.8 of the frame's 2.3 MP of quads.
- **The partial frame's copy-out**: 16 to 33 % of a partial frame on every host, and 75 % on the
  intel, whose GPU writes about 8 pixels a clock at 300 MHz. wgpu hands out
  each swapchain image uninitialized, so a partial frame writes all of it; only buffer age or a
  present with damage regions, neither of which wgpu exposes, would let it write less.

- **A scroll as a translation**: paint rects kept in the space of the nearest scroll viewport,
  so a scroll changes one offset and not every row under it, as Chromium's property trees hold
  a scroll as one transform node and paint chunks in its local space. Damage diffs screen rects
  row by row and the encoder culls on them, so both would read through the offset. A ceiling of
  about a quarter of `frame/cpu/scrolling`; not spiked. On the intel, `CascadeEngine::refresh` and
  `compute_paint_rect` are 27 % of `frame/cpu/scrolling`, 16 µs, and 0.9 % of `frame/gpu/scrolling`.
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
  coverage math is about 40 µs of the shadows' 115 µs on the AMD iGPU.
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
- **Shader costs with a profiler**: RGP on the AMD iGPU and Xcode's Metal System Trace on the fanless
  would split the main pass and `backdrop_frame/gpu/*/under`, which spends 1.6 ms over `hover`
  on the fanless.
