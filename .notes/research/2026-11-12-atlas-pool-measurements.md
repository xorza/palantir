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
