# Slint — reference notes for Palantir

Slint is a retained, declarative, reactive Rust GUI toolkit fronted by a custom DSL (`.slint`). It sits at the opposite end of the design space from Palantir: a compiler chews `.slint` files into a typed item tree of `Property<T>`s with a fine-grained dirty-tracking dependency graph; user Rust code binds callbacks and reads/writes properties; the runtime re-evaluates only the sub-bindings whose inputs changed and asks a pluggable `Renderer` to repaint just the dirty regions. Worth a careful read because the *layout solver* (a pure function over `LayoutInfo` constraints) is portable, the *property/dependency engine* is the real reactive analogue to egui's `IdTypeMap`, and the *partial renderer* is what an immediate-mode codebase eventually wishes it had.

All paths under `tmp/slint/`.

## 1. The `.slint` DSL — compile-time tree, runtime properties

There is no runtime DSL parser in user binaries. `slint-build`/`slint!` macro invokes the compiler crate (`internal/compiler/`) at build time. Pipeline (see `internal/compiler/passes/` — 40+ ordered passes):

- `parser/` → AST. `typeloader.rs` resolves imports and `langtype.rs` types. `object_tree.rs` is the high-level IR — a tree of `ElementRc` with named `Property<T>` slots and `Expression` bindings.
- `passes/lower_layout.rs` (1716 lines) rewrites `HorizontalLayout`/`GridLayout`/`VerticalLayout` syntax into a synthetic element with computed `x`/`y`/`width`/`height` *bindings* on its children. Layout in Slint is just sugar for property bindings calling `solve_box_layout` / `solve_grid_layout`. After this pass, layout is no longer a tree concept — it's a property graph.
- `passes/inlining.rs`, `const_propagation.rs`, `binding_analysis.rs` (computes the static dependency graph between properties to detect cycles and constant-fold), `lower_states.rs`, `lower_popups.rs`, … each pass is an `O(tree)` walk that mutates `object_tree::Document`.
- `llr/` (Low-Level Representation) flattens to `llr/item_tree.rs` — sub-components, sorted property lists, repeater expansions. `generator/{rust,cpp,llr}.rs` emits source (Rust struct with `Property<T>` fields and `init` closures, or C++).

So at runtime, `.slint` syntax has been *erased*. What you have is a generated `struct Foo { x: Property<f32>, … }` with `Foo::new()` setting up bindings. Callbacks (`callback clicked()` in DSL) become `Callback<Args, Ret>` fields — same machinery as `Property` but without a stored value. Signals are not an additional concept.

This bake-out is the entire architectural argument for the DSL: **the compiler can perform whole-program binding analysis**. `binding_analysis.rs` builds a graph of property → property dependencies, detects which properties are constant (folded), which form cycles (errored or two-way), and which are pure (can be inlined). An immediate-mode equivalent has none of this — every read/write is opaque to the framework.

## 2. Layout: `LayoutInfo` constraints, pure-function solvers

