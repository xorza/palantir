# `palantir/src` — review findings

Scope: every module under `palantir/src/` (production code only; test files,
benchmark implementations, and the APIs tests reach for were not reviewed).

Supersedes `.notes/REVIEW.md` and `.notes/RENDERER-REVIEW.md`. Everything those
two carried was re-verified against the working tree: shipped, withdrawn, and
rejected material is condensed into **Do not re-attempt** at the bottom rather
than taking up space at the top; what survives is listed here with refreshed
line anchors, alongside findings from a fresh read.

**When you address an item, delete it from this file.** These are observations,
not designs — each says what is wrong, not how to fix it. Performance items
state whether the claim is **measured**, **derived** from a measurement, or
**unproven**; nothing below should be credited with a speedup until its named
benchmark moves.

Groups are ordered by impact.

---

## The frame-skip gate is computed after the work it could skip

`Ui::post_record` (`ui/mod.rs:385`) runs `layout_engine.run` first and only then
computes `cascade_fingerprint` (`:406`), so the fingerprint can skip the cascade
but never the pass that produced its own inputs.

- [ ] **The fingerprint takes only `(&Forest, Display)`** — it already covers
      root identity, complete subtree authoring, placement, surface size and
      scale (`scene/cascade/mod.rs:633-675`) — **but it is computed after
      layout** (`ui/mod.rs:390-406`). On an identical recorded frame the
      retained `Layout` is discarded and rebuilt before the marker that proves
      it didn't need to be. **Derived.**

- [ ] **One retained fingerprint cannot serve both roles.**
      `frame_runtime.prev_cascade_fp` tracks the most recent record pass,
      including an earlier pass in the *same* frame. The scene the damage
      snapshot belongs to is a different marker: pass B can equal pass A while
      both differ from the last rendered frame, so the single field would
      wrongly skip structural damage on exactly that frame.

- [ ] **A layout skip would silently evict the text reuse rows.**
      `TextSystem::end_frame` retains only rows whose hot bit was set *during
      measure*; skipping the layout pass marks nothing hot, so every reuse row
      is dropped and the next real layout pays a full re-measure. Nothing in
      `Ui::frame` (`ui/mod.rs:426-441`) couples the two, so a skip added
      without also skipping `text.end_frame` reads as a regression two frames
      later, for the wrong reason.

- [ ] **`FrameProcessing::SingleLayout` / `DoubleLayout`**
      (`ui/frame_report.rs:25`, `:30`) are named after a layout count that
      would stop being true once layout can be skipped; they actually
      distinguish *record-pass* counts.

- [ ] **No benchmark isolates the identical-record lifecycle.**
      `frame/cached_cpu` deliberately substitutes a `Full` plan after
      `Damage::Skip` so every CPU arm measures the same pipeline — it always
      includes whole-tree encode + compose. It is a valid whole-frame number
      and the wrong instrument for a lifecycle skip.

## The cascade preflight verifies O(N), then the walk can throw the work away

- [ ] **`CascadesEngine::can_update` (`scene/cascade/mod.rs:527`) does a full
      `Rect` slice comparison (16 B/node) plus a full `subtree_ends` zip per
      layer** (`:552`, `:555-562`), on every frame where anything changed at
      all. **Derived.**

- [ ] **A paint-row *count* change is only detected mid-walk.**
      `run_tree::<true>` bails at `old_span.len != new_span.len`
      (`scene/cascade/mod.rs:824`), and `run` then calls `run_full` and redoes
      every layer from scratch (`:518-521`) — after the preflight scans and
      after however much of the incremental walk already ran. Adding one shape
      to one node therefore pays preflight + partial incremental + full
      rebuild. `OpenFrame::paint_rows` (`scene/tree/recording.rs:34`) already
      maintains that count during recording, so nothing consults it. **Derived.**

- [ ] **`run_tree` carries a recoverable-failure return type only for that
      one late bail** (`scene/cascade/mod.rs:695`, `:824`, `:922`), and
      `run_full` immediately `assert!`s the same value can never be false
      (`:598`).

