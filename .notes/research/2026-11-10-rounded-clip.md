# The rounded clip: stencil against coverage

Measured 2026-11-10 on branch `analytic-rounded-clip`, on an Apple laptop GPU (Metal), which writes
no timestamps inside a pass: a GPU case reads its pass from submit to done. Each comparison is a
per-case ABBA (`--exact <case> --measurement-time 3 --warm-up-time 1`, legs A, B, B, A); a row
gives both B-against-A changes. No other host ran.

The scene of `rounded_clip::bench`: 48 cards at 2560 × 1440, each four rows of a fill, a label, an
image and a polyline. `rect` clips each card to its rect; `inside` rounds it, the rows inset past
the arcs; `edge` rounds it, the rows across the arcs; `nested` rounds three clips a card, none
holding the next, the rows across every arc.

## The frame group, against the build before it

The gradient atlas as one group at slot 0 of every color pipeline moved no case past its noise:
`record_pass/gpu/*` within −5 % to +2.5 %, `main_pass/gpu/*` within −1.6 % to +5.5 %, the
`clip_pass` arms within ±1.6 % on the leg pairs that agree.

## Coverage in each draw, one pipeline for both, against the frame group

| case | change |
| --- | --- |
| `record_pass/gpu/rounded/per_item` | −61 % |
| `composer/cpu/clips/*` | −3 % to −7 % |
| `clip_pass/gpu/rect` | +17 % |
| `clip_pass/gpu/inside` | −5 % to −16 % |
| `clip_pass/gpu/edge` | −0.5 % to −1.2 % |
| `clip_pass/gpu/nested` | +14 % |
| `main_pass/gpu/scrolling` (Controls, no rounded clip) | +16 % to +23 % |
| `frame/gpu/{scrolling,resizing,theming,alternating}` | +6 % to +8.5 % |
| `clip_frame/gpu/rect/scrolling` | +8 % |
| `clip_frame/gpu/inside/scrolling` | −6 % |
| `clip_frame/gpu/edge/scrolling` | −3 % to −4 % |
| `clip_frame/gpu/nested/scrolling` | +8 % |

The showcase's pages that clip rounded, at the bench window's logical size: Tabs 22 → 14 draws,
Dock 22 → 14, Clip & transform 17 → 12, Motion 20 → 14.

Two diagnostic builds of that one pipeline, each against it:

- the coverage returning 1 at once: `main_pass/gpu/scrolling` −19 %, `clip_pass/gpu/rect` −14 %,
  `clip_pass/gpu/edge` −30 %: the clip's code, run or not, held the cost;
- an early return where a draw names no chain: `main_pass/gpu/scrolling` −13 %, `rect` −8 %,
  `edge` +2 %, `nested` +1.5 %: 7 % of the cost stayed with no clip run.

## Memory

A window's `Stencil8` held 1 B a pixel at its target's size, 3.5 MiB at 2560 × 1440, and a damage
canvas its own; both went. The clip pool takes 24 B a clip of a chain, a few hundred bytes for a UI.

## The pages, seen

`cargo shot` of Tabs, Dock and Clip & transform at 1600 × 1000, the branch against the frame
group: 30, 20 and 328 pixels differ, each on an arc of a rounded clip, by at most 155 of 255.
Zoomed 12 times, a clipped card's corner under the stencil steps a whole pixel at a time, with a
dark pixel where the card's own edge and the clip's disagree; under coverage it fades over one
pixel and the dark pixels go.
