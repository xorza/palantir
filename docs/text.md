# Text — status & remaining work

Tracks the move from the historical `chars * 8.0` placeholder to real shaped,
wgpu-rendered text. Supersedes the earlier `docs/text.md` and
`docs/text-wrapping.md` drafts: most of Phase 1–2 has landed, the wrapping
plan ("Option A") has not.

## What's done

- **Crate choice**: `glyphon` 0.9 (cosmic-text + etagere atlas), pinned in
  `Cargo.toml`. `cosmic-text` re-exported via glyphon for direct shaping.
- **Measurement layer** (`src/text/`): two concrete paths instead of a trait,
  by deliberate choice (the renderer needs concrete `FontSystem` access, a
  trait would just be a downcast):
  - `CosmicMeasure` — real shaping. Per-key shaped-`Buffer` cache, integral
    `TextCacheKey { text_hash, size_q, max_w_q }`, hash-only key (no string
    storage; collisions accepted). `split_for_render` hands out the disjoint
    `(&mut FontSystem, BufferLookup)` glyphon needs.
  - `mono_measure` — deterministic placeholder (`0.5 × font_size` per glyph,
    `font_size` line height). Used when no `CosmicMeasure` is installed; emits
    `TextCacheKey::INVALID` so the renderer drops the runs cleanly. Existing
    layout tests pin against this exact metric.
- **`Shape::Text` carries** `offset / text / color / measured / font_size_px /
  max_width_px / key`. `is_noop` filters empty/transparent runs.
- **Encoder**: emits `RenderCmd::DrawText { rect, color, key }`. Invalid keys
  are traced + dropped.
- **Composer**: applies transform + scale + pixel-snap to text origin, intersects
  with current clip → physical-px `TextRun { origin, bounds, color, key }` in
  `RenderBuffer.texts`, scissor-grouped in lockstep with quads.
- **Backend** (`renderer/backend/text.rs`): `TextPipeline` owns
  `Cache / TextAtlas / Viewport / TextRenderer / SwashCache`. Builds
  `TextArea`s by looking up shaped buffers in `CosmicMeasure`,
  `prepare`s once per frame, `render`s after all quads, `atlas.trim()` per
  frame. The `'static` → frame-borrow `transmute` on `scratch` is sound because
  the buffer is cleared before return.
- **`Ui` integration**: `install_text_system(CosmicMeasure)`, `measure_text(...)`,
  `text_mut()`. `examples/showcase` and `examples/helloworld` install one at
  startup; `Button::show` calls `measure_text` and writes a real `Shape::Text`.

## What's left

The remaining work splits into four areas: correctness bugs in the existing
pipeline (1–3), the wrapping plan that was scoped but never built (4),
allocation tightening (5), and editable text (6, deferred).

### 1. HiDPI / scale-factor correctness — **bug**

Today `TextPipeline::prepare` hard-codes `scale: 1.0` on every `TextArea`, and
shaping happens at logical px (`Metrics::new(font_size_px, font_size_px*1.2)`)
with no awareness of `Ui::scale_factor`. The composer pre-scales the origin to
physical px, but glyphs are rasterized at *logical* size and placed at
*physical* coordinates → text is half-size on a 2× retina display.

Two valid fixes; pick one and stick to it:

- **(A) Shape at physical px.** `measure_text` takes `scale_factor`, multiplies
  size + max_width before shaping, returns a logical `Size` (divide back). Cache
  keys then quantize physical px. Pro: glyphon's existing subpixel-position
  cache works. Con: cache invalidates on every scale_factor change (rare).
- **(B) Shape at logical px, render with `scale = scale_factor`.** Simpler,
  one-liner change in `TextPipeline::prepare`. Glyphon scales glyph output by
  the `TextArea.scale` field. Con: glyphon rasterizes at the scaled size
  internally, so atlas entries depend on scale anyway — no real cache win
  vs. (A).

**Recommendation: (B).** It's the change glyphon was designed for. Implementation:
thread `scale_factor` from `WgpuBackend::render` into `TextPipeline::prepare`,
set `TextArea.scale = scale_factor`. Adjust `bounds` if needed (`TextBounds` is
in physical px, already correct from the composer).

Test: `examples/helloworld` at `WindowEvent::ScaleFactorChanged` → text height
matches frame height.

### 2. Disabled / invisible cascade for text — **gap**

`Cascades` already resolves `disabled`/`invisible` per node, and the encoder
walks it for quads, but the text branch in `encoder/mod.rs:95` ignores it:
disabled buttons render their label at full color. Two pieces:

- **Skip on invisible.** The encoder's existing pre-walk already prunes invisible
  subtrees before reaching `Shape::Text`; verify with a test (no run emitted
  for an invisible button).
- **Dim on disabled.** Multiply `color` by the theme's disabled alpha
  (currently quads do this implicitly via `ButtonStyle.disabled.text`; the
  label color comes from `ButtonStyle` already, so this *should* work — but
  add a test to pin it because it's load-bearing for accessibility).

### 3. Font registry / bundled font — **gap**

`CosmicMeasure::new()` calls `FontSystem::new()` which scans system fonts.
Side effects:
- Startup time (50–500ms cold, varies wildly by OS).
- Nondeterminism in tests (different machines → different fallback chains →
  different metrics → flaky pin tests). Today only `mono_measure` is used in
  tests, but anyone wanting a shaped-text test will hit this.

Plan:
- `CosmicMeasure::with_bundled_font(bytes: &[u8])` constructor that uses
  `FontSystem::new_with_locale_and_db` on an empty DB and registers the bundle.
- Bundle Inter or DejaVu Sans via `include_bytes!` in a small `assets/`
  directory. Inter is ~310 KB subsetted (Latin + symbols); DejaVu is ~750 KB
  but has broader coverage. Pick Inter.
- Keep `CosmicMeasure::new()` as the system-font path for apps that want
  native-feeling text.
- Future `register_font(bytes)` proxies to `FontSystem::db_mut().load_font_data`.

Skipped from the original plan: a numeric `FontId`. Cosmic-text already keys
attrs on family name; until a widget actually needs to switch fonts mid-tree
this is YAGNI. Add when needed.

### 4. Wrapping & intrinsic sizing — **not started**

This is the biggest remaining piece. The earlier `text-wrapping.md` proposed
two options, A then B. Reaffirming that plan:

#### Option A — eager shape at unbounded width, reshape on constraint commit (**implement now**)

The `TextKey.max_w_q` field is already in place; this is purely about wiring.

**Authoring (widget side):**
- A widget that wants wrapping calls `ui.measure_text(text, size, None)` first
  → returns `(measured_max, key_unbounded)`. This is the max-content size and
  drives `Hug`.
- Compute `intrinsic_min` (width of the widest unbreakable run) by re-using the
  same shape pass — cosmic-text's `Buffer::layout_runs` exposes per-glyph
  cluster widths; iterate, track the max width between cluster breaks at
  whitespace. Cache it in the `CacheEntry` so each `(text, size)` pair
  computes it once. Avoid a second shape — read from the unbounded buffer.

**Shape representation:**
- New `Shape::Text` field `wrap: TextWrap`:
  ```rust
  pub enum TextWrap {
      Single,                       // current behavior
      Wrap { intrinsic_min: f32 },  // reshape-on-arrange enabled
  }
  ```
- `measured` keeps the unbounded-width result (max-content). Layout's
  `leaf_content_size` (`src/layout/mod.rs:181`) is unchanged.

**Arrange-time reshape hook:**
- New `LayoutEngine::reshape_text(node, width)` called from `arrange` *after*
  the parent commits a final width to a `Wrap` text node, only if
  `width != measured.width`:
  - `width >= measured.width`: no-op.
  - `width < intrinsic_min`: reshape with `max_w = intrinsic_min`, accept
    overflow (don't break unbreakable runs).
  - else: reshape at `width`, update arranged height, update the Shape's
    `measured` and `key` in place (the new key replaces the old one in the
    cache; the old entry is GC'd by frame retention — see §5).
- This is one localized addition to arrange, not a new pass. The two-pass
  measure/arrange model stands.

**Cost:** 0 reshapes in steady state, ≤1 per visible wrapping node on resize
frames. Cosmic-text shapes hundreds/frame at 60 fps comfortably.

**What A doesn't fix** (and we accept):
- `Grid Auto` column wrapping a paragraph: column width is committed during
  measure, before arrange's reshape runs → wrong column width.
- `Fill` distribution that wants min-content (intrinsic_min) as the floor:
  we use max-content, slightly over-allocates.

These are both rare in v1/v2. Trigger to revisit B is the first concrete
widget that hits one.

**Acceptance:**
- `VStack { Frame::fixed_w(200) { Text("long paragraph...") } }`: text wraps to
  200 px, height grows by line count.
- `VStack { Frame::fixed_w(20) { Text("supercalifragilistic") } }`: overflows
  at `intrinsic_min`, doesn't break the word.
- `cargo test`: existing button-label tests unchanged.
- Profiled showcase frame: 0 reshapes after warmup.

#### Option B — intrinsic-dimensions protocol (**defer**)

When a real victim of A's gaps appears (Grid/paragraph; flex/min-content),
promote intrinsic sizing to a first-class layout stage:

1. Bottom-up `intrinsic(node, axis) -> (min, max)` pre-pass.
2. Top-down resolve: parents pick widths for `Fill`/`Auto` children using the
   ranges as bounds.
3. Existing measure-with-final-width + arrange.

`Shape::Text.wrap` and `TextKey.max_w_q` carry forward unchanged — A is a
strict subset of B's behavior in the cases A handles.

#### Need a `Text` widget

There is no `Text` widget today, only `Button`'s label. Add
`src/widgets/text.rs`:

```rust
pub struct Text {
    element: Element,
    text: Cow<'static, str>,
    size_px: f32,
    color: Color,
    wrap: TextWrap,
}

impl Text {
    pub fn new(text: impl Into<Cow<'static, str>>) -> Self { ... }
    pub fn size(self, px: f32) -> Self { ... }
    pub fn color(self, c: Color) -> Self { ... }
    pub fn wrapping(self) -> Self { ... }       // toggles to Wrap
    pub fn show(self, ui: &mut Ui) -> Response { ... }
}
```

`show()` calls `ui.measure_text` (with `None` for max_w in the wrap case),
pushes a leaf `Element` with `Sizing::Hug × Hug`, attaches one `Shape::Text`.
Widget-level layout falls out of the leaf path automatically.

### 5. Allocation tightening — **partial**

Glyphon retains its instance buffer. `RenderBuffer.texts` and the
`TextPipeline.scratch` `Vec` already follow `clear()` + `reserve()`. Remaining:

- **`CacheEntry` GC.** `CosmicMeasure.cache` grows monotonically — a button
  that flips between three labels accumulates three entries forever, plus
  every re-shape on resize. Add `last_frame: u64` to `CacheEntry`, bump on
  hit, walk on `end_frame` and drop entries older than ~120 frames (~2s at
  60 fps). `HashMap::retain` is alloc-free.
  - Need to thread a frame counter into `Ui`. Today there isn't one;
    add `Ui.frame: u64`, increment in `begin_frame`, expose via
    `measure_text` → cache.
- **`Shape::Text.text: String` per-frame clone.** Each `Button::show` clones
  the label into the `Shape`. For static labels this is one heap alloc per
  button per frame (`Tree::clear` drops them, `set_text` stays the same).
  Two-step fix:
  1. Change to `text: Cow<'static, str>`. `&'static str` widgets (the common
     case) cost zero allocs.
  2. For dynamic strings, intern via `CosmicMeasure` → `Arc<str>` keyed on
     `text_hash`. Renderer no longer needs the string at all (cosmic buffers
     are looked up by `key`), so the `Shape` could in principle hold *only*
     the `key` plus an `Arc<str>` for debug printing — keep the `Cow` for
     diagnosability, drop it when profiling says so.
- **Profile gate.** Don't ship either of the above without a flamegraph
  showing the alloc on the hot path. Premature on a 100-button frame at
  60 fps.

### 6. Editable text — **deferred**

Blocked on the persistent `Id → Any` state map (CLAUDE.md §Status). When that
lands:
- `TextEdit` widget; one `cosmic_text::Editor` per `WidgetId` in state.
- Glyph-level hit-test: extend `HitEntry` with `Option<TextCacheKey>`; cursor
  via `Buffer::hit(x, y)`.
- IME: thread `winit` IME events through `InputState` → editor.
- Selection rendering: emit per-selection `RoundedRect` shapes as siblings of
  the `Shape::Text`.

## Open questions

- **Color space.** Glyphon outputs sRGB; the wgpu surface format is
  whatever winit picked. Verify alignment after the HiDPI fix — if text looks
  faded on a linear surface, premultiply on the way in or pick an sRGB view.
- **Atlas eviction under long sessions.** `atlas.trim()` runs each frame;
  glyphon evicts on shelf overflow. Keep an eye on memory once we have
  multi-font / multi-size content. No action until profiled.
- **Per-group text z-order.** Today text is prepared once and rendered after
  all quads → labels float above sibling backgrounds. Fix when the first
  widget needs interleaving (likely a panel-with-header where the header
  sits over a scrolled child). Options: per-group prepare/render, or
  glyphon's depth metadata.
