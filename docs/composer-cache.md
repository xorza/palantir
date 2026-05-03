# Composer cache (planning)

Status: **planning, not implemented**. This doc captures the design
exploration before any code lands. Lives in `docs/` (not `src/`)
because nothing in the codebase references it yet.

## What compose does today

`Composer::compose` (`src/renderer/frontend/composer/mod.rs`) walks
the cmd stream linearly and produces:

- `quads: Vec<Quad>` — 68-byte physical-px instance per `DrawRect[Stroked]`
  (rect + radius + fill + stroke, all scaled by `current_transform.scale ×
  display.scale_factor`, snapped per `display.pixel_snap`).
- `texts: Vec<TextRun>` — origin + scissor bounds + color + glyph key
  per `DrawText`.
- `groups: Vec<DrawGroup>` — `(scissor, quads_range, texts_range)`
  triples that batch consecutive draws with the same scissor.

State maintained during the walk:

- `current_transform: TranslateScale` — composed parent-of-self
  transform, mutated by Push/PopTransform.
- `clip_stack: Vec<URect>` — physical-px scissor stack, intersected
  against parent on every PushClip.
- `last_was_text: bool` — splits group when a quad follows text.

## Why this is harder than the encode cache

The encode cache wins because every cached cmd's `rect.min` is
**subtree-local**: subtract root origin at write, add live origin at
replay. Authoring inside the subtree (`PushClip(rect)`,
`PushTransform(t)`, draw rects) is captured in `subtree_hash`, so
`(subtree_hash, available_q)` is a complete key.

The composer's output depends on **ancestor cascade state** that
`subtree_hash` does NOT capture:

1. **Ancestor transform composition.** `current_transform` at the
   point we enter a subtree is the product of every ancestor's
   transform. A parent transform change (animation, scroll-as-translate,
   resize-driven layout shift) changes every descendant `quad.rect`
   by more than just an origin offset — the scale factor multiplies
   through. Cached quads are stale.
2. **Ancestor clip stack.** Group scissors are physical-px
   intersections of every active ancestor clip. A parent clip change
   shifts every descendant group's `scissor`. Cached groups are stale.
3. **Display scale + pixel snap.** A DPI change (monitor switch,
   user zoom) re-quantizes every coordinate. Cached buffers are stale.

So the composer cache key has to carry strictly more than the encode
cache key. The natural extension is

```
(WidgetId, subtree_hash, available_q, ancestor_transform, ancestor_scissor, scale, snap)
```

— i.e. cascade-in-key. That kills the encode cache's "survives parent
origin shift" property: any scroll / resize / DPI change busts every
descendant compose entry.

## Two viable designs

### A. Cascade-keyed (simple, narrower win)

Key on the full tuple above. Replay is a memcpy of the cached `quads`,
`texts`, `groups` slices into `RenderBuffer`. Hits only when nothing
in the cascade chain moved.

- **When it wins:** static panels (sidebars, footers, header bars,
  most non-animated UI) — exactly the same shapes the encode cache
  already wins on, *but only when the parent tree is also stable*.
- **When it doesn't:** any frame where an ancestor scrolls, resizes,
  or animates its transform. Encode cache still wins these; composer
  cache misses.
- **Estimated win:** narrows the encode-cache's "everything stable"
  case (currently dominated by composer iteration) substantially.
  Adds nothing on the "ancestor moved" frames where encode cache hit
  but composer cache misses.

### B. Translate-aware (broader win, more code)

Store quads/texts/groups in **ancestor-relative** form: subtract the
ancestor `current_transform.translate` and `parent_scissor.min` at
write time. On replay, walk the cached vectors and add the live
ancestor offset back. Key drops `ancestor_transform.translate` and
`ancestor_scissor.min`, keeps `ancestor_transform.scale`,
`parent_scissor.size`, `scale`, `snap`.

- **Pixel snap is the problem.** Snapping quantizes positions
  non-linearly: shifting a pre-snapped physical-px rect by a sub-pixel
  ancestor offset and re-snapping doesn't equal cold-composing at the
  shifted origin. Either:
  - (i) Skip snap on cached entries — accept sub-pixel artifacts on
    moved cached subtrees.
  - (ii) Store pre-snap floats and re-snap at replay — doubles the
    per-quad work on replay.
  - (iii) Only allow translate-aware caching when ancestor offset is
    integer-px (post-snap stable). Restricts hit set.
