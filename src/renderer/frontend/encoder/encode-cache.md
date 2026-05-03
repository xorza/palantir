# Encode cache

Subtree-skip on the encoder, mirroring the
[measure cache](../../../layout/measure-cache.md). Same key shape, same
arena pattern, but stores `RenderCmd` slices instead of `desired` sizes.

Code lives in `cache/` (this directory's sibling). The `Encoder` struct
owns the cache + cmd buffer and is the entry point from
`Frontend::build`.

## Mechanism

- **Key**: `(WidgetId, subtree_hash, available_q)` — same triple the
  `MeasureCache` uses.
- **Subtree-relative storage.** `data_arena` stores `rect.min` with the
  snapshot root's `origin` already subtracted. On replay the encoder
  translates back by the *current* frame's `layout.rect(id).min`, so a
  cached subtree survives parent origin shifts (scroll, resize,
  reflowed siblings) without invalidating. Net offset over an
  unchanged frame is zero — replay is byte-identical to a cold encode
  (pinned by `encode_cache_warm_frame_matches_cold_encode`).
- **Cascade is NOT in the key.** Inside a cached subtree the encoder
  reads `is_invisible` (descendant invisibility comes from authoring +
  in-subtree ancestors, captured in `subtree_hash`), `attrs.is_clip()`
  and `extras.transform` (authoring, in `subtree_hash`), and
  `screen_rect` only when `damage_filter.is_some()`. The cache is
  bypassed in the damage-filter branch, so `screen_rect` never
  influences a hit.

## Storage

- `EncodeSnapshot { subtree_hash, available_q, cmds: Span, data: Span }`,
  32 bytes.
- Three SoA arenas — `kinds_arena`, `starts_arena` (parallel,
  subtree-relative offsets), `data_arena` — plus an
  `FxHashMap<WidgetId, EncodeSnapshot>` index. Same `COMPACT_RATIO = 2`
  / `COMPACT_FLOOR = 64` as `MeasureCache`; eviction-locked via the
  shared `removed` sweep in `Ui::end_frame`.
- Hot path: same `subtree_hash` ⇒ identical cmd shape and payload
  sizes ⇒ in-place rewrite preserves snapshot positions. Size mismatch
  appends and marks the old range as garbage; tracked via `live_cmds`
  / `live_data` for the compaction trigger.

## Replay primitives

- `RenderCmdBuffer::extend_from_cached(kinds, starts, data, offset)`
  copies a cached subtree's slices into the live cmd buffer and shifts
  every rect-bearing payload's `rect.min` by `offset`.
- `bump_rect_min(kinds, starts, data, offset)` is the shared rect-shift
  helper, used by both `extend_from_cached` (replay) and
  `EncodeCache::write_subtree` (subtract origin at insertion time).
- `available_q` lives on `LayoutResult` (promoted from `LayoutScratch`
  to make it readable from the encoder without reaching into the
  engine).

## Tests

- `src/renderer/frontend/encoder/cache/tests.rs` — unit tests for the
  cache itself: round-trip at same/shifted origin, hash and
  `available_q` mismatch, in-place rewrite preserves positions, size
  change marks garbage, `sweep_removed` evicts and decrements live
  counters, compaction preserves lookups, `clear`.
- `src/renderer/frontend/encoder/tests.rs::encode_cache_warm_frame_matches_cold_encode`
  — integration test: warm-cache replay through `Frontend::build` is
  byte-identical to a fresh cold encode.

## Bench

`benches/encode_cache.rs`, A/B'd against an otherwise-identical
warm-cache frame with `__clear_encode_cache()` between iterations
(measure cache held hot in both arms, so the delta is purely
encoder work). Times are `end_frame()` end-to-end.

| Workload | cached | forced miss | win |
|---|---|---|---|
| `flat`   (~1000 leaves)            | 75.9 µs | 86.4 µs | 12.2 % |
| `nested` (100 × 32 nodes ≈ 3200)   | 376 µs  | 417 µs  | 9.8 %  |

The end-to-end percentage is diluted by the composer pass, which runs
in both arms; the encoder pass itself saves substantially more in
absolute terms.

`TINY_SUBTREE_THRESHOLD = 1` skips cache lookup + write for size-1
leaves: one `draw_rect`/`draw_text` is cheaper to re-emit than the
hashmap miss + insert that would replace it. K=2 was tried and
regressed `nested/forced_miss` by 7 %, so the threshold stays at 1.

Future-work items (composer cache, hit-hint propagation,
damage-aware encode replay, SIMD `bump_rect_min`, coarser
`available_q` quantization) live in `docs/todo.md`.
