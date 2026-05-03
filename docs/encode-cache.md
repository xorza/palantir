# Encode cache (Phase 3 of the cross-frame cache series)

Subtree-skip on the encoder, mirroring `MeasureCache`. Same key
shape, same arena pattern, but stores `RenderCmd` slices instead
of `desired` sizes. See `docs/measure-cache.md` for the parent
design.

**Status:** shipped. Lives at
`src/renderer/frontend/encoder/cache/`. Wired into `Encoder::encode`
(`src/renderer/frontend/encoder/mod.rs`); the `Encoder` struct owns
the cache + cmd buffer and is the entry point from `Frontend::build`.

## Bench numbers

`benches/encode_cache.rs`, A/B'd against an otherwise-identical
warm-cache frame with `__clear_encode_cache()` between iterations
(measure cache held hot in both arms, so the delta is purely
encoder work). Times are `end_frame()` end-to-end.

| Workload | cached | forced miss | win |
|---|---|---|---|
| `flat`   (~1000 leaves)            | 74.5 µs | 96.7 µs | 22.9 % |
| `nested` (100 × 32 nodes ≈ 3200)   | 372 µs  | 449 µs  | 17.0 % |

Nested falls slightly under the original ≥20 % goal because
`end_frame` includes the composer pass, which the encode cache
doesn't help — the encoder pass itself saves more than 17 % in
absolute terms but is diluted across the larger denominator. See
"Where the time goes" below.

## Decisions

- **Key**: `(WidgetId, subtree_hash, available_q)` — same shape as
  `MeasureCache`. Origin shifts handled by subtree-relative storage,
  not by including origin in the key.
- **Subtree-relative storage**: cmd rects stored with the subtree-root
  origin subtracted; on replay rects are translated back by the
  current root's `layout.rect(id).min`. Survives parent origin shifts
  (scroll / resize / reflowed siblings) without invalidating.
- **Cascade is NOT in the key.** Re-reading `encoder/mod.rs:34-134`
  against `cascade.rs:24-40`: inside a subtree we enter, the encoder
  reads (a) `is_invisible` per node — but for descendants this is
  determined by own authoring + ancestors *within* the subtree, both
  captured in `subtree_hash`; (b) `attrs.is_clip()`, `extras.transform`
  — authoring, in `subtree_hash`; (c) `screen_rect` — only when
  `damage_filter.is_some()`. Lock this with a comment in both files.
- **Damage-filtered frames bypass the cache.** When
  `damage_filter.is_some()`, the encoder reads `screen_rect` per-node,
  which folds ancestor cascade. Don't try to cache that path;
  full-repaint frames (resize, theme, first frame) and
  `damage_filter=None` paths get the win. Steady-state animated frames
  already have the existing in-encoder damage skip.
- **Reuse the `MeasureCache` shape**: SoA arenas, per-`WidgetId`
  snapshot, in-place rewrite on same-len, append-and-mark-garbage on
  size change, compaction at `live × 2`, sweep on `removed`.

## What shipped

- `EncodeSnapshot { subtree_hash, available_q, cmds: Span, data: Span }`.
- Three SoA arenas — `kinds_arena`, `starts_arena` (parallel,
  subtree-relative offsets), `data_arena` — plus an
  `FxHashMap<WidgetId, EncodeSnapshot>` index. Same `COMPACT_RATIO = 2`
  / `COMPACT_FLOOR = 64` as `MeasureCache`; eviction-locked via the
  shared `removed` sweep in `Ui::end_frame`.
- `RenderCmdBuffer::extend_from_cached` is the cache-replay primitive;
  `bump_rect_min` is the shared rect-shift helper used by both replay
  and `EncodeCache::write_subtree`.
- `available_q` was promoted from `LayoutScratch` to `LayoutResult` so
  the encoder can read it without reaching into the engine.
- Encoder is now an entity (`pub(crate) struct Encoder { cache, cmds }`);
  `Frontend` holds `Encoder` + `Composer`, both own their outputs and
  return them from the do-the-work method.

## Where the time goes

The bench measures `end_frame()`, which runs:

