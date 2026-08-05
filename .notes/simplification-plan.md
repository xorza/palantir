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

# 2. Collapse `PaintSink`

Now the highest-priority item. Unblocked: batch 1 settled that
`RecordedPaint` stays in `src`, so the only question left is whether the
trait earns its keep — and the argument below says it doesn't.

- [ ] `paint_sink.rs:78` declares a **10-required / 12-provided-method
  trait with exactly one production implementor** (`ComposeSession`).
  The second, `RecordedPaint`, is `cfg(test)`/`bench`-only. Its
  existence makes `Encoder::encode<S>` (`encoder/mod.rs:162`),
  `encode_node<S>`, `emit_one_shape<S>` (`:297`) and `emit_shadow<S>`
  generic over the sink, so the entire encoder monomorphizes for a type
  that never ships.
- [ ] The **provided half is the real content** — the no-op gates and
  brush/stroke lowering — and it is not sink-specific at all. It reads
  as an interface but it is the encoder's own lowering tier wearing a
  trait. Move it to free functions (or an `impl ComposeSession`) and
  make the encoder take `&mut ComposeSession` concretely.
- [ ] `RecordedPaint` can then be a `#[cfg]`-gated *decorator* around a
  concrete sink, or — cheaper — the tests can assert on the
  `RenderBuffer` the composer already produces. The compose bench's
  record-once-replay-many trick is the one real constraint; a recorded
  `Vec<PaintCall>` replayed into `ComposeSession` still works with the
  trait gone.
- [ ] Once `S` is gone, `LayerCtx`'s hand-written `Debug`
  (`encoder/mod.rs:244`) is the only thing left holding the encoder's
  debug story hostage — see batch 6.

---

# 3. De-specialize `CascadeEngine::run_tree`

The densest function in the crate: `cascade/engine.rs:306`, 226 lines,
`const INCREMENTAL: bool`, six `if INCREMENTAL` / `if !INCREMENTAL`
branches. The two instantiations want different inputs *and* different
outputs, which is the signal that this is two functions.

- [ ] **`TreeSink` (`engine.rs:38`) bundles `entries`, `hits`, `scopes`,
  `layer` — and the incremental instantiation writes none of the first
  three.** They are destructured at the top of the walk and then unused
  for its whole length.
- [ ] **`Frame::cascade_prefix` (`engine.rs:66`) is dead on the
  incremental path**: filled with `Hasher::new()` at every push
  (`:516–520`), never read. It is also the single field that forces
  `Frame` to carry a hand-written `Debug` (`:69`) — `Hasher`
  (`common/hash.rs`) has no `Debug`.
- [ ] Split into `run_tree_full` (writes every column, owns the prefix
  hasher and the sink) and `repair_paint` (walks dirty subtrees, writes
  `paint_arena` + `subtree_hashes` + `subtree_paint_rects` only). The
  shared middle — frame pop/rollup, transform+clip compose,
  `PaintRectCtx` construction — is three small helpers, most of which
  already exist (`finalize_frame`, `compute_node_paint`).
- [ ] Once `cascade_prefix` is off the incremental `Frame`, derive
  `Debug` on `Frame` (give `Hasher` a `#[derive(Debug)]` — see batch 6).

---

# 4. Stop hand-maintaining what the compiler already encodes

The shape tier keeps a parallel encoding of its own enum discriminants,
on a rationale that does not hold.

- [ ] **Four hand-written `tag()` functions** — `ShapeRecord::tag`
  (`shapes/record/mod.rs:243`), `QuadShape::tag` (`shapes/paint.rs:254`),
  `CurveBasis::tag` (`:142`), `ImageSource::tag` (`:376`) — plus a fifth
  in `layout_mode.rs`. Each carries a doc block about numbers being
  "frozen once shipped in a saved document" and a table of five retired
  tags. **Nothing is ever serialized**: `ContentHash` has no serde impl
  and no on-disk use; every consumer (subtree rollups, measure cache,
  cascade validity, damage diff) compares hashes produced in the same
  process run. The freeze buys nothing and costs a hand-maintained
  parallel numbering plus ~120 lines of doc explaining it.
- [ ] **`compute_record_hash` (`shapes/hash.rs`, 270 lines)** exists
  because the tags do. With the tags gone, most of it is
  `#[derive(Hash)]` plus the `approx::canon_bits` float canonicalization
  — which wants a `Canon` newtype or a `#[hash_visual]`-style helper on
  the float fields, not a 197-line match.
- [ ] **`ShapeRecord` mixes authoring inputs with lowering outputs.**
  `bbox`, `content_hash` and `fill_grad_hash` are derived, stored on the
  record, and then `compute_record_hash` has to exclude some by name
  (`bbox: _`) while folding others in as pre-computed sub-hashes. Split
  them into a parallel `Vec<ShapeDerived>` beside `Shapes::hashes`
  (`shapes/mod.rs`) — same SoA move the node columns already made. That
  drops `ShapeRecord` well under its pinned 88 B and makes the hash a
  clean fold over authoring fields only.
- [ ] If a stable tag is genuinely wanted later, `std::mem::discriminant`
  or an explicit `#[repr(u8)]` read is one line and cannot drift from
  the enum.

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

# 6. Debug / probe / always-on-diagnostics hygiene sweep

Mechanical, low-risk, one sitting. Three related messes.

**Hand-written `Debug` where derive works.** 18 manual impls; five are
`finish_non_exhaustive` field lists justified by "`Tree` /
`LayerLayout` don't implement `Debug`" — both derive `Debug`, as does
every other field of both structs. Each impl is ~15 lines that silently
omits any field added later.

