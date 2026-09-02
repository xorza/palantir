# Docking

Promoting Darkroom's dock and tab strip into Palantir as three widgets:
`TabStrip`, `TabbedView`, `DockView`.

This is a plan, not a survey. It names the API, the theme surface, the
decisions and their costs, and the order to build in.

## 1. What Darkroom already has

The dock is split cleanly in two. `core::document::dock` is the persisted
model — pure data and pure ops, no `Ui`. `gui::dock` is the view: a two-call
surface (`scan` in the navigation phase, `render` in the record) that never
learns what a pane contains, because the caller passes a `content` closure.

That boundary is why this is a promotion and not a rewrite. The only
Darkroom-shaped things in the whole tree are the `TabRef` enum, the `Requests`
sink, and the `Theme` struct.

| File | Lines | Holds | Fate |
|---|---:|---|---|
| `core/document/dock/mod.rs` | 550 | Flat pre-order split tree, the six `DockOp` appliers, `normalize`, `validate` | move |
| `core/document/dock/tests.rs` | 686 | Tree invariants, move / close / split coverage | move |
| `core/document/dock/*.rs` | 214 | `DockOp`, `DockPath`, `SplitSide`, `TabGroup`, errors | move |
| `gui/dock/mod.rs` | 443 | Split walk, pane focus, drag lifecycle, drop feedback | move |
| `gui/dock/strip.rs` | 377 | Chip row, accent cap, close button, dirty dot, split menu | move |
| `gui/dock/drag/*.rs` | 292 | `classify_drop` rect maths and its tests | move |

Total: 2 562 lines.

### What is genuinely missing

- **Overflow.** The strip is a plain `Panel::hstack`. Ten tabs in a narrow
  pane overflow with no scroll, no chevrons, and no overflow menu.
- **Keyboard.** No `Ctrl+Tab`, no arrow-key travel along the strip, no
  `Home` / `End`. The WAI-ARIA tab pattern asks for all three.
- **Styling.** Every colour, inset, and radius reads from Darkroom's own
  `Theme` or sits as a literal in `strip.rs`. There is no bundle to override.
- **A standalone tab bar.** A tab strip is useful without a dock. Today it
  cannot be reached without a `TabGroup`, a `Document`, and a `Requests`.

## 2. What the field does

Five libraries, two shapes. Everyone stores a tree of containers whose leaves
are tab groups. They differ on who owns the tab payload, and on how the
application changes the look.

| Library | Tree | Tab payload | Application hook | Worth taking |
|---|---|---|---|---|
| egui_dock | Surfaces → binary `Tree` of nodes | Stores the tab value itself | `TabViewer` trait: 3 required, ~15 defaulted | `AllowedSplits`; per-tab closability; floating windows as a second surface kind |
| egui_tiles | `Tiles` map + n-ary `Linear`, `Grid`, `Tabs` | Stores the pane value | `Behavior` trait: 2 required, ~35 defaulted | The breadth of the defaulted hooks; `simplification_options`; a trailing slot in the tab bar |
| Dear ImGui | `DockNode` tree per `DockSpace` | Window names | Flags plus the `DockBuilder` API | The *central node* — one node that never disappears, so the space is never empty |
| dockview | Grid of split views, groups as leaves | Framework components | Events plus an imperative API object | Layout serialisation as a first-class published contract |
| Qt | Four dock areas around a centre; `QTabWidget` is separate | `QWidget` | Subclassing and style sheets | The tab widget stands alone from the dock — two products, not one |

Two conclusions.

**Every mature docker converged on a trait with many defaulted methods.** Both
Rust crates did, and both required exactly the two things a closure cannot
carry: what a tab is called, and what it draws. That is the flexibility
surface, and a pile of builder closures is a worse spelling of it.

**Qt is right to separate the tab widget from the dock.** Most callers want a
tab bar. A dock is the rarer, heavier thing built beside one.

## 3. Three widgets, one shared strip

`TabStrip` is the shared widget. `TabbedView` and `DockView` are two peers
built on it — not a stack.

```
                 ┌─────────────┐
                 │  TabStrip   │  chips · cap · close · badge · overflow · drag
                 └──────┬──────┘
              ┌─────────┴─────────┐
     ┌────────┴────────┐  ┌───────┴────────────────────────┐
     │   TabbedView    │  │            DockView            │
     │ strip + content │  │ split tree · Splitter · docking │
     │ binds &mut usize│  │ leaf = strip + content, per group│
     └─────────────────┘  └────────────────────────────────┘
```

