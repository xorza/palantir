# Text Rendering Plan

Replace the `chars * 8.0` placeholder in `Shape::Text` with real shaping, layout, and GPU rendering. Goal: correct measurement-driven `Hug` sizing, crisp rasterized glyphs through the wgpu pipeline, and **zero per-frame heap allocations in steady state** (after warmup, all frame buffers reused via `clear()` + `reserve()`, matching the existing pattern in `Tree`, `Cascades`, `RenderBuffer`, `HitIndex`).

## Crate choice: glyphon (cosmic-text + etagere)

Surveyed: `glyphon`, `cosmic-text` (raw), `parley` (linebender), `wgpu_glyph` / `wgpu_text` (glyph_brush wrappers).

**Pick glyphon 0.9.x** (`grovesNL/glyphon`).

Why:
- Already pinned in `Cargo.toml`. Drop-in wgpu renderer — atlas (`etagere` shelf packing), upload, pipeline, instanced glyph quads all done.
- Built on `cosmic-text`: gives multi-line, BiDi, font fallback, system font discovery, and an editing model the day we want a text input widget. Same crate covers v1 button labels and v3 editable text.
- Active maintenance, simple API surface (`FontSystem`, `SwashCache`, `Cache`, `Viewport`, `TextAtlas`, `TextRenderer`, `Buffer`, `TextArea`).
- Glyph atlas is `R8Unorm` (grayscale) + `Rgba8Unorm` (color emoji), shelf-packed, evicts on overflow.

Why not parley: best-in-class shaping (HarfRust, fontique, skrifa, icu4x) but **no wgpu renderer** — we'd write our own atlas + rasterization layer. Bigger scope for a marginal shaping-quality gain at GUI sizes. Revisit if cosmic-text's shaper proves limiting (complex scripts, advanced OpenType features). The boundary is `Shape::Text` → shaped glyphs, so the shaper is swappable later without disturbing the renderer.

Why not raw cosmic-text: would force us to build the wgpu atlas/renderer ourselves — that's exactly what glyphon already is.

Why not wgpu_glyph / wgpu_text: glyph_brush + ab_glyph; no shaping, no fallback, no BiDi. Dead end for editable text.

## Architecture overview

Five touchpoints, in frame order:

1. **Authoring (widgets)** — call into a shared `TextSystem` to get a shaped `Buffer` for a `(WidgetId, text, font, size, max_width)` key. Write real `measured` into `Shape::Text`.
2. **Measure** — unchanged; already reads `Shape::Text.measured` in `leaf_content_size` (`src/layout/mod.rs:181`).
3. **Encode** — add `RenderCmd::DrawText { run: TextRunId, rect, color }`; remove the trace-and-drop branch at `src/renderer/encoder/mod.rs:87`.
4. **Compose** — add a parallel `text_runs: Vec<TextRunDraw>` to `RenderBuffer`, scissor-grouped in lockstep with quads.
5. **Backend** — add `TextPipeline` wrapping glyphon's `TextAtlas` + `TextRenderer` + `Viewport`. Submit alongside `QuadPipeline`.

Glyph hit-testing, cursors, selection, and IME are explicitly **out of scope** for v1. Hooks reserved (see §Phases).

## Data structures

### New

```rust
// src/text/mod.rs — owned by Ui, threaded into widgets via &mut Ui
pub struct TextSystem {
    pub(crate) font_system: cosmic_text::FontSystem,
    pub(crate) swash_cache: cosmic_text::SwashCache,
    pub(crate) cache: glyphon::Cache,           // wgpu pipeline cache (one per device)
    pub(crate) atlas: glyphon::TextAtlas,        // grayscale + color shelves
    pub(crate) viewport: glyphon::Viewport,      // viewport uniform
    pub(crate) renderer: glyphon::TextRenderer,  // owns instance buffer

    // Per-WidgetId shaped-buffer cache. Survives across frames; entries
    // touched-this-frame are kept, untouched are GC'd at end_frame.
    buffers: HashMap<TextKey, BufferEntry>,
    touched: HashSet<WidgetId>,                  // cleared each frame, reserve-pattern

    // Per-frame run table — one TextArea-equivalent per visible Shape::Text.
    // Cleared each frame; capacity retained.
    runs: Vec<TextRun>,                          // index = TextRunId
}

#[derive(Hash, Eq, PartialEq)]
struct TextKey {
    id: WidgetId,
    // Inputs that invalidate shaping. Hash strings via a stable hash of
    // &str at insert time — store the String only on first insert, reuse
    // on subsequent frames.
    text_hash: u64,
    font_id: FontId,
    size_q: u32,        // px * 64, fixed-point so HashMap key is integral
    max_w_q: u32,       // px * 64; u32::MAX == unbounded
}

struct BufferEntry {
    buffer: cosmic_text::Buffer, // owns the shaped lines
    last_frame: u64,             // for GC of stale entries
}

pub struct TextRun {
    pub buffer_key: TextKey,     // -> buffers[key].buffer
    pub origin: Vec2,            // physical-px top-left after transform/snap
    pub clip: Option<Rect>,      // physical-px scissor (already from cascade)
    pub color: glyphon::Color,
}

pub type TextRunId = u32;
pub type FontId = u32;           // index into TextSystem font registry
```

