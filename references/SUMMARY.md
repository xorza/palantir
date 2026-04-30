# References — Cross-Cutting Summary

This is the index over the 29 reference notes in `references/`. Each note dissects one prior-art project; this synthesizes what they say *together*. References use `[name.md §N]` shorthand.

## 1. Intro

The corpus covers four orthogonal axes a Rust GUI library has to commit on:

- **Authoring model.** From pure cursor-walk immediate-mode (raygui, imgui, nuklear, egui) through deferred-IM-with-tree (clay, Palantir) to retained widget trees (druid, masonry/xilem, vizia, floem, freya, slint, makepad, bevy_ui, iced, flutter), VDOM-diffed (dioxus), and DSL/codegen (slint, makepad).
- **Layout.** WPF measure/arrange (single `available` + min/max wrapper), Flutter `BoxConstraints` (min/max in both axes, single recursion), Yoga/Stretch/Taffy multi-pass flex with per-node caches, Morphorm's parent-recursive single-pass, Clay's three-axis size + DFS height-prop, egui's no-measure cursor + prior-frame fallback, Slint's pure-function `solve_box_layout`, Makepad's turtle with back-patch, raygui's "user supplies a Rect."
- **Rendering.** CPU tessellation into one mesh (egui), instanced typed batches with SDF (iced, makepad, quirky, planned for Palantir), GPU compute path-renderer (vello/masonry), Skia (freya), pluggable backends (slint, floem). Plus the pure-data layer: shape/draw command arrays (clay, nuklear, egui shapes).
- **Text.** Glyphon (cosmic-text + wgpu atlas), parley (shape-once / layout-many over harfrust+icu+fontique), cosmic-text (incremental editor-shaped buffer), makepad's in-house slug/SDF stack. All decompose into shape → line-break → glyph atlas / glyph quads.
- **State / identity.** ID-keyed external maps (egui `IdTypeMap`, imgui `ImGuiStorage`, nuklear `nk_table`, planned Palantir `Id → Any`); `WidgetPod`/`WidgetState` retained alongside the tree (druid, masonry); `ECS Entity` (bevy_ui); slotmaps + scope owners (floem); generational-box handles (dioxus); raw `Rectangle` (raygui — anti-pattern).

Palantir's chosen point: **deferred-IM with a per-frame arena tree, WPF-style two-pass measure/arrange, an `Id → Any` state map, instanced-batch wgpu renderer, cosmic-text/glyphon for text.** Closest existing design is Clay [clay.md §1-9]; closest in-process rejection is egui's "no real measure pass, fall back on prior frame + `request_discard`" [egui.md §1, §7]. Floem and Xilem are the strongest examples of the alternative branch (retained tree + signals or rebuild-views-diff-widgets) and explain in code what we're choosing not to pay for [floem.md §6, xilem.md §10].

## 2. Authoring model spectrum

A linear taxonomy from "draw immediately" to "compile a DSL":