`internal/core/layout.rs:24-50` is the contract:
```
LayoutInfo { min, max, min_percent, max_percent, preferred, stretch }
```
Six numbers per child per axis. `min`/`max` clamp; `preferred` is the desired natural size; `stretch` is the share weight when leftover space is distributed (Qt's `QSizePolicy` directly). `merge` (line 55) is `(min.max, max.min, pref.max, stretch.min)` — used both for combining wrapper constraints and for two-pass propagation up the tree.

The actual solver, `grid_internal::layout_items` (`layout.rs:268`), is the load-bearing 20-line function:
1. Set every cell's `size = pref`, sum to `pref_total`.
2. If `available >= pref_total`, call `adjust_items::<Grow>` (line 206); else `adjust_items::<Shrink>`.
3. `adjust_items` is a stretch-weighted distribution loop: each iteration finds `max_grow = min(can_grow / stretch)` across cells with headroom, distributes `min(to_distribute / total_stretch, max_grow) * stretch` per cell, repeats until `to_distribute <= 0` or no cell can grow. Bounded by min/max. Integer-pixel rounding handled at line 253. **This is exactly Palantir's `Sizing::Fill` distribution — done in one place, agnostic of orientation.**

Higher-level entry points are pure functions over `Slice<LayoutItemInfo>`:
- `solve_box_layout(data, repeater_indices) -> SharedVector<Coord>` (line 1203) — outputs `[pos0, size0, pos1, size1, …]`. Picks `Stretch`/`Center`/`Start`/`End`/`SpaceBetween`/`SpaceAround`/`SpaceEvenly` (lines 1234-1271) as a *post-process* on top of `layout_items`. Same model as WPF: solver fills, alignment translates leftover.
- `solve_grid_layout` (line 1021) — does row + column independently using the same `layout_items` over `to_layout_data` (line 316), which collapses cells into per-row/per-column `LayoutData` with min = max-of-cell-mins, max = min-of-cell-maxes, etc. Spans (line 374-426) re-run `layout_items` three times across the spanned cells (once for min, max, preferred) to project a multi-cell constraint back onto its parts. No iterative cycle-breaking like WPF Grid — Slint's grammar disallows the `*` ↔ `Auto` cross-axis ambiguity that forces WPF's loop.
- `solve_flexbox_layout` (line 1748), `solve_flexbox_layout_with_measure` (line 1774) — full CSS flexbox with wrap, `align-self`, `flex-grow`/`shrink`/`basis`. Used by the `Flickable` and the new flex syntax.

Constraint propagation upward: `box_layout_info` (line 1324) and `box_layout_info_ortho` (line 1353) sum/max children's `LayoutInfo`s and return the parent's. Each layout element exposes a `layoutinfo_h` and `layoutinfo_v` *property* whose binding calls one of these. Because they're properties, dirty tracking is automatic — change a child's `min-width`, the parent's `layoutinfo_h` invalidates, every layout property of children recomputes lazily on next paint.

The solver is **pointer-and-orientation-pure**: takes `Slice`, returns `SharedVector`. No tree walk, no recursion. The recursion is implicit in the property graph: child layout properties depend on parent allocated size which depends on grandparent's layout solve, etc.

## 3. Property/dependency engine — the reactive core

`internal/core/properties.rs` (1504 lines) is a hand-rolled, `unsafe`-heavy intrusive linked-list dependency tracker. Worth understanding because this is what "signal-style" actually costs.

**`Property<T>`** (`properties.rs:845`):
```
struct Property<T> {
    handle: PropertyHandle,  // tagged ptr: binding | dep-list-head
    value: UnsafeCell<T>,
    pinned: PhantomPinned,
}
```
The `PropertyHandle` (line 504) is a tagged `usize`: low bits flag "borrowed" (`0b01`) and "points to a binding" (`0b10`). Either it's the head of an intrusive doubly-linked list of *dependents* (other bindings that read me), or it's a `*mut BindingHolder` (my own binding closure) and *that* holds the dep list.

**`BindingHolder`** (`properties.rs:397`) is the type-erased binding: a `vtable` pointer, a `dirty: Cell<bool>`, and the user closure. Allocation goes through `alloc_binding_holder` (line 429) which builds a `&'static BindingVTable` for each closure type via the `HasBindingVTable` const trait (line 478). Drop, evaluate, mark_dirty, intercept_set — all dispatched through the vtable so the binding-holder layout is `T`-independent.

**Dependency tracking**: a thread-local `CURRENT_BINDING` (line 380). When `Property::get` runs (line 915), it calls `register_as_dependency_to_current_binding` which, if a binding is currently evaluating, pushes a `DependencyNode` linking the *current binding* into *this property's dependency list*. The list is intrusive — `DependencyNode` is owned by the binding (via `dep_nodes: SingleLinkedListPinHead<DependencyNode>` on `BindingHolder`, line 402) but threaded through the property's list. Drop the binding and its nodes auto-unlink. This is why everything is `Pin` and `unsafe`.

**Set/dirty propagation**: `Property::set` (line 964) walks the dep-list-head and marks every dependent binding dirty (`mark_dirty` vtable call). Bindings are *not* re-evaluated eagerly — the next `get` of a dirty binding's property triggers `update` which calls `evaluate`. Lazy pull, eager invalidate.

**`PropertyTracker`** (line 1184) is the same machinery exposed for "tell me when any of these properties change" — used by the partial renderer (one tracker per item, dirty ⇒ region needs repaint).

Two-way bindings (`internal/core/properties/two_way_binding.rs`) and animations (`properties_animations.rs`) plug in via the `intercept_set`/`intercept_set_binding` vtable slots.

Cost: every `Property<T>` is at least `usize + T + PhantomPinned` plus, when bound, a heap `BindingHolder<Closure>`. Every `get` does a thread-local check + atomic-feeling Cell ops. Every `set` walks a linked list. For the property volume Slint targets (hundreds per window), it's fine. For Palantir's per-frame-rebuild model, it would be both wasted work and architecturally redundant.

## 4. Renderer abstraction — pluggable `RendererSealed` + partial repaint

`internal/core/renderer.rs:26` defines `RendererSealed`: text measurement, font registration, scale factor, snapshot, `mark_dirty_region`. That's it — the trait is small because rendering is two-stage.

Stage 1: walk the item tree, calling **`ItemRenderer`** (`item_rendering.rs:441`) — `draw_rectangle`, `draw_border_rectangle`, `draw_image`, `draw_text`, `draw_text_input`, `draw_path`, `draw_box_shadow`, `combine_clip`, `save_state`/`restore_state`, `translate`/`rotate`/`scale`, `apply_opacity`. Each backend implements this trait against its own canvas.

Stage 2: a `Renderer` (one of three) drives the walk:
- **Software** (`internal/renderers/software/`) — a from-scratch CPU rasterizer producing a `SharedPixelBuffer<Rgba8Pixel>`. Targets MCUs (`#![no_std]`), no GPU dependency. Exists because the toolkit aims at embedded.
- **FemtoVG** (`internal/renderers/femtovg/`) — OpenGL/wgpu via the `femtovg` crate (NanoVG-style, tessellated triangles, glyph atlas). `wgpu.rs` is the wgpu surface adapter; `itemrenderer.rs` implements `ItemRenderer` by translating each call into femtovg path/fill/stroke ops.
- **Skia** (`internal/renderers/skia/`) — `skia-safe` bindings, with surface backends for Metal, Vulkan, D3D, OpenGL, software, and wgpu (`wgpu_27_surface.rs`, `wgpu_28_surface.rs`, `wgpu_renderer.rs`). The most featureful and the heaviest dependency.

The choice is at compile time via Cargo features. The `ItemRenderer` trait is the seam — adding a backend means implementing ~25 methods and providing surface setup.

**Partial rendering** (`internal/core/partial_renderer.rs`): wraps any `ItemRenderer` with a `PartialRenderer<T>` that maintains a `CachedRenderingData` per item (`cache_index`, `cache_generation`) and a `PropertyTracker` per item. Algorithm (header doc, line 1-15):

1. `compute_dirty_regions` walks the tree; if the item's bounding box changed *or* its render-property tracker is dirty, union into the dirty region. This pass also re-registers dependencies on geometry and on still-clean trackers.
2. `filter_item` is called for each item by the inner walk; uses cached geometry, no new dependencies registered.
3. Only items inside the dirty region get `draw_*` called; their tracker re-arms during draw so next frame catches changes.

This is the prize: ~zero work on idle frames, repaint scales with *what changed* not tree size. Possible only because every render-relevant input is a tracked `Property`.

## 5. Memory model — generated structs, item-tree vtables, repeaters

The compiler generates one Rust struct per `.slint` component (`Foo`, `FooInner`). User code obtains a `Rc<FooInner>` via `Foo::new()`. Inside: every property is a `Property<T>` field; every callback is a `Callback<…>`; every child sub-component is another generated struct, embedded inline or in a `Vec` for repeated rows.

The runtime navigation layer is **`ItemTreeVTable`** (`internal/core/item_tree.rs:47`): `visit_children_item`, `get_item_ref`, `get_subtree_range`, `get_subtree`, `get_item_tree`, `parent_node`. Each generated component implements this vtable so the framework can walk the tree without monomorphizing per component. `vtable::VRef`/`VRefMut` (the `vtable` helper crate) keeps it FFI-safe — same vtable layout in C++ generated code.

Mutation flow:
1. User writes `foo.set_label("hi")` — generated setter calls `Property::set` on the `label` field.
2. `set` invalidates dependents (computed properties, layout `width`/`height` bindings that read `label`'s text size, …).
3. Next paint pass: `PartialRenderer::compute_dirty_regions` finds the dirty trackers, unions a region.
4. Renderer paints just that region, pulling property values lazily, evaluating bindings on demand.

Ownership is explicit: the user holds `Foo`, which owns the entire tree (sub-components are inline or `Rc`-pinned in `Vec`s for repeaters). Repeaters (`for x in model: …` in DSL) materialize as `Repeater<SubComponent>` (`internal/core/items/`) backed by a `Model<T>` trait — push/pop on the model fires fine-grained inserts on the repeater, which allocates/drops sub-components without rebuilding siblings. No tree rebuild per frame, ever.

## 6. Why retained + reactive (per Goffart's blog/FAQ)

Slint's design notes (`README.md`, `FAQ.md`, blog posts at `slint.dev`) argue:
- **MCU target**: Slint runs on a 200 KB Cortex-M with no allocator. Per-frame tree rebuild + heap churn is impossible there. Retained + dirty-region partial repaint is the *only* shape that fits.
- **Designer tooling**: `.slint` files are editable in a live preview (`tools/slintpad`, VS Code extension). A compiled DSL with named properties is hugely easier to round-trip with a visual editor than a Rust expression DAG.
- **C++ / Node / Python parity**: the DSL is the lingua franca; each language gets generated bindings. An IM API would need to be reinvented per language and would lose the live-preview story.
- **Static binding analysis**: cycles caught at compile time, constant folding, reachability. Possible only because the binding graph is a first-class compiler input.

QML is the explicit prior art (Goffart wrote much of QtQuick). Slint's argument vs QML: pre-compile the DSL to native (no JS runtime), make property bindings `unsafe` Rust instead of QObject moc gunk, target MCU.

## 7. Lessons for Palantir

**Copy.**
- `LayoutInfo { min, max, min_percent, max_percent, preferred, stretch }` as the canonical per-child constraint bundle (`layout.rs:24`). Cleaner than carrying `Sizing` enum + style fields separately at the layout call. `merge` semantics (line 55) for combining wrapper + child are exactly what we want when collapsing margin/min-max into the panel's slot computation. Six floats per child per axis, copy-by-value, no allocations.
- The `grid_internal::layout_items` algorithm (`layout.rs:206-287`) — stretch-weighted grow/shrink with min/max clamps — is a drop-in replacement for our `Sizing::Fill` distribution. It handles the integer-pixel-rounding edge case (line 253: "less than a pixel per element, give it to max-stretch"), which our naive `available / fill_count` doesn't. Port `LayoutData` + `Grow`/`Shrink` + `adjust_items` + `layout_items` near-verbatim. ~80 lines, pure, well-tested (test at line 289).
- Solver-as-pure-function: `solve_box_layout` (`layout.rs:1203`) takes `Slice<LayoutItemInfo>` and returns `SharedVector<Coord>` of `[pos, size, pos, size, …]`. No tree, no walk. Mirror this — `fn arrange_box(items: &[Constraint], available: f32, spacing: f32, padding: Padding, alignment: Alignment) -> Vec<(f32, f32)>`. Makes the layout testable in isolation and shareable between HStack/VStack/Grid-row/Grid-column.
- Alignment-as-translation post-step (`layout.rs:1234-1271`). Solver always produces `Stretch`-style filled positions; `Start`/`Center`/`End`/`SpaceBetween`/`SpaceAround`/`SpaceEvenly` are computed by adjusting `(start_pos, spacing)` after the fact. Same as WPF and exactly the design Palantir already commits to.
- The `ItemRenderer` trait shape (`item_rendering.rs:441`) as a *seam* between layout-walk and pixel-emission. We don't need pluggable renderers, but having paint-pass code consume an `impl PaintSink` instead of a concrete wgpu encoder makes it straightforward to add a software/snapshot backend for tests. ~10 methods is enough.
- Partial-render concept (`partial_renderer.rs`): "compute dirty regions, only repaint inside." Out of scope for v1, but design the paint pass so it *could* be wrapped — emit per-node draw lists keyed by `WidgetId` and a `dirty_region: Rect` filter is a future bolt-on.

**Avoid.**
- The whole DSL + compiler stack. Slint is ~1700 lines of `lower_layout.rs` alone, plus 40 ordered passes, plus LLR, plus codegen. Palantir's value proposition is *Rust-native, immediate-mode*; reproducing `.slint` would erase that.
- `Property<T>` + intrusive dep lists (`properties.rs`, 1504 lines, mostly `unsafe`). The reactive engine pays its way only because a) bindings persist across frames (we rebuild every frame so persistence is irrelevant) and b) partial repaint exists. With record-then-layout each frame, the natural dependency is "did the recording differ?" — answerable by hashing or by trusting the user. The `unsafe` PropertyHandle bit-tagging, intrusive linked lists, thread-local CURRENT_BINDING, vtable for type erasure — all unnecessary infrastructure for an IM model.
- `vtable::VRef` / `ItemTreeVTable` (`item_tree.rs:47`). Slint needs these because items are heterogeneous and the tree walk happens in framework code that doesn't know the user's component type. Palantir's `Tree<Node>` is homogeneous — `Node` is a single struct with style, links, slice indices. Plain enum dispatch on shapes; no vtable.
- Multiple renderer backends (software + femtovg + Skia + 6 surface variants). We commit to wgpu. One backend, instanced SDF rounded-rects, glyphon for text. Refusing the abstraction tax is the point.
- `solve_flexbox_layout_with_measure` (`layout.rs:1774`). Full CSS flexbox is huge and most apps don't need wrap or `align-self` per-child. Stick with WPF-style HStack/VStack + Sizing::Fill until profiling justifies more.
- Per-item `CachedRenderingData` + `PropertyTracker` machinery for partial render. Premature for a tree we rebuild from scratch.

