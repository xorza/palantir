# Perf journal

What the optimization runs learned: how to measure on the hosts, where the time goes, what was
refused and why, and what is open or waits for a measurement. What was kept is in the commits,
each with its numbers per host. Runs 1 to 10 measured the old frame fixture, 1,245 nodes; the
showcase's first page, 246 nodes, replaced it, so only run 11's numbers compare with a new run, and
an older number here reads as a share, not a time.

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
    to 4.2 GB a build, 6.0 GB since the atlas pool (2026-11-12). It kills a user's processes when
    the SSH session that started them ends, so a run holds its session.
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
  - board: a VideoCore single-board computer; core 3, governor `performance` from the kernel
    command line (`cpufreq.default_governor`). It builds nothing, as a fat-LTO link there takes
    too long: its binaries are cross-built on a Mac with `zig cc -target aarch64-linux-gnu.2.41`
    behind a wrapper that drops `--fix-cortex-a53-843419`, `-Wl,-O1` and cc-rs's
    `--target=<triple>`, 4 minutes and 4.5 GB a build; `cargo zigbuild --profile bench
    --target aarch64-unknown-linux-gnu.2.41` builds it with no wrapper, 5 to 7 minutes. Cargo
    keeps a workspace's build directory under its cache, keyed by the workspace's path, whatever
    `CARGO_TARGET_DIR` says, so a native and a cross build of one worktree take turns. A bench
    binary reads no file at run time, so it runs from any path; only tests read
    `CARGO_MANIFEST_DIR`.
  - apple: a recent Apple laptop, Metal, unpinned; a bench build peaks at 3.9 GB in one process
    and takes 2 minutes warm. Its Xcode runs `xctrace` (Metal System Trace, Time Profiler); a
    board cross-build here and a local build together peak at 8.9 GiB of the process trees,
    `sccache` and `zig` included.
  - mac: a larger Apple laptop, the damage sweep's. A macOS Local Network permission belongs to
    the app that started the `tmux` server: a server started before it gets `No route to host` to
    every LAN address until it restarts.
- **Noise floors** (ABBA A/A):

  | host | CPU cases | GPU cases |
  | --- | --- | --- |
  | local | under 1 %, at most 2 % | up to 8.4 % (`frosted/scrolling`), 7.4 % (`blur/scrolling`), 6.3 % (`pair_near/scrolling`) |
  | fanless | under 0.6 % | under 0.9 % |
  | handheld | under 0.7 %; `frame/cpu/alternating` and `backdrop_frame/cpu/blur/cached` bimodal, 12 to 15 % | median 0.7 %, up to 11 % on `*/cached` |
  | windows | under 0.7 % | median 1 %, up to 8 %, 23 % on `pair_near/scrolling` |
  | board | under 0.3 % | under 0.3 %, `glass/hover` 0.7 %, `record_pass` 5 % |
  | intel | under 1 %, `frame/cpu/partial` 1.9 %, `typing` 1.7 % | under 1.5 %, `frame/gpu/alternating` 6.3 %, `frame/gpu/partial` 17 %, `backdrop_frame/gpu/blur/moving` 15 % |
  | apple | under 1.2 % | `frame/gpu` under 2.9 %, `main_pass/gpu/partial` 48 % |

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
  - intel: `perf_event_paranoid` is 2, so a call graph needs `--user-callchains`, or perf
    refuses every event. `INTEL_DEBUG=wm` with `MESA_SHADER_CACHE_DISABLE=true` prints each fragment shader's
    SIMD widths, instructions and spills; `/sys/class/drm/card*/gt_act_freq_mhz` reads the GPU's
    clock during a run. `perf report --children` over an LBR capture does not finish; a script that
    sums `perf script`'s call chains does. A frame case runs 1 s of warm-up and 3 s of samples
    whatever `--measurement-time` says, as its `Scale` sets both, so a capture of its frames alone
    starts after the setup and ends before the analysis (`perf record -D`).
  - apple: `xctrace record --template 'Metal System Trace'` over one case, then
    `xctrace export --xpath '…/table[@schema="metal-gpu-intervals"]'` gives each command
    buffer's vertex, fragment and blit intervals and its CPU-to-GPU latency; the Time Profiler's
    `time-profile` table holds a `tagged-backtrace` a sample. The export writes each value once
    with an `id` and every repeat as a `ref` to it, so a reader resolves refs before it groups
    rows: by `channel-name`, `event-label` and `cmdbuffer-id`, summing `duration` and reading
    `start-latency`.
  - board: `perf stat` takes the A76's `l1d_cache_refill`, `l2d_cache_refill`,
    `l1i_cache_refill`, `stall_backend` and `stall_frontend` in user mode (`perf_event_paranoid`
    is −1); `perf record -e <event>:u -c 2003` samples where each lands. `V3D_DEBUG=vs,shaderdb`
    prints a vertex shader's QPU code beside its counts.
  - Missing: no Xcode on the fanless, so no Metal trace there; RGP opens captures only in its GUI;
    the distribution's RenderDoc ships no Python module.

