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

- [ ] **The Clip/Ellipsis path is a second, partial layout engine layered over Cosmic Text.** `measure_truncated` restores an unbounded layout, walks its glyphs to derive a logical byte prefix, trims and rebuilds source text in shared scratch, separately shapes and caches an ellipsis advance, then shapes and caches another buffer. This adds dedicated cache and scratch state (`aperture/src/text/cosmic.rs:140`, `aperture/src/text/cosmic.rs:147`, `aperture/src/text/cosmic.rs:154`) plus roughly 130 lines of alternate layout policy that Cosmic Text has no equivalent for, so the semantics are aperture's alone to keep correct (`aperture/src/text/cosmic.rs:403`). The memoized ellipsis advance is measured standalone (`aperture/src/text/cosmic.rs:519`) and consumed as the width reservation (`aperture/src/text/cosmic.rs:431`), so it can differ from the marker's real advance once the prefix and `…` are shaped together in one context-sensitive run.