**Simplify.**
- Slint splits the layout call site (compiler-emitted property binding that calls `solve_box_layout`) from the solver (pure function over `Slice`). Palantir can collapse: panel's `arrange_children` *is* the solver, taking `&[Node]` and writing `Rect`s back. No property graph, no codegen. Same algorithm, half the surface area.
- `LayoutInfo` carries six fields; we can start with four (`min`, `max`, `preferred`, `stretch`) and add `*_percent` only if a real call site needs it. The `_percent` fields exist in Slint because `width: 50%` is valid DSL syntax — irrelevant when sizing is a Rust enum.
- Slint's grid solve runs `layout_items` separately for rows and columns and re-runs three times across span-groups (`layout.rs:392, 404, 412`) to project span constraints. If/when we add Grid, copy the per-axis decomposition but skip spans in v1 — most grids are dense and rectangular.
- The `Renderer` trait in slint has 20+ methods (font registration, snapshots, dirty-region marking, scale factor, multiple `register_font_*` variants for embedded). Our paint sink can have ~6 methods (`fill_rect`, `stroke_rect`, `draw_text`, `push_clip`, `pop_clip`, `flush`). Everything else is a wgpu detail.

**Single biggest takeaway:** Slint's layout solver is a clean, portable artefact buried inside a much larger reactive framework. The solver *doesn't depend on `Property<T>`* — `solve_box_layout` takes a `Slice<LayoutItemInfo>` and returns positions. Palantir can lift `LayoutInfo` + `layout_items` + `solve_box_layout`'s alignment post-step almost verbatim and inherit a battle-tested distribution algorithm without taking on any of the retained/reactive baggage. The DSL, the property engine, the partial renderer, the multi-backend `ItemRenderer` — all are answers to problems (MCU target, tooling round-trip, C++ FFI, idle-frame cost on retained trees) that an immediate-mode wgpu-only Rust crate doesn't have.
