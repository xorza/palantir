# druid — reference notes for Palantir

Druid is Raph Levien's pre-Xilem retained-mode Rust UI: `Data + Lens` for state binding, `Widget` trait with four lifecycle methods, Flutter-style `BoxConstraints` for layout, and a `WidgetPod` wrapper that owns dirty-flag plumbing. It's the canonical worked example of "what happens when you try to do retained-mode UI in Rust without GC", and the design Levien himself walked away from when he wrote the *Towards principled reactive UI* / *Xilem* posts. Worth reading carefully — most of what *doesn't* work is more instructive than what does.

Source paths below are under `tmp/druid/druid/src/`.

## 1. The `Widget<T>` trait — four methods plus `update`

`widget/widget.rs:78` defines the trait. Parametrised by `T: Data`, the app's data model:

```rust
pub trait Widget<T> {
    fn event    (&mut self, ctx: &mut EventCtx,     event: &Event,     data: &mut T, env: &Env);
    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &T,     env: &Env);
    fn update   (&mut self, ctx: &mut UpdateCtx,    old: &T, data: &T,                env: &Env);
    fn layout   (&mut self, ctx: &mut LayoutCtx,    bc: &BoxConstraints, data: &T,    env: &Env) -> Size;
    fn paint    (&mut self, ctx: &mut PaintCtx,                          data: &T,    env: &Env);
}
```

