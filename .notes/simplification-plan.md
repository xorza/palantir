# Simplification plan

Close read of `palantir/src` looking for overengineering, complexity that
isn't paying for itself, and data structures that want consolidating.
Findings are grouped into **batches sized for one sitting** and sorted by
priority — each batch is internally coherent, and later batches don't
block on earlier ones except where noted.

Overlaps with `.notes/review-palantir.md` (same codebase, different
lens); where an item appears in both, this file carries the concrete
move rather than the observation. **Delete an item when it's done.**

## What is *not* a finding

Worth stating, because it shapes what's below. Most of the sharing this
crate could plausibly want has already been extracted and is holding up:
`PerLayer`, `OverlayScope`, `toggle_row`, `widgets::chrome`,
`pipeline_utils` + `DynamicBuffer`, `shape::Lower` (no `Shape` enum),
`TreeItems` as the single interleave cursor, `shadow_paint_rect_local` /
`align_in_rect` / `stroked_bbox` as single formula sources. The
micro-optimizations (`Gaps` NaN-tagged f16 pair, `Index16` niche,
`CascadeInputHash` sign-bit pack, `F32Ext::fast_round`) are each small,
local, and load-bearing — they are not on this list.

**The per-event scans are measured, not indexed.** `Cascade::hits_under`
reverse-scans the hit rows on every pointer event, and the obvious move
is a spatial index — `composer/text_grid/` already has a tiled AABB grid
for the same query shape. The numbers say no. The real `FrameFixture`
produces **314 hit rows**, the scan costs **0.46 ns/row**, so a full
miss is ~150 ns against a 2.3 ms frame. `cascade/hit_test` measures that
curve now — a flat `topmost` early-exit against a linear `miss` — so a
regression in either row count or per-row cost shows up. It could not
before: every row carried the same full-screen rect, so the scan matched
on its first test and the group read 1-2 ns at every density while
measuring nothing. Revisit if a tree ever pushes hit rows into the
thousands; at 8k the miss is 3.7 µs.

`InputState::target_scroll_delta`'s linear scan is the same verdict for
a smaller reason: `frame_target_deltas` holds one row per *scrolled*
widget, so it is empty on every frame that isn't scrolling, and `find`
over an empty `Vec` is free.

The complexity that *is* on this list is structural: layers that exist
for one consumer, parallel encodings the compiler already has, and code
that ships in the library because nothing moved it out.

---

# 1. The test/bench/demo estate — mostly settled

**The original premise here was wrong and is recorded so it isn't
retried.** "Move the 6,483 lines of `src/**/bench.rs` into `benches/`
and let them reach in through `internals`" does not survive contact with
the drivers: they touch `Composer`, `RenderBuffer`, `Frontend`,
`CascadeEngine`, `DamageEngine`, `TextBackend`, `GpuCtx`, `Queue`,
`RecordStore`, `RecordPayloads`, `PaintSink`, the `Draw*Payload` types,
`Quad`, `URect`, `TextShaper`, `text::system`, and
`schedule::internals::Walk`. Publishing that would make the test-facing
surface *larger* than `pub mod bench` is today. The drivers stay
colocated, `pub mod bench` stays, and the `bench` feature stays — it is
a non-default, documented-unsupported flag with `docs.rs`
`all-features = false`, which is the same deal `internals` gets.

The same argument sinks moving `frame_fixture` / `demo_swatches` into
the showcase binary: the benches record that workload too, and they live
in `src`.

What that leaves is the target count, which is done: **21 bench targets
→ 2**. `criterion` holds every timing driver; `alloc` holds the dhat
one. Only that split is forced, by `dhat::Alloc` having to be *the*
global allocator — the earlier reading, that each dhat workload needed
its own binary, was wrong, and they are steps in one bench now.
`rustdoc::private_intra_doc_links` is denied with zero warnings, and
`dump_theme` has its `[[example]]` block.

