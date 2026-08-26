# Record-time geometry

Research notes on the oldest structural complaint against palantir's frame
model: **the record pass cannot see this frame's layout.** Widgets that need a
rect can only read last frame's, so callers compensate with relayout requests,
cross-frame geometry caches, and one-frame-late drawing.

This is a survey plus a set of proposals, not a plan. Nothing here is
committed.

## The problem in palantir's terms

The cycle is `record → measure → arrange → cascade → encode`. During record the
tree does not exist yet, so `Ui::response_for` reads the *previous* frame's
`Layout` + `Cascade` (`src/ui/mod.rs`). The one escape hatch is
`Ui::request_relayout`, which makes `FrameCycle::run` replay `record_pass` a
second time, capped at one retry (`src/ui/frame_cycle.rs`).

Darkroom's node canvas builds a compensating layer on top of that:
`CanvasGeometry` caches `node_sizes` and `PortLayer::offsets` across frames,
`Relayout` accumulates a whole-frame re-record request, and `GraphUI::prepass`
polls responses before `GraphUI::draw` runs.

### Three problems, not one

The consumers of that cache do not all need the same thing:

| consumer | what it needs | when it needs it |
|---|---|---|
| wire endpoints (`paint::wire`) | port centers | **draw time** — never needs to be known at record |
| breaker probe, rubber band, `node_screen` pointer tests | node rects | hit-test time — one frame late is *correct* |
| cull test, `node_world_rect` for view-fit | node sizes | **record time** — the only genuinely hard case |

Today all three route through the same stale cache, so the two easy cases
inherit the hard case's problems. Any fix should split them.

## Prior art

Seven distinct mechanisms, roughly ordered by how much machinery they cost.

### 1. Deferred draw closures

Compose's `Modifier.drawBehind` / `drawWithContent`, Flutter's `CustomPaint`,
WPF's `OnRender`. Record registers a callback; the framework invokes it after
layout with the resolved rect. Zero lag, no second pass.

Palantir already has this shape: `GpuPaint::paint` runs after `App::record`
returns, against the composed physical rect (`src/renderer/gpu_view.rs`). There
is no CPU-side sibling.

### 2. Anchors / geometry preferences

SwiftUI's `anchorPreference` + `GeometryProxy[anchor]`, CSS `anchor-name` /
`position-anchor`, Flutter's `LayerLink` + `CompositedTransformFollower`.

Record a *reference* to another element's box rather than a number; the
reference resolves after layout. Flutter's resolves as late as compositing, so
a follower tracks a leader with literally zero frame lag. Declarative — no
callback re-entrancy.

### 3. Composition during layout

Compose's `SubcomposeLayout` (and `BoxWithConstraints`, built on it), Flutter's
`LayoutBuilder`. Inverts the order for one subtree: the parent is measured
first, then the user's build closure runs *inside* the measure pass with the
constraints in hand. The exact, single-pass answer to "how many items fit" —
no throwaway work.

### 4. Lookahead pass

Compose's `LookaheadScope` + `approachLayout`. Runs a full measure/place pass
to compute *destination* geometry, then a second pass that has both target and
current. Notable for scoping the double-pass to a subtree and exposing both
values rather than discarding the first.

### 5. Multi-pass record with a size memo

egui 0.29's `UiBuilder::sizing_pass` + `Context::request_discard`; Dear ImGui's
`HiddenFramesCannotSkipItems` (a window renders invisibly for a frame to
measure, then shows). This is what `request_relayout` already is.

Worth noting how egui got there: the original two-pass design in issue #843 was
closed **not planned**, over 50–100% record overhead and input
double-processing. What shipped instead was narrower — a flag telling widgets
"shrink to minimum, don't expand", plus an explicit discard call.

### 6. On-demand synchronous measure

WPF's `Measure(availableSize)` → `DesiredSize` readable immediately; Masonry's
`LayoutCtx::run_layout(child)`. Trivial in a retained tree. Palantir already
follows the WPF measure/arrange contract, which makes this more natural here
than it would be in egui.

### 7. Reactive fixpoint

