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

### 1. `Layer` enum + `RootSlot` storage  ✅ shipped
- Add `Layer`, `RootSlot`, `tree.roots: Vec<RootSlot>`.
- `tree.begin_frame`: clear `roots`. `open_node` lazy-pushes a `Main`
  slot whenever it opens a node with empty `open_frames` (`anchor_rect`
  placeholder, patched at `end_frame`). `tree.end_frame(main_anchor:
  Rect)` patches every `Main` slot's anchor and stable-sorts by layer.
  Lazy push keeps the manifest in lockstep with records — no "root
  without a node" window — and slots in step 2's `Ui::layer` push as a
  per-layer override.
- `Tree::root()` deleted. Encoder iterates `tree.roots`; `Ui` reads
  `roots.first()` for the single-root layout call (step 3 plumbs the
  loop into `LayoutEngine::run` itself).
- Debug invariant: registered roots cover `records` and `shapes`
  contiguously starting at 0 — no orphan top-level nodes.
- Test: hash-rollup is root-local — two top-level subtrees, vary one,
  pin the other's `subtree_hash` is unchanged.

### 2. `Ui::layer(layer, anchor, body)`  ✅ shipped (v1, top-level only)
- Recording state on `Tree`:
  - `open_frames: [Vec<NodeId>; Layer::COUNT]` — per-layer ancestor
    stack. Only one layer is active at a time during recording; others
    sit empty between scopes.
  - `layer_anchor: [Rect; Layer::COUNT]` — anchor per layer, set on
    `Ui::layer` entry; `Main`'s slot is patched in `end_frame`.
  - `current_layer: Layer` — active layer for the next `open_node`.
- `Ui::layer` calls `Tree::push_layer(layer, anchor)` which enforces
  v1's top-level rule with two asserts: `current_layer == Main` and
  `open_frames[Main].is_empty()`. Body runs with `current_layer = layer`.
  `pop_layer` restores `current_layer = Main` after asserting the
  popup body closed every node it opened.
- `Layer::COUNT` comes from `strum::EnumCount` derive on the enum;
  `#[repr(u8)]` + sequential `0..5` discriminants make `layer as usize`
  a valid array index.
- Tests: `Main` + `Popup` recorded top-level → `tree.roots` sorted
  `[Main, Popup]`, `records.end()` ranges abut, anchor passes through;
  illegal mid-`Panel::show` call panics with a clear message.

### 3. Pipeline loop conversion  ✅ shipped
- `Encoder::encode` — `src/renderer/frontend/encoder/mod.rs:86` loops
  `for root in &tree.roots { encode_node(...) }`.
- `Cascades::run` — walks `0..tree.records.len()` in storage order
  (`src/ui/cascade.rs:141`); multi-root falls out for free since each
  root's first node has no parent on the ancestor stack.
- HitIndex / Damage — no edit needed; reverse-iter gives topmost-first
  once roots are layer-sorted, and damage works on screen rects.
- `LayoutEngine::run` — loops `tree.roots`, calling measure+arrange per
  root against `slot.anchor_rect` (`src/layout/mod.rs:184`). Empty
  `roots` is the no-widgets-recorded path (zero-sized result).
- `Ui::end_frame` — single-root pluck deleted; `layout.run` is called
  with just `(&tree, &mut text)`.

### 4. `Popup` widget + showcase tab  ⏳ partial (dropdown only)
- `Popup::anchored_to(rect: Rect)` with `.background`, `.padding`,
  `.click_outside(...)` builders; `.show(ui, body)` records into
  `Layer::Popup` via `Ui::layer`. Body is wrapped in an internal
  `Panel::vstack` so the layer's first node opens unconditionally.
  See `src/widgets/popup.rs`.
