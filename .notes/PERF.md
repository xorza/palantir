# Perf journal: backdrop and renderer (2026-10-28)

Squash-merged onto master; the run started at `4c3066448` on branch `perf-backdrop`.

## Hosts

- **local** — AMD Ryzen with a Radeon 680M (RDNA2 iGPU), Vulkan, Linux. Runs pinned with
  `taskset -c 2 setarch -R`; governor and EPP at `performance`.
- **m5** — 192.168.0.6, Apple M5 MacBook Air, Metal, macOS 27. No core pinning on macOS.
  Device time of a run of passes reads the wall clock there (no timestamps between passes), so
  the `atomic/backdrop_blur` cases on the M5 hold a submit-to-callback latency.

## Build RAM peaks

The `bench` profile's whole process tree, one build alone:

| host | peak | wall |
|---|---|---|
| local | 2.9 GB | 2 min 02 s |
| m5 | 3.3 GB | 1 min 13 s |

Two builds together peak at 4.2 GB local and 5.2 GB on the M5; they fit with margin.

## Filter

`backdrop|^atomic/(encode|composer|rect_grid|encoded_cache|frontend|schedule|record_pass|main_pass)/|^integration/frame/`

## Noise floor

A/A of the start binary, the whole filter, change of the second run against the first:

- **local**: CPU cases within 2 %; `atomic/*/gpu` within 1 %; `integration/*/gpu` frames up to
  8 % (`backdrop_frame/gpu/blur/hover` −8.4 %, `frame/gpu/resizing` −6.4 %). A frame change
  counts only past 8 % and mirrored in ABBA.
- **m5**: every case within 1 %. `atomic/backdrop_blur` reads the wall clock there and its
  ABBA spread reaches 4 % on the 512² cases.

## Numbers at the start (`4c3066448`)

| case | local | m5 |
|---|---|---|
| atomic/backdrop_capture/gpu/1920x1080 | 106.0 µs | — (no timestamps between passes) |
| atomic/backdrop_capture/gpu/3840x2160 | 411.8 µs | — |
| atomic/backdrop_blur/gpu/1/2048 | 6.47 ms | 3.30 ms |
| atomic/backdrop_blur/gpu/4/2048 | 20.67 ms | 10.34 ms |
| atomic/backdrop_blur/gpu/16/2048 | 2.22 ms | 1.94 ms |
| atomic/backdrop_blur/gpu/48/2048 | 1.08 ms | 1.43 ms |
| atomic/backdrop_blur/gpu/160/2048 | 1.04 ms | 1.43 ms |
| atomic/backdrop_composite/gpu/blur/2048 | 575.2 µs | — |
| atomic/backdrop_composite/gpu/frosted/2048 | 665.4 µs | — |
| atomic/backdrop_composite/gpu/glass/2048 | 468.5 µs | — |
| atomic/damage/cpu/backdrop_walk/8/16 | 190.3 ns | 116.4 ns |
| atomic/damage/cpu/backdrop_walk/64/256 | 19.37 µs | 10.84 µs |
| atomic/encode/cpu/full | 13.07 µs | 8.35 µs |
| atomic/main_pass/gpu/scrolling | 425.8 µs | 319.2 µs |
| integration/backdrop_frame/cpu/blur/scrolling | 233.4 µs | 135.9 µs |
| integration/backdrop_frame/gpu/blur/under | 3.11 ms | 1.68 ms |
| integration/backdrop_frame/gpu/blur/scrolling | 3.60 ms | 2.00 ms |
| integration/backdrop_frame/gpu/glass/under | 10.13 ms | 3.78 ms |
| integration/backdrop_frame/gpu/glass/scrolling | 12.70 ms | 4.89 ms |
| integration/backdrop_frame/gpu/modal/scrolling | 8.59 ms | 4.87 ms |
| integration/frame/cpu/scrolling | 229.7 µs | 132.0 µs |
| integration/frame/gpu/scrolling | 2.21 ms | 703.4 µs |

