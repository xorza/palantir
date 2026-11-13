# Batching plan

A proposal for one piece of work: when a step lands, its decision folds into `src/gpu/AGENTS.md`
and its numbers into `.notes/PERF.md`, and this file goes. Read off the tree at `3702202d6` on
2026-11-13. Nothing was built or run for it: a count not quoted from a test or a note is derived
from the code, and each step's gate is what measures it.

## Verdict

- **The scissor is a run's, not a group's.** A group closes at every clip whose rect differs,
  whether or not the clip cuts a draw, so quad runs and tier batches follow clip scopes: runs
  equal groups on every showcase page but four (`research/2026-11-10-draw-calls.md`). Skia sets no
  scissor for a draw inside its clip and chains ops under equal clips; a group then closes for
  paint order alone (step 3).
- **The frontend hands the backend runs in draw order.** Three tables anchored by group index,
  merged by a cursor each, carry an order the composer already knows; a run's scissor would grow
  all three (step 2).
- **A group is the field's layer, searched one deep.** The lane order inside it is sound and
  stays. What the field adds is the choice of layer a draw: the lowest it fits, not the last
  (step 6, gated).
- **Text and quads stay two draws** (Refused).
- **The gain is small where scopes are few and grows with them.** 18 to 8 draws moved the
  Controls page −0.1 to −1.6 % on an Apple GPU (same note). N fields in a column are a quad run
  each today and one in all after; the clip scene's 48 cards are a group each.

## The field

- **Runs in record order, split at each clip and each state**: Qt Quick, whose guide says
  "Clipping breaks batching. Never use on a per-item basis", and Dear ImGui, a command a clip rect.
  Palantir's scissor is here.
- **A scissor only where a draw needs one.** Skia Ganesh adds it only where the draw is not inside
  it (`GrClip::IsInsideClip`), as "a loose scissor increases the chance we can batch the draws";
  Graphite snaps it out to 4 px against "a tight scissor that forces a state change"; ImGui's
  tables merge the columns "where the contents didn't stray off their local clip rectangle".
- **A look-back past what a draw does not overlap**: Ganesh's op chains, 10 back; WebRender's
  alpha batches, until an overlap; Graphite's layers, 8 back. The quads' `QuadRuns` is here.
- **A layer a draw, by what it overlaps.** Zed gives a primitive one more than the highest order
  it intersects, or that order where everything there "has a kind of at most `max_shared_kind`,
  since those are drawn first", and sorts each kind by it. Graphite's layer is "a collection of
  independent Draws that are organized by BindingLists", and a draw searches the last 8 for one
  that allows it. A group with its lanes is such a layer; closing it for every later draw is the
  search one deep.
- **Why lanes stay.** A run list with a look-back alone cannot sink text recorded before a later
  quad run behind it, which a lane does by its place; Graphite's forward merging recovers part
  of it.

## Where the draws come from

- **A frame's draws** are, a group, one run a quad draw and one batch a tier, a mesh a draw and an
  image a draw a texture; and one draw a text batch, which alone spans groups.
- **A group closes** at a scissor change (`Ordering::break_for_clip`, on each clip push and pop
  whose rect differs), at an overlap that would swap paint order, and at a backdrop's barrier. Only
  the first is not owed to paint order.
- **A clip scope** is every `Scroll`, `TextEdit` and `Splitter` pane, an `Expander` body while it
  opens, a rounded tab strip, and what an app clips itself. Darkroom's graph canvas, image viewer
  and inspector are one each, and a string field inside a node is one inside the canvas's.
- **Counted today** (`rounded_clip_pages_draw_their_counts`, 1280 × 720 at scale 1): Tabs 14,
  Dock 14, Clip & transform 12, Motion 14. `atomic/composer/cpu/quads/clipped` expects a group a
  quad, 4096 at its heavy point.
- **What a command costs** is known on one host: on the intel, wgpu-core and the driver record a
  frame's commands in 56 to 84 µs of a CPU half of 210 to 380 µs, palantir's own upload and draw
  loop 3 % (`PERF.md`, "The GPU frames on the intel"). No note splits that time by command.

## Steps

In order; each ends with the gate of `AGENTS.md` and the census of step 1.

### 1. The census covers every page — done, `93ea5a4d5`

- **Landed.** `every_page_records_its_census` (`palantir-showcase/src/shot_tests.rs`) holds every
  page's whole `PassCounts`; `src/host/offscreen/tests/census.rs` the three scenes, built on the
  offscreen host from widgets so one scene gives both the counts and the closes; `PaintOrder`'s
  `group_closes` the tallies, a close by clip, paint order, barrier or end, and the draws after.