1. **Pure IM, no layout.** raygui [raylib.md §2]: every widget takes a `Rectangle`, the user *is* the layout engine. raygui's `RAYGUI_STANDALONE` mode (four function pointers) is the floor of what a UI library can be [raylib.md §5]. Confirms by negation that any ergonomic IM library needs *some* layout primitive.
2. **IM with cursor-walk.** imgui [imgui.md §2-3], egui [egui.md §1], nuklear [nuklear.md §3]. Widgets paint as they're called; layout is "advance a cursor" or "fill a row of slots." Identity is a hashed call-site stack (imgui `PushID`, nuklear `nk_murmur_hash(name, line)`, egui `Id::new` + `IdMap`). All three pay a *two-frame autofit* tax [imgui.md §3, egui.md §1]. Egui's `request_discard` + `sizing_pass` is the polished version [egui.md §1, §7].
3. **Deferred-IM with arena tree.** Clay [clay.md §2-4], Palantir. Record into a flat arena, run multi-pass layout, emit shape commands, paint. Identity is hashed parent-id + user string. Cost of a third-pass-for-text-wrap is fundamental [clay.md §4, §9].
4. **Retained widget tree, mutate via state binding.** Druid [druid.md §1-4], WPF [wpf.md §1-5], Iced [iced.md §1-2], Vizia [vizia.md §1-2], Bevy UI [bevy_ui.md §1-2]. Widget instances live across frames. Druid uses `Data + Lens` for change detection; vizia migrated `Lens → Signal` [vizia.md §2] for the same reason floem started there [floem.md §2]. Iced rebuilds the widget tree in `view()` each frame but persists `widget::Tree` state by `Tag` [iced.md §1] — closer to record-then-layout than druid is.
5. **Retained tree + fine-grained reactivity (signals).** Floem [floem.md §2], Vizia post-migration [vizia.md §2], Freya [freya.md §4]. Signals own state; effects translate signal changes into typed messages on retained `View`s; a dirty bit triggers minimal re-passes. The runtime is always a thread-local `!Send` `Runtime` [floem.md §2, slint.md §3]. Slint pushes this furthest with `unsafe` intrusive linked-list dependency tracking [slint.md §3].
6. **Two-tree: declarative `View` builds/diffs retained `Widget`.** Xilem [xilem.md §1-2]. User authors thin transient `View`s; framework diffs into masonry `Widget`s with `WidgetState` and `MeasurementCache`. Levien's escape from druid's lens system [druid.md §6, xilem.md §1] — retained side stays, users don't author it.
7. **VDOM diff.** Dioxus [dioxus.md §1-5]. Component returns `Element`; `VirtualDom` diffs against last frame's; renderer (Blitz / DOM / TUI) consumes a `Mutation` stream. Hooks-by-call-order, generational-box for state, `&'static Template` skeletons make the diff pointer-equality on the static parts.
8. **DSL / compiled.** Slint [slint.md §1], Makepad [makepad.md §1]. Separate language describes the tree; compiler emits Rust structs of `Property<T>` (slint) or `script_mod!` blocks (makepad). Whole-program binding analysis [slint.md §1]. Hot-reload becomes natural at the cost of inventing parser, IR, codegen.

**Convergent insight.** Identity is *positional + user-keyed*, no matter where on this spectrum: imgui hashed `PushID` stack [imgui.md §4], nuklear `nk_murmur_hash(name, seed)` [nuklear.md §4], egui `Id::with` [egui.md §2], clay parent-seeded FNV-1a `CLAY_ID_LOCAL` [clay.md §8], xilem `ViewId` path with generation bumps for variant changes [xilem.md §8], dioxus `node_paths` byte arrays [dioxus.md §2], floem slotmap `ViewId` [floem.md §1] — all the same insight in different syntax. Palantir's `WidgetId = hash(call-site + user key)` is the right shape [CLAUDE.md "Conventions"].

**Divergent insight.** Where state lives splits the field three ways:
- *External `Id → Any` map*, separate from tree (egui `IdTypeMap` [egui.md §3], imgui `ImGuiStorage` [imgui.md §8], nuklear `nk_table` pages [nuklear.md §5], **Palantir's plan** [DESIGN.md §4]).
- *On the persistent widget*, mutated via lenses or signals (druid `WidgetPod` [druid.md §3], masonry `WidgetState` [xilem.md §4], floem `ViewState` [floem.md §1]).
- *In the runtime*, keyed by scope or generation handle (dioxus `Scope` + generational-box [dioxus.md §1, §3], slint `Property<T>` linked into the dependency graph [slint.md §3]).

Palantir's choice (external map) is what every IM toolkit ends at because it survives tree rebuilds with no bookkeeping. Druid's `Data + Lens` and vizia's pre-migration `Lens` both ended in production failure on `Vec<T>` lensing [druid.md §5, vizia.md §2].

## 3. Layout

### Constraint shape

- **WPF: `available: Size` + framework-applied min/max wrapper around `MeasureOverride`** [wpf.md §1-2]. Single available size top-down, `Infinity` on an axis means "report intrinsic." Min/max/width are properties resolved by `FrameworkElement.MeasureCore` *outside* the panel's own logic. **Palantir's model.**
- **Flutter: `BoxConstraints { min, max }` per axis** [flutter.md §2, druid.md §2]. Both bounds travel in one bundle; child must satisfy. Forbids "give me child's intrinsic before constraining" — that requires the slow `IntrinsicWidth`/`IntrinsicHeight` API and is documented as O(n²) worst case [flutter.md §5].
- **Masonry: `LenReq = MinContent | MaxContent | FitContent(f64)`** [xilem.md §3]. Splits WPF's `Infinity`-as-intrinsic into the three CSS keywords; explicitly *not* a Flutter min/max constraint. Most refined version of WPF model in active development.
- **Yoga/Taffy/Stretch: `(known_dimensions, available_space: Definite | MinContent | MaxContent)`** [taffy.md §3, yoga.md §2-5, stretch.md §3]. Required because flex children get queried under multiple sizing modes during one resolution.
- **Morphorm: parent-axis `LayoutType` + child `Units`** [morphorm.md §1-2]. No constraint object at all; parent picks an axis, each child has a unit, clamping is per-axis.
- **Iced: `Limits { min: Size, max: Size, compression: Size<bool> }`** [iced.md §3]. WPF + Flutter hybrid; `compression` flag distinguishes "shrink-to-content" from "fill-available" without a separate `Sizing` enum on the child.
- **Slint: `LayoutInfo { min, max, min_percent, max_percent, preferred, stretch }`** [slint.md §2]. Six floats per child per axis, copy-by-value — most useful constraint bundle in the corpus, because it carries everything `solve_box_layout` needs without consulting children's properties.

**Position.** Palantir uses WPF's `available + Sizing` because Palantir already commits a per-axis sizing policy at recording time [CLAUDE.md "Sizing semantics"]. `BoxConstraints` and `Limits` are richer but their extra information (a propagated `min`) duplicates what `Sizing::Fixed`/`Hug`/`Fill` already encode. Slint's `LayoutInfo` is the right *internal* representation when layout logic is pulled into a pure function [slint.md §2, §7].

### Pass shape

- **WPF two-pass.** Post-order `Measure(available) → desired`, pre-order `Arrange(finalRect)`. Idle frames cheap because `Measure` short-circuits on `_previousAvailableSize` equality [wpf.md §1, §3]. **Palantir's chosen shape.**
- **Flutter one-recursion-with-constraints.** `performLayout()` reads `this.constraints`, calls `child.layout(childC, parentUsesSize)`, sets size, walks children to assign positions [flutter.md §2]. "One pass" is misleading: position assignment is a second walk, just inside the same `performLayout`.
- **Yoga `compute_internal`.** Nine numbered steps, recursive re-entry under different `SizingMode`s, 8-slot per-node measurement cache [yoga.md §3, §5]. Without the cache it's exponential in depth.
- **Stretch.** Same algorithm as Yoga, single-slot cache, did not work [stretch.md §3, §5]. Cautionary.
- **Taffy.** Same family, 9-slot cache keyed by `compute_cache_slot(known_dimensions, available_space)` [taffy.md §4]. Tolerant lookup via `is_roughly_equal`. Cache *persists across runs* if host keeps `Cache` alive.
- **Morphorm single-pass-with-recursion-on-stretch.** Non-stretch children resolve in one walk; stretch children get a deferred second walk [morphorm.md §4]. No input-keyed cache.
- **Clay multi-pass.** Bottom-up at close-time → top-down X → text-wrap → height-propagate DFS → top-down Y → aspect-ratio width → position+emit [clay.md §4]. The height-prop DFS is *required* once text wrap depends on resolved width; **Palantir cannot ship multi-line text without adding it.**
- **egui no-measure single-pass.** Cursor-walk; widgets paint as they go; "did you fit?" is checked against last frame's `WidgetRects` [egui.md §1, §4]. First-frame jitter, papered over by `request_discard` [egui.md §1].
- **Makepad turtle with back-patch.** Single-pass single-recursion, but `Fit` containers' background instance buffers are mutated post-hoc when the turtle ends [makepad.md §8].
- **Slint pure-function solver.** `solve_box_layout(items, available, …) -> Vec<(pos, size)>` is called from a property binding. The recursion is implicit in the property graph [slint.md §2].

### Caching

- **WPF.** `IsMeasureValid + DoubleUtil.AreClose(_previousAvailableSize)` short-circuit at top of `Measure` [wpf.md §1, §3]. `OnChildDesiredSizeChanged` propagates deltas upward.
- **Yoga.** 8 measurement slots + 1 layout slot per node, generation counter, `markDirtyAndPropagate` [yoga.md §3-4].
- **Stretch.** 1 slot. Worse than no cache because intra-frame flex re-measure thrashes [stretch.md §3, §5].
- **Taffy.** 9 slots, `compute_cache_slot` based on `(known_dimensions, available_space)` shape [taffy.md §4]. Tolerant equality.
- **Morphorm.** Output-only (`set_bounds`), no input-keyed memoization [morphorm.md §5]. Re-runs always.
- **Clay.** Re-runs always, but text-measurement cache keyed by `(text, config)` hash with generation eviction [clay.md §7].
- **egui.** Prior-frame `WidgetRects` is the de facto cache [egui.md §1, §6]. Plus `Memory::caches` per-frame computation cache.
- **Slint.** `PropertyTracker` per item, lazy pull / eager invalidate [slint.md §3].
- **Masonry.** `MeasurementCache` keyed by `MeasurementInputs { axis, len_req, cross_length }` [xilem.md §2, §4]. Per-axis. Persists across frames.
- **Vizia.** *Whole-tree relayout on any dirty bit*, candidly admitted [vizia.md §3]. Works because morphorm is fast and trees are small.

**Position.** Palantir rebuilds the tree every frame; v1 doesn't cache layout results between frames. Correct: caching is an *optimization* that retained engines need to be merely *correct* (Yoga/Taffy without cache is exponential). For text shaping, cache the shape result keyed by `(text, style, scale)` in the persistent state map; rebuilding `Buffer`s defeats `BufferLine.shape_opt` caching [cosmic-text.md §1, §7, parley.md §6, glyphon.md §6].

### Multi-pass / cyclic pathologies

- **WPF Grid `c_layoutLoopMaxCount`** [wpf.md §5, §8]. `Auto`↔`*` cross-axis dependencies force iterative re-measure capped at a constant. MS's own perf retros call Grid the top offender. Avalonia recommends `Panel` over `Grid` for overlap.
- **Flutter intrinsic O(n²)** [flutter.md §5]. `IntrinsicHeight` widget docs literally say "this class is relatively expensive."
- **egui `request_discard` + `sizing_pass` first-frame jitter** [egui.md §1, §7]. `Grid::show` runs once invisibly to establish column widths.
- **Yoga reentry under different `SizingMode`s** [yoga.md §5]. Cache is load-bearing.
- **Morphorm `Overlay` two-iteration stabilization** [morphorm.md §4].

**Position.** Avoid Grid in Palantir's prototype. If/when added, restrict to `Fixed + Auto + Star` without `*`↔`Auto` cross-axis cycles [wpf.md §7]. Slint already gets this right by disallowing the ambiguity in its grammar [slint.md §2].

### Sizing vocabulary

| Palantir | WPF | Clay | Morphorm | Yoga/Taffy | Iced | Slint |
|---|---|---|---|---|---|---|
| `Fixed(n)` | `Width=N` | `FIXED` | `Pixels(n)` | `flex-basis: Npx; grow=0` | `Length::Fixed` | `min=max=N` |
| `Hug` | `Auto` | `FIT` | `Auto` | `flex-basis: auto` | `Shrink` | `pref=intrinsic, stretch=0` |
| `Fill` | `*` | `GROW` | `Stretch(1.0)` | `flex-grow: 1` | `Fill` | `stretch>0` |
| (none) | `*N` | (none) | `Stretch(f)` | `flex-grow: N` | `FillPortion(N)` | `stretch=N` |
| (none) | (none) | `PERCENT(p)` | `Percentage(p)` | `flex-basis: P%` | (none) | `min/max_percent` |

**Convergence.** Three units are the irreducible minimum: fixed, hug-content, fill-leftover. Every system in the corpus has them under different names. Morphorm validates the four-unit `Pixels | Percentage | Stretch | Auto` set as a real production vocabulary [morphorm.md §7, vizia.md §6].

**Recommended Palantir extensions (in cheapness order):**
1. **`Fill { weight: f32 }`** (default `1.0`) — trivial in `resolve_axis`, mirrors morphorm `Stretch(f)` and CSS `flex-grow: N` [morphorm.md §7, yoga.md §7].
2. **`gap` field on `HStack`/`VStack`** — ~5 lines in arrange driver [yoga.md §7].
3. **Per-child cross-axis `align: Start | Center | End | Stretch`** — mirrors WPF `HorizontalAlignment` [wpf.md §6].
4. **`Percentage(p)`** — defer until a real call site demands it [morphorm.md §7].

## 4. Identity & state persistence

The corpus converges hard on "identity is a hash of (parent-id, user-or-positional key)". Concrete shapes:

- **egui `Id`**: `NonZeroU64` aHash of any `Hash` source; `Id::with(parent, child)` mixes [egui.md §2]. `IdMap = nohash IntMap`. Niche-optimized so `Option<Id>` is 8 bytes. **Highest-fidelity reference for Palantir's `WidgetId`.**
- **Imgui**: `ImGuiID` is `ImHashStr`/`ImHashData` of input bytes seeded by `IDStack.back()` [imgui.md §4]. `PushID`/`PopID` runs the stack.
- **Nuklear**: `nk_murmur_hash(name, seed)` where seed is contextual — `__LINE__` for tree nodes, sentinel constants for windows, a `seq` counter for edit-collision bumps [nuklear.md §4].
- **Clay**: parent-id-seeded FNV-1a, plus `CLAY_IDI(label, i)` numeric offset for collections [clay.md §8].
- **Dioxus**: `ScopeId(usize)` from `Slab<ScopeState>` plus `&'static Template` content hash for skeleton identity [dioxus.md §1-2]. Hooks-by-index is brittle in conditionals [dioxus.md §3].
- **Druid `WidgetId`**: `NonZeroU64` from a global counter [druid.md §1, §3].
- **Floem `ViewId`**: `slotmap::KeyData` newtype, `!Send` [floem.md §1].
- **Vizia `Entity`**: same shape via `EntityManager` [vizia.md §1].
- **Bevy UI**: `Entity` (64-bit generational) [bevy_ui.md §4].
- **Slint runtime**: positional in the generated struct [slint.md §1].
- **Masonry/Xilem**: `WidgetId` (`NonZeroU64`) for retained side; `ViewId` path for authoring side [xilem.md §8].
- **Raygui**: `Rectangle` value-equality. Anti-pattern [raylib.md §3, §5].