- [ ] **No bench arm covers a paint-row-count change**, so the fallback's cost
      relative to a paint-only change is unmeasured.

## The measure-cache restore is now the layout hot path

A direct consequence of shipping the arrange replay. Measured on `caches`, min
µs over 64 frames: `measure/cached` = 3.16 µs measure, 1.17 µs arrange. On a
root cache hit measure does no measuring, so that 3.16 µs is almost entirely
`restore_after_cache_hit` (`layout/engine.rs:227`). Arrange — the half everyone
was looking at — is now the cheaper one.

- [ ] **`scroll_content` is a dense `Vec<Size>` sized to every node** —
      cleared and zero-filled per layer per frame, duplicated in the snapshot,
      and slice-copied on every cache hit (`layout/engine.rs:235`) — for data
      with exactly one production writer (`layout/scroll/mod.rs`) and one
      reader (`widgets/scroll/mod.rs:199`). **Derived.**

- [ ] **The `text_spans` rebase is a per-node loop with a branch and an add,
      over the whole tree, on every cached frame** (`layout/engine.rs:241-250`)
      — and appears in none of the prior reviews. On a root hit `dest_start` is
      0, so the branch is doing nothing the whole way down. **Derived.**

- [ ] **The `cache_rebuild` arm adds a second per-node nested loop over
      `SLOT_COUNT`** (`layout/engine.rs:251-263`), NaN-testing each slot, plus
      another whole-subtree `copy_from_slice` for `available_q`.

- [ ] **`grid.hugs` is restored on every hit but read by neither replay
      branch.** The arrange replay made skip and translate independent of it
      (`layout/engine.rs:951-990`), so `restore_subtree` (`:267-272`) is dead
      work on the hot path and live only on the resize-bail path — the
      "three coordinated edits" contract documented at `layout/engine.rs:70-76`
      now costs more than it protects.

- [ ] **`restore_after_cache_hit` is not split by column in any benchmark**,
      so which of the four costs above dominates is unknown.

## Damage's moved-subtree leg pays a hash probe per node

- [ ] **Tier 1.5 does a `prev_map` hash probe, a `union_screens` fold, a
      `copy_from_slice`, and a `cascade_input` write for *every node* in a
      jumped subtree** (`scene/damage/mod.rs:574-658`, probe at `:604`) — every
      frame of every scroll gesture over a long list, where the structure
      inside the jump is known identical to last frame. **Unproven**; the code
      notes the leg was already optimized once (a per-row hash matcher that was
      ~18% of a scrolling frame).

- [ ] **The retained snapshot is keyed `WidgetId -> NodeSnapshot`**
      (`scene/damage/mod.rs:101`), so there is no stable slot a moved subtree's
      descendants could be reached through sequentially; identity, additions,
      removals and reparenting all share the one map.

- [ ] **No bench covers a scroll over a long list** (moved subtree, no
      authoring change), so whether probe or memcpy dominates is unknown.

## Two walks traverse the same tree in the same order, back to back

- [ ] **Cascade and damage build and copy the same rows twice.** Cascade builds
      paint rows into `paint_scratch` then copies into `paint_arena.rows`
      (`scene/cascade/mod.rs:825-826`); damage reads `paint_arena.rows` and
      copies into `arena.snaps` (`scene/damage/mod.rs:400`, `:611`). A dirty
      node's rows are built once and copied twice, with two ancestor stacks
      maintained over the same ancestry (`cascade`'s `Frame` stack vs damage's
      `parent_stack`).

- [ ] **The two cannot be fused while record replay exists.** Cascade runs
      inside `record_pass` (twice on a double-layout frame, `ui/mod.rs:373`)
      while damage runs once at the tail of `Ui::frame` (`:292`) because it
      needs `ids.removed` from `finalize_frame`. Fusing would make damage run
      twice and the first pass's diff would corrupt the snapshot baseline.

- [ ] **Same-frame record replay is a second lifecycle protocol.** A
      double-layout frame runs record, rollups, cascade and layout twice
      (`ui/mod.rs:248-266`); it is the reason `frame_had_action`
      (`input/mod.rs:320`) exists with its own reset semantics, and it is the
      direct blocker on the fusion above. No widget this crate ships calls
      `request_relayout` any more (`ui/mod.rs:552-566` says so).

