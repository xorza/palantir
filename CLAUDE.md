# Palantir

A Rust GUI crate. **Immediate-mode authoring API**, **WPF-style two-pass layout**, **wgpu rendering**.

Read `DESIGN.md` for the full design rationale before making non-trivial changes.

## Core architecture

Five passes per frame:

1. **Record** — user code (`Button::new().label("x").show(&mut ui)`) appends `Node`s and `Shape`s into an arena `Tree`. No painting yet.
2. **Measure** — post-order. Each node returns desired size given available size.
3. **Arrange** — pre-order. Parent assigns final `Rect` to each child.
4. **Cascade** — pre-order. `Cascades::rebuild` resolves disabled/invisible/clip/transform per node into a flat table; consumed by both encoder and hit-index so they can't drift.
5. **Encode + Paint** — pre-order. `renderer::encode` walks the tree → `Vec<RenderCmd>`; `Composer` groups by scissor and snaps to physical pixels; `WgpuBackend` submits instanced quad draws.

The tree is rebuilt every frame; widget *state* (scroll, focus, animation) lives in a separate `Id → Any` map keyed by `WidgetId` (hashed call-site/user key) — *not yet implemented*.

### Node vs Shape — the key split

- **`Node`** = layout participant. Has style, measured size (on `LayoutResult`, not on `Node`), parent/child links. Lives in `Tree.nodes`.
- **`Shape`** = paint primitive (`RoundedRect`, `Text`, `Line`, …). Stored flat in `Tree.shapes`, sliced per-node via a `Range<u32>` on `Node`. `RoundedRect` always paints the owner's full arranged rect — no per-shape positioning today.

Layout passes ignore Shapes; paint pass ignores hierarchy beyond walking it. This decoupling is load-bearing — keep it.

### Sizing semantics (WPF-aligned)

- `Sizing::Fixed(n)` — outer dimension is exactly `n` (includes padding).
- `Sizing::Hug` — outer dimension = content + padding (WPF's `Auto`).
- `Sizing::Fill(weight)` — take available space, distribute leftover by weight across `Fill` siblings (WPF's `*`).

`resolve_axis_size` in `src/layout/mod.rs` is the canonical implementation; `src/layout/{stack,zstack,canvas,grid}/tests.rs` pin it.

### Tree topology

Linked-list children (`first_child` / `next_sibling`), not `Vec<NodeId>` per node. O(1) append during recording, no per-node allocation. Inspired by Clay (`tmp/clay`) and `indextree`.

## Layout

```
src/
  cascade.rs           per-node disabled/invisible/clip/transform table
  element.rs           UiElement (wide builder) / NodeElement (compact) / UiElementExtras
  shape/               Shape enum (RoundedRect, Line, Text)
  tree/                Tree, Node, NodeId, NodeFlags (bit-packed sense+vis+align), GridDef
  ui/                  Ui recorder, ButtonTheme
  layout/              LayoutEngine, LayoutResult, stack/zstack/canvas/grid drivers
  primitives/          Vec2/Size/Rect/Color/Stroke/Corners/Spacing/Sizing/Track/Align/…
  input/               InputState, HitIndex (O(1) by-id lookup over Cascades output)
  renderer/            encode → compose → wgpu backend, instanced rounded-rect quads
  widgets/             Button, Frame, Panel (HStack/VStack/ZStack/Canvas factories), Grid, Styled mixin

examples/
  helloworld.rs        minimal wgpu-backed driver
  showcase/            multi-page demo of every layout / clip / transform / disabled / button style

scripts/
  fetch-refs.sh        clones reference UI/layout/renderer projects into ./tmp
```

## Reference notes in `./references/`

29 dense per-framework notes plus a cross-cutting synthesis. **Read `references/SUMMARY.md` first** — it indexes every other doc, takes positions on the design choices Palantir must make, and lists anti-patterns + open questions across the corpus. Each per-framework doc cites source code under `tmp/` with `file:line` and ends with explicit copy/avoid/simplify recommendations for Palantir.

Use the SUMMARY's "Quick-lookup matrix" (§13) to find which docs to read for a given task (HStack semantics, text widget, hit-testing, persistent state, etc.).

## Reference sources in `./tmp/`

`./tmp/` is gitignored and populated on demand by `./scripts/fetch-refs.sh` (shallow clones, re-runnable). The `references/*.md` notes already digest these — go to `tmp/` only when a note doesn't cover the specific question.

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

- Early-stage project. No external users, no published API. Prefer correctness, simplicity, and structural improvements over preserving the current API shape — rename, restructure, or break things freely when it makes the code better. Don't add deprecation shims, compatibility aliases, or migration helpers.
- No comments except for non-obvious *why*. Code is short and self-explanatory; keep it that way.
- Default to release `assert!` for invariant checks, not `debug_assert!` — `debug_assert!` is stripped in release and hides logic bugs in the build users actually run. Reserve `debug_assert!` for checks that are genuinely too expensive for release (e.g. O(n) inside a hot loop), and call out the tradeoff when choosing it.
- Edition 2024. Dependencies pinned to `*` for now (lockfile pins actual versions) — fine for prototype, pin before publishing.
- Tests in `lib.rs` pin layout semantics. Add a test whenever you change measure/arrange behavior.
- Don't add wgpu code paths to the layout/tree modules. Renderer goes in its own module when written.
- `WidgetId` is built from a hash of a user-supplied key. Keep IDs stable across frames so persistent state survives.

## Before reporting work as done

Always run, in this order, before confirming any code change:

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Fix anything that fails. Don't tell the user a change is complete unless these all pass.

## Finding duplicated code

Before refactoring or hunting for similar code by reading files, run jscpd — it's fast (~500ms) and avoids burning tokens:

```sh
npm_config_cache="$TMPDIR/npm-cache" npx --yes jscpd src/ --min-lines 5 --min-tokens 50 --ignore "**/tests.rs,**/tests/**" --reporters console
```

Drop the `--ignore` to include tests. Reports exact `file:line` ranges for each clone pair.

## Status

- [x] Geometry, tree, shape, recorder, measure/arrange
- [x] Layouts: HStack, VStack, ZStack, Canvas, Grid (WPF-style tracks + spans)
- [x] Widgets: Button (with state-driven `ButtonStyle`), Frame, Panel, Grid; `Styled` mixin
- [x] Tests pinning Hug/Fixed/Fill, alignment cascade, justify, padding/margin, span, collapsed children
- [x] wgpu paint pass: `WgpuBackend`, instanced rounded-rect quads, scissor + transform composition
- [x] winit event loop integration (showcase example)
- [x] Hit-testing against last frame's rects → `Response { hovered, pressed, clicked }`, with disabled/invisible/clip/transform cascade
- [x] Per-frame `Cascades` table shared by encoder + hit-index
- [ ] Real text measurement via glyphon (today: hardcoded 8px/char placeholder)
- [ ] Glyph atlas + text rendering in the wgpu pipeline
- [ ] Persistent state map (`Id → Any`) for scroll, focus, animation
- [ ] Drag tracking on top of `Active`-capture (rect-independent `drag_delta`)