- **Change.** `rounded_clip_pages_draw_their_counts` (`palantir-showcase/src/shot_tests.rs`)
  widens to every page and to the whole `PassCounts`, which gains the viewport pushes and the
  vertex and index binds the recorder issues uncounted. The composer's fixtures gain what the
  showcase lacks, each with its exact counts: a column of fields; a scroll viewport whose first
  and last rows are cut; a clipped canvas of a mesh, curves and cards with a field each, some
  cards overlapping. `PaintOrder` tallies why each group closed, and how many draws followed the
  close, under `TestOnly` as the pruner's `contains_tests`.
- **Why.** Every later step claims a count; the table states it before and after, exactly, and
  the tallies are step 6's gate. Refused: a throwaway counting patch, as the 2026-11-10 note used,
  which no later change can rerun.

### 2. The composer hands the backend runs in draw order — done

- **Landed.** `DrawRun` and `Lane` in `render_buffer::draw_run`; the decision folded into
  `src/gpu/AGENTS.md`, Shapes. Its gate, as met: the census moved only in scissors, down by one or
  two on 15 pages and the canvas and field scenes and up nowhere, as the old walk set a group's
  scissor where the group drew only text that set its own; the visual suite bit for bit. ABBA on
  the Apple laptop: `atomic/schedule/*` −35 to −70 %; `atomic/composer/cpu/*` within ±2 % but for
  the cases that list a run a draw, `clips/*` +2 %, `quads/clipped` +4.5 to +8 %,
  `higher_kind_overlap/text_between_mesh_over` +6 %, `mixed_overlap` +5 to +10 %, the time in
  `Ordering::flush` writing a 36-byte run where a group wrote 28 and a tier batch 12: about 2 ns a
  run listed, against 3.3 ns a run saved at each walk.

- **Change.** `RenderBuffer` holds one list of runs, each a lane, a span of that lane's rows and
  a scissor, in the order they draw; a backdrop's barrier is a run's index. The composer appends
  a group's runs as it closes: its quads, the text batches that end in it, its tiers in tier
  order. A batch that closes after its last group has closed appends then, as it overlaps none
  of that group's tier draws nor anything recorded since (`Ordering::order` closed it or the group
  at each that did). `DrawGroup`, `GroupBatch`, `TextBatch` and the per-tier batch lists go, and
  `ScheduleWalk` is one loop that cuts each run's scissor by the damage.
- **Why.** The order is decided once. Today the composer decides it and the schedule builds it
  again from three anchors, and the tier order is an invariant the two share
  (`HigherKindRects::conflicts`, `emit_group_body`). As Zed's `Scene::batches` and WebRender's
  batch lists hand their renderers batches in order.
- **Structural, and step 3 is its workload**: a scissor a run would otherwise go into each of the
  three tables and their cursors.
- **Gate.** No count of the census moves; the visual suite bit for bit; `atomic/schedule/*` and
  `atomic/composer/cpu/*` ABBA.

### 3. A run carries the scissor its draws need — done, its composer cost open (step 8)

- **Landed.** The decision folded into `src/gpu/AGENTS.md`, Shapes, with what it adds to this
  step: a text batch keeps its bounds as its scissor, a run holds the bounds it paints so a
  damage walk skips what it misses, and a clip that cuts a quad keeps its group's quads under it.
  The census falls or holds on every page, none rising; the visual suite bit for bit with the new
  clip-edge case; the GPU frame cases flat on the Apple laptop and on the board. Open:
  `atomic/composer/cpu/*` on the Apple laptop, as groups no longer split at clips, `clips/*` +39
  to +46 %, `quads/{clipped,rounded_clip}` +23 %, `higher_kind_overlap/{same_tier_image,
  mixed_overlap}` +22 and +24 %, `quads/plain` +10 %.

- **Change.** `break_for_clip` goes. A lane's run opens with the scissor its first draw needs:
  the clip in force where it cuts the draw, none where it does not. A draw of bounds `B`, seen as
  `V = B ∩ clip`, joins the lane's open run while that run's scissor `S` cuts it as its own clip
  does, `B ∩ S = V`, and opens the next otherwise. The quads' look-back keys on the scissor
  beside the form, so uncut quads either side of a cut one join. A text run takes a scissor only
  where a row of it turns; the composer keeps the bounds `closed_hit` rejects by.
