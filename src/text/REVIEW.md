# Text shaping and reuse review

The content cache and identity cache have legitimately different lifetimes:
shaped cosmic-text buffers are app-global, while `(WidgetId, ordinal)` reuse
must remain window-local. The current code nevertheless exposes that split as
a coordination protocol across layout, UI finalization, the frontend, and the
backend. A single conceptual owner is warranted at the API boundary; collapsing
both lifetimes into the clone-shared cache would lose the existing multi-window
isolation.

Current flow: `LayoutEngine` consults its window-local `TextSystem`, which owns
identity reuse and reaches into a shared `TextShaper` to obtain an unbounded
measurement and optionally a bounded one. The layout retains only measurements
and `TextCacheKey`s. Before rendering, the frontend asks `TextShaper` to
reconstruct an evicted buffer, then the backend borrows the same shaper again and
assumes that buffer is present.

## High: split ownership leaves cache correctness to callers

- [ ] **Reuse validity is an external promise rather than an invariant of the cache key.** `prepare_run` accepts `authoring_hash`, `text`, `text_hash`, and `ShapeParams` independently, but an occupied row is validated only by `authoring_hash`; the other inputs are ignored on a hit (`src/text/mod.rs:396`, `src/text/mod.rs:433`). Both layout call sites must therefore keep independently obtained node and text hashes synchronized with the raw shaping inputs (`src/layout/engine.rs:810`, `src/layout/engine.rs:822`, `src/layout/intrinsic/mod.rs:279`). If a caller ever supplies a hash that does not change with one of those inputs, the old measurement and key survive; the frontend then pairs that retained key with the current string, while `ensure_buffer` accepts an existing buffer without verifying its source (`src/renderer/frontend/encoder/mod.rs:345`, `src/text/cosmic.rs:533`). That failure mode can render stale glyphs or associate new glyphs with an old content key.

- [ ] **No API owns the complete identity-to-render-buffer transaction.** `TextSystem` now owns window-local identity rows and their lifecycle together with a clone of the shared `TextShaper`, but layout still retains only measurements and buffer keys. The renderer frontend later repairs buffer availability through its separate `TextShaper` handle, and the backend treats that repair as an invariant and panics when it was missed (`src/renderer/frontend/encoder/mod.rs:339`, `src/renderer/backend/text/mod.rs:302`). Changes to buffer reconstruction or eviction therefore still require synchronized knowledge across the text system and both renderer stages.

- [ ] **Reuse rows for vanished text ordinals remain indefinitely while their widget survives.** Rows are keyed by `(WidgetId, u16)`, but maintenance removes entries only when the whole `WidgetId` appears in the removed-widget set (`src/text/mod.rs:145`, `src/text/mod.rs:453`). An immediate-mode widget whose number of direct text shapes falls from many runs to a few leaves every higher ordinal resident; repeated peaks on stable widget IDs retain the maximum historical row count rather than the current text-run set, wasting memory for the lifetime of those widgets.

## Medium: reuse granularity and public surface add avoidable cost

- [ ] **A node-wide paint/layout hash invalidates shaping reuse for changes that cannot affect shaping.** Both layout paths use `tree.rollups.node[node]` as every text run's `authoring_hash` (`src/layout/engine.rs:810`, `src/layout/intrinsic/mod.rs:279`), while that hash includes node layout, bounds, panel state, chrome, child identities, and every direct shape (`src/scene/tree/mod.rs:191`, `src/scene/tree/mod.rs:215`, `src/scene/tree/mod.rs:250`). Even a text shape's color and local paint origin participate (`src/scene/shapes/hash.rs:52`). Consequently, a color animation, chrome change, unrelated sibling shape, or paint-position change refreshes all reuse rows on that node and re-enters shaping dispatch despite identical text metrics and glyph content.

- [ ] **The public construction API exposes `CosmicMeasure` as a second text-system concept without providing an independent capability.** The crate re-exports both `CosmicMeasure` and `TextShaper` (`src/lib.rs:122`); `CosmicMeasure` has private state and its only public constructor loads the same bundled fonts already exposed by `TextShaper::with_bundled_fonts`, while `TextShaper::with_cosmic` is the only public bridge between them (`src/text/cosmic.rs:215`, `src/text/cosmic.rs:241`, `src/text/mod.rs:321`). Consumers must understand an implementation-layer type to express no configuration that the primary type cannot already express, and the supported surface becomes coupled to cosmic-text's role in the internals.

- [ ] **Test-only dispatch accounting mutates production state on every shaping path.** `ShaperInner` unconditionally stores `measure_calls` specifically for cache-effectiveness tests, and public measurement, probing, identity misses, and bounded misses all increment it (`src/text/mod.rs:179`, `src/text/mod.rs:345`, `src/text/mod.rs:379`, `src/text/mod.rs:416`, `src/text/mod.rs:490`). Production frames therefore pay an extra shared mutable-state write in the hot text path solely for test observability, and the counter further entangles `TextSystem` with `TextShaper` internals.

## Resolved ownership constraint

- [ ] **A clone-shared identity map would make independent windows overwrite and evict one another's reuse rows.** Each `WindowDriver` owns a distinct `Ui`, but all are built from the same clone-shared resources (`src/host/window_driver.rs:35`, `src/host/window_driver.rs:219`). Widget identity has no window namespace: every `Ui` opens the same hard-coded viewport root, and automatic IDs hash only the authoring call site plus parent identity (`src/ui/mod.rs:350`, `src/primitives/widget_id.rs:65`, `src/ui/mod.rs:873`). Consequently, two windows recording the same widget code routinely produce the same `(WidgetId, ordinal)` key. A shared reuse map would make different content hashes or wrap widths replace the same row, and either window's removed-widget sweep could delete the other's live row (`src/text/mod.rs:433`, `src/text/mod.rs:453`). Adding `WindowToken` to the key would merely recreate per-window partitioning inside global storage and would also require explicit cleanup when closing a window currently drops its whole `WindowDriver` (`src/host/winit/mod.rs:464`). The viable meaning of one entity is therefore a window-local coordinator that owns identity reuse while referring to app-global shaping resources; the content-keyed cosmic buffers remain safely shareable with the one backend (`src/host/shared.rs:34`, `src/renderer/backend/mod.rs:121`).
