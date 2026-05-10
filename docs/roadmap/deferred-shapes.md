# Deferred shapes (post-arrange shape emission)

**Status:** research / design. Motivated by (1) the scroll first-frame
bar settle (`scroll.md` Now) and (2) a real node-graph UI workload
needing first-class connector edges between arbitrary nodes.

## Problem

Some shapes can't be authored at record time because their geometry
depends on this frame's arranged rects. Today, widgets that try (e.g.
scrollbars) read **last** frame's `LayoutResult` via persisted state —
which means cold-mounted widgets render blank for one frame and the
host has to schedule a redraw to see them settle.

Cases that hit this pattern:

1. **Scrollbar track/thumb** (shipped, settles on F+1 today).
   Geometry from `LayoutResult.{rect[outer], rect[inner],
   scroll_content[inner]}`.
2. **Focus ring** (`focus.md` Next). Outline matching the focused
   widget's arranged rect.
3. **TextEdit selection highlight** (`text-edit.md` Next).
   Selection-fill `Overlay` under shaped text; geometry from the
   shaped run.
4. **Sliding tab indicator** (`animations.md` deferred consumer).
   Bar that physically moves between active tabs — position/width
   from the active tab's arranged rect (and the previous tab's rect
   for spring start).
5. **Per-node damage-flash debug overlay** (`damage.md` Next).
   Outlines on `Damage.dirty` rects.
6. **Resize handles / split-pane gutters** (eventual). Hit-testable
   strips sized to the split pane's arranged extent.
7. **Node-graph connectors / links** (real workload, first-class).
   Bezier or polyline edges between two arbitrary nodes' arranged
   rects. Endpoints can be siblings, cousins, in different subtrees.
8. **Underlines / strikethroughs on wrapped text runs**. Sub-shape
   geometry from the shaper.

## Three-option scoping (originally for scroll only)

### Option (a) — host force-redraws once

`FrameOutput.needs_settle: bool`, host immediately re-redraws when set.
Frame N+1 records bars normally with populated state.

- **Cost:** ~15 LOC.
- **Fixes:** the visible bug.
- **Doesn't fix:** the underlying footgun. Every widget reading
  measured-size data during record needs its own opt-in.
- **Posture:** point fix; not a path forward for connectors (would
  require host-side full re-record per node-graph frame).

### Option (b) — push shapes after arrange

Reserve placeholder shape slots during record. Post-arrange step reads
this frame's `LayoutResult` and patches placeholders.

- **Cost:** ~120 LOC for scroll alone.
- **Fixes:** shape settle (geometry on F0).
- **Doesn't fix:** layout settle (gutter reservation still record-time
  for scroll).
- **Posture:** general fix for "shapes derived from measured size."
  Reusable across all 8 cases above. **The right shape if we're going
  to support connectors.**

### Option (c) — measure-time emission

Layout driver itself runs two-pass internally (CSS `overflow: auto`
semantics): measure once unconstrained, decide bar/gutter, re-measure
constrained.

- **Cost:** larger; re-introduces measure re-entry.
- **Fixes:** entire widget converges in one frame, no reflow.
- **Posture:** correct-by-construction but conflicts with the
  documented "single dispatch" measure contract. Scroll-specific.
  Not relevant to connectors (their endpoints don't affect parent
  layout).

## Why generic, not scroll-only

Six of the eight cases share one pattern: "draw a shape whose geometry
is derived from the arranged rect(s) of one or more nodes, on the
**same** frame those nodes were laid out."

The framework load is paid by the first widget that needs it (scroll):
- Placeholder shape slots reserved during record.
- `state_salt: u64` mixed into per-node hash so encode cache
  invalidates when resolver inputs change.
- `Tree::shape_mut(node, slot) -> &mut Shape` accessor.
- Post-arrange / pre-cascade pass that walks a registry and patches
  placeholders.

Once the load-bearing infrastructure exists, each subsequent consumer
is ~30 LOC (one `Resolver` variant + record-site reservation) instead
of another 120-line parallel registry.

## Sketch — `DeferredShapeRegistry`

```rust
pub(crate) struct DeferredShape {
    pub(crate) layer: Layer,
    pub(crate) owner: NodeId,        // node that owns the placeholder
    pub(crate) slot: u8,             // index within owner's reserved slots
    pub(crate) resolver: Resolver,
}

pub(crate) enum Resolver {
    NodeRect { node: NodeId, deflate_by_padding: bool },
    NodeOutline { node: NodeId, stroke: Stroke, radius: Corners },
    ScrollBar { viewport: NodeId, axis: Axis, theme_idx: u8 },
    Connector { from: NodeId, to: NodeId, /* style */ },
    // extend per consumer
}
```

Per-frame loop:

