# Text — status & remaining work

Tracks the move from the historical `chars * 8.0` placeholder to real shaped,
wgpu-rendered text. Phases 1–4 plus the architectural cleanup around them
have landed; alloc tightening (§5) and editable text (§6) remain.

## Architecture

Per-frame data flow for text:

1. **Authoring** — widget records a `Shape::Text { text, color, font_size_px,
   wrap }`. Inputs only — no measurement, no shaping at record time.
2. **Measure** — `LayoutEngine::shape_text` runs against `&mut TextMeasurer`,
   shapes each `Shape::Text` once unbounded; if `wrap == Wrap` and the
   parent committed a narrower width than the natural unbroken line,
   reshapes at `max(available_w, intrinsic_min)`. The result goes on
   `LayoutResult.text_shapes` keyed by `NodeId`.
3. **Encode** — encoder reads `Shape::Text { color }` plus
   `LayoutResult.text_shape(id).key`, emits `RenderCmd::DrawText { rect,
   color, key }`. Shapes whose key is `INVALID` (mono fallback) drop here.
4. **Compose** — turns logical-px commands into physical-px `TextRun`s with
   transformed origin and pre-scaled bounds; scissor-grouped in lockstep
   with quads.
5. **Render** — `TextRenderer` (wgpu-side) looks up each run's shaped buffer
   in the same `CosmicMeasure` cache layout populated, builds glyphon
   `TextArea`s, calls `prepare` once per frame and `render` after all quads.

### The two text façades + shared shaper

Single `CosmicMeasure` instance is shared between two roles via
`Rc<RefCell<CosmicMeasure>>` (`SharedCosmic`):

- **`TextMeasurer`** (`src/text/mod.rs`) — Ui side. Holds
  `Option<SharedCosmic>`. `measure(text, size, max_w)` dispatches: cosmic
  if present, [`mono_measure`] (deterministic 0.5×size/glyph fallback) if
  not. Used by `LayoutEngine::shape_text`.
- **`TextRenderer`** (`src/renderer/backend/text.rs`) — wgpu side. Holds
  `Option<SharedCosmic>` + glyphon device-bound state (`Cache`, `TextAtlas`,
  `Viewport`, `glyphon::TextRenderer`, `SwashCache`). `prepare()` borrow-
  mutably reads the cosmic cache to build `TextArea`s.

Construction:

```rust
let cosmic = palantir::text::share(CosmicMeasure::with_bundled_fonts());
ui.set_cosmic(cosmic.clone());
backend.set_cosmic(cosmic);
```

The `RefCell` is single-threaded insurance: layout and render are sequential
in the frame loop, so `borrow_mut()` never re-enters in practice.

### `Shape::Text` (authoring inputs)

```rust
Text {
    text: String,
    color: Color,
    font_size_px: f32,
    wrap: TextWrap,        // Single | Wrap
}
```

No `measured`, `key`, `offset`, or `max_width_px` — those were derived state
that pre-refactor leaked into the recorded shape and went stale on resize.

### `TextWrap`

- `Single` — shape once, never reshape. Default for labels and headings.
- `Wrap` — allow reshape during measure when the parent commits a width
  narrower than the natural unbroken line. The widest unbreakable run
  (longest word, computed during the unbounded shape via
  `LayoutRun.glyphs`) is the floor — text overflows the slot rather than
  breaking inside a word.

### Bundled fonts

`assets/fonts/` ships **Inter** Regular/Bold (~130 KB) for proportional UI
body and **JetBrains Mono** Regular/Bold (~530 KB) for monospace. Both
OFL 1.1; license texts shipped alongside.
`CosmicMeasure::with_bundled_fonts()` constructs a `FontSystem` from the
embedded TTFs (no system font scan — fast, deterministic). Default `Attrs`
requests `Family::Name("Inter")`. `CosmicMeasure::new()` remains for the
system-fonts opt-in.

## What's done

