# Floem — reference notes for Palantir

Floem is the Lapce team's Rust GUI toolkit: **fine-grained signal reactivity** (Leptos-style) wrapped around a **retained `View` tree** that delegates all layout to **Taffy** and rendering to a swappable backend (Vello / Skia / Vger / tiny-skia). It's the most useful counterpoint to Palantir because it answers "what if you keep the tree across frames and let signals do the diffing instead of rebuilding?" — i.e. the opposite trade-off from immediate-mode.

All paths under `tmp/floem/`.

## 1. The `View` trait and `ViewId`

`pub trait View` (`src/view/mod.rs:953`) is small: required `fn id(&self) -> ViewId`, optional `view_style`, `view_class`, `update`, `style_pass`, `event_capture`, `event`, `paint`, `post_paint`. Children are *not* a method on `View` — they live in the global `VIEW_STORAGE` keyed by `ViewId`. A view struct typically holds only `{ id: ViewId, … local fields … }`; the parent/child graph is sidecar state.

`ViewId` (`src/view/id.rs:58`) is a `slotmap::KeyData` newtype, `!Send`. `ViewId::new()` (line 94) inserts into a thread-local `VIEW_STORAGE` slotmap and registers the current root window as owner. Per-view state — including the Taffy `NodeId`, computed style, animations, event listeners, debug name — lives in `state(): Rc<RefCell<ViewState>>` indexed by id. There's no `Vec<Box<dyn View>>` parented hierarchy; it's all flat-store + lookup-by-id, very similar to Clay's arena and Palantir's `Tree`.

`AnyView = Box<dyn View>` (line 116) and `IntoView` (line 132) form the user-facing composition layer. `IntoView::into_view` is conversion; `IntoView::Intermediate: HasViewId` is the trick that lets `Decorators::style(...)` apply styles *before* the view is fully constructed — primitives like `&str`/`i32` go through `LazyView<T>` (line 705) which carries an eager `ViewId` but defers `Label::new` until `into_view()` is called. That's how `("Item One", "Item Two").v_stack()` works: each tuple element resolves to its own `ViewId` at composition time.

There is no `id_path` in the egui sense. Identity is *structural* — a `ViewId` is allocated once at construction and persists for the lifetime of that view in the tree. Reordering moves the slot, doesn't reassign the id. Removal triggers `ViewId::remove` (line 148), which drops scopes, taffy nodes, and listeners.

## 2. Signal/effect reactivity

The `floem_reactive` crate (`reactive/src/`) is a hand-rolled Leptos-clone:

- `Id` (`reactive/src/id.rs`) is the per-thing handle for signal/effect/scope.
- `Runtime` (`reactive/src/runtime.rs:37`) is one thread-local holding `signals: HashMap<Id, SignalState>`, `effects: HashMap<Id, Rc<dyn EffectTrait>>`, `current_effect: RefCell<Option<…>>`, `current_scope`, parent/child scope graph, and a `pending_effects` queue. The whole reactive system is thread-local and `!Send`.
- `Effect::new(f)` (`reactive/src/effect.rs:80`) registers an effect and runs it once; during the run, every `Signal::get()` calls `Runtime::add_observer` to record the dependency. On signal write, observers are re-queued. Standard fine-grained reactivity.
- `RwSignal<T>` / `ReadSignal<T>` / `Memo<T>` / `Trigger` / `derived_signal` are the public surface. Storage is `UnsyncStorage` (`Rc<RefCell<…>>`) by default; `SyncStorage` for cross-thread (`reactive/src/storage.rs`).
- `Scope` (`reactive/src/scope.rs`) owns a tree of disposable resources. Disposing a scope cascades to children — this is how view removal cleans up effects without leaks.

**Integration with the view tree** is the load-bearing trick. A reactive view is built like:

```
pub fn slider(percent: impl Fn() -> f32 + 'static) -> Slider {
    let id = ViewId::new();
    create_effect(move |_| {
        let percent = percent();          // signal subscription happens here
        id.update_state(percent);         // posts UpdateMessage to the view
    });
    Slider { id, percent: 0.0 }
}
```

(See `view/mod.rs:30-43` for the canonical example, and `widget update` in `views/dyn_stack.rs:137` for how the view consumes the message.) The effect closes over the signal, recomputes on change, and pushes the new value into the *retained* `View` instance via `id.update_state(Box<dyn Any>)` → `View::update(&mut self, …)` (line 979). The view then calls `id.request_layout()` / `request_paint()` / `request_style()` (`view/id.rs:849, 883, 888`) to mark dirty — these are cheap message-bus ops, not direct mutation. The framework drains the queue once per frame.

So the architecture is: **signals own state; effects translate signal changes into typed messages; views are persistent, mutated only via `update`; dirty marking on `ViewId` triggers minimal repasses**. This is Leptos's model with a retained UI tree instead of a DOM.

