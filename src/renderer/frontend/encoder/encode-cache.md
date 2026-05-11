# No encode cache

Removed in May 2026, shortly after the compose cache. Replaces the
previous design doc.

## Why we don't cache encode

Encode-cache contribution measured at **0.06%–0.9% of frame time**
across four workloads (light/heavy/dense/scroll). Full data:
`docs/encode-cache-investigation.md`. For comparison the measure
cache delivers ~35% on the same workloads.

The encoder is genuinely fast. It walks the SoA `Tree` columns
(cache-friendly), branches on `LayoutMode`, and pushes typed payloads
to a `Vec<u32>`. The cache replay was a `Vec::extend_from_slice` plus
per-cmd `start` adjust plus per-rect-cmd `rect.min` translate. Both
paths are O(cmds) memcpy-shaped; the constants are close enough that
re-encoding from scratch and replaying from cache run within ~1 µs of
each other.

The dense workload (6× shapes per row) was the giveaway: more cmds
should mean more amortization, but cache contribution **dropped to
0.06%**. The cache amortized nothing — it just did equal work in a
different shape.

## What was here

- `EncodeCache` — `FxHashMap<WidgetId, EncodeSnapshot>` over three
  parallel `LiveArena`s for `CmdKind`, `start: u32`, and `data: u32`
  (one snapshot per cache-eligible subtree).
- **Subtree-relative storage**: each cmd's `rect.min` was stored
  with the snapshot root's origin subtracted, then translated back
  by the current frame's origin on replay. Survived parent scroll /
  reflow without invalidating. Implemented by `bump_rect_min`,
  pinned by `CmdKind::has_leading_rect` + const offset asserts on
  every payload struct.
- **Subtree-relative `start` offsets** — payload offsets stored as
  offsets into the snapshot's local `data` slice, not the global
  arena. Compaction could move snapshots without touching them.
- `try_replay` / `write_subtree` — append cached cmds rebasing
  starts + rects on hit; rewrite snapshot in place (same hash) or
  append fresh (size mismatch) on miss.
- `EnterSubtree` / `ExitSubtree` cmd kinds in `RenderCmdBuffer`,
  along with `EnterSubtreePayload`, `EnterPatch`,
  `push_enter_subtree`, `push_exit_subtree`. Bracketed every
  cache-eligible subtree so the composer cache could splice over
  them; second consumer (this encode cache) used them to fast-forward
  past cached cmd ranges on replay. Both consumers now gone — the
  variants and machinery were removed with this cache.
- `EncodeOutcome::{Complete, Elided}` — encoder return type, governed
  whether `write_subtree` ran (off-screen-elided subtrees would
  produce incomplete snapshots if cached). With no cache, `encode_node`
  returns `()`; off-screen culling is just an early `return`.
- `TINY_SUBTREE_THRESHOLD = 4` — gated cache lookup + marker emission
  for tiny subtrees. Gone.
- `sweep_removed` integration on `Frontend` — fanout to encode + (then
  also) compose cache. With both render-side caches gone,
  `Frontend::sweep_removed` itself was removed; `Ui::post_record` no
  longer forwards the `removed` slice to the frontend.
- `clear_encode_cache` in `internals` — A/B helper for the bench.
- A pile of cache integration tests in
  `src/renderer/frontend/encoder/tests.rs`.

## What stayed

- The encoder itself: tree walk + cmd emission. Slightly simpler now
  (no marker emission, no cache_pending plumbing, no
  `EncodeOutcome` propagation).
- **Off-screen subtree culling**: if a node's screen rect doesn't
  intersect the viewport, the encoder early-returns and skips its
  whole subtree. Independent of the cache.
- **Damage filter**: leaf paint cmds skipped for nodes outside the
  damage rect. Clip / transform pairs still emitted so scissor groups
  and child transforms stay coherent.

## Bring it back if

- A future refactor makes encoder *expensive* — per-glyph layout
  inside encode, per-shape SDF prep, etc. Today encode is cheap
  because it's pure tree walk + payload push.
- A workload bench shows encode > 5% of frame time with no cache.
  Today it's < 5% even on the dense workload.

Otherwise: the work doesn't exist to cache.
