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

---

# Concrete design (after code review)

## Code-side anchor points

The existing code already does most of the heavy lifting:

- **`Tree.shapes: Vec<Shape>`** is a flat `Vec`, mutable by index. No
  data-structure work needed to patch a shape post-record — just an
  accessor.
- **`Shape::Line { a, b, width, color }`** already exists in the enum
  but the encoder currently logs-and-drops it
  (`src/renderer/frontend/encoder/mod.rs:131`). Implementing a
  Line / Bezier draw is a separate but small piece of renderer work
  — needed for connectors regardless.
- **`Shape::RoundedRect { local_rect, radius, fill, stroke }`** with
  `local_rect = Some(...)` is the placeholder shape for scrollbars
  today. Same field set works for any sub-rect shape.
- **`shape_span: Span`** on `NodeRecord` (set in `close_node`,
  `src/forest/tree/mod.rs:362`) covers parent + descendants, with
  the `TreeItems` iterator interleaving "this node's direct shapes"
  and child spans by index gap. Reserving placeholder shapes during
  record fits the existing model with no protocol change — they're
  just shapes pushed in their slot like any other.
- **Frame loop** (`src/ui/mod.rs:203–215`) already has a
  post-`layout.run` / pre-`cascades.run` slot where
  `ScrollRegistry::refresh` sits. The deferred-shape resolution pass
  goes here, ideally **subsuming** scroll's refresh.
- **`SubtreeRollups::compute_node_hashes`** (`src/forest/tree/mod.rs:181`)
  walks `bounds`, `panel`, `chrome`, `clip_radius`, every shape in
  the node's span, and the grid def. Adding `state_salt: u64` is one
  more sparse column hashed alongside.

What's missing:

- A `state_salt` sparse column on `Tree` (`SparseColumn<u64>`),
  hashed in `compute_node_hashes`.
- A `tree.shape_mut(node, slot) -> &mut Shape` accessor (or
  `shapes_mut_for(node) -> &mut [Shape]` returning the in-span
  slice).
- A `DeferredShapeRegistry` on `Ui` (mirrors `ScrollRegistry`,
  capacity-retained).
- An `Encoder` implementation for `Shape::Line` (and a `Shape::Bezier`
  variant for connector edges).
- A `Ui::resolve_deferred_shapes` pass between `layout.run` and
  `cascades.run`.

## Data shapes

```rust
// src/forest/tree/mod.rs additions
pub(crate) struct Tree {
    // ... existing fields ...
    /// Per-node opaque salt mixed into the authoring hash. Sparse:
    /// only widgets with deferred shapes set this. Read by
    /// `compute_node_hashes` so encode cache invalidates when
    /// resolver inputs change without a record-time shape diff.
    pub(crate) state_salt: SparseColumn<u64>,
}

impl Tree {
    pub(crate) fn set_salt(&mut self, id: NodeId, salt: u64) { ... }
    pub(crate) fn shape_mut(&mut self, owner: NodeId, slot: u8) -> &mut Shape { ... }
}
```