Slint: geometry are properties with lazy bindings; reading `foo.width` inside a
binding registers a dependency and forces evaluation on demand. Ordering stops
existing as a concept. Requires a retained property graph — a different
framework, not a feature.

### The framing worth stealing

Ryan Fleury's RAD UI deliberately accepts one frame of lag for *event
consumption only*, never for rendering: the render command buffer is built from
the finished tree at end of frame, so pixels are never stale; only hit-testing
is behind. That is the line palantir should draw too, and currently does not —
darkroom's wires paint from stale data.

## Proposals

### A. Anchored shapes — geometry references resolved after arrange

Let a shape carry an `Anchor` where a `Vec2` goes, resolved between arrange and
cascade. Declarative, per-frame allocation-free, and nothing downstream
changes — the same thing CSS anchor positioning and SwiftUI anchor preferences
express.

Covers endpoints, focus rings, badges, leader lines, selection brackets, and
overlay strokes. It does **not** finish a bezier connector on its own, because
the interior control points are a non-affine function of the endpoints — see
the design study below.

Worked out in detail under [Design study: anchored shapes](#design-study-anchored-shapes).

### B. A late-shape closure, for what anchors cannot express

`ui.defer(|late| …)` running post-arrange, with `late.rect_of(wid)` returning
*this* frame's rect. `GpuPaint` is the precedent; this is its CPU sibling.

Strictly more powerful than (A) and strictly more expensive: closures in slots,
re-entrancy, damage hashing over emitted output. Build (A) first; add this only
when something real needs it.

### C. A layout barrier — `ui.flush_layout()` mid-record

Run measure + arrange over everything recorded so far, then let the rest of the
record pass read this-frame rects by id. WPF's `UpdateLayout()`, scoped.

Darkroom already has the right shape for it: record the inner canvas with all
its nodes, close it, flush, then record wires against exact port centers using
completely ordinary code. No new shape kinds, no closures, no coordinate-space
rule, no arena. And it answers the cull, hit-test and view-fit cases too, not
just drawing.

Cheaper than it sounds — `MeasureCache` is keyed by
`(wid, subtree_hash, available)`, so the second measure is a hash compare. The
real cost is one extra arrange over the flushed subtree.

Needs a rect accessor that reads `Forest::current_node` + `Layout` directly
rather than going through `Ui::response_for`, which mixes in a cascade that is
still last frame's mid-record.

The risk is `getBoundingClientRect`: layout thrash is precisely what CSS anchor
positioning was invented to avoid. This has to stay one deliberate call, not
something widgets reach for casually.

A narrower variant is `ui.measure(|ui| …) -> Size` — record into a scratch
arena, measure only, return the size, discard. The direct answer to
`node_sizes`, but it records the subtree twice unless the measured tree is kept
and spliced in. At which point it has become —

### D. `SubcomposeLayout`

`ui.measured(id, |ui, available| …)`, with the record closure invoked from
inside `LayoutEngine::run`. Exact, single-pass, and the architecturally
heaviest item here: the layout engine has to re-enter `Forest::open_node`.

### E. Two cheap improvements to the existing mechanism

- `ui.is_sizing_pass()`, egui-style — so pass A can skip preview thumbnails,
  GPU views, and text rasterization it knows will be discarded. Small change,
  immediate win, no architecture involved.
- Scope it: `request_relayout_of(wid)` re-records one subtree instead of the
  frame. Subtree hashing makes this plausible.

### F. Cull against a conservative bound, not a measured size

Darkroom culls nodes at record time, which is *why* it needs sizes at record
time.

The tempting move — record everything and let the renderer cull — does not pay
off, and the palantir half of it already exists anyway: `encode_node` culls
off-screen subtrees against `Cascade::subtree_paint_rects`, with a second
damage-aware subtree cull beside it. What an off-screen node would still cost:

| pass | cost for an off-screen node |
|---|---|
| record | full user closure + shape records + hashing |
| measure | cheap — `MeasureCache` short-circuits on `(wid, subtree_hash, available)` |
| arrange | full per-node driver dispatch, no subtree skip |
| cascade | skipped on fingerprint match — but a pan changes every rect, so `can_update`'s `layout_hash` misses and it is a full rebuild |
| encode | already culled |

Panning a large graph is both the worst case and the common one. A record-time
cull avoids all of it; dropping it would be a regression.

What survives is the observation that culling never needed an *exact* size,
only a conservative upper bound — `CullRegion::keeps_node` already treats an
unknown size as "keep". Culling against `pos + MAX_NODE_SIZE` needs no cached
geometry, never culls something visible, and keeps the cull's full benefit.
That decouples culling from `node_sizes` without paying for off-screen nodes.

A caller-side change, though, not a palantir one.

## Design study: anchored shapes

What a clean, long-term (A) would actually look like against the current code.

### What already exists

Five facts decide the design.

1. **"Resolve a rect I do not know yet" is already the idiom.**
   `ShapeRecord::Quad` / `Image` / `Text` each carry
   `local_rect: Option<Rect>`, and `resolve_local_rect(owner_rect, local_rect)`
   (`src/renderer/frontend/encoder/mod.rs`) resolves `None` against the owner's
   arranged rect at encode time. Anchors are that mechanism with the target
   generalized from *the owner* to *any widget*.
2. **A live this-frame `WidgetId → (layer, node)` index exists.**
   `SeenIds::curr` (`src/scene/seen_ids/mod.rs`), filled by `record_endpoint`
   during record, reachable through `Forest::current_node`.
3. **`Layout::arranged_rect(Endpoint)` is complete the instant
   `LayoutEngine::run` returns.** With (2), `wid → this-frame rect` is two
   lookups and no new machinery.
4. **`paint_anims` is the structural template** — a per-tree registry of side
   data keyed by sorted shape index, registered through a sibling of
   `Ui::add_shape`, consumed in a later phase behind a monotone cursor.
5. **Coordinates sit where a patch can reach them.** Curve control points are
   inline (`CurveBasis::Cubic { p0..p3 }`), polyline points live in a
   `RecordStore` span. Both records carry a `bbox` that paint rects and damage
   read, so a resolve pass patches coordinates *and* recomputes bbox.

### Where resolution goes

`FrameCycle::post_record`, between `layout_engine.run` and
`cascade_engine.run`. Every arranged rect exists by then and nothing has yet
consumed shape geometry, so the cascade, encoder, composer and damage are all
untouched — they see ordinary shapes holding concrete numbers. Gated on an
"any anchors this frame?" counter, so an anchorless frame pays nothing.

Damage needs one addition. A shape row's `Paint.hash` is the *authoring* hash,
which does not change when an anchor moves; `Paint.screen` does, so most motion
is caught, but two ports swapping positions yields the same bbox *and* the same
authoring hash. The resolve pass therefore has to fold resolved coordinates
into the hash — one extra hash round, on anchored rows only.

### The coordinate-space rule

Resolve into **the shape owner's layout space**:
`anchor_rect.point(at) − owner_rect.min`, which is exactly the owner-local
`Vec2` every shape already stores. That equivalence is what buys the
"nothing downstream changes" property.

The constraint is that anchor and owner must share a transform chain. Routing
through the cascade's per-node transforms instead would be fully general, but
requires the cascade to have visited both nodes — which pulls resolution into
the cascade walk and forfeits the clean insertion point. Layout-space rule plus
a `debug_assert` is the better trade; CSS anchor positioning carries
essentially the same containing-block restriction.

### API

`Anchor` as a value type, so existing call sites keep compiling:

```rust
pub enum Anchor { Fixed(Vec2), Widget { id: WidgetId, at: RectPoint, offset: Vec2 } }
impl From<Vec2> for Anchor
```

Shape constructors take `impl Into<Anchor>`. `Fixed` resolves at lowering — no
arena entry, no pass cost. `Widget` pushes into an anchor arena beside the
gradient and polyline arenas and stamps the slot.

The alternative, an untyped `&[AnchorBinding { slot, .. }]` patch table, needs
a slot enum mirroring every record's field layout and grows with every shape
kind. Not worth it.

### The wrinkle

**Anchors alone do not finish a bezier connector.** `Wire::data` in darkroom
computes the interior control points from the endpoints, and not affinely:

```rust
let vertical  = ((p3.y - p0.y).abs() * 0.5).clamp(MIN_HANDLE, MAX_HANDLE);
let backreach = BACKREACH_GAIN * (p0.x - p3.x).max(0.0).sqrt();
```

Patching `p0` / `p3` leaves `p1` / `p2` stale — better than today, since the
endpoints land exactly and only the bow lags a frame during a node drag, but
not right.

Closing it needs something that runs *after* resolution. Either a richer
`CurveBasis::Link { p0, p3, rule: HandleRule }` variant — declarative and
alloc-free, but a fixed vocabulary — or a closure, which is general but boxes
per wire per frame and so violates the steady-state posture.

### Node-position anchoring is a separate tier

Anchoring a whole node's *position* to another widget (tooltips, popups,
context menus, dropdowns — all one frame late today) is a layout concern, not a
shape one: it needs arrange to be dependency-ordered.

