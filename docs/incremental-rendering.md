# Incremental rendering — research

Render only what changed since the last frame. Investigation of how
other libraries do it, what fits palantir's architecture, and a
staged proposal.

## Why bother

Today every frame runs the full pipeline regardless of whether
anything changed:

- Record (user code rebuilds the `Tree` from scratch).
- Layout (measure + arrange over the whole tree).
- Encode (tree → `Vec<RenderCmd>`).
- Compose (logical → physical px, scissor groups).
- wgpu upload + draw + present.

For a 100-node static UI this is sub-millisecond CPU + a few ms GPU.
At 60 fps with a 16 ms budget we have headroom — but at idle we're
burning power redrawing identical pixels. The real wins of
incremental rendering are:

1. **Battery / energy** — most UIs sit idle most of the time.
2. **Letting the OS compositor skip our window** — when nothing
   changed, the compositor doesn't have to re-composite our surface.
3. **Animation smoothness** — when only one thing animates, you
   don't want a hundred unrelated things re-rasterizing each frame.

## How others do it

Survey of approaches across architectures.

### Browser compositors (Blink / Chromium)

Most sophisticated: a multi-thread, multi-stage pipeline with paint
layers, raster invalidation, and compositor-thread scrolling.
DOM is partitioned into **paint layers**, each with its own GPU
texture. Per-frame work:

- **Paint invalidation** — main thread marks document regions dirty.
- **Raster invalidation** — only the dirty parts of each layer are
  re-rastered.
- **Compositor thread** — composites cached layer textures with
  transforms (so scrolling rarely repaints, just moves textures).

Trade-offs: huge engineering cost; sophisticated pre-paint passes;
bugs around `transform`/`will-change`/`opacity` hints inflating the
layer count. Years of tuning.

### Flutter

Retained scene-graph at engine level. Each frame produces a `Layer`
tree; layers retain GPU resources between frames. Embedder API has
**dirty region management** — tracked as one or more rectangles per
frame. Multiple rectangles fall back to full repaint (engineering
trade-off: union vs. multi-pass).

Reported speedup: ~2× on loading-spinner workloads, well below the
16 ms frame budget.

Trade-offs: requires the host (embedder) to plumb damage regions
into its present mechanism. Vulkan-based embedders historically
didn't, so they did full-screen repaint until additional plumbing
landed.

### SwiftUI

Tree-diff reconciliation at value level. Per-frame:

- A new immutable view tree is computed.
- Diffed against the previous tree using identity keys.
- Only changed subtrees re-render.

Doesn't operate on damage rectangles — operates at view-graph level.
Identity stability is critical: when identity breaks (e.g.
rearranging without keys), entire subtrees rebuild even if they
look the same. Same trap React has.

### LVGL (embedded)

Object-level `lv_obj_invalidate()` marks a rectangle dirty.
Refresh cycle merges overlapping rectangles before drawing.
`LV_INV_BUF_SIZE` (default 32) caps tracked rectangles before
falling back to full-screen invalidation.

Pragmatic for low-power MCUs: minimal pixel work per frame, but
tightly coupled to the imperative `widget.invalidate()` model.

### egui / imgui

Both are pure immediate-mode and re-tessellate everything every
frame. "Partial rendering" in egui means **frame skipping** — if
no input arrived and nothing requested a repaint, the frame doesn't
run at all (`Context::request_repaint()` opts back in for
animations / async updates). Within a frame, no partial drawing.

This is the cheapest model: simple, no diffing, no damage tracking.
Works because re-tessellating a typical UI is fast (sub-millisecond).

### wgpu present-with-damage

`VkPresentRegionKHR` / `EGL_KHR_swap_buffers_with_damage` exist on
native APIs but **not in wgpu** ([gfx-rs/wgpu#682](https://github.com/gfx-rs/wgpu/issues/682),
closed as not-planned). The WebGPU spec doesn't expose it.

Practical implication: even if we computed a damage rect, we can't
hand it to the OS compositor through wgpu. Partial redraw through
wgpu has to live at the GPU-work level (scissor + persistent
backbuffer), not at the present level.

## What palantir can actually do

Mapping the above to our architecture:

| Approach | Fit? | Cost | Win |
|---|---|---|---|
| **A. Frame skipping** (egui-style) | Excellent | ~50 LOC + winit hook | Skip the entire frame on idle. Dominates everything else for typical UIs. |
| **B. Layout / encode skip** when tree unchanged | Awkward | Hashing + cache invalidation | Saves CPU only. Most frames already cheap. |
| **C. Damage-region rendering** (CPU cull + scissor) | Real | ~500–800 LOC: change detection + damage rect + persistent backbuffer + encoder filter | When the change is small (hover, cursor, counter), shrinks CPU+GPU work proportionally to damage area. **Doesn't reach the OS compositor — that part is blocked by wgpu.** |
| **D. Layer cache** (Flutter-style) | Major | Per-layer offscreen RTs, diff, composite | Huge architectural shift. Real savings on complex animation; overkill for static UIs. |

### Why immediate-mode makes B genuinely awkward

Our tree is rebuilt every frame from user code. To skip layout/encode,
we'd need to detect "tree is identical to last frame." Options:

- **Hash the recording stream** as it's pushed (running hash of every
  `add_shape` / `push_node`). Compare to last frame's hash. If equal,
  reuse last frame's `RenderBuffer`.
