# Roadmap

Scratch ideas: impl WidgetId instead of impl Hash; 
ECS for SoA;
Spacing serializable nicely.
 surface.paint.add_to(ui);  - direct - surface add_to and no clipping info in element

## Now

- **Drag-to-pan scrollbar thumb.** Replace overlay shapes with per-axis
  bar leaf nodes (`Sense::Drag`, derived ids `("scroll-vbar", parent_id)`).
  `state.offset.main += drag_delta * (content - viewport) / (track - thumb)`,
  clamp. Click-to-page + hover-grow fall out once leaves exist.
- **`Scroll::scroll_to(WidgetId)`.** Compute target rect from
  `LayoutResult.rect`, set `ScrollState.offset`, clamp. One-frame-stale
  for just-recorded targets — defer the fallback.
- **Overlay / popup layer.** Tooltips, dropdowns, menus, modals draw
  outside parent clip + above siblings. Separate "always on top" tree
  merged into encoder.
- **accesskit integration.** Per-widget `accessibility_role` + dedicated
  tree pass. Cost grows with widget count — do early.

## Next

### TextEdit — remaining

v1 ships single-line typing, caret, codepoint backspace/delete +
arrows + home/end, click/drag-to-place, focus + `FocusPolicy`,
escape-to-blur, IME `Commit`. See `src/widgets/text_edit/design.md`.

- **Selection (visible + edits).** Selection-fill `Overlay` under text;
  shift+arrow / home/end / drag extends, plain arrow collapses,
  ctrl+a all-select; edits replace selected range. State + theme
  slots already there.
- **Glyph hit-test via `Buffer::hit`.** Replace O(n) `caret_from_x`
  scan with one shaped lookup. Same upgrade gives multi-line
  `byte_to_xy`.
- **Grapheme-aware boundary walks.** `unicode-segmentation` once
  selection lands.
- **Multi-line.** Enter inserts `\n`, PageUp/Down live, caret y from
  `Buffer::hit`, `TextWrap::Wrap` when builder sets `multiline`.
- **Clipboard.** `arboard` behind `Clipboard` trait on `Ui`; route
  ctrl/cmd+c/x/v from `frame_keys`.
- **IME preedit.** Currently dropped at translation. Plumb
  `InputEvent::ImePreedit { text, cursor }`, render underlined under
  caret, commit on `Ime(Commit)`.
- **`Ui::wants_ime()`.** Host gates `set_ime_allowed(true)` instead of
  unconditional.
- **Undo / redo.** Bounded ring buffer per `TextEditState`, coalesce by
  edit-kind + timestamp. Needs shortcut routing.
- **Caret blink.** Tick alpha off `dt` once an animation-tick infra
  consumer exists.

### Focus — remaining

v1 ships `focused`, `FocusPolicy`, programmatic `request_focus`,
click-to-focus, eviction-on-removal, escape-to-blur.

- **Tab cycling.** `Tab` / `Shift+Tab` over the cascade in pre-order,
  skipping non-focusable / disabled. Multi-line editors opt into
  consuming Tab.
- **Focus ring.** Centralized `focused`-state outline shape so a11y /
  high-contrast can boost it.
- **Focus-on-disabled rule.** Going disabled while focused should
  release focus. Pin it.
- **Focus restoration.** Optional remember-and-restore when a focused
  widget vanishes (modal-close → restore caller).

### Scroll polish

- **Wheel step from font metrics.** Drop fixed 40 px/line
  (`SCROLL_LINE_PIXELS`); use line-height of dominant font in
  scrolled content.

### Damage rendering

- **Multi-rect damage.** N disjoint regions instead of one union;
  avoids 50 % heuristic tripping on unrelated corners.
- **Incremental hit-index rebuild.** Only update `HitIndex` for dirty
  + cascade-changed nodes.
- **Debug overlay.** Flash dirty nodes + outline damage rect.
- **Damage-aware encode replay.** Today `damage_filter.is_some()`
  bypasses encode cache; gate replay on
  `screen_rect ∩ damage = ∅` instead.

### Invalidation

- **Property tracker.** Per-widget input-bag hash so encode cache
  decides invalidation without `(NodeHash, cascade)` equality.
- **`request_discard` for first-frame size mismatch.** Re-run frame
  invisibly when measure differs from last frame (text reflow,
  shape miss). egui-style.

### Tooling

