# Rounded-corner clipping

Containers can clip their content to rounded corners, applied uniformly
to quads and text (glyphon). Apps that never use `Surface::rounded(...)`
pay zero — no stencil texture, no stencil-variant pipelines.

## User-facing API

```rust
Panel::vstack()
    .background(Surface::rounded(Background {
        fill: BG,
        stroke: Some(Stroke { width: 1.0, color: BORDER }),
        radius: Corners::all(8.0),
    }))
    .show(ui, |ui| { ... });
```

`Surface { paint: Background, clip: ClipMode }` is the chrome primitive.
Sugar constructors:
- `Surface::scissor()` — clip-only, no paint. Used internally by `Scroll`.
- `Surface::clipped(bg)` — paint + scissor. Cheap "card with overflow hidden".
- `Surface::rounded(bg)` — paint + stencil-rounded clip. Lights up the stencil path.
- `From<Background> for Surface` — paint-only, no clip. Existing
  `.background(Background { … })` calls keep working.

`ClipMode { None, Rect, Rounded }` is exposed so callers can pick clip
behavior independent of the paint (e.g. `Surface { paint, clip:
ClipMode::Rect }` for "rounded paint, fast scissor" — accepts the visual
mismatch in exchange for skipping the stencil pass).

## Storage

- `PaintAttrs.clip: ClipMode` — 2 bits packed in the per-node attrs
  byte. Hot-path reads (cascade, encoder, hit-test) hit this directly.
- `ElementExtras.chrome: Option<Background>` — sparse side-table entry,
  populated only on nodes with chrome. Single source of truth for:
  - Painted background (encoder emits `DrawRect` from it).
  - Rounded-clip mask radius (from `chrome.radius`).
  - Rounded-clip mask inset (from `chrome.stroke.width`).

`Element` does NOT carry chrome. Chrome is a per-node-call concern,
threaded through `ui.node(element, surface, body)`: the `Ui` reads
`surface.clip` (with `Rounded` → `Rect` downgrade for zero-radius
paint), writes the clip mode bit onto `element`, and passes
`surface.paint` to `Tree::open_node` as the chrome param. The tree
stamps it into `extras.chrome` before the side-table allocation
check.

`Background` lives in `crate::primitives::background` (pure data).
`Surface` lives in `crate::widgets::theme` (pure data too — no
methods that mutate `Element`).

## Encode flow (per node, in `encoder/mod.rs::encode_node`)

1. **Chrome** — if `extras.chrome` is `Some` and not `is_noop()`, emit a
   `DrawRect` with the chrome's `radius` / `fill` / `stroke`. Chrome
   paints **before** the clip is pushed: the clip rect is deflated by
   `stroke.width`, so chrome's own stroke pixels would be clipped if it
   painted under the mask. The chrome's SDF in `quad.wgsl` self-clips
   correctly without needing a stencil mask.
2. **Push clip** — for `Rect` or `Rounded`:
   - `mask_rect = layout_rect.deflated_by(stroke.width)` so children
     clip just inside the painted stroke.
   - For `Rounded`: `mask_radius.tl = (chrome.radius.tl - stroke.width).max(0.0)`
     (per corner). The reduction keeps the mask's curve **concentric**
     with the painted stroke's inner edge — both have center at
     `(rect.min + paint.radius)`. Inflating instead would offset the
     curve center inward and produce a visible notch.
   - Emit `PushClip { rect }` or `PushClipRounded { rect, radius }`.
3. **Background-phase shapes** — iterate `tree.shapes.slice_of(id)`.
   `Shape::Text` emits `DrawText`. `Shape::RoundedRect` (if any custom
   widget pushes one via `ui.add_shape`) emits `DrawRect`. `Shape::Overlay`
   is deferred. `Shape::Line` is unsupported and trace-dropped.
4. Push transform if any (skipped on identity).
5. Recurse children.
6. Pop transform.
7. **Overlay-phase shapes** — iterate `tree.shapes` again, emit
   `Shape::Overlay` as `DrawRect`. Used by Scroll for scrollbar
   tracks/thumbs that paint above content but inside the clip.
8. Pop clip if any.