| § | Item | Status |
|---|---|---|
| 1 | HiDPI / scale-factor: `scale_factor` flows `ComposeParams` → `RenderBuffer.scale` → `TextRenderer::prepare` → `TextArea.scale`. Glyphs rasterize at the right size on retina. | done |
| 2 | Disabled cascade dims fill/stroke/text via `Color::dim_rgb` + `ButtonTheme.disabled_dim`. Pinned by `disabled_ancestor_dims_descendant_fill`. | done |
| 3 | Bundled fonts (Inter + JetBrains Mono). Examples use `with_bundled_fonts()`. | done |
| 4 | Wrapping (Option A) — `TextWrap::Wrap`, intrinsic_min from cosmic glyphs, single-pass reshape during measure. Pinned by `wrapping_text_grows_height_in_narrow_frame` + `wrapping_text_overflows_intrinsic_min_without_breaking_words`. Showcase: `examples/showcase/text.rs`. | done |
| — | Architecture: `Shape::Text` is inputs-only; layout owns shaping; `TextMeasurer` / `TextRenderer` façades hide cosmic; `SharedCosmic` shares the cache. | done |

## What's left

### 5. Allocation tightening — **partial**

Glyphon retains its instance buffer. `RenderBuffer.texts` and
`TextRenderer.scratch` already follow `clear()` + `reserve()`. Remaining:

- **`CacheEntry` GC.** `CosmicMeasure.cache` grows monotonically — a button
  that flips between three labels accumulates three entries forever, plus
  every reshape on resize. Add `last_frame: u64` to `CacheEntry`, bump on
  hit, walk on `end_frame` and drop entries older than ~120 frames (~2s at
  60 fps). `HashMap::retain` is alloc-free. Needs a frame counter on `Ui`
  (`begin_frame` increments, threaded through `TextMeasurer::measure`).
- **`Shape::Text.text: String` per-frame clone.** Each `Button::show` and
  `Text::show` clones the label into the `Shape`. For static labels this is
  one heap alloc per text node per frame.
  1. Change to `text: Cow<'static, str>`. `&'static str` labels cost zero
     allocs.
  2. For dynamic strings, intern via `CosmicMeasure` → `Arc<str>` keyed on
     `text_hash`. Cosmic looks shapes up by key — it never needs the string
     after the first shape — so the `Shape`'s string is purely for
     diagnostics; could become a debug-only field.
- **Profile gate.** Don't ship either of the above without a flamegraph
  showing the alloc on a hot path. Premature on a 100-button frame at 60 fps.

### 6. Editable text — **deferred**

Blocked on the persistent `Id → Any` state map (CLAUDE.md §Status). When
that lands:

- `TextEdit` widget; one `cosmic_text::Editor` per `WidgetId` in state.
- Glyph-level hit-test: extend `HitEntry` with `Option<TextCacheKey>`;
  cursor via `Buffer::hit(x, y)`.
- IME: thread `winit` IME events through `InputState` → editor.
- Selection rendering: emit per-selection `RoundedRect` shapes as siblings
  of the `Shape::Text`.

## Open questions

- **Color space.** Glyphon outputs sRGB; the wgpu surface format is
  whatever winit picked. If text looks faded on a linear surface,
  premultiply on the way in or pick an sRGB view.
- **Atlas eviction under long sessions.** `atlas.trim()` runs each frame;
  glyphon evicts on shelf overflow. Verify under multi-font / multi-size
  load.
- ~~**Per-group text z-order.**~~ Resolved. The wgpu backend now keeps
  a pool of `glyphon::TextRenderer`s sharing one `TextAtlas` (one
  renderer per `DrawGroup` with text). `submit` interleaves
  `prepare_group` + `render_group` calls per group, so a child quad
  declared after a label correctly occludes it. Atlas glyph cache is
  shared across the pool — no extra glyph rasterization. See
  `src/renderer/backend/text.rs` and the `text z-order` showcase tab.
  Pinned by `render_schedule_interleaves_text_per_group` in
  `backend/tests.rs`.
