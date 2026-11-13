# Atlas memory over the bench scenes (2026-11-12)

Step 0.1 of `.notes/proposals/atlas-pool.md`: `cargo atlas-memory`, 240 frames a run, each run a
host of its own, on an Apple laptop (Metal), the showcase's first page at 2560 × 1440 (2×), the
modal at 3840 × 2160 (2×). Bytes at a run's last frame, the peak where it differs, and the bytes
the atlas's live items fill. The timing cases of step 0 (`atomic/atlas_move`, `atomic/atlas_page`,
`atomic/atlas_pages`, `atomic/gradient_atlas/cpu/gap_row`) are built and not yet run.

## Every frame arm of the first page

The same 7.3 MiB in every arm: text mask 1.0 MiB, text color 256 KiB, icon mask 64 KiB, icon
color 1.0 MiB, shadow cutouts 4.0 MiB, gradient rows 512 KiB, and the CPU mirror 512 KiB beside
them. Live: the text mask holds 88–99 KiB (9 %), 535 KiB (52 %) under `theming`, which rasterizes
each font size anew; the color sides, the icon mask and the cutouts hold nothing; the gradient
rows hold one row, the fallback. No arm grows any atlas.

## Backdrops

| run | results (live) | blur, last (peak) | total, last (peak) |
|---|---|---|---|
| blur, frosted: moving | 8.0 MiB (382 KiB) | 3.1 MiB (26.8 MiB) | 18.4 MiB (42.2 MiB) |
| blur, frosted: scrolling | 8.0 MiB (382 KiB) | 26.8 MiB | 42.2 MiB |
| glass: moving | 128.0 MiB (10.4 MiB) | 3.0 MiB (55.0 MiB) | 138.3 MiB (190.3 MiB) |
| glass: scrolling | 128.0 MiB (10.4 MiB) | 55.0 MiB | 190.3 MiB |
| modal at 4K: cached, scrolling | 8.0 MiB (1.1 MiB) | 28.2 MiB | 43.6 MiB |
| cards, cards apart, many | 8.0 MiB (598–680 KiB) | 10.3–10.8 MiB | 25.7–26.2 MiB |
| stacked | 8.0 MiB (2.7 MiB) | 11.4 MiB | 26.8 MiB |
| pairs | 8.0 MiB (224 KiB) | 7.1–13.8 MiB | 22.4–29.1 MiB |
| first | 8.0 MiB (0 B) | 0 B | 15.3 MiB |

- **Glass holds 128 MiB of results for 10.4 MiB live.** Its light blur keeps a result at full
  resolution, and the toolbar's, 2560 physical px wide, passes the results atlas's budget edge of
  2048: the atlas grows to 4096², and its shrink, which packs into the budget edge, can place that
  result nowhere, so the atlas never comes back.
- **A first backdrop with nothing behind it makes the 8 MiB results atlas and fills none of it.**
- **The blur's own textures outweigh the results** at a coarse lattice, 7–28 MiB, and step down
  between expiry windows (26.8 → 3.1 MiB while a popup moves).
- **The cutout atlas holds nothing in any scene**, as no bench scene paints a shadow: 4 MiB made
  at startup for none.

## Moves, pages and the page branch on an Apple laptop

`atomic/atlas_move`, `atomic/atlas_page` and `atomic/atlas_pages`, criterion's median, the test
profile with `--bench`: each case times work submitted and waited for on the device, which the
profile of palantir's own code barely moves, so they stand until a bench-profile run. Every
waited submit costs about 250 µs on this machine, the floor of each case.

| case | 16 | 64 | 256 |
|---|---|---|---|
| move by copy, R8 24² | 256 µs | 314 µs | 931 µs |
| move by draw, R8 24² | 254 µs | 255 µs | 254 µs |
| move by copy, RGBA16F 128² | 258 µs | 346 µs | 1110 µs |
| move by draw, RGBA16F 128² | 256 µs | 260 µs | 549 µs |

- **A copy command costs about 2.8 µs here**, `(931 − 256) / 240`; a draw moves 256 glyphs in
  the floor. The draw wins past a few dozen items in every format, and 256 rectangles of 128²
  RGBA16F, 32 MiB, take a draw 300 µs past the floor and copies 850.
- **Making a page**: at the floor up to 1024² of RGBA8 and 256² of RGBA16F; 2048² of RGBA8
  (16 MiB) about 300 µs past it, 1024² RGBA16F 80 µs and 2048² RGBA16F (32 MiB) 620 µs.
