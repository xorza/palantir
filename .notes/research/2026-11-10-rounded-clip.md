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