- **Stable `WidgetId` per node** + per-node hash of authoring data.
  Diff against previous frame's tree.

Either way you pay the recording cost (user code runs). Recording
itself is already cheap (~50ns per node), so hashing on top to save
the layout pass that's also ~50ns/node is borderline.

The exception: **layout-heavy work** (cosmic-text shaping, grid
column resolution at scale). But cosmic already caches text shapes
across frames; grid resolution is arithmetic. The genuinely-expensive
work is already cached.

### Why C (damage-region rendering) is partial — what wins, what doesn't

Damage-region rendering has three independent levels of saving.
Two of them are reachable through wgpu; one isn't.

**Level 1 — CPU-side culling (reachable, biggest CPU saving).**
Once we know "only this rectangle changed," the encoder skips
emitting `DrawRect` / `DrawText` for any node whose `layout.rect`
doesn't intersect damage. The composer's `Vec<Quad>` shrinks
proportionally to the damaged-vs-untouched ratio. For a UI with one
hovering button on a 100-quad surface, the cmd stream shrinks from
~100 quads to ~1.

This drops:
- Quad upload bytes (uploaded `Vec<Quad>` shrinks).
- Vertex shader invocations.
- Glyphon `prepare()` work for skipped text runs (per-glyph atlas
  lookups, vertex emission).

For small-change frames this is the biggest win in the entire
pipeline — most UI frames change a small fraction of pixels.

**Level 2 — GPU-fragment scissor (reachable, smaller GPU saving).**
`pass.set_scissor_rect(damage)` discards fragment shader
invocations outside the rect. Required for correctness when an
oversize quad partially intersects damage and we don't want it
painting outside. Combined with `LoadOp::Load` (which keeps last
frame's pixels in the backbuffer), the surface outside damage is
visually identical to last frame.

Requires the swapchain configured to preserve previous contents.
On platforms where it doesn't (some compositors with `Mailbox`
present mode), we'd maintain an owned offscreen texture as the
"last frame" and blit the damage region from it to the new
swapchain image each frame.

