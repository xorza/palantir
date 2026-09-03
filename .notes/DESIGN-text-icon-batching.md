# Design: collapsing text batches across higher-kind draws

Investigation of review item 2, "Icons and text still draw as two batches"
(`.notes/REVIEW-text-glyph-atlas.md`).

The item proposes moving icons onto the text batch path. The investigation
says the cost it names is real, that it is **not** an icon problem, and that
a much smaller change fixes it for every draw kind at once. Two designs
follow. The recommendation is the small one.

## What is true today

**The two tenants already share everything but space.** One
`RasterProgram` — one shader, one group-0 layout, one sampler — and since
the `Bound::Raster` change, one pipeline pair and one bind state. What they
do not share is the atlas, the vertex buffer, and the bind group. That is
deliberate: a glyph miss evicting a tintable icon trades a ~1 µs
re-rasterisation for a 13-72 µs one.

**Text batches span groups; tier batches do not.** A `TextBatch` carries
its own `scissor` (the union of its runs' clipped bounds), its own
`rounded_clips`, and a `last_group` it drains at. A `GroupBatch` — what
every `PaintTier` uses — carries only an item span and the group that
drains it, because only text carries per-run bounds.

**The schedule draws a group as quads, then text, then tiers.** Within one
group, `emit_group_body` emits the quad range, drains every text batch whose
`last_group` is this group, then drains the tier batches in `PaintTier::ALL`
order. So text always paints under mesh, image, icon and curve of the same
group.

**A higher-kind draw closes the open text batch unconditionally.**
`ComposeSession::admit_higher_kind` culls against the clip and then calls
`close_batch()` with no further test. The reason it gives is sound: a batch
left open can accumulate text from a later group, which advances its
`last_group` past this one, and it would then drain *after* this group's tier
batches — putting text recorded *before* the draw on top of it.

**That close is what splits a labelled toolbar.** Eight buttons, each an
icon beside its label, compose to one icon batch and **eight** text batches.
`composer::tests::batching::labelled_toolbar_costs_one_icon_batch_and_a_text_batch_per_label`
pins the number, and its own doc already reaches the conclusion this
document argues:

> The number this pins is that the split is **text's**, not the icon
> atlas's … the same 8-way split already happens today for eight images or
> eight meshes interleaved with labels. That is what makes this a general
> tier-ordering cost rather than something icons introduced.

Its control test, `images_between_labels_split_text_the_same_way`, proves
the point with images.

## The finding

The close is unconditional where the argument for it is conditional.

Take a higher-kind draw `D` admitted in group `i`, with an open batch `B`
holding text `T0..Tk` recorded before it.

- If `B` closes in group `i`, it drains before group `i`'s tier batches, so
  `T0..Tk` paint under `D`. That is record order, and it is correct.
- If `B` stays open past group `i`, it drains at some group `L > i`, so `D`
  paints first and `T0..Tk` paint over it. That is a swap, and it is
  **visible only where `D` overlaps one of `T0..Tk`**.

The open batch's rects are already indexed, in the same tiled grid
`quad_forces_flush` queries, and that query pre-rejects on a union AABB
before it scans anything. So the test the close needs is one the composer
can already answer cheaply.

The other direction is already handled and needs no change: a text run
recorded *after* `D` consults `higher_kinds.any_overlap` in
`ComposeSession::text` and flushes the group when it overlaps, which moves
it past `D`'s tier batch. Text added to `B` after `D` without overlapping is
correct as it stands, because it draws after `D` and was recorded after it.

## Option A — close the batch only on overlap

One condition in `admit_higher_kind`:

```rust
if self.composer.batch.open_grid.any_overlap(bounds) {
    self.close_batch();
}
```

**What it buys.** A labelled toolbar keeps one text batch instead of one per
label. So does a row list, a menu, a tab strip, a tree — any layout that
puts a glyph run beside a non-overlapping icon, image, mesh or stroke. The
win is not icon-shaped; it lands on every tier.

**What it costs.** Two things, both worth pinning:

1. **Longer batches under partial damage.** `drain_text_batches` skips a
   batch whose scissor misses the damage rect. A batch spanning the whole
   toolbar always intersects, so its draw covers every run in it where eight
   small batches would have skipped seven. The instances are built either
   way — `prepare_batch` runs for every batch regardless of damage — so the
   cost is rasterising quads the scissor then discards, minus what the text
   backend's per-run y-cull already drops.
2. **Two pinned behaviours change.** `curves::compose_polyline_between_texts_splits_text_batch`
   uses a polyline that does *not* overlap either run, and
   `batching::quad_flushes_text_in_already_closed_batch_same_group` relies
   on "a polyline far from everything closes the text batch". Both need
   rewriting onto an overlapping draw, which is the case each doc actually
   argues about.

**What it does not touch.** The tier order, `PaintTier::Icon`'s position
above `Image`, the two atlases, the batch record types, the schedule, or the
backend.

## Option B — merge icons into the batch table

What the review proposes. A batch holds an ordered mix of text runs and icon
rows, `PaintTier::Icon` is removed, and the backend draws a batch as its
text span then its icon span — two draws, two bind groups, one pipeline.

**What it buys beyond Option A.** Icon batches would span groups the way
text does. Today a scroll list whose rows each open a group gets one icon
batch per row; merged, it gets one. Option A does not fix that, because it
leaves icons on the group-scoped `GroupBatch`.

**What it costs.**

1. **Icons lose their tier position.** `PaintTier::Icon` sits above `Image`
   so "an icon drawn over an image backdrop lands on top of it without
   forcing a group flush — the common toolbar-button shape". A batch drains
   *before* every tier, so a merged icon recorded after an overlapping
   image, mesh or curve would have to flush the group instead. That trades
   one cost for another on a shape the current order was chosen for.
2. **An intra-batch overlap test.** A text run joining a batch that already
   holds icons must close the batch when it overlaps one, because the batch
   draws all its text before any of its icons. That is a second rect index,
   per batch, on the text hot path.
3. **Icons enter the strict-bounds rule.** An icon has no per-instance clip,
   exactly like a glyph, so its clipped rect must join the scissor union and
   must close the batch when a strict bound would be widened. Today the
   group scissor clips icons exactly and for free.
4. **The batch record, the schedule step and the backend arm all change.**
   `TextBatch` grows an icon span, `RenderStep::Text` becomes a raster step,
   and the icon tier arm leaves `render_groups`.

**Neither option collapses a mixed group to one draw call.** Text and icons
hold different atlases and different vertex buffers, so a batch containing
both is two draws whatever the table looks like. The draw-call win in both
designs comes from *not splitting the text*, not from merging the tenants.

## Recommendation

Option A. It is a condition on a line that already exists, it fixes the
cost for every tier rather than for icons alone, and it costs nothing that
the tier order was designed to buy. What follows is what shipping it
looked like.

## What was built

Option A, as one condition in `ComposeSession::admit_higher_kind`:

```rust
if self.composer.batch.open_grid.any_overlap(bounds) {
    self.close_batch();
}
```

The method's doc now carries the conditional reason and names the
`higher_kinds` check in `ComposeSession::text` as the other half of the same
invariant — neither is sound alone. `Composer::higher_kinds`' own doc lost
the "every higher-kind draw also closes the batch" clause it justified its
per-flush clear with. The clear is still right: the tiers of a group have
drained before any later group runs, so they cannot reorder against a batch
that outlived them.

Four tests moved with it, each because the change made its premise false,
and two arrived to pin what the change bought.

- `curves::compose_polyline_over_prior_text_splits_text_batch` — was
  `..._between_texts_...` with a stroke that missed both runs. Now a table:
  the stroke over the first run splits the batch, the stroke clear of it
  does not.
- `batching::compose_culled_mesh_over_batch_text_keeps_one_batch` — the old
  mesh sat outside the clip *and* clear of the text, so it proved nothing
  once the close became conditional. The mesh now sits at the first label's
  own coordinates under a clip that discards it whole, which is what makes
  "cull first" the thing under test.
- `batching::quad_flushes_text_in_already_closed_batch_same_group` — its
  polyline closed the batch by existing. It now closes it by covering the
  label's far end, and still clears the quad in x, so the flush still comes
  from the closed batch rather than from the stroke.
- `batching::labelled_toolbar_costs_one_icon_batch_and_one_text_batch` and
  its image control — the numbers they pin went from eight text batches to
  one.
- `batching::icon_over_prior_label_splits_batch_and_over_later_label_flushes_group`
  — the ordering the coalescing rests on, read off one fixture twice.
- `batching::text_batch_drains_past_a_non_overlapping_image` — the
  reordering the change *allows*: one batch, two groups, and the batch's
  `last_group` past the image's.

`tests/visual/fixtures/icon.rs` proves the pixels. Its fixture records a
label, an icon over it, and a second label in a rect-clipped panel — a
scissor change flushes the group without closing the batch, which is what
gives an open batch somewhere later to drain. The icon box must be the tint
on every pixel. The pass runs text, icon, text, so the fixture still pins
the two `Bound::Raster` transitions it was written for.

Each of the four new or rewritten behaviours was proved live by mutation:

| mutation | what fails |
| --- | --- |
| never close | the two split tests, and the visual fixture at pixel (25, 9) |
| always close | the two toolbar tests and the drains-past test |
| test overlap before the cull | the culled-mesh test |

## What the measurement said

**The compose cost is not separable from this machine's drift.** Protocol
from `benches/AGENTS.md`: `--save-baseline` on the unconditional close, then
the conditional one against it, then a third leg with the code reverted
against the same baseline.

The third leg is the whole answer. Identical code read **+1.7% to +8.3%**
against its own baseline across the eight `text_between_mesh` cells, and
+19.8% on one `mixed_overlap` cell of the wider run. The conditional leg
read +5.8%, +2.5% and two uncompared cells on the same arms. The change is
somewhere under a few percent and the machine moves more than that, so no
number here is a measurement of it. Re-run it on a quiet machine if the
figure ever matters.

What did change is what the arms were added to count. `higher_kind_overlap`
grew `text_between_mesh_clear` and `text_between_mesh_over` — a 16-column
grid of labels, each with one mesh beside it or on it — and the harness now
asserts the batch count as well as the group count:

| arm | draws | text batches before | after |
| --- | --- | --- | --- |
| `text_between_mesh_clear` | 4096 | 4096 | **1** |
| `text_between_mesh_over` | 4096 | 4096 | 4096 |

The `over` arm is the control: every mesh covers its own label, so every one
of them still closes the batch.

## Option B, against that

Nothing measured here argues for it. The draw calls Option A leaves on the
table are per-group icon batches, and the workload that would show them
costing something — a list of rows each carrying an icon under its own clip
— still does not exist as a fixture. Until one does, Option B pays four
structural costs to collapse draws nothing has been shown to issue, and one
of those costs is the tier position that keeps an icon over an image
backdrop from flushing its group.

## Open question this does not answer

Whether a batch should coalesce without bound. Option A removes the most
common thing that closed one, so the ceiling is now a strict-bounds
mismatch, a rounded-clip change, or an overlapping quad. On a text-dense
frame under partial damage that may be too far: `drain_text_batches` skips a
batch whose scissor misses the damage rect, and a batch spanning a whole
toolbar always intersects. A frame-level reading on a text-and-icon-dense
scene is what decides it, and a coalescing cap is the lever if it does.