Nothing open here. For the record, since it reads alarming and isn't:
of ~130k lines under `src/`, ~47k are in-tree tests and ~6k are bench
drivers, and `src/ui/harness/` (1,588) plus `host/test_gpu.rs` compile
into the library under `internals`. All of it is `cfg`-gated out of a
normal build, so it is a *reading* cost, not a shipped one. Revisit only
if someone wants `src/` to read as library-only.

---

# 3. Split `CascadeEngine::run_tree`, or don't

`cascade/engine.rs`, 226 lines, `const INCREMENTAL: bool`, six
`if INCREMENTAL` / `if !INCREMENTAL` branches.

**Not a de-specialization.** Swapping the const generic for a runtime
`bool` moved nothing in `cascade/run` — paired A/B in both directions
each reported "improved", which is drift, not effect, on a ~2-3% noise
floor. It did shrink `cascade::engine` codegen from 16,918 to 13,494
bytes, since the const generic inlines the walk into both callers. So
the specialization buys dead-code elimination, not speed, and a split is
not constrained by preserving it.

**The machinery the incremental path carried is gone.** `TreeSink` is
now `Option` and that path passes `None`; `Frame::cascade_prefix` is
`Option<Hasher>` and it pushes `None`; the `bool` return documents that
only the incremental walk can answer `false`. What remains is one
question:

- [ ] Split into `run_tree_full` (writes every column, owns the prefix
      hasher and the sink) and `repair_paint` (walks dirty subtrees,
      writes `paint_arena` + `subtree_hashes` + `subtree_paint_rects`
      only).

      **Extract the shared middle first.** 73 of 226 lines sit under an
      `INCREMENTAL` branch; the other 153 are the walk — frame
      pop/rollup, the 28-line transform/clip/`PaintRectCtx` block, leaf
      handling. Lift that block into a named helper and re-read the
      function; it may not need splitting at all. Two walk loops that
      must agree on traversal order is a silent failure mode the single
      loop rules out by construction, and it is the cost this item has
      to justify.

---

# 4a. Delete the hash-tag freeze

The shape tier keeps a parallel encoding of its own enum discriminants,
justified by a document format that does not exist.

**The rationale is self-referential.** `ContentHash`
(`common/content_hash.rs:4`) derives no serde; palantir writes no files
at all (`fs::write|File::create|to_writer` has zero hits under `src/`);
every serde derive in the crate is the theme format. `"saved document"`
occurs exactly once in the codebase — in the doc comment that invents it
(`shapes/record/mod.rs:226`). Every consumer (subtree rollups, measure
cache, cascade validity, damage diff) compares hashes produced in one
process run.

**What the tags do is still real; `mem::discriminant` does it.** The
nested tags are not decoration — `QuadShape::tag` is what keeps a rect
and a shadow over the same rounded box apart once both hash under
`ShapeRecord::Quad`, and likewise for `CurveBasis` / `ImageSource`. That
disambiguation stays. Only the hand-picked numbering and the freeze go.

- [ ] Replace the four `tag()` fns with `mem::discriminant(…).hash(&mut
  h)` at their call sites in `compute_record_hash` (`shapes/hash.rs:26`,
  `:34`, `:159`, `:201`): `ShapeRecord::tag` (`shapes/record/mod.rs:243`),
  `QuadShape::tag` (`shapes/paint.rs:254`), `CurveBasis::tag` (`:142`),
  `ImageSource::tag` (`:376`). Give the three that lack it the
  `#[repr(u8)]` `ShapeRecord` already carries, so the hashed discriminant
  stays the one byte the current `write_u8` costs.
- [ ] Two more hand-numbered tables sit inline in the same file and go
  the same way: `hash_fit` (`shapes/hash.rs:257`, `ImageFit` 0-4) and
  `hash_brush` (`:241`, `ShapeBrush` 0/1).
- [ ] Delete the retired-tag table and the freeze doc blocks — ~83 lines
  of doc, ~115 with the fn bodies.