It is tractable for the same reason CSS restricts anchors to out-of-flow
elements: if the anchored node's *size* does not depend on its anchor, measure
is unaffected and arrange can settle anchored nodes in a second sweep. Cycles
need detecting and breaking. Worth keeping in view, not worth building first.

## If only one thing ships

**(C), the layout barrier.** It is strictly less new API surface for strictly
more coverage: no shape kinds, no arena, no coordinate-space rule, no anchor
lowering — and it answers drawing, culling, hit-testing and view-fitting
together, where anchors answer only drawing. It also makes the authoring code
read normally, which is the thing every caller workaround in darkroom is
currently paying to fake.

**(A)** stays the better *steady-state* mechanism — declarative, alloc-free,
native to damage — and is the right second step. Building the barrier first
would also settle empirically how much of the anchor design is still wanted.

**(E)** is the cheap additive one: a sizing-pass flag changes no behavior until
something opts in.

## Sources

- [egui #843 — Investigate a multipass (two-pass) version of egui](https://github.com/emilk/egui/issues/843)
- [egui 0.29.0 release — Multipass, `UiBuilder`](https://github.com/emilk/egui/releases/tag/0.29.0),
  [Sizing-pass flag #4535](https://github.com/emilk/egui/issues/4535)
- [Inside SubcomposeLayout: Jetpack Compose's Most Misunderstood API](https://blog.shreyaspatil.dev/inside-subcomposelayout-jetpack-composes-most-misunderstood-api/),
  [SubcomposeLayout and BoxWithConstraints internals](https://www.revenuecat.com/blog/engineering/subcomposelayout-internals)
- [LookaheadScope](https://composables.com/compose-ui/lookaheadscope),
  [approachLayout](https://composables.com/compose-ui/approachlayout),
  [Animations with Lookahead in Jetpack Compose](https://proandroiddev.com/animations-with-lookahead-in-jetpack-compose-60423fe0d1a7)
- [Anchor preferences in SwiftUI](https://swiftwithmajid.com/2020/03/18/anchor-preferences-in-swiftui/),
  [Inspecting the View Tree (Anchor Preferences)](https://swiftui-lab.com/communicating-with-the-view-tree-part-2/)
- [Using CSS anchor positioning — MDN](https://developer.mozilla.org/en-US/docs/Web/CSS/Guides/Anchor_positioning/Using),
  [Introducing the CSS anchor positioning API](https://developer.chrome.com/blog/anchor-positioning-api)
- [Anchoring Floating UI in Flutter with CompositedTransformTarget/Follower](https://luci-studio.com/blog/anchoring-floating-ui-in-flutter-with-compositedtransformtarget-and-compositedtransformfollower-b6008a4a/)
- [Dear ImGui — auto-resize hidden-frame issues #8959](https://github.com/ocornut/imgui/issues/8959),
  [#1417](https://github.com/ocornut/imgui/issues/1417)
- [Slint — Property System & Reactive Bindings](https://deepwiki.com/slint-ui/slint/2.2-property-system-and-data-binding)
- [Ryan Fleury — UI, Part 3: The Widget Building Language](http://www.dgtlgrove.com/p/ui-part-3-the-widget-building-language),
  [Part 2: Every Single Frame](https://www.dgtlgrove.com/p/ui-part-2-build-it-every-frame-immediate)