```rust
// src/ui/deferred.rs (new)
pub(crate) struct DeferredShape {
    pub(crate) layer: Layer,
    pub(crate) owner: NodeId,
    pub(crate) slot: u8,                // index inside owner's shape span
    pub(crate) resolver: Resolver,
}

pub(crate) enum Resolver {
    /// Outline matching `node`'s arranged rect, optionally inset by `inset`.
    /// Owner must be `node` or an ancestor.
    NodeOutline { node: NodeId, inset: f32 },
    /// Scrollbar track or thumb. Geometry from outer/inner rects + ScrollState.
    ScrollBar { spec: ScrollBarSpec },
    /// Edge between two arbitrary nodes' arranged rects (any layer pair).
    /// Owner must be a common ancestor of from + to.
    Connector {
        from: NodeId, from_port: PortAnchor,
        to: NodeId,   to_port: PortAnchor,
        style: ConnectorStyle,
    },
    /// Free-form rect in owner-local coords: caller supplies the rect
    /// directly. Used by tab-indicator-style consumers that compute
    /// geometry inline.
    Inline { rect: Rect, kind: InlineKind },
}

pub(crate) enum PortAnchor {
    Center, Top, Bottom, Left, Right,
    /// Owner-relative offset applied to the node's rect.
    Offset(Vec2),
}

pub(crate) enum ConnectorStyle {
    Line { width: f32, color: Color },
    BezierH { width: f32, color: Color }, // horizontal-tangent cubic
    BezierV { width: f32, color: Color },
}

#[derive(Default)]
pub(crate) struct DeferredRegistry {
    pub(crate) shapes: Vec<DeferredShape>,
}

impl DeferredRegistry {
    pub(crate) fn begin_frame(&mut self) { self.shapes.clear(); }
    pub(crate) fn push(&mut self, ds: DeferredShape) { self.shapes.push(ds); }

    /// Patch every reserved placeholder. Runs between layout.run and
    /// cascades.run.
    pub(crate) fn resolve(&self, forest: &mut Forest, results: &LayoutResult) {
        for d in &self.shapes {
            let layout = &results[d.layer];
            let tree = forest.tree_mut(d.layer);
            let shape = tree.shape_mut(d.owner, d.slot);
            *shape = match &d.resolver {
                Resolver::NodeOutline { node, inset } => { ... },
                Resolver::ScrollBar { spec } => { ... },
                Resolver::Connector { from, from_port, to, to_port, style } => {
                    let from_rect = layout.rect[from.index()];
                    let to_rect   = layout.rect[to.index()];
                    let owner_rect = layout.rect[d.owner.index()];
                    bezier_in_owner_space(from_rect, *from_port,
                                          to_rect,   *to_port,
                                          owner_rect, style)
                }
                Resolver::Inline { rect, kind } => { ... },
            };
        }
    }
}
```

## Connector geometry — first-class

Connectors are the load-bearing case. Concretely:

1. **Owner = common ancestor.** The connector lives on a node that
   contains both endpoints in its subtree (in node-graph UIs,
   that's the canvas). This is what guarantees encode-cache
   correctness: any layout change to either endpoint propagates
   into the canvas's `subtree_hash` via the existing rollup.
2. **Endpoints by `NodeId`.** The user code recording connectors
   already has `NodeId`s back from `ui.node(...)` calls (or stores
   them in app state keyed by domain id). The resolver reads
   `layout.rect[from.index()]` and `layout.rect[to.index()]` from
   the *current* frame's `LayoutResult`.
3. **Owner-relative coords.** The Bezier / line is computed in
   screen space then translated into the owner's local space (so
   the patched `Shape::Line { a, b, ... }` carries owner-relative
   `Vec2`s, just like `Shape::RoundedRect` with `local_rect`).
   This composes cleanly with the owner's transform during
   encode.
4. **Multi-segment paths.** A cubic Bezier compresses to one
   `Shape::Bezier { a, c1, c2, b, width, color }` shape per edge.
   Multi-stroke shapes (e.g. arrow head + line) use multiple
   reserved slots, one resolver per slot, or a richer resolver
   that emits N shapes (deferred — start with 1:1).
5. **Hit-testing** *(separate concern)*. Rect-only hit-test
   (`DESIGN.md` "Hit-test is rect-only today") doesn't see edges.
   Two paths, picked when needed:
     - **Edges as nodes.** Reserve a tiny zero-size leaf at the
       midpoint with `Sense::Click`. Cheap, fits the existing
       model; gives per-edge click + selection. Recommended.
     - **Per-shape hit-test.** Cascade snapshots carry per-shape
       hit data; bigger architectural change.
6. **Routing.** Bezier with horizontal/vertical tangents (the
   "graphedit S-curve") is the workhorse for node-graph UIs.
   Orthogonal routing with obstacle avoidance is a substantial
   second project; defer.
7. **Shape buffer placement.** Reserve placeholders on the canvas
   *before* recording any node children, so connectors paint
   under nodes (record order = paint order). To reserve N
   placeholders for N edges, push N `Shape::Bezier` with degenerate
   geometry during the canvas's record phase, before opening any
   node child.

## Encode-cache correctness

The contract for any deferred-shape consumer:

- **Owner is a common ancestor of every `NodeId` the resolver
  reads.** Authoring or layout changes on those endpoints
  propagate into the owner's `subtree_hash` automatically (via
  existing rollup), so cache misses correctly when geometry
  needs to change. **`Connector` and `NodeOutline` resolvers
  invalidate purely through this rollup — they need nothing
  more.**
