# cosmic-text — reference notes for Palantir

cosmic-text is the text engine behind COSMIC (System76's desktop), and the layer glyphon wraps for GPU rendering. It does shaping (harfrust, the rust port of HarfBuzz), font loading + fallback (fontdb + skrifa), bidi line breaking, layout into runs, an editor with cursor/selection/undo, and a swash-based glyph rasterizer cache. It is the de facto "good enough text in pure Rust" pick when parley is too heavyweight or too retained.

All paths are under `tmp/cosmic-text/src/`.

## 1. Buffer + shaping pipeline

`Buffer` (`buffer.rs:334`) owns `lines: Vec<BufferLine>` plus layout-affecting state: `metrics`, `width_opt`, `height_opt`, `scroll`, `wrap`, `ellipsize`, `monospace_width`, `tab_width`, `hinting`, plus a `DirtyFlags` bitset (`buffer.rs:22`) tracking `RELAYOUT | TAB_SHAPE | TEXT_SET | SCROLL`. Each `BufferLine` lazily holds a `shape_opt: Option<ShapeLine>` and `layout_opt: Option<Vec<LayoutLine>>`; mutating text or attrs sets `shape_opt = None`, mutating wrap/width sets `layout_opt = None`.

`Buffer::shape_until_scroll(font_system, prune)` (`buffer.rs:571`) is the work entry point: `resolve_dirty()` (`buffer.rs:426`) translates the bitset into per-line `reset_shaping`/`reset_layout` calls, then it walks lines from `scroll.line` forward, calling `line_layout` for each until either `total_height > scroll_end` or end-of-buffer. Lines outside the visible band are either skipped (`prune=false`) or eagerly evicted (`prune=true`, used for unbounded transcripts). This is **lazy and incremental**: edit one line, only that line reshapes; scroll, only newly visible lines shape.

`ShapeRunCache` (`shape_run_cache.rs:17`) sits inside `FontSystem.shape_run_cache` (`font/system.rs:163`, gated on `shape-run-cache` feature). Key is `(text: String, default_attrs, attrs_spans: Vec<(Range, AttrsOwned)>)` (`shape_run_cache.rs:9`); value is `Vec<ShapeGlyph>` plus an age. `trim(keep_ages)` is called once per frame to evict stale entries — typical for terminals/repls where the same prompt string shapes every frame. Note the cost: keys are owned `String`s, so the cache trades allocation for shaping work.

`ShapeBuffer` (`shape.rs:88`) is the hot scratch space — kept inside `FontSystem` (`font/system.rs:157`) so allocations live across calls. It holds a 6-entry FIFO `shape_plan_cache: VecDeque<(fontdb::ID, harfrust::ShapePlan)>` (`shape.rs:91`, `NUM_SHAPE_PLANS = 6` at `shape.rs:84`), a reusable `harfrust::UnicodeBuffer`, and per-script/span/word/glyph scratch vectors. The shape-plan cache is *FIFO not LRU* — a comment at `shape.rs:89-90` is explicit about this; with more than 6 distinct (font, script, language, features) tuples in flight you thrash. `shape_fallback` (`shape.rs:119`) is the actual harfrust call: builds a key, searches the FIFO linearly, falls through to `ShapePlan::new` on miss.

## 2. FontSystem — the global lock you can't avoid

`FontSystem` (`font/system.rs:132`) bundles: `fontdb::Database`, `font_cache: HashMap<(ID, Weight), Option<Arc<Font>>>` (`:140`), per-script monospace ID indices, a per-font codepoint-support cache (`FontCachedCodepointSupportInfo`, `:72`, with bounded-size sorted vectors of supported/not-supported codepoints — 512/1024 caps at `:78-79`), `font_matches_cache: HashMap<FontMatchAttrs, Arc<Vec<FontMatchKey>>>` capped at 256 (`:154`, `:183`), the `ShapeBuffer` scratch, a `BTreeSet` for fallback iteration, and (optionally) the `ShapeRunCache`.

Construction is **slow**: `FontSystem::new` (`:191`) calls `db.load_system_fonts()` (`:438`); the docstring at `:186-190` warns "up to a second on release, ten times longer on debug, call once and share." Every method that does real work takes `&mut self` — `get_font` (`:302`), `get_font_matches` (`:359`), `get_font_supported_codepoints_in_word` (`:341`). All shaping is implicitly `&mut FontSystem`.

**The Arc<Mutex<FontSystem>> pitfall.** Because every shape/measure/render needs `&mut FontSystem`, any app sharing it across widgets/threads ends up with `Arc<Mutex<FontSystem>>` and serializes through one giant lock. The `BorrowedWithFontSystem<'a, T>` helper (`font/system.rs:462`) bundles `&mut FontSystem` with `&mut T` so call sites read like methods — but the borrow is exclusive for the whole call chain. iced and egui both wrestle with this; iced keeps a single FontSystem on the renderer, egui hides it inside `epaint::Fonts`. There is no fine-grained interior mutability — the caches are plain `HashMap`s.

The `db_mut` accessor (`:291`) clears `font_matches_cache` on every call, so any "load a font at runtime" path nukes match results across all attrs.

## 3. Attrs — per-range font/color/style

`Attrs` (`attrs.rs`, `pub use fontdb::{Family, Stretch, Style, Weight}` at `:12`) is a borrow-friendly struct of `family: Family<'a>` plus weight/stretch/style/color/letter-spacing/feature-tags/cache-flags/metadata. `AttrsOwned` is the heap version. `AttrsList` (`:496`) is `{ defaults: AttrsOwned, spans: RangeMap<usize, AttrsOwned> }` — a `rangemap::RangeMap` keyed on byte offsets in the line. `add_span(range, attrs)` (`:531`) inserts and the rangemap auto-merges/splits adjacent equal ranges. `get_span(index)` (`:543`) returns the override or `defaults`. The `//TODO: have this clean up the spans when changes are made` at `:494` is honest: edits don't shift spans.

`set_rich_text` (`buffer.rs:1102`) is the rich-text entry: takes `I: IntoIterator<Item = (&'r str, Attrs<'r>)>` plus an optional `Vec<(Range, Attrs)>` of overrides per chunk and builds the `BufferLine` set.

`CacheKeyFlags` (`attrs.rs` re-export, real def in `glyph_cache.rs:7`) is per-glyph and travels through shaping into `ShapeGlyph.cache_key_flags` (`shape.rs:238`): `FAKE_ITALIC | DISABLE_HINTING | PIXEL_FONT`. Color is also carried per-glyph via `color_opt`, so a single shape run can paint multiple colors without re-shaping.

## 4. Edit support

`Editor<'buffer>` (`edit/editor.rs:20`) is `{ buffer_ref: BufferRef, cursor, cursor_x_opt, selection: Selection, cursor_moved, auto_indent, change: Option<Change> }`. `BufferRef` (`edit/mod.rs:68`) is a three-way `Owned(Buffer) | Borrowed(&mut Buffer) | Arc<Buffer>` so editors can be cheaply constructed against shared buffers; `Arc::make_mut` is invoked on write (`edit/mod.rs:189`) for COW.

`Action` (`edit/mod.rs:23`) is the discrete input enum: `Motion(Motion) | Insert(char) | Backspace | Delete | Click{x,y} | Drag{x,y} | Scroll{pixels} | …`. The editor is event-driven — there is no "current frame's input"; you push `Action`s, the editor mutates the buffer, the buffer's dirty flags pick up the work next `shape_until_scroll`.

Undo lives in the **`vi` extension**, not the core editor. `ViEditor` (`edit/vi.rs:187`) wraps an `Editor` plus a `cosmic_undo_2::Commands<Change>` log. `Change` (`edit/mod.rs:124`) is `Vec<ChangeItem { start, end, text, insert }>`; `ChangeItem::reverse` flips `insert` (`edit/mod.rs:117`). `undo()`/`redo()` (`edit/vi.rs:288-300`) walk the log and replay items. The base `Editor` records pending `change: Option<Change>` but the storage and pivot tracking ("changed since save") is the vi layer's responsibility (`eval_changed` at `edit/vi.rs:50`). For Palantir's purposes: building a non-vi editor means owning the `Change` log yourself.

Selection (`edit/mod.rs:140`) has `None | Normal | Line | Word`; bounds and highlight rectangles are computed lazily via `LayoutRun::highlight` (`buffer.rs:66`), which yields `(x_left, width)` spans that are pre-cut for bidi mixed runs.

## 5. Swash glyph cache

`SwashCache` (`swash.rs:132`) is a separate user-owned struct, **not** part of `FontSystem`. It holds a `swash::scale::ScaleContext`, `image_cache: HashMap<CacheKey, Option<SwashImage>>`, and `outline_command_cache: HashMap<CacheKey, Option<Box<[Command]>>>`. `get_image(font_system, cache_key)` (`swash.rs:164`) is the workhorse — `entry().or_insert_with(|| swash_image(...))`.

`CacheKey` (`glyph_cache.rs:19`) is `{ font_id, glyph_id, font_size_bits, x_bin, y_bin, font_weight, flags }`. `SubpixelBin` (`:65`) is a quarter-pixel bin (`Zero | One | Two | Three`); `SubpixelBin::new` (`:73`) returns `(integer_pos, bin)`. This is the standard "quantize subpixel position" trick — at most 4 bins per axis = 16 cache entries per (font, size, glyph) instead of unbounded float keys. The `PIXEL_FONT` flag (`:13`) rounds to integer positions for crisp pixel fonts.

Critically, `SwashCache` operates per-glyph at *render* time, given a `LayoutGlyph`. cosmic-text itself does not assume an atlas — `SwashImage` is a CPU bitmap (`SwashContent::Mask` or `Color`). glyphon is the layer that takes these images and packs them into a wgpu atlas via etagere. If you write your own renderer you can skip glyphon entirely and consume `SwashImage` directly (which `examples/editor-libcosmic` and the `LegacyRenderer` callback at `edit/editor.rs:64` demonstrate).

The `Font::id()` is `fontdb::ID` (an interned u32 inside fontdb), so `CacheKey` is `Hash + Eq` and small (~24 bytes). The test at `swash.rs:257-303` is interesting context: it documents that `swash::ScaleContext` leaks variation-axis state across builds when you use the `variations()` builder — they switched to `normalized_coords()` to avoid the bug. If you use swash directly, follow this pattern.

## 6. Where it's slower / faster than parley

Faster:
- **Incremental updates.** Per-line `shape_opt` invalidation + `shape_until_scroll`'s visible-only loop means a 1MB log file with one line edited reshapes only that line. parley's `Layout` is built fresh from the `LayoutContext` each rebuild; incremental editing is the user's job.
- **No tree.** cosmic-text's output is a flat `Vec<LayoutRun>`. Hit-testing and cursor math walks the vec linearly; for terminals/editors this is exactly what you want.
- **`ShapeRunCache` for repeated identical strings.** Terminal prompts, repeated UI labels — keyed by full text, hits cheaply.

Slower:
- **No bidi-correct paragraph cache.** `BidiParagraphs` (`bidi_para.rs`) re-runs unicode-bidi every shape; parley caches the bidi resolution in `Layout`.
- **Single-threaded.** Every public method is `&mut FontSystem`; no rayon, no parallel paragraph shaping. parley separates `LayoutContext` (mutable scratch) from `Layout` (immutable result) and builds them more amenably to fanout.
- **FIFO shape-plan cache size 6** (`shape.rs:84`). A page mixing Latin + Arabic + CJK + emoji + a code-font monospace + an italic = 6 plans, and any sixth entry evicts the oldest. parley uses a larger LRU.
- **Font-matches cache eviction is a full clear** (`font/system.rs:362`) when over 256 entries — not LRU. Worst case under heavy attrs churn: every new attrs combination clears the lot.
- **Allocation-heavy keys.** `ShapeRunKey` owns `String` and `Vec<(Range, AttrsOwned)>`; per-glyph `LayoutGlyph` is fat. parley aims tighter packing.

For a relatively static UI with hundreds of small labels, parley's batch model wins; for terminals, code editors, chat logs, and anywhere with edit locality, cosmic-text wins.

## 7. Lessons for Palantir

**Use cosmic-text for shaping; do not write our own.** harfrust + fontdb + bidi-para + line breaking is months of work and we get it for one dependency. The licensing (MIT/Apache) is fine.

**Own a single `FontSystem` on the `Ui`/recorder, not in widget state.** Pass `&mut FontSystem` into the measure pass — measure is already `&mut self` on the tree, this composes. Do not reach for `Arc<Mutex<FontSystem>>` until we genuinely have parallel measure (which Palantir does not, and probably should not). The lock contention horror stories (egui's `Fonts`, iced's `font::Storage`) all stem from "I want to measure text from anywhere" — Palantir's measure pass has a single owner and a single call site, so no Mutex needed.

**Cache `Buffer`s in the persistent state map, not on the tree.** A `Text` widget should look up `Buffer` by `WidgetId` from the `Id → Any` map, mutate it during measure (`set_text` if changed, `set_size` from available, `shape_until_scroll`), read `layout_runs` for the measured size, and emit a `Shape::Text { buffer_handle, color }` referring to the same handle. Rebuilding the `Buffer` every frame defeats `BufferLine.shape_opt` caching. This matches how `egui::Galley` and iced's `Paragraph` are reused.

**Defer rasterization to the paint pass, and keep `SwashCache` on the renderer.** Measure only needs `LayoutRun.line_w` / glyph advances, not images. The renderer module owns `SwashCache` and the wgpu glyph atlas; emit `(font_id, glyph_id, x, y, color)` triples, atlas-resolves at draw time. Same separation glyphon already lives by — good signal.

**Mirror the dirty-flag pattern for our own redraw scheduling.** `Buffer.dirty: DirtyFlags` (`buffer.rs:22`) with `RELAYOUT | TAB_SHAPE | TEXT_SET | SCROLL` cleanly distinguishes "needs reshape" from "needs relayout" from "scrolled, just walk further." Palantir's redraw decision (next frame needed?) can use the same shape: `WidgetState` carries flags that the `Ui` ORs together at end-of-frame.

**Skip the `vi` undo crate.** `cosmic_undo_2` is fine but tied to `Change`; if/when Palantir grows a TextField, write a small undo log (vec of inverse ops + cursor pivot) and don't pull a third-party crate for it.

**Watch out for:**
- The shape-plan FIFO size of 6. If a UI mixes more than ~5 distinct fonts/scripts on screen we need to bump `NUM_SHAPE_PLANS` (it's `pub`-able with a fork or upstream PR).
- `font_matches_cache` clear-on-overflow rather than LRU. For a hundred-button UI with varying weights this is fine; if it ever bites, replace with `lru` crate.
- `FontSystem::new()` blocking the main thread for ~1s on first boot. Construct off-thread before showing the window, or ship a bundled-fonts subset and use `new_with_fonts` (`font/system.rs:196`).
- `BufferRef::Arc` exists, but `Arc::make_mut` clones the whole `Buffer` on first edit. If we ever share buffers across widgets, prefer `Borrowed` against the persistent-state map.