State storage shapes:

- **egui `IdTypeMap`** — `(Id, TypeId) → Box<dyn Any>`, clone-on-read, optional serde [egui.md §3]. **Best fit for Palantir's plan** [DESIGN.md §4].
- **imgui `ImGuiStorage`** — sorted `ImVector<ImGuiStoragePair>` per window [imgui.md §8]. O(log N) lookup.
- **nuklear `nk_table` pages** — fixed-cap chained pages, *linear scan* [nuklear.md §5]. State values must fit in `nk_uint`. Anti-pattern beyond ~50 entries.
- **dioxus `generational-box`** — `(data_ptr, NonZeroU64 generation)` slab with `Owner<S>` lifetime [dioxus.md §3]. Use-after-free as typed error.
- **druid `WidgetPod`** — pile of dirty bits and `merge_up` flag-bubbling [druid.md §3]. Anti-pattern needed only because tree is retained.

**Position.** egui's design wins on every axis. Copy verbatim [egui.md §2, §3, §8]. Generational-box pattern from dioxus is worth keeping in mind for any case where Palantir hands out a `Copy` reference outliving the originating widget (drag handles, animation tokens) [dioxus.md §7].

## 5. Tree topology

Two axes: how children are linked and how the tree is stored.

### Children linkage

- **Linked-list children (`first_child` / `next_sibling`)** — Palantir today [CLAUDE.md], `indextree`. O(1) append during recording, no per-node `Vec` allocation.
- **Index-slice children, contiguous ranges per parent** — Clay [clay.md §2]. `children.elements: int32_t*` points into a shared `layoutElementChildren` array, filled at close-time from a scratch buffer. Better cache locality and BFS-friendly.
- **`Vec<NodeId>` per node** — taffy `TaffyTree` and most retained engines [taffy.md §1].
- **Pointer-based** — Yoga `vector<Node*>`, druid trait-object pods [yoga.md §2, druid.md §3]. Per-node heap allocations.