**The dock does not reimplement tabs.** It records the same `TabStrip`, the
same `TabsTheme`, the same chip ids and overflow behaviour.

What the two do *not* share is the content container and the state binding:

| | `TabbedView` | `DockView` leaf |
|---|---|---|
| Binding | mutates `&mut usize` | mutates nothing, emits `DockOp` |
| Chip key | the index | a hash of the tab `T` |
| Content id | keyed by the view | keyed by the **group** |

The last row is load-bearing. Darkroom keys the content area by group so that
switching tabs leaves the container in place, which is what lets a view that
first records on this pass still be handed its arranged size. A `TabbedView`
cannot supply that key, because it has no group.

The content area is a `Panel::vstack` with a stable id — about fifteen lines.
Sharing it would need an id-strategy parameter and a no-mutate mode on
`TabbedView`, which is a parallel abstraction, not reuse.

### Who owns what

- **`TabStrip`** — the chip row alone. Chip geometry, the selection cap, the
  close button, the badge slot, overflow, and drag sensing. Draws nothing
  below itself.
- **`TabbedView`** — a strip over a content area, bound to a `&mut usize`.
  Mirrors `ComboBox::new(&mut selected, &options)` exactly.
- **`DockView`** — the split tree. Walks nodes onto `Splitter`s, records each
  leaf as a strip over a group-keyed content area, and owns the drag-docking
  gesture.

Each layer emits a different vocabulary: `TabStripResponse` (clicked, closed,
drag edges), `TabsAction` (activated, closed, reordered), and
`Vec<DockOp<T>>` (the caller applies these).

## 4. The surface

### The simple case

```rust
let mut page = 0usize;
TabbedView::new(&mut page, &["Colour", "Geometry", "Metadata"])
    .closable(false)
    .overflow(TabOverflow::Scroll)
    .show(ui, |ui, page| match page {
        0 => colour_page(ui),
        1 => geometry_page(ui),
        _ => metadata_page(ui),
    });
```

No trait, no ops, no tree. The value binding and the `&[S: AsRef<str>]` option
slice come straight from `ComboBox`, so the two widgets read the same at a
call site.

### The strip on its own

```rust
pub struct TabItem {
    pub key: u64,
    pub label: InternedStr,
    pub closable: bool,
    pub badge: bool,
    pub icon: Option<IconHandle>,
}

TabStrip::new(&items).selected(active).focused(has_focus).show(ui)
```

Chip ids derive from `key`, never from the strip slot: the scan reads *last*
frame's response, and an undo can have rearranged the strip since, so a
slot-keyed id would hand one chip's click to another tab.

### The dock

```rust
// The caller implements this — the GpuView / GpuPaint arrangement.
pub trait DockTabs {
    type Tab: Copy + Eq + Hash + Debug;

    fn title(&mut self, ui: &mut Ui, tab: Self::Tab) -> InternedStr;
    fn content(&mut self, ui: &mut Ui, tab: Self::Tab, size: Option<Vec2>);

    fn closable(&mut self, _tab: Self::Tab) -> bool { true }
    fn draggable(&mut self, _tab: Self::Tab) -> bool { true }
    fn badge(&mut self, _tab: Self::Tab) -> bool { false }
    fn icon(&mut self, _tab: Self::Tab) -> Option<IconHandle> { None }
    fn tab_menu(&mut self, _ui: &mut Ui, _tab: Self::Tab) {}
    fn look(&mut self, _tab: Self::Tab) -> Option<&WidgetLook> { None }
}
```

```rust
// Once, at start-up. `TabRef::Graph` is the pinned tab: it never closes,
// so the tree is never empty. The seed scopes every derived widget id.
let mut dock = DockState::new("darkroom.dock", TabRef::Graph);

// Every frame, in three calls.
dock.scan(ui, &mut ops);              // reads last frame, emits ops
requests.extend_view(ops.drain(..));  // the caller's own pipeline
DockView::new(&dock, &mut ops)
    .min_pane(220.0)
    .max_depth(4)
    .allowed_splits(AllowedSplits::All)
    .show(ui, &mut tabs);
```

For an application with no undo layer and no validation,
`DockView::run(ui, &mut dock, &mut tabs)` does all three in one call.

### The model

