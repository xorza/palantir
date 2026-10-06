# Frame bench: why the 6800U numbers are slower than the README

Measured on 2026-10-05 on the Ryzen 7 6800U / Radeon 680M (`beelink-ser5-dev`),
CachyOS, Mesa 26.2.4. The README numbers came from `b7b77cfd` (2026-08-06) on
the same machine under Debian.

## Result

The OS change is not the main cause. Code changes after `b7b77cfd` cause most
of the increase:

- **GPU `scrolling` and `resizing`:** one commit, `e0500557` "Blur shadows by
  integrating the Gaussian over the box", adds approximately 2.5 ms per full
  frame. This is an intentional accuracy change in the quad shader.
- **CPU arms:** no single commit. `cached_cpu` increases from 125.6 µs to
  162.7 µs (+30%) in many steps of 2–8 µs from August to October. The
  largest steps are `ced36fc3` (+7.2 µs), `d68485de` (+4.6 µs) and
  `86739680` (+3.6 µs).
- **OS (Debian → CachyOS):** CPU arms approximately +4%. GPU `cached` and
  `partial` approximately +20% and +12%. The GPU part is possibly the Mesa
  version. These numbers cannot be separated further without a Debian run.

## Totals

All values are criterion point estimates.

| arm | README, Debian, `b7b77cfd` | `b7b77cfd`, CachyOS, no `x86-64-v3` | `b7b77cfd`, CachyOS, `x86-64-v3` | HEAD `77719e8a`, CachyOS |
| --- | ---: | ---: | ---: | ---: |
| `cached_cpu` | 130 µs | 135.8 µs | 125.6 µs | 164 µs |
| `partial_cpu` | 145 µs | 149.9 µs | 140.1 µs | 179 µs |
| `scrolling_cpu` | 201 µs | 209.8 µs | 194.2 µs | 225 µs |
| `resizing_cpu` | 317 µs | 340.6 µs | 312.6 µs | 326 µs |
| `cached_gpu` | 1.12 ms | 1.36 ms | 1.23 ms | 1.37 ms |
| `partial_gpu` | 1.37 ms | 1.53 ms | 1.44 ms | 1.57 ms |
| `scrolling_gpu` | 4.25 ms | 4.10 ms | 3.91 ms | 7.69 ms |
| `resizing_gpu` | 5.38 ms | 5.56 ms | 4.84 ms | 8.51 ms |

`b7b77cfd` has no `.cargo/config.toml`, so its README numbers are a baseline
`x86-64` build. HEAD builds with `target-cpu=x86-64-v3`. The third column
removes that difference. The HEAD column is the median of three full runs.

## Timeline

Every commit below was built with `target-cpu=x86-64-v3` and run with the full
`frame` matrix.

| commit | date | `cached_cpu` | `scrolling_cpu` | `scrolling_gpu` | `resizing_gpu` |
| --- | --- | ---: | ---: | ---: | ---: |
| `b7b77cfd` | 08-06 | 125.6 µs | 194.2 µs | 3.91 ms | 4.84 ms |
| `b9630f53` | 08-16 | 123.6 µs | 194.2 µs | 4.24 ms | 5.37 ms |
| `d429ed36` | 08-19 | 123.1 µs | 192.5 µs | 4.00 ms | 5.43 ms |
| `adc49644` | 08-20 | 124.8 µs | 192.4 µs | 4.26 ms | 5.47 ms |
| `4ce9b693` | 08-27 | 126.8 µs | 195.5 µs | 4.10 ms | 5.50 ms |
| `b41d7834` | 08-30 | 128.4 µs | 201.4 µs | 4.35 ms | 5.48 ms |
| `24124b62` | 09-02 | 132.0 µs | 205.5 µs | 4.11 ms | 5.38 ms |
| `a2c36c9d` | 09-04 | 130.1 µs | 202.8 µs | 4.09 ms | 5.34 ms |
| `10ed7563` | 09-06 | 135.5 µs | 210.2 µs | 4.33 ms | 5.38 ms |
| `e2df72d5` | 09-07 | 135.3 µs | 210.0 µs | 4.06 ms | 5.35 ms |
| `91e17114` | 09-18 | 135.7 µs | 209.3 µs | 4.18 ms | 5.47 ms |
| `3d750d00` | 10-03 | 160.6 µs | 241.9 µs | 4.61 ms | 5.64 ms |
| `593854db` | 10-04 | 162.3 µs | 247.2 µs | 4.60 ms | 5.47 ms |
| `50025ce6` | 10-04 | 162.4 µs | 247.5 µs | 4.91 ms | 5.62 ms |
| `e0500557` | 10-04 | 163.3 µs | 250.0 µs | 7.49 ms | 8.11 ms |
| `77719e8a` | 10-05 | 162.7 µs | 225.7 µs | 7.73 ms | 8.54 ms |