- **External cross-frame state goes through an internal salt.**
  Some resolvers read state that doesn't trace back to any node's
  authoring (scroll offset, focused id, active-tab id). For those
  — only `ScrollBar` and a future `TabIndicator` today — the
  registry quantizes the relevant state, hashes it, and writes
  the hash into `Tree.state_salt[owner]` at registration time.
  `compute_node_hashes` mixes the salt in.
  **This is an internal mechanism. The salt is not a public API
  knob, not a `pub(crate)` widget surface, and not something the
  framework user names or thinks about.** It lives entirely
  inside the `Resolver` enum's match-arm logic. A widget author
  writing a custom widget reaches for the registry the same way
  whether their resolver needs salt or not — the choice is
  encoded in the `Resolver` variant, not at the call site.
- **Placeholder shapes are recorded as constants.** All resolved
  geometry is deterministic given (subtree authoring, available_q,
  owner salt). Cache hits replay correct cmds.

This contract is what makes the system safe to fold into the
existing measure / encode caches without special-casing.

## Per-frame lifecycle

```
[1] Record
    User code:
      // canvas widget records connectors
      for edge in graph.edges() {
          let slot = canvas.reserve_placeholder(ui);
          ui.deferred.push(DeferredShape {
              layer, owner: canvas_id, slot,
              resolver: Resolver::Connector { from, to, ... },
          });
      }
      // canvas mixes graph version into salt for encode cache
      ui.tree_mut().set_salt(canvas_id, graph.revision());
[*] forest.end_frame
    compute_node_hashes (includes state_salt + placeholder shapes)
    compute_subtree_hashes
[2] layout.run (measure + arrange) — populates LayoutResult.rect
[*] Ui::resolve_deferred_shapes  (NEW pass, replaces ScrollRegistry::refresh)
    walks DeferredRegistry, reads LayoutResult, patches Tree.shapes
    via tree.shape_mut(owner, slot)
[3] cascades.run, [4] input.end_frame, [5] damage.compute,
[6] frontend.build (encode reads patched shapes)
```

## Migration

The deferred-shape framework subsumes scroll's `ScrollRegistry`.
Migration:

1. Land `state_salt` sparse column + `tree.shape_mut`.
2. Land `DeferredRegistry` + `Resolver::ScrollBar` variant.
3. Refactor `Scroll::show` to: (a) reserve 4 placeholders on
   outer, (b) push 4 `DeferredShape { resolver: ScrollBar { ... }}`,
   (c) set salt = quantized `(offset, viewport, content)`. Remove
   `ScrollRegistry`; geometry math moves into the `ScrollBar`
   resolver match arm.
4. Implement `Shape::Line` / `Shape::Bezier` in encoder + composer.
5. Land `Resolver::Connector` and a node-graph showcase tab.
6. (Later) Land `Resolver::NodeOutline` for focus ring, etc.

## Known constraints to call out in code

- **Shape buffer post-record mutation contract.** The encode cache
  trusts that `(subtree_hash, available_q)` determines the cmd
  buffer. After this lands, that becomes
  `(subtree_hash, available_q)` — full stop, with the understanding
  that `subtree_hash` already mixes in `state_salt`. The
  `compute_node_hashes` doc comment must be updated to call out
  state_salt's role and the contract that resolver outputs are
  pure functions of the keyed inputs.
- **Owner-must-be-ancestor.** Pin in a debug assert in
  `DeferredRegistry::resolve` (or at push time), since violations
  silently cause stale cache hits — exactly the bug class the
  framework is supposed to eliminate.
- **Slot reservation must be contiguous and counted.** A widget
  that reserves N placeholders and registers M < N resolvers
  leaves un-patched constant placeholders in the shape buffer —
  fine, they paint as no-ops. M > N is a panic in `shape_mut`.
- **Layer crossing.** A connector whose endpoints are on different
  layers (e.g. one in `Main`, one in `Popup`) is an error today;
  resolvers index per-layer `LayerResult`. If cross-layer
  connectors become a workload, the resolver gains a layer field
  per endpoint. Park.

