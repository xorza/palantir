# parley — reference notes for Palantir

Parley is the Linebender text-layout crate: real Unicode text layout (BiDi, line breaking, font fallback, justification) sitting above `harfrust` (Rust port of HarfBuzz) for shaping and `skrifa`/`fontique` for font handling, and below `vello`/`swash`-style renderers for painting. It is the most credible Rust answer to "real" text layout, and the design Linebender expects Xilem and Vello to converge on.

All paths are under `tmp/parley/`.

## 1. Two-stage architecture: shape, then layout

Parley splits text processing in two:

1. **Build phase** (`builder.rs:249-316`, `build_into_layout`). Runs once per text+style change. Bidi resolution (`bidi.rs`), grapheme/word/line-break analysis via `icu` (`analysis/mod.rs`), font selection per character cluster, and shaping via `harfrust` (`shape/mod.rs:64-`). Output is a `Layout<B>` containing flat arrays of `RunData`/`ClusterData`/`Glyph` keyed by byte ranges. `harfrust` shapes once; the result is cached in `LayoutContext::scx` (`shape/mod.rs:28-49`, three `LruCache`s for `ShaperData`, `ShaperInstance`, `ShapePlan`).
2. **Layout phase** (`layout/line_break.rs`, `layout/alignment.rs`). Runs every time the available width changes. `Layout::break_all_lines(max_advance)` (`layout/layout.rs:161`) does greedy line breaking over the already-shaped clusters, then `Layout::align(alignment, opts)` (line 177) sets per-line offsets. **Re-line-breaking does not re-shape** — that's the design payoff.

The split matters because shaping is the expensive part (font lookups, OpenType feature application, harfrust state machines). A `PlainEditor` resizing its text box re-runs only line-break + align; only edits invalidate the shape cache.

`LayoutContext` (`context.rs:23`) is the long-lived scratch space: bidi resolver, char-info buffer, style table, shape caches. `FontContext` (`font.rs`) holds the `fontique::Collection` font database. Both are intended to be app-globals; the docstrings explicitly say "constructed rarely (perhaps even once per app)" (`lib.rs:35`).

## 2. RangedBuilder, TreeBuilder, StyleRunBuilder

Three input shapes, all producing the same `Layout`:

- **`RangedBuilder`** (`builder.rs:21`). Flat `Vec` of `(StyleProperty, Range<usize>)`. `push_default(prop)` sets a base; `push(prop, range)` applies an override; later pushes win on overlap. Internally a `RangedStyleBuilder` (`resolve/range.rs`) splits overlapping ranges into a non-overlapping `Vec<StyleRun>` at `finish()` time. This is the CSS-spans-on-text model.
- **`TreeBuilder`** (`builder.rs:169`). Stack-based: `push_style_span`/`push_style_modification_span`/`pop_style_span` with `push_text` between them. The HTML/inline-element model. `push_style_modification_span` only changes specific properties, leaving the rest inherited. `set_white_space_mode` is per-span (`builder.rs:221`). The builder accumulates a `String` as you push text and returns it from `build()`.
- **`StyleRunBuilder`** (`builder.rs:80`). Lower-level: caller supplies a deduplicated style table via `push_style` (returns `u16` index) and contiguous non-overlapping `push_style_run(idx, range)` covering the full text. Skips parley's range-splitting. For callers (rich-text editors, syntax highlighters) that already track styled runs themselves.

All three share the same `build_into_layout` (`builder.rs:249`): finish styles → `analyze_text` → `shape_text` → swap inline boxes in → `data.finish()`. The builder choice is purely an authoring-side convenience.

`InlineBox` (`inline_box.rs`) lets the caller embed a non-text rectangle (image, button, replaced element) at a byte offset; it participates in line breaking and gets baseline-aligned. `kind` is `InFlow` or `OutOfFlow` for IME and decorations.

## 3. Layout result: Line / Run / Cluster / Glyph hierarchy

The `Layout<B>` (`layout/layout.rs:25`) holds `LayoutData<B>` with parallel flat vectors — there is no nested ownership tree. Iteration produces lightweight borrowing wrappers:

- **`Layout::lines()`** → `Line<'a, B>` (`layout/line.rs:16`). Each `Line` carries `metrics: LineMetrics`, `text_range`, and an `item_range` into `data.line_items`. Items are either `TextRun` or `InlineBox`.
- **`Line::runs()`** → `Run<'a, B>` (`layout/run.rs:14`). A run is a maximal span with the same font, size, script, BiDi level, and style — i.e. one `harfrust::shape` invocation. `RunMetrics` (`layout/run.rs:224`) has ascent/descent/line-gap/underline-offset.
- **`Run::clusters()`** → `Cluster<'a, B>` (`layout/cluster.rs:17`). A cluster is one user-perceived character (one `char` in simple scripts, several for ligatures or combining marks). `ClusterData` (`layout/data.rs:17-56`) packs glyph count + glyph offset (with the `0xFF` sentinel meaning "single glyph stored inline as a glyph id" — niche optimization), text length, advance width, ligature flags. `Cluster` is the right granularity for hit-testing and cursor placement.
- **`Cluster::glyphs()`** → `Glyph` (`layout/glyph.rs:6`). The render-time atom: glyph id, x/y advance, x/y offset, style index.

For painting, the convenient iterator is `Line::items()` → `PositionedLayoutItem::{GlyphRun, InlineBox}` (`layout/line.rs:189` for `GlyphRun`). A `GlyphRun` is a `Run` already positioned at a baseline x/y, ready to feed into a glyph atlas. The lib.rs example (`lib.rs:63-74`) is the canonical paint loop.

`break_all_lines` is the convenience entry point; the lower-level `break_lines() -> BreakLines<'_, B>` (`layout/layout.rs:155`, `line_break.rs`) yields per-line via a stateful iterator and supports `set_line_max_height` for "stop here, the caller will reposition" — useful for flowing around floats. `YieldData` (`line_break.rs:68`) is `LineBreak | MaxHeightExceeded | BoxBreak`.

## 4. Editor support: PlainEditor, Cursor, Selection