- Anchor rect = caller-supplied screen rect; first frame after open
  is one frame stale (matches Scroll's wheel-pan posture).
- `ClickOutside::{Block, Dismiss}` enum is exposed but the dismissal
  signal is not yet plumbed through `Response`; both modes currently
  behave like `Block`. Wire dismissal alongside step 5/6.
- `examples/showcase/popup.rs`: dropdown menu (button toggles
  `MenuState.open`; popup hosts three menu-item buttons; selecting
  one writes `last_choice` and closes the popup).
- Tooltip + confirm-modal stubs deferred to steps 5/6 — they need
  `Modal::show` + `Tooltip::for_widget` rather than reusing `Popup`.
- Verifies (by inspection in showcase): clip escape (popup paints
  outside the central tab panel's clip), hit-test priority (clicks on
  popup items beat clicks on the trigger row underneath), draw order
  (popup paints over the toolbar).

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
  `NodeId(0)` (`src/tree/mod.rs:346`). The whole pipeline
  (`LayoutEngine::run`, `Cascades::run`, `Encoder::encode`) takes that
  one root. Multi-root is a delete + loop in each.
- **Pre-order = paint order.** `encode_node` walks
  `tree.tree_items(id)` interleaving direct shapes with child recursion
  (`src/renderer/frontend/encoder/mod.rs:348`). Later siblings paint
  over earlier ones. There's no `zIndex` / `Order` knob.
- **Sub-rect shapes interleave with children, but stay under ancestor
  clip.** `Shape::RoundedRect { local_rect: Some(_) }` and
  `Shape::Text { local_rect: Some(_) }` paint at owner-relative coords
  in the slot they were pushed in (`src/shape.rs:11-19`). Used for
  scrollbar tracks/thumbs and TextEdit carets. The slot mechanism
  controls *paint order within the owner*, not clip — every interleaved
  shape is still under every ancestor's `PushClip`. Wrong tool for
  popups.
- **Cascade composes clip via intersection** (`src/ui/cascade.rs:172`).
  A popup recorded as a normal child of a clipped parent inherits the
  intersection.
- **Caches key on `(WidgetId, subtree_hash, available_q)`** —
  popup must look like any other subtree to encode/measure/compose
  caches; no infra change needed if we keep that invariant.
- **HitIndex is pre-order with reverse-iter** (`src/ui/cascade.rs:87`).
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
    pub(crate) first_node: u32,   // index into tree.records
    pub(crate) layer: Layer,
    pub(crate) anchor_rect: Rect, // surface rect for Main/Modal,
                                  // anchor screen-rect for Popup/Tooltip
}