## GPU: the Gaussian shadow integral

`e0500557` replaces one `erf` of the signed distance with an exact integral of
the pixel filter over the rounded box. Near a blurred corner, each fragment
does 4 corners × 2 arc halves × 12 slices (`BLUR_ARC_SLICES`), and each slice
evaluates `filter_cdf` twice. Each `filter_cdf` evaluates two `erf` and two
`exp`. So a fragment near a corner does approximately 200 `erf` instead of one.

The draw list does not change. At the first full frame of `scrolling`, the
fragment count increases by only 4% (8.07 M → 8.39 M, from the wider shadow
reach), but the quad pass time increases from approximately 0.9 ms to 3.0 ms.
In the RADV dump, the quad fragment shader increases from approximately 3 200
to 34 000 lines of NIR, with 16 loops.

The commit message gives the reason: the old path made sharp corners twice as
dark as the blur, and a box that was small against σ much too dark. The cost
is in the fragments near the corners of blurred shadows, so it scales with
the number and size of shadows on screen.

`50025ce6` (CSS-compatible shadow layering and geometry) adds approximately
0.3 ms to `scrolling_gpu`.

## CPU: many small steps

`cached_cpu` with `x86-64-v3`, from 125.6 µs to 162.7 µs:

| interval | change | cause |
| --- | ---: | --- |
| `b7b77cfd` → `91e17114` (08-06 → 09-18) | +10 µs | Gradual. Not bisected. |
| `91e17114` → `50b34a49` (09-18 → 09-27) | +8 µs | `ced36fc3`, see below. |
| `5c00212b` → `65781b1c` (merge of a 130-commit branch) | +17 µs | See below. |
| `65781b1c` → HEAD | +1 µs | Flat. `scrolling_cpu` improves by 7% from the cascade refresh (`f58cf2f4`). |

Inside the branch that `65781b1c` merges (position 0 is the base `5c00212b`,
position 130 is the tip `d19d3673`):

| position | commit | `cached_cpu` | step |
| ---: | --- | ---: | ---: |
| 0 | `5c00212b` | 144.8 µs | |
| 8 | `02d94a50` | 143.8 µs | |
| 9 | `d68485de` Reserve a resolved widget id until its record | 148.4 µs | +4.6 µs |
| 13 | `9cdb1cb8` | 148.9 µs | |
| 14 | `355c3f7e` Scene: layout rollup first, full hash built from it | 151.2 µs | +2 µs |
| 15 | `67dc8167` | 151.9 µs | |
| 16 | `d7cd8752` Cascade: one key for the frame skip and the paint-only repair | 154.4 µs | +2.5 µs |
| 42 | `3985c709` | 155.6 µs | |
| 43 | `86739680` Bound the worst-case compose and GPU frame | 159.2 µs | +3.6 µs |
| 48–130 | | 158.7–160.3 µs | flat |
| merge | `65781b1c` | 161.5 µs | |

