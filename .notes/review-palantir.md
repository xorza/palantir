# Palantir module review

Scope: `palantir/` — `src/`, `benches/`, `Cargo.toml`. Test *structure* and
the APIs tests reach through are out of scope except where production code is
shaped by them.

**When you address an item, delete it.** This file lists open findings only —
no "done" markers, no resolved section, no history.

---

## Comments record the history of the code rather than its current state

22% of `src/` is comment lines (28.7k of 129.6k); 13 files are over 50%
(`scene/cascade/mod.rs` 55%, `input/response.rs` 54%, `ui/mod.rs` 53% of 1041
lines). Much of that volume is narration of refactors that already landed,
which is why the factual claims in it drift out of sync with the code.

The specific drift found in the first pass — five stale `Background` sizes,
`ChromeRow`, `Brush`, the dead `resolve_look` / `handle_input` /
`Node::columns` / `Cascade.paint_rect` references, the split `into_columns`
doc block, and eight superseded-design narrations — has been corrected.

- [ ] That correction was **site-by-site over a sample, not an audit**. The
  pass followed grep hits for the specific stale symbols and sizes already
  identified; it did not sweep every file for history narration or for other
  doc references to renamed items. The same class of drift is likely present
  in files the sample did not reach — the 50%-comment files above are where
  it would concentrate.
- [ ] No lint guards the class. `Cargo.toml`'s `[lints.rust]` sets
  `missing_debug_implementations`, `unreachable_pub`, and
  `items_after_test_module`, but nothing denies
  `rustdoc::broken_intra_doc_links`, so a doc link to a renamed item degrades
  to a warning in a build that already prints none. Running
  `RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links" cargo doc --no-deps
  --all-features` surfaced seven broken links that no other gate reports;
  six are fixed, and the seventh is below.
- [ ] `primitives/image.rs:49` links `[`crate::Shape::Image`]`. `Shape` is a
  unit-struct constructor namespace, so the live item is `Shape::image` —
  the same stale enum-variant spelling that `image_registry.rs`,
  `shape/style.rs`, `shape/polyline.rs`, `text/mod.rs`, and `ui/mod.rs`
  carried. Left alone because that file was under concurrent edit.

## Production code is generic, indirected, or public solely to serve test and demo consumers

- [ ] `renderer/frontend/paint_sink.rs`: `PaintSink` is a 10-required /
  12-provided-method trait with exactly **one** production implementor
  (`ComposeSession`). The second implementor, `RecordedPaint`, is
  `cfg(test)`/`bench`-only. Its existence makes `Encoder::encode<S>`,
  `encode_node<S>`, `emit_one_shape<S>`, and `emit_shadow<S>` generic over the
  sink and forces the whole encoder to monomorphize.
- [ ] 6,436 lines of criterion/dhat driver code live inside `src/**/bench.rs`
  and are re-exported through a `pub mod bench` on the library, with 21
  four-line wrapper targets in `benches/` and 21 near-identical `[[bench]]`
  blocks in `Cargo.toml`. The `bench` feature therefore changes the crate's
  public API surface.
- [ ] `src/frame_fixture/` (1,578 lines of a designed demo screen) is a module
  of the library and `FrameFixture` is a `pub use` in `lib.rs`, gated on
  `bench` **or** `showcase`.
- [ ] `src/demo_swatches.rs` is `pub` in `lib.rs` with the comment "Public only
  because the `showcase` binary is a separate crate from this library and
  cannot reach a `pub(crate)` one; not part of the supported API."
- [ ] `src/ui/harness/` (1,588 lines) plus `host/test_gpu.rs` are compiled into
  the library and re-exported through `pub mod internals`.
- [ ] Counting these together: of 129.6k lines under `src/`, roughly 56k are
  tests, bench drivers, harnesses, and demo fixtures, and three of the crate's
  feature flags exist to publish parts of them.

## `Ui` is a god object, and a second type exists to work around that

- [ ] `ui/mod.rs` is 1,041 lines (53% comments) for one struct with 17 fields
  spanning every subsystem in the crate — `forest`, `theme`, `state`,
  `gpu_views`, `resources`, `layout_engine`, `layout`, `cascade`, `input`,
  `input_policy`, `cascade_engine`, `display`, `damage_engine`, `anim`,
  `frame_runtime`, `window_requests`, `window_frame` — and ~45 public methods.
- [ ] `ui/frame_cycle.rs:40` states the reason `FrameCycle` is a separate type:
  "the passes reach across nearly every field on `Ui` — grouping those fields
  would only obscure the one consumer that legitimately wants all of them."
  The workaround is documented; the shape it works around is not addressed.
- [ ] `Ui` needs a hand-written `Debug` printing 3 of 17 fields
  (`ui/mod.rs:111`).

## Const-generic specialization where the two instantiations want different inputs and outputs

- [ ] `scene/cascade/engine.rs:306`: `run_tree::<const INCREMENTAL: bool>` is
  226 lines with six `if INCREMENTAL` / `if !INCREMENTAL` branches.