`IntoView for RwSignal<T>` (line 742) and `for Box<dyn Fn() -> IV>` (line 733) wrap automatically into `DynamicView` — a view whose body is a closure run inside an effect, which swaps its single child whenever signals change.

## 3. Taffy integration

Floem doesn't implement layout at all. Every `View` owns a Taffy `NodeId` (`view/id.rs:248`):

```rust
pub fn new_taffy_node(&self) -> NodeId {
    self.taffy().borrow_mut().new_leaf(taffy::style::Style::DEFAULT).unwrap()
}
```

The Taffy tree (`view/storage.rs:32`: `pub type LayoutTree = taffy::TaffyTree<LayoutNodeCx>;`) lives once per window in `VIEW_STORAGE.taffy: Rc<RefCell<TaffyTree<…>>>`. View parent/child relationships are mirrored into Taffy via `add_child`/`set_children` (`view/id.rs:307, 358, 397`). The view's `Style` (Floem's CSS-like struct) is converted to `taffy::Style` via `to_taffy_style` whenever the style pass dirties a node.

Layout is a single Taffy call (`window/state.rs:753`):

```rust
pub fn compute_layout(&mut self) {
    self.root_view_id.taffy().borrow_mut()
        .compute_layout_with_measure(
            self.root_layout_node,
            taffy::prelude::Size { width: AvailableSpace::Definite(...), ... },
            |known, available, node_id, ctx, style| match ctx {
                Some(LayoutNodeCx::Custom { measure, .. }) => measure(...),
                None => taffy::Size::ZERO,
            },
        );
    ...
    self.needs_box_tree_commit = true;
}
```

`LayoutNodeCx::Custom { measure, finalize }` is how text and images participate: views that need intrinsic measurement install a measure closure on their Taffy node (see `text/layout_state.rs:436`). Taffy then calls back into Parley/glyph layout during its own measure pass. After Taffy finishes, `update_box_tree_from_layout` walks the result and writes local-space rects into a separate `BoxTree` (`box_tree.rs`) — that's the structure consulted for hit-testing, paint walking, and damage tracking. Three trees, by purpose: View tree (logic + state), Taffy tree (layout), Box tree (visual rectangles).

## 4. Renderer abstraction

`floem_renderer::Renderer` (`renderer/src/lib.rs:82`) is the backend trait: `begin`/`finish` bracket a frame, with `set_transform`, `clip`/`clear_clip`, `fill`, `stroke`, `draw_text_lines`, `draw_svg`, `draw_img` between. All geometry uses `peniko::kurbo` types — `Rect`, `RoundedRect`, `Affine`, `BezPath`, `Stroke`, `Brush`. Text is `parley`-based (`renderer/src/text/`).

Backends live in sibling crates: `vello/` (default, GPU scene-graph), `skia/`, `vger/`, `tiny_skia/` (CPU). Each implements the same trait and is selectable via Cargo features. `paint/renderer.rs` is the framework-side wrapper.

Painting (`paint/mod.rs`) walks a *stacking-context-ordered* list of `BoxTree` `ElementId`s — not the View tree directly. `collect_stacking_context_items_into` (`view/stacking.rs`) flattens the tree into a z-ordered draw list respecting `z-index`, opacity, transforms, popups. `View::paint` is called per element with a `PaintCx` that proxies the underlying `Renderer`. Children are painted automatically by traversal; `View::paint` only emits the view's *own* content (background, custom drawing). `paint_bg`/`paint_border`/`paint_outline` are framework-provided and called around `View::paint`.

The split `ElementId` (`box_tree.rs:46`) vs `ViewId` is interesting: one View can own multiple Elements (a scroll view owns: content, vertical bar, horizontal bar). Hit-testing and paint operate on Elements; logic and event delivery on Views. Many-to-one: `ElementId.view_id() → ViewId`.

## 5. State propagation and child diffing

There is **no per-frame VDOM diff**. The retained tree only changes when:

1. A view *explicitly* mutates itself in `View::update` (called when something `id.update_state(…)`s it from an effect).
2. A `dyn_stack`/`DynamicView` runs its child-rebuild effect.

`dyn_stack` (`views/dyn_stack.rs:81`) is the keyed-list primitive. User supplies `each_fn`, `key_fn`, `view_fn`. Inside a single `Effect::new` (line 92), each signal change re-collects items, hashes keys into an `FxIndexSet`, and computes a `Diff { removed, moved, added, clear }` against the previous run via `diff()` (line 194) — basically Solid's keyed reconciler, three SmallVecs of ops. The diff is `id.update_state`d into the `DynStack` view, which `apply_diff`s it against `self.children: Vec<Option<(ViewId, Scope)>>`. Removals dispose scopes (cleaning up effects); additions call `view_fn` inside the captured scope so child reactivity is parented correctly.

For non-list dynamic content there's `DynamicView` (`views/dyn_view.rs`), which has a single child swapped on signal change — no diff, just dispose-old + mount-new.