### Tree storage

- **`Vec<Node>` arena, `usize` indices** — Palantir, clay [clay.md §1-2], taffy `examples/custom_tree_vec.rs` [taffy.md §1]. Simplest, cache-dense.
- **`Slab<T>` keyed by stable id** — dioxus `scopes: Slab<ScopeState>` [dioxus.md §1], floem `VIEW_STORAGE` slotmap [floem.md §1], bevy_ui ECS storage [bevy_ui.md §1]. Generational counters guard against reuse.
- **Three-tree split (Widget/Element/RenderObject)** — Flutter [flutter.md §1]. Configuration / persistent instance / live layout participant. Required for hot-reload + retained `State`. Dioxus has its own variant: VNode + `VNodeMount` slab + renderer's ElementId slab [dioxus.md §4-5].
- **Multiple sidecar tables keyed by id** — freya's `tree.{elements, layout, text_style_state, effect_state, layers}` [freya.md §1]; floem's view tree + Taffy tree + Box tree [floem.md §3].
- **Two `unsafe`/page-allocated arenas** — nuklear `nk_buffer` front/back ends with `nk_pool` of `nk_page_element`s [nuklear.md §6].
- **One byte buffer with linked-list embedded commands** — nuklear's `nk_command_buffer`, splice-by-pointer-patch [nuklear.md §2].

**Position.** Linked-list children as Palantir has them is fine for v1. Clay's contiguous index slices are strictly better for cache and BFS, but require a scratch ring during recording — defer until profiling justifies it [clay.md §9]. Three-tree splits are a retained-mode tax [flutter.md §1, §8].

## 6. Painting pipeline

Six distinguishable rendering shapes:

1. **Screen-space `Shape` enum + CPU tessellation per frame.** egui's `epaint` [egui.md §5]. `Shape::{Rect, Circle, Path, Text(Galley), Mesh, Callback, Noop}`. `Tessellator::tessellate_shapes` produces `Vec<Mesh>`; `egui-wgpu` uploads and runs one pipeline. Every rounded-rect becomes real triangles.
2. **Lyon-tessellated mesh batches.** Iced before SDF [iced.md §6]; lyon as generic CPU tessellator [lyon.md §1-7]. Lyon's `GeometryBuilder` trait — algorithm pushes vertices into user-owned typed sink — is the clean factoring [lyon.md §4, §7].
3. **Instanced typed batches with SDF shader per primitive kind.** Iced's current quad pipeline [iced.md §6], Makepad per-shape shader [makepad.md §2-3], quirky's pipeline-per-primitive [quirky.md §2]. **Palantir's chosen shape.** Iced's `Quad { position, size, border_color, border_radius, border_width, shadow_color, shadow_offset, shadow_blur_radius, snap }` instance + SDF rounded-box fragment is reusable wholesale [iced.md §6]. Makepad's per-instance `#[repr(C)] → bytemuck cast → wgpu vertex buffer` is the Rust idiom [makepad.md §3, §10].
4. **GPU compute path renderer.** Vello [vello.md §1-3], used by Masonry [xilem.md §6]. Six parallel SoA streams → ~14 compute dispatches → per-tile command list → fine raster. Coverage analytic, no triangle pipe. Overkill for UI-volume scenes [vello.md §7]; the encoding model itself is high-leverage [vello.md §7].
5. **Scene-graph ItemRenderer trait** with multiple backends. Slint's `RendererSealed` + `ItemRenderer` (~25 methods) backed by software / FemtoVG / Skia [slint.md §4]; floem's `Renderer` trait backed by Vello / Skia / Vger / tiny-skia [floem.md §4]. Cost: every renderer-touching change pays N times.
6. **Black-box do-everything Skia.** Freya [freya.md §3]. ~50 MB linked, every visual feature one Skia call away.

Plus three orthogonal layers worth lifting:

- **Render-command arrays as a clean decouple-point.** Clay's `Clay_RenderCommandArray { boundingBox, renderData, id, zIndex, commandType }` [clay.md §6]; nuklear's typed-shape command list [nuklear.md §2]; egui's `ClippedShape` flat list [egui.md §5]; iced's `Layer { quads, triangles, primitives, images, text, pending_*}` [iced.md §5]. **Palantir's `Tree.shapes` is already this.** Discipline to keep: paint pass walks shapes and emits a typed command per variant, never speaks wgpu directly during the walk [clay.md §9].
- **Per-tile or per-clip scissor stack as commands, not stack-on-encoder.** Clay emits `SCISSOR_START`/`SCISSOR_END` [clay.md §6]; nuklear pushes a scissor command rather than mutating GPU state [nuklear.md §2]; iced's `Layer` opens a sub-layer for clipping [iced.md §5]. Lets the converter split draws on the boundary cleanly.
- **`ShapeRect::Full` sentinel** — Palantir's mechanism for "use my owner's full arranged rect at paint time" [CLAUDE.md "Node vs Shape"]. Corpus equivalents: egui's `ShapeIdx::Noop` + back-patch [egui.md §1], makepad's turtle align-list back-patch [makepad.md §8]. Palantir's sentinel is *better* than back-patching because it doesn't mutate the recorded shape list — resolution is a paint-pass concern.