**Level 3 — OS compositor damage (NOT reachable through wgpu).**
The OS compositor still treats our whole surface as
"potentially changed" because wgpu's `Surface::present()` takes no
damage argument. Native APIs that accept it
(`VkPresentRegionKHR`, `EGL_KHR_swap_buffers_with_damage`,
Wayland's `surface_damage_buffer`) don't have a wgpu pass-through.
This is the only level that translates to real *system-wide energy
savings* (compositor blends, drop-shadow recomputation, scan-out).

So C is genuinely useful for shrinking palantir's own pipeline
work but doesn't unlock the system-level battery win — that path
remains blocked by [gfx-rs/wgpu#682](https://github.com/gfx-rs/wgpu/issues/682).

### Why D (layer cache) is the long-term answer if we ever animate heavily

Per-subtree offscreen render target + composite to surface gets you
"only changed subtrees re-rasterize." Real win when one subtree
animates while the rest is static. But:

- Each layer = an offscreen texture (memory cost).
- Need to decide *which* subtrees become layers (heuristic or
  user-flag like CSS `will-change`).
- Compositing pass each frame.
- Dirty propagation per layer.

This is a months-long project, not a session.

## Recommendation: ship Stage 1, plan for Stage 3, defer the rest

Stage 1 (frame skipping) handles the "nothing changed" case.
Stage 3 (damage-region rendering) handles the "small change" case.
Together they cover most real UI workloads. Stage 2 turns out to
be redundant once Stage 3 lands; Stage 4 is months of work for
animation-heavy scenarios we don't have.

### Stage 1 — Frame skipping (ship next)

Add a `should_repaint` mechanism modeled after egui:

```rust
impl Ui {
    /// Returns true if the next frame must rebuild + render. False
    /// means the previous frame's pixels are still valid; the host
    /// can skip the whole pipeline.
    pub fn should_repaint(&self) -> bool { ... }

    /// Force a repaint — typically called from animation frames or
    /// async tasks that just produced new state.
    pub fn request_repaint(&self) { ... }
}
```

Triggers for a repaint:

- New input event since last frame (mouse move/click, keyboard,
  resize, scale-factor change).
- An active interaction state (hover, press) where the rendered
  output depends on hover/press.
- Explicit `request_repaint()` from user code (animations, async).

Host integration (winit):

```rust
fn redraw(&mut self) {
    if self.ui.should_repaint() {
        // Run the full pipeline.
        self.ui.begin_frame();
        build_ui(&mut self.ui);
        self.ui.layout(...);
        self.ui.end_frame();
        self.backend.submit(...);
    }
    // Otherwise: do nothing. The OS compositor keeps showing the
    // last presented frame, no GPU/CPU work this tick.
}
```

Cost: ~50 LOC across `Ui`, the input dispatch, and the showcase
host. No layout-pass or backend changes.

Win: idle frames cost zero CPU/GPU. Battery savings on laptops.
Real-world effect: a desktop that's already producing 60 fps for a
static UI now produces 0 fps when nothing changes.

Tests:
- After `begin_frame` / `end_frame` with no input deltas, calling
  `should_repaint()` returns false.
- After an `InputEvent::PointerMove`, returns true for at least
  one frame.
- After `request_repaint()`, returns true on the next call.

### Stage 2 — Conditional layout skip (mostly redundant once Stage 3 lands)

Skip layout/encode/compose entirely when the recorded tree is
identical to last frame. Detect via per-frame tree hash;
on hit, reuse last `RenderBuffer`. Saves CPU only.

Mostly redundant once Stage 3 ships — Stage 3 already culls down
to damaged nodes, so the residual cost of running through the full
tree is small. Don't bother unless a profile after Stage 3 shows
the unchanged-tree path is still expensive.

### Stage 3 — Damage-region rendering (medium project, real win)

Three pieces, in dependency order:

**3a. Per-node change detection.** For each node, hash its
authoring data (rect, fill, stroke, text content/color/size,
visibility cascade). Compare to the previous frame's hash for the
same `WidgetId`. Differences mark the node as dirty. Persist
hashes across frames keyed on `WidgetId`.

**3b. Damage rect computation.** Union the bounding boxes of all
dirty nodes (in screen space, post-transform/clip cascade) into
one rect. If the damage union exceeds a threshold (e.g., 50% of
surface), fall back to full repaint — same trick LVGL uses with
`LV_INV_BUF_SIZE`.

**3c. Pipeline filtering.** Encoder skips emitting `DrawRect` /
`DrawText` for nodes whose rect doesn't intersect damage. Backend
sets `LoadOp::Load` (so untouched pixels persist), wraps the pass
in a damage scissor, and uses an owned persistent backbuffer when
the swapchain doesn't preserve previous contents.

Estimated effort: ~500–800 LOC across `Tree` (hashes), encoder
(filter), composer (scissor unchanged — same plumbing today), and
backend (`LoadOp::Load` + persistent texture). Test surface needs
both small-change frames and large-change frames (fallback to
full repaint).

Win: small-change frames shrink quad upload + vertex/fragment
work + glyphon prepare to roughly damaged-area proportional
cost. **Does not** save the OS compositor's recomposite — that
remains [wgpu's missing capability](https://github.com/gfx-rs/wgpu/issues/682).

### Stage 4 — Layer cache (skip — premature)

Reach for this only when a real animation workload demands it. Until
then, the architectural investment doesn't pay back.

## Open questions / future revisits

- **Animation primitives.** Once palantir grows tween/spring helpers,
  Stage 1's `request_repaint()` becomes the integration point. Worth
  designing the mechanism with that in mind (a ticker that auto-
  repaints while any animation is in flight).
- **wgpu damage region support.** Track [#682](https://github.com/gfx-rs/wgpu/issues/682);
  if it ever lands, Stage 3 unlocks meaningful power wins for the
  partial-update case (scrolling UIs, video panels).
- **WidgetId stability.** Stage 2 (and a future Stage 4) both rely
  on `WidgetId` being stable across frames for the same logical
  widget. Today's `auto_stable()` (call-site hash) does the right
  thing; per-iteration ids in loops still need explicit keys.

## Sources

- [Chromium "How cc Works"](https://chromium.googlesource.com/chromium/src/+/master/docs/how_cc_works.md)
- [Chromium GPU-accelerated compositing](https://www.chromium.org/developers/design-documents/gpu-accelerated-compositing-in-chrome/)
- [Flutter dirty-region management PR](https://github.com/flutter/engine/pull/35022)
- [Flutter retained-rendering RFC](https://github.com/flutter/flutter/issues/21756)
- [SwiftUI diffing engine — Rens Breur](https://rensbr.eu/blog/swiftui-diffing/)
- [LVGL rendering pipeline & invalidation](https://deepwiki.com/lvgl/lvgl/4.1-drawing-pipeline-and-image-processing)
- [egui Discussion #1948 — repaint mechanism](https://github.com/emilk/egui/discussions/1948)
- [wgpu #682 — present with damage (closed, not planned)](https://github.com/gfx-rs/wgpu/issues/682)
- [EGL_KHR_swap_buffers_with_damage spec](https://registry.khronos.org/EGL/extensions/KHR/EGL_KHR_swap_buffers_with_damage.txt)