- **An occluder's cover is cut to its clip.** `record_opaque_cover` holds the cover inside the
  rounded clips alone, which is sound while a group has one scissor; with a scissor a run, a cut
  occluder would prune a quad it covers only outside its clip.
- **Why.** A scope whose content its clip does not cut costs nothing, and one that cuts costs
  the runs of its cut draws, not a group. The field's second family, above.
- **Refused.** A scissor a group, narrowed as its draws need: a cut draw would still close every
  lane. A rect clip in each instance's chain, refused by measurement (`src/gpu/AGENTS.md`,
  Rounded clip). Cut boxes clipped on the CPU, as a glyph is: exact for a sharp, solid,
  borderless, unturned box alone. Clip distances: see Open.
- **Kept.** A turned run's scissor, cut or not: its quads fade half a pixel past each turned edge.
- **Cost.** A partial frame draws, once a damage rect, every run whose scissor meets it, and a
  run with none meets each; the encoder has culled the list to the damage, so what repeats is
  vertices. `QuadRuns::join` copies its whole group twice once one quad moves, and groups grow:
  copy from the first moved quad on.
- **Risk.** A draw that shades past the bounds it was admitted with is cut by its scope's scissor
  today, and would no longer be. Read as safe: a still quad shades its `urect`
  (`AA_HALF_WIDTH ≤ 0.5`), a turned one its `turned_reach`, a shadow its rect grown by its halo, a
  curve its `StrokeBbox`, a mesh its bbox grown by the fringe, an image and an icon their rect.
  Not read: what an app's effect shades. The visual suite decides.
- **Gate.** The census falls on every page with a scope and rises on none; `quads/clipped` is one
  group. `cargo visual` bit for bit against goldens written from the unchanged tree, with one new
  case first: every draw kind flush inside a clip's edge, and across it. ABBA on a desktop GPU and
  the board: `atomic/composer/cpu/*`, `atomic/clip_pass/gpu/rect`, `integration/clip_frame/*`,
  `integration/frame/gpu/{scrolling,partial,alternating}`.

### 4. The main pass's families share a pipeline layout and an index buffer — done

- **Landed.** `AtlasGroup::pipeline_layout` and `Programs::indices`; the decision folded into
  `src/gpu/AGENTS.md`, Shapes. Census: viewport pushes fall on every page, Shadows 13 to 1;
  index binds fall where images and effects draw, Materials 13 to 7; bind groups rise by one to
  three where a fill follows an image, Colors, Images, Materials, the cost named below. ABBA of
  `integration/frame/gpu/*` and `atomic/main_pass/gpu/scrolling`: the board within ±0.3 %; the
  intel within its noise, its two base runs apart by up to 10 %.

- **Change.** Fills, shadows, rasters, meshes and curves build over one layout, the frame group
  and the atlas group; each binds the atlas group through the recorder, which elides it while
  bound. `Programs` holds one `TemplateIndices`, which the quad, image and effect families draw
  through.
- **Why.** The five build over five layouts of two shapes, and wgpu-core tells layouts apart by
  identity (`Arc::ptr_eq`): a change clears the immediates through the hal
  (`wgpu-core` 30.0.2, `command/pass.rs`, `change_pipeline_layout`) and `PassRecorder::bind`
  pushes the viewport again, two commands a switch of family. The three index buffers hold the
  same bytes, and the recorder, which compares by address, binds one again at each switch among
  the three.
- **Refused.** A layout a shape, two in all: a fill and a text run, the commonest switch, would
  still change layout.
- **Cost.** wgpu-core refuses a draw with a group of its layout unbound
  (`Binder::check_compatibility`), so a fill after an image, whose texture takes the slot, binds
  the atlas group again, one command where the switch saves two.
- **Gate.** The pushes and binds step 1 counts; ABBA of `integration/frame/gpu/*` and
  `atomic/main_pass/gpu/scrolling` on the intel and the board, where a command costs most.

### 5. One raster stream, if the census asks

- **Change.** One instance stream for both raster tenants, a `RasterPass` keeping its atlas and
  uploads; a raster run's rows in record order, text rows and icon rows; the `Icon` tier goes. A
  cut icon drawn at its raster's size clips as a glyph does (`RasterQuad::clipped`); a resampled
  or turned one takes the run's scissor, as a turned text run.
- **Why.** Text and icons draw through one pipeline and one group from two vertex buffers, so a
  group that holds both draws twice, and an icon batch never spans groups as a text batch does.
  `PERF.md` lists it to measure.
