# Cascade subtree-skip cache

Cross-frame subtree cache for `CascadesEngine::run`. Skips the per-node
walk for subtrees whose `(WidgetId, subtree_hash, parent_prefix,
root_rect_q)` matches a snapshot from the previous frame; on hit,
blits the cached per-node rows (`Cascade`, `subtree_paint_rect`,
`EntryRow`, paint `Span`) and per-paint rows (`Paint`,
`shape_to_paint` links) into the live cascade arenas.

Live: `src/ui/cascade/cache.rs` + integration in
`src/ui/cascade/mod.rs`. Sweep eviction rides on the same
`Ui::finalize_frame` plumbing as `MeasureCache` and friends.

## Bench

ASUS ROG (i9-13980HX, P-core), `cargo bench --bench frame --features internals`:

| arm          | no cache  | with cache | delta |
| ------------ | --------- | ---------- | ----- |
| cached_cpu   | 102.66 µs | 96.05 µs   | **−6.4%** |
| partial_cpu  | 141.14 µs | 131.67 µs  | **−6.7%** |
| resizing_cpu | 1423.5 µs | 1450.4 µs  | +1.9% (noise, p=0.80) |

GPU arms within ±5% noise band.

## Design

### Eligibility

Only subtrees with `span >= MIN_CACHEABLE_SPAN` (currently 256) are
probed and captured. The threshold is calibrated against the bench
fixture (~840-node tree): one root-ish subtree (~820 nodes) clears
the bar and accounts for nearly every hit; intermediate ancestors
(30–500 nodes) were captured at lower thresholds but never amortized
their write cost.

### Hit poisoning of ancestors

On a cache hit at depth d, every frame on the walk stack (i.e. every
strict ancestor of the hit subtree) has its `paint_capture_start` set
to `u32::MAX` — the "skip capture" sentinel. The reasoning is direct:
a subtree that contains a static (cache-hitting) descendant *and*
arrived through a miss is itself dynamic — its own hash will shift
again next frame and any snapshot of it would be dead weight. Without
this rule, partial-damage workloads (one animated counter / blinking
caret) re-captured the root subtree every frame at full size, turning
the cache into a net loss (+8% partial regression in the prototype).
With the rule, partial sees 0 captures in steady state and crosses
into a clean win.

### In-place rewrite on same-shape captures

When evicting a snapshot whose new capture has the same node /
paint / shape-link counts, the cache overwrites the existing arena
slots rather than evict-and-append. Without this, an animated widget
whose authoring hash shifts every frame would grow the arenas
monotonically and violate the alloc-free invariant
(`alloc_free` test pins zero blocks in steady state).

### Storage

Per-node arenas (`rows: Vec<Cascade>`, `sptrs: Vec<Rect>`,
`entries: Vec<EntryRow>`, `paint_spans: Vec<Span>`) share a
`node_live` liveness counter. Per-paint and per-shape-link data
ride on `LiveArena`s (`paints`, `shape_links`). Compaction is not
yet wired — `release` marks slack in place; if arena bloat shows
up under long-lived workloads, add the same mark-garbage compaction
path `MeasureCache` uses.

Per-snapshot key/extent metadata:

```text
Snapshot {
    key: ProbeKey,                  // 32 B (subtree_hash + parent_prefix + rect_q)
    nodes: Span,                    // range in 4 per-node arenas
    paints: Span,                   // range in `paints` arena
    shape_links: Span,              // range in `shape_links` arena
    root_paint_rect: Rect,          // 16 B (parent-stack rollup on hit)
}
```

### Frame state

`Frame::paint_capture_start: u32` doubles as the "should capture this
frame on pop?" signal:

- Set to `cascades.paint_arena.rows.len()` at push for cacheable subtrees.
- Set to `u32::MAX` for non-cacheable subtrees (sub-threshold span).
- Overwritten to `u32::MAX` on every ancestor when a descendant hits
  (the poisoning rule above).

`finalize_and_capture` reads the field on pop: `u32::MAX` skips the
recompute + insert path; any other value triggers the capture.

The probe key for capture is recomputed at pop time rather than
stashed on `Frame` — `stack.last().cascade_prefix.finish()` gives the
parent prefix (the stack was popped already), and the other inputs
come straight from `tree.rollups` and `layout.rect`. Saves 32 B per
`Frame` on the push side; the extra `finish()` call only fires on
captures (rare in steady state).

### Why the encode cache postmortem warnings didn't apply

`docs/cache-history/encode.md` declined a per-subtree encode cache
because re-encoding was already `Vec<u32>::extend_from_slice`-shaped
and the cache's "store-relative, rebase on replay" did equal work in
a different shape. The cascade walk is **not** memcpy-shaped — every
node runs `compute_paint_rect` (per-shape transform composition,
per-shape paint-rect emission via `lift_to_screen`/`Rect::union`) +
`build_cascade_prefix` + `finish_cascade_input`, all genuine arithmetic
the cache replaces with `extend_from_slice` of pre-computed rows.

What the postmortem *did* correctly predict: a naive cache that
captures every miss balloons the bookkeeping cost above the savings.
The `MIN_CACHEABLE_SPAN = 256` gate plus the hit-poisoning rule are
what made the bench cross from "marginal speedup + partial
regression" into "clean win on every CPU arm".

## Bring it forward if

- Cache hit rate degrades in a real-app workload — instrument
  `CascadeCache::{hits, misses, captures, nodes_blit}` via the
  existing `ui.cascade_cache()` accessor in the showcase HUD.
- Arena bloat shows up on long-lived workloads — add compaction
  borrowed from `MeasureCache`.
- A profile shows the in-place rewrite path as a hot spot — the
  current implementation does per-entry SoA push rebuilding from
  `widget_id() / rect() / sense() / focusable() / disabled() /
  layout_rect()` slices, which is the costliest per-node op. A bulk
  Soa-column writer (likely needing `unsafe` into `soa-rs`) would
  trim it.