```rust
pub enum DockOp<T> {
    ActivateTab { tab: T },
    OpenTab     { tab: T },
    CloseTab    { tab: T },
    MoveTab     { tab: T, to: DockDrop },
    SetRatio    { split: DockPath, ratio: f32 },
    FocusPane   { group: TabGroupId },
}

impl<T: DockTab> DockState<T> {
    pub fn new(seed: impl Hash, pinned: T) -> Self;
    pub fn apply(&mut self, op: DockOp<T>);
    pub fn scan(&self, ui: &mut Ui, ops: &mut Vec<DockOp<T>>);
    pub fn groups(&self) -> impl Iterator<Item = &TabGroup<T>>;
    pub fn active_tabs(&self) -> impl Iterator<Item = T> + '_;
    pub fn find_tab(&self, tab: T) -> Option<TabAddress>;
    pub fn retain_tabs(&mut self, keep: impl FnMut(T) -> bool);
    pub fn validate(&self) -> Result<(), DockError>;
}
```

Every op tolerates a stale address, because one is built from a response of the
frame before and applied a phase later. The op vocabulary carries over from
Darkroom unchanged.

## 5. Two phases, and one frame of lag

Palantir's record pass cannot see this frame's layout, so `Ui::response_for`
answers with the previous frame's rects and interaction. For hit-testing that
is correct — the user pressed on what they saw. For the tree walk it is not:
if the widget learns of a tab click *during* the record, the pane it draws is
the one the click replaced.

Darkroom already solved this by splitting the surface in two. `scan` runs in
the navigation phase, the document applies the op, and only then does the
record walk run. Promoting the dock means promoting that discipline, not
hiding it.

```
proposed   frame N−1 responses
                    ↓
           [ scan ] ──ops──> [ apply ] ──new tree──> [ record ] ──> frame N shows the switch

rejected   [ show: scan then record ] ──ops──> [ apply ] ──> frame N shows the old tree
                                                    └── one frame late ──> frame N+1 catches up
```

The whole cost of the two-call surface is one extra line at the call site. The
whole cost of hiding it is that every tab switch, close, and drop lands a frame
after the click.

### Where a drop lands

The classification is pure rect maths over last frame's pane rects, which is
exactly right: panes hold still while a tab is dragged, so the picture the user
drops onto is the picture the maths ran against.

- **Strip band** — an insertion slot between chips: `Into { index }`.
- **Inner 50% per axis** — join the group: `Into { index: len }`.
- **Four outer wedges**, bounded by the content rect's diagonals — split toward
  the nearest edge, compared on normalised distance so a wide pane does not
  bias toward top and bottom.
- **At the nesting cap** every wedge degrades to a join, so the widget never
  offers a split the model would refuse.

## 6. Two theme bundles

Palantir's rule is one bundle per widget, hung off `Theme`, overridable per
instance with `.style(&bundle)`. A bundle that would only restate another one
does not exist — `ComboBoxTheme` carries geometry alone and takes its colours
from `button` and `context_menu`. The dock follows that: dividers read
`Theme::splitter`, strips read `Theme::tabs`, and `Theme::dock` holds only what
is dock-specific.

| Bundle | Field | Carries |
|---|---|---|
| `TabsTheme` | `active`, `inactive` | Two `StatefulLook` packs, one per selected state — the `ToggleTheme` shape, so hover and press resolve through the same `pick` precedence as every other widget |
| | `accent`, `accent_idle` | The selection cap. `accent_idle` paints when the view does not hold focus, which is what makes one pane read as "where actions go" |
| | `accent_thickness` | Cap breadth. The chip lifts its inner top inset by the same amount, so the cap adds no height |
| | `strip`, `strip_padding`, `gap`, `hline` | The band behind the chips and the hairline under it |
| | `corner`, `padding`, `min_width`, `max_width` | Chip geometry. `max_width` is what lets a long title ellipsise instead of pushing its neighbours out |
| | `close`, `close_size`, `badge`, `badge_size` | The `×` look pack and the unsaved-changes dot |
| `DockTheme` | `preview_fill`, `preview_stroke`, `preview_corner` | The translucent region a drop would occupy |
| | `caret_width` | The insertion mark between two chips |
| | `ghost` | A `WidgetLook` for the chip trailing the pointer |
| | `edge_fraction` | How far in from each edge the split wedges reach. `0.25` today |

`min_pane`, `max_depth`, and `allowed_splits` change what the dock will *do*,
not what it looks like. They sit on the builder, where a caller sets them once,
and never in a serialised theme file a designer might edit.