- [ ] `TreeSink` (`engine.rs:38`) bundles `entries`, `hits`, `scopes`, `layer`,
  but the incremental instantiation writes none of the first three — they are
  destructured and then unused for the whole walk.
- [ ] `Frame::cascade_prefix` is a `Hasher` that the incremental instantiation
  fills with `Hasher::new()` at every push and never reads
  (`engine.rs:516–520`), and it is the field that forces `Frame` to carry a
  hand-written `Debug` (`engine.rs:69`).

## Hand-maintained parallel encodings of data the compiler already has

- [ ] Five hand-written `tag()` functions (`layout_mode.rs:111`,
  `shapes/paint.rs:142/254/376`, `shapes/record/mod.rs:242`) duplicate enum
  discriminants. The stated rationale — "the enum's `repr` discriminant is
  unread, so variants can be reordered without moving a hash"
  (`shapes/hash.rs:9`) — buys nothing: `ContentHash` is never serialized
  (no serde impl, no on-disk use), so no consumer survives a process boundary.
- [ ] `ShapeRecord` (88 B) mixes authoring inputs with derived data: `bbox`,
  `content_hash`, and `fill_grad_hash` are lowering outputs stored on the
  record, and `compute_record_hash` then has to exclude some of them by name
  (`bbox: _`) while folding others in. The 197-line hash function is split
  between arms that hash fields inline and arms that read a pre-computed
  sub-hash off the record.
- [ ] `LayerCtx` (`encoder/mod.rs:243`) and `PaintRectCtx`
  (`cascade/engine.rs:707`) carry hand-written `Debug` impls justified by
  "`Tree` / `LayerLayout` don't implement `Debug`". Both types derive `Debug`,
  as do every other field of both structs. Each impl is ~15 lines of
  `finish_non_exhaustive` boilerplate that silently omits any field added
  later.
- [ ] 26 hand-written `fmt::Debug` impls in total; the `Frame`, `DamageInput`,
  `LayerCtx`, `PaintRectCtx`, and `Ui` ones are all field-listing
  `finish_non_exhaustive` blocks.

## Structs without `#[derive(Debug)]`, against the crate's own lint intent

`Cargo.toml` denies `missing_debug_implementations`, which only reaches public
types. These are all non-public and carry no `Debug` at all:

- [ ] `InputState` (`input/mod.rs:296`) — the crate's central input state
  machine, 20 fields.
- [ ] `Watches` (`input/watch.rs:90`).
- [ ] `AnimMap` (`animation/mod.rs:564`), `AnimMapTyped` (`:289`),
  `TickResult` (`:317`), `SpringStep` (`animation/spring.rs:59`).
- [ ] `Hasher` and its two `Pair` helpers (`common/hash.rs:39`, `:144`, `:197`)
  — used by every hash site in the crate.
- [ ] `ChildIter` and `TreeItems` (`scene/tree/iter.rs:16`, `:61`) — the two
  iterators every walk in the crate goes through.
- [ ] `TreeSink` (`scene/cascade/engine.rs:38`), `LayerWalk`
  (`scene/damage/walk.rs:103`), `PassState` (`backend/schedule/mod.rs:331`),
  `Bound` (`backend/mod.rs:824`).
- [ ] `AxisCtx`, `JustifyOffsets`, `AxisAlignPair`, `AxisPlacement`
  (`layout/support.rs:167`, `:269`, `:352`, `:358`).
- [ ] `ChromeHashBytes` (`scene/shapes/lower.rs:196`).

## Long multi-phase functions

- [ ] `WgpuBackend::submit` (`backend/mod.rs:395`) — 265 lines covering
  destructure, format ensure, scissor build, a scoped upload phase, dim pass,
  main pass, backbuffer copy, overlay pass, timestamp resolve, belt finish, and
  submit.
- [ ] `InputState::on_input` (`input/mod.rs:594`) — 258 lines; a single match
  with 10 arms, each doing hit-testing, capture mutation, queue pushes, and
  outcome derivation inline.
- [ ] `emit_one_shape` (`encoder/mod.rs:296`) — 251 lines, one arm per
  `ShapeRecord` variant with the per-variant geometry resolution inline.
- [ ] `run_tree` (`cascade/engine.rs:306`) — 226 lines (see the const-generic
  group above).
- [ ] Others over 180 lines: `text_edit::pass` (212), `grid::measure_inner`
  (198), `compute_record_hash` (197), `AnimMapTyped::tick` (193),
  `text_edit::run_input` (188).

## Per-event and per-frame linear scans where an index already exists elsewhere in the crate

- [ ] `Cascade::hits_under` (`cascade/mod.rs:261`) reverse-scans **every**
  interactive row testing `rect.contains(pos)`. It runs on every
  `PointerMoved` (via `hit_test_targets`, three filters), on every press
  (twice — `hit_test` plus `hit_test_focusable`), on every release, and again
  at `end_frame`. There is no spatial index, though
  `composer/text_grid/` implements a tiled AABB grid for the same kind of
  query.
