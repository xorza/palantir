# Todo

Open work pulled from `docs/`. Each item is shipped-when-conditions-merit;
no committed roadmap.

## Damage rendering (`docs/damage-rendering.md`)

- **Per-node `RenderCmd` cache.** Cache encoder output per `NodeId`; on a clean node whose `(NodeHash, cascade row)` matches, replay the slice instead of re-encoding. CPU win on every partial-repaint frame.
- **Multi-rect damage.** Replace the single union rect with N disjoint regions (clustered from the per-node dirty set). Avoids the 50% heuristic tripping when two unrelated corners change.
- **Incremental hit-index rebuild.** Only update `HitIndex` entries for dirty nodes (and any whose cascade row changed) instead of walking every node every frame.
- **Debug overlay.** Toggleable mode that flashes dirty nodes red and outlines the damage rect — trivial once the per-node dirty set has a real consumer.
- **Tighter damage on parent-transform animation.** A dedicated transform-cascade pass to collapse deep-subtree damage to a tight bound; only worth it if profiling shows the current union is too coarse.
- **Fuse `compute_hashes` into `Cascades::rebuild`.** Both walk every node once. ~10 µs saving on 100 nodes — defer until traces show it's hot.
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

## Layout — Stack (`docs/layout-potential-features.md`)

- **`flex-basis` + `flex-shrink`.** Preferred size separate from sizing policy + independent shrink/grow weights. Triggers on the third user request for "I want a preferred size that's neither min nor max" — at that point pick between in-tree and Taffy.
- **`flex-wrap` (multi-line wrapping).** New `LayoutMode::Flow` (~200 LOC) for chip lists / tag clouds / responsive button bars. Land when the first widget needs it.
- **`align-items: baseline`.** Leaves report a `baseline: f32` alongside their measured size; stack alignment grows a baseline branch. Triggers on the first form-label widget that visibly needs it.
- **`row-reverse` / `column-reverse`.** Lands with the broader RTL story; not standalone.
- **`order` (visual reordering).** Defer indefinitely — immediate-mode authors can just reorder calls.
- **Percentage sizes.** New `Sizing::Percent(f32)` resolving against parent's resolved size. Only worth it when a layout genuinely can't be expressed via Fill weights.

## Layout — Grid (`docs/layout-potential-features.md`)

- **`Track::repeat(n, t)` + `Track::minmax(min, max)` + `Track::fit_content(n)`.** Ergonomic shorthands over existing primitives. Bundle as one PR when track-list verbosity gets annoying.
- **Named areas.** Parser for `"header header" "sidebar main"` syntax + name resolution at recording time. Land when an example layout is painful via numeric placement.
- **`grid-auto-flow` + `grid-auto-rows` / `grid-auto-cols`.** Automatic placement for cells without explicit `(row, col)`. Plausible early — first photo-gallery / dashboard widget will need it.
- **Subgrid.** Child grids inherit parent's tracks for cross-grid alignment. Substantial; gate on a real form widget hitting the "use one big Grid" wall.
- **Aspect-ratio constraints.** New `Element.aspect_ratio: Option<f32>` for image tiles / video thumbnails. Knock-on effects on intrinsic queries.

## Layout — universal

- **`position: absolute` / `sticky`.** Escape-flow children for overlays and sticky headers. Today's workaround is a top-level ZStack overlay; replace when the first real modal widget needs it.
- **BiDi / RTL writing direction.** `Ui::set_direction(Ltr | Rtl)` with per-subtree overrides. Affects stack child order, alignment defaults, padding/margin start/end semantics, scroll direction. Significant; gates on first user request.
- **Logical properties (`margin-inline-start` etc.).** Lands with BiDi/RTL.
- **`transform-origin`.** New `Element.transform_origin: Vec2` applied during cascade. First widget with rotation/scale around a center triggers it.
