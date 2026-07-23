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
measurement and optionally a bounded one. The layout retains measurements and
`TextCacheKey`s, while each rendered run also carries a compact span to its
source bytes in the window's retained `RecordStore`. The backend resolves those
bytes only for encoded-cache misses; `TextShaper::with_render_buffers` restores
all requested shaped buffers and lends the render split under one exclusive
borrow.

## High: split ownership leaves cache correctness to callers

- [x] **Reuse validity was an external promise rather than an invariant of the cache key.** `prepare_run` now accepts only the run identity, actual text, and `ShapeParams`. `TextSystem` computes the text hash itself and compares the normalized, validated shaping parameters as part of each reuse row's inputs, so callers cannot retain stale measurements by supplying an unrelated authoring or text hash.

- [x] **No API owned the complete identity-to-render-buffer transaction.** `TextSystem` owns window-local identity rows and their lifecycle together with a clone of the shared `TextShaper`. Render runs retain their record-local source span, and the backend hands all encoded-cache misses to `TextShaper::with_render_buffers`; that one method restores the requested buffers and exposes them to the renderer without an intermediate availability promise. The frontend no longer knows about reconstruction, and encoded-cache hits avoid the shaper borrow entirely.

- [ ] **Reuse rows for vanished text ordinals remain indefinitely while their widget survives.** Rows are keyed by `(WidgetId, u16)`, but maintenance removes entries only when the whole `WidgetId` appears in the removed-widget set (`src/text/mod.rs:145`, `src/text/mod.rs:453`). An immediate-mode widget whose number of direct text shapes falls from many runs to a few leaves every higher ordinal resident; repeated peaks on stable widget IDs retain the maximum historical row count rather than the current text-run set, wasting memory for the lifetime of those widgets.

## Medium: reuse granularity and public surface add avoidable cost

- [x] **A node-wide paint/layout hash invalidated shaping reuse for changes that cannot affect shaping.** The identity cache no longer accepts the node rollup hash. Its validity derives only from text content and shaping parameters, so paint, chrome, child, and position changes preserve the row.

- [ ] **The public construction API exposes `CosmicMeasure` as a second text-system concept without providing an independent capability.** The crate re-exports both `CosmicMeasure` and `TextShaper` (`src/lib.rs:122`); `CosmicMeasure` has private state and its only public constructor loads the same bundled fonts already exposed by `TextShaper::with_bundled_fonts`, while `TextShaper::with_cosmic` is the only public bridge between them (`src/text/cosmic.rs:215`, `src/text/cosmic.rs:241`, `src/text/mod.rs:321`). Consumers must understand an implementation-layer type to express no configuration that the primary type cannot already express, and the supported surface becomes coupled to cosmic-text's role in the internals.

- [ ] **Test-only dispatch accounting mutates production state on every shaping path.** `ShaperInner` unconditionally stores `measure_calls` specifically for cache-effectiveness tests, and public measurement, probing, identity misses, and bounded misses all increment it (`src/text/mod.rs:179`, `src/text/mod.rs:345`, `src/text/mod.rs:379`, `src/text/mod.rs:416`, `src/text/mod.rs:490`). Production frames therefore pay an extra shared mutable-state write in the hot text path solely for test observability, and the counter further entangles `TextSystem` with `TextShaper` internals.

## Resolved ownership constraint

- [ ] **A clone-shared identity map would make independent windows overwrite and evict one another's reuse rows.** Each `WindowDriver` owns a distinct `Ui`, but all are built from the same clone-shared resources (`src/host/window_driver.rs:35`, `src/host/window_driver.rs:219`). Widget identity has no window namespace: every `Ui` opens the same hard-coded viewport root, and automatic IDs hash only the authoring call site plus parent identity (`src/ui/mod.rs:350`, `src/primitives/widget_id.rs:65`, `src/ui/mod.rs:873`). Consequently, two windows recording the same widget code routinely produce the same `(WidgetId, ordinal)` key. A shared reuse map would make different content hashes or wrap widths replace the same row, and either window's removed-widget sweep could delete the other's live row (`src/text/mod.rs:433`, `src/text/mod.rs:453`). Adding `WindowToken` to the key would merely recreate per-window partitioning inside global storage and would also require explicit cleanup when closing a window currently drops its whole `WindowDriver` (`src/host/winit/mod.rs:464`). The viable meaning of one entity is therefore a window-local coordinator that owns identity reuse while referring to app-global shaping resources; the content-keyed cosmic buffers remain safely shareable with the one backend (`src/host/shared.rs:34`, `src/renderer/backend/mod.rs:121`).
