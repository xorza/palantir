# Palantir crate review

Findings from a full read of `src/` (~97k lines, 505 files). Each item is a
checklist entry: **when you address one, delete it.** The file lists open
findings only — no "done" markers, no resolved section.

Findings are grouped by the root cause they share, and the groups are ordered
by severity and benefit. Descriptions state what is wrong and where; they
deliberately do not propose fixes.

Two things are deliberately out of scope: the record-time-geometry limitation
(already surveyed in `.notes/record-time-geometry.md`) and test structure.
Behavioural defects found along the way are logged separately in
`.notes/ISSUES.md` rather than here.

---

## Documentation that contradicts the code it describes

Distinct from the group above: these do not state invariants, they simply
describe something that is no longer there. Every reviewer hit this
independently, in every subsystem, which makes it a process problem rather than
a set of isolated slips. Several name symbols that do not exist anywhere in the
crate — grep-checkable and still wrong.

- [ ] `src/scene/damage/mod.rs:2` — the module doc says the prev-frame snapshot
      is rebuilt "via the `entry()` API — vacant slots get inserted, occupied
      slots get diffed and either updated or evicted", and `:26` refers to "the
      Vacant arm". There is no `.entry(` call anywhere under
      `src/scene/damage/`. The same doc describes a field `DamageEngine.dirty`
      that does not exist (it is `counters.dirty`) and cites `SeenIds::record`,
      whose real name is `record_endpoint` (`:102`).

- [ ] `src/lib.rs:53` — the published feature table lists six flags;
      `Cargo.toml` declares eight. Missing are `bench` and `golden` — and
      `golden` gates `pub mod golden`, a real part of the published surface, so a
      consumer reading the docs cannot discover it exists. The `internals` row
      claims it "adds the `internals` and `bench` modules", but `bench` is gated
      on its own feature and the dependency runs the other way
      (`bench = ["internals", …]`): enabling `internals` never produces a `bench`
      module.

- [ ] `src/renderer/frontend/mod.rs:5` and
      `src/renderer/frontend/composer/mod.rs:34` — both state that the composer
      "owns the output"/"owns its output buffer". `Frontend` owns `buffer`
      (`mod.rs:69`) and lends it per call.

- [ ] `src/renderer/mod.rs:4` — says the frontend "owns the per-frame
      allocations (cmd vec, render buffer) and turns `&Tree` into
      `&RenderBuffer`". There is no cmd vec — `Frontend::build`'s own doc
      (`src/renderer/frontend/mod.rs:83`) says encoder paint calls land directly
      in a live `ComposeSession` — and `build` takes a `FrameScene`.

