# Tree data structure redesign

A clean-slate redesign of `Tree` and friends. Treats the
existing implementation as one data point, not a constraint.

This doc:

1. Maps every per-`NodeId` field to the passes that read it,
   counts cache misses per pass, sizes the structs.
2. Surveys prior art for cache-friendly tree layouts (browsers,
   ECS, GPU encoders, immediate-mode UIs).
3. Reframes the design space along two **orthogonal** axes:
   - **Topology** — single-tree-with-reorder vs forest-of-trees.
   - **Field layout** — SoA grain (today's 5 cols vs tighter
     packing).
4. Proposes a concrete redesign that wins on both axes while
   staying *simpler* than what's there now.

> **Terminology note.** When this doc says "the v2 reorder
> pass," "v2 mid-recording," or just "v2," it always means the
> **popup-feature v2** shipped via `docs/popups.md` steps 1-6 —
> i.e. the *current implementation* of mid-recording popup
> support that this doc proposes to replace. v2 is a feature
> version, not a doc version.

---

## 0. TL;DR

- **Topology axis** — adopt a forest of per-layer trees (one
  arena per `Layer` enum variant) instead of one shared arena
  with end-frame reorder. No trade-off vs cache.
- **Field-layout axis** — split today's 64-byte `LayoutCore` into
  three smaller cells along access boundaries. The cascade hot
  loop reads one `u8` per node out of a 64-byte cache line today
  — 98% waste. Split fixes it without sacrificing measure-pass
  density.
- **Combined recommendation** — `Forest { trees: [Tree;
  Layer::COUNT] }` where each `Tree` carries 7 dense per-NodeId
  columns (down from 5 fat ones), all hot-fields-only. Sparse
  cold extras stay as today. Net read traffic per cascade frame
  drops by ~3-5×; net code drops ~−400 LOC.

---

## 1. Field-by-field access map

**Method.** Grep + read every pass that touches `tree.records.*`,
`tree.bounds`, `tree.panel`, `tree.chrome`, `tree.shapes`,
`tree.rollups.*`. Tabulated below: rows are fields (with size in
bytes), columns are passes. Cell content = "H" if read in that
pass's hot inner loop, "C" if cold (sparse / once-per-tree),
blank if not read.

Sizes are from `cargo +nightly print-type-sizes` (estimated where
unavailable from the source).