- **Scale change still busts.** Display scale change OR ancestor
  scale change (`current_transform.scale`) requires full re-compose.
- **Estimated win:** survives ancestor scroll/translate, the most
  common animated case. ~2× more code than A; lots of edge cases on
  snap.

## Recommendation

**Ship A first** (cascade-keyed). Reasons:

1. Mirrors the encode cache structure 1:1 — same `WidgetId`-keyed
   `FxHashMap`, same SoA arenas (`quads_arena`, `texts_arena`,
   `groups_arena`), same `live_*` / `COMPACT_RATIO` machinery.
   ~250-300 lines of cache code, mostly mechanical.
2. Replay is a memcpy of three slices (no per-element rewrite). Hot
   path is shorter than the encode cache's `bump_rect_min` loop.
3. Bench numbers will tell us whether B is worth the snap-handling
   tax. If A pulls nested past 30 % of `end_frame`, B's marginal win
   on animated frames may not justify the complexity.

Defer B unless a real workload (scroll-heavy app, animated parent)
proves cascade-keyed misses are the dominant cost.

## Subtree-marker mechanism: decided

The composer needs to know "I'm now entering subtree X (`WidgetId`),
spanning cmds [N..M]". Today it has zero subtree awareness — just
`cmds.raw_iter()`. Three mechanisms were considered (parallel
`Vec<SubtreeMark>`, in-stream `EnterSubtree/ExitSubtree` cmds,
composer reading `EncodeCache` directly).

**Decision: cmd-stream markers.** Add two `CmdKind` variants:

```rust
EnterSubtree(EnterSubtreePayload { wid: WidgetId, exit_idx: u32 })
ExitSubtree
```

`exit_idx` lets a cache-hit fast-forward past the cmd range without
re-walking. Encoder emits the pair around each cached subtree;
composer dispatches normally, mirroring the existing
`PushClip/PopClip` and `PushTransform/PopTransform` stack discipline.

**Why this over the alternatives:**

- **Composes for free with the encode cache.** Markers are just cmds
  in the stream — `EncodeCache` already stores and replays cmd slices
  via `extend_from_cached`. Recursive composer-cache hits (cached
  panel containing cached frame) work mechanically; no fourth arena
  on `EncodeCache`, no parallel marker replay path.
- **Self-describing.** Nesting and ordering fall out of stack
  discipline, same as the two existing marker pairs in the cmd
  stream.
- **No coordination invariant** between cmd buffer and a side table.

**Trade-off accepted:** always-on cost of ~22 bytes per cached
subtree in the cmd buffer (kind+start × 2 + 12 byte payload), plus
two extra match arms in the composer's hot loop. On the nested
workload (~100-1000 candidate subtrees) this is 2-22 KB extra per
frame and adds dispatch arms to ~3200 iterations.

**Validation gate before committing to A:** Step 1's first sub-task
is a **spike** — add `EnterSubtree`/`ExitSubtree` as no-op cmds (encoder
emits, composer treats as fall-through), rerun
`benches/encode_cache.rs`, verify cold-frame overhead is under ~3 %.
If not, revisit (gate emission by subtree-cmd-count threshold, or
fall back to the parallel-Vec design).

## Implementation steps

**Step 1 — Plumbing.** Land the marker mechanism without any
caching.

1. Add `EnterSubtree` / `ExitSubtree` to `CmdKind` and `RenderCmd`,
   with `EnterSubtreePayload { wid, exit_idx }`. Push helpers on
   `RenderCmdBuffer`.
2. `Encoder::encode_node` emits `EnterSubtree(wid, ?)` before the
   shape loop and `ExitSubtree` after children — mirrors the existing
   cache-capture/write_subtree bracketing. `exit_idx` is patched in
   at `ExitSubtree` time (encoder records the `EnterSubtree`'s
   `start` offset, rewrites the payload's `exit_idx` field once the
   close cmd index is known).
