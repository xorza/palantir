# Overlay / popup layer — plan

Tooltips, dropdowns, context menus, and modals must (a) draw outside
their declaring parent's clip, (b) draw above all siblings regardless
of pre-order, and (c) hit-test on top. Today's tree is single-rooted,
paint order = pre-order, and clip cascades from ancestors — so
none of (a)/(b)/(c) is reachable through a normal child node.

This doc has two halves: the **checklist** (working punch-list, top)
and the **rationale** (what was rejected and why, bottom).

## Target shape (one-line)

Multi-root arena `Tree` with `Layer`-tagged roots; pipeline passes loop
over `tree.roots` (stable-sorted by layer at `end_frame`).

```rust
enum Layer { Main, Popup, Modal, Tooltip, Debug }   // total order

struct RootSlot {
    first_node:  u32,
    layer:       Layer,
    anchor_rect: Rect,
}

// in Tree:
roots: Vec<RootSlot>,
```

## Checklist

Each slice compiles + ships the showcase.

### 1. `Layer` enum + `RootSlot` storage
- Add `Layer`, `RootSlot`, `tree.roots: Vec<RootSlot>`.
- `tree.begin_frame`: clear `roots`. `tree.end_frame`: push the `Main`
  slot (`first_node = 0`, `Display::logical_rect()`), stable-sort by
  layer (no-op for one root).
- Replace `tree.root()` callers with `for root in &tree.roots { … }`.
  Single root keeps current behavior bit-identical.
- Test: hash-rollup is root-local — record two synthetic roots, assert
  `subtree_hash[root_b]` ignores `root_a`.

### 2. `Ui::layer(layer, anchor, body)`
- Save `tree.current_open`, set to `None`.
- Push deferred `RootSlot { first_node: tree.layout.len() as u32, … }`.
- Run `body`; first `open_node` becomes the new root.
- Restore `current_open`.
- Test: record `Main` + a `Popup` root via `Ui::layer`, assert
  `tree.roots` is sorted `[Main, Popup]` post-`end_frame`.

### 3. Pipeline loop conversion
Convert these entry points to iterate `tree.roots`:
- `LayoutEngine::run` — `run(root.first_node, root.anchor_rect)` per root.
- `Cascades::run` — already loops; just drive from `tree.roots`.
- `Encoder::encode` — one `encode_node` per root, in order.
- HitIndex / Damage — no edit; reverse-iter already gives topmost-first
  once roots are layer-sorted, and damage already takes screen rects.
- Showcase must render identically (single-root path collapses).

### 4. `Popup` widget + showcase tab
- `Popup::anchored_to(rect: Rect, |ui| { … })` with `ClickOutside`
  flag.
- Anchor rect = caller-supplied screen rect (typically last-frame
  `Response.state.rect` of the trigger). One-frame stale on first open
  is acceptable (matches Scroll's wheel-pan posture).
- New `examples/showcase/popup.rs`: dropdown menu, hover tooltip, and
  a confirm modal stub.
- Verifies: clip escape, hit-test priority, draw order across layers.

### 5. `Modal` click-eat leaf
- `Modal::show(|ui| …)` records (a) a full-surface `RoundedRect` with
  `dim_color`, (b) a leaf with `Sense::Click` + empty handler covering
  the same rect to swallow stray clicks.
- `ClickOutside::Dismiss` opt-in (default = block).

### 6. `Tooltip` + follow-cursor
- `Tooltip::for_widget(id)` reads last-frame anchor rect; or
  `.follow_cursor()` reads `ui.input.pointer_pos()` + fixed offset.
- Auto-dismiss when anchor loses hover (`Response.hovered() == false`
  for N frames, or immediate — pick one and pin).

## Invariants to keep

- `tree.layout` stays one flat SoA arena; multi-root only adds the
  `roots` manifest.
- Cache keys unchanged: `(WidgetId, subtree_hash, available_q)`.
- `subtree_hash` rollup terminates at each root's `subtree_end[first_node]`.
- Roots are never nested in `recording_parent` (their first node has no
  parent entry). Existing walks already gate on that.

## Out of scope (queue in `roadmap/`)

- Layout-anchored popups (anchor by `WidgetId`, deferred measure after
  `Main` arrange).
- Animated open/close.
- Submenu chains with arrow-key nav.
- OS-level / multi-window popups.

---

# Rationale

Background notes for re-litigation. Skip on first read.

## What's there today

- **Single-rooted tree.** `Tree::root() -> Option<NodeId>` returns
  `NodeId(0)` (`src/tree/mod.rs:335`). The whole pipeline
  (`LayoutEngine::run`, `Cascades::run`, `Encoder::encode`) takes that
  one root. Multi-root is a delete + loop in each.
- **Pre-order = paint order.** `encode_node` walks
  `tree.children(id)` recursively (`src/renderer/frontend/encoder/mod.rs:251`).
  Later siblings paint over earlier ones. There's no `zIndex` /
  `Order` knob.
- **`Shape::Overlay` is the only "above children" affordance.** Same
  node, post-children, still inside owner clip + untransformed
  (`src/shape.rs:27`, encoder phase 2 at
  `src/renderer/frontend/encoder/mod.rs:259`). Used for scrollbar
  thumbs. **Cannot escape ancestor clip** — it's still under every
  ancestor's `PushClip`. Wrong tool for popups.
- **Cascade composes clip via intersection** (`src/ui/cascade.rs:172`).
  A popup recorded as a normal child of a clipped parent inherits the
  intersection.
- **Caches key on `(WidgetId, subtree_hash, available_q)`** —
  popup must look like any other subtree to encode/measure/compose
  caches; no infra change needed if we keep that invariant.
- **HitIndex is pre-order with reverse-iter** (`src/ui/cascade.rs:86`).
  Topmost-first comes for free if popup nodes land *last* in
  `entries`.

## Reference designs

- **egui** — `Order` enum (Background/Middle/Foreground/Tooltip/Debug)
  with per-layer `PaintList`s drained in z-order at end-of-frame
  (`references/egui.md:46`). Strongest match for our pipeline shape.
- **Clay** — `zIndex` on render commands; sort step before scissor
  grouping.
- **imgui** — window list with explicit topmost-popup gating in
  hit-test.
- **Masonry** (alluded to in `DESIGN.md`) — separate "always on top"
  tree merged into encoder pass.

## Proposed shape: multi-root tree, layered roots

Keep one arena `Tree`, but allow N roots in the storage order, each
tagged with a `Layer`. The pipeline iterates roots in layer order;
within a root, pre-order = paint order as today.

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    Main,       // the normal tree
    Popup,      // dropdowns, context menus, anchored panels
    Modal,      // dims-and-blocks; consumes input under it
    Tooltip,    // ephemeral, never blocks input
    Debug,      // overlays, FPS, dev tooling — top
}
// Total order: Main < Popup < Modal < Tooltip < Debug
```

Storage:

```rust
pub(crate) struct RootSlot {
    pub(crate) first_node: u32,   // index into tree.layout
    pub(crate) layer: Layer,
    pub(crate) anchor_rect: Rect, // surface rect for Main/Modal,
                                  // anchor screen-rect for Popup/Tooltip
}