| field                       | size | record | hash<sub>n</sub> | hash<sub>s</sub> | measure | arrange | cascade | encode | damage | reorder |
| --------------------------- | ---: | :----: | :--------------: | :--------------: | :-----: | :-----: | :-----: | :----: | :----: | :-----: |
| `widget_id`                 |    8 |   W    |        H         |        H         |    H    |    -    |    H    |   H    |   H    |    H    |
| `shape_span`                |    8 |   W    |        H         |        H         |    H    |    -    |    -    |   H    |    -    |    H    |
| `subtree_end`               |    4 |   W    |        H         |        H         |    H    |    H    |    H    |   H    |    H    |    H    |
| `layout.mode`               |    8 |   W    |        H         |         -        |    H    |    -    |    -    |   -    |    -    |    -    |
| `layout.size`               |   16 |   W    |        H         |         -        |    H    |    H    |    -    |   -    |    -    |    -    |
| `layout.padding`            |   16 |   W    |        H         |         -        |    H    |    H    |    -    |   H    |    -    |    -    |
| `layout.margin`             |   16 |   W    |        H         |         -        |    H    |    H    |    -    |   -    |    -    |    -    |
| `layout.align`              |    2 |   W    |        H         |         -        |    -    |    H    |    -    |   -    |    -    |    -    |
| `layout.visibility`         |    1 |   W    |        H         |         -        |    H    |    H    |    H    |   H    |    -    |    -    |
| `attrs.sense`               |    ¼ |   W    |        H         |         -        |    -    |    -    |    H    |   -    |    -    |    -    |
| `attrs.disabled`            |    ¼ |   W    |        H         |         -        |    -    |    -    |    H    |   -    |    -    |    -    |
| `attrs.clip`                |    ¼ |   W    |        H         |         -        |    -    |    -    |    H    |   H    |    -    |    -    |
| `attrs.focusable`           |    ¼ |   W    |        H         |         -        |    -    |    -    |    H    |   -    |    -    |    -    |
| `bounds.transform`          |   12 |   W*   |        -         |        H         |    -    |    H    |    H    |   H    |    -    |    -    |
| `bounds.position`           |    8 |   W*   |        H         |         -        |    -    |    H    |    -    |   -    |    -    |    -    |
| `bounds.grid`               |    8 |   W*   |        H         |         -        |    H    |    H    |    -    |   -    |    -    |    -    |
| `bounds.min/max_size`       |   16 |   W*   |        H         |         -        |    H    |    -    |    -    |   -    |    -    |    -    |
| `panel.gap/line_gap/…`      |   16 |   W*   |        H         |         -        |    H    |    -    |    -    |   -    |    -    |    -    |
| `chrome` (`Background`)     |   24 |   W*   |        H         |         -        |    -    |    -    |    -    |   H    |    -    |    -    |
| `rollups.node`              |    8 |   -    |        W         |        H         |    H    |    -    |    -    |   -    |    H    |    -    |
| `rollups.subtree`           |    8 |   -    |        -         |        W         |    H    |    -    |    -    |   H    |    H    |    -    |
| `rollups.has_grid`          |   ⅛  |   W    |        -         |         -        |    H    |    -    |    -    |   -    |    -    |    H    |
| `manifest.id_per_node` (v2) |    2 |   W    |        -         |         -        |    -    |    -    |    -    |   -    |    -    |    H    |
| `recording.shape_root_id`   |    2 |   W    |        -         |         -        |    -    |    -    |    -    |   -    |    -    |    H    |

`W*` = sparse write — only when the builder set the field.

Pass abbreviations: `record` = `open_node`/`add_shape`/`close_node`;
`hash_n` = `compute_node_hashes`; `hash_s` =
`compute_subtree_hashes`. `measure`/`arrange` are the layout
recursive descents. `cascade` = `Cascades::run`. `encode` =
`Encoder::encode`. `damage` = `Damage::compute`.

### 1.1 Read patterns by pass

**Cascade hot loop** (`src/ui/cascade.rs:141`, runs `for i in 0..n`):

```
reads per record:
  layout[i].visibility          (1 byte of a 64-byte LayoutCore)
  attrs[i].{sense,disabled,clip,focusable}    (1 byte, packed)
  widget_id[i]                  (8 bytes)
  subtree_end[i]                (4 bytes via stack pop check)
  bounds.get(i).transform       (sparse, mostly None)
total hot bytes: ~14, spread across 4 SoA slices + 1 sparse col.
```

LayoutCore is 64 bytes per record but cascade reads ONE byte from
it. **98% cache waste on the `layout` stream during cascade.**
This is the largest single inefficiency in the current layout.

**Measure descent** — reads everything in `LayoutCore` plus
sparse `bounds`/`panel`. The 64-byte read is justified here.

**Arrange descent** — reads `layout.{size, padding, margin, align,
visibility}` (≈51 bytes of 64) + `bounds` (sparse). Density
mostly fine.

**Encode descent** — reads `layout.padding` only (16 bytes of 64,
75% waste), plus `attrs.clip`, `widget_id`, `subtree_end`, sparse
`chrome`, and walks `shape_span`/`shapes`.

**Hash node compute** — reads everything (full hash). 100%
density — SoA wins nothing here vs AoS.

**Damage compute** — reads `widget_id` + `rollups.subtree` per
record. Two thin streams. Already cache-tight.

### 1.2 Field access affinity

Group fields that are always read together. From the matrix:

