# Text module simplification review

The review covers every production file under `aperture/src/text` and traces the
layout, record-store, `TextEdit`, and renderer consumers. Tests, benches,
fixtures, and gated test-support code were excluded from the critique except
where they prove that a production branch exists only to support them.

The module currently carries several overlapping interpretations of text:
Cosmic Text shapes glyphs, `probe.rs` reconstructs caret and selection
semantics, and `cosmic.rs` reconstructs truncation and line-breaking
semantics. The ownership path is similarly layered:

```text
RecordedText
  -> TextShapeInput
  -> TextSystem reuse map
  -> TextShaper / ShaperInner
  -> CosmicMeasure buffer cache
  -> TextRenderSession
  -> TextBackend
```

`TextEdit` bypasses `TextSystem` and enters through `TextShaper::layout`, while
`TextSystem` bypasses the `TextShaper` surface and calls `ShaperInner::dispatch`
directly. This makes the apparent layers poor guides to where policy and state
actually live.

## High: parallel shaping policies duplicate incomplete text semantics

- [ ] **The Clip/Ellipsis path is a second, partial layout engine layered over Cosmic Text.** `measure_truncated` restores an unbounded layout, walks visual glyphs to derive a logical byte prefix, trims and rebuilds source text in shared scratch, separately shapes and caches an ellipsis advance, then shapes and caches another buffer. This adds dedicated cache and scratch state plus roughly 130 lines of alternate layout policy; in BiDi runs, visual glyph iteration does not guarantee monotonically increasing `g.end` values, so the selected `request.text[..cut]` can be the wrong logical prefix, and the standalone ellipsis advance can differ after context-sensitive prefix-plus-ellipsis shaping (`aperture/src/text/cosmic.rs:137`, `aperture/src/text/cosmic.rs:141`, `aperture/src/text/cosmic.rs:148`, `aperture/src/text/cosmic.rs:391`, `aperture/src/text/cosmic.rs:407`, `aperture/src/text/cosmic.rs:417`, `aperture/src/text/cosmic.rs:436`, `aperture/src/text/cosmic.rs:442`, `aperture/src/text/cosmic.rs:445`, `aperture/src/text/cosmic.rs:458`, `aperture/src/text/cosmic.rs:494`).

## Medium: cache and render boundaries duplicate ownership without containing it

- [ ] **`TextSystem` duplicates measurement ownership while bypassing the `TextShaper` abstraction it ostensibly coordinates.** `CosmicMeasure` already stores the complete measurement beside each content-keyed buffer, while `TextSystem` adds a per-window map containing another unbounded measurement, another bounded measurement, repeated keys, a hot bit, a sweep threshold, and removed-widget maintenance. Its miss paths reach through `TextShaper.inner` and invoke `ShaperInner::dispatch` directly, so the extra cache tier also exposes the shaper's borrow and dispatch internals instead of containing them; this leaves two cache policies and duplicate measurement residency to coordinate at frame finalization (`aperture/src/text/cosmic.rs:98`, `aperture/src/text/cosmic.rs:127`, `aperture/src/text/cosmic.rs:547`, `aperture/src/text/mod.rs:126`, `aperture/src/text/system.rs:18`, `aperture/src/text/system.rs:46`, `aperture/src/text/system.rs:83`, `aperture/src/text/system.rs:85`, `aperture/src/text/system.rs:127`, `aperture/src/text/system.rs:141`, `aperture/src/text/system.rs:149`).

- [ ] **The render-session module adds forwarding declarations without establishing a dependency boundary.** `cosmic.rs` imports the renderer-facing placement and bitmap types and implements glyph extraction and rasterization, while `render.rs` imports `CosmicMeasure` and `GlyphRasterKey`, stores `RefMut<CosmicMeasure>`, and forwards both operations one-for-one. The backend still imports `text::cosmic` directly for raster keys and subpixel construction, so changes to the supposedly hidden Cosmic layer propagate through `cosmic.rs`, `render.rs`, the encoder, and the atlas despite the facade's duplicate surface (`aperture/src/text/cosmic.rs:29`, `aperture/src/text/cosmic.rs:195`, `aperture/src/text/cosmic.rs:252`, `aperture/src/text/render.rs:10`, `aperture/src/text/render.rs:20`, `aperture/src/text/render.rs:31`, `aperture/src/text/render.rs:41`, `aperture/src/renderer/backend/text/encode.rs:29`, `aperture/src/renderer/backend/text/atlas.rs:3`).