- [ ] Delete `shape_record_tags_are_distinct_and_pinned`
  (`shapes/record/tests.rs:17`, ~215 lines). It is a *third* copy of the
  numbering, and its own doc concedes "a brand-new variant still has to
  be added to the table below by hand". Discriminants make the property
  it checks unfalsifiable.

**Not in scope: `layout_mode.rs`.** An earlier read counted
`PackedLayoutMeta::tag` (`layout/types/layout_mode.rs:111`) as a fifth of
these. It isn't. That is a bit-field getter — `(self.0 >> 24) as u8` —
and the numbering in `From<LayoutMode>` (`:119`) packs a tag **plus a
16-bit payload** into one `u32` for `Grid(GridDefId)` /
`Scroll(ScrollSpec)` / `Scrollbars(ScrollbarsDefId)`.
`mem::discriminant` cannot carry a payload. It is a codec that shares a
method name, and it carries no freeze doc.

**Not in scope: replacing the match with `#[derive(Hash)]`.** Recorded
so it isn't retried. The tags are 6 of `compute_record_hash`'s 197 match
lines; the other four jobs are load-bearing and a derive does none of
them.

- `f32` has no `Hash`, so `derive(Hash)` on `ShapeRecord` does not
  compile. Every float goes through `approx::canon_bits`, which folds
  sub-`EPS` and `-0.0` to `0` and canonicalizes NaN — visual-equivalence
  semantics, not a bit cast.
- Frame-local fields must be **excluded**: `points` / `colors` /
  `vertices` / `indices` are `Span`s into a per-frame arena,
  `ShapeBrush::Gradient(GradientId)` is a record-local handle,
  `RecordedText.source` is an arena span. Two byte-identical polylines in
  consecutive frames get different spans whenever anything upstream
  changes size, so hashing them makes the damage diff repaint every shape
  after an edit. `impl Hash for RecordedText`
  (`primitives/interned_str.rs:163`) already hand-writes exactly this
  exclusion.
- Precomputed sub-hashes **stand in** for that excluded payload
  (`content_hash`, `fill_grad_hash`, `RecordedText.hash`). A derive
  cannot substitute one field for another.
- Naming every field instead of `..` is deliberate (`shapes/hash.rs:83`):
  a new field fails to compile until someone decides whether it is
  hashed. A derive makes the opposite choice silently, and the failure
  mode is a missed repaint — a compile error traded for a visual bug.

The `Canon` newtype / `#[hash_visual]` helper is a real idea but belongs
in batch 9 with the `approx.rs` twin hash families; it saves ~40 call
lines, not 200. `anim-derive/` is already in the tree to host a derive.

---

# 4b. Split derived fields off `ShapeRecord`

`ShapeRecord` mixes authoring inputs with lowering outputs. `bbox`,
`content_hash` and `fill_grad_hash` are derived, stored on the record,
and then `compute_record_hash` has to exclude some by name (`bbox: _`)
while folding others in as pre-computed sub-hashes.

- [ ] Split them into a parallel `Vec<ShapeDerived>` beside
  `Shapes::hashes` (`shapes/mod.rs:45`) — the same SoA move the node
  columns already made. The hash becomes a fold over authoring fields
  only.

**The footprint win is measured, not assumed.** `ShapeRecord` is 88/8
live, matching its `hot_struct_sizes` pin. The widest variant is `Curve`:
`CurveBasis::Cubic` 36 + `width` 4 + `fill` 12 + `fill_grad_hash` 8 +
`cap` 1 + `bbox` 16 = 77 → 80, plus the outer tag → 88. Moving the three
derived fields off leaves Curve 56, Mesh 48, Polyline 32, Quad ~64, Text
unchanged at ~56 — **88 → 64 B**, a 27% cut on the hot per-shape buffer.

**Two consumers block it, and neither is in the bullet above.**

