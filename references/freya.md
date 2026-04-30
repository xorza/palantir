# Freya — reference notes for Palantir

Freya is a cross-platform native GUI in Rust that renders with Skia. It started as a Dioxus renderer, but as of 0.4 (`tmp/freya/crates/freya/Cargo.toml:2`) it ships its own reactive runtime and only depends on Dioxus through `dioxus-devtools` for hot-reload (`crates/freya-core/Cargo.toml:64`). Layout is done by **Torin**, a custom engine written for Freya — *not* Taffy. The retained-mode tree, custom layout engine, and Skia renderer make it almost the opposite of Palantir's design, which is why it's an interesting reference.

All paths are under `tmp/freya/crates/`.

## 1. Architecture

User code is declarative and component-based: `fn app() -> impl IntoElement` returns a tree built with `rect().width(...).child(...)` builders (`freya/src/lib.rs:19-32`). `Element` (`freya-core/src/element.rs:23`) is the retained tree node; `Component` (same file) is the user-defined function-component.

The runtime — `Runner` in `freya-core/src/runner.rs` — drives a reactive scope graph (`Scope`, `ReactiveContext`, `ScopeId`) very similar to Dioxus's hooks: `use_state`, `use_effect`, etc. live under `freya-core/src/hooks/`. When a signal changes, the runner re-runs only the affected scopes and diffs their returned `Element`s against the previous tree, producing a `DiffModifies` update that mutates the persistent `Tree` (`freya-core/src/tree.rs:76`). The tree is **retained across frames** — only diffs apply.

Per-frame data hangs off `Tree`: `tree.elements`, `tree.layout`, `tree.text_style_state`, `tree.effect_state`, `tree.layers`. Each is keyed by `NodeId`.

## 2. Layout: Torin (custom, two-phase)

Torin (`tmp/freya/crates/torin/`) is a measure-then-arrange engine like WPF, but with a few extras. `Node` (`torin/src/node.rs:20-60`) holds `width`, `height`, `min/max_width/height`, `main_alignment`, `cross_alignment`, `padding`, `margin`, `direction`, `position`, `content`. Sizes are `Size::{Pixels, Percentage, Inner, Fill, FillMin, RootPercentage, ...}` — the equivalents of `Fixed`/`Hug`/`Fill` plus a few extras (`torin/src/values/size.rs`).

`MeasureContext::measure_node` (`torin/src/measure.rs:104`) is the recursive entry. Every call returns `(must_cache, LayoutNode)`. The `Phase` enum (`measure.rs:41`) — `Initial` vs `Final` — exists because some strategies (alignments, content-fit) genuinely need two passes: measure children to know the content size, *then* re-position them with the alignment offset known. This is the same trick we considered for `Sizing::Hug` + alignment in non-trivial cases.

Two important differences from Palantir's plan:

- **Incremental relayout.** Torin keeps a `dirty: FxHashMap<Key, DirtyReason>` (`torin/src/torin.rs:96-103`). `DirtyReason::{None, Reorder, InnerLayout, ...}` lets it skip subtrees whose layout can't have changed. `RootNodeCandidate` (`torin.rs:36`) walks up to find the closest common ancestor of all dirty nodes and re-measures from there — not from the root. Worth it because the tree is retained.
- **Translate-only fast path.** If a node's only change is `offset_x`/`offset_y` (e.g. scroll), `measure_node` (`measure.rs:122-138`) skips re-measuring entirely and calls `recursive_translate` to shift the cached subtree. Cheap scroll without invalidation.

## 3. Renderer: Skia, layered, painter-style

`RenderPipeline::render` (`freya-core/src/render_pipeline.rs:33-60`) iterates `tree.layers` (a `BTreeMap<i16, Vec<NodeId>>`) in z-order, and for each node calls `Element::render(ctx)` which issues Skia draw commands directly into the supplied `Canvas`. Skia handles tessellation, glyph rasterization, blending, clipping, blur — all the "vector + text" hard parts.