Parley ships `editing::PlainEditor<T>` (`editing/editor.rs:92`) — a "single style across the buffer" editor that owns the `Layout`, a `String`, an active `Selection`, and IME compose state. It's small (1286 LOC) and structured as `PlainEditor` (state) + `PlainEditorDriver<'a, T>` (`editor.rs:158`, returned from `editor.driver(font_cx, layout_cx)`) which holds the contexts for the duration of a mutation. This split lets contexts stay app-global without lifetime contagion on the editor field.

The driver API is a flat list of editing verbs: `move_left/right/up/down`, `move_word_left/right`, `move_to_text_start/end`, `move_to_hard_line_start/end` (paragraph) vs `move_to_line_start/end` (visual), `select_*` mirroring all the move ops, `delete/backdelete`, `delete_word/backdelete_word`, `insert_or_replace_selection`, `select_word_at_point(x, y)`, `extend_selection_to_point(x, y)` for drag, `set_compose`/`finish_compose`/`clear_compose` for IME. That's the full keyboard contract — copy it.

`Cursor` (`editing/cursor.rs:16`) = byte index + `Affinity` (which side of a line break the cursor sits on). `Selection` (`editing/selection.rs:16`) = anchor `Cursor` + focus `Cursor` + horizontal-affinity hint for up/down preservation. `Selection::geometry(layout)` (`selection.rs:497`) returns `Vec<(BoundingBox, line_idx)>` rectangles ready to paint as the highlight; `geometry_with` (line 509) is the streaming variant. Cursor caret geometry comes from `Cursor::geometry(layout, size)` (referenced at `selection.rs:182`).

Hit-testing is `Cursor::from_point(layout, x, y)` (used in `move_to_point`, `editor.rs:433`). Vertical movement uses a remembered horizontal anchor (`selection.rs:296`) so up-then-down returns to the same column even across short lines.

`Layout` is invalidated on edit (`layout_dirty: bool`, `editor.rs:116`); `refresh_layout` rebuilds it. Generation counter (`Generation`, `editor.rs:33`) lets renderers skip when nothing changed.

## 5. Font fallback: fontique

`fontique` is parley's font crate. `Collection` enumerates installed fonts (per-OS scanners in `fontique/src/backend/`); `Query` (`fontique/src/lib.rs`) does the matching. Fallback is keyed by `(Script, Option<Language>)` → `Vec<FamilyId>` in `FallbackMap` (`fontique/src/fallback.rs:14`). The `canonical_locale` table (`fallback.rs:200-292`) is hand-curated CLDR-derived data: for `Hani` it splits `ja`/`ko`/`zh-CN`/`zh-TW`/`zh-HK`/`zh-MO`/`zh-SG` and uses `locale.script() == "Hant"` to default unspecified Chinese to Traditional vs Simplified; for `Arab` it tracks `ar`, `fa`, `ur`, `ps-AF`/`ps-PK`, etc. — same shape as fontconfig's fallback policy but readable in one file.

The shaping loop (`shape/mod.rs:120-` onward) walks character clusters, queries fonts in priority order, and starts a new run when font changes. `CharCluster::status` returns `Complete`/`Discard`/`Keep` for whether a font covers all chars in the cluster — the standard "use first font that covers this grapheme" policy.

`source_cache.prune(128, false)` (`context.rs:100`) is called on every builder construction to bound the loaded-font-data cache. Fonts aren't kept resident; the cache is LRU.

## 6. Lessons for Palantir

**Why parley is the right ceiling target, not the v1 target.**

Parley solves problems Palantir doesn't have yet: Latin-script left-to-right Button labels and Text widgets need none of bidi, complex shaping, or script fallback. The cost is real — `harfrust` + `icu` + `fontique` + `skrifa` is on the order of a megabyte of dependency plus first-frame analysis cost on every text change. For a Hello-World button this is wildly overspecified.

**Three options for v1.**

1. **`glyphon`** (`tmp/glyphon`). Wraps `cosmic-text` and ships an `etagere`-backed wgpu glyph atlas plus a textured-quad pipeline. Closest to "drop in and render text" for wgpu. Cost: pulls cosmic-text's full text engine (still smaller than parley) and assumes you've already produced a `Buffer` via cosmic-text. Right answer if we want shipping text in one weekend.
2. **`cosmic-text`** (`tmp/cosmic-text`). Pop-OS's text engine: shaping via `rustybuzz` (the predecessor to `harfrust`), buffer-based API (`Buffer::set_text`), fontdb font lookup, swash for rasterization. Slightly older lineage than parley; more battle-tested in COSMIC desktop. API is more buffer-shaped than range-builder-shaped — fits an immediate-mode "give me a layout, paint it" workflow well.
3. **Hand-rolled with `swash` or `skrifa`+`harfrust`**. Fastest at runtime, smallest dependency, but we'd be writing the BiDi/line-break/fallback layer ourselves. Not worth it; that's parley's whole job.

**Recommendation.** v1 = glyphon (or cosmic-text + a small wgpu atlas, if glyphon's API constraints bite). `Shape::Text` already exists in `src/shape.rs`; the paint pass is what consumes it, and that's where the choice is made — `tree.rs`/`layout.rs`/`ui.rs` need no changes. Move to parley when we want one of: rich text spans (different sizes/weights mid-line), proper line-breaking inside layout containers, RTL/CJK input methods, accessibility tree integration (parley has accesskit support behind a feature, `layout/accessibility.rs`).

**Concrete things to copy regardless of which engine wins.**

- **Two-stage API split**: shape once, re-line-break on width change. Even with glyphon we should structure our `Text` widget so that resizing a panel doesn't re-shape; cache the shape result keyed by `(text, style, scale)` and only re-run line-breaking on width change. This maps onto Palantir's `Measure` returning a "shape handle"-sized cluster list, then `Arrange` calling `break_to_width(final_rect.w)`.
- **Range-attributed input**: when we eventually do rich text, `RangedBuilder`'s "default + override spans" API is right. The TreeBuilder model is more natural for HTML-like input but worse for code-style "highlight bytes 12..18 red" use cases. Palantir is closer to the latter.
- **Cluster-granularity hit-testing**: don't try to hit-test glyphs (ligatures break this). The `Cluster` type with `text_range` is the right unit for cursor positioning.
- **PlainEditor's driver split**: editor state lives on the widget; contexts (font db, scratch space) live on the app `Ui`. `editor.driver(&mut fcx, &mut lcx)` is the borrow-the-world-for-this-mutation idiom. Worth mirroring when Palantir gets a TextEdit widget.
- **Selection geometry as `Vec<Rect>`**: the multi-rect-per-line model handles wrapped selections naturally and aligns with our `ShapeRect::Offset` painting.

**What to skip.**

- The `LruCache` triple-cache around harfrust state. Glyphon already does this internally. Parley exposes it because parley is the engine; we'd be a layer above.
- The fontique `canonical_locale` table. fontdb (cosmic-text) and the OS-default fallback are good enough until we ship CJK input. Don't hand-port 200 lines of locale rules for a prototype.
- TreeBuilder, StyleRunBuilder. Just RangedBuilder when the time comes.
- AccessKit integration until winit + a11y are wired through the rest of Palantir. Parley's `LayoutAccessibility` (`layout/accessibility.rs`, gated behind `accesskit` feature) is the model to copy when that lands.

**Single biggest takeaway.** Parley's shape-once / layout-many split is the structural lesson. Whatever engine v1 uses, the Palantir `Text` widget should be designed so that an `HStack` resizing its children re-runs line breaking only — never re-shapes. That decision is made in the widget's measure/arrange impl, not in the text engine, so it's free to get right now even before the engine choice settles.