## 7. Decisions, and what each one costs

### The tab payload is a key, not the content

`DockState<T: Copy + Eq + Hash>`. Darkroom's `TabRef` already is one. egui_dock
stores the tab value itself, which forces `&mut Tab` through every viewer
method.

A key keeps `DockState` `Clone + PartialEq + Serialize`. Darkroom's undo layer
diffs snapshots for a no-op, and that only works because vector equality *is*
structural equality here.

**Cost:** the application resolves a key into a title and a body on every
frame. That is one match arm per tab kind, and it is what `DockTabs` exists
for.

### The widget emits ops and never mutates

`&DockState<T>` in, `&mut Vec<DockOp<T>>` out. The obvious alternative is
`&mut DockState`, matching `Splitter`'s `&mut f32`.

Darkroom applies view ops through a queue that also carries graph edits and
application commands, keeps them out of undo, and validates the tree at save. A
widget that mutated mid-record would step around all three.

**Cost:** two calls where one would do. `DockView::run` collapses them for
applications with no such pipeline.

### Widget ids come from a seed on the state

`DockState::new("darkroom.dock", …)` stores a seed; every chip, pane, and
splitter id derives from it and from the tab or group it belongs to — never
from a strip slot.

This is what lets `scan` be a method on the state rather than an associated
function needing the view's id passed twice. It also survives an undo that
rearranges the strip mid-gesture, because no id is positional.

**Cost:** eight bytes in the serialised layout, and a rule to state in the
docs — one `DockState` per tab domain.

### Binary splits in version one

egui_tiles uses n-ary linear containers with a share per child, and it is the
better model. Palantir's `Splitter` is binary, Darkroom's tree is binary, and
686 lines of tests are written against it.

The node enum keeps room for a `Linear` variant, and the op vocabulary already
addresses splits by path rather than by child count.

**Cost — named, not hidden:** in a row of three panes the second divider lives
inside one half of the first, so dragging the first moves the second on screen.
An n-ary container is the only fix, and it needs a new n-way widget beside
`Splitter`.

### A trait for the dock, a value binding for the tabbed view

`DockView` plus `DockTabs` mirrors `GpuView` plus `GpuPaint`, the one place
Palantir already asks a caller to implement a trait. Six defaulted questions
per tab do not fit in builder closures without boxing one per frame.

`TabbedView::new(&mut usize, &[S])` mirrors `ComboBox::new`. A dialog with
three pages should not implement a trait.

**Cost:** two idioms in one module tree. They are separated by the layer
boundary, and each matches the precedent nearest to it.

### One pinned tab, taken at construction

Dear ImGui's central node exists so a dock space is never empty. Darkroom gets
the same property from a `Main` tab that refuses to close.
`DockState::new(seed, pinned)` makes it a constructor argument.

The tree then has no empty state at all: the pinned tab's group cannot
collapse, so the root always survives and `focused` always has somewhere to
fall back to.

**Cost:** an application that wants a genuinely closable-to-empty dock cannot
have one. No caller in sight wants that, and the invariant removes a whole
class of empty-state bugs.

### A `Vec<T>` per group stays nested

The style guide prefers one flat buffer with the shape beside it. Here the
exception it names applies exactly: groups are few, and each group's tab list
is pushed to independently long after construction.

The split *tree* is still flat — one `Vec<DockNode>` in canonical pre-order,
with index children and no per-node box. That is better than egui_dock's
heap-indexed vector, which wastes slots on an unbalanced tree.

**Cost:** one allocation per group when a group is created. Never per frame.

## 8. The plan

Seven phases. Each one compiles, tests, and shows something in the showcase
before the next starts. Phases 1 to 5 land in Palantir and leave Darkroom
untouched; phase 6 is the swap.

**All seven are done.** Where the shipped code departs from what is written
above it, section 11 says so — read that before trusting section 4's API
listing, which is the plan's spelling rather than the crate's.

### 1. TabsTheme and TabStrip — done

The chip row alone: two look packs, the accent cap, close button, badge slot,
deterministic chip ids from a caller-supplied key, and horizontal scroll for
overflow. No content area, no tree.

- `widgets/theme/tabs.rs`
- `widgets/tabs/{mod,tab_strip,tab_item,tests}.rs`

Verify: the chain scoped to `-p palantir`, plus the visual suite
(`tests/visual/fixtures/tabs.rs`, golden `tab_strip`).

### 2. TabbedView — done