- [ ] `NanCheck for ShapeRecord` (`shapes/record/mod.rs:311`) is the
  crate's single NaN gate, and it is `O(1)` *because* `bbox` is on the
  record: every bulk input has been folded into a `bbox` under the AABB
  NaN contract, so one `Rect` test replaces an `O(n)` scan of the points
  or vertices that produced it. With `bbox` moved off, `has_nan(&self)`
  cannot see it. `Shapes::add` (`shapes/mod.rs:70`) has to test record and
  derived together, which means `Lower::lower` (`shape/mod.rs:80`) returns
  both and `NanCheck` stops being the right trait for this type.
- [ ] `ShapeRecord::bbox_local(&self, owner_size)` (`:188`) needs the
  derived row threaded in from its cascade caller
  (`cascade/engine.rs:873`).

Do 4a first — it is pure deletion and independent of this. 4b is a real
refactor whose payoff is the byte count, not the line count.

---

# 5. Stop copying theme and look values per widget per frame

Directly against the stated "steady state must be heap-alloc-free after
warmup / per-frame allocation is a real metric" posture.

- [ ] **`WidgetTheme::resolve` (`theme/mod.rs:419`)** clones
  `ui.theme.text` into `fallback_text` **unconditionally** (`:428`),
  before knowing whether the picked `WidgetLook` even has `text: None`,
  then clones the picked `WidgetLook` itself (`:433`). Both run per
  themed widget per frame. Hoist the text clone behind
  `look.text.is_none()`; pass the look by reference and only materialize
  `AnimatedLook` when a live spec needs a target.
- [ ] **`chrome::resolve_container` (`widgets/chrome.rs:19`)** does
  `explicit.or_else(|| theme_bg.cloned())` — a 124 B `Background` clone
  per `Panel` / `Grid` / `Popup` per frame whenever the theme supplies
  `panel_background`. This is exactly the copy `Widget::record`,
  `Forest::open_node`, `Tree::open_node` and `shapes::lower::background`
  all take `&Background` to avoid. Return `Option<&Background>` (or a
  small `Cow`-shaped enum) so the borrowed path survives the whole
  chain.
- [ ] **`AnimRow<AnimatedLook>` is 472 B** (pinned in
  `lib.rs::hot_struct_sizes`) — `current` + `target` + motion state, each
  carrying a full `Background` + `TextStyle`, in an
  `FxHashMap<(WidgetId, AnimSlot), _>`. One row per animated widget. The
  animated surface is actually small (fill colour, stroke colour, text
  colour); the rest is snap-carried. Consider animating a narrow
  `AnimatedLookDelta` of the interpolated channels and reconstructing the
  look from the picked `WidgetLook`, instead of storing two whole looks.
- [ ] **The three toggles resolve the same theme slot three times per
  `show`**: once in the widget for geometry (`checkbox/mod.rs`,
  `radio/mod.rs`, `switch.rs`), once in `toggle_row` for `row_gap`
  (`toggle.rs`), and once inside `WidgetTheme::resolve`. Resolve once in
  `toggle_row` and hand the slice down.

---

# 9. Micro-consolidations and the long-function inventory

A half-hour sweep, plus a list to work through separately.

- [ ] **`Cascade::run_full` (`cascade/engine.rs:223`)** does
  `by_id.clone_from(&forest.ids.curr)` — a full `WidgetId → Endpoint`
  hashmap copy on every full rebuild. The doc explains *why* a snapshot
  is needed (relayout pass B reads pass A while `curr` is cleared); it
  doesn't need to be a copy — double-buffer the two maps and swap. Not
  an input-hot-path item: `run_full` runs on structural change, not in
  steady state. Worth it to drop an allocation-shaped copy from a path
  the alloc gate watches.