1. layout (measure cache hit ⇒ ~free in both arms)
2. cascade rebuild
3. damage compute
4. **encoder** (cache hit vs forced miss — only this differs)
5. composer (cmd stream → quads — runs in both arms)

So the cached-vs-miss delta is purely encoder work, but it sits on top
of (2) + (3) + (5) which are shared denominators. On the nested
workload, the composer alone walks ~3 200 cmds emitting one quad each;
that's the floor that pulls the percentage down. The encoder pass
itself is saving substantially more than 17 % in absolute terms.

## Follow-up wins (in rough order of bang-for-buck)

1. **Composer pass.** It's the biggest constant in `end_frame` once
   the encode cache is in. Same key idea would work: snapshot
   `(quads, texts, groups)` per cached encode subtree, rewrite scissor
   rects under the current root origin at replay time. Bigger ROI than
   anything below — the structural pattern is identical to this cache,
   just one layer further down the pipeline. Estimated win: pulls
   nested closer to the cumulative ratio (cached encode × cached
   compose).

2. **Hit-hint propagation.** Both caches key on
   `(WidgetId, subtree_hash, available_q)` and sweep on the same
   `removed` list, so a measure-cache hit is by construction an
   encode-cache hit too (modulo independent `__clear` for benches).
   Layout can write a `Vec<bool>` (or a packed bit on `LayoutResult`)
   marking which subtree roots were measure-cache hits this frame; the
   encoder reads the bit and skips its own hash-map probe on those
   subtrees, going straight to the arena slice copy. Saves one
   `FxHashMap::get` per cached subtree. Tiny per-call win, only sound
   while the two caches stay eviction-locked. Worth doing once a
   profile shows hashmap lookups in the top frames.

3. **Damage-aware encode cache.** Currently
   `damage_filter.is_some()` bypasses the cache entirely, so animated
   frames don't get the win. The follow-up is a damage-aware replay:
   subtree-skip when `screen_rect ∩ damage = ∅`. The cached cmds are
   already correct (they don't change with damage region — they're the
   full subtree); we'd just gate the replay on intersection. Closer to
   a damage optimization than to this cache, but composes naturally.

4. **Shape-payload SIMD on `bump_rect_min`.** The replay loop reads
   2× f32 / writes 2× f32 per rect-bearing cmd. With 3 200 cmds, that's
   ~12 800 f32 ops on the hot path. The kinds array is small enough
   for a SIMD-friendly precomputed mask (rect-bearing bit per cmd) so
   `bump_rect_min` could vectorize the rect shifts. Only worth it if
   profiles show this loop hot — currently it's dominated by composer
   iteration, not the rewrite.

5. **Bypass cache for tiny subtrees.** A subtree of 1-2 cmds costs
   more in hashmap probe + write_subtree bookkeeping than it saves on
   replay. A `min_cmds_for_cache: usize` (say 4) threshold would skip
   speculative writes for leaves and small panels. Speculative — needs
   a profile showing per-leaf overhead before tightening.

6. **Coarser `available_q` quantization.** 1-logical-px granularity
   may bust the cache on sub-pixel parent drift (e.g. animated `Fill`
   children). Bump to 2 px or 4 px if a profile shows hash-match /
   avail-mismatch as a common miss path.

## Decisions revisited

The "Decisions" section above documents the design choices from the
planning phase. After landing, all of them held:

- Subtree-relative storage: round-trip is byte-identical (no float
  drift on integer-px inputs; tests pin this).
- Cascade-not-in-key: confirmed by reading `encoder/mod.rs` —
  `damage_filter.is_some()` is the only path that reads `screen_rect`,
  and we bypass the cache in that branch.
- Reusing `MeasureCache` shape: SoA arenas + per-`WidgetId` snapshot +
  in-place rewrite + append-on-mismatch + compact at `live × 2`.
- `extend_translated` (the doc's original primitive name) was dropped
  in favor of `extend_from_cached` once `EncodeCache` stored
  subtree-relative slices directly. Same correctness anchor
  (`bump_rect_min`), simpler call site.