- **Record:** widget pushes a `DeferredShape` entry + reserves
  placeholder shape slots on its owner node. Sets `state_salt` on
  the owner if resolver inputs include cross-frame state (scroll
  offset, focused id, etc.).
- **end_frame:** per-node hash includes `state_salt` and the
  *unpatched* placeholder shapes (constants).
- **Measure / Arrange:** unchanged.
- **`Ui::resolve_deferred_shapes`** (new pass, between arrange and
  cascade): walks the registry, computes each rect from `LayoutResult`,
  patches the placeholder via `tree.shape_mut(owner, slot)`.
- **Cascade / Encode:** unchanged.

Encode-cache correctness:

- Per-node hash sees salt + constant placeholders.
- Patched shapes don't change hash, but **same salt + same
  available_q ⇒ same arrange ⇒ same patched shapes**, so cached cmds
  are correct.
- Anything that changes resolver inputs (scroll offset, focused id,
  active tab, source/target node positions) must be threaded into the
  owner's salt. That's a discipline, but it's local to each consumer.

## Connector-specific concerns

Connectors break two assumptions of the simple model:

1. **No natural owner.** A connector between nodes A and B doesn't
   belong to A or B — it's an edge in a graph. Owner has to be the
   common ancestor (the graph canvas), not either endpoint.
2. **Geometry is multi-rect.** Bezier control points come from
   *two* arranged rects (plus port offsets). The resolver reads
   both endpoints from `LayoutResult`.
3. **Salt depends on both endpoints' rects.** Connector cache hits
   only when both endpoint positions are unchanged. Mixing both
   endpoints' rect hashes (or quantized positions) into the canvas's
   salt invalidates correctly when either moves.
4. **Variable count.** A graph has N edges, often N >> 1. Reserving
   placeholder slots requires knowing N at record time — fine, the
   user code knows its edge list.
5. **Z-order.** Edges typically paint behind nodes (or above; design
   choice). The canvas is a `ZStack`; reserve placeholder shapes on
   the canvas *before* recording node children so they paint behind.
6. **Hit-testing on edges.** Click-on-edge for selection / delete is a
   real workflow. The deferred-shape system patches `Shape`s in the
   shape buffer, but hit-test goes through the cascade's `HitIndex`
   built from arranged rects + sense flags. Edges aren't nodes.
   Either:
     - Edges become full nodes (with `Sense::Click`, zero-size rect
       at midpoint), and the deferred shape system patches their
       shapes. Hit-test gets per-edge testing for free, but adds N
       nodes per graph.
     - Add a per-shape hit-test path (Shape carries optional sense),
       cascade walks shapes. New axis of complexity; rejected today
       (`DESIGN.md` "Hit-test is rect-only today").
   First option fits the existing architecture; second is a larger
   bet. Picking is workload-driven.
7. **Routing complexity.** Bezier with two control points handles
   straight S-curves; orthogonal routing (Manhattan with obstacle
   avoidance) is much more. Start with bezier; promote when needed.

## Open questions

- **Resolver as enum vs trait.** Enum keeps it alloc-free + obvious;
  trait allows host-supplied resolvers (for app-specific geometry).
  Default to enum; revisit if a real consumer needs the open set.
- **Sub-rect geometry (text underlines, glyph hit-rects).** May not
  fit the `NodeId → Rect` shape — rect comes from inside a shaped
  run, not from a node's arranged rect. Either extend `Resolver` with
  `TextRun { node: NodeId, byte_range: Range<usize> }` and read from
  `LayoutResult.text_shapes`, or keep these inline.
- **Salt semantics.** Quantized to integer logical px? Hash of the
  resolver inputs? Per-shape vs per-node?
- **Multi-shape resolvers.** Connectors emit a single Bezier line;
  if a future consumer wants N shapes from one resolver call (e.g.
  multi-segment path), do we reserve N slots or change the
  contract?

## Implementation order

1. **Land the framework with two consumers (scroll + connectors).**
   Connectors are the load-bearing case — they require multi-endpoint
   resolvers and salt-from-both-ends, which forces the design to be
   correct from the start. Scroll alone could be solved with a
   simpler scheme that wouldn't generalize.
2. **Defer focus ring / tab indicator / damage-flash variants.**
   They become trivial extensions once the framework lands.
3. **Defer text underline / selection.** Sub-rect geometry probably
   wants its own pass; assess after the connector consumer sits in
   the showcase.

## References

- Live consumer: `src/widgets/scroll.rs::push_bar` (record-time;
  reads from `ScrollState`).
- `ScrollRegistry::refresh` (`src/widgets/scroll.rs:85`) is the
  shape the generic registry generalizes.
- `LayoutResult` (`src/layout/result.rs`) is the post-arrange data
  source.
- `Tree.shapes` (`src/forest/tree/`) is the patch target;
  `records.shapes()[i]` gives per-node `Span`.
