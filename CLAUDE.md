# Palantir

A Rust GUI crate. **Immediate-mode authoring API**, **WPF-style two-pass layout**, **wgpu rendering**.

Read `DESIGN.md` for the full design rationale before making non-trivial changes.

## Core architecture

Four passes per frame:

1. **Record** — user code (`Button::new(id).label("x").show(&mut ui)`) appends `Node`s and `Shape`s into an arena `Tree`. No painting yet.
2. **Measure** — post-order. Each node returns desired size given available size.
3. **Arrange** — pre-order. Parent assigns final `Rect` to each child.
4. **Paint** — pre-order. Walk tree, resolve each shape's offset against its owner's `Rect`, batch into wgpu draws. *Not yet implemented.*

The tree is rebuilt every frame; widget *state* (scroll, focus, animation) lives in a separate `Id → Any` map keyed by `WidgetId` (hashed call-site/user key).

### Node vs Shape — the key split

- **`Node`** = layout participant. Has style, measured size, final rect, parent/child links. Lives in `Tree.nodes`.
- **`Shape`** = paint primitive (`RoundedRect`, `Text`, `Line`, …). Owner-relative position. Stored flat in `Tree.shapes`, sliced per-node via `shapes_start..shapes_end`.

Layout passes ignore Shapes; paint pass ignores hierarchy beyond walking it. This decoupling is load-bearing — keep it.

`ShapeRect::Full` is a sentinel meaning "use my owner's full arranged rect," resolved at paint time. Lets shapes be declared before the node has a size.

### Sizing semantics (WPF-aligned)

- `Sizing::Fixed(n)` — outer dimension is exactly `n` (includes padding).
- `Sizing::Hug` — outer dimension = content + padding (WPF's `Auto`).
- `Sizing::Fill` — take available space, distribute leftover equally across `Fill` siblings (WPF's `*`).

If you change `resolve_axis` in `src/layout.rs`, re-run the lib tests — they pin this contract.

### Tree topology

Linked-list children (`first_child` / `next_sibling`), not `Vec<NodeId>` per node. O(1) append during recording, no per-node allocation. Inspired by Clay (`tmp/clay`) and `indextree`.

## Layout

```
src/
  geom.rs       Vec2, Size, Rect, Color, Stroke, Sizing, Spacing, Style
  shape.rs      Shape enum + ShapeRect::{Full, Offset}
  tree.rs       Tree, Node, NodeId, WidgetId, ChildIter
  ui.rs         Ui recorder (parent stack, begin_node/end_node, container helper)
  layout.rs     measure + arrange + HStack/VStack drivers
  widgets/
    button.rs   Button::new(id).width(x).label(s).show(&ui) → Response
  lib.rs        re-exports + unit tests pinning layout semantics

examples/
  helloworld.rs minimal driver (no wgpu yet — prints arranged rects)

scripts/
  fetch-refs.sh clones reference UI/layout/renderer projects into ./tmp
```

## Reference sources in `./tmp/`

`./tmp/` is gitignored and populated on demand by `./scripts/fetch-refs.sh` (shallow clones, re-runnable). Use it for cross-checking design decisions against real codebases instead of guessing or web-searching.

Most relevant when working on:

- **Layout / measure-arrange** → `tmp/wpf` (the model we emulate), `tmp/taffy`, `tmp/morphorm`, `tmp/yoga`, `tmp/clay` (arena tree)
- **Immediate-mode patterns** → `tmp/egui`, `tmp/imgui`, `tmp/clay`, `tmp/nuklear`
- **wgpu renderer / batching** → `tmp/egui` (`crates/egui-wgpu`), `tmp/iced` (`wgpu` crate), `tmp/quirky`, `tmp/vello`, `tmp/wgpu`
- **Text** → `tmp/glyphon`, `tmp/cosmic-text`, `tmp/parley`
- **Vector shapes** → `tmp/lyon`, `tmp/kurbo`, `tmp/vello`
- **Reactive / retained Rust UIs for contrast** → `tmp/iced`, `tmp/xilem`, `tmp/dioxus`, `tmp/floem`, `tmp/slint`, `tmp/makepad`

If the directory is missing or stale, run the script before doing research:
```sh
./scripts/fetch-refs.sh
```

## Conventions

- No comments except for non-obvious *why*. Code is short and self-explanatory; keep it that way.
- Edition 2024. Dependencies pinned to `*` for now (lockfile pins actual versions) — fine for prototype, pin before publishing.
- Tests in `lib.rs` pin layout semantics. Add a test whenever you change measure/arrange behavior.
- Don't add wgpu code paths to the layout/tree modules. Renderer goes in its own module when written.
- `WidgetId` is built from a hash of a user-supplied key. Keep IDs stable across frames so persistent state survives.

## Status

- [x] Geometry, tree, shape, recorder, measure/arrange, Button, HStack/VStack
- [x] Lib tests covering Hug/Fixed/Fill on both axes
- [ ] Real text measurement via glyphon
- [ ] wgpu paint pass (RoundedRect SDF shader, glyph atlas, instanced quads)
- [ ] winit event loop integration
- [ ] Hit-testing against last frame's rects → `Response { hovered, clicked, ... }`
- [ ] More layouts: Grid, Dock, Canvas
- [ ] Persistent state map (`Id → Any`)