- [ ] `InputState::target_scroll_delta` / `target_scroll_delta_mut`
  (`input/mod.rs:525`, `:532`) linear-scan `frame_target_deltas` on every
  scroll/zoom event and once per widget in `response_for`.
- [ ] `LayerLayout::rect_hash` (`layout/mod.rs:152`) bulk-hashes the entire
  per-node `rect` column, and `CascadeEngine::can_update`
  (`cascade/engine.rs:160`) calls it once per layer per frame before deciding
  whether it can skip work.
- [ ] `Cascade::run_full` does `by_id.clone_from(&forest.ids.curr)` — a full
  `WidgetId → Endpoint` hashmap copy on every full rebuild
  (`cascade/engine.rs:223`).

## Theme resolution repeats work and copies the values the rest of the crate threads by reference

- [ ] `widgets/chrome.rs:25`: `resolve_container` does
  `explicit.or_else(|| theme_bg.cloned())` — a 124 B `Background` clone per
  `Panel` / `Grid` / `Popup` per frame whenever the theme supplies
  `panel_background` and the caller did not override it. This is exactly the
  copy that `Widget::record`, `Forest::open_node`, `Tree::open_node`, and
  `shapes::lower::background` all take `&Background` to avoid.
- [ ] `WidgetTheme::resolve` (`theme/mod.rs`) clones `ui.theme.text` into
  `fallback_text` unconditionally, before knowing whether the picked
  `WidgetLook` even has `text: None`, and clones the picked `WidgetLook`
  itself. Both run per themed widget per frame.
- [ ] The three toggles resolve the same theme slot three times per `show`:
  once in the widget for geometry (`checkbox/mod.rs:223`, `radio/mod.rs:813`,
  `switch.rs:322`), once in `toggle_row` for `row_gap` (`toggle.rs:591`), and
  once inside `WidgetTheme::resolve`.

## Always-on debug machinery in the release paint path

- [ ] Explicit-`WidgetId` collision reporting is unconditional in release:
  `Forest::report_explicit_collision` emits a `tracing::error!` and pushes to
  `Forest::collisions`, and `emit_collision_overlays` (`encoder/mod.rs:259`)
  runs at the end of every `Encoder::encode` to paint magenta outlines. The
  `Vec<CollisionRecord>` field, its `pre_record` clear, and the encoder's
  final pass are all present in a shipping build.
- [ ] Four near-identical per-pass probe modules (`layout/probe.rs`,
  `scene/damage/probe.rs`, `scene/cascade/probe.rs`, `text/cache_probe.rs`)
  built on a `gated_cell!` macro in `common/probe.rs`, each opening with a
  20–25-line doc essay explaining which of the two gates it chose and why.
  `common/probe.rs` is 129 lines to produce two zero-sized wrapper types.

## Naming collisions across subsystems

- [ ] "Probe" names five unrelated things: `TextProbe` (public text-geometry
  API), `TextLayoutProbe` (a shaper lease), and `LayoutProbe` / `DamageProbe` /
  `CascadeProbe` / `AtlasProbe` / `EncodedProbe` (build-gated counters). A
  reader hitting `probe` has no way to tell which kind is meant.
- [ ] Two modules named "layout probe": `layout/probe.rs` (layout-pass
  counters) and `text/layout_probe.rs` (shaped-text geometry lease).
- [ ] `Frontend::build`, `Encoder::encode`, `Composer::begin`,
  `ComposeSession`, `PaintSink`, `RecordedPaint`, `RenderBuffer`,
  `RecordStore`, `RecordPayloads`, `record_sink`, `RecordApp`,
  `FrameCycle::record_pass` all use "record" for at least three distinct
  concepts (authoring a tree, capturing paint calls for tests, and the SoA row
  storage).

## Smaller consolidations

- [ ] `Forest::push_shape` (`scene/forest.rs:330`) takes a closure returning
  `Option<u32>` but only ever tests `.is_some()`; `add_gpu_view` satisfies it
  by returning a `Some(0)` sentinel (`:284`). `add_shape_animated` cannot use
  the helper at all and re-spells the open-node gate and the row counter
  (`:295–317`).
- [ ] `Forest::current_tree` (`:395`) is a private helper with one caller
  (`current_parent_id`).
- [ ] `Composer::any_higher_kind_overlap` (`composer/mod.rs:363`) is a
  one-line forwarder to `self.higher_kinds.any_overlap`, and its doc comment
  duplicates `HigherKindRects::any_overlap`'s.
- [ ] `Cargo.toml` carries 21 `[[bench]]` blocks that differ only in `name`,
  each repeating `harness = false` and `required-features = ["bench"]`.
- [ ] `examples/dump_theme.rs` is the only example not declared in
  `Cargo.toml`; `counter` and `custom_widget` both have explicit `[[example]]`
  blocks, so the omission reads as an oversight rather than a choice.
