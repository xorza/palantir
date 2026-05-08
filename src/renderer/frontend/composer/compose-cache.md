# No compose cache

Removed in May 2026. This file replaces the previous design doc.

## Why we don't cache compose

Compose-cache contribution measured at **~0.2% of frame time** in
steady state and **~0% under scroll** — see
`docs/compose-cache-under-scroll.md` for the full bench.

The contribution stays at ~0.3% even on a heavy workload (rounded
stencil clips on every panel + row, real cosmic-text shaping, deeper
nesting, strokes). For comparison the measure cache delivers ~35% on
the same workloads. The composer's actual job — scaling, snapping,
scissor-grouping a few thousand quads + text runs — is fast enough
that caching the output of it is dwarfed by the cache machinery
itself (snapshot probe, in-place rewrite, sweep, compaction, marker
emission).

The caching layer was also wrong-shaped for scroll. Its key included
`cascade_fp = hash(current_transform, parent_scissor, scale, snap,
viewport)`, so any ancestor `Scroll` mutating its translation per
frame busted every snapshot under it. Encode cache survived (its key
is subtree-relative); compose paid pure overhead during scroll.

## Why we didn't make it survive scroll

Two options were on the table:

1. **Subtree-relative compose cache** (variant B in the deleted doc):
   store quads / text runs ancestor-relative, drop the translation
   bits of `cascade_fp`, rebase per-quad on splice. **Pixel-snap
   soundness is the blocker.** Snapping in subtree-local coords
   yields different sub-pixel offsets than screen-space snap when the
   parent translate has a fractional component. Browsers solve this
   by snapping the *layer's composite transform* to integer pixels —
   only sound when a per-layer texture exists. For a flat quad list
   you'd be quantizing the scroll offset to whole physical pixels
   (Slint accepts this in its software renderer); a UX regression
   for a 0.2% cache.

2. **Per-Scroll offscreen texture (compositor-style layer cache).**
   The canonical industry answer — Flutter `RepaintBoundary`,
   Chromium cc layers, Compose `graphicsLayer`, WPF `BitmapCache`,
   WebRender picture caching all do this. Render scroll content to a
   wgpu texture once, blit-with-offset on subsequent frames. Big
   architectural addition; valuable when scroll perf becomes
   workload-driven; deliberately out of scope when this cache was
   removed. See `docs/compose-cache-under-scroll.md` for the
   recommendation.

## What was here

- `ComposeCache` — `FxHashMap<WidgetId, ComposeSnapshot>` over three
  parallel `LiveArena`s for `Quad`, `TextRun`, `DrawGroup` (one
  snapshot per cache-eligible subtree).
- `cascade_fingerprint(t, scissor, scale, snap, viewport) -> u64` —
  the cascade-state hash mixed into the key.
- `try_splice` / `write_subtree` — splice a hit into the output
  buffer rebasing group ranges; rewrite the snapshot on miss.
- `subtree_stack: Vec<SubtreeFrame>` — composer scratch tracking open
  `EnterSubtree`s during the walk so `ExitSubtree` could capture the
  produced tail slices.
- `sweep_removed` — drop snapshots for `WidgetId`s evicted this
  frame, fanned from `SeenIds`'s removed slice along with the other
  caches.

The `EnterSubtree` / `ExitSubtree` markers in `RenderCmdBuffer` were
also removed when the encode cache was subsequently deleted (see
`encode-cache.md`). They had no other consumer.

## Bring it back if

- A workload bench shows compose taking >5% of frame time.
- The frame already has an offscreen texture per `Scroll` (option 2
  above), and a remaining hot path inside that texture would benefit
  from caching cmd-stream output.

Otherwise: don't.