## Fixed per-kind GPU replay forces the composer to recover authoring order

- [ ] **The composer's largest subsystem exists to undo the backend's
      reorder.** Render order within a group is fixed at quads → text → meshes
      → images → curves (`renderer/frontend/composer/mod.rs:54-67`), so
      `HigherKindRects`, two `TextRectGrid`s, `quad_forces_flush` (`:378`),
      `closed_hit` (`:405`), and the strict-bounds batch rule (`:1497-1507`)
      all exist to detect and flush the cases where that reorder would change
      the picture. Everything in it is a reasonable optimization *of* the
      current design, so nothing local there is worth touching while the
      order-preservation question is open.

## `response_for`'s fast path is gated on the pointer being off-surface

- [ ] **`frame_quiescent` requires `pointer_pos.is_none()`**
      (`input/mod.rs:927`), so the whole-interaction-half short-circuit
      (`:966-975`) fires only when the cursor has left the window — the *rare*
      case. Whenever the pointer is anywhere over the surface, every widget
      pays the three-button capture scan, the drag math, the `pointer_local`
      transform, and the scroll lookup, whether or not anything is captured.
      **Unproven** — no bench distinguishes pointer-present from
      pointer-absent idle frames.

- [ ] **`scroll_delta_for` is a linear scan run once per widget per frame**
      on that same non-quiescent path (`input/mod.rs:502-507`, called at
      `:1045`), over a list that is almost always empty or one entry.

## Repeated search where the input is already monotonic

- [ ] **`bake_stops` restarts the stop search at index 1 for every one of the
      256 LUT texels.** `lerp_at` (`renderer/gradient_atlas/bake.rs:47`) opens
      with `let mut upper = 1;` and walks forward from there; the stops are
      sorted and the sampled `t` values increase monotonically across the
      caller's loop (`:34-43`), so the cursor can only move forward.
      `O(LUT_SIZE × stop_count)` for what the data supports as
      `O(LUT_SIZE + stop_count)`.

- [ ] **The `gradient` bench has one arm** (`gradient/repeated_chrome`,
      `renderer/frontend/bench.rs:89`) and never bakes a LUT, so there is no
      instrument for the item above.

## Layout driver identity restated across three dispatches

- [ ] **Three exhaustive `LayoutMode` matches must stay synchronized** —
      `measure_dispatch` (`layout/engine.rs:821`), `arrange`
      (`layout/engine.rs:897`), and `intrinsic::content_intrinsic`
      (`layout/intrinsic/mod.rs:205`). Adding a driver needs three edits, and
      `Scroll` delegates differently in each phase. A fourth exhaustive match
      over the same enum, `arrange_depends_only_on_slot`
      (`layout/types/layout_mode.rs:38`), carries a soundness contract with no
      compile-time tie to the three.

## Functions that run past 150 lines with several concerns each

Not length for its own sake — each of these interleaves decisions that are
independently testable.

- [ ] **`DamageEngine::compute` is 427 lines** (`scene/damage/mod.rs:293`),
      holding five diff tiers, the `MOVED_SUBTREE` sentinel round-trip
      (`:457` → `:574`), a nested mini parent-stack (`:581-599`), the
      predamage fold, and the eviction tail in one body.

- [ ] **`Scroll::show` is 283 lines** (`widgets/scroll/mod.rs:464`) covering
      wheel/pinch routing, zoom gating, two bar gestures, wrapper patching,
      and the nested record — with the state mutation buried in a 90-line
      block expression (`:548-632`).

- [ ] **`WgpuBackend::submit` is 272 lines** (`renderer/backend/mod.rs:393`)
      mixing the upload phase, three debug-overlay paths, pass selection, the
      backbuffer copy, timestamp resolve, and belt recall.

- [ ] **`InputState::on_input` is 257 lines** (`input/mod.rs:571`) — one match
      whose arms each derive their own `observable` and `frame_had_action`
      answer from different rules; the rules are documented on the field
      (`:296-319`) rather than at the arms that implement them.

