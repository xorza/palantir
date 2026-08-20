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

These do not state invariants, they simply describe something that is no longer
there. Every reviewer hit this independently, in every subsystem, which makes it
a process problem rather than a set of isolated slips. Several name symbols that
do not exist anywhere in the crate — grep-checkable and still wrong.

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
      quantified. Two wrapper docs still describe the path it replaced:
      `src/primitives/color/mod.rs:394` ("go through
      `half::slice::HalfFloatSliceExt::{…}`") and
      `src/primitives/brush/gradient/mod.rs:146` ("via the batched slice path").
      Both go through `F16x4::lanes`, which calls `_mm_cvtph_ps` directly.

- [ ] `src/renderer/frontend/composer/mod.rs:23` — the comment says `pub(crate)`
      is "only so the `text_grid` benchmark can reach the gated `internals`
      harness; every item inside stays `pub(super)`." Neither half is true:
      `TILE_SIZE`, `TILE_CAP`, `TextRectGrid`, `spill`, `start_frame`, `clear`,
      `push` and `any_overlap` are all `pub(crate)`, there is no `internals`
      harness in `text_grid`, and the only outside consumer is
      `text_grid::bench`, a child module that already sees private items.

- [ ] `src/input/bench.rs:8` and `:102` — describes what it measures as
      "`recompute_hover` + `recompute_scroll_target` linear walk over cascade
      entries", and says the scroll region exists "so `recompute_scroll_target`
      succeeds". Neither symbol exists in the crate; the code is
      `refresh_pointer_targets` calling `Cascade::hit_test_targets`.

- [ ] `src/display.rs:18` — says the host "hands it to `WindowDriver::frame`".
      `WindowDriver` has `cpu_frame` and `render_to_texture`, no `frame`.

- [ ] `src/renderer/image_registry.rs:16` — points at `renderer::texture_id` for
      "`TextureId` + its source". No such module exists: the id is
      `primitives::texture_id`, the source is `renderer::texture_id_source`.

- [ ] `src/widgets/widget.rs:107` — `Widget::show`'s doc names "the handful
      (`Frame`, `Panel`, `Grid`, `Separator`)" as its callers. `Text`
      (`src/widgets/text.rs:118`), `ProgressBar` (`progress_bar.rs:56`) and
      `Spinner` (`spinner.rs:93`) also call it.

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

- [ ] `src/primitives/half_simd/mod.rs:67` — claims "Both f16 lane predicates in
      the crate are this one test"; `src/primitives/approx.rs:105` and `:121`
      hold two more.

- [ ] `src/primitives/span.rs:7` — directs readers to the `Range<u32>`
      conversions; three of `Span`'s four `From` impls (`:35`, `:55`, `:62`)
      have no callers.

- [ ] `src/renderer/gradient_atlas/bench.rs:49` — says "Requires the `internals`
      feature" one line above a run command using `--features bench`, in a
      module gated `#[cfg(feature = "bench")]`.

- [ ] `src/primitives/image.rs:22` — `ImageFit::Fill`'s doc calls it "the legacy
      'no fit' behaviour", a compatibility framing the project's stated posture
      rejects.

- [ ] `src/bin/showcase/pages/shapes.rs:156` — the comment states the cell
      "Exercises the alloc-free claim"; `stress` (`:163`) allocates a
      ~90 KB `Mesh` every frame.

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

- [ ] `src/layout/types/align.rs:174` and
      `src/layout/types/overlay/mod.rs:113` — "offset a box inside a slot per
      alignment" exists twice. `Align::place_in` computes
      `Center => (outer - content) * 0.5`, `Right/Bottom => outer - content`,
      else `0`, floored at zero; `align_cross` computes the same offsets over
      `AxisAlign` instead of `HAlign`/`VAlign`, and clamps to a `bounds` rect
      rather than flooring. The first is consumed outside layout entirely
      (`src/scene/shapes/record/mod.rs:278`,
      `src/renderer/frontend/encoder/mod.rs:378`,
      `src/widgets/text_edit/text_geometry.rs:116`) and its doc claims it is
      "one definition for all of them" — true for text, not for the layout pass
      beside it. Two alignment vocabularies each grew their own placement
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
      computes its own clamp (`view_state.rs:63`) and applies it as a negative
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

- [ ] `src/ui/frame_cycle.rs:29` imports `cascade_fingerprint` bare and calls it
      unqualified at `:360`, while the same file correctly qualifies two others.

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

- [ ] `src/layout/zstack/mod.rs:69` and `src/layout/grid/arranging.rs:105` —
      placing a child on both axes is re-derived at both sites, differing only in
      the alignment policy applied, with no two-axis consolidation anywhere.

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
