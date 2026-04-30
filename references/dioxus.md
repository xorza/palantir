# Dioxus — reference notes for Palantir

Dioxus is a React-shaped retained/reactive UI for Rust. The user writes function components that return `Element`; a `VirtualDom` runs them, diffs the returned tree against the previous one, and emits a stream of `Mutation`s to a renderer (web DOM, native via Blitz, TUI, Liveview, SSR, …). It is the polar opposite of Palantir's record-then-layout immediate-mode model — which is exactly why it's useful as a foil. This file pins down the parts that are clever even if we don't want them.

All paths under `tmp/dioxus/packages/`.

## 1. VirtualDom: scope slab + dirty-set scheduler

`VirtualDom` (`core/src/virtual_dom.rs:205`) is a small struct: `scopes: Slab<ScopeState>`, `dirty_scopes: BTreeSet<ScopeOrder>`, `runtime: Rc<Runtime>`, and an mpsc `rx` for `SchedulerMsg::{Immediate(ScopeId), TaskNotified, EffectQueued, AllDirty}`. Every component instance is a `ScopeState` keyed by `ScopeId(usize)` (`scopes.rs:14`). Reserved IDs are pinned: `ROOT=0`, `ROOT_SUSPENSE_BOUNDARY=1`, `ROOT_ERROR_BOUNDARY=2`, `APP=3` (`scopes.rs:50`). The slab guarantees IDs are stable between `wait_for_work` calls; recycling only happens at scheduler boundaries so external code can hold IDs across one frame.

The render loop is **not** "rebuild the world every frame." `rebuild` (`virtual_dom.rs:579`) runs exactly once at startup; afterwards `render_immediate` (line 598) only re-runs *dirty* scopes. A scope is marked dirty by `mark_dirty(id)` (line 395) which inserts a `ScopeOrder { height, id }` into `dirty_scopes`. The `BTreeSet` ordering by `height` means parents render before children — when a parent re-renders and its diff changes a child's props, the child is already at the right place in the queue. `process_events` (line 493) drains the channel, `wait_for_work` (line 444) async-awaits new work, and `poll_tasks` (line 506) interleaves task wakeups + queued effects with re-renders.