Two builds of the same code differ by up to ±1.5 µs (code layout), so a step
of 2 µs is near the noise. The steps at `d68485de` and `86739680` are clear.

## Early interval: `ced36fc3`

`91e17114` → `50b34a49`: 135.7 µs → 144.0 µs. One commit causes all of it:

| commit | `cached_cpu` |
| --- | ---: |
| `91e17114` (parent) | 135.7 µs |
| `ced36fc3` Use exact sRGB transfers and f16 draw colors | 142.9 µs |
| `46f49ac0` | 143.1 µs |
| `250d96c9` | 142.8 µs |
| `27600e34` | 143.6 µs |
| `778a4214` (`Cargo.lock`: `glam` 0.33.7 → 0.33.9 and five others) | 143.7 µs |
| `7f7978ee` | 144.4 µs |
| `7e1a338e` | 144.3 µs |

`ced36fc3` changes 80 files. Apart from the colour transfers and the f16 draw
colours, it also changes the text system (`font_family.rs`, `shaper.rs`,
`system.rs`) and the frame cycle. These measurements do not show which part
costs the 7 µs.

## Method

- `taskset -c 2 setarch -R`, with the SMT sibling idle. Governor `powersave`,
  EPP `balance_performance`. The pinned core ran at 4.64 GHz.
- Every build used `--config` to add `target-cpu=x86-64-v3`, so builds before
  `.cargo/config.toml` existed compare with later builds.
- Run-to-run noise of one binary is less than 0.5% on the CPU arms and
  approximately 10% on the GPU arms.

**A trap in the method.** `git archive | tar -x` gives every file the commit
time as its mtime. When several commits extract into the same directory and
build into the same target directory, an older commit time always reads as
"not changed". Cargo then rebuilds the crate only when the version, the lock
file or the profile changes, and it silently reuses the previous binary. The
first bisection here was wrong for this reason. Touch every extracted file
before the build, or give each commit its own target directory.

## What was recovered (2026-10-06)

Measured on the same machine, CachyOS, with `x86-64-v3`.

| step | effect |
| --- | --- |
| Before the plan (`c4a28240` to `507f14db`): cutout tables, in-step widget ids, the sRGB lookup, the occlusion tile chains | `cached_cpu` 162.7 → 141 µs, `scrolling_gpu` 7.7 → 5.3 ms |
| Shadow grid (`2676bf74`) | shadow fragments 8.39 M → 5.85 M; `scrolling_gpu` −0.9 ms, `resizing_gpu` −0.75 ms |
| Input-scope memo (`dfdd6823`) | `cached_cpu` −1.8 µs |
| GPU arms on the desktop strategy (`2c608f01`) | a method change: `cached_gpu` 1.41 → 0.13 ms, because a skip frame no longer copies |

Tried and reverted, each slower: folding the rollups at `close_node`
(+22 µs), a word compare of gradient stops (+1.5 µs), a 48-byte `IdEntry`
(+6.3 µs), and sending a full frame after a partial one through the
backbuffer (no gain).

The full run after these steps, against the old README:

| arm | old README | now |
| --- | ---: | ---: |
| `cached_cpu` | 130 µs | 139.6 µs |
| `partial_cpu` | 145 µs | 153.8 µs |
| `scrolling_cpu` | 201 µs | 201.3 µs |
| `resizing_cpu` | 317 µs | 303.6 µs |
| `cached_gpu` | 1.12 ms | 0.13 ms (desktop strategy) |
| `partial_gpu` | 1.37 ms | 1.53 ms (desktop strategy) |
| `scrolling_gpu` | 4.25 ms | 3.40 ms (desktop strategy) |
| `resizing_gpu` | 5.38 ms | 3.71 ms (desktop strategy) |

The GPU arms beat the old numbers. `cached_cpu` and `partial_cpu` are
still 9–10 µs above them; the CPU remainder has no single cause left that
a profile shows.