- **Cost.** An icon over an image or a mesh closes the group, where the `Icon` tier sits above
  both today. Under step 6 it costs that icon's layer alone.
- **Gate.** Build it only where step 1's tallies show more groups holding text and icons than
  icons over images, or after step 6; then the census and the visual suite.

### 6. A draw takes the lowest layer it fits, if the tallies ask

- **Change.** A row carries a layer: over every earlier draw it overlaps, that draw's layer, and
  one more where that draw's lane replays after its own; a barrier raises the floor. Rows sort by
  layer, stably, when a barrier's span ends, and a layer appends its runs as a group does. Among
  the layers a draw fits, it takes the lowest that already holds its lane, so it opens no run a
  higher one has.
- **Why.** Conflicts that do not touch share a layer. Today each closes the group for every
  later draw, whatever that draw overlaps: k overlapping pairs of cards are k closes, each a run
  more of every lane drawn after. The field's fourth family: Zed's rule is this one, its kinds the
  lanes.
- **It ends** the text batch that spans groups, a layer's text being one run; the `Icon` tier's
  place above `Image`; and "One ordered lane" (`PERF.md`, Open).
- **Refused.** An R-tree a frame, as Zed's `BoundsTree`: the per-lane `RectGrid`s answer the same
  question with a layer beside each rect. A draw reads the lanes above its own, as today, and of
  its own and those below only the rects above the floor, so a frame with no conflict asks what
  it asks today. An order that counts every overlap, as Graphite's `BoundsManager` gives its
  sorted list: each level of nesting becomes an order, which Graphite settles with a depth
  buffer. A layer a quad form: a solid box on a gradient one would lift its text a layer; forms
  stay runs inside the quad lane, joined by the look-back.
- **Cost.** The rect sets live for a barrier's span, not a group; a rect past the floor is read
  to its end, with no first hit to stop at; rows move once in a frame that holds a conflict.
- **Gate.** Step 1's tallies: closes for paint order with draws after them, on the canvas
  fixture and in Darkroom. Then a spike, kept only where `atomic/composer/cpu/*` holds its floor
  and the census falls.

### 7. An uncut run skips its clip pass — done

- **Status.** The composer marks each row whole or cut against its unclamped reach, as the
  viewport's edge cuts glyph quads too, and a turned run's batch still binds only to a clip's
  cut. On the Apple laptop: `text_atlas/*/prepare` −17 to −18 %, the frame cases and
  `encoded_cache/*` within the floor.

- **Change.** A row the composer saw uncut, with no chain, skips `RasterPass::clip_since`, which
  reads and writes back every glyph of every run each frame.
- **Risk.** It holds only if a run's ink bounds every glyph quad; a `debug_assert!` that an uncut
  run cuts none proves it first.
- **Gate.** The text backend's atomic cases; refuse it within the floor.

### 8. The composer pays for runs per draw no more than it did for groups — in progress

- **Status.** Done: a run's bounds come from unions the composer keeps anyway, a tier's from
  its overlap grid and the quads' from one union a group; the join reads shaded rects only past
  its one-key fast path, passes over a group of one key whose clips cut no quad, and copies the
  group only where a quad moved; a closing group lists each tier's open run without a pass
  through its closed runs. On the Apple laptop against the step 2 tree: `quads/clipped` +15 %,
  `quads/rounded_clip` +7 %, `quads/plain` +6 %, `same_tier_image` +16 %, `mixed_overlap`
  +19 %, `clips/*` +35 to +43 %. What is left is the paint-order overlap grids answering over
  one group where clips used to cut many: their union no longer rejects, so each draw walks a
  tile, the quad's queries of the higher tiers and of the closed text most of it, and the prune
  over the whole group.
- **Refused: a fence a clip scope.** A clip whose scissor lay apart from every rect the grids held
  fenced its scope, and a draw in it scanned only the rects pushed since, as an empty group
  would. Its own check is a tile walk a clip, the walks it saved were as cheap, and `clips/rect`
  ran 8 % slower than without it.

- **Change.** Take back the composer time step 3 added, now that one group spans every clip: a
  tier draw's run update (its scissor test and its bounds union, about 4 ns a draw on the Apple
  laptop), the quads' look back and prune over a whole group where a clip used to cut it short,
  and the quad runs' bounds at the group's close. Candidates, each its own bench in one build:
  the bounds taken where the draw already holds its visible rect, not read again at the close; a
  run's bounds kept only where a partial frame reads them; the prune and the look back skipping a
  quad its clip leaves apart from every other.
