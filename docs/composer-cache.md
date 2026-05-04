# Composer cache

Subtree-skip on the composer, mirroring the
[encode cache](../src/renderer/frontend/encoder/encode-cache.md). Same
arena pattern, but stores per-subtree `RenderBuffer` slices (`Quad`s,
`TextRun`s, `DrawGroup`s) instead of `RenderCmdBuffer` cmds.

Code lives in `src/renderer/frontend/composer/cache/`. The `Composer`
struct owns the cache + output buffer and is the entry point from
`Frontend::build`.

## Mechanism

- **Key**: `(WidgetId, subtree_hash, available_q, cascade_fp)` — the
  encode-cache triple plus a 64-bit `FxHash` over the ancestor state
  the subtree's *physical-px* output depends on.
- **Cascade-keyed.** `cascade_fp` covers `current_transform`,
  `parent_scissor` (top of `clip_stack`), `display.scale_factor`, and
  `display.pixel_snap`. Any change misses; the encode cache still hits
  through these because its subtree-relative storage absorbs origin
  shifts. Net effect: a parent scroll / transform animation / DPI flip
  busts every descendant compose entry but the encoder still skips
  re-emission.
- **Subtree-relative groups.** `groups_arena` stores each
  [`DrawGroup`]'s `quads` / `texts` ranges with the snapshot's start
  offsets already subtracted. On replay the splicer adds the current
  frame's `out.quads.len()` / `out.texts.len()` back, so a cached
  snapshot survives changes to the *number* of quads / texts the
  parent emitted before it (pinned by
  `compose_cache_warm_frame_matches_cold_compose`).

## Subtree marker mechanism

The encoder brackets every cache-eligible subtree with two cmds:

```rust
EnterSubtree(EnterSubtreePayload { wid, subtree_hash, avail, exit_idx, .. })
ExitSubtree
```

`EnterSubtreePayload` is 32 bytes (`bytemuck::Pod`-clean — `wid` /
`subtree_hash` are repr-transparent newtypes around `u64`, plus
`AvailableKey { i32, i32 }`, `exit_idx: u32`, and a 4-byte trailing
`_pad` for u64 alignment). `exit_idx` is patched in-place when the
matching close is recorded — `RenderCmdBuffer::push_exit_subtree`
rewrites the payload word — so a cache hit fast-forwards the cmd
iterator past the cached range without re-scanning.

The composer iterates by index over `kinds`/`starts` (not via the
`Cmd` iterator) so it can jump straight to `exit_idx + 1` on a hit.
At every marker (hit or miss) the composer flushes the current group
and resets `last_was_text = false` so the cached subtree's first
group doesn't merge with the parent's tail and the parent's first
post-splice quad doesn't trip the text-then-quad split rule against
the inner subtree's last text.

## Storage

- `ComposeSnapshot { subtree_hash, available_q, cascade_fp, quads,
  texts, groups }` — 48 bytes.
- Three SoA arenas — `quads_arena: Vec<Quad>`, `texts_arena:
  Vec<TextRun>`, `groups_arena: Vec<DrawGroup>` — plus an
  `FxHashMap<WidgetId, ComposeSnapshot>` index. Same `COMPACT_RATIO =
  2` / `COMPACT_FLOOR = 64` as `EncodeCache`; eviction-locked via the
  shared `removed` sweep in `Ui::end_frame`
  (`Frontend::sweep_removed` cascades to both caches).
- Hot path: same `(subtree_hash, avail, cascade_fp)` *and* same length
  triple `(quads_len, texts_len, groups_len)` ⇒ in-place rewrite
  preserves snapshot positions. Length mismatch appends and marks the
  old range as garbage; tracked via `live_quads` / `live_texts` /
  `live_groups` for the compaction trigger.

## Tests

- `src/renderer/frontend/composer/cache/tests.rs` — unit tests for the
  cache itself: round-trip lookup returns subtree-relative groups,
  hash / `available_q` / `cascade_fp` mismatch all miss, in-place
  rewrite preserves positions, `sweep_removed` evicts and decrements
  live counters, `clear`.
- `src/renderer/frontend/composer/tests.rs::cache_integration` —
  integration tests: warm-cache replay through `Frontend::build` is
  byte-identical to a fresh cold compose; a `__clear_compose_cache()`
  between two warm frames also reproduces byte-identical output;
  cache populates non-trivially on a 50-node workload.

## Bench

`benches/compose_cache.rs`, A/B'd against an otherwise-identical
warm-cache frame with `__clear_compose_cache()` between iterations
(measure + encode caches held hot in both arms, so the delta is
purely composer work). Two arm pairs:

- **Full pipeline** (`{flat,nested}/{cached,forced_miss}`): times
  `begin_frame() + build() + end_frame()` end-to-end. The
  compose-cache delta is invisible at this granularity (~0–1 % on
  this workload) because compose is a tiny fraction of `end_frame`
  cost — the dominant cost lives in the layout / cascade / damage
  passes.
- **Compose-only** (`{flat,nested}/compose_only/{cached,forced_miss}`):
  re-runs `Composer::compose` over the last frame's cmd buffer in a
  tight loop, isolating the compose stage.

| Workload | compose_only/cached | compose_only/forced_miss | speedup |
|---|---|---|---|
| `flat`   (~1000 leaves)            | 11.5 ns | 24.6 ns | 2.1× |
| `nested` (100 × 32 nodes ≈ 3200)   | 12.0 ns | 1697 ns | **141×** |

On a cache hit at root the composer reads one `EnterSubtree`, looks
up, splices the cached arrays, and fast-forwards to `ExitSubtree + 1`
— ~12 ns total.

## Threshold

`TINY_SUBTREE_THRESHOLD = 4` (raised from 1 when the markers landed):
gates both encode-cache eligibility and `EnterSubtree`/`ExitSubtree`
emission. Subtrees smaller than this aren't worth the
hashmap-probe + marker-emission tax against re-composing 1–4 quads.
Verified by bench against the pre-Step-1 baseline:
`nested/forced_miss` improved 1.8 % (the marker+threshold bump nets
out faster than the original threshold-1 baseline; `flat` and
`*/cached` arms within noise).

## Open variants (deferred)

The original plan considered a "translate-aware" variant B that
stores quads ancestor-relative and rebases by the parent's translate
on replay, surviving ancestor scroll/translate animations. Skipped:
re-snapping pre-snap floats per replay quad doubles the per-quad
work, and the snap-handling tests are nontrivial. Revisit if a
real workload (scroll-heavy app, animated parent) shows
cascade-fingerprint misses dominating compose cost.

Future-work items (damage-aware compose replay, SIMD splice, coarser
cascade fingerprint quantization) live in `docs/todo.md`.
