# What a backdrop blur's passes cost (2026-10-29)

Hosts as in `.notes/PERF.md`: local is a Radeon 680M on RADV, m2 an Apple M2 on Metal.

## A set of passes has a fixed cost

A temporary bench blurred `n` regions of 8 × 8 texels at σ 32 px (three tents and the two
residual axes, five passes a job), on surfaces of 256² to 4096². Wall clock on m2, device time
between passes local.

| case | m2, a pass per job and stage | m2, one pass a stage | local, a pass per job and stage |
|---|---|---|---|
| 1 job, 256² | 465 µs | 458 µs | |
| 1 job, 2048² | 463 µs | 457 µs | 41 µs |
| 1 job, 4096² | 463 µs | 456 µs | |
| 8 jobs, 2048² | 2.16 ms | 459 µs | 248 µs |
| 16 jobs, 2048² | 4.04 ms | | 491 µs |

- The cost does not follow the surface: the passes load and store only what their scissors
  cover, or the M2 skips tiles without geometry.
- On m2 each job adds about 240 µs, 48 µs a pass; drawn in one pass a stage, eight jobs cost what
  one does. Local, each job adds about 30 µs.

## Where a whole frame's device time goes (local)

`integration/backdrop_frame/gpu/*/scrolling`, device time split between the main pass, the
captures and the blurs, on the base of run 5:

| scene | main pass | captures | blurs |
|---|---|---|---|
| modal, 4K | 2.22 ms | 394 µs | 732 µs |
| cards, 8 panes | 793 µs | 248 µs | 610 µs |
| many, 12 panes | 1.01 ms | 162 µs | 463 µs |
| stacked, 2 panes | 619 µs | 76 µs | 345 µs |
| pair, 2 panes | 568 µs | 81 µs | 173 µs |
| first, its pane on the clear | 571 µs | | |

## Reach

σ 16 logical px at scale 2 is 32 physical px: three halvings, a residual of radius 16 at the
last level, and a reach of `(2 + 16) · 8 + ⌈3 · 7 / 2⌉ = 155` physical px, 77.5 logical.

## Run 5, start against end

ABBA, wall time per frame (`integration/backdrop_frame/gpu`), from `e4cf054a8` to the end of run
5. The 680M's frames move up to 8 % between runs of one binary.

| case | m2 start | m2 end | 680M start | 680M end |
|---|---|---|---|---|
| cards/scrolling | 4.60 ms | 3.92 ms | 5.18 ms | 4.72 ms |
| many/under | 6.35 ms | 5.53 ms | 5.44 ms | 4.84 ms |
| blur/moving | 1.27 ms | 1.12 ms | 2.20 ms | 2.05 ms |
| frosted/under | 1.85 ms | 1.68 ms | 2.99 ms | 2.90 ms |
| frosted/scrolling | 2.39 ms | 2.24 ms | 3.41 ms | 3.21 ms |
| pair_apart/scrolling | 1.94 ms | 1.79 ms | 3.02 ms | 3.11 ms |
| glass/sliding | 1.42 ms | 1.36 ms | 2.20 ms | 1.94 ms |
| modal/scrolling | 5.01 ms | 4.91 ms | 7.15 ms | 7.38 ms |
| stacked/scrolling | 2.53 ms | 2.53 ms | 3.28 ms | 3.31 ms |

`atomic/backdrop_composite/gpu/blur/2048`: 605 → 531 µs on the 680M; `frosted/2048` 695 →
620 µs. The CPU arms of `backdrop_frame` read 2 to 5 % slower across the two binaries, as code
placement moved them before (`.notes/PERF.md`, Measuring); no CPU code on their path changed
past a few nanoseconds a frame.