**Position.** v1 paint pass: walk `Tree.shapes` pre-order, bucket by variant (`RoundedRect`, `Text`, `Line`), emit typed instance buffers, run one pipeline per variant. Borrow iced's quad shader and instance struct verbatim [iced.md §6]; cosmic-text/glyphon for text [glyphon.md §6, cosmic-text.md §7]; lyon adapter only when `Shape::Path` lands [lyon.md §7]. Vello's blurred-rounded-rect closed-form formula is worth lifting straight into the SDF shader for drop-shadows [vello.md §3, §7]. Vello-as-renderer is the future "if Palantir grows into a graphics editor" upgrade path, not v1 [vello.md §7, xilem.md §6].

## 7. Text

Three crates form a stack, plus one outlier:

- **glyphon** [glyphon.md §1-6] — wgpu middleware over cosmic-text. Etagere bin-packed mask + color atlases (`R8Unorm` + `Rgba8Unorm[Srgb]`), LRU eviction, doubling growth, one instanced quad per glyph (28-byte vertex), `prepare`/`render` split [glyphon.md §1, §3-4]. Caller owns `Buffer` + `FontSystem` + `SwashCache`. **The right fit for Palantir v1.**
- **cosmic-text** [cosmic-text.md §1-7] — text engine: harfrust shaping, fontdb + skrifa fallback, bidi, line breaking, swash rasterization, an editor with `Action`/`Cursor`/`Selection`. `Buffer.dirty: DirtyFlags { RELAYOUT | TAB_SHAPE | TEXT_SET | SCROLL }` is a clean dirty model [cosmic-text.md §1]. `FontSystem` is `&mut`-everywhere with sneakily-bad shape-plan FIFO size of 6 and `font_matches_cache` clear-on-overflow [cosmic-text.md §2, §6]. Construction blocks for ~1s loading system fonts.
- **parley** [parley.md §1-6] — Linebender's text-layout crate. Two-stage: build (shape+bidi+font-select) then layout (line-break+align). Shaping cached separately from line-breaking — re-line-break on width change is free [parley.md §1, §6]. `harfrust` + `icu` + `fontique` + `skrifa` is on the order of a megabyte of dependency [parley.md §6].
- **Makepad in-house slug/SDF stack** [makepad.md §6]. Loader / shaper / rasterizer (sdfer + msdfer) / atlas / slug-atlas. Skip [makepad.md §10].

**The single shape-once / layout-many lesson.** Whichever engine is used, the text widget should re-shape only on `(text, font, scale)` change, not on width change [parley.md §6, cosmic-text.md §7]. Width changes re-line-break only. Cache the `Buffer` (cosmic-text) or `Layout` (parley) handle in the persistent state map keyed by `WidgetId` [cosmic-text.md §7, glyphon.md §6].

**Lifecycle alignment with Palantir's four passes** [glyphon.md §6]:
- *Record:* push `Shape::Text { buffer_id, color, bounds }` referencing a cached `Buffer`. No shaping yet.
- *Measure:* if `(text, attrs, max_width)` changed, `buffer.set_size(...) + shape_until_scroll`; read sized `layout_runs`.
- *Arrange:* assigns owner rect; nothing text-specific.
- *Paint:* build `Vec<TextArea>` once per frame, call `text_renderer.prepare` once, `render` inside the pass. Call `atlas.trim()` at end-of-frame [glyphon.md §6].

**FontSystem ownership.** One `FontSystem` on the recorder, *not* `Arc<Mutex<FontSystem>>` [cosmic-text.md §7]. egui's `Fonts` and iced's `font::Storage` both wrestle with this; lock contention comes from "I want to measure text from anywhere" — Palantir's measure pass has a single owner.

**Position.** v1 = glyphon + cosmic-text [glyphon.md §6, cosmic-text.md §7]. Move to parley when one of these arrives: rich text spans (different size/weight mid-line), accessibility tree integration, RTL/CJK input methods [parley.md §6]. Subpixel AA is the known glyphon ceiling — defer until it bites [glyphon.md §5].

## 8. Vector geometry

The Linebender stack is the consensus answer:

- **kurbo** = "where and what shape" [kurbo.md §1, §6]. f64-only by design — curve algorithms lose accuracy fast in f32 [kurbo.md §5]. `Shape` trait + `path_elements(tolerance)` + `as_rect`/`as_circle` downcasts is the right vocabulary for hit-testing arbitrary paths [kurbo.md §1]. `Affine` (full 2D) vs `TranslateScale` (uniform-scale + translate that *preserves `Rect`/`Circle`/`RoundedRect` types*) [kurbo.md §3] — the right escape hatch for zoom/DPI without forcing every shape to become a `BezPath`.
- **lyon** = CPU path tessellator [lyon.md §1-2]. Robust sweep-line fill with intersection handling, strip-based stroke with miter/bevel/round joins. Output via `GeometryBuilder` trait. **Add only when `Shape::Path` lands** [lyon.md §7]; do *not* route `RoundedRect` or `Line` through lyon (dedicated SDF shaders).
- **peniko** = paint vocabulary [peniko.md §1-5]. `Brush = Solid(AlphaColor) | Gradient | Image` with two generic params for borrowed-vs-owned [peniko.md §1]. `Compose × Mix = BlendMode` from W3C Compositing 1 [peniko.md §4]. Shared `Extend` enum between gradients and image samplers [peniko.md §3, §6].
- **vello / color** = the renderer + color science layer [vello.md, peniko.md §2].

**Recommendation tree:**

1. Today: keep Palantir's `geom.rs` (~200 lines of f32 `Vec2/Rect/Color`). Mark a known fork point in `DESIGN.md`: switching to kurbo is mandatory if Palantir ever consumes Vello/Parley/peniko [kurbo.md §6, peniko.md §7].
2. When `Shape::Path` lands: add lyon for tessellation; consider switching `geom.rs` to kurbo at the same time so the path API uses `kurbo::BezPath` natively [lyon.md §7, kurbo.md §6].
3. If gradients/images arrive before kurbo: lift the `Brush<I, G>` enum shape into Palantir's own `Paint` enum; copy `Compose`/`Mix` enums verbatim [peniko.md §7].
4. Stay with f32 in `geom.rs` until the kurbo cutover. f32→f64 cast happens at the wgpu boundary [kurbo.md §5].

