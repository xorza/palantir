# Todo

remove dim from frontend

Open work pulled from `docs/`. Each item is shipped-when-conditions-merit;
no committed roadmap.

## Damage rendering (`docs/damage-rendering.md`)

- **Per-node `RenderCmd` cache.** Cache encoder output per `NodeId`; on a clean node whose `(NodeHash, cascade row)` matches, replay the slice instead of re-encoding. CPU win on every partial-repaint frame.
- **Multi-rect damage.** Replace the single union rect with N disjoint regions (clustered from the per-node dirty set). Avoids the 50% heuristic tripping when two unrelated corners change.
- **Incremental hit-index rebuild.** Only update `HitIndex` entries for dirty nodes (and any whose cascade row changed) instead of walking every node every frame.
- **Debug overlay.** Toggleable mode that flashes dirty nodes red and outlines the damage rect — trivial once the per-node dirty set has a real consumer.
- **Tighter damage on parent-transform animation.** A dedicated transform-cascade pass to collapse deep-subtree damage to a tight bound; only worth it if profiling shows the current union is too coarse.
- **Manual damage verification.** Visual A/B against `damage = None` to catch the case where the diff misses something.

## Text (`docs/text.md`, `docs/text-reshape-skip.md`)

- **Layer B — `CosmicMeasure.cache` eviction.** Refcount `TextCacheKey` by live `WidgetId`s; sweep via `SeenIds.removed()` so the shaped-buffer table doesn't leak. Defer until a string-churn workload demonstrates the leak.
- **Wallclock bench for the reuse cache.** `benches/layout.rs` runs without cosmic, so it can't see the Layer A win. Add a cosmic-enabled variant with N=100 static labels and quote real µs/frame numbers.
- **`Shape::Text.text: String` allocs.** Each `Text::show` clones into the shape every frame. Move to `Cow<'static, str>` for static labels; intern dynamic strings via `Arc<str>` keyed on `text_hash`. Profile-gate before shipping.
- **Editable text.** `TextEdit` widget with one `cosmic_text::Editor` per `WidgetId`, glyph-level hit-test (`Buffer::hit`), IME plumbing through `winit`, selection rendering as sibling `RoundedRect` shapes. Blocked on the persistent `Id → Any` state map.
- **Color-space verification.** Glyphon outputs sRGB; confirm text doesn't look faded on a linear surface format and document the rule.
- **Atlas eviction under multi-font / multi-size load.** Verify `atlas.trim()` + glyphon's shelf overflow holds up over a long session.

## Persistent state

- **`Id → Any` state map.** Cross-frame storage keyed by `WidgetId` for scroll, focus, animation, editor state. Gates `TextEdit`, drag tracking, persistent scroll position, and any "remembered between frames" widget concern.
- **Drag tracking.** Build on the existing `Active`-capture so `drag_delta` works rect-independent (pointer can leave the originating widget mid-drag).