## Where the time is

- **The CPU frame** (old fixture, local `frame/cpu/hovering` 116 µs, LBR call stacks): record is
  nearly all of it. `open_node` 21.6 %, `Ui::response_for` 10.1 %, `post_record` 9.7 %, `Text::show`
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
- **The main pass on the AMD iGPU** (old fixture, `main_pass_batch`): the quads are
  raster-bound, as a shader that returns its fill at once times the same, and blending off saves
  27 % of them; the shadows are two thirds raster, a third coverage math. Every fragment shader
  sits at 24 to 48 VGPRs and 20 to 32 waves a SIMD, with no spills.
- **Backdrop passes** (`.notes/research/2026-10-28-backdrops.md`): a blur pass costs the Apple
  laptop about 48 µs whatever it draws and the AMD iGPU about 6 µs; on the AMD iGPU a frame's
  backdrop work is the composite, then the blurs, then the captures (cards: 793, 610 and 248 µs).
  The composite on the fanless's 4K `modal/scrolling` costs 1.4 ms over a backdrop that returns at
  once: the B-spline read 0.29 ms, grain and saturation 0.21, the luminance ceiling and fill 0.16,
  the dither nothing; on the AMD iGPU at 2048² the B-spline is a third of it and the grain 90 µs.
- **On the board the GPU is the frame**: `frame/gpu/scrolling` 18 ms, its CPU half 0.55 ms; a whole
  2560 × 1440 main pass 17.1 ms, of which the tabled shadows 7.7 (coverage 5.8: cutout lookups
  2.1, source clip 1.4), the quads 4.6, text 0.16, and every fragment entry trivial 4.8. In
  `backdrop_frame/gpu/blur/scrolling` (52 ms) the blur passes take 8 ms, the composite 4.6, the
  shadows 7.2, the quads 4.7, the rest the pass breaks, each a load and a store of the whole
  target. A blur residual at 2048² takes 70 ms at σ 1 and 153 ms at σ 4, each tap a
  `textureLoad` of `Rgba16Float`.

- **The GPU frames** (run 10, old fixture): a GPU frame waits for its device each frame, so the
  CPU and the GPU never overlap in the bench. On the APUs the CPU half is most of it: on a handheld
  the CPU is busy 77 % of `frame/gpu/hovering`, record at an IPC of 1.6 against 2.6 in the CPU
  frame. The passes of a partial frame are small; the copy of the whole backbuffer onto the
  target is not: without it `frame/gpu/hovering` and `partial` run 16 to 33 % faster on every host
  (diagnostic). Of a whole repaint's main pass the quads and the shadows are most, about half and
  a third; windows's `frame/gpu/scrolling` spends over half its time in its submit and fence
  wait.

- **Where the CPU frame's cycles are not** (2026-11-11, the intel's counters over each
  `frame/cpu` arm): IPC 3.3 to 3.5, 2.9 for `theming`; L1 misses 1.3 to 2.4 % of loads, the
  last-level cache read 0.01 to 0.3 times per 1k instructions, the dTLB under 0.01. The frame
  fits in the L2: data layout and cache locality move nothing, and fewer instructions still are
  the only lever. Store-forward blocks run at 8 to 12 per 1k instructions, from a large value
  written field by field and read back whole: `Forest::add_draw`'s record 13 % of them, memcpy of
  an `AnimatedLook` 7 %, `FamilyKeys::key`'s memcmp 7 %, `lower::chrome`, `Button::show`,
  `Paint::is_valid` and `Shapes::push` 4 to 5 % each. Removing three of those sources moved the
  frames under 1.5 % (Rejected): at this IPC the blocks overlap other work.