- **The page branch costs nothing measurable here**: the steady text scene drawn eight times
  over in one pass, 226 µs with one texture of each kind against 223 µs through four pages and
  the branch.
- Not measured here: lavapipe and the Windows AMD iGPU. The bench-profile runs below replace these
  numbers for the Apple laptop, the board and the handheld.

# The pool against its base, on three hosts (2026-11-12)

`1851f60e2`, the commit before the pool's step 1, against `93ce0dcfe`, its step 9; bench profile.
Hosts as `.notes/PERF.md` names them: the apple (Metal, unpinned), the board (V3D, core 3,
cross-built on the Mac with `cargo zigbuild --target aarch64-unknown-linux-gnu.2.41`) and the
handheld (RADV, core 2). Each case ABBA, a criterion run of its own a leg; the medians of the
second pair (B2 against A2), and the first pair's in parentheses where the two disagree by more than
3 points, as a first A leg on the handheld and the apple ran cold. Both builds carried the same
measurement patch, not committed: each iteration timed alone, `iter_custom` summing them, for the
tail. The atomic and memory cases ran the pool's build alone.

## Frames and text

| case | apple | board | handheld |
|---|---|---|---|
| `text_atlas/cache_churn/frame` | 283.0 → 327.9 µs, +15.9 % | 1.46 → 2.05 ms, +40.7 % | 440.4 → 804.6 µs, +82.7 % |
| `text_atlas/cache_churn/prepare` | 34.6 → 47.7 µs, +38.0 % | 133.8 → 185.6 µs, +38.7 % | 162.1 → 234.6 µs, +44.7 % |
| `text_atlas/mixed_stable_churn/frame` | 283.7 → 328.4 µs, +15.8 % | 1.45 → 2.06 ms, +42.3 % | 436.5 → 805.4 µs, +84.5 % |
| `text_atlas/stable_keys/prepare` | 27.2 → 31.2 µs, +14.8 % | 110.4 → 125.6 µs, +13.7 % | 65.2 → 73.6 µs, +12.9 % |
| `text_atlas/steady_warm/prepare` | 4.8 → 5.5 µs, +13.7 % | 18.6 → 21.3 µs, +14.2 % | 11.6 → 12.6 µs, +8.3 % |
| `text_atlas/steady_warm/frame` | +0.1 % | +0.6 % | +2.6 % |
| `text_atlas/zoom_smooth/frame` | −0.1 % | +0.6 % | +1.1 % |
| `frame/cached` | +0.4 % | −0.3 % | +1.1 % |
| `frame/hovering` | +1.4 % | 2.53 → 2.45 ms, −3.1 % | +4.2 % (−0.3 %) |
| `frame/partial` | +0.2 % | 2.84 → 2.76 ms, −2.7 % | −1.8 % |
| `frame/typing` | +0.4 % | 2.70 → 2.62 ms, −2.8 % | −1.0 % |
| `frame/alternating` | +0.8 % | +1.4 % | +2.2 % (−2.6 %) |
| `frame/resizing` | 306.0 → 310.5 µs, +1.5 % | 4.27 → 4.39 ms, +2.8 % | +2.4 % (−0.3 %) |
| `frame/scrolling` | +0.6 % | 4.24 → 4.35 ms, +2.6 % | +3.8 % (−0.1 %) |
| `frame/theming` | 388.4 → 393.8 µs, +1.4 % | 5.10 → 5.26 ms, +3.1 % | 1.53 → 1.55 ms, +1.4 % |
| `backdrop_frame/glass/moving` | +1.3 % | 53.0 → 36.1 ms, −31.9 % | +1.7 % |
| `backdrop_frame/glass/sliding` | +1.0 % | 53.1 → 36.2 ms, −31.8 % | +3.2 % |
| `backdrop_frame/glass/under` | +1.5 % | 125.9 → 95.8 ms, −23.9 % | +2.0 % |
| `backdrop_frame/glass/scrolling` | 1.51 → 1.56 ms, +3.1 % | 142.1 → 114.6 ms, −19.4 % | 4.37 → 4.56 ms, +4.4 % |
| `backdrop_frame/glass/hover` | ±15 %, the pairs opposite | 2.49 → 2.59 ms, +4.4 % | +0.2 % (+4.4 %) |
| `backdrop_frame/many/under` | +0.1 % | 98.2 → 89.4 ms, −8.9 % | +1.3 % |
| `backdrop_frame/cards/scrolling` | −1.3 % | 77.1 → 72.6 ms, −5.9 % | −0.7 % |
| `backdrop_frame/pair_near/scrolling` | +0.3 % | 24.8 → 23.3 ms, −6.1 % | +0.9 % |
| `backdrop_frame/{blur,frosted}/{moving,under,scrolling}` | −0.3 to +2.3 % | −1.3 to −3.6 % | −0.3 to +1.4 % |
| `backdrop_frame/{blur,frosted}/sliding` | +1.6 to +1.8 % | 5.48 → 5.76 ms, +5.0 % | −0.7 %, +4.4 % |
| `backdrop_frame/first/scrolling` | +1.2 % | 7.74 → 7.96 ms, +2.9 % | +1.2 % |
| `backdrop_frame/stacked/scrolling` | +1.7 % | 43.3 → 44.0 ms, +1.7 % | +0.2 % |
| `backdrop_frame/modal/scrolling` | 1.95 → 2.04 ms, +4.7 % | 100.9 → 107.8 ms, +6.8 % | 7.20 → 7.28 ms, +1.1 % |
| every `*/cached` and `*/hover` of a backdrop | within ±1 % | within ±2.1 % | within ±3 % |