## 9. Convergent best practices

Things the corpus agrees on:

- **Identity = hashed parent + user-or-positional key.** §4 above.
- **State outside the tree, by stable id, clone-on-read.** egui `IdTypeMap`, imgui `ImGuiStorage`, nuklear `nk_table`, planned Palantir [DESIGN.md §4]. Floem and vizia confirm the converse: putting state on retained widgets forces lenses or signals [floem.md §6, vizia.md §6].
- **Hit-test against last frame's rects.** egui [egui.md §6], imgui [imgui.md §6] (against current rects, but same "use what's already laid out"), iced [iced.md §8], Palantir [DESIGN.md §5]. One-frame input lag is imperceptible.
- **Shape-once / layout-many for text.** parley [parley.md §1, §6], cosmic-text [cosmic-text.md §1, §7], glyphon [glyphon.md §6]. Width changes re-line-break only.
- **Alignment-as-arrange-time-translation.** WPF `ArrangeCore` shrinks slot to `unclippedDesiredSize` then translates leftover [wpf.md §6]; slint `solve_box_layout` post-processes alignment after the solver [slint.md §2]; clay's BFS top-down sizing then DFS pre-order position [clay.md §4]; iced `Node::move_to` after children compute [iced.md §4]. Panels never deal with extra slot space.
- **Clip via scissor stack as command, not encoder mutation.** clay [clay.md §6], nuklear [nuklear.md §2], iced [iced.md §5].
- **Use mature font fallback if you support i18n.** fontique [parley.md §5] and fontdb [cosmic-text.md §2] both encode CLDR-shaped (script, language) → font priority lists. Don't reinvent.
- **One renderer pipeline per primitive kind, instance buffer per kind, accumulated for the frame.** iced [iced.md §5-6], makepad [makepad.md §3, §10], quirky-but-only-in-the-`Quads`-batched-case [quirky.md §2]. *Not* one primitive per widget.
- **Per-instance struct = `#[repr(C)] + bytemuck::Pod + Zeroable`, cast straight to wgpu buffer.** quirky [quirky.md §2], makepad [makepad.md §3], iced [iced.md §6]. No marshalling.
- **`prepare` / `render` split for renderer middleware.** glyphon [glyphon.md §4], iced wgpu [iced.md §5], quirky [quirky.md §6]. Prepare has `&mut Device, &mut Queue, &mut Cache`; render has `&'a` references and writes into someone else's `RenderPass`.
- **Atlas trim at end of frame.** glyphon `atlas.trim()` [glyphon.md §1, §6].
- **One `FontSystem` per app, owned by recorder, not behind a Mutex.** cosmic-text [cosmic-text.md §7].
- **Render-commands as flat sorted array, layout core unaware of GPU.** clay [clay.md §6, §9], egui Shape enum [egui.md §5], nuklear command buffer [nuklear.md §2]. Palantir's `Tree.shapes` is the same shape — keep the discipline.

## 10. Divergent design choices Palantir must make

Where the corpus disagrees and we must commit:

| Decision | Options | Rec | Why |
|---|---|---|---|
| Constraint shape | WPF `available + Sizing` / Flutter `BoxConstraints` / Masonry `LenReq` / Slint `LayoutInfo` | **WPF + per-axis `Sizing`** | Palantir already commits sizing policy at recording time [CLAUDE.md]; `BoxConstraints` `min` duplicates `Sizing::Hug`/`Fixed` [druid.md §2, xilem.md §10]. Slint's `LayoutInfo` is the right *internal* representation when layout is pulled into pure functions [slint.md §7]. |
| Pass shape | WPF two-pass / Flutter one-recursion / Yoga reentrant / Clay multi-pass | **WPF two-pass + height-prop DFS for text wrap** | Two-pass is pinned in `DESIGN.md`. Clay shows the third pass is mandatory once text wraps [clay.md §4, §9]. |
| Intrinsic-size API | Yes, separate (Flutter) / no, just measure (WPF, Palantir) | **No, just measure** | Two-pass already runs intrinsic queries. Flutter's separate `IntrinsicWidth`/`Height` is its O(n²) slow path [flutter.md §5, §8]. |
| Layout cache | Persistent (Yoga, Taffy, Masonry) / output-only (Morphorm) / none (Palantir today) | **None for v1, Taffy-style 9-slot if it bites** | Per-frame rebuild makes caching unnecessary for correctness [yoga.md §7]. Stretch's single-slot proves "worse than no cache" exists [stretch.md §3, §6]. |
| Brush vocabulary | peniko enum / typed args per primitive | **Typed args for v1, lift `Brush<I, G>` shape when needed** | Peniko's transitive deps (color, kurbo, linebender_resource_handle) are heavy [peniko.md §7]. |
| Geometry crate | kurbo / glam / handrolled `geom.rs` | **Handrolled f32 v1, kurbo when `Shape::Path` lands** | f64 robustness matters for path algorithms; doesn't matter for axis-aligned rects [kurbo.md §5-6]. Mark as fork point in DESIGN.md. |
| Tessellation | lyon / Vello / SDF + glyphon | **SDF-instanced quads + glyphon for v1, lyon when `Shape::Path` lands** | Vello has fixed pipeline cost calibrated for tens-of-thousands of paths [vello.md §7]; iced quad shader is the right primitive for rounded rects + borders + shadows [iced.md §6]. |
| Renderer abstraction | Pluggable (slint, floem) / one concrete (Palantir, makepad) | **One concrete (wgpu)** | Slint+floem each pay 2-4× the renderer-touching cost [slint.md §7, floem.md §6]. |
| Reactive system | Signals (floem, vizia, slint) / VDOM diff (dioxus) / nothing (egui, imgui, Palantir) | **Nothing** | Frame rebuild *is* the dependency tracking. Signals' `unsafe`-heavy intrusive lists [slint.md §3] and `Arc<RefCell>`-on-signal cost [floem.md §6] are pure overhead in IM. |
| Tree topology | Linked-list children (Palantir, clay) / contiguous index slices (clay) / Vec<NodeId> per node | **Linked-list now, consider clay's slices later** [clay.md §2, §9] |
| Children of a flexbox | Yes (Taffy) / no, native panels only / both via feature flag | **Native HStack/VStack/Dock + optional Taffy via `palantir/taffy` feature** | Native panels are <100 LOC each [taffy.md §9, wpf.md §7]. Bevy_ui is the cautionary tale of full Taffy integration cost [bevy_ui.md §5]. |
| Animation / transitions | Property bindings (slint), interpolator-on-style (vizia), per-frame closures (egui) | **Per-frame closures via state map + tween crate** [DESIGN.md "Non-Goals"] |
| Hot reload | DSL (slint, makepad) / generated templates (dioxus) / not v1 | **Not v1** [makepad.md §7, slint.md §1] |

