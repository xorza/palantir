# ScrollView — implementation plan

Short plan for adding a `ScrollView` widget. Status: not started; this
file is design-stage only.

## What we already have

The rendering side is mostly in place — scroll = clip + translate, both
exist:

- **Clip**: `Element.clip` → `PaintAttrs::is_clip()` (bit 4); resolved
  in `cascade.rs:188` (intersects screen-rect with parent clip);
  applied in `renderer/frontend/encoder/mod.rs:51` via
  `push_clip`/`pop_clip`. Clip is pre-transform; transform applies
  inside.
- **Transform**: `Element.transform: Option<TranslateScale>` in extras;
  composed in `cascade.rs:183` (`parent_transform.compose(node_transform)`);
  affects descendants only. `examples/showcase/transform.rs` exercises it.
- **Measure-to-arrange contract**: drivers report desired sizes
  unchanged; `support.rs:21` clamps to `[min, max]` but children can
  paint past their slot — exactly what scroll needs (content > viewport).

## What's missing — the actual blockers

In rough dependency order:

1. **Scroll-wheel input**. `InputEvent` (`src/input/mod.rs:21`) has no
   `Scroll` variant; `from_winit` ignores `WindowEvent::MouseWheel`.
   Add `InputEvent::Scroll(Vec2)` (logical-pixel delta after
   `LineDelta` → pixel conversion using font metrics or a fixed step),
   route through `on_input` to the hovered scroll-capable widget.
2. **Drag delta on `Active` capture** (TODO in CLAUDE.md). Needed for
   touch / scrollbar-thumb dragging. Track `press_pos` + `last_pos` on
   `InputState`; expose `drag_delta()` rect-independent. Independent of
   scroll itself but enables scrollbar interaction.
3. **Content-size negotiation**. ScrollView's measure must pass
   `LenReq::Unbounded` along the scrolled axis to its child so the
   child reports its full intrinsic size; ScrollView returns its own
   `Hug`/`Fixed` size to the parent. Cross axis behaves like a normal
   panel. The intrinsic protocol (`src/layout/intrinsic.md`) already
   supports per-axis unbounded queries.

## Widget shape

`Scroll` widget = a single Element node that:

- carries `clip = true`
- owns one logical child (the content). At record time the user
  writes `Scroll::new().vertical().show(ui, |ui| { ... })` and the
  closure records children directly under the scroll node — same
  pattern as `Panel`.
- in measure: queries content with unbounded main axis; reports its
  own `Sizing` (typically `Fill` or `Fixed`) outward; stashes
  `content_size` for arrange.
- in arrange: clamps offset to `[0, content_size - viewport].max(0)`,
  writes `Element.transform = TranslateScale::from_translation(-offset)`
  so the cascade walks already-computed children with the translation
  applied. **No extra layout pass.**
- pulls/pushes scroll offset through the state map keyed by its
  `WidgetId`.
- consumes scroll input when hovered + content overflows.

Arrange-time mutation of `transform` is the load-bearing trick: it lets
us add scroll without touching the layout drivers. The cascade reads
`transform` after arrange finishes, so this is safe.

## Slicing into shippable steps

Each step lands with tests + (where visible) a showcase tab.

1. **`InputEvent::Scroll` + winit translation.** Unit-test
   `LineDelta`/`PixelDelta` mapping. `InputState` exposes
   `frame_scroll_delta(): Vec2`, cleared each frame.
2. **`Scroll` widget — vertical only, no scrollbar.** Clip + transform
   trick, offset clamp, consumes wheel when hovered. Showcase tab: tall
   text column inside a fixed-height scroll. Test: arrange produces
   expected `transform` for a given offset; offset clamps when content
   shrinks.
3. **Horizontal axis + both-axes.** Same widget, axis flag.
4. **Drag delta on `Active`.** Plumbing only; reuse for touch-drag
   scroll on the existing widget.
5. **Scrollbars.** Separate widget overlay drawn on top of the scroll
   node; reads offset/content/viewport from its parent's state row.
   Thumb drag uses `drag_delta`.

Out of scope for v1: momentum/overscroll/elastic, virtualization (see
`docs/roadmap.md` "Virtualization / windowed children"), nested
scroll-chaining policy, sticky headers.

## Off-screen cost — what each pass does

Without virtualization, scroll content pays full per-pass cost
regardless of what's visible:

- **Measure / arrange**: unconditional on every node. ScrollView passes
  `LenReq::Unbounded` along the scrolled axis, so the child reports its
  full intrinsic size; arrange positions every grandchild at its
  natural position. Inherent — measure has to know total content size
  to clamp the offset. Cost is O(content), not O(viewport).
- **Cascade**: walks every node. Clip intersection in `cascade.rs:188`
  shrinks off-screen rects to empty, which is what makes hit-testing
  correctly ignore them. Cheap.
- **Encode**: walks every node pre-order; today emits leaf shapes for
  every visible (non-`Hidden`) node regardless of clip. The encoder
  already has a `damage_filter: Option<Rect>` path
  (`renderer/frontend/encoder/mod.rs:25,68`) that skips leaf emission
  when a node's screen rect misses the filter — built for damage
  rendering, but exactly the mechanism we want for clip culling.
- **GPU**: scissor discards off-screen fragments, so no shading. CPU
  encode + compose + instance-buffer write still runs.

So a 10k-row list in a 600px viewport still does 10k of measure,
arrange, cascade, encode every frame, with the GPU scissoring ~9.97k of
the emitted instances. Correct, but only fine up to hundreds of items.

### Cheap wins to fold in

1. **Clip-cull the encoder.** Reuse the `damage_filter` machinery:
   while a `push_clip` is active, skip leaf shape emission for
   descendants whose screen rect doesn't intersect the current clip.
   Push/pop pairs still emit so composer state stays coherent. ~Free
   order-of-magnitude on encode for tall scroll content; also helps
   any `clip = true` panel.
2. **Skip cascade/encode recursion under empty clip.** When a subtree
   root's screen rect is fully outside the root viewport, short-circuit
   descent. Trickier — `Active` capture and (future) focus may want
   off-screen rects to stay live. Defer until a workload asks.

Real virtualization (the "virtual children" hook in
`roadmap.md`) is the only path to O(viewport) measure cost,
and is a separate, larger project. v1 ships unvirtualized scroll that's
correct; clip-cull #1 above buys another order of magnitude on encode.

## Open questions

- **First-frame size**: scroll content desired size is unknown frame 0
  → offset clamp is wrong frame 0. Acceptable (one-frame visual blip)
  or trigger a `request_discard`-style invisible re-run? Defer; couple
  to the request-discard work in `roadmap.md`.
- **Scroll capture vs hover**: should a scrolled-into widget keep
  focus/hover during the scroll, or does the gesture suppress hover?
  Match egui (suppress) unless a workload says otherwise.
