# Xilem / Masonry — reference notes for Palantir

Source roots (locally cloned):
- `tmp/xilem/xilem_core/src/` — the `View` reactive core (`view.rs`, `view_ctx.rs`, `view_sequence.rs`, `views/`).
- `tmp/xilem/masonry_core/src/` — the retained widget engine (`core/widget.rs`, `core/widget_state.rs`, `core/widget_pod.rs`, `passes/{layout,paint,event,update,compose,accessibility}.rs`, `layout/{len_req,len_def,length}.rs`).
- `tmp/xilem/xilem/src/` — winit + masonry glue.
- `ARCHITECTURE.md` at repo root has the dependency graph; Masonry sits on `vello` + `parley` + `tree_arena`.

## 1. Two-layer split: `View` (declarative) vs `Widget` (retained)

Xilem's reactive layer in `xilem_core` is a tree of cheap, short-lived `View` values. The trait (`xilem_core/src/view.rs`):

```rust
pub trait View<State, Action, Context: ViewPathTracker>: ViewMarker + 'static {
    type Element: ViewElement;        // the retained thing it builds (a Masonry widget pod)
    type ViewState;                   // private bookkeeping kept across rebuilds
    fn build(&self, ctx, &mut State) -> (Self::Element, Self::ViewState);
    fn rebuild(&self, prev: &Self, view_state, ctx, element: Mut<'_, Self::Element>, &mut State);
    fn teardown(&self, view_state, ctx, element);
    fn message(&self, view_state, msg, element, &mut State) -> MessageResult<Action>;
}
```

Each app frame the user closure produces a fresh `View` tree from app state. The framework diffs it against the previous tree: same type at the same position → `rebuild` mutates the existing widget in place; different type → `teardown` then `build`. `Mut<'_, Self::Element>` is the projected mutable handle into the Masonry tree. `ViewSequence` (`view_sequence.rs`, `view_sequences/impl_tuples.rs`) generalises the same protocol to ordered lists with stable per-index identity. This is the "two-tree" pattern: a thin transient view tree authoring a heavy retained widget tree, with diff edges that touch only what changed.

## 2. Masonry's `Widget` trait

`masonry_core/src/core/widget.rs` defines the retained side. Relevant lifecycle:

```rust
fn measure(&mut self, ctx: &mut MeasureCtx, props, axis: Axis, len_req: LenReq, cross_length: Option<f64>) -> f64;
fn layout(&mut self, ctx: &mut LayoutCtx, props, size: Size);
fn compose(&mut self, ctx: &mut ComposeCtx);                // post-layout transforms (scroll, animation)
fn pre_paint / paint / post_paint(&mut self, ctx, props, painter: &mut Painter);
fn on_pointer_event / on_text_event / on_access_event / on_action / on_anim_frame / update;
fn register_children / children_ids;
fn accessibility_role / accessibility(node: &mut accesskit::Node);
```

The closest WPF analogue is the pair `MeasureOverride(availableSize) -> desiredSize` and `ArrangeOverride(finalSize) -> renderSize`. Two important differences:

- Masonry's `measure` returns a single `f64` *length on one axis*, not a 2D `Size`; the framework drives it once per axis. Cross-axis can be passed as a hint via `cross_length`. This breaks WPF's coupling between width and height measurement and lets the engine cache per-axis (`layout/measurement_cache.rs`, `MeasurementInputs { axis, len_req, cross_length }`).
- `layout(size)` is the *arrange* step. It receives the parent's chosen content-box `Size` and is responsible for calling `LayoutCtx::compute_size` / `run_layout` / `place_child` on every child. The widget does not return a size from `layout`; the parent already committed.

## 3. `LenReq` instead of an unbounded `availableSize`

The measurement input (`masonry_core/src/layout/len_req.rs`):

```rust
pub enum LenReq { MinContent, MaxContent, FitContent(f64) }
```

This is a richer version of WPF's `availableSize`. WPF uses `f64::INFINITY` to mean "tell me your intrinsic size", which conflates *min-content*, *max-content*, and "fit a finite cap" into one value. CSS sidesteps it with min/max-content keywords; Masonry encodes those directly. Note: it is **not** a Flutter-style `BoxConstraints { min, max }`. The size constraints (`Dimensions`, `MinSize`, `MaxSize`) live as widget *properties*; Masonry resolves them around the `measure` call (see `passes/layout.rs::measure_border_box`, `resolve_len_def`). So the widget itself only sees a request kind on one axis, plus an optional cross hint.

## 4. Layout invalidation

`masonry_core/src/core/widget_state.rs` carries `request_layout`, `needs_layout`, `request_paint`, `needs_paint`, plus a per-widget `MeasurementCache`. `WidgetState::merge_up` bubbles `needs_*` flags toward the root after every pass. The layout pass (`passes/layout.rs`) short-circuits on subtrees where `needs_layout` is false, reusing cached `layout_border_box_size` and `origin`. `request_layout` (`core/contexts.rs:1622`) sets the dirty bit; the next frame re-measures only the dirty path. The doc on `WidgetState` is explicit about avoiding "zombie flags" — every pass must `recurse_on_children` so flags clear cleanly.

## 5. Scene rendering (paint pass)