- [ ] **`LayerLayout::rect_hash` (`layout/mod.rs:152`)** bulk-hashes the
  entire per-node `rect` column, and `CascadeEngine::can_update`
  (`engine.rs:160`) calls it once per layer per frame *before* deciding
  whether it can skip work. Fold the rects into a running hash during
  arrange (which already writes every rect) instead of re-reading the
  column. Bounded small — `cascade/run/paint_only`, a whole incremental
  run *including* this hash, is 1.24 µs — so this is a shape fix, not a
  perf one.
- [ ] **`Forest::push_shape` (`forest.rs:331`)** takes a closure
  returning `Option<u32>` but only tests `.is_some()`; `add_gpu_view`
  (`:282`) satisfies it by returning a `Some(0)` sentinel; and
  `add_shape_animated` (`:296`) can't use the helper at all, so it
  re-spells the open-node gate and the row counter. Make the closure
  return `bool` and give `add_shape_animated` the frame it needs — or
  drop the helper and let the three sites be three sites.
- [ ] **`Forest::current_tree` (`:396`)** is a private helper with one
  caller (`current_parent_id`).
- [ ] **`Composer::any_higher_kind_overlap`
  (`composer/mod.rs:363`)** is a one-line forwarder to
  `self.higher_kinds.any_overlap`, and its doc duplicates
  `HigherKindRects::any_overlap`'s.
- [ ] **`primitives/approx.rs` carries two parallel hash families** —
  `hash_f32`/`hash_vec2`/`hash_size`/`hash_rect` (exact, `eq_bits`) and
  `hash_visual_*` (canonicalized, `canon_bits`) — eight free functions
  differing only in which bit-canonicalizer they call. Two generic
  functions parameterized on the canonicalizer, or a `Canon<f32>`
  newtype with a `Hash` impl, covers both. 4a defers the
  `#[hash_visual]`-derive idea to here.
- [ ] **Long multi-phase functions**, in descending size. Each is one
  sitting on its own; none is urgent, all are hard to read:
  `WgpuBackend::submit` (`backend/mod.rs:395`, 265 lines — destructure,
  format ensure, scissor build, upload phase, dim pass, main pass,
  backbuffer copy, overlay pass, timestamp resolve, belt finish,
  submit), `InputState::on_input` (`input/mod.rs:594`, 258 — one match,
  10 arms, each doing hit-test + capture mutation + queue push + outcome
  derivation inline), `emit_one_shape` (`encoder/mod.rs:297`, 251 — one
  arm per variant with geometry resolution inline), `run_tree` (226,
  batch 3), `compute_record_hash` (197 — 4a takes ~6 lines off it and 4b
  a few more; the match itself stays), `text_edit::pass` (212),
  `grid::measure_inner` (198), `AnimMapTyped::tick` (193).

---

# 10. `Ui` is a god object, and `FrameCycle` is the workaround

Listed last because it is the one item with no cheap move — it is a
design question, not a cleanup.

- [ ] `ui/mod.rs` is 1,042 lines (over half comments) for one struct with
  **17 fields spanning every subsystem** — `forest`, `theme`, `state`,
  `gpu_views`, `resources`, `layout_engine`, `layout`, `cascade`,
  `input`, `input_policy`, `cascade_engine`, `display`, `damage_engine`,
  `anim`, `frame_runtime`, `window_requests`, `window_frame` — and ~45
  public methods.
- [ ] `ui/frame_cycle.rs:40` states the reason `FrameCycle` is a separate
  type: "the passes reach across nearly every field on `Ui` — grouping
  those fields would only obscure the one consumer that legitimately
  wants all of them." The workaround is documented; the shape it works
  around is not addressed.
- [ ] The plausible split is **authoring surface** (forest, theme, state,
  anim, gpu_views, window_requests — what `App::record` touches) vs.
  **pipeline state** (layout+engine, cascade+engine, damage_engine,
  display, frame_runtime — what `FrameCycle` touches), with `input` and
  `resources` shared. That makes `FrameCycle` a function over the second
  group instead of a borrow-splitting device over all seventeen, and
  gives `Ui` a derivable `Debug`. Worth a design note before any code.