The CPU cost of three panes is within the A/A noise of the frame without them
(`backdrop_frame/cpu/blur/scrolling` against `frame/cpu/scrolling`).

## Kept

- **In-range blur reads skip the mirror.** A tent block inside its level reads
  straight, else folds every read; the residual tests its window once and sums symmetric pairs.
  `backdrop_blur/gpu/4/2048` 20.65 → 3.25 ms local, 10.77 → 3.67 ms m5; every case faster on
  both hosts; `backdrop_frame/gpu/glass/scrolling` 12.70 → 3.65 ms local. Variants measured on
  the way, mean of ABBA, change against base on `4/2048` and `160/2048`:
  - a test per read in every pass (v1): −80 % / +7.5 % local, +1 % / +2.5 % m5.
  - v1 with symmetric pairs (v5): −84 % / +7.2 % local, −26 % / +2.4 % m5.
  - one test per tent block, mirror by remainder at the edge, plain residual (v8): +0.1 % /
    −3.3 % local.
  - per-fragment window test in the residual, remainder fallback (v16): −83 % / −0.1 % local.
  - v8 with v16 (v24): −83 % / −3.3 % local, −66 % / −34 % m5.
  - v24 with the unsigned-compare mirror everywhere (v32): −84 % / +3.1 % local: a test per read
    in the tent's edge fallback costs more than the remainders it skips.
  - v24 with the unsigned-compare mirror in the residual only (v72, kept): −84 % / −3.7 % local,
    −66 % / −34 % m5; m5 within 1 % of v24.

- **A tent inside its level is four bilinear fetches.** `[1 3 3 1] / 8` per axis is
  half `[1 3] / 4` and half `[3 1] / 4`, each one fetch at a quarter phase that a 4-bit
  sub-texel precision holds exactly. M5: `blur 48/2048` 939 → 382 µs, `160/2048` 983 → 415 µs,
  `frame modal/scrolling` 3.61 → 2.79 ms, `frame blur/scrolling` 1.63 → 1.37 ms. Local: −4 % to
  −6 % on every blur with a level; frames within noise. The M5 waits on texture reads in the
  tents, the 680M does not. The goldens hold bit for bit on the 680M. A capture whose format does
  not filter keeps sixteen loads through a second layout.

- **A glass pane's bezel reads its result once.** The unbent read ran in the bezel
  too and was discarded. Local `backdrop_composite` −0.5 % to −1.3 %; M5 frames within 1 %.

- **A plan solves its taps only where a blur runs.** `BlurPlan::new` ran Newton
  steps over `exp` twice per pane per frame (1 % in `BlurPlan::new` and 1.1 % in `exp` of
  `backdrop_frame/cpu/frosted/scrolling` on the 680M) for the cascade and the composer, which read
  no tap. M5 `backdrop_frame/cpu` −1.7 % to −2.5 %; 680M −0.3 % to −1.5 %, and `cached` +2.5 %,
  where neither cascade nor plan runs (profiled): code placement.
- **Only a rect is noted as a pane.** `Forest::note_pane` ran for every shape, 0.7 %
  of a cached frame. M5 `frame/cpu` and `backdrop_frame/cpu` −0.4 % to −1.2 %.

## Rejected

- **Dither by the sRGB slope**: one `pow` per channel for six; no change on either
  host (local composite within 0.4 %, M5 frames within 0.6 %). The composite does not wait on
  its transcendentals, and the exact encode-add-decode stays.

- **`LoadOp::Clear` on the tent and first-axis passes** (v3 against v1): +8 % to +15 % on the
  680M's 512² cases, as a clear writes the whole mip; no change on the M5, so a tile load of the
  level is not what its passes pay.