`passes/paint.rs` walks the tree and fills a `Scene` (Masonry's wrapper over `vello::Scene`) by calling each widget's `pre_paint` / `paint` / `post_paint`. Painting is decoupled from layout: a widget marked `request_paint` but not `request_layout` is repainted using cached `origin`/`size`. Scenes are cached per-widget (`scene_cache: HashMap<WidgetId, (Scene, Scene, Scene)>`) and grouped into `VisualLayer`s for compositing. Primitives are vector: filled/stroked paths, gradients, glyph runs, clips, blurs, transforms — no quads, no atlas. The widget never talks to the GPU directly; it only emits a retained-mode display list.

## 6. Vello

Vello is a *compute-shader* 2D renderer. Instead of rasterising shapes into instanced quads, it encodes the scene into GPU buffers and runs a pipeline of compute kernels that do path tiling, sorting, and fine rasterisation, ending in a single image-store pass. This buys you correct anti-aliased arbitrary-curve paths, gradients, and blurs at primitive cost, without per-shape shader specialisation. The trade-off is heavy compute-shader requirements (no WebGL, awkward on older mobile GPUs) and a much larger renderer surface.

For Palantir, Vello is overkill: rounded rects + text + a few lines is exactly the workload an instanced-quad pipeline (Iced-style, `tmp/iced/wgpu/`) handles in ~200 lines of WGSL — SDF rounded-rect shader, glyph atlas, instanced draws. Keep Vello as a future option behind a feature flag if you ever need arbitrary paths.

## 7. Text via Parley

`masonry_core/src/core/text.rs` re-exports `parley::StyleProperty`/`StyleSet` and uses `parley::Layout` for shaping. Text widgets call into Parley during `measure` to build a shaped layout for both `MinContent` (longest unbreakable run) and `FitContent(w)` (wrapped at width `w`); the laid-out `parley::Layout` is then cached on the widget and replayed during `paint` by walking `PositionedLayoutItem`s and emitting glyph runs into the `vello::Scene`. Shaping happens during measurement — the `MeasurementCache` ensures it's not redone every layout pass.

## 8. View identity and stable state

Identity in the view tree is *positional*, namespaced by `ViewId` (`xilem_core/src/view_ctx.rs`):

```rust
pub struct ViewId(u64);
trait ViewPathTracker {
    fn push_id(&mut self, id: ViewId);
    fn with_id<R>(&mut self, id: ViewId, f: impl FnOnce(&mut Self) -> R) -> R;
    fn view_path(&mut self) -> &[ViewId];
}
```

Container views push a `ViewId` per child before recursing into `build`/`rebuild` (`view_sequences/impl_tuples.rs` uses tuple index; `impl_option.rs` and `views/any_view.rs` bump a *generation* counter when the variant changes, invalidating the old child's path). The full path `&[ViewId]` is the routing key for messages, and is what tells `rebuild` whether two views at "the same place" are really the same. `memoize` (`views/memoize.rs`) caches `ViewState` keyed by an equality token to skip rebuild entirely. Masonry's own `WidgetId` (`core/widget.rs:42`, `NonZeroU64`) is allocated once at `build` time and persists across rebuilds — it is the retained-side identity, not the authoring-side one.

## 9. Events and AccessKit

Events flow through dedicated passes (`passes/event.rs`, `passes/action.rs`, `passes/update.rs`). Hit testing uses cached `bounding_box` rects on `WidgetState`. Events bubble; widgets emit `Action`s up; Xilem's `View::message` walks the `[ViewId]` path to deliver them to the view that produced the widget. Accessibility is first-class: every widget implements `accessibility_role()` and `accessibility(&mut accesskit::Node)`, run as its own pass (`passes/accessibility.rs`) so a screen reader sees a tree shaped by widgets, not by views.

## 10. Lessons for Palantir

- **Stay with `available: Size`, not `BoxConstraints`.** Masonry's `LenReq` is the actually-interesting upgrade over WPF: it splits min-content / max-content / fit-content. Min/max box constraints are a Flutter concern that doesn't help if your size policy is already `Hug`/`Fill`/`Fixed`. If you ever want shrink-wrap text and intrinsic-min sizing both, steal `LenReq`; you don't need full `BoxConstraints`.
- **Per-axis measure is worth copying.** `measure(axis, len_req, cross_length) -> f64` plus a `MeasurementInputs` cache key lets text and other reflowable widgets cheaply answer "how tall at width W?" without recomputing the orthogonal axis. Your single `measure(available: Size) -> Size` will need a similar two-call pattern once you wire up real text.
- **Identity by positional path.** The `ViewId` stack pushed during recursion is exactly what your `WidgetId = hash(call-site + user key)` already does. Masonry confirms the rule: identity is *path-positional*, with explicit generation bumps for variant changes (`Option`, `OneOf`, `AnyView`). For your `if`/match-style containers, bump a generation counter rather than relying on call-site hashes alone — call sites collide across branches.
- **Dirty bits live on the persistent state, not the tree.** Masonry's `WidgetState` flags survive across frames because the widget tree is retained. Yours is rebuilt every frame, so `needs_layout` is implicit (you re-measure everything). What you *can* steal is the `MeasurementCache` keyed on `(axis, len_req, cross_length)` — store it in the persistent `Id → Any` state map, indexed by `WidgetId`, so text shaping survives across frames even though the tree doesn't.
- **Vello: skip it.** An immediate-mode crate's renderer should be one wgpu render pass with a few instanced pipelines (rounded-rect SDF, glyph quads, lines). Vello would dominate your binary size, GPU requirements, and complexity for zero gain on a button-and-text UI. Revisit only if you need arbitrary vector paths.
- **Steal the scene-cache concept anyway.** Even with instanced quads, caching the per-widget primitive list when neither layout nor style changed is the same idea as Masonry's `scene_cache`, and maps cleanly to `wgpu::RenderBundle` reuse on stable subtrees.
- **AccessKit is cheap to design in early.** Adding `accessibility_role` on widgets and an opt-in pass that emits `accesskit::TreeUpdate` from your arranged tree is a one-week job if you plan for it now and a month if you don't.