Strip over content, bound to `&mut usize`, with the content area recorded under
a stable id so a body can read its arranged size on the pass it first records.
A showcase page follows here, not later.

- `widgets/tabs/tabbed_view.rs` (`mod.rs` declares and holds no type — see
  section 11)
- `bin/showcase/pages/tabs.rs`

Verify: the chain, plus the golden `tabbed_view`.

### 3. DockState and its ops — done

The tree, generic over `T`: flat pre-order storage, the six ops, `normalize`,
`validate`, the seed, and the pinned tab. Darkroom's 686-line suite ports with
its assertions intact.

- `widgets/dock/dock_state.rs`
- `widgets/dock/{dock_node,dock_op,dock_path}.rs`
- `widgets/dock/{split_side,tab_group,error,tests}.rs`

Verify: the chain. No rendering yet.

### 4. DockView and DockTabs — done

The recursive walk: splits onto `Splitter` with the ratio drag surfacing as
`SetRatio`, leaves as a `TabStrip` over a group-keyed content area. `scan`
covers activation, close, and pane focus. Drag docking is not in this phase.

- `widgets/dock/dock_view.rs` (`mod.rs` declares and holds no type)
- `widgets/dock/dock_tabs.rs`

Verify: the chain, plus harness tests on pane rects — a split dock tiles its
panes and strips, a chip click switches the pane on the same frame, a close
click removes without activating.

### 5. Drag docking — done

`classify_drop` and its rect tests, the drag gesture in `Ui` widget state
rather than application state, and the preview and ghost chip on
`Layer::Tooltip`. `DockTheme` arrives with them.

- `widgets/theme/dock.rs`
- `widgets/dock/{pane_geometry,tab_drag}.rs` — named for the type each holds
- `bin/showcase/pages/dock.rs`

Verify: the chain, plus the visual suite (golden `dock_split_panes`).

### 6. Darkroom migrates — done

Delete `gui/dock` and `core/document/dock`. `Document` holds a
`DockState<TabRef>`; the session implements `DockTabs`; `Requests::push_view`
carries `DockOp<TabRef>`. The theme bridge fills the two new bundles beside the
splitter tweak it already makes.

- `gui/dock/` — removed
- `core/document/dock/` — removed
- `gui/window/dock_panes.rs` — new; the record pass composes the borrows, so
  the implementor lives beside `MainWindow` rather than under the session
- `gui/theme/palantir_bridge.rs`

Verify: the chain, `-p darkroom -p palantir`.

### 7. What Darkroom never had — done

Keyboard travel along the strip on the WAI-ARIA tab pattern, an overflow menu
listing the chips that scrolled out, `AllowedSplits`, and an allocation gate
proving a steady-state dock frame allocates nothing.

- `TabOverflow` stays in `widgets/tabs/tab_strip.rs` — a config the widget
  takes is a satellite of it, not a module
- `tests/alloc/fixtures/dock.rs` — the suite's fixtures live in that directory

Verify: the chain, plus `tests/alloc`.

## 9. What stays in Darkroom

- `TabRef` itself, and the label each variant resolves to.
- The unsaved-changes rule — which tab kinds show the document, and so reserve
  the badge.
- The split context menu's wording, through `DockTabs::tab_menu`.
- Document validation. `DockState::validate` checks structure; "exactly one
  group holds the `Main` tab" is Darkroom's own sentence to write.

## 10. Risks

- **The drop preview paints from last frame's rects.** It is exact while panes
  hold still, which is every frame of a drag except the first after a layout
  change. The general fix is the layout barrier in `record-time-geometry.md`;
  this design does not wait for it.
- **Golden images move.** Any change to strip chrome shifts them. Run the
  visual suite on phases 1, 2, and 5, not only the unit chain.
- **Palantir builds standalone.** It is a submodule. Nothing here needs a new
  crate, and its `Cargo.toml` must not start inheriting from the enclosing
  workspace.
- **Scratch buffers move into `Ui` state.** Darkroom's `DockUi` owns its label
  and chip buffers across frames. A Palantir builder is rebuilt each frame, so
  they move to `Ui::state_mut`, the way `Splitter` and `Scroll` already keep
  theirs.

## 11. What shipped, against this plan

Seven phases, all landed. What follows is every place the crate disagrees
with what is written above, and why — so section 4's API listing is read as
the plan's spelling rather than as the signature.

### The API moved