| group                    | fields                                                                      | always co-read by              |
| ------------------------ | --------------------------------------------------------------------------- | ------------------------------ |
| **A: hit/visit flags**   | `visibility`, `attrs` (sense/disabled/clip/focusable), `widget_id`          | cascade, encode (partial)      |
| **B: subtree skip**      | `subtree_end`                                                               | every walk                     |
| **C: measure inputs**    | `mode`, `size`, `min/max_size`, `padding`, `margin`, `align`, `gap`/`panel` | measure, arrange (partial)     |
| **D: shape ownership**   | `shape_span`                                                                | encode, hash_n, leaf-text      |
| **E: cache key**         | `widget_id`, `rollups.subtree`                                              | damage, layout cache, encode cache |
| **F: paint chrome**      | `chrome`                                                                    | encode                         |
| **G: transform**         | `bounds.transform`                                                          | cascade, arrange, encode       |

**Observation: groups A and B are tiny and read by every pass.**
Groups C, D, F are fat and read by specific passes. Groups E, G
are sparse-read.

This is a textbook hot/cold split case.

---

## 2. Online prior art — cache-aware tree layouts

### 2.1 Vello — six parallel SoA streams

Vello (Linebender's GPU compute renderer) encodes scene data into
six independent `Vec`s — `path_tags`, `path_data`,
`draw_objects`, `transforms`, `styles`, `info`. Each compute
dispatch reads only the streams it needs. (`tmp/vello/crates/encoding/src/encoding.rs`)

**Lesson.** SoA pays off when you have many passes with disjoint
field needs. Parallel streams cost zero extra memory bandwidth —
each pass loads only its slices.

### 2.2 Web browsers — render tree + paint layer tree + compositor

Blink and WebKit split the render tree (one node per styled
element) from the paint layer tree (one entry per stacking
context — i.e. per visual layer). The render tree handles
geometry and styles in a unified arena; the layer tree is
maintained as a *separate, sparser* tree built out of the render
tree, mapping to compositor surfaces. Compositing is per-layer
parallel.

**Lesson.** Logical separation matches *paint dependency*. Layers
get their own tree because they're independent paint targets.
Maps directly to Palantir's `Layer` enum.

### 2.3 imgui — `ImDrawListSplitter`

Per-window `ImDrawList`, plus `ImDrawListSplitter` (multiple
sub-lists merged at flush) for tables/columns that interleave
content. (`imgui_internal.h`)

**Lesson.** When two regions need independent z-ordering within a
single owner, give them separate buffers and merge in submission
order. Don't share one buffer and reorder.

### 2.4 egui — paint lists per `Order`

`GraphicLayers([IdMap<PaintList>; Order::COUNT])` —
five independent `PaintList`s, one per Order variant
(Background/Middle/Foreground/Tooltip/Debug). Drained in z-order
at end-of-frame. (`tmp/egui/crates/epaint/src/layers.rs`)

**Lesson.** Per-layer storage is the natural representation when
layers are an enum. No reorder pass.

### 2.5 Bevy ECS — archetype storage + dense components

Each entity belongs to an "archetype" (set of component types).
Each archetype stores its components in dense per-component
arrays. Adding/removing a component moves the entity to a
different archetype (memcpy of the row).

**Lesson.** Group entities by access pattern, not by entity id.
For Palantir: nodes that have a `chrome` are a different
"archetype" from nodes that don't — but the per-frame rebuild
cost makes archetype migration overkill.

### 2.6 Clay — flat arena, no layers

Clay uses a single flat arena of `Clay__LayoutElement`s
(`tmp/clay/clay.h`). One pass for layout, one for render commands.
No layers (z-index is per render command, sorted at flush).

**Lesson.** When layers don't exist, the simple flat arena is
plenty. Palantir's complexity comes from layers.

### 2.7 Synthesis

| design                 | layer model                          | cache layout      |
| ---------------------- | ------------------------------------ | ----------------- |
| Vello                  | none (renderer)                      | 6 SoA streams     |
| Browser layer tree     | per-layer separate tree              | per-tree dense    |
| imgui                  | per-window + Splitter                | per-buffer dense  |
| egui                   | per-Order PaintList                  | per-layer dense   |
| Bevy ECS               | per-archetype                        | per-component SoA |
| Clay                   | none                                 | flat AoS          |
| **Palantir today**     | **single tree + end-frame reorder**  | **5-col SoA**     |

Palantir is the outlier on layer model — every other reference
that has layers gives them their own buffer. Palantir's SoA
choice is shared with Vello and Bevy and is correct, but the
column granularity (5 fat cols) wastes cache on hit-test.

---

## 3. Two orthogonal axes

The previous design doc covered axis 1 only.

### Axis 1: Topology

| | (A) single tree + reorder | (B) forest |
| --- | --- | --- |
| storage | one Soa per Tree | one Soa per Layer |
| mid-recording | reorder at end_frame | dispatch by current_layer |
| cross-layer contamination | possible (the bug we hit in step 4) | impossible by construction |
| LOC budget | +280 reorder, +60 scratch, +parallel cols | per-layer Soa default = empty Vec; dispatch ~30 LOC |
| pipeline iteration | for slot in &tree.manifest.slots | for tree in &forest.trees |

### Axis 2: Field layout (SoA grain)

| | (i) coarse — today's 5 cols | (ii) tight — 7-8 cols hot, sparse cold |
| --- | --- | --- |
| `LayoutCore` | one 64-byte struct | split into hot/cold halves |
| cascade hot read | 64 bytes (1 useful byte) | ~16 bytes (all useful) |
| measure descent | one cache line | 2 cache lines (still cheap) |
| code complexity | one struct, one Hash impl | 2 structs, 2 hash impls |

Both axes are independent — pick any combination.

---

## 4. Recommended composition: forest + tighter cells

### 4.1 Topology — forest

Replace `Tree` with `Forest { trees: [Tree; Layer::COUNT] }`.
Each `Tree` is the pre-popup-v2 shape: a pre-order SoA arena
with no reorder, no `id_per_node`, no `shape_root_id`.

```rust
pub(crate) struct Forest {
    /// One arena per layer. Index by `layer as usize`. Empty
    /// layers have empty Vecs (zero alloc).
    pub(crate) trees: [Tree; Layer::COUNT],
    /// Recording-only state: layer-scope stack + active layer.
    /// `(current_layer, layer_anchor)` move into here from the
    /// per-tree level.
    pub(crate) recording: RecordingState,
}

pub(crate) struct Tree {
    // Pure pre-popup-v2 shape — no manifest, no reorder scratch.
    records: Soa<NodeRecord>,
    bounds:  SparseColumn<BoundsExtras>,
    panel:   SparseColumn<PanelExtras>,
    chrome:  SparseColumn<Background>,
    shapes:  Vec<Shape>,
    grid:    GridArena,
    rollups: SubtreeRollups,
    /// Within a single layer, you can still have multiple roots
    /// (e.g. Popup with eater + body, recorded as two top-level
    /// scopes).
    roots:   Vec<RootSlot>,
    /// Recording-only: ancestor stack for this tree.
    open_frames: Vec<NodeId>,
}
```

#### Recording flow

`current_layer` selects the active `Tree`. `open_node` pushes
into `forest.trees[current_layer].records`. `add_shape` pushes
into `forest.trees[current_layer].shapes`. Mid-recording
`ui.layer(Popup, …)` saves `current_layer` on a stack, switches
to Popup, body records into Popup's tree. Pop restores.

```rust
impl Forest {
    fn open_node(&mut self, element, chrome) -> NodeId {
        self.trees[self.recording.current_layer as usize]
            .open_node(element, chrome)
    }
    fn add_shape(&mut self, shape: Shape) {
        self.trees[self.recording.current_layer as usize]
            .add_shape(shape)
    }
    fn close_node(&mut self) {
        self.trees[self.recording.current_layer as usize]
            .close_node()
    }
    fn push_layer(&mut self, layer: Layer, anchor: Rect) {
        self.recording.push_scope(layer, anchor);
    }
    fn pop_layer(&mut self) {
        self.recording.pop_scope();
    }
}
```

Every per-tree call is a one-line dispatch through
`current_layer`. Mid-recording: the next `open_node` goes into
`trees[Popup]`. No interleaving. No contamination.

#### End-of-frame

```rust
impl Forest {
    fn end_frame(&mut self, main_anchor: Rect) {
        for (i, tree) in self.trees.iter_mut().enumerate() {
            if tree.is_empty() { continue; }
            let anchor = if i == Layer::Main as usize {
                main_anchor
            } else {
                self.recording.layer_anchor[i]
            };
            tree.end_frame(anchor);  // patch Main root anchor;
                                     // compute hashes; assert.
        }
    }
}
```

No reorder. No global sort. Each tree finalizes independently.

#### Pipeline passes

Each pass loops over layers in paint order (the `Layer` enum is
already ordered). Today's `Encoder::encode` and
`LayoutEngine::run` already iterate `tree.manifest.slots` — same
pattern, different collection.

```rust
pub(crate) fn encode(forest: &Forest, …) {
    for layer in Layer::iter_paint_order() {
        let tree = &forest.trees[layer as usize];
        for root in &tree.roots {
            encode_node(tree, …, NodeId(root.first_node), …);
        }
    }
}
```

`Cascades::run` becomes per-tree and merges entries across trees
in layer order. Reverse-iter still gives topmost-first because
layers append in paint order = under-to-over.

#### Edge cases

- **Empty popup body** — popup tree stays empty; pipeline passes
  skip it via `if tree.is_empty() { continue; }`. No special
  case. Today this required a `RootSlot` not being pushed.
- **Popup-only frame** — no Main records. Main tree empty;
  Popup tree has the only content. Today: edge case in
  `assert_recording_invariants` and the implicit Main slot push.
  Forest: each tree just finalizes independently.
- **Popup eater + body as two top-level roots** — same layer.
  Each is a `RootSlot` in `tree.roots`. Trivially handled by
  per-layer `roots: Vec<RootSlot>`.

#### What goes away

- `manifest.id_per_node` (forest doesn't bucket records by layer).
- `recording.shape_root_id` (no shared shapes Vec → no
  cross-bucket leak possible).
- `reorder_records` + `ReorderScratch` + `RootManifest::sort_*`
  scratch.
- `Tree::layer_of` (each tree IS a layer).

### 4.2 Field layout — split `LayoutCore` along access lines

Today:

```rust
struct LayoutCore {                 // 64 bytes
    mode: LayoutMode,                // 8     — measure dispatch
    size: Sizes,                     // 16    — measure
    padding: Spacing,                // 16    — measure + encode
    margin: Spacing,                 // 16    — measure
    align: Align,                    // 2     — arrange
    visibility: Visibility,          // 1     — cascade + encode + measure short-circuit
    // 5 bytes padding
}
```

Proposed:

```rust
// Read by every walk's hot loop. Tightly packed.
struct NodeFlags {                   // 4 bytes
    visibility: Visibility,          // 1
    sense: Sense,                    // ¼ (packed)
    disabled: bool,                  // ¼
    clip: ClipMode,                  // ¼ (2 bits)
    focusable: bool,                 // ¼
    // 3 spare bits
}

// Read by measure/arrange/hash. The fat one.
struct LayoutBox {                   // 56 bytes
    mode: LayoutMode,                // 8
    size: Sizes,                     // 16
    padding: Spacing,                // 16
    margin: Spacing,                 // 16
}

// Read by arrange. Tiny.
type Align = u16;                    // 2 bytes — already its own column-friendly
```

Per-tree SoA columns:

| col            | size | hot for                                   |
| -------------- | ---: | ----------------------------------------- |
| `widget_id`    |    8 | hash_n, cascade, encode, damage, cache key |
| `shape_span`   |    8 | encode, hash_n, leaf-text                 |
| `subtree_end`  |    4 | every walk (skip)                          |
| `flags`        |    4 | cascade hot loop                           |
| `align`        |    2 | arrange                                    |
| `layout_box`   |   56 | measure, arrange, hash_n                   |

Total per record (dense): 82 bytes — slightly more than today's
77 due to `align` being its own field. But **cascade reads
14 bytes (`flags` + `subtree_end` + `widget_id`) instead of 64**.

### 4.3 Cache-miss math

For a 1000-node tree (typical mid-sized UI), one cascade pass:

| | today | proposed | speedup |
| --- | --- | --- | --- |
| bytes read per record | 64 (layout) + 1 (attrs) + 8 (widget_id) + 4 (subtree_end) ≈ 77 | 4 (flags) + 8 (widget_id) + 4 (subtree_end) = 16 | |
| cache lines / record  | 2 (layout) + 1 (others overlap)                                 | 1                                              | |
| total bytes / 1k records | ~77 KB                                                       | ~16 KB                                         | **~5× less** |

Cascade is one of the hottest passes (runs every frame regardless
of cache state). Encoder gets a similar (smaller) win because it
no longer pulls all of `LayoutCore` to read `padding` — it'd read
`layout_box` (still fat) but only when it actually emits text.
For non-text shapes, encoder reads `flags.clip` + `widget_id` +
`shape_span`.

### 4.4 What about measure / arrange?

Measure and arrange read most of `LayoutCore` per record. Today:
one cache line per record. Proposed: `layout_box` is 56 bytes, so
also one cache line per record. **No regression.**

Arrange additionally reads `align` (2 bytes, separate column). One
extra cache line stream — but `align` is 2 bytes per record so a
1k-tree fits in 2 KB and is fully prefetched. Negligible.

### 4.5 Hash compute

`compute_node_hashes` reads everything per record. With 6 dense
columns instead of 5, that's one extra stream — but each stream
is sequential and prefetcher-friendly. Net overhead: zero in
practice (memory bandwidth, not latency, is the bottleneck for
sequential reads).

### 4.6 What stays sparse

`bounds`, `panel`, `chrome` remain `SparseColumn<T>` — most leaves
have neither a transform nor panel-only fields nor chrome. The
existing cold path is correct.

### 4.7 What's added

- **`Forest` struct** (~30 LOC).
- **`NodeFlags` struct + Hash + pack** (~30 LOC).
- **`LayoutBox` struct + Hash** (~25 LOC).
- **Pipeline pass per-layer outer loops** (~15 LOC across 5
  passes).

---

## 5. Combined LOC delta

| change                        |   LOC |
| ----------------------------- | ----: |
| Delete `tree/reorder.rs`     | −280  |
| Delete `id_per_node`         |  −20 |
| Delete `shape_root_id`       |  −15 |
| Delete `RootManifest::sort_*`|  −60 |
| Delete reorder fixup tests   | −150 |
| Delete `assert_recording_invariants` cross-bucket | −30 |
| Add `Forest` struct + dispatch | +50 |
| Add `NodeFlags` + `LayoutBox` split | +55 |
| Per-layer outer loops in passes | +90 |
| Update tests for forest API  | +110 |
| **Net**                      | **−250** |

Plus a 3-5× reduction in cascade-pass cache traffic. (Encoder
gets a smaller win on non-text leaves.)

---

## 6. Migration plan

Three phases. Phase 1 is the topology change; phase 3 is the
field split (gated on phase 2's bench results, per Q1 in §8).

### Phase 1 — topology

`Tree` → per-layer arena. `Forest { trees: [Tree; Layer::COUNT],
recording: RecordingState }`. Drop `reorder.rs`,
`manifest.id_per_node`, `recording.shape_root_id`,
`RootManifest`'s sort scratch. ~−500 LOC. No behavior change
beyond eliminating the reorder pass.

The phase is internally subdivided as proposed in earlier
analysis:

- **1a. Encapsulate.** Pull the v1-shaped `Tree` interior into a
  `TreeInner` sub-struct that doesn't depend on multi-layer
  infrastructure. The current `Tree` becomes a thin wrapper.
  No behavior change.
- **1b. Introduce `Forest`.** Add `Forest { trees: [TreeInner;
  Layer::COUNT], recording: RecordingState }`. Update `Ui` to
  hold `Forest` instead of `Tree`. Pipeline passes adopt the
  per-layer iteration pattern. Recording API (`open_node` etc)
  dispatches via `current_layer`.
- **1c. Delete.** Remove `RootManifest` (replaced by per-tree
  `roots`), `ReorderScratch`, `id_per_node`, `shape_root_id`,
  `reorder.rs`, `assert_recording_invariants`'s cross-bucket
  logic. ~−500 LOC.
- **1d. Per-tree finalize.** Move `compute_node_hashes` /
  `compute_subtree_hashes` / `reset_hashes_for` onto `TreeInner`;
  `Forest::end_frame` loops over trees and calls each
  `tree.end_frame()`.

Each sub-step compiles + tests pass. Reversible at each
checkpoint.

### Phase 2 — bench

Build the cascade microbenchmark from Q2 (§8). Run on synthetic
trees of N ∈ {100, 500, 2000, 10000} nodes. Decide whether to
ship phase 3.

### Phase 3 — field split (gated on Phase 2 results)

Replace `LayoutCore` with:

- `LayoutBox` (56 B, measure-pass density)
- `NodeFlags` (4 B, cascade-pass density)
- `align: Align` (2 B, arrange-pass)

`widget_id` and `flags` stay in separate columns (Q4).
~+55 LOC of struct defs, ~−10 LOC at read sites (clearer field
paths). No behavior change.

The field split is independent and reversible — skip if phase 2
shows cascade isn't hot.

---

## 7. Why this is *simpler*, not just faster

A common worry with cache optimizations is that they trade
clarity for speed. Here, the proposal is *simultaneously simpler
and faster*:

- **Forest deletes one entire concept.** New contributors don't
  have to learn the reorder pass, `id_per_node`, `shape_root_id`,
  the 9-step permutation, or the cross-bucket invariant.
- **Field split groups by purpose.** `NodeFlags` reads as "the
  per-node hit/visit toggles." `LayoutBox` reads as "the things
  the layout solver needs." Today's `LayoutCore` mixes both —
  `visibility` is a flag that travels with `padding` for
  historical reasons, not architectural ones.
- **Sparse extras semantics unchanged.** No new concepts in the
  cold path.

The change is "fewer parts and clearer roles" — the kind of
simplification that pays for itself on the first read by a new
contributor.

---

## 8. Decisions (locked in)

Five questions came up during review; each had A/B/C options
with trade-offs spelled out. Locked-in decisions:

### Q1. Migration sequencing → **B: topology first, field-split second**

Two phases. Phase 1 (forest) is pure cleanup with no behavior
change beyond deleting the reorder pass and its supporting
columns. Phase 2 (field split) is a localized struct refactor
that benches in isolation against the just-stabilized topology.

Each phase reversible independently. Combined commit was
considered but rejected — bigger blast radius, harder to bisect.

### Q2. Bench before field split → **B: yes, bench first**

The cache-miss math in §4.3 is unambiguous in theory, but CPU
prefetcher heroics sometimes erase predicted wins. Before
shipping the field split:

- Build a synthetic tree with N ∈ {100, 500, 2000, 10000} nodes
  using the existing `benches/measure_cache.rs` pattern.
- Run `Cascades::run` 1000× per N, measure ns/run.
- If cascade is <0.1 ms on 2000 nodes, the field split is
  theater — defer indefinitely.
- If cascade is >1 ms on 2000 nodes, ship the split.

~30 min of benchmark scaffolding. Catches the case where the
optimization doesn't matter.

### Q3. `NodeFlags` + `align` column layout → **A: two separate columns**

`flags: Vec<NodeFlags>` (4 B/node, cascade-only) and `align:
Vec<Align>` (2 B/node, arrange-only) stay independent.

Each pass touches the minimum bytes; cascade pulls 4 KB/1k nodes
and arrange pulls 2 KB/1k nodes — every byte useful. The
"merged 6-byte column" alternative wasted 33% of cascade reads
and 66% of arrange reads to save one column declaration. Bit-
packing `align` into spare `NodeFlags` bits was over-engineering
for 2 bytes.

### Q4. `widget_id` placement → **A: keep `widget_id` as its own column**

8-byte dense column. Damage and cache-key lookups read only
`widget_id` and walk a thin 8-byte-per-node stream. Merging into
a 12-byte `RecordHandle { widget_id, flags }` would give cascade
one stream instead of two but waste 33% of damage's reads.
Damage runs every frame; the win there is decisive.

### Q5. Cache scoping → **A: keep global caches in v1**

`MeasureCache` / `EncodeCache` / `ComposeCache` stay as global
hashmaps keyed on `(WidgetId, subtree_hash, available_q)`. Forest
changes nothing on the read (cache-hit) side. The only delta
is the per-frame sweep, which already runs over the global
hashmap; per-layer split would shrink the sweep but not the hit
path. Defer until profiling shows sweep cost.

The `WidgetId`-shared-across-layers case (e.g. a button id used
in both popup and Main) keeps one cache entry rather than two —
small win on memory, no cost on correctness.

### Q6. `Tree::open_node`'s dead `anchor` parameter → **C: doc it; revisit later** (open)

Today's `Tree::open_node(element, chrome, anchor)` consumes
`anchor` only when minting a new `RootSlot` (no parent on the
ancestor stack). For child opens it's discarded. The signature
implies the anchor is always relevant, when really it's
"load-bearing in 5% of calls, dead in 95%."

**Options considered:**

- **A. Split into `open_root` + `open_child`.** Two single-purpose
  methods. `Forest::open_node` dispatches based on
  `tree.open_frames.is_empty()`. Function name conveys what the
  signature does. ~+15 LOC (private `open_inner` helper to dedup
  the shared body).
- **B. Pending-anchor on `Tree`.** `Tree::set_pending_anchor(rect)`
  called from `Forest::push_layer`; `Tree::open_node(element,
  chrome)` reads pending anchor on root mint. Cleanest signature
  but pushes recording-time concern (anchor for next root) into
  per-tree state — a category error since `Forest::recording`
  owns recording-time state.
- **C. Keep current signature, document the wart.** Comment on
  `Tree::open_node` explaining the parameter is consumed only on
  root mints. Zero LOC delta. Preserves the wart's visibility but
  doesn't fix it.

**Decision: C for now**, with the dead-parameter case flagged in
the function's doc comment so future readers don't trip on it.

**Why open**: this is purely an internal-to-`tree/mod.rs` cleanup
— external callers go through `Forest::open_node` and never see
`Tree::open_node` directly. The cost-benefit of A vs C is
small either way; the choice depends on whether `tree/mod.rs`
ever grows enough that the dead parameter is worth the +15 LOC
of split. Revisit when the next non-trivial change to the
recording API comes up. Lean if/when revisited: **A**, because
the function name carrying the meaning is worth the helper.

---

## 9. Recommendation

1. Adopt the **forest topology** (§4.1) as migration phase 1.
   Every reference design with layers (egui, imgui, browser
   layer trees) uses per-layer storage; only Palantir today is
   the outlier with single-tree-plus-reorder.
2. After topology lands and tests are stable, run the cascade
   benchmark (Q2 in §8). If the speedup justifies it, **split
   `LayoutCore`** into `LayoutBox` + `NodeFlags` per Q3 (two
   separate columns) and Q4 (`widget_id` independent). ~+55 LOC
   of struct defs; no behavior change.
3. Keep caches **global** for the initial migration (Q5);
   revisit if profiling shows sweep cost.
4. **Skip** archetype-style migration (Bevy ECS pattern) and
   index-list-per-root — the forest covers their wins without
   the costs.
