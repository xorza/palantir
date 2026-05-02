# Layouts todo

Open layout work pulled from `docs/layout-potential-features.md`.
Each item is shipped-when-conditions-merit; no committed roadmap.

## Stack

- **`flex-basis` + `flex-shrink`.** Preferred size separate from sizing policy + independent shrink/grow weights. Triggers on the third user request for "I want a preferred size that's neither min nor max" — at that point pick between in-tree and Taffy.
- **`flex-wrap` (multi-line wrapping).** New `LayoutMode::Flow` (~200 LOC) for chip lists / tag clouds / responsive button bars. Land when the first widget needs it.
- **`align-items: baseline`.** Leaves report a `baseline: f32` alongside their measured size; stack alignment grows a baseline branch. Triggers on the first form-label widget that visibly needs it.
- **`row-reverse` / `column-reverse`.** Lands with the broader RTL story; not standalone.
- **`order` (visual reordering).** Defer indefinitely — immediate-mode authors can just reorder calls.
- **Percentage sizes.** New `Sizing::Percent(f32)` resolving against parent's resolved size. Only worth it when a layout genuinely can't be expressed via Fill weights.

## Grid

- **`Track::repeat(n, t)` + `Track::minmax(min, max)` + `Track::fit_content(n)`.** Ergonomic shorthands over existing primitives. Bundle as one PR when track-list verbosity gets annoying.
- **Named areas.** Parser for `"header header" "sidebar main"` syntax + name resolution at recording time. Land when an example layout is painful via numeric placement.
- **`grid-auto-flow` + `grid-auto-rows` / `grid-auto-cols`.** Automatic placement for cells without explicit `(row, col)`. Plausible early — first photo-gallery / dashboard widget will need it.
- **Subgrid.** Child grids inherit parent's tracks for cross-grid alignment. Substantial; gate on a real form widget hitting the "use one big Grid" wall.
- **Aspect-ratio constraints.** New `Element.aspect_ratio: Option<f32>` for image tiles / video thumbnails. Knock-on effects on intrinsic queries.

## Universal

- **`position: absolute` / `sticky`.** Escape-flow children for overlays and sticky headers. Today's workaround is a top-level ZStack overlay; replace when the first real modal widget needs it.
- **BiDi / RTL writing direction.** `Ui::set_direction(Ltr | Rtl)` with per-subtree overrides. Affects stack child order, alignment defaults, padding/margin start/end semantics, scroll direction. Significant; gates on first user request.
- **Logical properties (`margin-inline-start` etc.).** Lands with BiDi/RTL.
- **`transform-origin`.** New `Element.transform_origin: Vec2` applied during cascade. First widget with rotation/scale around a center triggers it.