### Changed

```rust
// src/shape/mod.rs — add a font/size pair so authoring can shape correctly.
// `measured` stays — measure pass is unchanged.
Shape::Text {
    offset: Vec2,
    text: String,           // keep for now; v2: intern via TextSystem
    color: Color,
    measured: Size,
    font_id: FontId,
    size_px: f32,
    max_w_px: Option<f32>,  // None == unbounded; Some triggers wrapping
    run: Option<TextRunId>, // filled by encoder, not authoring
}

// src/renderer/encoder/mod.rs
pub enum RenderCmd {
    PushClip(Rect),
    PopClip,
    PushTransform(TranslateScale),
    PopTransform,
    DrawRect { /* unchanged */ },
    DrawText { run: TextRunId, rect: Rect, color: Color },
}

// src/renderer/buffer.rs
pub struct RenderBuffer {
    pub quads: Vec<Quad>,
    pub text_runs: Vec<TextRunDraw>,        // NEW; cleared each frame
    pub groups: Vec<DrawGroup>,             // gets text_range alongside quad_range
    /* ... */
}

pub struct DrawGroup {
    pub scissor: ScissorRect,
    pub quads: Range<u32>,
    pub texts: Range<u32>,                  // NEW
}

pub struct TextRunDraw {
    pub run: TextRunId,
    pub origin_phys: Vec2,
    pub color: Color,
}

// src/ui/mod.rs
pub struct Ui {
    /* existing fields */
    pub(crate) text: TextSystem,            // NEW; lifetime = device lifetime
}
```

### Backend additions

```rust
// src/renderer/backend/text.rs
pub struct TextPipeline {
    // glyphon owns its pipeline + instance buffer internally — we just
    // hold the renderer. The atlas/viewport/cache live on TextSystem
    // because authoring needs FontSystem/SwashCache too, and atlas
    // sharing with the renderer is by &mut.
}
```

Glyphon expects you to call `renderer.prepare(device, queue, font_system, atlas, viewport, &[TextArea], swash_cache)` once per frame, then `renderer.render(atlas, viewport, &mut pass)` inside the render pass. Our `TextRun` table maps 1:1 to `TextArea`.

## Allocation strategy (zero-alloc steady state)

Established pattern in this codebase: every per-frame Vec lives on a long-lived owner, gets `clear()` + `reserve(n)` at frame start, never reallocated in steady state. We follow it exactly.

| Buffer | Owner | Frame-start op | Steady-state alloc |
|---|---|---|---|
| `TextSystem.runs` | `Ui` | `clear()` | none |
| `TextSystem.touched` | `Ui` | `clear()` | none |
| `RenderBuffer.text_runs` | `Composer` | `clear()` | none |
| `glyphon::TextRenderer` instance buffer | `TextSystem` | grows monotonically | none after warmup (glyphon retains capacity) |
| `cosmic_text::Buffer` per widget | `TextSystem.buffers` | reused if `TextKey` matches | **none** when text/size/font/width unchanged |
| `Shape::Text.text: String` | `Tree.shapes` | freed by `Tree.shapes.clear()` each frame | **one alloc per Text shape per frame** — see below |

The only remaining per-frame alloc is the `String` clone inside `Shape::Text` at authoring time (e.g. `src/widgets/button.rs:95`). Two-phase fix:

- **v1**: leave it. One `String` per text shape per frame is acceptable and matches today's behavior.
- **v2**: replace `Shape::Text.text: String` with `text: Cow<'static, str>` + a `TextSystem`-owned interner (FxHashMap from hash → Arc<str>). Then static labels (`"OK"`, `"Cancel"`) cost zero allocs after first frame and dynamic strings still work via owned `Cow`.

Shape inputs that gate buffer reshaping (`text_hash`, `font_id`, `size_q`, `max_w_q`) are all integral — `TextKey` is `Copy`, no allocation in the lookup. `text_hash` is computed once at authoring time from `&str` (FxHash, no alloc).

GC of stale `BufferEntry`s: at `end_frame`, walk `buffers` and drop any whose `last_frame` is older than N frames (e.g. 60). Bounded amortized cost; `HashMap::retain` is alloc-free.

## Phases

### Phase 1 — Plumbing & static rendering (no real shaping yet)