- **`TabItem::badge` is a `TabBadge`, not a `bool`.** Three states: no dot,
  the dot's box drawn empty, the dot inked. A `bool` cannot say "reserve the
  box, ink nothing", so a save would have resized Darkroom's graph chip and
  shifted every chip to its right. `DockTabs::badge` returns the same enum.
  Both crates pin the property.
- **`DockTabs::look` is gone.** A per-tab `&WidgetLook` puts a lifetime on
  `TabItem`, and so on the item buffer the strip keeps across frames. The
  per-strip `.style(&TabsTheme)` override covers what a caller in sight
  wants; a per-*tab* one can come back when a caller needs it.
- **`DockTabs::tab_menu` takes a `DockTabMenu` bundle** — the tab, its group,
  the op sink, and the menu's close handle. Without the sink an item could
  not emit the split it names, and without the group it could not address
  one.
- **`max_depth` and `allowed_splits` sit on `DockState`, not on `DockView`.**
  The model is what enforces them: `apply` refuses a deeper split,
  `validate` rejects a tree holding one, and `scan` resolves a drop on
  release — a phase where no builder exists. A second copy on the widget
  could only disagree with this one. `min_pane` and `overflow` stay on the
  builder, where the plan put them.
- **`TabStripResponse` reports pointer and keyboard activation apart** —
  `clicked` and `keyed`. The dock's scan already holds the click a phase
  earlier, so the record pushes `keyed` alone rather than queueing the same
  op twice. The cost is named where it is paid: a keyboard move lands one
  frame after the press, because the strip resolves an arrow against an
  input scope that only exists while it records.
- **`TabsTheme::padding` is `chip_padding`.** The bundle `#[serde(flatten)]`s
  `SlotDefaults`, which carries a `padding` of its own, and the two collide
  on the wire.
- **`TabGroupId` is a counter, not a UUID.** Palantir has no `uuid`
  dependency and will not grow one for this. Two states built by the same
  calls still carry the same ids and compare equal, which is the property
  the undo no-op diff rests on.
- **Two fields the plan's theme table did not name**: `TabsTheme` carries
  `hline_thickness` beside `hline` (a hairline with no breadth is one
  hard-coded number), and `DockTheme` carries `ghost_padding` and
  `ghost_offset` (a `WidgetLook` says nothing about the box around the
  label or where it sits relative to the pointer).

### The file layout moved

`mod.rs` in both `widgets/tabs/` and `widgets/dock/` declares and holds no
type, so every type sits in a file named after it — the crate's own rule.
`drop_zone.rs` is `pane_geometry.rs` and `drag.rs` is `tab_drag.rs` for the
same reason. `TabOverflow` stays with `TabStrip`: a config a widget takes is
a satellite of it. The Darkroom implementor is `gui/window/dock_panes.rs`,
where the borrows it needs are already composed.

### The frame fixture does not record either widget

`FrameFixture`'s module doc freezes its node structure — adding to it
retargets every recorded bench series at once — so both are listed in that
suite's `EXCLUDED` with their reasons. A steady-state dock frame is measured
instead by `tests/alloc/fixtures/dock.rs`, over a surface of its own. It
reads a strict zero, in both the one-call and the two-call surface.

### What is left

Nothing from this plan. Three things it named as future work stay future:

- **Binary splits.** Section 7 names the cost and it is unchanged: in a row
  of three panes the second divider lives inside one half of the first, so
  dragging the first moves the second on screen. The fix is an n-ary
  container, which needs a new n-way widget beside `Splitter`.
- **The drop preview paints from last frame's rects.** Exact while panes
  hold still, which is every frame of a drag except the first after a layout
  change. The general fix is the layout barrier in `record-time-geometry.md`.
- **Floating windows** as a second surface kind, which section 2 marked
  worth taking from egui_dock and no phase claimed.

## Sources

- [egui_tiles — `Behavior`](https://docs.rs/egui_tiles/latest/egui_tiles/trait.Behavior.html)
- [rerun-io/egui_tiles](https://github.com/rerun-io/egui_tiles)
- [egui_dock — crate docs](https://docs.rs/egui_dock/latest/egui_dock/)
- [egui_dock — `DockArea`](https://docs.rs/egui_dock/latest/egui_dock/widgets/dock_area/struct.DockArea.html)
- [Dear ImGui — Docking wiki](https://github.com/ocornut/imgui/wiki/Docking)
- [Dockview](https://dockview.dev/)
- [rc-dock](https://github.com/ticlo/rc-dock)