- **The text churn regresses on every host, on its hit rate.** Counted on the apple under the test
  profile, `cache_churn/prepare` evicts 9.8 glyphs a frame on the base and 15.6 on the pool, both at
  a 4 MiB mask budget: the pool rasterizes about 1.5 times the glyphs a frame (macOS `sample` puts 1.47
  times the base's `TextGlyphs::rasterize` samples in each frame). `AtlasPool::allocate` holds 11 %
  of the pool's samples, `ShelfPacker::allocate` 4 % of the base's. The churn walks 512 scales,
  each again 512 frames later, past the 400 frames an idle item lives under no pressure.
- **`stable_keys/prepare` and `steady_warm/prepare`, +13 to +15 % on the apple and the board,**
  rasterize nothing; not profiled.
- **The board's glass frames run 19 to 32 % faster**, the frames whose results passed the old
  results atlas's 2048 edge; `cards`, `many`, `pair_near` 6 to 9 %. `modal/scrolling` at 4K is
  +4.7 % on the apple and +6.8 % on the board, the whole repaints of the first page +1.4 to +3.1 %
  on the board, the partial ones −2.7 to −3.1 % there.
- **The tail follows the median.** On the board, pinned and quiet, each case's p99.9 and worst frame
  move with its median: `frame/theming` worst 10.8 → 11.0 ms, `modal/scrolling` 101.2 → 108.1 ms,
  `glass/moving` 53.2 → 36.3 ms. One case has a worst frame of its own: `cache_churn/prepare`, 3.71
  → 7.87 ms in every leg (median 84 → 202 µs). The apple's and the handheld's worst frames are
  scheduler stalls, up to 300 ms on the apple in either build, so only their p99.9 reads: within
  the median's change but for `glass/scrolling` on the handheld, 5.1 → 5.9 ms.

## Moves and pages

Criterion's median, the pool's build; a case waits for its device.

| case | apple | board | handheld |
|---|---|---|---|
| copy, R8 24², 16 / 64 / 256 | 260 / 311 / 879 µs | 0.65 / 2.42 / 10.99 ms | 222 / 684 / 2477 µs |
| draw, R8 24², 16 / 64 / 256 | 260 / 260 / 260 µs | 2.22 / 2.23 / 2.28 ms | 107 / 112 / 123 µs |
| copy, RGBA8 128², 16 / 64 / 256 | 258 / 314 / 902 µs | 0.86 / 3.54 / 19.16 ms | 257 / 825 / 3167 µs |
| draw, RGBA8 128², 16 / 64 / 256 | 260 / 298 / 477 µs | 2.61 / 2.94 / 4.25 ms | 130 / 202 / 500 µs |
| copy, RGBA16F 128², 16 / 64 / 256 | 260 / 365 / 1060 µs | 1.04 / 4.50 / 36.75 ms | 286 / 960 / 4015 µs |
| draw, RGBA16F 128², 16 / 64 / 256 | 258 / 255 / 859 µs | 5.24 / 5.75 / 7.79 ms | 153 / 311 / 1054 µs |
| page R8, 256² / 1024² / 2048² | 260 / 260 / 260 µs | 138 / 380 / 1157 µs | 105 / 109 / 108 µs |
| page RGBA8, 256² / 1024² / 2048² | 261 / 260 / 575 µs | 148 / 474 / 1435 µs | 125 / 110 / 112 µs |
| page RGBA16F, 256² / 1024² / 2048² | 260 / 327 / 940 µs | 174 / 795 / 2840 µs | 132 / 114 / 130 µs |

- **A copy command costs 2.6 µs on the apple, 43 µs on the board, 9.4 µs on the handheld**, R8 24²,
  `(t₂₅₆ − t₁₆) / 240`.
- **On the handheld a draw wins at every count**: 16 glyphs 107 µs against 222, the 64 copies of a
  frame's budget 684 µs against 112.
- **On the board a draw costs a pass over the whole destination page**, 2.2 ms for a 2048² R8 page
  and 5.1 ms for RGBA16F before its first rectangle, so copies win below about 64 glyphs, and the
  frame budget's 64 copies cost about 2.4 ms there.
- **On the apple they tie up to 16 rectangles**; past 64 the draw wins.
- **Making a page costs the board 1.2 to 2.8 ms at 2048²**, the handheld nothing past its floor.

## The gradient flush

`atomic/gradient_upload` stages 32 dirty rows a gap apart, as 32 copies (`split`) or one copy over
the gaps (`merged`); `atomic/gradient_atlas/cpu/gap_row` bakes one row again from its ramp, what a
folded clean row costs now that no CPU mirror holds it.

| | apple | board | handheld |
|---|---|---|---|
| split, gap 16 / 32 / 48 / 64 / 128 | 252 / 260 / 258 / 254 / 253 µs | 1.49 / 1.61 / 1.68 / 1.74 / 2.08 ms | 327 / 330 / 330 / 330 / 330 µs |
| merged, gap 16 / 32 / 48 / 64 / 128 | 219 / 313 / 443 / 543 / 990 µs | 0.51 / 0.93 / 1.58 / 1.75 / 3.54 ms | 230 / 390 / 724 / 943 / 1754 µs |
| a row baked, linear / Oklab | 0.69 / 1.58 µs | 5.44 / 11.91 µs | 2.58 / 8.75 µs |
| a row from a mirror | 0.02 µs | 0.07 µs | 0.03 µs |

- **Staging alone, merged and split cross between gaps 16 and 32 on the apple and the handheld**
  and near 64 on the board, where `MERGE_GAP_ROWS` (48) was set.
- **A folded row's bake costs more than its staging on every host**, 3 to 27 times. A straight-line
  fit gives a copy 4.6 µs on the apple, about 8 on the handheld and 45 on the board, and a staged
  row 0.22, 0.32 and 0.83 µs: a gap pays to fold up to about 5 linear or 2.6 Oklab rows on the
  apple, 7.2 or 3.5 on the board, 2.9 or 0.9 on the handheld.

## Memory with the pool

`cargo atlas-memory`, 240 frames a run, the same bytes on all three hosts. Against the base
(above): every arm of the first page 7.3 MiB → 770 KiB, of which the gradient rows hold 512 KiB
for one live row (2 KiB) and the mask page 256 KiB for 88 KiB; `theming` 1.5 MiB. Backdrops, peak:
glass 190.3 → 68.8 MiB, its results 128 MiB → 13.0 MiB, all of it live; blur and frosted 42.2 →
29.6 MiB; the modal at 4K 43.6 → 31.0 MiB; cards 25.7 → 13.1 MiB; a first backdrop over nothing
15.3 MiB → 770 KiB. The backdrop blur's own textures, outside the pool, are now most of every
backdrop scene: 55 MiB of glass's 68.8, 28.2 of the modal's 31.0. No scene reaches a list's soft
budget: the mask list peaks at 1 MiB (`theming`) of 8, the half-float list at 13 MiB (glass) of
32.

## Where the regressions come from (2026-11-13)

Bisected on the board over the pool's steps, and spiked with switches read at run time in one
build, so each comparison runs one binary. The spikes are measurement code, not committed.

- **Every GPU regression on the board is the page switch.** A shader that reads a list's pages
  picks one through a `switch` over four bindings: the raster quads (`mask_load`, `color_load`),
  the shadow cutouts (`cutout_texels`) and the backdrop composite (`palantir_result`, five
  textures, each arm the whole B-spline). V3D compiles every arm. Its `V3D_DEBUG=shaderdb`, base
  against the pool: the raster fragment 196 → 943 instructions and 4 → 2 threads, the turned
  raster 135 → 689, the backdrop composite 619 → 1340. The bisect puts `frame/gpu/scrolling`'s
  +2.7 to +3.1 % at step 3, the raster's pages, and `modal/scrolling`'s +6.9 %, `frosted/sliding`'s
  +5.0 % and glass's −32 % at step 5, the results' pages. With every switch replaced by a read of
  page 0, which every bench scene fills alone: `frame/gpu/scrolling` −2.7 %, `theming` −2.4 %,
  `main_pass/scrolling` −3.0 %, `modal/scrolling` −7.2 %, `blur/sliding` −5.7 %,
  `frosted/sliding` −5.8 %, `glass/hover` −2.9 %; each takes back the regression. On the apple
  the same spike takes `modal/scrolling` −3.6 %, its other cases within a noisy run. The sRGB
  decode the color pages added moves nothing on the board.
- **The text churn regression is the age-out.** `PageList::age_out` runs under the soft budget
  once a list's one page has grown past its first edge, and evicts what idled 400 frames; the
  churn walks 512 scales, so each glyph goes just before its scale comes back. Every eviction of
  `cache_churn/frame` is an age-out (26 a frame); with the age-out off none is, the whole set
  fitting the 4 MiB budget. Apple, bench profile: `cache_churn/frame` 328 → 281 µs (the base's
  283), `mixed_stable_churn/frame` 329 → 281 µs, `cache_churn/prepare` 47.6 → 7.1 µs (the base's
  34.6). WebRender evicts under its threshold only with several textures in a list
  (`TextureCache::should_continue_evicting`: `bytes_allocated < threshold &&
  !uses_multiple_atlases` returns `None`); a page grown in place is one texture.
- **The one-frame grace changes nothing here**: exempting only what this frame drew moves no
  case's evictions.
- **The glyph lookups' +8 to +15 % (`steady_warm/prepare`, `stable_keys/prepare`) is not the
  placement's rewrite**: the replay with the cached instance in place of `placed_at` moves them
  within ±1 %. About 0.7 ns a glyph, in `AtlasPool::stamp` and the replay loop; not profiled
  further.

## The fixes against the pool (2026-11-13)

`d8d3f02ae` against the working tree that builds the page-reading pipelines for their pages,
binds a composite's one result texture, ages a list only at its budget or across pages, moves by
draw where a pass moves only what it draws, and folds gradient gaps up to 3 rows; bench profile,
thin LTO, ABBA as above, the second pair's change. The text churn cases are left out: the bench
now runs them at a 1 MiB mask budget, so they compare with no earlier run.

- **The board** (V3D, `V3D_DEBUG=shaderdb`): the raster fragment 943 → 200 instructions, the
  turned raster 689 → 274, the composite 1340 → 619, the pre-pool count. `main_pass/scrolling`
  −2.9 %, `main_pass/partial` −2.9 %; whole repaints `scrolling` −2.7 %, `resizing` −2.7 %,
  `alternating` −2.7 %, `theming` −2.6 %, the partial ones within ±0.6 %. Backdrops:
  `modal/scrolling` 107.8 → 100.0 ms, −7.2 %; glass −5.1 to −8.0 %; `stacked` −5.6 %; the slides
  −5.5 %; `cards_apart` −4.2 %; the rest −1.3 to −3.8 %, the cached within ±1 %.
- **The apple**: `modal/scrolling` −4.7 %, `main_pass/partial` −7.3 %, `main_pass/scrolling`
  −1.5 %, glass −1.3 to −1.8 %, the other backdrops −0.2 to −2.3 %, the frames −0.2 to −1.4 %;
  `blur/moving` and `blur/sliding` moved by ±12 % between legs, noise.
- **The handheld**: every case within its floor, glass −0.8 to −3.3 %.
- **The age-out at the starting budget**: with the age-out off, as one page under the budget now
  is, `cache_churn/frame` 328 → 281 µs and `cache_churn/prepare` 47.6 → 7.1 µs on the apple, where
  the pool's base took 283 and 34.6 (the spike above).
