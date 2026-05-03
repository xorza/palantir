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

## Sketch of A's implementation

1. **Per-subtree compose snapshot.** When `Encoder::encode` writes a
   subtree to its cache, also signal the composer: "this WidgetId
   covers `cmds[cmd_lo..cmd_hi]`; if you compose it under stable
   ancestors, cache the resulting `quads`/`texts`/`groups` slices."
   This means the composer needs the same per-subtree boundary
   markers the encoder has.
2. **Composer reads per-subtree boundaries.** Either:
   - Encode cache exposes its `EncodeSnapshot` table to the composer
     (subtree boundaries in cmd-index space); composer correlates as
     it walks the stream.
   - Encoder writes a parallel `Vec<SubtreeMark { wid, cmd_lo, cmd_hi }>`
     for the composer to consume.
3. **Cascade fingerprint.** Composer hashes
   `(current_transform, parent_scissor, scale, snap)` at the
   subtree boundary; that's the cascade half of the cache key.
4. **`ComposeCache`**: `FxHashMap<WidgetId, ComposeSnapshot>` with
   `ComposeSnapshot { subtree_hash, available_q, cascade_fingerprint,
   quads: Span, texts: Span, groups: Span }` over three arenas.
5. **Eviction-locked** with measure + encode caches: same `removed`
   sweep at `Ui::end_frame`.

## Open questions before implementation

- **Group splitting at subtree boundaries.** Cached groups must not
  bleed into the surrounding group state. May require flushing the
  active group at every subtree boundary on cold compose, even when
  scissor is unchanged — small overhead on cold frames, simpler
  bookkeeping.
- **`last_was_text` discipline at replay.** Cached `groups` start
  with their own scissor; cold replay must flush the surrounding
  group before splicing. Same flush-at-boundary discipline as above.
- **Memory floor.** Three vec arenas × per-`Quad` 68 bytes + per-text
  ~32 bytes + per-group ~32 bytes. On the nested workload (~3 200
  cmds → ~3 200 quads), live `quads_arena` ~ 220 KB. Comparable to
  the encode cache's `data_arena`.
- **Interplay with damage.** Damage-filtered frames already bypass
  the encode cache; compose cache must follow the same rule. Animated
  frames hit neither cache; that's where B's translate-aware variant
  would matter.

## Effort estimate

A, no B: ~1-2 days of focused work. ~300 lines of cache code mirroring
`encoder/cache/`, ~50 lines of composer hook plumbing, ~100 lines of
tests. Bench harness already exists (`benches/encode_cache.rs`
pattern).

B, on top of A: another ~1-2 days, mostly snap-handling tests and the
ancestor-relative storage rewrite.

Add to `docs/todo.md` "Encode cache" section's #1 with a pointer here
once the plan is firm.