- **The board's main pass is its fixed cost** (`main_pass/gpu/scrolling` 4.13 ms on the
  showcase page): every quad fragment returning at once takes 9 % off it, every text fragment
  6 %, both 12 %; the rest is the raster, the tile loads and stores, and the vertices. Its
  pipelines on V3D: one fragment shader of 2148 instructions at 2 threads with 15:25
  spills:fills, one of 613 at 2 threads, the rest 9 to 420 at 4 threads, none spilling.
- **The board's main pass, taken apart** (run 12, `main_pass/gpu/scrolling`, diagnostic builds
  switched by `PALANTIR_SPIKE`): no draw at all, the clear, the tile stores of 2560 × 1440 and
  the submit, 1.84 of its 4.13 ms, about the store bandwidth of 14.7 MB; the template's vertex
  stage computed twice +0.87 ms, so the vertices were about a fifth of it (now indexed and
  without trigonometry, `TemplateIndices`). Of the 3.81 ms left, the boxes' interiors are 1.40
  ms, which is fill: their fragments return the fill at once, blending off moves 0.01 ms and a
  fragment that returns a constant everywhere takes 0.40 ms off the whole pass. The showcase
  page's 74 boxes cover 2.42 of its 3.69 MP, the sidebar and three cards 1.96 of it. Its partial
  frames do not draw: with every draw skipped, `frame/gpu/hovering` and `partial` move under
  0.5 %, so their 2.4 to 2.7 ms are the canvas and the copy of the backbuffer to the target.
- **The apple's GPU frames** (run 12, Metal System Trace): each command buffer waits 117 to 190
  µs from its commit to its start on the GPU, more than the GPU works: a whole repaint's main
  pass is 11 to 12 µs of vertices and 73 to 79 µs of fragments, a partial frame's 4 µs and the
  blit of the backbuffer to the target 65 µs, the upload's blit 1.5 µs. The bench waits for the
  device each frame, so `frame/gpu/*` there reads the latency more than the work.
- **The board's CPU frame** (run 12, `perf stat` over `frame/cpu/*` on core 3): IPC 2.0 to 2.2;
  L1D refills 7 to 9 per 1k instructions, L2 refills 4.5 to 7.5, the last level 0.1 to 0.3;
  backend stalls 15 to 24 % of cycles, frontend 8 to 13 %, L1I refills 5 to 11 per 1k
  instructions, the dTLB nothing. The L2 refills land where the L1I refills do (the showcase's
  page closures, `EditField::pass`, `Forest::widget_id`), so the L2 serves code as much as data;
  the L1D refills follow the cycles (`response_for`, `post_record`, `open_node`). A layout
  change that cut the data a frame compares, the chrome hashes as a column, moved no frame
  (Rejected), as data layout moved none on the intel.
- **Current numbers** (run 12, `perf-frame` at `eebd66bfd`, ABBA medians against `f33148a82`;
  µs). Against it the board's whole repaints run 6.2 to 7.3 % faster and its main pass 8.2 %, the
  apple's 1.1 to 1.7 % and 4.9 %; the board's CPU frames read +0.1 to +1.2 %, which no change of
  the run reaches (the CPU harness runs no GPU code), so code placement:

  | arm | apple cpu | apple gpu | board cpu | board gpu |
  | --- | --- | --- | --- | --- |
  | `cached` | 19.9 | 20.1 | 83.7 | 86.7 |
  | `hovering` | 23.3 | 262 | 106 | 2,405 |
  | `partial` | 30.6 | 277 | 142 | 2,704 |
  | `alternating` | 35.5 | 331 | 160 | 5,402 |
  | `scrolling` | 39.8 | 298 | 170 | 4,195 |
  | `resizing` | 49.4 | 307 | 212 | 4,274 |
  | `typing` | 57.3 | 296 | 251 | 2,583 |
  | `theming` | 75.7 | 392 | 360 | 5,125 |
  | main pass, `scrolling` | | 90.2 | | 3,796 |

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