- `event` (`widget.rs:87`) — pointer/key/command, `&mut T` so handlers mutate app state directly.
- `lifecycle` (`widget.rs:101`) — `WidgetAdded`, `Size`, `HotChanged`, `FocusChanged`, `RouteFocusChanged` etc. Internal bookkeeping; widget can update *its own* state but not data.
- `update` (`widget.rs:127`) — diff hook fired when `Data::same(old, new) == false`. Widget compares `old_data` vs `data` and calls `ctx.request_layout()` / `ctx.request_paint()` accordingly. **This is the load-bearing method**: it's how a retained tree learns the model changed.
- `layout` (`widget.rs:149`) — Flutter-style: take `BoxConstraints`, return `Size`. Containers loop `child.layout(bc')`, then `child.set_origin(point)`.
- `paint` (`widget.rs:161`) — render via `piet::RenderContext` (Druid's Cairo/Direct2D abstraction).

Each method takes a *different* context type. The contexts share a common state struct under the hood but expose different capability sets — e.g. only `EventCtx` lets you mutate data, only `UpdateCtx`/`EventCtx` can `request_layout`, only `PaintCtx` derefs to `RenderContext`. Five methods × five contexts is the price of retained-mode in Rust: every recursive operation gets its own pass with its own borrow shape.

## 2. `BoxConstraints` — Flutter-style min/max in both axes

`box_constraints.rs:28`:

```rust
pub struct BoxConstraints { min: Size, max: Size }
```

Doc-comment at `box_constraints.rs:9` is explicit: *"The layout strategy for Druid is strongly inspired by Flutter, and this struct is similar to the Flutter BoxConstraints class."* Containers shrink the constraint before recursing (`shrink`, line 143) and clamp child output (`constrain`, line 89). `BoxConstraints::UNBOUNDED` (line 37) carries `max = (∞, ∞)` — the "tell me your intrinsic size" mode. `tight(size)` (line 65) collapses min == max for forced sizes. A child *must* return a size satisfying `min <= s <= max`, and `debug_check` (line 116) warns if either is degenerate.

This is strictly more expressive than WPF's single `availableSize` + separate `MinWidth`/`MaxWidth` properties: `BoxConstraints` ships the min and max along with the request, in one bundle, so the *child* sees them — useful for "fill at least this much, at most that much" widgets like `Flex` flex children. The cost is that every container has to explicitly construct the right `BoxConstraints` for each child, which is most of `widget/flex.rs`'s body. `Flex` runs *two* layout passes per frame internally (`flex.rs:21`): non-flex children get unbounded constraints first, then flex children get the leftover space as both min and max.

## 3. `WidgetPod<T, W>` — the wrapper that makes retained-mode work

`core.rs:39`. Every child reference inside a container is held as `WidgetPod<T, ChildW>`, never as the bare `Widget`:

```rust
pub struct WidgetPod<T, W> {
    state: WidgetState,            // origin, size, dirty bits, focus/hot/active, paint insets, ...
    old_data: Option<T>,
    env: Option<Env>,
    inner: W,
    debug_widget_text: TextLayout<ArcStr>,
}
```

`WidgetState` (`core.rs:63`) holds:
- `id: WidgetId` (a `NonZeroU64` counter, `widget.rs:243`),
- `size: Size`, `origin: Point`, `parent_window_origin: Point`,
- `paint_insets: Insets`, `baseline_offset: f64`,
- `invalid: Region` — sub-rect to repaint,
- a pile of dirty bits: `needs_layout`, `request_anim`, `is_hot`, `is_active`, `has_focus`, `has_active`, `children_disabled_changed`, `children_view_context_changed`, `view_context_changed`, `is_explicitly_disabled`, `ancestor_disabled`, …

The pod's job is to wrap each `Widget` method with the bookkeeping the trait method itself isn't allowed to know about:

- `WidgetPod::layout` (`core.rs:533`) clears `needs_layout`, sets `is_expecting_set_origin_call` (line 549–550, a debug guard for parents that forget to call `set_origin`), runs `inner.layout`, fires `LifeCycle::Size` if the size changed (line 565–570), then `merge_up`s the child's state flags into the parent (line 573, `merge_up` defined at `core.rs:1280`-ish). The pattern repeats for `event`, `lifecycle`, `update`, `paint` — each pod method opens its own context, dispatches to `inner`, then merges child flags upward.
- `set_origin` (`core.rs:289`) is a separate call the *parent* makes after `child.layout`. So a container's `layout` is "for each child: `child.layout(bc')`; `child.set_origin(point)`; sum/max sizes". The same shape as WPF, but expressed as two method calls instead of one.
- Dirty-flag bubbling: `child_state.needs_layout |= ...` (`core.rs:1285`) merges child requests into the parent every pass — this is how `request_layout()` from a leaf reaches the root.

**Without `WidgetPod`, Druid's `Widget` trait could not be reasonably implemented by users**: every widget would have to remember to clear `needs_layout`, save `old_data`, propagate `is_hot`, etc. The pod hides ~1500 lines of state-machine plumbing behind a uniform interface. It is also where almost all of Druid's complexity lives.

## 4. `Data` + `Lens` — equality-based change detection

`data.rs:96`:

```rust
pub trait Data: Clone + 'static {
    fn same(&self, other: &Self) -> bool;
}
```

`Data::same` is *like* `PartialEq` but cheaper: for `Arc<T>`, comparing pointers (`Arc::ptr_eq`) is enough — same allocation ⇒ same value. Druid threads `Data` everywhere because `update(old: &T, new: &T)` needs a fast "did it change?" predicate to decide whether to re-render a subtree. The `druid_derive::Data` macro auto-implements it field-wise.

`lens/lens.rs:26`:

```rust
pub trait Lens<T: ?Sized, U: ?Sized> {
    fn with    <V, F: FnOnce(&U)     -> V>(&self, data: &T,     f: F) -> V;
    fn with_mut<V, F: FnOnce(&mut U) -> V>(&self, data: &mut T, f: F) -> V;
}
```

Lenses are how a child widget addressed by `Widget<U>` is plugged into a parent over `Widget<T>`. Each child holds a `Lens<T, U>`, and `LensWrap` (`widget/lens_wrap.rs`) projects through it on every pass. `Lens::with` returns by closure rather than reference — required so a lens may *synthesise* the projected value on the fly (e.g. an `Arc<Vec<T>>` lens that clones the inner vec). `LensExt` (`lens.rs:46`) gives `then`/`map`/`get`/`put` for composition.

The combination — `Data` for "has it changed?" and `Lens` for "where in the model does this widget read from?" — is Druid's whole reactive story. Levien's *Towards principled reactive UI* lays out the rationale: avoid re-rendering when `data.same(old) == true`, and let lenses make that check granular per-subtree.

## 5. Where Druid hit walls

Levien's *Xilem* post and the Mason/Cheats+Stables follow-ups enumerate where this design ran out of road:

- **Collections.** `Data` isn't impl'd for `Vec<T>` because clone is O(n) and `same` would have to compare element-wise. Druid's escape hatches are `Arc<Vec<T>>` (rely on `Arc::ptr_eq`, force users into copy-on-write) or the `im` crate (`data.rs:60`–`66`) for persistent immutable structures. Both are friction. `widget/list.rs:178` shows the canonical `impl ListIter<T> for Arc<Vec<T>>`. Mutating an item still clones the whole vec.
- **Lensing into a `Vec` element.** A `Lens<Vec<T>, T>` indexed by `usize` is unsound across edits: deletions shift indices. `lens::Index` works for fixed positions but not for "this row in a dynamic list". Druid's `List` widget sidesteps this by re-creating per-row pods on each `update` whose `Data` changed, but it can't deliver stable per-row identity for things like animation or focus.
- **Closures with state.** A widget like `Button::new(|data, env| /*…*/)` wants to capture state, but the closure has to be `Fn`, not `FnMut`, because `update` can be called many times. Capturing things like a per-button counter requires plumbing state through `T` or stashing it on the widget — both ugly. Levien's *Cheats+Stables* post calls this out as the "no good way to express mutable local state" problem.
- **Async and side effects.** `event` is the only method that can mutate `T`, but it runs synchronously. Real apps want async: launch a request, store the result. Druid's answer is `ExtEventSink` + `Command` round-trips, which forces every async result into the event loop as a typed message, hand-routed by `Selector`. Boilerplate-heavy and weakly typed.
- **Component reuse / "scaling components".** A widget tree is `Widget<AppData>` — anything reusable has to be generic over its own slice of state via `Lens`. Building a tabbed editor, a list of editable rows, or any composite with both shared and per-instance state turns into lens gymnastics. There's no clean way to factor a widget that "owns" some local state and exposes a typed interface to its parent.
- **Performance ceiling on `update`.** Every change to `T` walks the whole pod tree calling `update` with `Data::same` checks, even if 99% of the tree doesn't depend on the changed field. The only way to skip a subtree is for *its* `Data` projection to compare `same`. Lensing has to be fine-grained or the diff degenerates.

## 6. Why Levien left for Xilem

The recurring theme in *Towards principled reactive UI* (May 2022) and the *Xilem* announcement: retained-mode + `Data + Lens` is a leaky abstraction over what users actually want, which is a *function from state to UI*. Druid asks the user to maintain a persistent widget tree and surgically update it; React/SwiftUI/Elm ask for a fresh tree each frame and let the framework diff. Levien concluded that in Rust, the "fresh tree each frame" path is cheaper than expected if the tree is made of cheap value types (Xilem's `View` trait, `xilem_core/src/view.rs`) and the *retained side* is moved underneath, hidden from the user (Masonry).

Net: the four-method `Widget` trait, `WidgetPod` pod-and-state wrapper, and `Lens` projections all survived — they just moved to the engine layer (Masonry) where users don't write them. The user-facing layer became `View::build/rebuild/teardown`. Concretely, Xilem keeps Druid's two-pass `measure`/`layout` and per-pod state, but replaces the user-authored `Widget<T>` + `Lens<App, T>` pairing with a transient `View` tree authored by closures over app state.

## 7. Lessons for Palantir

**Copy.**
- The `Widget` method split itself is sound: layout, paint, event, and a "model changed" hook are genuinely orthogonal. Palantir collapses this into a single per-frame record-then-pass pipeline, but the *names* and the *invariants* (no mutation in `paint`, no painting in `layout`) carry over.
- `Lens::with(closure)` instead of `&U` return — the closure form lets you synthesise borrows. If we ever expose a "view a slice of persistent state" API on the `Id → Any` map, mirror that signature.
- `Data::same` as cheap-equality — useful for memoising shape lists in the future scene cache. `Arc::ptr_eq` semantics give O(1) "did this `Arc<Galley>` change?" without re-shaping text.

**Don't copy.**
- `BoxConstraints { min, max }` is the wrong primitive *for our policy*. Palantir's `Sizing::{Fixed, Hug, Fill}` already commits a sizing policy per axis at recording time; the parent computes the child's slot, the child returns its desired size, the parent arranges. There is no widget that needs to negotiate "I'd like at least M but no more than N" *via the constraint* — that's already encoded in `Sizing`. Adding `BoxConstraints` would mean every widget has to honour two redundant systems. Stay with WPF's single `available: Size` + `Sizing` enum, as in `src/layout.rs::resolve_axis`. The narrow place to consider min/max is the future `MinSize`/`MaxSize` style fields, applied as a clamp in the framework wrapper around `measure`/`arrange`, à la WPF's `MeasureCore` — never as an argument the panel sees.
- `WidgetPod` as a user-visible wrapper. Druid pays for it because the tree is retained and per-node state has to live *somewhere*. Palantir rebuilds the tree every frame; per-node *layout* state is just `Node` in the arena, and per-widget *persistent* state goes into the `Id → Any` map keyed by `WidgetId`. The pod's `merge_up` flag-bubbling becomes unnecessary — `request_layout` is "you'll get a fresh layout next frame anyway".
- Five contexts (`EventCtx`/`LifeCycleCtx`/`UpdateCtx`/`LayoutCtx`/`PaintCtx`). Druid splits them to encode capability differences inside the type system, but the resulting maze of `ChangeCtx` traits, `state.merge_up` calls, and `is_expecting_set_origin_call` debug asserts (`core.rs:550`, `:610`) is most of what makes the codebase intimidating. With record-then-pass, each pass has one context; capabilities are obvious from which pass you're in.
- `Data + Lens` plumbed through every widget. Our model is "user code reads its own app state directly during the record pass" — no projection, no `same`, no derive. State binding is a non-problem in immediate-mode authoring.
- `Arc<Vec<T>>` and the `im` crate for collections. The lens-into-vec problem is structural to retained mode + lenses; immediate-mode authoring sidesteps it entirely. A user just writes `for item in &self.items { Row::new(item.id).show(ui); }`.
- Druid's async story (`ExtEventSink`/`Command`/`Selector`). Palantir's plan should be: the user's frame closure is an ordinary `FnMut` over their own state, and async results land in that state through whatever channel they like (`mpsc`, `tokio::watch`, polling a `Future`). The framework stays out of the data flow.

**Single biggest takeaway.** Druid is the cautionary tale: in Rust, every retained-mode primitive — `Data`, `Lens`, `WidgetPod`, the five-context split — is there because the *user* is forced to carry state across frames. Rebuilding the tree every frame removes those primitives wholesale. The four-method `Widget` trait isn't wrong; what's wrong is making users implement it. Levien's escape hatch was Masonry-under-Xilem; ours is no retained tree at all.