Backend is in `freya-engine/src/skia.rs:1-40`: `skia_safe` re-exports plus per-OS GPU backends (`mtl` on macOS, `gl` elsewhere, `vk` on Linux/Windows). `freya-winit` (`freya-winit/src/renderer.rs`) creates the surface and pumps render ticks. There is no own batching, no instancing, no SDF — Skia does all of it. Custom shaders are not really part of the public API.

Cost: Skia is huge (~50 MB linked) and binary distribution is awkward. Benefit: the renderer is essentially "free" — every visual feature is one Skia call away.

## 4. State model

Reactive, not immediate. State lives in `Signal<T>` / `use_state` handles whose `.read()`/`.write()` track read dependencies in a `ReactiveContext`. Mutations enqueue dirty scopes; the runner re-evaluates affected components, diffs the returned `Element` trees, and writes minimal mutations into `Tree`. Identity for diffing is the `DiffKey` (`freya-core/src/diff_key.rs`) — the equivalent of React's `key` prop, used to keep state across reorders.

Hot-reload (`freya-core/src/lib.rs:38`, behind `hotreload` feature) uses `dioxus-devtools` + `subsecond` to swap component bodies live.

## 5. Lessons for Palantir

**Worth copying:**

- **Dirty-set incremental layout.** Torin's `DirtyReason` enum + closest-common-ancestor relayout is the right move *if* Palantir ever moves to a retained tree. As-is, Palantir rebuilds the tree every frame, so this doesn't apply — but if first-frame measure cost ever becomes an issue, persisting the tree and dirtying selectively is the path. Don't take it now; remember it exists.
- **Translate-only fast path for scroll.** Even with a fresh tree per frame, if a `ScrollArea` records the same children at a different offset, a paint-pass shortcut that adds an offset transform instead of re-resolving every shape is appealing. Consider a `Transform` shape that wraps a subtree's paint range.
- **Two-phase measure (`Phase::Initial`/`Final`)** is a real answer to "alignment needs to know content size before placing." Palantir's current measure is single-shot post-order; for `main_alignment: Center` over `Hug` content, we'll need either a final-position pass during arrange or a `Phase::Final` re-measure. Do it in arrange — cheaper.
- **`DiffKey`-style explicit keys.** Even though Palantir is record-every-frame, persistent state still needs stable IDs across frames; the WPF approach of "key = call-site hash + user salt" matches what Freya does for diffing, and stable across reorders is the same problem.

**Avoid (or note as a different design point):**

- **Retained tree with diffing.** Freya pays significant complexity (`Runner`, scope graph, dirty propagation, `DiffModifies`) to support retained mode. Palantir's "rebuild every frame, state separate" is far simpler and matches immediate-mode authoring. The whole `freya-core/src/runner.rs` exists because the tree is retained — we don't need any of it.
- **Skia.** It does everything, but it's a 50 MB C++ blob, awkward to ship, and forecloses on doing typed instanced batches (rounded-rect SDF, glyph atlas) ourselves. Palantir's wgpu+SDF plan is leaner per-rect and gives full pipeline control. The egui-class binary size win (~3 MB total) is worth keeping.
- **Reactive runtime.** Signals/effects are nice for users but require global mutable context (`CurrentContext`, `ReactiveContext`) and async glue. Immediate-mode + ID-keyed `Memory` (egui-style) achieves "state survives frames" with one map and zero scheduler.
- **Separate per-aspect maps in `Tree`** (`elements`, `layout`, `text_style_state`, `effect_state`, `layers`). Palantir's flat `Tree.nodes`/`Tree.shapes` is simpler at our scale. Only consider per-aspect tables if profiling shows cache misses on hot fields.

The high-order takeaway: Freya is the *retained, declarative, do-everything* end of the design space. Reading it is useful precisely as a contrast — every choice in `DESIGN.md` lines up against an alternative Freya already shipped.