- [ ] `src/primitives/half_simd/mod.rs:5` opens by explaining that the module
      exists to bypass `half::slice::HalfFloatSliceExt`, with the frame win
      quantified. Four wrapper docs still describe the path it replaced:
      `src/primitives/color/mod.rs:394` ("go through
      `half::slice::HalfFloatSliceExt::{…}`"), `src/primitives/spacing.rs:62`
      ("Routes through `half`'s platform-specific batched f16→f32 path"),
      `src/primitives/corners.rs:91` ("same `half` slice path"), and
      `src/primitives/brush/gradient/mod.rs:139` ("via the batched slice path").
      All four go through `F16x4::lanes`, which calls `_mm_cvtph_ps` directly.

- [ ] `src/renderer/backend/queue.rs:1` — says the tally happens "with the
      `internals` feature enabled" and that "without `internals`, the wrapper is
      a zero-cost passthrough"; the gate at `:32` is `bench`.
      `src/renderer/backend/write_stats.rs:1` says "gated behind the `bench`
      feature" while `src/renderer/backend/mod.rs:25` stacks `internals` on top
      of `bench` — and since `bench = ["internals", …]` the first attribute is
      dead.

- [ ] `src/renderer/frontend/composer/mod.rs:23` — the comment says `pub(crate)`
      is "only so the `text_grid` benchmark can reach the gated `internals`
      harness; every item inside stays `pub(super)`." Neither half is true:
      `TILE_SIZE`, `TILE_CAP`, `TextRectGrid`, `spill`, `start_frame`, `clear`,
      `push` and `any_overlap` are all `pub(crate)`, there is no `internals`
      harness in `text_grid`, and the only outside consumer is
      `text_grid::bench`, a child module that already sees private items.

- [ ] `src/primitives/urect/mod.rs:240` — `URect16`'s doc says it is "Used where
      many rects are stored in a hot Pod struct (e.g. `TextRun.bounds`)".
      `TextRun.bounds` is a plain `URect` (`src/renderer/backend/text/mod.rs:180`),
      and `URect16` has zero references outside its own definition.

- [ ] `src/input/bench.rs:8` and `:102` — describes what it measures as
      "`recompute_hover` + `recompute_scroll_target` linear walk over cascade
      entries", and says the scroll region exists "so `recompute_scroll_target`
      succeeds". Neither symbol exists in the crate; the code is
      `refresh_pointer_targets` calling `Cascade::hit_test_targets`.

- [ ] `src/display.rs:18` — says the host "hands it to `WindowDriver::frame`".
      `WindowDriver` has `cpu_frame` and `render_to_texture`, no `frame`.

- [ ] `src/input/sense.rs:112` — claims `DOUBLE_CLICK_RADIUS` matches
      "`TextEdit`'s `MULTI_CLICK_RADIUS`", the only mention of that identifier in
      the crate. `:106` and `:112` also intra-doc-link `[crate::input::Capture]`,
      a private struct in another module.

- [ ] `src/renderer/image_registry.rs:16` — points at `renderer::texture_id` for
      "`TextureId` + its source". No such module exists: the id is
      `primitives::texture_id`, the source is `renderer::texture_id_source`.

- [ ] `src/shape/curve.rs:64` — `impl Default for CurveBasis`'s doc calls it
      "the `Default` a `DrawCurvePayload` literal falls back to".
      `DrawCurvePayload` (`src/renderer/frontend/payload.rs:557`) derives no
      `Default`, and the impl has no caller.

- [ ] `src/widgets/widget.rs:107` — `Widget::show`'s doc names "the handful
      (`Frame`, `Panel`, `Grid`, `Separator`)" as its callers. `Text`
      (`src/widgets/text.rs:118`), `ProgressBar` (`progress_bar.rs:56`) and
      `Spinner` (`spinner.rs:93`) also call it.

- [ ] `src/widgets/checkbox/mod.rs:69`, `src/widgets/radio/mod.rs:76`,
      `src/widgets/switch.rs:67` — the same pasted comment claims the theme slot
      is named there and "**this is the only place it is named**". Each names it
      again in the `WidgetTheme::resolve` fallback closure a dozen lines below.

- [ ] `src/widgets/theme/toggle.rs:14` — `ToggleTheme`'s doc lists its consumers
      as "`Checkbox`, `RadioButton`, future toggle/segmented controls" and omits
      `Switch`, a current consumer with fields (`track_aspect`) in that struct.

- [ ] `src/renderer/frontend/composer/session.rs:88` — `scaled_rect`'s doc calls
      it "the opening move of `rect`, `shadow`, `image`, and `text`". There are
      no `rect` or `shadow` handlers; both were folded into `quad`.

- [ ] `src/renderer/frontend/composer/text_grid/mod.rs:108` and `:142` —
      attribute profiling numbers to `Composer::compose`, a function that no
      longer exists. `src/renderer/frontend/composer/mod.rs:127` says
      `GroupCursors` bundles "five parallel fields"; it has six.

- [ ] `src/ui/frame_cycle.rs:153` — the comment says the `PaintOnly` path should
      "pass an empty set instead of stale state from the previous frame".
      `compute_paint_only` (`src/scene/damage/mod.rs:373`) takes no removed set
      at all.

- [ ] `src/primitives/color/mod.rs:549` — the note above `linear_to_oklab` says
      there is "no other in-crate caller until slice 2 wires the atlas through
      the encoder/composer" and explains a `dead_code` concern. The function has
      had callers in `src/renderer/gradient_atlas/bake.rs:31` and `:85` since,
      and carries no allow.

- [ ] `src/primitives/nan.rs:24` — the module doc's closing line, "`f32` is the
      leaf every other impl bottoms out in", is wrong: the impls bottom out in
      `f32::is_nan`, not in `NanCheck for f32`, which is never reached.

- [ ] `src/primitives/half_simd/mod.rs:67` — claims "Both f16 lane predicates in
      the crate are this one test"; `src/primitives/approx.rs:105` and `:121`
      hold two more.

- [ ] `src/primitives/span.rs:7` — directs readers to the `Range<u32>`
      conversions; three of `Span`'s four `From` impls (`:35`, `:55`, `:62`)
      have no callers.

- [ ] `src/primitives/size.rs:45` — `is_inf`'s doc describes "callers using this
      as a 'no upper bound' sentinel". It has zero callers, tests included.

- [ ] `src/renderer/gradient_atlas/bench.rs:49` — says "Requires the `internals`
      feature" one line above a run command using `--features bench`, in a
      module gated `#[cfg(feature = "bench")]`.

- [ ] `src/scene/layer.rs:31` — the `PerLayer` doc says "Three ways in… The
      backing array is private so those stay the only spellings", immediately
      above two `IntoIterator` impls (`:109`, `:117`) that add a fourth and have
      no callers.

- [ ] `src/primitives/image.rs:22` — `ImageFit::Fill`'s doc calls it "the legacy
      'no fit' behaviour", a compatibility framing the project's stated posture
      rejects.

- [ ] `src/bin/showcase/pages/shapes.rs:156` — the comment states the cell
      "Exercises the alloc-free claim"; `stress` (`:163`) allocates a
      ~90 KB `Mesh` every frame.

- [ ] `examples/custom_widget.rs:85` — `ui.intern(self.value.to_string())`
      carries the comment "no lingering `String` alloc". `to_string()` allocates
      every frame and the intern then copies it.

- [ ] `src/bin/showcase/pages/overlays.rs:246` — the comment "Static strings
      only — no per-frame alloc" sits forty lines below `:206`, which allocates
      five.

- [ ] `src/bin/showcase/shell.rs:41` — `Body`'s doc says "the two that own
      cross-frame resources"; the enum has three such variants, and two further
      pages need the same thing and use `thread_local!` statics instead.

### Doc blocks attached to the wrong item

Same cause, different mechanism: an insertion or reorder separated a comment
from what it describes, and nothing catches it.

- [ ] `src/renderer/backend/text/mod.rs:207` — the ten-line doc beginning
      "Upload this frame's accumulated glyph instances in one belt write…"
      sits on `shaper()` at `:220`, with the actual shaper sentence appended to
      its tail. `flush` at `:224` — which carries the load-bearing "atlas
      uploads share the same submit as the draws that read from them" ordering
      constraint — is undocumented.

- [ ] `src/scene/cascade/paint_rect.rs:1` — two `//!` blocks. The second is
      verbatim from `src/scene/cascade/engine.rs:1` and describes neither this
      file's contents nor its role; a reader landing here is told the wrong
      thing first. It also reaches through a `super::` doc path.

- [ ] `src/renderer/render_buffer/mod.rs:82` — an eight-line block describing
      curve instances, followed by four lines describing icon draws, all
      attached to `icons`. `curves` and `curve_batches` at `:97` have no
      documentation at all.

- [ ] `src/lib.rs:119` — the nine-line block describing the `internals` module
      is glued to the `golden` block and documents `pub mod golden`;
      `pub mod internals` at `:133` has no doc. Separately, the comment
      explaining the `Animatable` same-name re-export (`:163`) sits above
      `pub use diagnostics::DebugOverlayConfig;`, ~45 lines from its subject.

- [ ] `src/text/bench.rs:45` — the block beginning "The face every arm shapes
      in, stated once…" is concatenated with `LEADING_RATIO`'s own paragraph
      into one doc comment on `LEADING_RATIO`, leaving `UI_FACE` undocumented
      and `LEADING_RATIO` reading as a `TestShape`.

- [ ] `src/primitives/color/mod.rs:216` and `:238` — `ColorU8` carries two doc
      blocks, one before `#[repr(C)]` and one between the derive and the struct,
      both saying the same thing.

- [ ] `src/renderer/frontend/encoder/collision_overlay.rs:33` — `emit`'s doc
      restates the module doc at `:1` almost verbatim, and the function opens
      with an `is_empty` early return immediately before a `for` loop over the
      same collection (`:40`).

- [ ] `src/renderer/backend/mod.rs:1` — the module a reader enters the backend
      through has no `//!` at all, which is why its orientation material ended
      up as ~90 lines of inline comments inside `submit`. Five
      `raster_atlas` satellites are also undocumented (`side.rs`,
      `clock_sweep.rs`, `atlas_slot.rs`, `content_type.rs`,
      `packed_metadata.rs`), as are all six files in `src/icons/` and
      `src/text/wrap.rs`.
---

## Abstractions extracted halfway

A shared core was factored out and the surface around it was not, so each tenant
re-implements the same wrapper. Distinct from plain duplication: the right
seam exists and stops one layer short.

Investigated in full and **closed**. What was real is fixed: the `AnyTyped`
container written twice, `emit_text_chunks` shadowing `TextChunk::split`, the
press path walking the hit table twice, four sweeps with three emptiness
guards, `FillAxis::scaled` open-coding the slow form on a per-quad path, eight
byte-identical `background()` builders, three inline text-emission chains (and
the `Shape::text` signature underneath them), the `Corners`/`Spacing` wrapper
surface, the `Spacing` NaN gap, and `lower::`'s two conventions.

The rest were rejected with evidence:

- **`PerGroupBatch` is not bypassed.** `pending_at` is generic over both batch
  types and called with both; `drain_text_batches` drains on a range predicate
  because each text batch also needs its own scissor, damage intersection and
  mask chain.
- **The four rect pipelines already share their seam** — all end in
  `urect_from_phys`, and the mesh path's "shared scaler" is `Rect::scaled_by`,
  exactly as its comment says. Transform, snap and fringe are per-tier policy.
- **`resolve_container`'s "four spellings" are `Option::unwrap_or`.** Tooltip
  and Modal borrow, `ContextMenu` needs an owned value because
  `Popup::background` takes ownership, `Frame` has no theme slot. The only
  thing `resolve_container` adds is the clip default, which none of them has.
- **`toggle_row`'s seam is where its doc says it is.** The three toggles read
  three *different* theme slots with different fields, and the reads must
  happen before the `&mut Ui` reborrow. What survives above it is the
  three-line preamble below, not toggle-specific logic.
- **The nine eager preambles are three lines, and a helper saves one.**
  `let mut widget` / `response` / `id` — the last two both derive from the
  first, and `id` must be bound before `widget` is mutably borrowed. A named
  result struct would trade nine lines for a concept. Worth doing only to
  *name* the eager path the way `Widget::show` names the lazy one, which is a
  documentation change.
- **The stack/grid fill solvers diverge on purpose.** `freeze_distribute`
  freezes every violator per pass, grid freezes the first found, and
  `cross_driver_tests/fill_solvers.rs` pins the difference. Unifying them
  changes how `Sizing::fill` resolves for users once two clamps are violated
  at once — a product decision, not a refactor.

---

## One fact with several owners

State that is copied rather than referenced, so the copies can disagree and
usually need an assert or a reset protocol to keep them from doing so.

- [ ] `src/renderer/frontend/composer/mod.rs:60` and `:75` — the active clip is
      stored twice. `set_clip` is called immediately after every `clip_stack`
      push/pop with the top frame's values, so `current_scissor` is a cached
      projection of the stack. The two are then read inconsistently:
      `cull_bounds` and the clear-fold test consult `current_scissor`, while
      `ComposeSession::text` (`session.rs:871`) reaches into `clip_stack.last()`
      for the same value.

- [ ] `src/scene/seen_ids.rs:109`, `:113`, `:133` and
      `src/scene/cascade/mod.rs:211` — "which widgets exist this frame" has four
      owners with three lifetimes: `curr`, `prev` (whose own doc says "Only the
      keys matter — values are stale across frames", so it stores an unread
      `Endpoint` per widget per frame purely to keep a `mem::swap` alloc-free),
      `discarded`, and `Cascade::by_id`, produced by an O(N) `clone_from` on
      every full rebuild.

- [ ] `src/ui/frame_cycle.rs:82` — one `FrameStamp` is fanned into
      `FrameRuntime::time`, `Ui::display`, `InputState::frame_time`, and
      `FrameRuntime::prev_stamp`, which after the frame holds a byte-identical
      copy of the first two. `first_frame` is derived twice from
      `prev_stamp.is_none()` (`:81` and `src/ui/frame.rs:249`), and `FrameStamp`
      itself is both the host's per-frame input and the retained prior-frame
      record.

- [ ] `src/layout/grid/mod.rs:61`, `:177`, `src/layout/grid/resolving.rs:16` —
      grid track state has three simultaneous owners plus a fourth in the
      snapshot: `AxisScratch` per nesting depth, `GridTrackStore` with a
      duplicate `sizes_pool`, and the measure cache's packed copy. The durable
      store exists only because the depth-stack scratch is clobbered by sibling
      grids before arrange, so `resolve_or_reuse` compares against a recorded
      `total` to decide whether to copy sizes back into the scratch that
      computed them. Its own doc admits the type outgrew its name.

- [ ] `src/widgets/scroll/mod.rs:42` — `ScrollGeometry` carries `content` and
      `space.bar_viewport`, then embeds a `ScrollBounds` whose `content` and
      `viewport` are copies of those same two values, so `geom.content` and
      `geom.bounds.content` are one number stored twice in one `Copy` struct.

- [ ] `src/host/winit/window.rs:112`, `src/host/window_driver/mod.rs:96` and
      `:433`, `src/host/offscreen.rs:203` — the surface's physical size lives in
      three places (`Window::config`, `WindowDriver::target.physical`,
      `Ui::display.physical`), and `render_to_texture` spends two
      `debug_assert!`s checking they still agree; `TargetKey::describes` exists
      solely to serve the second. `pixel_snap` has the same shape: the
      authoritative value is on `WindowDriver`, but `Display::pixel_snap`
      defaults to `true`, so a host that forgets to splice it back silently gets
      snapping regardless of `OffscreenHostBuilder::pixel_snap(false)`.

- [ ] `src/ui/resources.rs:14` and `src/ui/mod.rs:149` — the `TextShaper` handle
      sits on `Ui` twice, as `ui.resources.text` (used by `probe_text`) and
      `ui.layout_engine.text.shaper` (used by measure). `FrameCycle` then ticks
      the shared text clock by reaching two levels into another subsystem
      (`src/ui/frame_cycle.rs:111`, `:370`) for state that belongs to
      `UiResources`.

- [ ] `src/ui/mod.rs:119` and `:121` — `Ui` is used as a two-way mailbox for
      host state. `window_frame` is never written by the recorder — the winit
      host assigns the whole struct each frame and the driver resets it to
      `default()` after drain — while `window_requests` is appended by the
      recorder and cleared by the host at two different points
      (`src/host/winit/window.rs:98`, `src/host/window_driver/mod.rs:381`). The
      host's own `Window::close_requested` is a third copy of the same bool.

- [ ] `src/host/winit/gpu.rs:130` and `src/host/core.rs:47` — both derive
      `max_texture_dimension_2d` from the same device with a character-identical
      panic message, and `SurfaceManager`'s field doc explains the caching as if
      it were the only copy.

- [ ] `src/host/winit/window.rs:98`, `src/host/window_driver/mod.rs:382` and
      `:416` — `close_vetoed` is cleared in three places. Since `Window::frame`
      always reaches `finish`, the drain clears it at the end of every winit
      frame, making the pre-frame clear unreachable-by-effect. The veto's
      one-frame lifetime is stated in three files and enforced by none.

- [ ] `src/renderer/backend/mod.rs:86` and `:101` — `Backbuffer` and `Stencil`
      are declared with private fields by the backend, which provides the only
      constructors and stores neither: `WindowDriver` holds the `Option`s and
      hands `&mut` back in. The seam shows at
      `src/host/window_driver/mod.rs:469`, an infallible `expect` placed
      immediately after the call whose only job was to make it infallible, and
      in `ensure_backbuffer`'s bare `-> bool` that the host decodes into a
      `debug_assert` about plan escalation.

- [ ] `src/widgets/text_edit/edit_state.rs:104` and
      `src/widgets/text_edit/editor.rs:51` — two copies of the
      "did the host replace the buffer" rule, body for body, one fed the probe's
      hash and one recomputing `hash::hash_str` over the whole buffer. Three
      pieces of state exist only to keep them from fighting (`expected_hash`,
      `local_edit_pending`, `history_checked`), and the two hashes are minted
      differently — `TextShapeKey::content_hash` maps `0 → 1`, the raw one does
      not — so they are only usually equal.

- [ ] `src/renderer/render_buffer/mod.rs:167` — `time` is written twice per
      frame: `start_frame` sets `Duration::ZERO` with a comment explaining this
      is not the real value, and `Frontend::build` stamps it unconditionally
      after compose (`src/renderer/frontend/mod.rs:98`). The field is
      meaningless for the whole compose pass.

- [ ] `src/input/mod.rs:1009` vs `:1143` — `pointer_local_for` recomputes the
      three lookups whose result `response_for` already stored in
      `ResponseState::pointer_local`.

- [ ] `src/renderer/frontend/payload.rs:481` — `DrawImagePayload.gpu_view` is
      set two lines before `is_noop` reads it and is read nowhere else;
      `ComposeSession::image` branches on the `paint: Option<&GpuPaintRef>`
      argument and ignores the flag. The same fact travels on two channels into
      one call, one of them a field on every `DrawImagePayload`, its
      `PartialEq`, and the capture enum.

- [ ] `src/renderer/gradient_atlas/mod.rs:139` — five per-row columns
      (`rows`, `baked`, `row_epoch`, and `MruList`'s `prev`/`next`) that must
      always agree, with `capacity()` answering from one, `grow()` resizing each
      by hand, and a comment admitting "Independent resizes make equal lengths a
      convention rather than a type invariant" above the `debug_assert!` that
      papers over it. `rows` additionally stores a second copy of every live
      `GradientLutKey` so eviction can find the outgoing key.

- [ ] `src/renderer/gpu_view.rs:154` vs `src/widgets/gpu_view.rs:49` —
      `GpuPaintRef` exists so carriers can keep `derive(Debug)`, but `GpuView`
      stores the raw `Rc<RefCell<dyn GpuPaint>>` and therefore hand-writes the
      very `Debug` impl the wrapper was introduced to eliminate; `Ui::gpu_view`
      also takes the raw type and wraps it at two match arms.

---

## Cost the stated posture forbids

`CLAUDE.md` makes steady-state heap-allocation-free a hard property and puts
release asserts off per-frame paths. These sites contradict it.

- [ ] `src/widgets/text_edit/edit_state.rs:19` — undo history is
      `VecDeque<EditDelta>` where `EditDelta` owns two `String`s, up to 128
      entries × 2 heap blocks. Worse, `replace_range`
      (`src/widgets/text_edit/editor.rs:107`) unconditionally builds both
      Strings and *then* hands the delta to `push_delta`, which in the common
      typing case coalesces it into `undo.back_mut()` and drops them — so each
      keystroke allocates and frees two Strings, in a widget whose steady state
      is pinned alloc-free by `tests/alloc/fixtures/widgets.rs`.

- [ ] `src/widgets/text_edit/view.rs:26` — `selection_rects:
      Option<Box<TinyVec<[Rect; 16]>>>` requires the caller to `take()` the box,
      build a fresh 256-byte stack `TinyVec` as fallback, pick between them,
      thread the reference through two functions, then decide whether to promote
      — fifteen lines of storage plumbing, three levels of indirection, a fresh
      default-initialised array every frame, and a promotion that never demotes,
      to express a retained scratch buffer.

- [ ] `src/ui/frame_stats.rs:18` — the overlay builds three `String`s with
      `format!` per record pass (six on a double-layout frame) and then interns
      the result, copying it again. `Ui::fmt(format_args!(…))`
      (`src/ui/mod.rs:657`) exists precisely to format into the arena directly,
      and `frame_fixture` uses it throughout.

- [ ] `src/widgets/tooltip/mod.rs:171` and `:225` — four `state_mut` operations
      every frame for every trigger with a tooltip attached, even when the
      pointer is nowhere near it; `state_mut` is `get_or_insert_with`, so the
      read-only probe materialises a row for every such trigger, and
      `global_state_id()` re-hashes a key string on each call.
      `src/widgets/combo_box/mod.rs:125` does two round-trips per frame for one
      bool. `ContextMenu::show` (`context_menu/mod.rs:131`) documents the
      cheaper `try_state` pattern in detail — the crate has it and two widgets
      do not use it.

- [ ] `src/widgets/text_edit/mod.rs:291`, `:314`, `:435` and
      `src/widgets/text_edit/input.rs:85` — `Ui::response_for` (a cascade lookup
      plus a layout lookup plus a scratch read) is called four times for the
      same id in one frame with nothing between that can change the answer;
      `Scroll` does it twice for its own id on top of the four `Bars::read`
      performs.

- [ ] `src/scene/damage/walk.rs:208` — `classify` probes `prev.get(&widget_id)`
      and returns a `Copy` tier; every non-skip arm then hashes the same key
      again (`insert` at `:247`, `get_mut` at `:259`, `remove` at `:271`).
      `on_subtree_moved` (`:359`, `:367`) does `get` then `get_mut` per
      descendant just to write one field, so the scroll/pan hot path pays three
      probes per moved node.

- [ ] `src/scene/seen_ids.rs:149` — `pre_record` extends `discarded` with every
      key in `curr`, so every action- or relayout-triggered second record pass
      fills a hash set with one entry per widget and then probes `curr` for each.
      An id recorded in pass A that survived from last frame is already caught by
      the prev-minus-curr diff; `discarded` only adds value for ids created and
      destroyed inside one frame.

- [ ] `src/renderer/gradient_atlas/bake.rs:35` — `t` increases monotonically
      across the 256-texel loop, but `lerp_at` restarts its segment scan at
      `upper = 1` on every call and re-reads the first and last stop offsets each
      time. The module doc (`gradient_atlas/mod.rs:28`) makes a point of hoisting
      the linear decode out of this exact loop; the same reasoning was not
      applied to the search. `oklab_stops` is also built and passed for
      `Interp::Linear` bakes that never read it.

- [ ] `src/layout/grid/mod.rs:440` and `src/layout/grid/measuring.rs:61` — every
      non-Fixed grid measure walks `active_children` folding per-track intrinsic
      ranges, once via the `intrinsic_min` query at `src/layout/pass.rs:247` and
      again in `measure_inner`, into two different destinations with the track
      clamp applied at different points.

- [ ] `src/icons/icon_rasterizer.rs:38` — `trees: FxHashMap<IconRef,
      Option<usvg::Tree>>` accumulates one parsed document per icon ever
      rasterized and is pruned only by `forget_sets`. Every neighbouring cache is
      bounded by a frame window (`PROBATION_KEEP_FRAMES`, the encoded-run cache,
      the atlas clock sweep); this one has no ageing at all.

- [ ] `src/icons/icon_raster_key.rs:50`, `:55` and
      `src/renderer/frontend/composer/session.rs:475` — `F32Ext::fast_round`
      (`src/primitives/num.rs:12`) exists because "baseline x86-64 has no
      `roundss` (SSE4.1), so `.round()` compiles to an out-of-line `roundf` call
      in the per-quad snap and pixel-alignment paths", and `Rect::scaled_by`,
      `snap_text_scale`, `text/key.rs` and the text encoder all use it. The icon
      tier — the newest — bypasses it: `ComposeSession::icon` runs per icon per
      frame and calls `IconRasterKey::for_box` (two `.round()`s) then
      `centred.round()` (two more), so four out-of-line `roundf` calls per icon
      per frame. `src/layout/scrollbars/mod.rs:162` and `src/ui/mod.rs:509` are
      on the same footing.

- [ ] `src/primitives/transform.rs:54` — `TranslateScale::new` carries two
      release `assert!`s, and `compose`, `from_translate_scale_about` and
      `anchored_at` all funnel through it, so
      `parent_transform.compose(t.anchored_at(…))`
      (`src/scene/cascade/engine.rs:392`) pays them twice per transformed node
      per frame. The comment at `engine.rs:379` prices the call as "3×mul+3×add",
      which is what it would be without them.

- [ ] `src/shape/polyline.rs:72` — `is_noop`, a query, opens by calling
      `PolylineColors::assert_matches`, a release `assert_eq!` run for every
      polyline every frame. The comment concedes the placement was chosen by call
      order: "asserted here because this is the first thing `Shapes::add` calls."

- [ ] `src/bin/showcase/pages/pan_zoom.rs:262` — a `String` per cell per frame:
      576 cells in `Content::Grid`, 960 across the two `cell_grid` calls in
      `Content::Document` — the page its own module doc advertises as the
      benchmarking workload. `canvas_polylines` (`:329`) additionally `collect()`s
      six fixed-length `Vec<Vec2>` per frame, and
      `src/bin/showcase/pages/text_edit.rs:151` allocates a `String` only to hash
      it into a `WidgetId` on the next line, nine times per frame.

- [ ] `src/bin/showcase/pages/shapes.rs:163` — `stress` allocates
      `Mesh::with_capacity(2500, 14406)` (~90 KB) every frame it is on screen,
      fills it, discards it, and re-runs `content_hash` over all 2500 vertices
      because the fresh mesh's `cached_hash` is always cold. `Mesh` documents
      itself as built for retention.

- [ ] `src/bin/showcase/pages/dialogs.rs:50` and fifteen further sites in the
      same file — the two-bool page state is re-probed out of `Ui::state_mut`
      sixteen times per frame, each a fresh hash-map lookup.

---

## Library API gaps that push boilerplate into every app

Found from the call site: the showcase and examples repeat the same
workarounds, which makes these library-side findings rather than app ones.

- [ ] `src/ui/mod.rs:797` — `Ui::state_mut` lends out of `&mut Ui`, so the
      borrow cannot survive the widget calls that read it. Eight showcase pages
      consequently `std::mem::take` the row out and write it back at the end of
      `build`, or re-probe per field access; `src/bin/showcase/pages/text_edit.rs:5`
      documents the workaround as if it were a design. There is no page- or
      subtree-scoped state carrier between "app root struct" and "per-widget
      row".

- [ ] `src/ui/mod.rs:524` — `Ui::debug_overlay_mut` hands out a `RefMut` that
      cannot survive `&mut Ui` widget calls, and has no read-only accessor to
      pair with a setter, so `src/bin/showcase/shell.rs:451` takes three separate
      borrows to copy three bools onto the stack and re-borrows to write them
      back.

- [ ] `src/ui/mod.rs:364` — `Ui::set_vsync` queues a one-shot request that
      nothing reads back, so apps mirror it (`src/bin/showcase/shell.rs:239`) to
      avoid recreating the swapchain. The neighbouring `Ui::window_open`
      (`:528`) is documented as the source of truth precisely "instead of
      mirroring the state in app code" — two adjacent capabilities taking
      opposite positions.

- [ ] Widget text setters take `impl Into<TextInput>`, so `format!(…)` compiles
      and reads naturally while the non-allocating `ui.fmt(format_args!(…))` is
      a separate two-step call nothing steers toward. Both examples
      (`examples/counter.rs:23`, `examples/custom_widget.rs:85`) — the crate's
      teaching surface, one of them the README doctest — use the allocating
      form.

- [ ] `src/widgets/context_menu/mod.rs` — `ContextMenu::style` and
      `MenuItem::style` accept only `&Theme`, so "styled or default" cannot be
      expressed as data and becomes control flow at every row:
      `src/bin/showcase/pages/overlays.rs:310` builds an `Option`, branches for
      the panel, defines a closure whose only job is the match, and then writes
      the separator twice under a `match` because `#[track_caller]` cannot reach
      through a closure body.

- [ ] `src/scene/node/mod.rs:391` — three id mechanisms (implicit `Salt::Auto`
      from `Node::new`, explicit `.auto_id()`, `.id_salt(…)`) with no call-site
      signal for which a given widget needs. `Node::new` already stores
      `Salt::Auto(WidgetId::auto_stable())` and every builder constructor is
      `#[track_caller]`, so the ~20 `.auto_id()` calls in the showcase are
      ceremony; the case where it matters occurs nowhere in it.

- [ ] `src/widgets/panel/mod.rs:55` and `src/widgets/grid.rs:98` — `Configure`
      exposes ~25 node setters but not `transform`, so `Panel::transform` and
      `Grid::transform` are two copies of `self.node.transform = t` with a
      cross-referencing doc. Inside `show()` bodies the crate then mixes both
      styles freely, writing `bar.margin`, `node.gaps`, `child_align` and
      `justify` as raw fields on nodes that implement `Configure`
      (`splitter/mod.rs:204`, `toggle.rs:74`, `combo_box/mod.rs:83`,
      `context_menu/mod.rs:317`, `slider.rs:95`, `separator.rs:87`).

- [ ] `src/widgets/theme/mod.rs:226` — `Theme::text_scale` stores font sizes
      already multiplied by the scale *plus* the scale factor, so the two can
      desync. The field is therefore private, needs a bespoke
      `deserialize_text_scale` (`theme/serde.rs:38`), and `set_text_scale` must
      compute a ratio, pre-validate every metric, then walk the theme tree twice
      — a walk whose sole purpose is ten hand-written destructuring visitors
      across ten files, each re-listing every field with `_` bindings, plus a
      runtime backstop test for what destructuring cannot cover. One caller.

- [ ] `src/widgets/checkbox/mod.rs:74` and `:87` (and the same shape in
      `radio/mod.rs`, `switch.rs`, `context_menu/mod.rs`, `drag_value/mod.rs`) —
      every themed widget names its theme slot twice, once to copy out geometry
      scalars and once in the `WidgetTheme::resolve` fallback closure, which
      re-reads `ui.theme()` and re-runs the same `unwrap_or_else`. A mistyped
      slot in one of the two halves is silent. All 19 `style()` builders across
      the crate are byte-identical bodies with near-identical docs.

- [ ] `src/widgets/gpu_view.rs:78` — `repaint(false)`, the "save work" knob, is
      what destroys the framework-owned target: retention is keyed to appearing
      in this submit's `frame_targets`, which conflates "unchanged" with "gone".
      A frame forced by any other widget frees the off-screen texture and the
      next repaint calls `GpuPaint::init` again — which is why the public trait's
      `init` cannot promise once-per-view and its doc has to instruct every
      implementor to guard against re-entry.

- [ ] `src/bin/showcase/support.rs:166` — `section` takes an id argument that
      restates its title in 59 of 62 call sites; `row` and `tiles` (`:199`,
      `:209`) take `"<section>-tiles"` string literals at all 46. All three are
      generic over `<H: Hash + Copy>` with a single instantiation. `demo_cell`
      and `note` (`:190`, `:261`) key widget identity on display copy, so editing
      a caption resets that cell's state.

---

## Files organised by topic rather than by owning type

The stated convention is one major struct per file, named after it, with impls
in that file. It is followed in most of the crate and abandoned in a consistent
set of places — usually a `mod.rs` or a `*_utils`/`support` file that became a
bag.

- [ ] `src/renderer/backend/mod.rs:149` — 1168 lines holding `WgpuBackend` (which
      owns nine subsystems and every render pass) plus six independent types
      (`Backbuffer`, `Stencil`, `SubmissionTargets`, `Submission`,
      `BackendConfig`, `BackendResources`), none a satellite of the others, and
      `submit` at 295 lines.

- [ ] `src/renderer/frontend/payload.rs:102` — eight standalone payload types
      plus `BrushSource`/`GpuFillFields`/`ResolvedGradient`, each with its own
      `is_noop` and constructors. The consequence is already visible:
      `impl DrawIconPayload` (`:501`) sits between the `DrawImagePayload` struct
      (`:455`) and its own impl (`:511`).

- [ ] `src/window.rs:30` — eleven top-level types (`WindowToken`,
      `WindowDirectory`, `WindowConfig`, `WindowGeometry`, `CursorIcon`, `Vsync`,
      `PendingWindow`, `WindowCommands`, `WindowRequests`, `WindowOutput`,
      `WindowFrameState`) and no `Window` struct.

- [ ] `src/widgets/text_edit/view.rs:41` — twelve types (`ViewState`,
      `InteractionState`, `ShapeCtx`, `LayoutInput`, `TextLayout`, `Probed`,
      `GeometryInput`, `TextGeometry`, `ViewUpdateInput`, `ViewUpdate`,
      `CaretPaint`, `PaintInput`) plus six free functions. The sharpest case is
      `InteractionState` (`:49`), a one-field drag anchor — pointer state, not
      view state — whose `normalize` is a byte-offset repair identical in
      schedule to `EditState::normalize`; `input.rs` calls the two as a pair at
      three separate points.

- [ ] `src/input/mod.rs:47` — 1153 lines holding `Capture`, `Press`, `PressDrag`,
      `Release`, `ReleaseKind`, `PressRun`, `EventOutcome`, `TargetScrollDelta`,
      `InputEvent` and `InputState`. `InputEvent` is the crate's public
      host-facing event vocabulary and the direct sibling of `PointerEvent` and
      `KeyboardEvent`, both of which have their own files. The press-capture
      cluster is a self-contained state machine.

- [ ] `src/ui/frame.rs:23` — seven types (`WakeReasons`, `FrameStamp`,
      `FrameInput`, `FrameClassifyInput`, `Wake`, `FrameRuntime`, `FramePlan`)
      and no `Frame`.

- [ ] `src/primitives/interned_str.rs:26` — six types (`InternedStr`,
      `TextEpoch`, `TextInput`, `InternedText`, `TextSource`, `RecordedText`).
      Within it, `TextSource` is a `repr(transparent)` one-field wrapper over
      `Span` whose only method indexes that span, and `RecordedText::resolve`
      does nothing but forward to it.

- [ ] `src/scene/record_store.rs:32` — six types, of which `RecordedGradients` is
      an independent interner with its own lifecycle, in a file named for
      `RecordStore`.

- [ ] `src/scene/node/columns.rs:22` — six independent types (`Gaps`,
      `BoundsExtras`, `PanelExtras`, `LayoutCore`, `NodeFlags`, `NodeColumns`),
      five of them substantial and each pinned individually in `lib.rs`'s
      `hot_struct_sizes` table, i.e. treated as first-class layout-critical types
      everywhere else.

- [ ] `src/widgets/context_menu/mod.rs:222` and `:385` — three public widget
      types (`ContextMenu`, `MenuItem`, `MenuSeparator`) plus two response/state
      types and `MenuShortcut`, in a directory module that already has a
      `tests.rs` sibling.

- [ ] `src/layout/support.rs:201` — five structs and twelve free functions
      spanning four unrelated jobs (text-run extraction, axis size resolution,
      justify distribution, alignment placement). Several take their own type as
      the sole meaningful argument where a method would do. `AxisAlignPair`
      (`:348`) is the only struct in the module without `#[derive(Debug)]`,
      escaping the crate's `missing_debug_implementations = "deny"` only because
      it is `pub(super)`.

- [ ] `src/layout/engine.rs:78` — the file named for `LayoutEngine` also defines
      `LayoutScratch` (its own major struct with its own lifecycle doc), the
      `NO_ARRANGE_SRC` sentinel, and `resolve_sizing` (`:238`), the whole
      per-node sizing pipeline. `cache_rebuild` (`:145`) is per-frame state on
      the persistent engine, then threaded into `restore_after_cache_hit` as a
      bare `bool`.

- [ ] `src/renderer/gpu_view.rs` — defines no `GpuView`; the struct of that name
      is in `src/widgets/gpu_view.rs`. The crate has two `gpu_view.rs` files and
      the one that is not the widget holds the trait, two ctx structs,
      `GpuPaintRef` and `GpuViewEntry`.

- [ ] `src/renderer/backend/stencil.rs:19` — holds `STENCIL_FORMAT` and
      `stencil_test_state()` but not `struct Stencil`, which is in
      `src/renderer/backend/mod.rs:101`. "The stencil" is two files and neither
      is named for the other's half.

- [ ] `src/renderer/backend/pipeline_utils.rs:76` — a grab-bag of one real struct
      (`StencilVariant`), two param bundles and five free functions.
      `raster_atlas/quad.rs:18` defines `RasterQuad` plus six `pub(crate)` free
      functions that are the natural surface of the type or of `RasterAtlas`.

- [ ] `src/renderer/gradient_atlas/handle.rs:14`, `mru.rs:40`,
      `src/renderer/plan.rs:22`, `src/renderer/render_owner.rs:6`,
      `src/diagnostics/gpu_stats.rs:98`, `src/primitives/transform.rs:16`,
      `src/primitives/fill_wire.rs:30` — files named for a topic holding a single
      type named something else (`SharedGradientAtlas`, `MruList`, `RenderPlan`,
      `RenderOwnerId`, `GpuPassStats`, `TranslateScale`; `fill_wire.rs` names
      neither of its two). `handle.rs` is the generic-filename shape the
      convention exists to prevent.

- [ ] `src/renderer/render_buffer/batch.rs:7` — four independent types
      (`DrawGroup`, `TextBatch`, `GroupBatch`, `PaintTier`), none named `Batch`.

- [ ] `src/widgets/chrome.rs:19` and `src/widgets/toggle.rs:25` — neither holds a
      struct of its name; both are files of `pub(super)` free functions, which
      also cuts against the crate's method-over-free-function preference.

- [ ] `src/text/cosmic/mod.rs:251`, `retention.rs:60`, `truncate.rs:160`,
      `glyphs.rs:17` — `CosmicMeasure`'s inherent impl is spread over four files
      reaching its `pub(super)` fields directly. The cost is documented in the
      code: `retention.rs:9` says "reading any one of them alone is how the
      ticket-leak regression got written."

- [ ] `src/scene/tree/recording.rs:94` — `Placement` is not recording state: it
      carries `available(surface)` and `origin(measured, surface)`, whose callers
      are `src/layout/engine.rs:120` and `:523`, and it is read by
      `cascade_fingerprint`. A layout-policy type with layout math sits in the
      scene recorder's scratch file, in a file where no type matches the name.

- [ ] `src/scene/tree/rollups.rs:31` — `SubtreeRollups` holds two per-node hash
      columns, one whole-tree authoring hash, one count fingerprint, and
      `container_text: FixedBitSet` (a layout worklist). Only the first two are
      subtree rollups, and `reset_for` resets four of five fields, leaving
      `cascade_static` to be overwritten later.

- [ ] `src/scene/shapes/lower.rs:43` — `ChromeInput` is a parameter bundle for
      `Tree::open_node`, declared in a module of lowering functions that never
      mentions it; it is constructed at `src/scene/forest.rs:215` and
      destructured at `src/scene/tree/mod.rs:408`.

- [ ] `src/text/key.rs:10` — `TEXT_METRICS_ERROR` and `text_metrics_valid` are a
      shared validation predicate used by shape recording, theme scaling and
      theme deserialization; four of the five call sites are outside `src/text`
      entirely. They live in the file named for `TextShapeKey`, which uses them
      once in a `debug_assert`.

- [ ] `src/animation/spring.rs:251` — `DURATION_SNAP_EPS_SQ` and
      `within_duration_snap_eps` are the *duration* motion model's snap floor, in
      a file documented as "Damped spring step", with a comment existing only to
      explain why they are there. `src/animation/serde.rs:8` holds `AnimSpec`'s
      serde impls, where the convention puts a foreign-trait impl in the file of
      the type it is for.

- [ ] `src/input/sense.rs:99`, `:107`, `:113` — `DRAG_THRESHOLD`,
      `DOUBLE_CLICK_WINDOW` and `DOUBLE_CLICK_RADIUS` are press-run and drag-latch
      tunables read only by the capture state machine, parked in the bitflags
      file that describes which interactions a widget participates in. Two are
      `pub(crate)` and one `pub(super)` despite identical reach.

- [ ] `src/layout/grid/{measuring,resolving,arranging}.rs` — the split is not a
      layering: `measuring` imports from `resolving` *and* `arranging`, while
      `resolving` imports `resolve_fixed` back from `measuring`. `resolve_fixed`
      is documented as "Phase 1 of `resolve_axis`" but lives in the measure file
      because `measure_inner` also calls it standalone, which is what creates the
      cycle. No file can be read or changed independently.

---

## Dead, speculative, and single-caller surface

The convention is to remove unused code, or say why it is kept and silence the
warning. Several of these do the second half without the first being true.

- [ ] `src/primitives/urect/mod.rs:168` — 14 functions across ~140 lines of a
      314-line file, behind a blanket `#[allow(dead_code)]`, whose own doc says
      "Nothing calls these yet, which is the whole reason they are gathered
      here". The blanket allow also means any future method added there goes
      unchecked.

- [ ] `src/primitives/urect/mod.rs:240` — `URect16`, its `Hash`, `new`,
      `to_urect` and both `From` conversions have zero references outside their
      own definitions. It is also a second major struct in a file named for the
      first, and breaks that file's accessor vocabulary (`min`/`size` vs bare
      `x`/`y`/`w`/`h`).

- [ ] Trait impls with no callers, invisible to the dead-code lint:
      `impl From<ShapeStroke> for Stroke` (`src/scene/shapes/paint.rs:86` — every
      real conversion goes the other way), `impl Default for CurveBasis`
      (`src/shape/curve.rs:64`), and both `IntoIterator` impls for `PerLayer`
      (`src/scene/layer.rs:109`, `:117`).

- [ ] `src/primitives/nan.rs:30`, `:46`, `:61` — `NanCheck` never appears as a
      bound outside its own file; all 42 `.has_nan()` call sites are concrete
      method calls, so the `f32`, `Option<T>` and `[T]` impls are unreachable.
      The trait also duplicates the NaN half of every type's `is_noop`:
      `Shadow::is_noop` and `Shadow::has_nan` walk the identical four fields, as
      do the `Color` and `Rect` pairs.

- [ ] `src/primitives/approx.rs:41` — two eight-function hash families identical
      apart from `eq_bits` vs `canon_bits`. `hash_vec2` (`:47`) has zero callers;
      `hash_size` and `hash_rect` have one each.

- [ ] `src/primitives/size.rs:45` (`is_inf`), `src/primitives/rect/mod.rs:103`
      (`approx_zero`) — zero callers anywhere including tests.
      `Color::midpoint` (`color/mod.rs:134`) and `Mesh::append`
      (`mesh.rs:175`) are reached only from their own test modules.

- [ ] `src/widgets/theme/mod.rs:175` — `Theme::menu_button` is a full second
      `ButtonTheme` that no `show()` resolves against; it is reachable only if an
      app writes `Button::style(&theme.menu_button)` by hand. It still costs a
      theme field, a `from_palette` line, a `for_each_text` line and full serde
      surface, and `ButtonTheme::menu_button(p)` already exists as a constructor
      producing the same value.

- [ ] `src/widgets/theme/widget_look/stateful_look.rs:58` →
      `context_menu/menu_item.rs:68` → `context_menu/mod.rs:72` — a three-level
      `pub` chain, each level existing for exactly one caller, the top of which
      is called from nowhere outside `context_menu/tests.rs`. All three are
      re-exported from `lib.rs` with multi-sentence docs.

- [ ] `src/lib.rs:272` — `FrameProcessing` is exported but has no non-test
      consumer; its own doc and `FrameReport::processing`'s doc both say it is
      "informational, used by tests / benches / profilers".

- [ ] `src/input/shortcut.rs` — `Mods::ALT`, `Mods::SHIFT` and
      `Shortcut::ctrl_alt` have zero references anywhere in the repo, tests
      included.

- [ ] `src/scene/cascade/mod.rs:292` — `hit_test_targets` is generic over three
      independent `impl Fn(Sense) -> bool` closures, and every non-test caller
      passes exactly `Sense::hovers`, `Sense::scrolls`, `Sense::pinches`. The
      comment defends the arms on monomorphisation grounds; there is one
      instantiation.

- [ ] `src/renderer/backend/schedule/mod.rs:186` — `for_each_step` is generic
      over `impl FnMut(RenderStep)` and immediately stores `&mut emit` as
      `&mut dyn FnMut` (`:338`), so every push goes through a vtable anyway. The
      generic costs a monomorphisation per caller plus a hand-written `Debug`
      (`:346`) whose only reason to exist is that the `dyn` field has nothing to
      format.

- [ ] `src/renderer/frontend/paint_sink.rs:82` — `PaintSink` carries a doubled
      method surface (an ungated `quad`/`text`/… and a provided `draw_*` gate)
      for a single production implementor. Four gates have byte-identical
      bodies, `draw_text` breaks the pattern by taking three loose arguments,
      and the gate is documented as unenforceable because the ungated half is
      crate-visible. The encoder still pays `&mut dyn PaintSink` dispatch per
      paint call for a trait whose second implementor is test-only.

- [ ] `src/renderer/backend/queue.rs:32` — a `Deref` newtype and a second module
      exist to tally `write_texture`, which has exactly two call sites in the
      crate (`gpu_gradient_atlas.rs:159`, `image_pipeline/textures.rs:333`), yet
      `&Queue` is threaded through `GpuCtx` and every uploader.

- [ ] `src/renderer/backend/image_pipeline/mod.rs:130`, `:152`, `:170`, `:177` —
      four of nine methods are one-line forwarders carrying 10–20 lines of doc
      apiece duplicating the delegate's. `retire_render_owner` is a three-hop
      chain, each hop one statement, each repeating the same `cfg_attr` and its
      own justification comment.

- [ ] `src/renderer/backend/overlay_pass.rs:167` — `upload_overlays` has one
      caller, exists only to copy an `ArrayVec<Rect>` into an
      `ArrayVec<Quad>` differing by four constant fields, and parameterises a
      `stroke_color` that is always `DAMAGE_OVERLAY_COLOR`.

- [ ] `src/layout/cache/mod.rs:346` — `snapshot_subtree` takes a range to mirror
      `restore_subtree`, but every caller passes `index..index + 1`; it then
      re-reads `layouts[i]` and re-matches `LayoutMode` to recover the id the
      caller just discarded, so capture pays a double mode-match per node.

- [ ] `src/scene/shapes/mod.rs:114` — `add_gpu_view` ends with the verbatim body
      of `push` (`:95`) minus the return, so `Forest::add_gpu_view`
      (`src/scene/forest.rs:286`) writes `Some(0)` — a sentinel index that means
      nothing — to satisfy `push_shape`'s protocol. The doc on `Shapes::add`
      calls the index-ignoring path "the legacy 'fire and forget' path", but
      `push_shape` reads it and there is no caller that ignores it.

- [ ] `src/scene/seen_ids.rs:59` and `:209` — four types
      (`Endpoint`, `CollisionRecord`, `PendingExplicitCollision`,
      `EndpointOutcome`), a `pending` queue and a linear `position()` scan exist
      so `record_endpoint` can return a two-variant enum matched on for every
      node opened every frame. The only consumer is `collision_overlay::emit`,
      gated `#[cfg(debug_assertions)]`; the `tracing::error!` at
      `src/scene/forest.rs:242` already carries both endpoints and is what
      survives into release.

- [ ] `src/scene/damage/walk.rs:158` — `subtree_end` re-implements
      `Tree::subtree_end_of` (`src/scene/tree/mod.rs:121`).
      `src/scene/damage/region/mod.rs:249` — `impl From<Rect> for DamageRegion`
      is ungated production code whose only callers are test modules, while its
      near-twin `from_rects` twenty lines above *is* gated.

- [ ] `src/widgets/text_edit/input.rs:257` — `dispatch_action` is a five-line
      `pub(super)` wrapper around two calls with one caller; `:180` has
      `drag_anchor.unwrap_or(hit)` inside a branch already guarded by
      `drag_anchor.is_some()`. Five `unicode.rs` helpers (`:102`, `:127`, `:141`,
      `:175`, `:200`) are `pub(crate)` with no caller outside `text_edit`.

- [ ] `src/widgets/text_edit/view.rs:474` — `measured(input)` branches on
      `text.is_empty()` to choose between `display_size` and `content_size`, but
      `resolve_geometry` sets `display_size` to exactly `measured` whenever the
      text is non-empty, so both arms return the same value. `record` already
      reads `display_size` directly for the same quantity (`:416`) while
      `block_node` goes through the helper (`:523`), and `block_offset` aligns
      from `measured` while the node it describes is sized from `display_size`.

- [ ] `src/widgets/text_edit/mod.rs:526` and `src/widgets/text_edit/input.rs:19`
      — three parallel signal structs relay the same booleans with each hop
      renaming rather than adding (`blur` → `cancelled`, `edited` → `changed`).
      `InputResult::was_focused` is pure pass-through, read from
      `view.prev_focused` and returned to a caller holding the same
      `&mut TextEditState`; the `EditSignals` literal is duplicated at `:361` and
      `:510`.

- [ ] `src/frame_fixture/mod.rs:119` — `FrameFixture::render` is a one-line
      delegate to `pub(crate) fn build_ui(state: &mut FrameFixture, …)`, and both
      are live. A free function whose first parameter is `&mut FrameFixture` is
      the method it wraps.

- [ ] `src/text/shaper.rs:49` — `Metric` is gated to a single variant in
      production, so `ShaperInner::cosmic()` returns an `Option` that is always
      `Some`, `shapes_buffers()` always returns `true`, `TextSystem` caches that
      constant in a field it destructures on every `measure`, and `supersede`,
      `tick_frame` and `glyphs` each carry an `if let Some`/`.expect` for an
      unreachable case. None of it is `cfg`-gated, so a shipping build pays for a
      state only `TextShaper::test_mono` can reach.

- [ ] `src/renderer/frontend/composer/higher_kind.rs:34` — `conflicts`
      hand-writes the tier ordering as four match arms and six disjunctions
      though `PaintTier` derives `Ord` and the module's own test (`:132`) asserts
      the whole matrix is just `incoming < recorded`.
      `Composer::any_higher_kind_overlap` (`composer/mod.rs:356`) is a one-line
      forward with two callers.

- [ ] `src/scene/record_store.rs:112` — `RecordStore` contains nothing but
      `payloads: RefCell<RecordPayloads>` (pinned by a test at `:279`), its four
      methods are borrow-then-delegate one-liners, and `payloads` is
      `pub(crate)` and borrowed directly by callers anyway. Inside it,
      `TextStore.bytes` is a *second* `RefCell`, so `intern_str` takes a shared
      borrow of the outer and a mutable borrow of the inner; `clear` takes
      `&self` and mutates through the cell though its only caller holds
      `&mut self`.

- [ ] `src/scene/record_store.rs:61` — `RecordedGradients` hand-implements
      separate chaining (`heads: FxHashMap<u64, GradientId>`, a `next: Vec<u32>`
      link array, a `GRADIENT_CHAIN_END` sentinel) on top of an `FxHashMap` that
      already resolves collisions with equality confirmation.

---

## Numeric predicates re-derived per call site

The crate owns named predicates and constants for these, and the copies use
different tolerances — so the same question gets different answers depending on
which call site asks it.

- [ ] `src/primitives/color/mod.rs:206`, `:345`, `:366`,
      `src/primitives/brush/gradient/stops/mod.rs:34` — four spellings of
      0..1-float-to-u8, two of which round differently: two use
      `(x.clamp(0,1) * 255.0).round()`, one uses `(c.r * 255.0 + 0.5) as u8` with
      no clamp, one uses `(offset.clamp(0,1) * 255.0 + 0.5) as u8`.

- [ ] `src/widgets/scroll/mod.rs:322`, `:451`, `src/widgets/scroll/state.rs:112`
      — three sites test zoom identity as `(x - 1.0).abs() > f32::EPSILON`
      (~1.19e-7) while `primitives::approx` owns the crate's visual epsilon
      (`EPS = 1e-4`) and exposes `approx_zero`/`noop_f32`. At `:451` the fast
      path that skips installing a `TranslateScale` is additionally gated on an
      exact `state.offset != Vec2::ZERO` with `vec2_approx_eq` available.

- [ ] `src/host/winit/input/mod.rs:14` — clamps the scale factor with
      `max(f32::EPSILON)`, five orders of magnitude below the crate's own floor,
      instead of the shared `display::scale_factor_is_valid` that both
      `OffscreenHost::frame_offscreen` and `FrameCycle` assert against. The
      windowed host also never validates the `f64` it stores from
      `ScaleFactorChanged` (`src/host/winit/mod.rs:400`), so a bad value produces
      absurd logical coordinates before panicking several layers later, while the
      offscreen host rejects it at its door.

- [ ] `src/primitives/stroke.rs:47`, `src/primitives/shadow.rs:94`,
      `src/primitives/brush/gradient/linear.rs:200` — the exact and visual
      canonicalization policies are mixed *within* a single hash: `Stroke::hash`
      folds `width` through `canon_bits` (visual) but `color` through
      `Color::hash`, which uses `eq_bits` (exact). So a colour differing by 1e-5
      fragments a "visual" cache key while a stroke width differing by the same
      amount does not.

- [ ] `src/layout/axis.rs:38` — `Axis::main_b` exists and its doc names Scroll as
      the caller, but `ScrollSpec::pans` and `ScrollSpec::contributes`
      (`src/layout/types/layout_mode.rs:252`, `:274`) and
      `scrollbars::axis_rects` (`src/layout/scrollbars/mod.rs:136`) each spell
      the match out by hand.

- [ ] `src/primitives/approx.rs:105` and `:121` — `noop_f16_bits` and
      `opaque_f16_bits` each recompute `EPS_BITS`, and `Corners::approx_zero`
      (`src/primitives/corners.rs:124`) computes it a third time with an inline
      `crate::primitives::approx::EPS` path in the expression, which the
      convention forbids. These are `F16x4`'s domain, not that of a module
      documented as f32 comparisons.

- [ ] `src/layout/types/align.rs:241`, `src/layout/support.rs:425`,
      `src/layout/types/overlay/mod.rs:113` — "offset a box inside a slot per
      alignment" exists three times. `align_in_rect` computes
      `Center => (outer - content) * 0.5`, `Right/Bottom => outer - content`,
      else `0`, floored at zero; `arrange_axis` computes exactly that per axis
      over `AxisAlign` instead of `HAlign`/`VAlign`; `align_cross` computes the
      same offsets but clamps to a `bounds` rect rather than flooring. The first
      is consumed outside layout entirely (`src/scene/shapes/record/mod.rs:273`,
      `src/renderer/frontend/encoder/mod.rs:368`,
      `src/widgets/text_edit/view.rs:334`) and its doc at `align.rs:229` claims
      it is "one definition for all of them" — true for text, not for the layout
      pass beside it. Two alignment vocabularies each grew their own placement
      arithmetic.

- [ ] `src/widgets/slider.rs:135` vs `src/widgets/splitter/mod.rs:258` —
      `pointer_to_fraction` and `pointer_to_ratio` independently map a
      container-local pointer coordinate to a 0..1 share minus a reserved centre
      band. The "tolerate a reversed `[min, max]`, then clamp" idiom appears three
      times (`slider.rs:150`, `drag_value/mod.rs:45` and `:82`).

---

## Layered hop-through with no normalising boundary

- [ ] `src/text/system.rs:162`, `:191`, `src/text/shaper.rs:138`,
      `src/text/cosmic/mod.rs:314`, `src/text/mono.rs:62`,
      `src/text/glyphs.rs:125` — six independent empty-text guards along one call
      chain, each with its own comment explaining why *this* layer is the one
      that has to. The wrap-floor contract has the same shape: `shaper.rs:198`
      and `cosmic/mod.rs:324` carry byte-identical `debug_assert!`s with the
      identical message.

- [ ] `src/renderer/frontend/composer/mod.rs:56` vs `session.rs:52` —
      `Composer`/`ComposeSession` split one algorithm by allocation lifetime
      rather than by responsibility. Nearly every step lives on `Composer` and
      takes the buffer back as a parameter, so the session re-passes `self.out`
      at ~14 call sites; the group/batch state machine and the paint handlers
      that drive it end up in two files with the buffer shuttled between them,
      and `transform_stack`/`current_transform` split one logical stack across
      the seam.

- [ ] `src/renderer/frontend/composer/session.rs:226` — `ComposeSession::quad`
      is ~165 lines doing five jobs, one of them frame-global: the whole-frame
      clear fold at `:259` calls `discard_composed`, wipes every column of the
      `RenderBuffer`, sets `clear_override` and returns — nested inside the
      `QuadGeom::Rect` arm of a match whose stated job is reducing geometry to
      physical space. `PackedQuad` exists to separate the per-shape half from the
      shared half; the fold defeats it.

- [ ] `src/renderer/frontend/encoder/mod.rs:245` — `emit_one_shape` takes seven
      arguments including `text_ordinal`, which the caller must increment; to
      know when, `encode_node` (`:672`) re-dispatches with
      `matches!(shape, ShapeRecord::Text { .. })`, duplicating the discrimination
      the callee just performed. `owner_rect` is passed alongside `id` though it
      is exactly `ctx.layout.rect[id.idx()]`, which the callee already indexes
      other columns by.

- [ ] `src/renderer/image_registry.rs:120` — the module doc says it "owns only
      the stateful lifecycle", but `ImageRegistry` also stores and enforces
      `max_texture_dimension_2d` and exposes it as `Ui::max_image_dimension` —
      two jobs, an `Rc`-shared queue and an immutable device constant that is
      threaded separately into two other constructors from the same call site.
      `register` (`:167`) also takes `borrow_mut()` before the size validation
      that can early-return, holding the borrow across `self.ids.reserve()`.

- [ ] `src/host/window_driver/mod.rs:398` — `deny_window_requests` takes two
      opposite positions on one class of unserviceable request: `opens`/`closes`
      are a hard panic while `cursor` and `vsync`, which the recorder writes into
      the same `WindowRequests`, are accepted and discarded. The discard is now
      deliberate and commented, but the split still means an app cannot tell
      which of its window calls the offscreen host will honour without reading
      this function.

- [ ] `src/host/winit/runtime.rs:115`, `:119`, `:127`, `:217`, `:231`, `:249` —
      `windows` is a bare `Vec<Window>` searched by two keys with the predicate
      spelled out at each site. `draw` (`:127`) re-implements `by_id` verbatim,
      and `WinitHost::window_event` (`src/host/winit/mod.rs:373`) has already
      resolved the window via `by_id` before dispatching `RedrawRequested`, which
      throws that away and scans again — two scans per redraw event.

- [ ] `src/widgets/text_edit/mod.rs:96` — `TextEdit` builds
      `Node::scroll(ScrollSpec::BOTH)`, keeps its own `ViewState::scroll`,
      computes its own clamp (`view.rs:63`) and applies it as a negative
      translation — a second implementation of what `Scroll` does with
      `ScrollState::offset`, `clamp_to_natural` and its own transform.

- [ ] `src/widgets/text_edit/input.rs:213` and `src/widgets/text_edit/menu.rs:31`
      — the same indexed keyboard-drain loop in the only two places in the crate
      that have one; `menu::show` then threads `Editor::new`'s exact argument
      list through itself and rebuilds a throwaway `Editor` per action, though
      `Editor` borrows only `text` and `edit`. The menu's drain also omits the
      `filter.takes(KeyClass::of(kp))` gate that `input.rs:217` documents as
      load-bearing against double dispatch.

- [ ] `src/layout/pass.rs:285` — the dispatcher's doc states every driver is a
      module exporting `measure`/`arrange`/`intrinsic`, then lists five of the
      seven it calls. `scroll` and `scrollbars` export a pair, and their intrinsic
      policy lives inline in `content_intrinsic`
      (`src/layout/intrinsic/mod.rs:265`) — twenty-five lines of scroll-specific
      reasoning in the file meant to be pure dispatch.

- [ ] `src/layout/zstack/mod.rs:64` — `zstack::arrange` matches
      `LayoutMode::Scroll(_)` on its child and clamps before placement, encoding
      Scroll's semantics inside an unrelated driver at one of five arrange sites.
      A `Scroll` inside an `HStack`, `Grid`, `Canvas` or `WrapStack` gets no such
      clamp.

- [ ] `src/text/root.rs:14` — `TextRoot` is documented as "A run's *unbounded*
      shape", but `measure_wrapped` and `measure_truncated` both return one for
      width-bounded shapes and `CacheEntry.root` stores one per bounded entry,
      where `retention.rs:29` admits "both are inert". Every bounded caller has
      to know by convention to read `.size` only.

- [ ] `src/scene/damage/region/mod.rs:65` — `DamageRegion` carries `budget_px` (a
      merge-policy knob) and `coverage` (valid only when built through
      `collapse_from`, documented as `0.0` otherwise), then rides as a 140-byte
      `Copy` value through `RenderKind::Partial` into the encoder filter and
      backend scissors, none of which merge another rect. The two-state
      `coverage` forces a hand-written `PartialEq` (`:82`) that excludes it — a
      type whose equality has to lie because one field is only sometimes
      meaningful.

---

## Convention drift

Smaller, mechanical, and cheap to settle; grouped so they can be swept in one
pass rather than argued one at a time.

- [ ] `mod internals` has drifted into six cfg gates and four visibilities across
      34 sites: 14 use `#[cfg(test)]`, 8 use
      `#[cfg(any(test, feature = "internals"))]`, 8 use
      `#[cfg(any(test, feature = "bench"))]`, 2 use `#[cfg(feature = "bench")]`,
      1 uses `#[cfg(feature = "internals")]`, and
      `src/renderer/backend/text/mod.rs:290` invents
      `#[cfg(any(feature = "bench", all(test, feature = "internals")))]`.
      Visibility ranges from private (`src/text/cosmic/mod.rs:440`,
      `src/scene/forest.rs:426`, three in `src/renderer/gradient_atlas/`) through
      `pub(super)` (`src/text/probe/mod.rs:371`,
      `src/widgets/scroll/state.rs:207`) to `pub(crate)` (24 sites) and `pub`
      (`src/lib.rs:133`). Because `bench = ["internals"]` and not the reverse,
      the eight `bench`-gated modules are invisible to both integration suites,
      and the fourteen `cfg(test)` ones are invisible to any integration test —
      so a module named `internals` may or may not be an integration reach-in,
      with nothing at the name to say which.

- [ ] `src/renderer/backend/mod.rs:29` vs `:46` — two items from the same module
      tree are imported via `crate::renderer::backend::…` while the other eleven
      use `self::…`, in one contiguous import block, making "one canonical path
      per item" false at the module root.

- [ ] `src/animation/mod.rs:20`, `src/animation/serde.rs:5` — free functions
      imported bare and aliased with `as` (`step as spring_step`,
      `params_are_valid as spring_params_are_valid`) precisely because the bare
      names are meaningless at the call site, which is the failure the
      namespace-qualification rule exists to prevent.
      `src/ui/frame_cycle.rs:29` imports `cascade_fingerprint` bare and calls it
      unqualified at `:346`, while the same file correctly qualifies two others.

- [ ] `src/text/cosmic/retention.rs:122`, `:168`, `glyphs.rs:26`, `:84`,
      `truncate.rs:87`, `:119` — declared `pub(crate)` inside a privately
      declared `mod cosmic` whose own type is `pub(super)`, so the wider marker
      is inert and misdescribes the surface.

- [ ] `src/ui/mod.rs:110` — `input_policy` is a bare `pub` field on the god
      object while `FocusPolicy`, the other input-configuration knob, lives
      inside the private `InputState` behind an accessor pair. The only in-crate
      writers of `input_policy` are tests.

- [ ] `src/scene/damage/mod.rs:392` — `push_screen` is `pub(super)`, exposing it
      to all of `scene`, when a private fn would already be visible to the two
      descendant modules that use it.

- [ ] `src/layout/grid/mod.rs:264` — `slice_mut_pair` returns a bare
      `(&mut [f32], &mut [f32])` twenty lines below `ranges` (`:257`), which
      returns a named `HugRanges` specifically because two adjacent `&[f32]`
      parameters were swappable and its doc says so.
      `src/layout/grid/arranging.rs:95` builds a four-element tuple of
      same-typed `f32`s.

- [ ] `src/scene/cascade/engine.rs:333` — `run_tree` destructures a five-element
      tuple out of a `match` where the five parent-context fields already exist
      as a named `Frame`.

- [ ] `src/layout/support.rs:389`, `src/layout/zstack/mod.rs:69`,
      `src/layout/grid/arranging.rs:105` — placing a child on both axes is
      re-derived at three sites differing only in the alignment policy applied;
      `support.rs` already has the single-axis consolidation (`cross_place`,
      `:439`) but nothing covering the two-axis case.

- [ ] `src/widgets/theme/mod.rs:440` — `WidgetTheme::resolve` re-implements
      `ThemeDefaults::default_padding`/`default_margin`
      (`src/scene/node/mod.rs:594`).

- [ ] `src/shape/mod.rs:43` — the module is declared `pub(crate) mod sealed` but
      the `Lower` doc calls it "a private module" and the inline block calls it
      "the private module" twice more; within that block, lines 56–60 are then
      restated word-for-word as the bullet at 63–67. Two traits named `Lower`
      also live in the file, so every impl site and doc link must disambiguate.

- [ ] `src/shape/image.rs:24`, `mesh.rs:19`, `shadow.rs:19`, `icon.rs:54` (`at`);
      `image.rs:53`, `mesh.rs:24`, `icon.rs:66` (`tint`); `rect.rs:51`,
      `shadow.rs:24` (`corners`); `curve.rs:59`, `polyline.rs:21` (`cap`);
      `rect.rs:46`, `triangle.rs:27` (`stroke`) — eleven byte-identical setter
      bodies over four field names, five identical `is_noop` opening lines, nine
      repetitions of the same `#[allow(private_interfaces)]` plus the identical
      comment, and `local_rect: Option<Rect>` declared five times.

- [ ] `src/golden/mod.rs:15` — `pub use` re-exports out of a `pub(crate) mod
      diff` from a `mod.rs`, against the rule that only `lib.rs` defines the
      published surface. `RowStats` (`src/golden/diff.rs:198`) lacks
      `#[derive(Debug)]`, as does `enum Mode` at
      `src/bin/showcase/pages/clip.rs:88`.

- [ ] `src/layout/cache/integration_tests.rs` — a test file in a production
      directory under a name that is neither `tests.rs` nor a gated module at the
      end of the file it reaches into.

- [ ] `src/widgets/modal.rs:41`, `src/widgets/spinner.rs:38`,
      `src/widgets/grid.rs:36`, `src/widgets/button.rs:22`,
      `src/widgets/frame/mod.rs:18` — five `#[allow(clippy::new_without_default)]`
      suppressions on widget constructors, a repeated friction rather than five
      independent decisions.

- [ ] `src/widgets/scroll/mod.rs:31`, `:93` — `previous_scroll_content` and
      `scroll_wrappers` are free functions with one caller each in a module whose
      convention prefers methods; `hide_bars` (`:238`) assigns `self.bar_mode`
      directly while its sibling `overlay_bars` delegates to `bar_mode(…)`.
</content>
</invoke>