- [ ] **`emit_one_shape` is 256 lines** (`renderer/frontend/encoder/mod.rs:294`)
      with per-variant payload assembly inline in the dispatch.

## Near-duplicate bodies

- [ ] **`ComposeSession::curve` and `ComposeSession::arc` are the same nine
      steps** (`renderer/frontend/composer/mod.rs:1131` and `:1209`) — width
      scaling, `stroke_bbox_urect`, `enter_higher_kind`, the `rotation != 0.0`
      pivot block, a `ColorU8` conversion, and a `push_sub_instances` with a
      `CurveInstance` prototype differing only in which lanes carry geometry.

- [ ] **`Scroll::show`'s two per-axis loops differ only in what they do with
      the result** (`widgets/scroll/mod.rs:566-594` and `:595-622`): both
      iterate `[(Axis::Y, ..), (Axis::X, ..)]`, both skip on `!panned`, both
      call `scrollbars::bar_geometry` with the same five arguments derived the
      same way.

- [ ] **`scroll_wrappers`' exhaustive destructure does not actually cover the
      whole node.** It binds every field with no `..`
      (`widgets/scroll/mod.rs:290-315`) precisely so a new `Node` field can't
      vanish — then deliberately drops `clip` and `transform`, which
      `Scroll::show` re-derives 350 lines later (`:672`, `:678-680`). The
      guarantee the destructure exists for stops at the two fields most likely
      to be forgotten.

- [ ] **`run_dim_pass` and `run_overlay_pass` are the same shape**
      (`renderer/backend/mod.rs:665` and `:983`): `begin_load_pass` plus one
      `self.debug.draw_*` against `fmt.quad.select(false)` and
      `self.gradient.bg`; only the target view and the draw call differ.

## Release asserts on a per-frame path

- [ ] **`MeasureCache::capture_tree` runs six release `assert_eq!`s per tree
      per rebuild frame** (`layout/cache/mod.rs:213-218`) plus a seventh
      inside the text branch (`:245`). All seven are internal
      column-length invariants, not public-API contracts — the category the
      crate's own assert policy reserves `debug_assert!` for.

## Docs describing code that no longer exists

- [ ] **`Ui::close_scope` carries 35 lines of doc, the first 25 of which
      document a different function** (`ui/mod.rs:902-936`). The block opens
      "`[Self::layer]`, for an overlay that **owns input** while it is up"
      and describes `owner` / re-opening every frame — an entry point that no
      longer exists (`claim_keyboard`, `KeyboardCapture`, and
      `with_keyboard_capture` are all gone; the mechanism is now
      `Configure::input_scope` at `scene/node/mod.rs:512`). The actual
      `close_scope` doc starts mid-block at `:927`.

- [ ] **`Backbuffer::size`'s justifying comment is wrong**
      (`renderer/backend/mod.rs:85-88`). It claims `wgpu::Texture::size()`
      costs "~15 µs/frame … 14% of trace time" through an Arc traversal; that
      measurement predates wgpu 30 making the call an inline field read.

- [ ] **`InputState`'s doc points at a type that isn't there**
      (`input/mod.rs:235`): "lives in `crate::scene::cascade::Cascade`" — the
      type is `Cascades`. `frame_pointer_events`'s doc says it is "read
      through `Self::pointer_events`, which layer-gates it against
      `Self::pointer_events`" (`:352-353`) — the sentence names the same
      function twice.

- [ ] **`DamageEngine::compute`'s doc promises a `Damage::Skip` on a
      degenerate surface** (`scene/damage/mod.rs:264-266`) while
      `DamageInput::surface` (`:176-183`) says `collapse_from` asserts on
      exactly that case "rather than degrading silently".

- [ ] **`run_main_pass`'s stencil comment contradicts its own method doc.**
      The doc (`renderer/backend/mod.rs:688-695`) states the tail clear —
      "not rect disjointness" — is what keeps one rect's stencil writes out of
      another's reads; the attachment comment twelve lines down (`:726-729`)
      justifies the once-per-pass clear by "the rect-disjointness invariant".