Re-rendering a scope is `run_scope` (`scope_arena.rs:47`): reset `hook_index` to 0, run `before_render` callbacks, call `props.render()` inside a `ReactiveContext::reset_and_run_in` (which clears subscribers and re-collects them on signal reads — that's how reactivity is automatic). The returned `Element` is then `deep_clone`d (line 80) — a load-bearing copy so the just-rendered VNode tree shares no `Rc` cells with the previously-mounted tree, since the diff later mutates `mount` cells in-place.

## 2. Templates: the Dioxus innovation

`VNode` (`core/src/nodes.rs:96`) wraps `Rc<VNodeInner>` with a `Cell<MountId>`. `VNodeInner` (line 45) has three fields that matter: `template: Template`, `dynamic_nodes: Box<[DynamicNode]>`, `dynamic_attrs: Box<[Box<[Attribute]>]>`. `Template` (line 270) is **all `&'static`**: `roots: &'static [TemplateNode]`, `node_paths: &'static [&'static [u8]]`, `attr_paths`, plus a `xxh64` content hash (line 309) for cross-crate identity.

The `rsx!` macro lifts every static piece of markup — tag names, attribute names, attribute literal values, text fragments, structural shape — into a `static` `Template` constant. Only interpolated `{expr}`/`{var}` slots become `DynamicNode::{Text, Fragment, Component, Placeholder}` (`nodes.rs`). Diffing `VNode` against `VNode` (`diff/node.rs:15`) starts with `if self.template != new.template { replace }` (line 32) — and since both sides point at the same `&'static Template`, in the common case it's a pointer comparison that succeeds. Then it walks only `dynamic_nodes` and `dynamic_attrs` in lockstep (line 54). The static skeleton is never re-walked.

This is the core Dioxus speedup over naive React: a typical component has 90% static markup, and the diff cost is proportional to the dynamic surface only. The renderer-side `Mutation` stream (`core/src/mutations.rs:125` — `AppendChildren`, `AssignId`, `CreatePlaceholder`, `CreateTextNode`, `ReplacePlaceholder`, `SetAttribute`, …) targets dynamic slots via `node_paths` byte-arrays into the static template, which the renderer hydrates once per template instance and then patches.

## 3. Hooks without JS runtime guarantees

Dioxus's hook implementation is in `Scope::use_hook` (`scope_context.rs:331`):

```rust
let cur_hook = self.hook_index.get();
self.hook_index.set(cur_hook + 1);
let mut hooks = self.hooks.try_borrow_mut().expect(...);
if let Some(existing) = self.use_hook_inner::<State>(&mut hooks, cur_hook) {
    return existing;
}
self.push_hook_value(&mut hooks, cur_hook, initializer())
```

`Scope` (line 45) holds `hooks: RefCell<Vec<Box<dyn Any>>>` and `hook_index: Cell<usize>`. `run_scope` resets `hook_index` to 0 on every render (`scope_arena.rs:56`); each `use_hook` call bumps the counter and indexes into the vec. **Order is the identity** — same as React. Initial render extends the vec; subsequent renders downcast and clone the existing slot. The check `if cur_hook >= hooks.len()` (line 375) detects "first render" — every other case is "must already exist," and a mismatch panics with the rules-of-hooks message (line 387). The `try_borrow_mut` on `hooks` is what catches "called a hook inside a hook" (line 343): the outer hook is still holding the borrow. There's a `cfg!(debug_assertions)` escape hatch for `subsecond` hot-patching that allows hook *type* swaps after a code patch (line 381).

Note that `State: Clone + 'static` is a hard requirement and the value is `clone`d out on every read. Real persistent state goes through `generational-box` (`packages/generational-box/src/lib.rs`): `GenerationalBox<T>` is a `Copy` handle into a slab keyed by `(data_ptr, NonZeroU64 generation)` (line 26). The `Owner<S>` ties slots to scope lifetime — when the scope drops, the generation increments and any stale `GenerationalBox` reads return `BorrowError::Dropped`. `Signal<T>` (`signals/src/signal.rs:13`) is a `GenerationalBox<SignalData<T>>` plus reactive-context subscriber tracking. So `use_signal(|| 0)` (`hooks/src/use_signal.rs:39`) stores a `Signal` (`Copy`, 16 bytes) in the hook slot, and the actual `T` lives in the generational arena owned by the scope. Reads through `Signal` register the current `ReactiveContext` as a subscriber; writes notify subscribers, which marks the corresponding scope dirty.

## 4. rsx!: macro → component tree

`packages/rsx/src/` is the macro; `template_body.rs`, `element.rs`, `component.rs`, `node.rs`, `attribute.rs`, `text_node.rs` cover the syntax tree, and `assign_dyn_ids.rs` numbers dynamic slots. The macro emits a `static TEMPLATE: Template = Template::new(&[...], &[...], &[...])` and a `VNode::new(TEMPLATE, dynamic_nodes, dynamic_attrs, key)` per call site. Conditionals (`if`/`match`) and loops (`for`) are *expressions* that return `Element`; they show up in the parent template as `DynamicNode::Fragment` slots, so the parent's static skeleton stays intact even though the children change shape. Components like `MyButton { label: "x", onclick: ... }` lower to `DynamicNode::Component(VComponent { name, render_fn, props: Box<dyn AnyProps> })` — no scope is created at macro time; scopes materialize only when the diff first reaches that slot via `create_scope` (`scope_arena.rs:11`). Hot-reload (`packages/rsx-hotreload/`) works because templates are `&'static`-addressed by hash: swap the static, parent's diff sees a different template, `replace` runs.

## 4b. Runtime: the cross-cutting context

Beside `VirtualDom`, there's a thread-local `Runtime` (`core/src/runtime.rs:32`) installed via `RuntimeGuard` whenever user code runs. It owns: `scope_stack: RefCell<Vec<ScopeId>>` (the current render path, used by `current_scope_id()` so hooks/`use_context` know which scope they're in), `suspense_stack`, `scope_states: RefCell<Vec<Option<Scope>>>` (parallel to `VirtualDom::scopes`, holding the per-scope `Scope` state separately so `&ScopeState` can hand out a `Ref<'_, Scope>` without aliasing the slab), `mounts: RefCell<Slab<VNodeMount>>` (line 70 — pairs each mounted `VNode` to its renderer-side `ElementId`s and dyn-node IDs), `elements: RefCell<Slab<Option<ElementRef>>>`, `pending_effects`, `dirty_tasks`, and the mpsc `sender`. The split between `VirtualDom` and `Runtime` is essentially "the tree owns scopes, the runtime owns ambient state user code can reach without an explicit handle." `Runtime::current()` does a thread-local lookup; this is what makes `use_signal()` and friends callable as bare functions without passing context everywhere.

`ElementRef` is `(MountId, byte-path)` — a stable address inside a mounted template. Events bubble by walking up `mounts[mount].parent` chains. `VNodeMount.mounted_attributes: Box<[ElementId]>` and `mounted_dynamic_nodes: Box<[usize]>` (`nodes.rs:33`) are the renderer's view of one template instance. This is the bookkeeping that makes "the renderer keeps a real DOM, the diff sends patches" work — and it's exactly what an immediate-mode crate doesn't need. Worth seeing once to know what we're not doing.

## 5. Renderer abstraction: WriteMutations + Blitz

The renderer interface is `WriteMutations` (`core/src/mutations.rs`): a trait with `append_children`, `create_text_node`, `create_placeholder`, `assign_node_id`, `replace_node`, `set_attribute`, `hydrate_text_node`, `load_template`, etc. `VirtualDom::render_immediate(&mut impl WriteMutations)` streams calls, never holding the trait object across awaits. Renderers either implement it directly (`Mutations::default()` is a `Vec<Mutation>` collector for tests) or process events into a real backend.

The native renderer (`packages/native/`) is a thin shim over **Blitz**, a Servo/Stylo-backed HTML+CSS engine. `launch` (`native/src/lib.rs:84`) builds a `VirtualDom`, wraps it in `DioxusDocument` (re-exported from `dioxus_native_dom`), and hands it to `blitz_shell` for windowing + `anyrender_vello` for paint. Because Blitz is "real" CSS — Stylo for cascade, Taffy for flex/grid layout, Vello for vector paint — Dioxus components author HTML elements (`div`, `button`, …) with CSS attributes; Blitz handles all layout and rendering. There's no Dioxus-side layout engine. The `tmp/dioxus/packages/native/src/dioxus_renderer.rs` exposes a `use_wgpu` hook so user wgpu code can splice into Vello's frame.

The web renderer (`packages/web/`) targets `web-sys` — `WriteMutations` calls become DOM API calls. `packages/liveview/` keeps the VirtualDom on the server and sends `Mutation`s over a WebSocket. `packages/ssr/` walks the VNode tree once and renders strings. The TUI renderer is gone in current main; Freya (out-of-tree) replaces it with a Skia + Torin layout backend. The point is that `WriteMutations` is the only contract a renderer needs to satisfy, and Blitz is the closest thing to a "default native" backend — but it's HTML/CSS, not a Dioxus-native widget set.

## 6. Bump arenas and node reuse

Dioxus pre-1.0 used `bumpalo` for VNodes — render-into-bump, swap bumps each frame. Current `master` has dropped that: `VNode` is `Rc<VNodeInner>` (`nodes.rs:97`) and `dynamic_nodes` is `Box<[DynamicNode]>`. The frame allocator is gone; Rust's `Box`/`Rc` plus the static `Template` references handle most of the pressure. The remaining "arena" is `VirtualDom::scopes: Slab<ScopeState>`, the `Runtime::mounts: RefCell<Vec<VNodeMount>>` that pairs each mounted VNode to its renderer-side `ElementId`s, and the `generational-box` storage that backs signals. Arena thinking moved from "one bump per frame" to "long-lived slabs keyed by stable ID, with generation counters guarding dangling references."

## 7. Lessons for Palantir

**Copy.**
- `Slab<T>` keyed by stable `Id(usize)` for any per-widget persistent state (focus, scroll, animation). Cheap, stable, allows external references across frames. Matches `DESIGN.md §4`.
- The `BTreeSet<(height, id)>` priority queue idea — if Palantir ever does partial relayout, dirty nodes processed parent-first by height drop is the right ordering.
- `generational-box`-style `(ptr, generation)` handles for any case where a widget needs to hand out a `Copy` reference to its own state (drag handles, animation tokens). Cheaper than `Rc<RefCell<T>>` and surfaces use-after-free as a typed error.
- `WriteMutations`-style trait interface between layout/paint output and the actual renderer. We will want this when winit + wgpu lands so SSR-of-rects, golden-image tests, and a software backend can share the paint stream.
- Templates as `&'static` content-hashed structures is the right answer for hot-reload: static identity = pointer compare = cheap diff. If Palantir adds hot-reload, statically-addressed shape templates would let the recorder skip work on unchanged subtrees.

**Avoid (this is most of the file).**
- The whole VirtualDom + diff loop. Palantir rebuilds the tree every frame by design (`DESIGN.md §3`). No diff means no `MountId`, no `last_rendered_node`, no `dirty_scopes`, no `ScopeOrder`-by-height, no `deep_clone` to break shared `Rc`s, no scheduler channel. The mutation stream collapses into "walk the freshly-recorded tree and paint it." That's the immediate-mode bargain.
- Hooks-by-call-order. The `hook_index: Cell<usize>` + `Vec<Box<dyn Any>>` design enforces "no hooks in conditionals" via runtime panic. Palantir's `WidgetId` is a hash of a user-provided key, so state lookup is by *identity*, not by *position* — putting a button inside an `if` doesn't blow up. Keep the hash-key design; never adopt index-based hook slots.
- Reactive context auto-subscription. `ReactiveContext::reset_and_run_in` (`scope_arena.rs:67`) collects subscribers on every read; mutations dispatch to subscribers. This is the right tool for a retained tree where you want to skip re-running components — but in a frame-rebuild model every signal read is "free" (the function runs unconditionally) and subscriber bookkeeping is pure overhead. Don't bring `Signal`/`Memo` over.
- `deep_clone` on every `Element` returned from a component (`scope_arena.rs:80`) — a cost paid only because the diff later mutates `Cell<MountId>` on the live tree. We have no live tree to mutate.
- Templates+dynamic-slots indirection. Brilliant for retained diffing; pure overhead for a recorder. Our `Tree.shapes` slice already does the "static skeleton" job at the paint layer (each node's shape range is its template).
- Blitz as the "native renderer." Blitz is Servo: Stylo cascade + Taffy layout + Vello paint. If Palantir wanted that, we'd just use Blitz directly — the whole point of this crate is *not* paying for HTML/CSS semantics. Note for contrast: Dioxus has no Dioxus-native layout. They outsourced.
- Async-everywhere. `wait_for_work().await`, `poll_tasks`, `Suspense`, `wait_for_suspense`, `pending_effects`. The runtime is a tokio-style executor for components. Palantir is sync per-frame; the user's event loop drives recording. Anything async (network, file IO) lives outside the UI loop and pokes state via `request_redraw`.
- Multiple renderer backends as a feature gate. Dioxus pays for `WriteMutations` being a trait everywhere; Palantir has exactly one renderer (wgpu) and can keep the paint-pass concrete until that's no longer true.

**Single biggest takeaway.** Dioxus's whole architecture — scope slab, hooks-by-index, reactive contexts, templates, deep-clone, mount cells, `Mutation` stream — is the cost of *not* re-running every component every frame. Each piece is an optimization that recovers some of the work an immediate-mode loop simply doesn't do. Reading Dioxus is the clearest way to see what immediate mode buys you in lines of code that don't have to exist. The parts worth porting (slab+id, generational handles, mutation-as-trait) are the parts that aren't about retained-tree bookkeeping.