Style/layout/paint propagation runs through dirty flags on `ViewState`. `request_layout` posts `UpdateMessage::RequestLayout`; the next frame's update phase drains messages and marks the view's Taffy node dirty (`mark_view_layout_dirty`, `view/id.rs:266`). Taffy's own incremental layout then re-measures only the dirty subtree. This is genuine *partial* re-layout — same trick WPF uses, but driven by `mark_dirty` on the Taffy tree rather than WPF's `MeasureQueue`.

## 6. Lessons for Palantir

**Copy.**
- Sidecar state pattern: views (in our case, nodes) hold an id; mutable per-node state lives in a separate map keyed by that id. Floem's `VIEW_STORAGE` and Palantir's `WidgetId → Any` future map (`DESIGN.md §4`) converge here.
- Scope-tree disposal for cleanup: when a node disappears, dispose a `Scope` and let it cascade-cancel any state/effects it owned. Cleaner than tracking lifetimes per resource.
- Renderer trait shaped around `peniko::kurbo` + a small set of primitives (`fill`, `stroke`, `draw_text_lines`, `draw_svg`, `draw_img`, `clip`, `set_transform`). When Palantir's wgpu pass lands, target this same surface — a `kurbo`-shaped abstraction lets us swap or A/B test backends and reuses Floem's text stack (Parley) effectively for free.
- Three-pronged identity: layout id, paint id, logic id may not be 1:1 (`ElementId` vs `ViewId`). A scroll widget genuinely is multiple rectangles. We may eventually need this; remember it exists.
- Keyed diff in *one* place (`dyn_stack`), not as a global VDOM. The 80-line `diff()` over `FxIndexSet` is the entire reconciler — copy verbatim if/when we add reactive list helpers on top of the immediate-mode core.

**Avoid.**
- Don't adopt fine-grained signals as the foundation. Floem needs them because the tree is retained and you have to find another way to express "the UI depends on this state." Palantir rebuilds the tree every frame, so dependency tracking is the for-loop the user already wrote. Adding signals on top would duplicate the mechanism. (This is exactly Xilem's bet, and Palantir sides with Xilem here, not Floem.)
- Don't use Taffy. Floem gets flexbox/grid/block for free, but at the cost of: a second tree to mirror, `Style → taffy::Style` conversion every restyle, an opaque measure-callback boundary, and Taffy's own perf characteristics (cache-heavy, designed for retained mode). Our `Sizing::{Fixed,Hug,Fill}` covers the WPF subset we actually care about in <300 lines and is dirt simple to reason about. Reach for Taffy only if we discover we need flexbox semantics that don't reduce cleanly to our model.
- Don't split into View tree + Layout tree + Box tree. Floem needs three because retained nodes, layout-engine nodes, and visual rectangles all have different lifetimes. Palantir rebuilds the lot every frame, so `Tree.nodes` + `Tree.shapes` is enough; resolve world rects on the fly during paint.
- Don't put the reactive runtime in a thread-local. Floem's `RUNTIME` thread-local is a constant source of `!Send` headaches and "must be on UI thread" panics. If we ever add signals (for animation, async data), make the runtime a value passed to `Ui` so multi-window/headless cases stay clean.
- Don't ship four renderer backends. Floem maintains Vello + Skia + Vger + tiny-skia — that's a tax on every renderer-touching change. Pick one (wgpu-direct for perf, or wrap Vello if we want kurbo paths cheaply) and commit.

**Simplify.**
- Floem's `View` trait has 9 hooks (`update`, `style_pass`, `event_capture`, `event`, `paint`, `post_paint`, `view_style`, `view_class`, `debug_name`). Palantir's widget contract is `fn show(self, &mut Ui) -> Response` — a function, not a trait. Keep it that way. Behaviour customization happens by composing recorded shapes and `WidgetId`-keyed state, not by overriding methods.
- Floem's message bus (`UpdateMessage::RequestLayout`/`RequestPaint`/`RequestStyle`/`RequestBoxTreeUpdate`/…) exists because dirtying happens *between* frames from arbitrary effects. With record-every-frame there's no message bus needed: the next frame's recording *is* the update.
- `IntoView` + `Intermediate` + `LazyView` is three traits to make `"hello"` stylable before becoming a `Label`. Palantir's `Button::new(id).label("hello")` builder achieves the same with one impl block.
- Floem inserts `Effect::new` calls for every dynamic prop. Our equivalent is the user's normal Rust closure, evaluated once per frame inside their record callback. Cheaper, simpler, no subscription tracking, no leak surface.

**Single biggest takeaway:** Floem proves you can build a polished Rust UI by combining "Leptos signals" + "retained view tree" + "Taffy" + "swappable kurbo renderer" — but each of those four is a meaningful complexity import. Palantir's bet is the opposite: rebuild the tree, walk it twice, paint it, no signals, no Taffy, one renderer. Floem is the reference for the renderer abstraction (copy) and for what the reactive-retained branch costs (avoid).
