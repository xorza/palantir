# Draw calls of a showcase frame

Measured 2026-11-10 on branch `shaders` at `183cb577f`, on an Apple laptop GPU (Metal). Each
showcase page, opened in a `Harness` at the bench surface (2560 × 1440 at scale 2), settled for
three frames, then repainted whole by a resize of 2 px. Counters sat on the wgpu render pass
(`draw` and `draw_indexed`, `set_pipeline`, `set_bind_group`, after the recorder's dedupe) and on
each run loop; the patch was not committed.

## Totals

| page | draws | pipelines | binds | quads | text | other |
|---|---:|---:|---:|---:|---:|---|
| Controls | 18 | 10 | 9 | 6 | 11 | curve 1 |
| Colors | 25 | 17 | 20 | 7 | 11 | image 5, curve 2 |
| Tabs | 33 | 24 | 17 | 11 | 16 | mask 4, curve 2 |
| Dock | 24 | 19 | 12 | 12 | 7 | mask 4, curve 1 |
| Custom widget | 18 | 7 | 7 | 4 | 13 | curve 1 |
| Overlays | 23 | 11 | 12 | 5 | 15 | shadows 1, curve 1, effect 1 |
| Text input | 25 | 11 | 11 | 8 | 17 | |
| Focus & keyboard | 28 | 12 | 11 | 7 | 20 | curve 1 |
| Sizing & spacing | 25 | 10 | 9 | 5 | 19 | curve 1 |
| Containers | 28 | 9 | 9 | 5 | 23 | |
| Text | 29 | 9 | 9 | 5 | 24 | |
| Clip & transform | 28 | 19 | 11 | 10 | 13 | mask 4, curve 1 |
| Scroll & zoom | 38 | 10 | 10 | 15 | 23 | |
| Shapes | 30 | 18 | 7 | 16 | 14 | |
| Strokes | 19 | 10 | 9 | 5 | 13 | curve 1 |
| Gradients | 30 | 20 | 9 | 16 | 14 | |
| Shadows | 33 | 17 | 10 | 10 | 18 | shadows 5 |
| Materials | 23 | 16 | 22 | 8 | 12 | effect 3 |
| Effects | 21 | 9 | 8 | 5 | 14 | effect 2 |
| Icons | 22 | 7 | 8 | 5 | 16 | icon 1 |
| Images | 24 | 9 | 9 | 5 | 16 | image 2, curve 1 |
| Motion | 30 | 23 | 15 | 10 | 13 | mask 4, curve 3 |
| GPU view | 7 | 5 | 5 | 3 | 3 | image 1 |
| App & windows | 24 | 9 | 9 | 6 | 18 | |

Text is 3 to 24 draws a frame, about half to four fifths of all.

## Why a text batch closes

| page | cut run | rounded clip | backdrop barrier | quad over it | tier over it |
|---|---:|---:|---:|---:|---:|
| Controls | 10 | 0 | 0 | 0 | 0 |
| Tabs | 11 | 4 | 0 | 0 | 0 |
| Dock | 2 | 4 | 0 | 0 | 0 |
| Overlays | 13 | 0 | 1 | 0 | 0 |
| Text | 23 | 0 | 0 | 0 | 0 |
| Scroll & zoom | 22 | 0 | 0 | 0 | 0 |
| Shadows | 17 | 0 | 0 | 1 | 0 |
| Materials | 8 | 0 | 3 | 0 | 0 |
| Motion | 10 | 2 | 0 | 0 | 0 |

The other pages follow Controls: every close but the frame's last is a cut run. A run an ancestor
clip cuts batches only with runs of identical bounds, as the text shader has no per-instance clip,
so each such run draws alone. Many cuts are of a pixel: the run's reach, its ink padded by the
scale step's slack, passes a clip by one pixel where its glyphs do not, as
`(41, 139, 286 × 36)` against `(42, 140, 285 × 34)` on Controls. The rest are runs a scroll
viewport cuts at its edge.

## Quad runs

A group's quads draw one run per change of draw (`QuadDraw`), in paint order. Against the fewest
runs a group could take, its distinct draws:

| page | runs | fewest | groups |
|---|---:|---:|---:|
| Shapes | 16 | 6 | 5 |
| Gradients | 16 | 6 | 5 |
| Shadows | 15 | 11 | 7 |
| Overlays | 6 | 6 | 5 |
| every other page | = groups | = groups | 3 to 15 |

A group closes where the scissor changes, so the groups of a page follow its clips.

## After a text run clips its own glyphs (step 9a)

The same pages, with an unturned run's glyphs clipped to its bounds on the CPU: no batch closes
at a cut run, and text takes 1 to 5 draws a page.

| page | draws before | draws after | text before | text after |
|---|---:|---:|---:|---:|
| Controls | 18 | 8 | 11 | 1 |
| Tabs | 33 | 22 | 16 | 5 |
| Dock | 24 | 22 | 7 | 5 |
| Text | 29 | 6 | 24 | 1 |
| Scroll & zoom | 38 | 16 | 23 | 1 |
| Shapes | 30 | 17 | 14 | 1 |
| Materials | 23 | 15 | 12 | 4 |
| Motion | 30 | 20 | 13 | 3 |

A text batch now closes only at a rounded clip (Tabs, Dock, Clip & transform, Motion), a
backdrop barrier (Materials, Overlays) or a quad over it (Shadows). `integration/frame/gpu/*`
(the Controls page) moved −0.1 % to −1.6 % on the Apple GPU, where a draw call is cheap.

## After a group's quads join their runs (step 9d)

Shapes and Gradients drop from 17 draws to 7, their quads from 16 runs to 6, the fewest their
groups' keys allow. Shadows keeps 15 runs for 11 keys: its shadows overlap their sources, and a
shadow's entry, which the cutout plan picks after the composer, splits runs the key joins. Every
other page draws as after step 9a. `atomic/composer/cpu/quads/*` moved within ±2 % of the base on
a rerun; a first run read +20 % to +42 % on two cases, which the rerun did not repeat.