## Benchmark gaps

Each gates a finding above; none is a finding on its own.

- [ ] **Identical-record lifecycle** — record + rollup + gate + damage with no
      forced frontend work. Control: `frame/cached_cpu`.
- [ ] **Cascade with a paint-row *count* change.** Control: a paint-only change
      (where the incremental walk succeeds).
- [ ] **Scroll over a long list** — moved subtree, no authoring change; probes
      vs bytes copied. Control: a static list.
- [ ] **`restore_after_cache_hit` split by column.** Control: forced miss.
- [ ] **Maximum-stop LUT bake.** Control: a two-stop gradient.
- [ ] **Idle frame with the pointer over the surface vs off it**, for
      `response_for`.

---

## Do not re-attempt

Not findings. Recorded so the ground is not re-walked; each was examined and
closed.

**Shipped since the source audits.** Layer-ordered keyboard capture (a `Modal`
sees Escape under a capturing `Popup`; a `TextEdit` inside a popup receives
typing); `HostHandle::run_on_main` returning `Result<(), HostDisconnected>`;
`DragValue`'s `inherit_chip_node` carrying node policy across the chip→editor
swap; the **arrange replay** (arrange 89.18 → 1.17 µs on `measure/cached`,
30–150× across arms, whole layout pass 92.4 → 4.36 µs); `Sense::ABSORB_POINTER`
replacing `Modal`'s `BLOCK` and `Popup`'s hand-written eater sense; the
image-shader nearest-filter branch; scissor deduplication through one
`cur_scissor` state; adjacent same-texture image-draw coalescing (`images/shared`
3.6–4.1 → 1.3 µs); one-entry text-grid spill; `encode_node`'s dropped
`ChromeRow` copy; `classify_frame` → `take_frame_plan`; the paint-sink pipeline;
`text/mod.rs` split 1032 → 401 production lines.

**Withdrawn on measurement.**
- *Encoded text-cache sweep cadence* — the whole pass is ~1.6 ns per live row
  (`encoded_cache_sweep` bench); a cadence gate would trade uniform per-frame
  cost for a spike with no average to recover.
- *Backend bind-state tracking split, including text* — ~11 ns per recorded
  step, ~29 ns per text batch; the full fix recovers ~4 µs on a fixture
  engineered to produce 256 consecutive text batches, which real frames don't.
- *Backend last-binding cache* — structurally impossible alongside run
  coalescing: once adjacent runs merge, no two consecutive lookups can share an
  id, so a one-entry cache has a guaranteed 0% hit rate.

**Rejected after being built.**
- *A `Configure` delegation macro for the 20 identical `node_mut` impls.*
  `node` is private to each widget module, so a single roster in
  `widgets/mod.rs` needs 19 visibility escalations; per-widget invocation gives
  up the roster, which was the only benefit.
- *Live keyboard-ownership resolution.* With popups B then A recording in that
  order, B reads while topmost-so-far and A reads after displacing it, so
  *both* receive the key. Stable-during-frame / resolve-at-end is right; only
  the starting value can be stale.
- *An overlay recorder unifying `Popup` / `Modal` / `Tooltip` chrome and
  placement.* The three resolve different theme slots against different fields,
  placement is already two library calls, and the two scrims are structurally
  different — the shared type's fields would be the differences.

**Examined and correctly rejected in the source documents.** Physical module
reorganization; owner-partitioned retained render targets; range-aware mesh
uploads; a retained command buffer or compose cache; merging typed
render-buffer columns; unifying the image/curve/mesh/text pipelines; globally
sorting higher-kind primitives; replacing retained scratch with locally
collected iterators; `Backbuffer::size`'s cached field (the comment is wrong,
the field is not); two hand-built full-viewport quads; the debug dim quad's
missing `FillKind::SOLID.with_fast()`; `shader_template::specialize`'s startup
string copies; merging the three "did it change?" hash families;
`Cascades::by_id.clone_from`; the composer's `OcclusionPruner`; per-layer
iteration over five layers; `Tree::compute_rollups` re-hashing.
</content>
</invoke>