## 11. Anti-patterns from the corpus

Concrete things to *not* do:

- **Painting during user code (cursor-walk IM).** imgui [imgui.md §3], egui [egui.md §1]. Forces two-frame autofit jitter and manual right-align caller-side measurement.
- **One `DrawablePrimitive` per widget with own buffers.** quirky [quirky.md §1, §6]. 100 buttons → 100 single-instance draws.
- **Quirky's "vignette" rounded-rect.** `pow(|x|², 2)`-based fade is not an SDF [quirky.md §3]. Use iced's real `rounded_box_sdf` [iced.md §6].
- **Glyphon `TextRenderer::new` per widget per frame.** quirky [quirky.md §4]. One TextRenderer per frame.
- **Stretch's no-op `Allocator::free`.** Atomic global counter monotonically grows [stretch.md §4, §6].
- **Stretch's single-slot cache.** Worse than no cache [stretch.md §3].
- **Stretch's `Node = (instance_id, local_id)` plus dual hashmap.** Costs a hash probe per access for nothing [stretch.md §2, §6].
- **Druid's `Lens<Vec<T>, T>` indexed by `usize`.** Unsound across element insert/delete reorders [druid.md §5]. Vizia confirmed the failure and migrated [vizia.md §2].
- **WPF's `LayoutTransform`.** ⅓ of `MeasureCore` is `FindMaximalAreaLocalSpaceRect`; useless without arbitrary 2D transforms [wpf.md §7]. Avalonia and Uno both dropped it [wpf.md §8].
- **WPF dispatcher coupling.** Single STA dispatcher creates HWNDs that never free [wpf.md §8].
- **WPF DependencyProperty leaks.** `AddValueChanged` pins listeners forever [wpf.md §8].
- **WPF Grid `Auto`↔`*` cross-axis cycles.** Hits `c_layoutLoopMaxCount` [wpf.md §5, §8].
- **Vizia's CSS subset.** `vizia_style` alone is bigger than Palantir's whole codebase [vizia.md §4, §6].
- **Vizia's whole-tree relayout on any dirty bit** [vizia.md §3].
- **Yoga's per-node pixel rounding.** Accumulates 1px gaps at fractional scales [yoga.md §6]. Taffy fixed it (commit aa5b296) [taffy.md §5].
- **Yoga `Errata` flags as bug-compat config.** Required to keep React Native classic working [yoga.md §6].
- **Dioxus hooks-by-call-order.** `hook_index: Cell<usize>` panics on hooks-in-conditionals [dioxus.md §3].
- **Dioxus `deep_clone` on every Element returned from a component** [dioxus.md §1, §7].
- **Slint full reactive `Property<T>` machinery.** 1504 lines, mostly `unsafe` [slint.md §3].
- **Three-tree split (Widget/Element/RenderObject).** Flutter [flutter.md §1, §8].
- **Multiple renderer backends.** Slint (software + femtovg + Skia + 6 surface variants) [slint.md §4], floem (Vello + Skia + Vger + tiny-skia) [floem.md §4].
- **cosmic-text shape-plan FIFO size 6** [cosmic-text.md §6].
- **cosmic-text `font_matches_cache` clear-on-overflow** [cosmic-text.md §6].
- **Floem thread-local `RUNTIME`** [floem.md §6]. `!Send` everywhere.
- **Vello full pipeline at UI volumes** [vello.md §7]. ~14 dispatches calibrated for thousands of paths.
- **Bevy ECS as UI authoring surface** [bevy_ui.md §5].
- **Bevy `Children` + parallel `TaffyTree` double bookkeeping** [bevy_ui.md §5].
- **Raygui `Rectangle`-as-identity** [raylib.md §3, §5].
- **Raygui `GuiDisable()` global state override** [raylib.md §5].
- **Nuklear linear-scan state tables** [nuklear.md §5].
- **Nuklear `nk_uint`-only state values** [nuklear.md §5].
- **Iced `Node { children: Vec<Node> }`** [iced.md §4]. Heap allocation per container per frame.
- **Iced `Element` / `dyn Widget` trait dispatch** [iced.md §9]. Wrong for Palantir's closed-shape model.
- **Druid five contexts (`EventCtx`/`LifeCycleCtx`/`UpdateCtx`/`LayoutCtx`/`PaintCtx`)** [druid.md §1, §7].
- **Lyon for rounded rects, lines, text** [lyon.md §7]. Tessellating a rounded rect into ~24 triangles per corner just to lose AA.
- **Egui's CPU tessellation of every shape** [egui.md §5]. Acceptable consequence of `Shape` enum being shared with non-wgpu backends. We're wgpu-only.

## 12. Open questions for Palantir

Aggregated from across the corpus and the project's own design docs:

1. **Re-measure on size changes during arrange.** WPF allows constrained re-measure; one pass each may be enough [DESIGN.md §3]. Add `request_discard`-style second frame on size mismatch (egui's approach) [egui.md §1, §7] only if a real case arises.
2. **Closure lifetimes vs arena.** How does `ui.stack(|ui| ...)` mutate the same arena without `RefCell` everywhere? [DESIGN.md "Open Questions"].
3. **Taffy vs native panels primary.** Native primary, Taffy as opt-in feature flag [DESIGN.md, taffy.md §9]. Confirmed by bevy_ui's integration cost and floem's three-tree shape [bevy_ui.md §5, floem.md §3].
4. **Frame paint order and `Frame` semantics.** "Background shape declared on parent before recursing into children" must be the recorded contract [egui.md §8]. Confirm when `src/widgets/frame.rs` lands.
5. **Push constants vs shared UBO for camera.** Quirky proves shared UBO works on stock wgpu [quirky.md §6]. Start with UBO.
6. **Subpixel AA.** glyphon doesn't do LCD striping; visibly fuzzier on 1× monitors [glyphon.md §5]. Defer until it bites; parley/vello is the upgrade path.
7. **Persistent state map keying.** `(WidgetId, TypeId) → Box<dyn Any>` per egui, or `WidgetId → Box<dyn Any>`? Egui's two-key form is more robust for swapping widget types at the same call site [egui.md §3].
8. **First-frame text shape.** glyphon assumes user has shaped a `Buffer` ahead of time [glyphon.md §2]. Palantir's measure pass needs `&mut FontSystem` access during measure.
9. **Damage / dirty tracking.** Ship v1 full-redraw [DESIGN.md §7]. Slint's `PartialRenderer` + `PropertyTracker` is the model when profiling demands it [slint.md §4].
10. **Virtualization for long lists.** Flutter sliver protocol is the standard answer but adds a parallel layout protocol [flutter.md §6]. Prefer "virtual children" hook on a single node yielding measured children on demand within visible window. WPF virtualization is famously fragile [wpf.md §8].
11. **Grid layout.** When/if added, restrict to `Fixed + Auto + Star` without `*`↔`Auto` cross-axis cycles [wpf.md §7, slint.md §2].
12. **Animation primitives.** State map needs an interpolator-on-`Id-Any-map` story when transitions arrive [vizia.md §6].
13. **Theming / styling.** Inline `Style` structs only [DESIGN.md "Non-Goals"]. Vizia's CSS-as-DSL is the expensive alternative [vizia.md §4]. Designers who want skinnability get a JSON/RON theme file.
14. **Multi-window.** Single surface for v1. Egui's `Viewport` + `IdMap<PaintList>` per surface is the model [egui.md §5].
15. **Accessibility.** Add later via accesskit. Masonry's per-widget `accessibility_role + accessibility(node)` + dedicated pass is the model — "one-week job if planned for now, a month if not" [xilem.md §9-10].
16. **Hot reload.** Out of scope for v1 [makepad.md §7, slint.md §1].
17. **Async / I/O.** Sync per-frame; user's event loop drives recording [DESIGN.md §5]. Don't ship Druid-style `ExtEventSink`/`Command` round-trip [druid.md §5, §7].
18. **`Spacing::Auto` parent-padding inheritance.** Morphorm's "child's `left: Auto` defers to parent's `padding_left`" is a small win [morphorm.md §3, §7].
19. **Encoding model.** Vello's tag-encoded flat-stream representation is high-leverage even at our scale [vello.md §7]. Worth doing when shape volume grows beyond a few hundred per frame.

## 13. Quick-lookup matrix

| Task | Primary refs | Secondary |
|---|---|---|
| HStack/VStack semantics | wpf.md §1-5, clay.md §4-5, slint.md §2, taffy.md §3 | morphorm.md §1-4, yoga.md §5-7 |
| Sizing enum extensions (`Fill { weight }`, gap, align) | morphorm.md §7, yoga.md §7, slint.md §2, wpf.md §6 | iced.md §3 |
| Multi-pass / cycle pathologies | wpf.md §5, §8, flutter.md §5, egui.md §1, §7, yoga.md §5 | clay.md §4 |
| Scroll regions / virtualization | flutter.md §6, freya.md §2, wpf.md §8 | egui.md §1 |
| Text widget (v1) | glyphon.md §1-6, cosmic-text.md §1-7, iced.md §7 | parley.md §6 |
| Text widget (later) | parley.md §1-6 | xilem.md §7 |
| Focus / keyboard | parley.md §4, masonry passes/event [xilem.md §9], floem `focus_pass` | imgui.md §6, vizia.md §5 |
| Hit-testing | egui.md §6, iced.md §8, imgui.md §6, kurbo.md §2 | clay.md §8 |
| Animation / transitions | vizia.md §4, makepad.md §9, slint.md §3 | egui.md §3 |
| Persistent state (`Id → Any`) | egui.md §2-3, imgui.md §4, §8, nuklear.md §4-5, dioxus.md §3 | masonry [xilem.md §4] |
| Theming / inline style | vizia.md §4 (anti-pattern), peniko.md §1-5 | makepad.md §1 |
| Image loading | peniko.md §6, glyphon.md §1, iced.md §5 | bevy_ui.md §3 |
| Custom shapes / canvas | lyon.md §3-7, vello.md §1-3, kurbo.md §1-4 | makepad.md §2-3 |
| Identity / WidgetId | egui.md §2, clay.md §8, nuklear.md §4, imgui.md §4 | xilem.md §8, dioxus.md §1 |
| wgpu paint pipeline | iced.md §5-7, quirky.md §2, makepad.md §3-5, glyphon.md §3-4 | bevy_ui.md §3 |
| SDF rounded-rect shader | iced.md §6, makepad.md §10 (sdf.rs), vello.md §3 (blur) | quirky.md §3 (anti-pattern) |
| Render-command decoupling | clay.md §6, §9, nuklear.md §2, egui.md §5 | iced.md §5 |
| Encoding model (flat streams) | vello.md §1, §7 | clay.md §1-2 |
| Clip / scissor stack | clay.md §6, nuklear.md §2, iced.md §5 | imgui.md §5 |
| Hot reload | makepad.md §7, slint.md §1, freya.md §4 | dioxus.md §4 |
| Accessibility | xilem.md §9, parley.md §4 | freya.md §3 |
| Async / events | druid.md §5 (anti-pattern), vizia.md §5, dioxus.md §7 | iced.md §1, §8 |
| Tree topology | clay.md §1-2, dioxus.md §1, floem.md §1, taffy.md §1 | bevy_ui.md §1 |
| Geometry / kurbo | kurbo.md §1-6, peniko.md §5 | lyon.md §3 |
| Reactive runtime (what to skip) | floem.md §2, vizia.md §2, slint.md §3 | xilem.md §1 |
| Two-tree separation (View/Widget) | xilem.md §1-2 | dioxus.md §1, freya.md §1 |
| Cautionary tale: retained mode | druid.md §1-7, vizia.md §2, freya.md §1, §4, slint.md §3 | flutter.md §1 |
| Cautionary tale: layout engine port | stretch.md §1-6 | yoga.md §3 |
| Cautionary tale: pure IM ceiling | imgui.md §1-3, raylib.md §1-5, nuklear.md §3 | egui.md §1 |