1. Add `src/text/mod.rs` with `TextSystem` skeleton: `FontSystem`, `SwashCache`, `Cache`, `Atlas`, `Viewport`, `TextRenderer`. Construct in `Ui::new(device, queue, format)` (yes, `Ui::new` grows wgpu params — needed because the atlas is device-bound).
2. Bundle a default font (Inter or DejaVu Sans) via `include_bytes!` and register in `FontSystem`. Single `FontId(0)` for v1. No font discovery yet.
3. Extend `Shape::Text` with `font_id`, `size_px`, `max_w_px`, `run`.
4. In `Button::show`, call `ui.text.shape(id, &label, font_id, size_px, None)` → returns `(measured, /* updates internal cache */)`. Write real `measured`.
5. Encoder: replace the drop branch with `RenderCmd::DrawText`. Allocate a `TextRunId` by pushing onto `TextSystem.runs`.
6. Composer: route `DrawText` into `RenderBuffer.text_runs`, transform/snap origin, intersect against current clip.
7. Backend: in `Renderer::render`, after building `TextArea`s from `RenderBuffer.text_runs`, call `text_renderer.prepare(...)` then per-group `text_renderer.render(...)` between scissor switches.
8. Smoke test: showcase example button labels render at correct size, Hug sizing actually hugs.

### Phase 2 — Cascade correctness & scissor

1. Verify transform cascade composes correctly with glyphon's coordinate space (glyphon takes physical px directly; we already snap in `Composer`).
2. Verify clip rect: glyphon supports per-`TextArea` bounds; map our `Cascade.clip` → `TextArea.bounds`. Confirm no glyph leaks past clipped scrollers.
3. Disabled cascade: dim text color, same as quads.
4. Pixel snapping at fractional scale factors — confirm subpixel offset cache in glyphon handles `scale_factor != 1.0`.

### Phase 3 — Allocation tightening

1. Profile a 100-button frame with `cargo flamegraph` or `dhat-rs`. Confirm zero-alloc in steady state.
2. Implement the `Cow<'static, str>` + interner upgrade for `Shape::Text.text` if profiling shows the per-frame `String` alloc on the hot path.
3. `BufferEntry` GC at `end_frame`. Tunable retention window.
4. Consider trimming the atlas on long sessions (glyphon does shelf eviction; verify it's adequate).

### Phase 4 — Editable text (deferred; gated on persistent state map)

Requires the `Id → Any` state map in CLAUDE.md's TODO. Then:
- `TextEdit` widget, `cosmic_text::Editor` per `WidgetId` in state.
- Glyph-level hit-test: extend `HitEntry` with optional `TextRunId`; cursor lookup via `Buffer::hit(x, y)`.
- IME: thread `winit` IME events through `InputState` → editor.
- Selection rendering: emit a sibling `RoundedRect` shape per selection rect.

## Open questions

- **Font discovery** — bundle one font (v1) or call `FontSystem::new()` to pick up system fonts? System fonts blow up startup time and add nondeterminism in tests; bundle for now, expose `Ui::register_font(bytes)` for the app to add more.
- **Color space** — glyphon outputs sRGB; surface format in `WgpuBackend` is whatever `WindowSurface` chose. Verify alignment; may need to surface-format-match or premultiply manually.
- **MSAA** — backend has none today. Glyph atlas is alpha-blended; no MSAA needed for text. Quads may want MSAA later, independent decision.
- **Long-text widgets** — for paragraphs of text, do we shape eagerly during `show()` or lazily during measure? Eager is simpler and fits the immediate-mode model; lazy would matter only if shaping cost dominates frame time. Start eager.

## Files to touch

| File | Change |
|---|---|
| `Cargo.toml` | bump `glyphon = "*"` (already pinned), add `cosmic-text = "*"` (re-exported by glyphon, but explicit is clearer) |
| `src/text/mod.rs` | NEW — `TextSystem`, `TextKey`, `TextRun`, `BufferEntry` |
| `src/shape/mod.rs` | extend `Shape::Text` |
| `src/widgets/button.rs` | call `ui.text.shape(...)`, drop hardcoded `8.0` |
| `src/ui/mod.rs` | own `TextSystem`; thread it into begin/end frame |
| `src/renderer/encoder/mod.rs` | add `DrawText`, remove drop branch |
| `src/renderer/composer/mod.rs` | route `DrawText` into `text_runs`, scissor groups |
| `src/renderer/buffer.rs` | add `text_runs`, extend `DrawGroup` |
| `src/renderer/backend/mod.rs` | hold `TextPipeline`, call prepare/render |
| `src/renderer/backend/text.rs` | NEW — thin wrapper over glyphon's renderer |
| `examples/showcase/` | text-only page demonstrating sizes, wrapping, clipping |

## Acceptance

- All existing layout tests pass unchanged (measure pass reads `measured` same as before).
- New tests pinning: button width with real font matches `measured.width` ± rounding; wrapped text with `max_w_px` produces expected `measured.height`; disabled text dims; clipped text doesn't leak.
- `cargo flamegraph` of a steady-state showcase frame: no `alloc::*` symbols on the hot path inside the text system after frame ~5.
- `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test` all green.
