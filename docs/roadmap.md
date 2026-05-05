# Roadmap

impl widgetid instead of impl hash?


## Now — concrete, motivated, ready to start

- **ScrollView v1.** Plan in `docs/scrollview.md`. `InputEvent::Scroll` from
  winit, vertical-only widget, offset stored via `state_mut`. Unblocks the
  showcase's "tall content" gap.
- **Drag tracking on `Active` capture.** Track `press_pos` + `last_pos` on
  `InputState`; expose `drag_delta()` rect-independent so the pointer can
  leave the originating widget mid-drag. Pre-req for scrollbars + touch-drag.
- **Cross-frame measure short-circuit.** Key `(WidgetId, available, sizing)
  → desired` and skip measure for unchanged subtrees, the way WPF does with
  `_previousAvailableSize` and Masonry does with `MeasurementCache`. Composes
  directly with damage rendering: if measure didn't fire, encode cache stays
  valid.
- **Overlay / popup layer.** Tooltips, dropdowns, context menus, and modals
  must draw outside their parent's clip and above siblings regardless of
  pre-order. Typically a separate "always on top" tree merged into the
  encoder pass. Showcase feels half-built without it.
- **accesskit integration.** "One week if planned now, a month if not"
  (Masonry, via `references/SUMMARY.md`). Per-widget `accessibility_role` +
  dedicated tree pass. Cost grows with widget count — do it before the
  surface area gets big.

## Next — concrete, queued behind Now

### Persistent-state consumers

- **Focus subsystem.** Tab order, focus ring, keyboard nav, focus-on-disabled
  rules. Distinct from the state map — needs its own pass.
- **`TextEdit` widget.** One `cosmic_text::Editor` per `WidgetId` via
  `state_mut_with` (add the public API when this lands), glyph-level
  hit-test (`Buffer::hit`), IME, selection rendering as sibling shapes.
- **IME + clipboard plumbing.** Both required for `TextEdit`.

### Damage rendering

- **Multi-rect damage.** Replace the single union rect with N disjoint
  regions (clustered from the per-node dirty set). Avoids the 50% heuristic
  tripping when two unrelated corners change.
- **Incremental hit-index rebuild.** Only update `HitIndex` entries for
  dirty nodes (and any whose cascade row changed) instead of walking every
  node every frame.
- **Debug overlay.** Toggleable mode that flashes dirty nodes red and
  outlines the damage rect — trivial once the per-node dirty set has a real
  consumer.
- **Damage-aware encode replay.** Currently `damage_filter.is_some()`
  bypasses the encode cache entirely, so animated frames don't benefit. The
  cached cmds are already correct (full subtree, damage-independent); gate
  the replay on `screen_rect ∩ damage = ∅`.

### Invalidation

- **Property tracker / fine-grained dirty propagation.** Hash each widget's
  input bag per frame so the encode cache can decide invalidation without a
  full equality check on `(NodeHash, cascade row)`. Distinct from damage
  rects — this tracks data-input change, not screen-rect change.
- **`request_discard` equivalent for first-frame size mismatch.** When
  measure produces a different size than last frame (text reflow, cosmic
  shape miss), re-run the frame invisibly the way egui does. First-frame
  text widths are likely wrong today.

### Tooling

- **Profiling spans (tracy or puffin).** One-line `profile_function!` per
  pass; cheap and the "optimize aggressively" posture wants per-pass
  timings on demand.
- **Snapshot / golden-image renderer tests.** Pixel-diff each showcase tab
  against a checked-in reference; catches renderer regressions unit tests
  miss.
- **Pixel-snapping audit at fractional scales.** Yoga shipped accumulating
  1px gaps at scale=1.5; Taffy fixed it (commit aa5b296). Add tests at
  1.25 / 1.5 / 1.75 to pin behavior.
- **Color-space verification.** Glyphon outputs sRGB; confirm text doesn't
  look faded on a linear surface format and document the rule. Applies to
  every shape — verify surface format matches shader assumptions and pin a
  test.
- **HiDPI / scale-factor change handling.** Per-monitor DPI changes
  mid-session must invalidate atlas, text shape cache, and the proposed
  layout cache.

## Later — real work, gated on a workload

### Text

- **Layer B — `CosmicMeasure.cache` eviction.** Refcount `TextCacheKey` by
  live `WidgetId`s; sweep via `SeenIds.removed()` so the shaped-buffer
  table doesn't leak. Defer until a string-churn workload demonstrates the
  leak.
- **`Shape::Text.text: String` allocs.** Each `Text::show` clones into the
  shape every frame. Move to `Cow<'static, str>` for static labels; intern
  dynamic strings via `Arc<str>` keyed on `text_hash`. Profile-gate before
  shipping.
- **Atlas eviction under multi-font / multi-size load.** Verify
  `atlas.trim()` + glyphon's shelf overflow holds up over a long session.
- **Wallclock bench for the reuse cache.** `benches/layout.rs` runs without
  cosmic, so it can't see the Layer A win. Add a cosmic-enabled variant
  with N=100 static labels and quote real µs/frame numbers.

### Caches