- **A smaller `Response`** (2026-11-11, intel core 2, ABBA against `70a8b430e`): `response_for`
  is 6.8 % of a hover frame, but flat over its lookups and branches, with no copy in it, and a
  `ResponseState` grown by 64 B, to 216, moved record and every CPU frame within ±0.5 %, the
  pairs disagreeing. Its 152 B are written once a widget; a compaction of its public fields would
  buy nothing measurable.

- **A name compared and hashed over its own bytes** (2026-11-11, intel core 2, ABBA against
  `3a089549d`): a hover frame spends 1.8 % in libc's compare of a font family's whole 63-byte
  name, `spec_eq<u8, u8, 63>`, on each look's equality. Comparing the slice of its length instead
  took `animation/cpu/steady` +4.7 % and the frames −0.2 to +0.9 %: the slice adds a length test
  and a variable-length compare where the array's is fixed.

- **Run 12, the board and the apple, ABBA:**
  - the chrome rows' hashes in a column of their own, compared as one slice where
    `RollupInputs::hold_for` read 8 B of each 104 B row: every CPU frame within ±0.5 % on the
    board (floor 0.3 %), within ±1 % on the apple;
  - `DamageRegion::add` scanning its rects as branch-free lanes, bit for bit the fold
    (`theming` folds 126 to 146 rects into 8 a frame, 5 % of it on both hosts): `region_add`
    `min_growth` −22 %, `cascade` +26 %, `append` +8 %, `damage/full_repaint` +2.6 %,
    `frame/cpu/theming` +0.4 %;
  - the backbuffer drawn to the target on V3D (`BackbufferOut::Draw`), never measured on a tiler
    but Apple's: `frame/gpu/hovering` 2.41 → 4.07 ms, `partial` and `typing` alike, +68 %;
  - a box interior laid out as a cap a corner, a central quad and two side strips in place of
    the fan from one corner, as a tiler bins a triangle into every tile it touches: V3D's main
    pass −0.48 %, the apple's −0.48 %, but a gradient or an image interpolates its varyings by
    the triangle, so 22 to 44 pixels of four goldens moved by 1 or 2 levels. The ceiling, the
    interior as two triangles with every pixel wrong, was −1.9 %;
  - no blending for the quads on V3D (diagnostic): −0.3 % of the main pass.

- **Three store-forward sources of the CPU frame** (2026-11-11, ABBA over `integration/frame/*`,
  `atomic/record/cpu/frame` and `atomic/animation/cpu/steady`, apple, intel and board):
  - `Shapes::push` and `Tree::push_shape` inlined, so the record stays in `add_draw`: CPU frames
    apple −0.4 to −1.5 %, intel −0.4 to −1.0 %, board +0.3 to +1.1 %;
  - the animation tick reading its request in place and returning nothing for a resting row,
    no `T` copied in or out: `animation/cpu/steady` apple +21 %, intel −10.7 %, board −3.1 %, the
    frames within ±0.5 %;
  - `InlineName` aligned to 8 and compared and hashed as eight words, `FamilyKeys::key` taking
    `&FontFamily`: CPU frames +1 to +2.5 % on every host.
- **One quad per box on the board** (`ShapeCells::OneQuad` for every form, diagnostic): its
  `main_pass/gpu/scrolling` +8.4 %, `frame/gpu/scrolling` +7.4 %; the template's vertices cost
  V3D less than the distance every pixel would measure.

- **Shape templates, the rest of the plan** (fanless, ABBA):
  - the template in the effect stage: with no coverage math at all, `backdrop_frame/gpu`'s
    frosted and glass frames move within ±1 %, as the effect's blur and lens are its fragment;
  - an opaque interior pass: every quad without blending, edges wrong, takes the template's main
    pass 149 → 137 µs and its frame 490 → 481 µs, −2 %, the ceiling a depth attachment and a
    depth state in every pipeline would buy;
  - one arc segment a corner: −1.2 % on the main pass, near the floor, and a 41 % overshoot
    ring on a large circle;
  - the region and the ring radius packed into one varying: −0.6 %; an interior zigzag in place
    of the fan: no change;
  - the strokes as templates: no bench on this GPU draws enough strokes, and the curve
    pipeline's batch timer needs the in-pass timestamps Metal lacks; the strokes' join seams
    moved 235 levels in an earlier try.