3. `Composer::compose` adds two no-op match arms for the new kinds.
4. Add a `last_was_text = false` reset after `EnterSubtree` /
   `ExitSubtree` in the composer (subtrees own their own group flow;
   they shouldn't inherit text→quad split state from outside).
5. Force a group flush at `EnterSubtree` and `ExitSubtree` so
   cached groups don't bleed into the surrounding group state.
   Small cold-frame overhead, simpler bookkeeping.

**Acceptance gate:** existing `composer/tests.rs` and
`encoder/tests.rs::encode_cache_warm_frame_matches_cold_encode` pass
unchanged (the cmd-stream output gets longer, but the
`RenderBuffer` it composes to is byte-identical). `benches/encode_cache.rs`
shows < 3 % cold-frame regression.

**Step 2 — Cache.** Add `ComposeCache` mirroring `EncodeCache`.

1. **`ComposeCache`** at `src/renderer/frontend/composer/cache/`:
   `FxHashMap<WidgetId, ComposeSnapshot>` with
   `ComposeSnapshot { subtree_hash, available_q, cascade_fingerprint,
   quads: Span, texts: Span, groups: Span }` over three SoA arenas
   (`quads_arena: Vec<Quad>`, `texts_arena: Vec<TextRun>`,
   `groups_arena: Vec<DrawGroup>`). Same `COMPACT_RATIO` /
   `COMPACT_FLOOR` / `live_*` discipline as `EncodeCache`.
2. **Cascade fingerprint.** Composer hashes
   `(current_transform, parent_scissor.unwrap_or(default),
   scale, snap)` at each `EnterSubtree`. The hash is the cascade
   half of the cache key alongside `(wid, subtree_hash, available_q)`
   — but `subtree_hash` and `available_q` aren't free; they need to
   be in the marker. Either extend `EnterSubtreePayload` to carry
   them (~16 more bytes), or look them up via a side-channel from
   the encode cache. Decision: extend the payload; keeps the
   "self-describing cmd stream" property.
3. **Hook into compose loop.** On `EnterSubtree`: build cascade
   fingerprint, try lookup. On hit: splice cached `quads/texts/groups`
   into output (rebasing intra-group `quads`/`texts` ranges by
   current `out.quads.len()` / `out.texts.len()`), advance the cmd
   iterator to `exit_idx`, continue. On miss: push a frame onto a
   new `compose_subtree_stack: Vec<ComposeFrame>` capturing
   `(wid, fingerprint, quads_lo, texts_lo, groups_lo)`, continue
   normally. On `ExitSubtree`: pop the frame, call
   `compose_cache.write_subtree(...)`.
4. **Eviction-locked** with measure + encode caches:
   `Frontend::sweep_removed` cascades to `composer.cache.sweep_removed`.
5. **`__clear_compose_cache`** on `Composer` and `Frontend`,
   exposed via `Ui::__clear_compose_cache` for benches.

**Step 3 — Tests + bench.**

- `composer/cache/tests.rs`: round-trip at same/different ancestor
  state, fingerprint mismatch → miss, in-place rewrite, sweep,
  compaction. Mirror of `encoder/cache/tests.rs`.
- Integration: warm-frame `Frontend::build` cmd→compose path is
  byte-identical to a cold-build via fresh `Composer`.
- `benches/compose_cache.rs`: same flat/nested workloads. A/B `cached`
  vs `__clear_compose_cache()` `forced_miss`.

## Open questions parked

- **Memory floor.** Three vec arenas × per-`Quad` 68 bytes + per-text
  ~32 bytes + per-group ~32 bytes. On the nested workload (~3 200
  cmds → ~3 200 quads), live `quads_arena` ~ 220 KB. Comparable to
  the encode cache's `data_arena`.
- **Interplay with damage.** Damage-filtered frames already bypass
  the encode cache; compose cache follows the same rule. Animated
  frames hit neither cache; that's where B's translate-aware variant
  would matter.

## Effort estimate

| Phase | Scope | Estimate |
|---|---|---|
| Step 1 spike | EnterSubtree/ExitSubtree as no-ops; bench cold overhead | ~2 hours |
| Step 1 ship | Plumbing + group-flush discipline + test gate | ~0.5 day |
| Step 2 ship | ComposeCache + hookup + tests | ~1 day |
| Step 3 ship | Bench + measure | ~0.5 day |
| **Total A** |  | **~2 days** |

B, on top of A: another ~1-2 days, mostly snap-handling tests and the
ancestor-relative storage rewrite. Skip until evidence motivates it.
