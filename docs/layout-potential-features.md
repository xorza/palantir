# Potential features

Known capability gaps and the reasoning around if/when/how to close them.
**Not a roadmap** — nothing here is committed. Use this as a source of
truth when an issue or PR proposes "let's add X" so we don't re-litigate
each time.

Each entry includes:
- **What** — the feature, with CSS / equivalent-system reference.
- **Use case** — concrete UI that needs it.
- **Path** — how we'd build it (in-tree extension, new layout mode, or
  Taffy-backed). See `src/layout/intrinsic.md` "Future direction" for the
  α/β/γ/δ Taffy framing.
- **Trigger** — what makes it worth doing.

When something here gets built, move the entry into the relevant feature
doc and remove it here.

## Layout — Stack (HStack / VStack)

As described in `src/layout/intrinsic.md`, Stack supports:
`Sizing::Fixed | Hug | Fill(weight)` per child, intrinsic min-content
floor for Fill, max-size clamp, gap, justify, per-child / parent-default
align. That's the committed scope.

### `flex-basis` — preferred size separate from sizing policy

CSS: `flex-basis: 200px; flex-grow: 1` means "I want to be 200 px;
distribute leftover from there proportional to grow weight."

**Use case:** equal-width tabs that shrink under pressure
(`flex-basis: 150px; flex-shrink: 1`); form inputs with a preferred
width and shrink/grow tolerance.

**Path:** new `Element.flex_basis: Option<f32>` field, consumed by
`stack::measure` Fill distribution. Compositional with `flex-shrink` —
once flex-basis lands, flex-shrink is ~one extra knob. **Or** Taffy
α/β/γ.

**Trigger:** third user request for "I want a preferred size that's
neither min nor max." Two requests = "use Grid as a workaround." Three =
α/β/γ vs δ decision.

### `flex-shrink` distinct from `flex-grow`

CSS: independent weights for shrinking under pressure vs growing into
slack.

**Use case:** "everything shrinks uniformly when too small, but only
specific children grow into extra space." Currently we have one weight,
shared.

**Path:** would land alongside `flex-basis` (the same compositional
piece). **Or** Taffy.

**Trigger:** see `flex-basis`.

### `flex-wrap: wrap`

Multi-line wrapping when children overflow.

**Use case:** chip lists, tag clouds, responsive button bars where
buttons wrap to a new row when the bar narrows.

**Path:** **not** a flag on Stack — fundamentally a different layout
mode. New `LayoutMode::Flow` (or `Wrap`) widget with its own algorithm,
its own design doc. Or Taffy-backed flex-wrap.

**Trigger:** first widget that needs it (likely a tag/chip widget). The
new-mode path is small enough (~200 LOC) that this is realistic in-tree.

### `align-items: baseline`

Cross-axis alignment by text baseline rather than top/center/bottom.

**Use case:** label + input rows where the label text and the input's
internal text align by baseline regardless of input's height.

**Path:** structural change — leaves report a `baseline: f32` along
with their measured size; stack alignment math grows a baseline branch.
Affects `LayoutResult` schema (extra per-node f32) and every leaf
widget. Cosmic gives us per-line baseline cheaply.

**Trigger:** first widget that visibly needs baseline alignment (form
labels are the canonical case; today users get away with center
alignment).

### `order` — visual reordering

CSS: `order: 2` to move a child after `order: 1` siblings without
changing source order.

**Use case:** RTL UIs with mirrored layouts, or "show this first on
mobile, last on desktop" patterns.

**Path:** new `Element.order: i32` field, sort children by it during
arrange. Cheap.

**Trigger:** essentially never needed in immediate-mode (the user can
reorder calls). Defer indefinitely.

### `row-reverse` / `column-reverse`

Stack with axis reversed without manually reordering.

**Use case:** RTL UIs.

**Path:** new `Stack::reverse` builder method or `LayoutMode::HStackRev`
variant. Or part of a broader RTL story.

**Trigger:** RTL support in general.

### Percentage sizes

CSS: `flex-basis: 50%`, `width: 25%`. **Distinct from `Fill` weights:**
`Fill(0.5)` distributes leftover proportional to other Fill weights
(relative to siblings), while percentage = parent fraction (absolute).
They coincide only in the "two equal Fill children" case; diverge as
soon as a non-Fill sibling exists or the Fill count changes.

**Use case:** "the sidebar is exactly 25% of the parent regardless of
the main content's other siblings"; layouts where one child must
anchor to a parent fraction independent of the rest.

**Path:** new `Sizing::Percent(f32)` variant. Resolves against
parent's resolved size (chicken-and-egg with Hug parents — same
gotcha CSS has, resolved by treating percent-of-Hug as auto).

**Trigger:** first widget that genuinely can't express its layout via
Fill weights or shared-Fill ratios. Most "I want 50%" cases are
expressible as `Fill(1) + Fill(1)`; the gap is parent-fraction
independent of siblings.