pub(crate) struct Tree {
    // … existing fields …
    pub(crate) roots: Vec<RootSlot>,
}
```

`tree.layout` stays one flat SoA arena. `subtree_end[i]` still defines
each root's span; the only new invariant is "a root's first node has
no entry in `recording_parent`." Existing walks already respect that
— they read `recording_parent[i]` only when present.

### Recording API

User-facing entry point. Closure body opens with no `current_open`,
so its first `open_node` becomes a new root.

```rust
impl Ui {
    /// Record into a side layer. The closure's first widget becomes
    /// a new root tagged with `layer`, anchored at `anchor`. Nesting
    /// is allowed (a popup can spawn a tooltip); the inner layer
    /// inherits the outer's `current_open = None`.
    pub fn layer<R>(
        &mut self,
        layer: Layer,
        anchor: Rect,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> R { ... }
}

// Sugar widgets:
pub struct Popup    { /* anchor: WidgetId or Rect, alignment, … */ }
pub struct Tooltip  { /* anchor: WidgetId, follow_cursor: bool, … */ }
pub struct Modal    { /* dim_color, click_outside: ClickOutside, … */ }
```

Mechanics inside `Ui::layer`:

1. Save `tree.current_open` and reset to `None`.
2. Push a deferred `RootSlot` (first_node = `tree.layout.len()`,
   layer, anchor).
3. Run `body` — its first `open_node` records as a root, descendants
   nest normally. Each closed child returns to its parent within
   *this* layer's stack.
4. Restore the saved `current_open`. The popup's nodes have been
   appended to the arena; `tree.roots` records the slot.

Result: a single contiguous SoA arena, with `tree.roots` declaring
which spans paint at which layer. Recording cost is one root-slot
push per popup.

### Root order vs layer order

Recording order ≠ layer order (a popup recorded mid-tree must paint
*after* the Main root). Two options:

- **Sort `tree.roots` by layer at `end_frame`** — stable sort, ties
  broken by recording order. Iteration is by `tree.roots`, never
  `first_node`. Cheap: there are ~1–10 roots, never thousands.
- **Layer-bucket scan in each pass** — keep `tree.roots` in record
  order, but pipeline passes loop over layers outer-then inner. More
  cache-friendly since roots in a given layer are usually adjacent.

Pick the sort: simpler invariants, every consumer reads one slice
order. Stable sort guarantees popup-A-then-popup-B (recorded in
that order) paint in that order within the Popup layer.

### Anchor + sizing

Each root carries its anchor `Rect` — the rect `LayoutEngine::run`
will measure/arrange against:

- **Main** — `Display::logical_rect()` (today's behavior).
- **Modal** — `Display::logical_rect()`. The dim is a `RoundedRect`
  the modal pushes itself; click-blocking is the modal's own leaf
  with `Sense::Click`.
- **Popup** — caller-supplied screen rect, typically derived from the
  anchor widget's last-frame rect via `Response.state.rect`. One-
  frame-stale on the first frame the popup opens (the anchor was
  recorded the frame before for an open-on-click flow). Same model
  as Scroll's wheel-pan clamp; matches existing posture.
- **Tooltip** — same as Popup; `follow_cursor: true` uses
  `ui.input.pointer_pos()` + a fixed offset.

Layout-anchored popups (pull anchor rect from
`LayoutResult.rect[anchor_node]` so the same-frame anchor is usable)
is a follow-up — needs `Ui::layer` to defer measure until *after*
the Main root is arranged. v1 = caller-supplied screen rect.

### Pipeline changes per pass

| pass            | today                                     | with multi-root                                                        |
| --------------- | ----------------------------------------- | ---------------------------------------------------------------------- |
| Record          | one open stack                            | `Ui::layer` saves/restores `current_open`, appends a `RootSlot`        |
| `tree.end_frame`| rolls subtree_end, hashes, etc.           | also `roots.sort_by_key(\|r\| r.layer)`                                |
| `LayoutEngine::run`| takes one root + surface                  | loops over `tree.roots`, `run(root, root.anchor_rect)` each            |
| `Cascades::run` | one walk; ancestor stack reset on roots   | already loops `0..n`; just keep ancestor stack per-root (it does)      |
| `Encoder::encode`| one `encode_node(root)`                   | loops over `tree.roots` in order; each call is independent             |
| `Composer`      | reads `RenderCmdBuffer` linearly          | unchanged — the cmd stream already serializes the roots back-to-back   |
| HitIndex        | reverse-iter `entries`                    | unchanged — popup entries appended last → topmost-first                |
| Damage          | screen-rect diff                          | unchanged — cascade emits screen_rect for popup nodes too              |

The encoder and cascade already start from "no parent state" at the
root. The only real edit is replacing `if let Some(root) = tree.root()
{ run(root, …) }` with `for root in &tree.roots { run(root.first_node,
root.anchor_rect, …) }` in the few entry points.

### Caches stay correct

Encode/measure/compose caches key on
`(WidgetId, subtree_hash, available_q)` and walk a subtree
independently of where it sits in the arena. Popups become extra
subtree roots; the cache hits/misses on each independently. The same
`SeenIds.removed()` sweep evicts a popup's `WidgetId`s when the
popup closes, no special path.

The one thing to verify: popup's `subtree_hash` rollup must not
accidentally fold the Main tree's hash. Today the rollup walks
`subtree_end[i]` via `i+1` advance with a `next < end` guard
(`src/tree/mod.rs:151`) — already root-local. Multi-root tree:
each root's rollup terminates at its own `subtree_end[first_node]`,
unaffected by sibling roots. Pin with a unit test.

### Hit-test ordering

`Cascades` builds `entries` in storage order. Reverse iter gives
topmost-first. With sorted roots, storage order = layer order, so
Tooltip beats Popup beats Main. The existing
`hit_test`/`hit_test_focusable` calls work unchanged.

**Modal click-blocking**: the modal layer's own leaf widget covers
the surface with `Sense::Click` and an empty handler — eats clicks
that would otherwise fall through. No special infrastructure.

### Input dismissal

Popups close on:

- **Escape** while a child has focus — handled by the popup widget
  via `frame_keys`.
- **Click outside** — popup widget reads
  `ui.input.last_press_pos()` and checks against its own arranged
  rect; if outside and `ClickOutside::Dismiss`, the host's open
  flag flips. The popup has hit-test priority *over* the Main
  tree, so a click on the popup itself fires its own click handlers
  first — but when the click misses the popup, the Main-layer
  hit-test still runs. Acceptable: most native dropdowns do the
  same (clicking another menu item dismisses the open one and
  selects the new one in one click).

For modals, click-outside-to-dismiss is opt-in (a confirmation
modal usually shouldn't); the dim leaf consumes clicks by default.

## Why multi-root over `Order`-on-Element

Considered: tag every `Element` with a `Layer`, walk the existing
single tree, sort emitted cmds by layer at compose. Rejected:

- Clip cascade still composes ancestors — a popup nested inside a
  clipped panel inherits the clip, defeating (a). Splitting the
  cascade per layer-of-self breaks the "one ancestor stack" model
  in `Cascades::run`.
- The encode cache keys on `subtree_hash`, not on layer; if layer
  affected paint order without affecting the hash, a layer flip
  would silently replay the wrong order. Folding layer into the
  hash works but invalidates more aggressively than needed.
- Multi-root doesn't break any existing invariant: each root is
  just an independent subtree the caches/cascade/encoder already
  handle.

## Why not separate per-layer `Tree`s

Considered: a `Vec<Tree>` keyed by `Layer`. Rejected: forces every
shared structure (caches, hashes, scratch) to grow a layer
dimension. Multi-root inside one arena keeps all per-frame
machinery flat — one `subtree_end`, one `hashes`, one `roots`
manifest.