- **Profiling spans (tracy / puffin).** `profile_function!` per pass.
- **Snapshot / golden-image renderer tests.** Pixel-diff showcase tabs.
- **Pixel-snapping audit at fractional scales** (1.25 / 1.5 / 1.75).
  Yoga shipped 1px gaps; Taffy fixed (aa5b296).
- **Color-space verification.** Confirm Glyphon sRGB output on linear
  surface; pin a test.
- **HiDPI / scale-factor change handling.** Per-monitor DPI changes
  must invalidate atlas + text cache + (future) layout cache.

## Later — workload-gated

### Text

- **`CosmicMeasure.cache` eviction (Layer B).** Refcount
  `TextCacheKey` by live `WidgetId`s, sweep via `SeenIds.removed()`.
- **`Shape::Text.text` allocs.** `Cow<'static, str>` for static labels;
  intern dynamic via `Arc<str>` keyed on text hash.
- **Atlas eviction under multi-font / multi-size load.**
- **Wallclock bench for the reuse cache.** `benches/layout.rs` runs
  without cosmic — add a cosmic variant for real µs/frame numbers.

### Caches

- **Cross-frame intrinsic-query cache.** Key on
  `subtree_hash + axis + req`.
- **Real-workload validation (measure cache).** Bench numbers are
  synthetic; showcase doesn't push the 400 µs ceiling.
- **Subtree-granularity encode cache.** Replay contiguous range when
  no descendant dirty; pairs with Vello-style flat stream.
- **Hit-hint propagation between caches.** Measure-cache hit implies
  encode-cache hit (same key, eviction-locked); skip encoder's
  `FxHashMap::get`.

### Renderer / GPU

- **Instance buffer capacity-retention audit.** Confirm encode →
  compose → backend retain `Vec` capacity.
- **wgpu staging belt.** Replace ad-hoc `queue.write_buffer` with
  `StagingBelt`.
- **Offscreen render targets / mask layer.** Blocks real drop shadows,
  blur, masked compositing, tab transitions.
- **Push constants vs shared UBO** for camera / scissor (SUMMARY §12.5).

### Input

- **Event coalescing / key repeat / double-click timing.** Centralize
  the 250 ms window, OS key-repeat, mouse-motion coalescing.
- **Drag-and-drop with MIME-typed payloads.** Distinct from
  `Active`-capture drag; needs payload typing + drop targets +
  OS file drops.

### Layering

- **Explicit z-order beyond pre-order.** Clay's `zIndex` model;
  relevant once popups exist.
- **Multi-window / multi-viewport.** egui's `Viewport` +
  `IdMap<PaintList>`. Single-surface today.

### Long-list / scroll

- **Virtualization** — virtual-children hook over Flutter's slivers;
  only path to O(viewport) measure.
- **Inertia scrolling** — velocity decay + `request_repaint`. Needs
  animation-tick consumer.
- **Bounce / rubber-band.** Pure feel.
- **Touch drag.** No touch plumbing today.
- **Keyboard scrolling** (PgUp/Dn/Home/End). Needs focus.
- **Sticky / pinned headers.** Non-trivial layout integration.
- **Nested scroll-chaining.** Browsers chain to parent at child end;
  v1 = innermost wins.

### i18n

- **RTL / mirroring.** cosmic-text handles BiDi glyph-side; stack/grid
  arrangement + alignment defaults need an LTR/RTL flag.

### Tooling

- **Per-frame scratch arena (`bumpalo`).** Replace per-pass capacity
  retention with one shared arena.

### Damage (lower-impact)

- **Tighter damage on parent-transform animation.** Dedicated
  transform-cascade pass collapsing deep-subtree damage.
- **Manual damage verification.** Visual A/B against `damage = None`
  to catch missed diffs.

## Speculative — profile-gated

- **Skip cascade/encode recursion under empty clip.** Composer-level
  cull already drops leaves; recursion skip trickier (Active /
  future focus may want off-screen live).
- **SIMD `bump_rect_min`.** Bit-per-cmd rect-bearing mask, vectorize
  over rect payloads.
- **Tiny-subtree threshold (encode cache).** `min_cmds_for_cache` ≈ 4
  before `write_subtree`.
- **Coarser `available_q` quantization** (encode and/or measure).
  Bump from 1 px on sub-pixel parent drift.
- **Cold-cache mitigations (measure cache).** Skip-collapsed,
  size-threshold, amortized compact — if resize jank shows.
- **Spatial index for hit-test at high N.** Quad-tree / BVH; matters
  at thousands of nodes.
- **Contiguous children slices.** Clay's `int32_t*`-into-shared-array
  for cache locality and BFS (SUMMARY §5).
