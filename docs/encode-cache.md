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

## Steps

1. **Add `available_q` to `LayoutResult`.** One `Vec<AvailableKey>`
   parallel to `desired`, written in `LayoutEngine::measure`. Encoder
   needs the same key dimension the measure cache uses; right now it's
   intra-engine.

2. **Translate-aware append on `RenderCmdBuffer`.** Add
   `extend_translated(&mut self, src: &Self, cmd_range: Range<u32>,
   data_range: Range<u32>, offset: Vec2)`: copies cmd kinds,
   recomputes `starts`, copies the data slice, and rewrites
   `rect.min` in `DrawRect`/`DrawRectStroked`/`DrawText`/`PushClip`
   payloads in-place. `PushTransform` / `Pop*` pass through untouched
   (transforms are subtree-local; they compose with parent at
   composer-time).

3. **`EncodeCache` (`src/renderer/frontend/encoder/cache.rs`).**
   Mirror `layout/cache/mod.rs`. Two arenas (`kinds: Vec<CmdKind>`,
   `data: Vec<u32>`) plus per-id `EncodeSnapshot { subtree_hash,
   available_q, kinds_start, kinds_len, data_start, data_len }`.
   `try_lookup` returns the cmd/data slice pair. `write_subtree` does
   the subtract-origin rewrite at insertion time. In-place rewrite
   when both `kinds_len` and `data_len` match (~always: same
   `subtree_hash` → identical cmd shape and payload sizes).
   `sweep_removed`, `compact`, `clear`. Same `COMPACT_RATIO` /
   `COMPACT_FLOOR` constants.

4. **Hook into `encode_node`.** Before the shape loop, when
   `damage_filter.is_none()` and not invisible: `if let Some(hit) =
   cache.try_lookup(wid, subtree_hash, available_q)` →
   `out.extend_translated(...)`, return. On miss: capture `out.len()`
   and `out.data.len()` as `snap_start`s, run normal encode for self +
   children, then `cache.write_subtree(...)` with the resulting slices
   and `origin = layout.rect(id).min`. Cache passed in as a new `&mut
   EncodeCache` parameter on `encode` / threaded onto `Frontend`.

5. **Tests** (`encoder/cache_tests.rs`).
   - Cached replay byte-identical to cold encode (after origin
     translate).
   - In-place rewrite keeps snapshot positions stable across unchanged
     frame.
   - Subtree-skip across origin shift produces correctly translated
     cmds.
   - Authoring change (color, text, added child) busts the right slot.
   - `damage_filter = Some(_)` does not consult the cache.
   - `sweep_removed` evicts; compaction preserves slot validity.

6. **Bench.** Extend `benches/measure_cache.rs` with `encode_flat` /
   `encode_nested` groups: build the same workloads, time
   `frontend.build()` `cached` vs `__clear_encode_cache()`
   `forced_miss`. Target: ≥20% on the nested workload.

7. **Docs + status.** Update this doc with shipped status + numbers.
   Tick a box in `CLAUDE.md` Status. Cross-link from
   `measure-cache.md` "Not done — deferred" since that doc currently
   implies encode-cache is the next obvious extension.

## Follow-up: hit-hint propagation

Both caches key on `(WidgetId, subtree_hash, available_q)` and sweep
on the same `removed` list, so a measure-cache hit is by construction
an encode-cache hit too (modulo independent `__clear` for benches).
Layout can write a `Vec<bool>` (or a packed bit on `LayoutResult`)
marking which subtree roots were measure-cache hits this frame; the
encoder reads the bit and skips its own hash-map probe on those
subtrees, going straight to the arena slice copy. Saves one hash
lookup per cached subtree.

Tiny win, only sound while the two caches stay eviction-locked.
Don't block the initial encode-cache landing on this — add it as a
follow-up once benches show the per-subtree lookup cost matters.

## Risks / open questions

- **Float determinism on translate.** Subtract+re-add `origin` can
  drift one ulp vs a cold-encode comparison. Tests compare with an
  epsilon; if showcase shows visible jitter, store the delta and apply
  once at replay rather than round-tripping.
- **Damage-frame coverage.** Skipping the cache on
  `damage_filter=Some` means animated frames don't benefit. If
  profiles show that's the bottleneck, the follow-up is a damage-aware
  variant (subtree skip when `screen_rect ∩ damage = ∅`), which is
  closer to a damage optimization than to this cache.
- **Stroke / no-stroke variant split.** `RenderCmdBuffer` already
  splits `DrawRect` and `DrawRectStroked` by kind, so the data-len
  check in step 3 is a sufficient size guard — a fill→stroked switch
  flips the kind and the rewrite falls through to the append path
  correctly.