- **Why.** Step 3 trades composer time for fewer commands: the census fell, the GPU frames held,
  and `integration/clip_frame/cpu/*` moved +2 to +4 %, but the atomic cases above still read
  +10 to +46 %.
- **Gate.** `atomic/composer/cpu/*` ABBA within ±2 % of the step 2 tree (`bd30eb4e2`) on the Apple
  laptop and the board; the census and the visual suite unmoved.

## Refused

- **One pipeline for text and quads**, as egui and Dear ImGui draw everything through one
  textured shader over a white texel. Its fragment would read the template's varyings and a page,
  where an Apple GPU pays for each varying a fragment reads and V3D compiled a choice among four
  pages from 196 to 943 instructions (`src/gpu/AGENTS.md`, Shapes and Atlases), to save one draw
  a group.
- **Solid boxes as raster quads**, the white texel inside the raster stream, so a box over text
  keeps the batch. Only a sharp, solid, pixel-aligned box fits a raster quad; a quad over text
  closed one batch on one page of 24; and a stretched texel takes the resampled path, four taps
  a pixel.
- **A clip in every instance**, as Zed's `content_mask`: its quad is one quad whose fragment
  measures everything, where a template's vertices are the shape, so the cut needs clip distances
  (Open) or the fragment's chain (refused above).
- **Opaque draws sorted by pipeline under a depth buffer**, as Qt Quick's and Graphite's opaque
  passes: refused already (`src/gpu/AGENTS.md`, Shapes).

## Open, no workload

- **Clip distances for cut draws.** After step 3 a scope costs runs only where its clip cuts
  content, a scroll viewport's edge rows. `Features::CLIP_DISTANCES` would cut those in the
  vertex stage (Metal, Vulkan with `shaderClipDistance`, GL desktop or `EXT_clip_cull_distance`):
  four outputs more a vertex, a variant of every family, and the scissor kept for a device
  without. Gate: step 1's tallies showing cut runs as the larger share of the draws left.
- **An image a draw.** Images of one texture join only as neighbors, and a preview a node is a
  draw a node. The quads' look-back keyed by texture joins repeats; distinct textures need an
  atlas of small images, as Qt Quick's, which keeps large ones out. Gate: a canvas of 50
  previews.
- **A mesh a draw**, and a shadow's entry splitting a run its key joins (`PERF.md`, To measure).
- **A curve orders by its whole bound.** A wire across the canvas conflicts with every draw
  under its `StrokeBbox`; its sub-instances' bounds would be near exact. It matters most under
  step 6, where one conflict no longer hides the next.

## Sources

- Skia Ganesh, [`ClipStack.cpp`](https://github.com/google/skia/blob/main/src/gpu/ganesh/ClipStack.cpp)
  and [`OpsTask.cpp`](https://github.com/google/skia/blob/main/src/gpu/ganesh/ops/OpsTask.cpp).
- Skia Graphite, [`ClipStack.cpp`](https://github.com/google/skia/blob/main/src/gpu/graphite/ClipStack.cpp),
  [`Device.cpp`](https://github.com/google/skia/blob/main/src/gpu/graphite/Device.cpp),
  [`BoundsManager.h`](https://github.com/google/skia/blob/main/src/gpu/graphite/geom/BoundsManager.h);
  its layers at m153, [`DrawListTypes.h`](https://skia.googlesource.com/skia/+/chrome/m153/src/gpu/graphite/DrawListTypes.h)
  and [`DrawListLayer.cpp`](https://skia.googlesource.com/skia/+/chrome/m153/src/gpu/graphite/DrawListLayer.cpp).
- Zed, [`bounds_tree.rs`](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/bounds_tree.rs)
  and [`scene.rs`](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/scene.rs).
- WebRender, [`batch.rs`](https://github.com/servo/webrender/blob/main/webrender/src/batch.rs).
- Qt Quick, [the scene graph's default renderer](https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph-renderer.html).
- Dear ImGui, [`imgui_tables.cpp`](https://github.com/ocornut/imgui/blob/master/imgui_tables.cpp),
  `TableMergeDrawChannels`.
- wgpu 30.0.2: [`Features::CLIP_DISTANCES`](https://docs.rs/wgpu-types/30.0.2/wgpu_types/struct.Features.html#associatedconstant.CLIP_DISTANCES);
  `wgpu-core`, `command/pass.rs` and `command/bind.rs`.
- egui, [`WHITE_UV`](https://docs.rs/epaint/latest/epaint/constant.WHITE_UV.html).