## Backend stencil path (in `renderer/backend/`)

`FrameOutput::has_rounded_clip()` is set by the composer when it sees
any `PushClipRounded`. The backend branches on it.

**Plain path** (`has_rounded_clip == false`): the existing color-only
render pass. No stencil texture allocated. No stencil-variant pipelines
built. Bit-for-bit identical to pre-feature.

**Stencil path** (`has_rounded_clip == true`):
- `Backbuffer.stencil: Option<StencilAttachment>` — lazy `Stencil8`
  texture, allocated on first rounded frame, kept warm thereafter.
- `QuadPipeline::ensure_stencil(device)` — lazy-builds two pipelines:
  - `mask_write` — color writes off, `fs_mask` fragment discards
    outside the SDF (so the rasterizer's bounding box doesn't get
    stamped); stencil op = `Replace(N)`.
  - `stencil_test` — color writes on, stencil compare = `Equal` against
    a dynamic reference set by `pass.set_stencil_reference`.
- `TextRenderer.stencil_renderers: Vec<GlyphonRenderer>` — second pool
  of glyphon renderers built with `depth_stencil = Some(test_state)`,
  shares the same `TextAtlas` as the no-stencil pool (glyphon caches
  pipelines by `(format, multisample, depth_stencil)`, so the atlas
  carries both pipeline variants without forking). Selected via
  `text::StencilMode::Stencil`.
- Render pass opens with color attachment + stencil attachment
  (`LoadOp::Clear(0)`, `StoreOp::Discard` — stencil never survives across
  frames; the cmd-buffer replays mask writes every frame).
- Per-group, the backend follows a write→draw→clear cycle for rounded
  clips:
  1. If `g.rounded_clip.is_some()`: bind `mask_write`, `set_stencil_reference(1)`,
     draw the mask quad (one instance per rounded clip in the frame).
  2. Bind `stencil_test`, draw `g.quads`.
  3. Render text via stencil-aware glyphon, same `stencil_reference`.
  4. If rounded: bind `mask_write` again with `set_stencil_reference(0)`,
     draw the mask quad — `Replace(0)` clears the stencil region back to 0
     so the next group sees clean stencil regardless of clip ordering.

The per-group write/clear pattern handles ordered siblings (rounded
clip A then non-rounded clip B at the same physical region) without
nesting support.

**Composer inheritance**: when a `Rect` clip is pushed inside a
`Rounded` ancestor, the composer's clip stack inherits the ancestor's
`rounded_clip` so the inner group's draws still pass `stencil_test` at
`ref=1`. Without this, the inner group would draw at `ref=0` over
stencil-1 pixels and disappear.

## Surface APIs

`Surface::scissor()` is the only construction without a paint
(Background is `Default::default` — fully transparent; encoder skips
emitting it via `is_noop`). Used by Scroll for its viewport clip.

`Surface::rounded(bg)` with `bg.radius.approx_zero()` downgrades to
`ClipMode::Rect` inside `ui.node` (encoder never sees a rounded clip
without a radius). The `const fn` constructor itself can't downgrade
(no const equality on f32) — runtime path catches it.

## Tests

- `composer/tests.rs::push_clip_rounded_lands_radius_on_group`,
  `push_clip_rect_emits_no_rounded_data` — composer plumbing.
- `encoder/tests.rs::clip_rounded_emits_push_clip_rounded_when_background_has_radius`,
  `clip_rounded_falls_back_to_scissor_without_background` — encoder
  invariant.
- `encoder/tests.rs::manually_pushed_rounded_rect_shape_emits_draw_rect`,
  `text_shape_emits_draw_text` — pin every shape arm of the
  background-phase iteration.
- `tests/visual/fixtures/widgets.rs::surface_rounded_clips_full_fill_child` —
  golden image with per-corner radii, 1px green stroke, full-fill black
  child clipped inside the inset rounded mask.
- Showcase tab `rounded clip` — side-by-side comparison of no-clip /
  scissor / rounded.

Future extensions (nested clips, partial-damage fixture) tracked in
`docs/roadmap/renderer.md` and `docs/roadmap/damage.md`.