- **Cross-frame intrinsic-query cache.** `LayoutEngine::intrinsic` is
  intra-frame only. A second column keyed on `subtree_hash + axis + req`
  would compose cleanly. Skip until a workload proves it matters.
- **Real-workload validation (measure cache).** Bench numbers are
  synthetic. The showcase doesn't push against the 400 µs ceiling, so the
  cache's user-visible win is unverified.
- **Subtree-granularity encode cache.** Replay a contiguous range when no
  descendant is dirty, instead of N per-node slice replays. Cheaper memcpy
  and pairs with a Vello-style flat stream representation.
- **Hit-hint propagation between caches.** Both caches key on `(WidgetId,
  subtree_hash, available_q)` and sweep on the same `removed` list, so a
  measure-cache hit implies an encode-cache hit. Layout writes a
  `Vec<bool>` (or packed bit on `LayoutResult`) marking measure-cache-hit
  roots; encoder reads the bit and skips its own `FxHashMap::get`. Tiny
  per-call, only sound while the two caches stay eviction-locked.
  Profile-driven.

### Renderer / GPU

- **Instance buffer capacity-retention audit.** Confirm encode → compose →
  backend retains `Vec` capacity across frames. The alloc harness covers
  Ui-side state but doesn't pin the renderer pipeline. Iced, quirky, and
  makepad all keep typed instance buffers across frames.
- **wgpu staging belt / upload pool.** Replace ad-hoc `queue.write_buffer`
  calls with `wgpu::util::StagingBelt` to batch instance + uniform uploads.
- **Offscreen render targets / mask layer.** No render-to-texture path
  today, which blocks real drop shadows beyond SDF, blur, masked
  compositing, and tab transitions. Mark as a known fork point in
  `DESIGN.md`.
- **Push constants vs shared UBO for camera/scissor.** Open question from
  `references/SUMMARY.md §12.5`. UBO works on stock wgpu (quirky proves
  it); document the choice.

### Input

- **Event coalescing / key repeat / double-click timing.** winit delivers
  raw events; UI conventions (250ms double-click window, OS key-repeat
  rate, mouse-motion coalescing) need a centralized layer.
- **Drag-and-drop with MIME-typed payloads.** Distinct from
  drag-tracking-with-`Active`-capture — needs payload typing, drop targets,
  OS file drops.

### Layering

- **Explicit z-order beyond pre-order.** Clay's `zIndex` field on render
  commands is the model; becomes relevant once popups exist.
- **Multi-window / multi-viewport.** egui's `Viewport` + per-surface
  `IdMap<PaintList>` is the reference design. Single-surface today.

### Long-list / scroll

- **Virtualization / windowed children.** Once scroll exists. Prefer a
  "virtual children" hook on a single node yielding measured children for
  the visible window over Flutter's heavyweight sliver protocol.

### i18n

- **RTL / mirroring.** cosmic-text handles BiDi glyph-side, but stack/grid
  arrangement and alignment defaults need an LTR/RTL flag.

### Tooling

- **Per-frame scratch arena.** A project-wide `bumpalo` for things that are
  genuinely per-frame transient, instead of every pass solving
  capacity-retention separately.

### Damage (lower-impact)

- **Tighter damage on parent-transform animation.** A dedicated
  transform-cascade pass to collapse deep-subtree damage to a tight bound;
  only worth it if profiling shows the current union is too coarse.
- **Manual damage verification.** Visual A/B against `damage = None` to
  catch the case where the diff misses something.

## Speculative — profile-gated micro-wins, defer indefinitely

- **SIMD `bump_rect_min`.** Replay loop reads/writes 2× f32 per rect-bearing
  cmd (~12 800 ops on the nested workload). Precompute a bit-per-cmd
  "rect-bearing" mask alongside the kinds array; `bump_rect_min` then
  vectorizes over rect payloads. Only worth it if profiles show this loop
  hot.
- **Tiny-subtree threshold (encode cache).** Caching a 1–2-cmd subtree
  costs more in hashmap probe + `write_subtree` bookkeeping than it saves.
  Add a `min_cmds_for_cache` (≈4) gate before `write_subtree`.
- **Coarser `available_q` quantization (encode side).** 1-logical-px
  granularity may bust the cache on sub-pixel parent drift. Bump to 2 px or
  4 px if a profile shows hash-match / avail-mismatch as a frequent miss
  path.
- **Coarser `available` quantization (measure side).** Currently 1 logical
  px. If jittery `Fill` children show cache misses on sub-pixel parent
  drift, bump granularity. Wait for evidence.
- **Cold-cache mitigations (measure cache).** If a workload ever shows
  resize-frame jank, candidates: skip snapshot writes for collapsed
  subtrees, gate writes by subtree-size threshold, amortize compact across
  frames.
- **Spatial index for hit-test at high N.** `HitIndex` is O(1) by-id but
  pointer→node walks the cascade table; quad-tree / BVH only matters at
  thousands of nodes but the data is there. Profile-gated.
- **Contiguous children slices.** Clay's `children.elements: int32_t*` into
  a shared array beats linked-list children for cache locality and BFS.
  SUMMARY §5 marks this as "strictly better, defer until profiling
  justifies."