- [ ] `Frame` (`cascade/engine.rs:69`) — falls out of batch 3
- [ ] `PaintRectCtx` (`cascade/engine.rs:707`)
- [ ] `LayerCtx` (`encoder/mod.rs:244`)
- [ ] `DamageInput` (`damage/mod.rs:163`)
- [ ] `FrameScene` (`frontend/mod.rs:56`)
- [ ] `Ui` (`ui/mod.rs:113`) — prints 3 of 17 fields

**Structs with no `Debug` at all**, against `Cargo.toml`'s
`missing_debug_implementations = "deny"` (which only reaches public
types, so these slip through):

- [ ] `InputState` (`input/mod.rs:296`) — the crate's central input state
  machine, 20 fields
- [ ] `Watches` (`input/watch.rs:90`)
- [ ] `Hasher` and its `Pair` helpers (`common/hash.rs`) — used by every
  hash site in the crate; blocking batch 3's derive
- [ ] `AnimMap` (`animation/mod.rs:564`), `AnimMapTyped` (`:289`),
  `TickResult` (`:317`), `SpringStep` (`animation/spring.rs`)
- [ ] `ChildIter` / `TreeItems` (`scene/tree/iter.rs:16`, `:61`) — the two
  iterators every walk goes through
- [ ] `TreeSink` (`cascade/engine.rs:38`), `LayerWalk`
  (`damage/walk.rs:103`), `PassState` (`backend/schedule/mod.rs:331`)
- [ ] `AxisCtx` / `JustifyOffsets` / `AxisPlacement`
  (`layout/support.rs:167`, `:269`, `:358`)
- [ ] `ChromeHashBytes` (`shapes/lower.rs`, inline)

**Always-on debug machinery in the release paint path.**

- [ ] Explicit-`WidgetId` collision reporting is unconditional in
  release: `Forest::report_explicit_collision` (`forest.rs:237`) pushes
  to `Forest::collisions`, and `emit_collision_overlays`
  (`encoder/mod.rs:260`) runs at the end of **every** `Encoder::encode`
  to paint magenta outlines. The `Vec<CollisionRecord>` field, its
  `pre_record` clear, and the encoder's final pass all ship. Gate the
  overlay behind `DebugOverlayConfig` (which already exists and is
  app-global) or `debug_assertions`; keep the `tracing::error!`.
- [ ] Four near-identical per-pass probe modules (`layout/probe.rs`,
  `damage/probe.rs`, `cascade/probe.rs`, `text/cache_probe.rs`) built on
  `gated_cell!` in `common/probe.rs` — 129 lines to produce two
  zero-sized wrapper types, each module opening with a 20–25-line essay
  on which of the two gates it chose. One gate parameter
  (`Probe<const BENCH: bool>`) or one shared counter struct would cover
  all four.

---

# 7. Index the per-event scans

Each of these runs on the input hot path or once per frame, and an index
for the same query already exists elsewhere in the crate.

- [ ] **`Cascade::hits_under` (`cascade/mod.rs:261`)** reverse-scans
  *every* interactive row testing `rect.contains(pos)`. It runs on every
  `PointerMoved` (via `hit_test_targets`, three filters), twice on every
  press (`hit_test` + `hit_test_focusable`), on every release, and again
  at `end_frame`. There is no spatial index — while
  `composer/text_grid/` implements a tiled AABB grid for exactly this
  query shape and is already benched. Reuse it (or a cheap row-major
  tile bucket over `hits`, rebuilt once per cascade run).
- [ ] **`InputState::target_scroll_delta` / `_mut`
  (`input/mod.rs:525`, `:532`)** linear-scan `frame_target_deltas` on
  every scroll/zoom event *and* once per widget in `response_for`. The
  row count is tiny but the second caller makes it O(widgets × targets)
  per frame. A `WidgetIdMap` or a "last hit index" memo removes it.
- [ ] **`Cascade::run_full` (`cascade/engine.rs:223`)** does
  `by_id.clone_from(&forest.ids.curr)` — a full `WidgetId → Endpoint`
  hashmap copy on every full rebuild. The doc explains *why* a snapshot
  is needed (relayout pass B reads pass A while `curr` is cleared); it
  doesn't need to be a copy — double-buffer the two maps and swap.
- [ ] **`LayerLayout::rect_hash` (`layout/mod.rs:152`)** bulk-hashes the
  entire per-node `rect` column, and `CascadeEngine::can_update`
  (`engine.rs:160`) calls it once per layer per frame *before* deciding
  whether it can skip work. Fold the rects into a running hash during
  arrange (which already writes every rect) instead of re-reading the
  column.

---

# 9. Micro-consolidations and the long-function inventory

A half-hour sweep, plus a list to work through separately.

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
  newtype with a `Hash` impl, covers both.
- [ ] **Long multi-phase functions**, in descending size. Each is one
  sitting on its own; none is urgent, all are hard to read:
  `WgpuBackend::submit` (`backend/mod.rs:395`, 265 lines — destructure,
  format ensure, scissor build, upload phase, dim pass, main pass,
  backbuffer copy, overlay pass, timestamp resolve, belt finish,
  submit), `InputState::on_input` (`input/mod.rs:594`, 258 — one match,
  10 arms, each doing hit-test + capture mutation + queue push + outcome
  derivation inline), `emit_one_shape` (`encoder/mod.rs:297`, 251 — one
  arm per variant with geometry resolution inline), `run_tree` (226,
  batch 3), `compute_record_hash` (197, batch 4), `text_edit::pass`
  (212), `grid::measure_inner` (198), `AnimMapTyped::tick` (193).

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