- **Shadows: the blur's reciprocals per instance**, handed on flat from `vs_shadow`: bit-exact,
  nothing gained (board 17.11 → 17.08 ms, fanless +2.0 %). The SFU does not bound the tabled shadows.
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
  handheld −0.5 to −4.4 % but +1 % on the blur frames. A held capture serves a backdrop inside the
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

- **A look animates in place** (2026-11-11, intel core 2, ABBA against `3a089549d`). The
  animation tick takes its request by reference and writes the value to show into its target,
  returning whether the row settled; `Widget::wear` goes through `Ui::animate_in_place`, and
  `Ui::animate` calls it and returns the value. A look at rest compares in place and copies
  nothing, where a by-value target was copied into the request, into the tick and back out, each
  a `memcpy` of the whole look, 2.5 % of a hover frame in libc. `animation/cpu/steady` −20 %,
  `instant` −32 %, `duration` −1.6 %, `spring` −2.2 %; record −0.9 %, frames −0.8 to +0.5 %,
  within the floor. A row that retargets clones the target into the row where it moved it.

- **A paint carries a short effect name** (2026-11-11). A material is half of every `Paint` and
  its effect's name, inline at 64 B, half of the material: a `Paint` grown by a material's 116 B
  measured, ABBA, apple record +4.6 %, CPU frames +1.2 to +3.9 %, animation +5 to +16 %; intel
  (core 2) record +2.8 %, frames +0.5 to +2.1 %, animation +1 to +26 %; encode and cascade,
  which read the lowered rows, within ±1.4 %. An effect name now sits in 24 B
  (`InlineEffectName`, 23 bytes and its length), a font family's keeps 63: `Paint` 232 → 192 B,
  `Chrome` 240 → 200, an animation row of a look 368 → 328. Against `f50bf6154` on the intel:
  record −1.1 %, frames −0.1 to −0.9 %, and with every animation snapping, as under reduced
  motion, record −1.2 %, frames +0.2 to −1.1 %. `atomic/animation/cpu/instant` +34 %: it is
  libc's `memcpy` of a look cloned, moved into a request and returned by value, whose
  store-forward blocks and split loads rise with the new size (53.8 → 68.3 and 53.8 → 67.4 per
  1k instructions), a path the frames do not show. Refused: the material behind an `Arc`,
  which allocates where an app builds a material in its frame; a table of materials a paint
  names by handle, which takes a theme file's materials out of plain data.

- **Normals in the instance, refused** (run 12): each edge's outward normal worked out on the
  CPU and read by the template from 32 B more a quad, in place of a `length` and a division per
  edge in every vertex. Board `main_pass/gpu/scrolling` −0.92 %, `frame/gpu/scrolling` −0.46 %;
  apple main pass +0.0 %, its frames +0.3 %, within the floor. Refused for 31 % more bytes in
  every template instance, quads, shaped images and effects alike, and a second place a shape's
  geometry is worked out, for half a percent of one GPU's frame.

- **Halos and shadows on the template** (2026-11-11, the apple and the board, ABBA,
  `atomic/effect_pass/gpu/*`: 28 cards at 2560 × 1440, each glowing or casting a drop shadow 24 px
  past its edge). The template between two outlines (`1ec40141c`) moved the guard within each
  floor, board `main_pass/gpu/scrolling` +0.02 %, and kept V3D's vertex shaders at their
  instruction counts (`V3D_DEBUG=shaderdb`, 367 in binning, 441 to 505 rendering); on the apple it
  is not bit for bit, its fast math scheduling the rewrite otherwise. The halo (`08b1f7f29`)
  against the glow over its card's rect grown by the reach: apple 230 → 152.5 µs, −34 %, board
  12.48 → 7.84 ms, −37 %, the glows alone −49 % and −47 %. The halo as a bit of the instance's
  flags in one pipeline, refused: the plain glows +4.2 % on the apple, +1.0 % on the board, a
  varying more read by every fragment. Shadows on the template (`74590267f`) against the 3 × 3
  grid: apple 318 → 286 µs, −9.9 %, board 15.60 → 14.83 ms, −5.0 %, the guard within the floor;
  the grid already left out most of a card's inside, so the template saves the corners' squares.