## Layout — Grid

As described in `src/layout/intrinsic.md`, Grid supports:
`Track::fixed | hug | fill(weight)` with `min`/`max` clamps,
explicit `(row, col)` placement + spans, gap, intrinsic-aware Auto under
constraint. Committed scope.

### `repeat(N, …)` ergonomic

CSS: `grid-template-columns: repeat(12, 1fr)`.

**Use case:** wide grids without verbose track lists.

**Path:** trivial — `Track::repeat(n, t)` builder returning
`Vec<Track>`. Pure ergonomic, no semantic change.

**Trigger:** annoyance threshold. Land alongside Step B if it bothers
anyone.

### `minmax(min, max)` shorthand

CSS: `grid-template-columns: minmax(100px, 1fr)`.

**Use case:** clearer than `Track::fill().min(100.0)`.

**Path:** trivial — `Track::minmax(min, max)` constructor that picks the
right inner `Sizing` based on min/max relationship.

**Trigger:** ergonomics; bundle with `repeat`.

### `fit-content(N)` standalone constructor

CSS: `grid-template-columns: fit-content(200px)`.

**Use case:** Auto track capped at N. **We have it** as
`Track::hug().max(N)`; just no dedicated constructor.

**Path:** trivial — `Track::fit_content(n)` builder.

**Trigger:** ergonomics.

### Named lines / named areas

CSS: `grid-template-areas: "header header" "sidebar main"`. Children
reference areas by name instead of `(row, col)` indices.

**Use case:** semantic clarity in complex grids; easier responsive
remapping.

**Path:** parser for the area string + name resolution at recording
time. Pure ergonomic — same expressive power as numeric placement.

**Trigger:** first multi-section layout that's painful to express via
indices.

### `grid-auto-flow`

Automatic placement of cells without explicit `(row, col)`. CSS:
`grid-auto-flow: row | column | dense`.

**Use case:** photo gallery, dashboard cards, anything that's "fill the
grid in order, wrap rows."

**Path:** placement algorithm in `Grid::show` body that assigns
`(row, col)` to cells without explicit placement. The "dense" packing
is the harder variant; row/column-only flow is straightforward.

**Trigger:** first widget that needs auto-placement. Plausible early.

### `grid-auto-rows` / `grid-auto-cols`

Implicit tracks for cells placed outside the explicit template.

**Path:** tied to auto-flow; comes with it.

**Trigger:** with auto-flow.

### Subgrid (CSS Grid Level 2)

A nested Grid inherits its parent's tracks so cells across grids align.

**Use case:** form fields that align label/input columns across multiple
field groups.

**Path:** tracks become a referenceable resource; child grids declare
"inherit from parent's columns." Substantial. **Or** Taffy γ (full
CSS Grid).

**Trigger:** real form widget that hits the workaround "use a single
Grid for everything" wall.

### Aspect-ratio constraints

CSS: `aspect-ratio: 16/9`. Width and height co-vary.

**Use case:** image tiles, video thumbnails, square avatars.

**Path:** new `Element.aspect_ratio: Option<f32>` field. Affects
measure math (one axis derived from the other). Knock-on effects on
intrinsic queries (which axis is the "free" one?).

**Trigger:** first widget needing aspect-locked sizing.

## Universal layout

### `position: absolute` / `relative` / `sticky` (escape from flow)

Children that escape normal layout flow. Sticky headers, fixed
sidebars, modal overlays.

**Today:** `Canvas` provides absolute placement, but it's a separate
parent layout mode, not an escape hatch within a flex/grid container.

**Path:** new `Element.escape_flow: bool` (or `Element.position:
PositionMode`). Affects every container's measure (escaped children
contribute 0 to content size) and arrange (escaped children get a
parent-coordinate slot independent of siblings). Sticky requires
scroll-aware compositing — gates on scroll widget.

**Trigger:** first overlay/modal widget. Plausible early; the workaround
(top-level ZStack with the modal as a child) is acceptable for now.

### BiDi / RTL writing direction

Right-to-left layout flow.

**Today:** hardcoded LTR. Cosmic-text supports RTL shaping; we don't
flip layout.

**Path:** new `Ui::set_direction(Ltr | Rtl)` global, with per-subtree
overrides. Affects HStack child order, alignment defaults, scroll
direction, padding/margin "left" vs "start".

**Trigger:** first user that asks for an RTL UI. Significant work; not
incremental.

### Logical properties

CSS: `margin-inline-start` instead of `margin-left`. Tied to BiDi.

**Path:** with RTL.

**Trigger:** with RTL.

### `transform-origin`

CSS: `transform-origin: 50% 50%`. We have transforms but origin is
fixed at top-left.

**Path:** new `Element.transform_origin: Vec2`, applied during cascade.

**Trigger:** first widget with rotation/scale around a center.

## Non-layout gaps

(Reserved for future entries — text editing, input gestures, animation,
etc.)