pub(crate) struct Tree {
    // … existing fields …
    pub(crate) roots: Vec<RootSlot>,
}
```

`tree.records` stays one flat SoA arena. `records.end()[i]` still
defines each root's span; the only new invariant is "a root's first
node is opened with `open_frames` empty," which is automatic if
`Ui::layer` saves/clears `open_frames` around the body.

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
2. Push a deferred `RootSlot` (first_node = `tree.records.len()`,
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
| `tree.end_frame`| rolls `records.end()`, hashes, etc.       | takes `main_anchor: Rect`, pushes `Main` slot, sorts `roots`           |
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
`records.end()[i]` via `i+1` advance with a `next < end` guard
(`src/tree/mod.rs:198-202`) — already root-local. Multi-root tree:
each root's rollup terminates at its own `records.end()[first_node]`,
unaffected by sibling roots. Pin with a unit test.

### Mid-recording layer changes (v2 deferral)

**Status today:** `Ui::layer` asserts top-level recording (no node
currently open). The natural user pattern — opening a popup from
inside a `Panel::show` callback when a button is clicked — is forbidden
in v1.

**Why it's hard.** The arena depends on pre-order subtree contiguity:
`tree.children(parent)` walks `next..end[parent]` and skipping over
foreign records is a load-bearing assumption shared by every walk
(measure, arrange, cascade, encode, hash rollup). Recording a popup
mid-`Panel::show` interleaves the popup's nodes between the panel's
own children, splitting the panel's subtree across non-adjacent
index ranges. Main's `end[0]` ends up over-spanning the popup, so
`tree.children(panel)` would visit the popup's nodes as if they were
panel children.

**Required user pattern in v1:**

```rust
let mut menu_open = false;
let mut anchor_rect = Rect::ZERO;
Panel::vstack().show(&mut ui, |ui| {
    let r = Button::new("menu").show(ui);
    if r.clicked() { menu_open = true; }
    anchor_rect = r.state.rect;
});
if menu_open {
    ui.layer(Layer::Popup, anchor_rect, |ui| { ... });
}
```

Workable, but loses the locality of "open the popup right where I
clicked."

**Solution space (v2, when motivated):**

| approach              | mechanic                                                                                       | invasiveness                                                                                           | user ergonomics                                              |
| --------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------ |
| **Closure queue**     | `Ui::layer` boxes the body into `Vec<Box<dyn FnOnce(&mut Ui) + 'static>>`; drained at frame end | low — one Vec, drain step in `end_frame`                                                               | bad — body must be `'static` (no stack borrows; Rc/Cell only) |
| **Staging arena**     | popup body records into a parallel SoA buffer; spliced into `records` at end of outer scope    | high — every record-side write needs to know which buffer (`records`, `shapes`, `bounds`, `panel`, `chrome`, `subtree_has_grid`, `widget_id`s, etc.) | best — body works exactly like inline                        |
| **End-frame reorder** | record interleaved as today; reorder records after `close_node` for outer scope so subtrees become contiguous; fix up `end[]` / `shapes[]` / `roots.first_node` / sparse columns | moderate — single fixup pass walks records once, rewrites indices                                      | best — body works exactly like inline                        |

The closure queue is cheapest to ship but most restrictive: closures
that capture `&mut some_local_state` won't compile, forcing users to
thread state through `Rc<RefCell<…>>` or `Ui::state`. egui takes this
hit; it's livable but rough.

The staging arena is cleanest for users but is a major plumbing
change — every `tree.records.push(…)` / `tree.add_shape(…)` /
sparse-column write needs an explicit target buffer. Roughly 30–50
call sites.

The end-frame reorder is the favored path: recording stays inline (no
API change in the body), the fixup is concentrated in one place
(`tree::end_frame` or close to it), and the cost is O(records) once
per frame — comparable to existing `compute_subtree_hashes`. The fixup
pass is mechanical: identify each popup's contiguous block, move it to
the end of `records`, rewrite `end[]` for the surrounding parent to
exclude the popup's old range, rewrite affected shape/bounds/panel
indices, rewrite `roots[i].first_node`. `WidgetId`s are stable. The
hash pass would run after the reorder.

**Status:** the showcase popup tab (`examples/showcase/popup.rs`)
records the popup inline from inside the central `Panel::show` body,
which trips v1's top-level assert and panics on the trigger click.
It's a pinned regression target for v2; expand the awkward two-phase
workaround only if v2 stalls. The design below replaces the original
"motivated when…" recommendation — we're motivated now.

### v2 plan: end-frame reorder (concrete)

#### Recording state with mid-recording layers

After Main recording completes, with one mid-recording popup opened
inside `Panel::show`:

```
records: [parent, mc1, mc2, popup_root, ps1, ps2, mc3, mc4]
ends:    [parent → past mc4,
          mc1 → mc2, mc2 → popup_root, popup_root → mc3, ps1, ps2,
          mc3 → mc4, mc4 → past mc4]
roots:   [(first_node=0,   layer=Main,  …),
          (first_node=3,   layer=Popup, …)]
shapes:  interleaved similarly — popup's shape range sits between
         mc2's and mc3's in the flat buffer.