- **Shape templates indexed** (run 12; `e2e0d0a0c`, `955437499`, `d6a91df8d`): the template
  draws its 64 points a box through an index buffer where it drew 138 vertices, its arc
  directions are sums of unit vectors, and a transparent solid fill's interior collapses. V3D's
  vertex stage for the template is about 500 instructions, run in binning and per tile; the
  first two took its binning shader from 506 to 433 and its render shader from 556 to 503.
  Against `f33148a82`, ABBA: board `main_pass/gpu/scrolling` −7.8 %, `frame/gpu/scrolling`
  −7.2 %, `resizing` −6.9 %, `theming` and `alternating` −5.9 %, `clip_frame/gpu/rect/scrolling`
  −3.7 %; apple main pass −3.7 %, its whole repaints −1.1 to −1.4 %; the collapse −0.4 % of the
  board's main pass and −1.1 % of the apple's. Bit for bit in the visual suite on the apple.

- **The renderer plan's steps, each against its parent** (2026-11-11; an Apple laptop unpinned
  and the intel, ABBA, the outliers again at three rounds):
  - the f32 corner points (`3c010b1d1`, off master, squashed into `c11023bb1`): the main pass and
    the frame within ±0.5 % on both;
  - the effect pipelines per shape form (`76abc89e6`): within noise on both. The intel's
    backdrop frames are no floor to read a change off: an A/A of `blur/moving` read +14.7 %;
  - the quad keys (`adb0e9bae`): the intel's composer cases 0 to −4 %; the Apple laptop's quads
    +4 to +9 %, as the stream turned into a `Soa` whose `push` soa-rs 1.0.1 leaves out of line, so
    a row went through the stack. A fork with `push` inlined and its growth cold, as `Vec::push`,
    gives the Apple laptop's quads −4 to −8 % and images −25 %, the intel's −1 % and −4 %
    (the workspace `Cargo.toml`, until tim-harding/soa-rs#51 ships);
  - the one stroke payload (`061887796`): `curves/alone` +28 % on the intel and +9 % on the Apple
    laptop. A curve's basis and its `StrokeBbox` reached `CubicBezier::bbox` and
    `StrokeBbox::urect` out of line, written field by field and read back in wider loads, which
    store-to-load forwarding refuses: half the bbox's samples sat on its first load. Inlined, the
    two run against master −38 % on the intel and −52 % on the Apple laptop, below the step's
    parent; the frame cases within ±0.6 %.

- **The clip folded away where no draw names one** (2026-11-10, an Apple laptop, ABBA): with one
  pipeline for both, the clip's code cost a frame with no rounded clip 16 to 23 % of its main pass,
  and an early return on no chain 7 %, as the clip's registers stayed; a coverage that returns 1 at
  once took `main_pass/gpu/scrolling` −19 %. With the clip folded away, the frame cases of a page
  with no rounded clip run within ±0.3 % of a build with no clip at all.
- **A draw inside its clips names none** (same host): `clip_pass/gpu/inside` −12 %, at
  `clip_frame/cpu/*/scrolling` +3 to +4 %.

- **Shape templates** (fanless, ABBA, `atomic/main_pass/gpu/scrolling` and
  `integration/frame/gpu/scrolling`): a box, a circle and a triangle draw as a vertex-stage
  template (`vs_template`), 221 → 146 µs and 577 → 483 µs. On this GPU the quads' fragments are
  half the main pass: a fragment that returns its fill at once took it to 116 µs, where the AMD
  iGPU timed the quads as raster. The cost follows the varyings a fragment reads: the template
  sharing the reference's varyings plus its own three took 315 µs, a 3 × 3 grid of the same 311,
  the grid with one varying added 178; the template's own 13 scalars 180 µs, its colors packed
  as f16 149, and a vertex stage with no indexed arrays, which spilled 240 bytes, 146. On the
  showcase page the hull saves 0.2 % of the quads' 2.4 MP and the bands and rings are 3 % of it.

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
- **The cutout tables keep what they baked**: each table's placement holds the span of it baked in
  earlier frames, so a frame bakes only what it lacks. Refused: a cache per window's plan, which another window's bake would
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
- **A lens's spectrum tables are a buffer**, which the backdrops' group binds. Refused: the tables
  as WGSL constant arrays, which V3D indexes by a select over every entry. Measured together with
  glass's pipelines of its own, since undone (Open, "One glass"): Pi glass frames −26 to −41 %.
- **A glass backdrop's footprint holds what its lens reads, not its reach**: `LensOptics::margin`,
  from the backdrop's narrowest side where the cascade knows it, the box's own, its shadow left
  out. Glass frames −5.6 to −10.6 % on the
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
  physical pixels, and 300 for all of the surface under backdrop footprints. Sum of CPU and GPU,
  each scenario's regret past its cheapest decision as a percent of that frame, over six stages of
  content, scale and surface; `scene_glass`, added after, is without and with the backdrop term:

  | host | mean, one share 0.4 and a 20,000 px² budget | mean, now | worst, then | worst, now | `scene_glass`, no backdrop term | with it |
  | --- | --- | --- | --- | --- | --- | --- |
  | mac | 3.6 | 0.7 | 37 | 17 | 10.8 | 0.6 |
  | handheld | 17.8 | 2.2 | 137 | 78 | 15.1 | 1.3 |
  | windows | 3.7 | 0.4 | 105 | 30 | 12.3 | 0.0 |
  | board | 15.8 | 2.8 | 243 | 42 | 14.6 | 0.6 |

  Without the bare `plain` stage, the mean goes from 4.0 to 1.1. The worst left is a caret on a
  screen of two rows, which repaints whole. Rows count as they draw: a child marker draws nothing,
  and a frame culls a row off the surface however it repaints, so a long list or a zoomed-out graph
  is no dense screen. The backdrop term's 200 scored 0.06 better than its 300, and reads a share of
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

- **A depth-ordered opaque pass on Apple GPUs** (run 10; since the shape templates, see
  "Rejected"): with no quad blending the fanless's
  `frame/gpu/scrolling` ran 12.4 % faster and its main pass 19.6 % (diagnostic); the handheld, windows
  and local moved within their floors. Realizing it exactly takes each opaque quad's core drawn
  first with a depth value, so the later draws, the quad's own blended edge among them, skip the
  pixels it covers (Skia Graphite's opaque pass): a depth attachment, a depth state in every
  pipeline and a value per instance. A core-and-ring split with
  no depth puts a pipeline switch after each such quad. Not spiked. On the intel, blending off takes
  8.6 % off `frame/gpu/scrolling`'s device wait, of which the opaque interiors are most, as the
  showcase's sidebar and its two large cards hold 1.8 of the frame's 2.3 MP of quads.
- **The partial frame's copy-out**: 16 to 33 % of a partial frame on every host, and 75 % on the
  intel, whose GPU writes about 8 pixels a clock at 300 MHz; on the board the whole of it, as its
  partial frames do not draw (run 12). wgpu hands out
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
- **One glass, its spectrum behind a branch** (`gpu::backdrop`, "One glass"): glass is the stock
  effect `effect/stock/glass.wgsl`, one program with its lens's spectrum behind a branch. That
  undid glass's pipelines of its own, `QuadForm::BackdropLens` and `BackdropSpectrum`, which had
  refused the branch as V3D spilled its registers on every backdrop, and which measured Pi glass
  frames −26 to −41 % together with the spectrum buffer (Decisions). Not measured on V3D since
  ("To measure").
- **The glass pipeline spills on V3D**: 1410 instructions, 32:74 spills:fills, two threads.
- **Glass's σ is the plan's most costly**: σ 4 physical runs the residual at level 0, 25 taps an
  axis; the bilinear pair at the 8 sub-texel bits the board and handheld report moves a pair's weight by
  up to 0.39 % of the peak.
- **Glass chrome keeps the lens's whole reach**: the cascade keeps no chrome lift, so
  `LensOptics::margin` reads no narrowest side for it.
- **A triangle's glass keeps the lens's whole reach**: `LensOptics::margin` bounds a sample by a
  rounded rect's chords, which a triangle's corner breaks. A bound of its own would take the
  capture back to what it reads.
- **A direct frame still copies its captures**: no sampled swapchain on Metal or DX12
  (Rejected). Skipping every copy took the fanless 5 to 12 % off the frames that capture.
- **A backdrop's break test reads one bounding box** of what was painted since the break, so an
  L-shaped layout breaks anew though no draw touches the footprint; no bench has one.
- **Shadow coverage per instance**: `filter_cdf_series` takes `1 / σ` and `h²/6` per pixel; the
  coverage math is about 40 µs of the shadows' 115 µs on the AMD iGPU.
- **A shadow corner stream per shadow, not per quad**: in a frame with a shadow every quad
  uploads 16 B of `CornerTables` that only shadow instances read. No bench shows the upload.
- **One ordered lane of runs for every family**: the quad lane replays in record order and every
  other family in a fixed tier order after it (`PaintTier`), closing a group where order would
  break. WebRender batches every kind in one list with a look-back, which would remove the
  quad-first asymmetry and the tier conflict test. Structural, and no workload asks for it. Gate:
  the draw count and the main pass on the showcase pages
  (`.notes/research/2026-11-10-draw-calls.md`).
- **The rect form for a sharp fill**: a sharp unturned solid box draws the template's 64 points
  where a plain image draws four, as `FillForm` refuses `ShapeForm::Rect`. Gate: the
  main pass on V3D, whose vertex floor is 4.8 ms ("The board's vertex floor"); the vertex stage
  costs nothing measurable on an Apple GPU.
- **A material box's footprint takes in its drop shadow's halo**: a backdrop's rect is its
  shape's paint rect, so its capture and its damage hold the halo the box does not read. A rect of
  the box alone needs each node's shape lift and clip, which the cascade keeps only as a scale
  (`PlaneCascade::shape_scales`).
- **`Ordering::order` inlined into the curve lowering**: x86 needs it inlined (as a call,
  `curves/alone` +20 %), the fanless's `curves/alone` runs 21 % slower so and its `same_tier_mesh`
  23 % faster; no profile says why.
- **The board's tabled shadows** run at 2 threads where V3D runs 4 at 16 registers; a lean entry
  for the straight bands could run on 4, at twice the shadow draws. Ceiling, measured on the
  grid the template replaced: 15.0 against 17.1 ms.
- **The board's pass breaks**: each backdrop capture ends the main pass, 3.8 ms on V3D. Painting
  what lies under the backdrops into capture canvases first trades the breaks for drawing each
  footprint twice.
- **The board's vertex floor**: the main pass with every fragment entry trivial takes 4.8 ms.
- **The cutout bake's cost model** assumes a bake a frame; with held tables more keys would pay,
  but which shadows read a table is a rendering decision.
- **Shader costs with a profiler**: RGP on the AMD iGPU and Xcode's Metal System Trace on the fanless
  would split the main pass and `backdrop_frame/gpu/*/under`, which spends 1.6 ms over `hover`
  on the fanless.

## To measure

- One glass shader with its spectrum branch on V3D (`gpu::backdrop`, "One glass").
- The f32 corner points of a closed shape on V3D (`Quad` 64 → 100 B, step `3c010b1d1`).
- Three batching changes, each built only once its bench approves it:
  - Quad runs joined again on `QuadDraw` in the backend, so a shadow's cutout entry splits no run
    its key joins: the Shadows page's 15 runs toward 11, against `atomic/composer/cpu/quads/*` and
    `integration/frame/gpu/*`, which pay a second reorder of each group's quads, corner tables
    and entries. Refused: the entry decided in the composer, which would move the cutout plan, a
    backend's knowledge of its atlas, into the frontend.
  - Icons in the raster lane, batched across groups as text is, as WebRender batches images and
    text in one list: the draws on the Icons and Dock pages and a toolbar of icons in clipped
    groups. Cost: icon and text batches order against each other.
  - Opaque images occluding the quads under them, the image's opacity learned at upload: a
    full-bleed photo over cards, on the AMD iGPU's main pass. No showcase page has one.