- **The residual as a compute pass over workgroup memory** (two spikes against the in-range
  reads). A 16×16 workgroup reads its windows into workgroup memory once, then sums its
  taps there. First cut: +298 % on `4/2048` local, as the vertical tile strode 224 dwords
  between lanes, one LDS bank for a whole wave; +3000 % on the M5, where wgpu zero-fills
  workgroup memory on every dispatch. With a lane-contiguous vertical tile and no zero fill:
  local +15 % / +2.3 % / −1.9 % / −0.2 % on `1`, `4`, `16`, `160` at 2048; M5 +86 % / +23 % /
  +3 % / −1.9 %, and −10 % on the 512² high-σ cases, where a compute pass saves a render pass's
  fixed cost. Both GPUs serve the residual's reads from their texture caches as fast as from
  workgroup memory; the extra barrier and fill lose. FidelityFX Blur's single-dispatch design
  wins on hardware with a slower texture path.
- **Halve at σ above 2 texels instead of 4.** The level-0 residual at σ 4 is the costliest blur,
  and one more halving makes it a quarter of the texels. Probe of the impulse response against
  the Gaussian: the worst deviation grows from 0.30 % to 3.5 % of the peak (σ 2.01 at one
  level, residual radius 3), past the 0.4 % `blur_plan` promises. Skia's threshold of 4 holds.

## Ideas

- **Done: mirror by integer modulo in every blur read.** `mirror` runs two `i32` `%` per read, and
  every tent read and residual tap mirrors both or one axis. `atomic/backdrop_blur/gpu/4/2048`
  takes 20.7 ms on the 680M and 10.3 ms on the M5: a full-resolution residual of radius 16,
  2048² × 66 reads. An in-range read needs one unsigned compare.
- **Rejected: load of the whole level on every blur pass.** Each tent and the residual's first axis begin
  their pass with `LoadOp::Load` over a mip as large as half the largest surface; a tiler loads it
  whole. What lies outside the scissor is never read by the job, so `Clear` serves.
- **Done: residual taps in symmetric pairs.** `tap(k) == tap(−k)`: summing the two reads first halves
  the multiplies and the tap lookups.
- **Rejected: the residual as a compute pass over workgroup memory.** At level 0 the residual is 33
  texture reads a texel each way; a 16×16 workgroup reading its windows into workgroup memory
  once reads (16 + 40) × 16 texels for 256 outputs. AMD FidelityFX Blur keeps the intermediate
  in thread-group shared memory for the same reason
  (<https://gpuopen.com/manuals/fidelityfx_sdk/techniques/blur/>).
- **Skip the capture copy.** Tent 1 (or a level-0 residual) can read the target itself after
  the pass breaks, where the target binds as a texture: −106 µs at 1080p, −412 µs at 4K on the
  680M. A swapchain image is a new texture each frame, so its group is a per-frame allocation;
  only the backbuffer path keeps one.
- **Rejected: dither by the sRGB slope.** The composite encodes and decodes each channel (six `pow`) to add
  one encoded step; one `pow` gives the slope. Changes the noise, not its size.
- **Done: the tent as four bilinear fetches.** `[1 3 3 1] / 8` per axis is half `[1 3] / 4` and
  half `[3 1] / 4`: one fetch each at a quarter-texel phase, which a sub-texel precision of 4
  bits (Vulkan's minimum) holds exactly, so the research note's refusal of linear tap pairs does
  not reach it. 16 loads become 4 fetches; tent 1 is most of a high-σ blur.

## Open

Stopped here at the user's request; the loop is not finished.

- **Share a pass break between panes whose footprints nothing between them touches.** A capture
  needs only what paints under its footprint, so a pane can capture at the last barrier when
  nothing painted since it touches its footprint; the toolbar, sidebar and popup of
  `backdrop_frame` would break the pass twice, not three times, and a grid of separate frosted
  cards once. Every break stores and loads the whole target on the M5. A timing hack that sends
  every capture to the first barrier is written but not measured.
- **Profile and read the assembly of the renderer's CPU hot paths** (`encode_node`,
  `push_quad`, the schedule walk): not done.
- **M5 drift.** The MacBook Air has no fan: one ABBA run read a base binary 16 % slow on both of
  its A runs, and interleaved short runs then showed no difference. Confirm any surprising M5
  result by alternating runs.