```

Two breakages versus v1 invariants:

1. `tree.children(parent)` walks `next..ends[parent]` stepping by
   `ends[next]`. With popup interleaved, it yields popup_root as a
   spurious child of `parent`. Same problem in `subtree_hash` rollup
   (`src/tree/mod.rs:296`), `Cascades::run` (`src/ui/cascade.rs:141`,
   walks `0..n` linearly assuming pre-order = storage order),
   `LayoutEngine::run` per-root entry points, the encoder's
   `encode_node` (`src/renderer/frontend/encoder/mod.rs:176`), grid
   measure/arrange (`src/layout/grid/mod.rs:344`, `:548`), and every
   `for i in 0..n` linear scan in damage / measure-cache / encode
   cache.
2. `assert_recording_invariants` (`src/tree/mod.rs:320`) requires
   roots to abut in record order with no gaps — popup at index 3
   leaves a gap in Main's coverage [0, 3) and [6, 8).

The fix: rearrange `records` (and the columns indexed by `NodeId`)
so each root's records are contiguous in storage order and roots
appear in layer order. Once that's true, every existing walk works
unchanged and `0..n` regains its pre-order = storage-order property.

#### Per-record root tag

Add a `root_id: u16` parallel column on `Tree`, indexed by `NodeId.0`:

- Stamped at `open_node` from a new recording-only field
  `Tree::current_root_id: u16`.
- `current_root_id` is set on each `RootSlot` push (in `open_node`
  when `open_frames[layer]` is empty) — the root's own index in
  `tree.roots` at push time. Saved/restored by `push_layer` /
  `pop_layer` so a nested popup's root_id reverts to the outer
  scope's on close.
- `u16` matches the practical roots-per-frame ceiling (already
  bounded by `Layer::COUNT × open scopes`); promote to `u32` if a
  workload ever proves us wrong.

`root_id` is _not_ a third hash input or a paint-time attribute —
it's recording-only scratch. `compute_subtree_hashes` runs after the
reorder, so hashes stay invariant of recording order.

#### Reorder algorithm (in `Tree::end_frame`, before hashes)

Inputs at entry: `records` (length N), `shapes` (length S), sparse
columns, `roots` (sorted by record order), `root_id[0..N]`.

1. **Stable-sort `roots` by layer.** Already done; keep position.
2. **Build `record_perm: Vec<u32>` of length N.** Walk
   `roots` in layer-sorted order. For each root R, scan
   `0..N` and append every `i` where `root_id[i] == R.idx_in_roots`
   to `record_perm`. Linear in N per root, ≤ small-constant roots,
   so O(N · roots) ≈ O(N).
3. **Build `inv_perm: Vec<u32>`** so `inv_perm[old_i] = new_i`.
4. **Permute `records` (SoA columns).** For each old-`NodeRecord`
   field (`widget_id`, `shapes`, `end`, `layout`, `attrs`), apply
   the permutation: `new[inv_perm[old_i]] = old[old_i]`. soa-rs
   doesn't ship a permute; do per-column with a scratch buffer
   capacity-retained on `Tree`.
5. **Rewrite `end[]`** post-permute: for each new record at position
   `i`, `new_end[i] = inv_perm[old_end[i] - 1] + 1` only if every
   old NodeId in `[i, old_end[i])` maps to a contiguous new range
   `[i, new_end[i])`. The compaction-by-bucket guarantees this for
   the records of a single root; and since each root's records sit
   contiguously in the new order, descendants land contiguously.
   Concretely: `new_end[i] = i + (old_end[old_i] - old_i)` — the
   subtree length is preserved, only the base shifts.
6. **Permute the shape buffer.** Walk records in new order; for
   each new record `i` (= old record `old_i`), copy
   `old_shapes[old_shapes_span.start..old_shapes_span.start+len]`
   into `new_shapes`. Track the new start, write back into the
   record's `shapes.start`. `len` carries over unchanged. One pass,
   O(S).
7. **Permute sparse side columns** (`bounds`, `panel`, `chrome`)
   and the `subtree_has_grid` bitset by the same `inv_perm`.
   Sparse columns expose `clear`/`push`/`get`; rebuild via
   `clear()` + push in new order.
8. **Rewrite `roots[i].first_node`** to its new position
   (`inv_perm[old_first_node]`).
9. **Drop `root_id`** for the frame (or leave it in place as
   debug-only — re-stamped each `begin_frame`).

After step 9 the tree looks identical to what v1 produces for the
same final structure, so `compute_node_hashes` /
`compute_subtree_hashes` and every downstream pass run unchanged.

#### Edge cases

- **Empty popup body.** `ui.layer(Popup, anchor, |_| {})` records no
  nodes → no `RootSlot` pushed. The popup contributes zero records
  to the reorder; `roots` only has Main. Same path as v1's "popup
  was conditionally not opened."
- **Nested popups.** A popup body that opens its own popup pushes a
  third `RootSlot`. `current_root_id` save/restore on `push_layer`
  guarantees each record gets the right tag. Reorder treats the
  inner popup as a separate bucket; layer-order sort places it
  after the outer popup. _Pin with a unit test._
- **Popup recorded but never opened a node.** A `Ui::layer` scope
  with no `open_node` inside the body never pushes a `RootSlot` — no
  bucket, nothing to reorder. The empty layer scope is silent.
- **`Main` as the only recorded layer.** `record_perm` becomes the
  identity (one bucket covering all records in storage order).
  The permutation pass should fast-path this — bail out if `roots`
  has only the implicit Main slot.
- **Shape order across siblings.** Each record's `shapes.start /
  len` covers parent + descendants. Within a root's bucket, shapes
  stay in old relative order (we walk old records in old order
  inside each bucket). Across roots, popups' shapes move to the
  end of `shapes`. `TreeItems` walks by `shapes.start` per record
  — unaffected.
- **Empty Main with popup-only frame.** A frame that records only a
  popup (no Main root) still produces `roots = [popup_slot]`. Main
  bucket is empty; popup bucket fills `record_perm` directly. The
  empty-Main `assert_recording_invariants` path needs to allow a
  zero-length Main bucket at the front (or skip the implicit-Main
  push if no Main records existed).

#### Cost

- One scratch `Vec<u32>` of length N for `record_perm` and one for
  `inv_perm` (capacity-retained on `Tree`).
- One scratch buffer per SoA column (or one combined `Vec<NodeRecord>`
  if soa-rs supports rebuild from a slice — TBD).
- One scratch `Vec<Shape>` for the shape permutation.
- O(N) work per record + O(S) for shapes + O(N) per sparse column.
  Comparable to `compute_subtree_hashes`; should fit in the same
  per-frame budget without showing up in a profile.

#### Open questions

1. **soa-rs permutation primitive.** Does soa-rs offer in-place
   permute, or do we have to drain into per-column scratch and rebuild?
   Affects step 4. If neither, the pragmatic path is a temporary
   `Vec<NodeRecord>`, one rebuild per frame.
2. **Sparse-column rebuild cost.** `bounds`/`panel`/`chrome` are
   sparse — most leaves leave them default. Permuting via `clear()` +
   push in new order is O(populated entries), cheaper than O(N). Pin
   with a microbench before optimizing further.
3. **`subtree_has_grid` bitset permutation.** `FixedBitSet` doesn't
   permute in place. Either rebuild after compute_subtree_hashes
   (which already walks the new layout) or accept an extra O(N)
   bit-shuffle pass.
4. **Should the reorder run only when a non-Main root exists?** Yes
   — the common case is Main-only and pays the no-op fast-path. Gate
   the pass on `roots.len() > 1 || roots[0].layer != Main`.
5. **Debug invariant updates.** `assert_recording_invariants` runs
   _before_ the layer sort today (`src/tree/mod.rs:244`). After v2
   it needs to assert post-reorder — roots cover records contiguously
   in layer order. Worth adding a pre-reorder invariant too: each
   bucket (per root_id) is contiguous in record-order pre-order
   (i.e. root_id only changes when a new root opens or a layer pop
   returns to its parent's tag).

#### Test plan

Mirrors the `tree/tests.rs` fixtures already in place for v1 multi-
root (top-level Main + Popup). Add fixtures that:

- record `Main { mc1, mc2, Popup { ps1, ps2 }, mc3, mc4 }` and
  assert post-`end_frame` storage order is `[parent, mc1, mc2, mc3,
  mc4, popup_root, ps1, ps2]`.
- assert `tree.children(parent)` yields exactly `[mc1, mc2, mc3,
  mc4]` — no popup leak.
- assert `subtree_hash[parent]` is invariant across "popup body
  varies" perturbations (rollup terminates at `parent.end` which now
  excludes popup).
- record nested popups (`Main { Popup { Popup { … } } }`) and pin
  the three-bucket layout.
- pin per-frame `Main`-only fast path: with no `ui.layer` calls,
  `record_perm` is identity and `records` storage is bit-identical
  to a v1 run.
- repeat the existing showcase scenario (button click → mid-record
  popup) end-to-end through `Ui::end_frame` to confirm the showcase
  panic is gone.

#### Out of scope for v2

- Cross-frame reorder caching (the reorder is cheap; redo each frame).
- Reorder during recording (eager-at-`pop_layer`). Lazy at `end_frame`
  is simpler and matches the "one fixup pass" doc target.
- Restructuring `Cascades::run` to walk per-root explicitly. With
  the reorder in place, `0..n` storage-order remains valid pre-order;
  no change needed.
- `Ui::layer` from inside a leaf widget. Leaves don't run a `body`
  closure today — there's no recording window in which to call
  `ui.layer`. Not blocked, just unreachable.

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
