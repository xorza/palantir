# Palantir crate review

Findings from a read of `src/` (~86.6k non-test lines, 505 files). Each item is
a checklist entry: **when you address one, delete it.** The file lists open
findings only — no "done" markers, no resolved section.

Findings are grouped by the root cause they share, and the groups are ordered by
severity and benefit. Descriptions state what is wrong and where; they
deliberately do not propose fixes.

Two things are deliberately out of scope: the record-time-geometry limitation
(already surveyed in `.notes/record-time-geometry.md`) and test structure.
Behavioural defects found along the way are logged separately in
`.notes/ISSUES.md` rather than here.

---

## More prose than any change can keep current

The doc-drift findings below are not isolated slips, and the density is why.
Excluding tests, `src/` is **27,426 comment lines against 53,584 lines of code —
31%**. Twenty-five files over 120 lines are more than 49% comment;
`src/ui/mod.rs` is 883 comment lines out of 1,655. Much of that volume is not
contract but narrative: benchmark anecdotes with numbers, rejected alternatives
argued at length, and the history of how the current shape was reached. None of
it is checkable, all of it ages, and the volume is what makes the drift below
inevitable rather than accidental.

- [ ] The same prose is duplicated verbatim across sibling files rather than
      stated once. `src/widgets/switch.rs:59`, `src/widgets/checkbox/mod.rs:60`
      and `src/widgets/radio/mod.rs:56` carry the same six-line comment
      ("Everything this widget takes off its theme slot, before
      `ToggleChrome::record_row`'s `&mut Ui` reborrow…"), differing only in the
      widget name. `ToggleChrome`'s own doc (`src/widgets/toggle_chrome/mod.rs:18`)
      already states the arrangement.

- [ ] Documentation effort is distributed almost at random. Over eighty
      production files carry no `//!` at all, including the crate's most central
      ones — `src/ui/mod.rs` (the `Ui` type), `src/layout/mod.rs`,
      `src/layout/engine.rs`, `src/layout/stack/mod.rs`,
      `src/renderer/render_buffer/mod.rs`, `src/scene/shapes/mod.rs`, and every
      file in `src/shape/`. Also undocumented at module level:
      `src/text/wrap/mod.rs`, `src/icons/{icon_atlas,icon_set,svg_facts}.rs` and
      the three `icons/icon_*` directories, and
      `src/renderer/backend/raster_atlas/{atlas_slot,clock_sweep,side}.rs`.

- [ ] Published docs speak in an internal roadmap vocabulary that means nothing
      to a reader and dates the text. `src/primitives/brush/gradient/linear.rs:70`
      ("Slice 2 always emits…") and `:73` ("a slice 2.5 polish task");
      `src/primitives/color/mod.rs:549` ("no other in-crate caller until slice 2
      wires the atlas"); `src/primitives/image.rs:32` and
      `src/renderer/frontend/encoder/geometry.rs:138` ("future slice can add a
      scissor").

- [ ] `src/primitives/color/mod.rs:549` — a three-line `//` comment about
      keeping `clippy -D warnings` quiet on "a clean step-1-only branch" sits
      *between* `srgb_to_linear` and `linear_to_oklab`'s own `///` doc,
      describing a branch state that no longer exists.

---

## Documentation that contradicts the code it describes

These do not state invariants, they simply describe something that is no longer
there. Several name symbols that do not exist anywhere in the crate —
grep-checkable and still wrong.

- [ ] `src/scene/damage/mod.rs:2` — the module doc says the prev-frame snapshot
      is rebuilt "via the `entry()` API — vacant slots get inserted, occupied
      slots get diffed and either updated or evicted", and `:26` refers to "the
      Vacant arm". There is no `.entry(` call anywhere under
      `src/scene/damage/`. The same doc (`:47`) describes a field
      `DamageEngine.dirty` that does not exist — it is `counters.dirty`.

- [ ] `src/lib.rs:53` — the published feature table lists five flags;
      `Cargo.toml` declares seven. Missing are `bench` and `golden` — and
      `golden` gates `pub mod golden`, a real part of the published surface, so a
      consumer reading the docs cannot discover it exists. The `internals` row
      claims it "adds the `internals` and `bench` modules", but `bench` is gated
      on its own feature and the dependency runs the other way
      (`bench = ["internals", …]`): enabling `internals` never produces a `bench`
      module.

- [ ] `src/renderer/mod.rs:4` — says the frontend "owns the per-frame
      allocations (cmd vec, render buffer) and turns `&Tree` into
      `&RenderBuffer`". There is no cmd vec — `Frontend::build`'s own doc says
      encoder paint calls land directly in a live `ComposeSession` — and `build`
      takes a `FrameScene`.

- [ ] `src/primitives/half_simd/mod.rs:5` opens by explaining that the module
      exists to bypass `half::slice::HalfFloatSliceExt`, with the frame win
      quantified. Four wrapper docs still describe the path it replaced:
      `src/primitives/color/mod.rs:394` ("go through
      `half::slice::HalfFloatSliceExt::{…}`"), `:435` (`ColorF16::unpack`, "via
      the batched f16→f32 slice path"), `:452` (`From<Color> for ColorF16`,
      "Batched f32→f16 pack via the slice path"), and
      `src/primitives/brush/gradient/mod.rs:156` ("via the batched f16 slice
      path"). All go through `F16x4::lanes` / `F16x4::from_lanes`, which call
      `_mm_cvtph_ps` / `_mm_cvtps_ph` directly.

- [ ] `src/input/bench.rs:8` and `:103` — describes what it measures as
      "`recompute_hover` + `recompute_scroll_target` linear walk over cascade
      entries", and says the scroll region exists "so `recompute_scroll_target`
      succeeds". Neither symbol exists in the crate; the code is
      `InputState::refresh_pointer_targets` calling `Cascade::hit_test_targets`.

- [ ] `src/display.rs:18` — says the host "hands it to `WindowDriver::frame`".
      `WindowDriver` has `cpu_frame` and `render_to_texture`, no `frame`.

- [ ] `src/renderer/image_registry.rs:16` — points at `renderer::texture_id` for
      "`TextureId` + its source". No such module exists: the id is
      `primitives::texture_id`, the source is `renderer::texture_id_source`.

- [ ] `src/widgets/widget.rs:107` — `Widget::show`'s doc names "the handful
      (`Frame`, `Panel`, `Grid`, `Separator`)" as its callers. `Text`,
      `ProgressBar` and `Spinner` also call it.

- [ ] `src/widgets/theme/toggle.rs:14` — `ToggleTheme`'s doc lists its consumers
      as "`Checkbox`, `RadioButton`, future toggle/segmented controls" and omits
      `Switch`, a current consumer with fields (`track_aspect`) in that struct.

- [ ] `src/renderer/frontend/composer/session.rs:126` — `scaled_rect`'s doc calls
      it "the opening move of `rect`, `shadow`, `image`, and `text`". The
      session has no `rect` or `shadow` handler; both were folded into `quad`
      (`:249`).

- [ ] `src/renderer/frontend/composer/mod.rs:117` — `GroupCursors`' doc says the
      bundle exists so "the flush-boundary contract is one value instead of five
      parallel fields". The struct has three fields (`quads`, `texts`,
      `higher: [u32; PaintTier::COUNT]`).

- [ ] `src/renderer/frontend/composer/text_grid/mod.rs:108` — attributes a
      profiling number to `Composer::compose`, a function that no longer exists.

- [ ] `src/primitives/half_simd/mod.rs:67` — claims "Both f16 lane predicates in
      the crate are this one test"; `src/primitives/approx/mod.rs:105` and `:121`
      hold two more.

- [ ] `src/primitives/span.rs:7` — directs readers to the `Range<u32>`
      conversions as the way to build a `Span`. Only `From<Range<u32>>` has a
      caller (`src/scene/tree/mod.rs:362`); `From<Range<usize>>` (`:45`),
      `From<Span> for Range<u32>` (`:55`) and `From<Span> for Range<usize>`
      (`:62`) have none.

- [ ] `src/primitives/image.rs:22` — `ImageFit::Fill`'s doc calls it "the legacy
      'no fit' behaviour", a compatibility framing the project's stated posture
      rejects.

- [ ] `src/bin/showcase/shell.rs:41` — `Body`'s doc says "the two that own
      cross-frame resources"; the enum has three such variants (`State`,
      `GpuView`, `Fixture`).

### Doc blocks attached to the wrong item

Same cause, different mechanism: an insertion or reorder separated a comment
from what it describes, and nothing catches it.

- [ ] `src/primitives/spacing/mod.rs:30` — `Spacing::all` carries a two-line doc
      whose first line ("No spacing on any edge.") describes the `ZERO` constant
      that `f16x4_lanes!` generates, not `all`.

- [ ] `src/scene/cascade/paint_rect.rs:1` — two `//!` blocks. The second is
      verbatim from `src/scene/cascade/engine.rs:1` and describes neither this
      file's contents nor its role; a reader landing here is told the wrong
      thing first. It also reaches through a `super::` doc path.

- [ ] `src/lib.rs:203` — the comment explaining the `Animatable` same-name
      re-export sits above `pub use diagnostics::DebugOverlayConfig;`, roughly
      forty-five lines from its subject.

- [ ] `src/primitives/color/mod.rs:216` and `:238` — `ColorU8` carries two doc
      blocks, one before `#[repr(C)]` and one between the derive and the struct,
      both saying the same thing.

- [ ] `src/renderer/frontend/encoder/collision_overlay.rs:40` — `emit` opens
      with an `is_empty` early return immediately before a `for` loop over the
      same collection (`:45`).

---

## One question answered several different ways

The crate owns named predicates, constants and helpers for these, and the copies
use different tolerances or different spellings — so the same question gets a
different answer depending on which call site asks it.

- [ ] "Does this paint anything?" is the most-repeated contract in the crate and
      has no type. `fn is_noop` is defined **thirty** times as an inherent
      method (`Color`, `ColorU8`, `ColorF16`, `Stroke`, `Shadow`, `Background`,
      `Brush`, `CurveBrush`, gradients, `Mesh`, `TranslateScale`, `ShapeStroke`,
      `LoweredShadow`, every `Draw*Payload`, every `*Shape`), and
      `fn is_paint_empty` four more times (`Size`, `Rect`, `URect`, `QuadGeom`)
      for the same question under another name. Signatures disagree —
      `self` against `&self`, `const` against not, and
      `DrawImagePayload::is_noop` takes an argument. The neighbouring `NanCheck`
      question, of exactly the same shape, *is* a trait.

- [ ] "Reset for the next frame" is spelled nine ways across roughly
      twenty-five retained caches: `clear`, `reset`, `reset_for`, `begin_frame`,
      `finish_frame`, `end_frame`, `pre_record`, `post_record`, `begin_pass`.
      Two of them mean different things in different modules — `end_frame` takes
      a `frame: u64` in `raster_atlas`/`cosmic` and no argument in
      `input_state`/`icon`.

- [ ] Eleven sites test a UI-scale quantity against `f32::EPSILON` (~1.19e-7),
      five orders of magnitude below the crate's own visual epsilon
      (`primitives::approx::EPS = 1e-4`, with `approx_zero` / `noop_f32` /
      `vec2_approx_eq` beside it): `src/widgets/scroll/state.rs:145`,
      `src/widgets/scroll/mod.rs:287` and `:306`,
      `src/widgets/scroll/bars.rs:136`, `src/widgets/slider/mod.rs:160`,
      `src/widgets/splitter/mod.rs:233` and `:254`,
      `src/layout/scrollbars/mod.rs:130`,
      `src/renderer/gradient_atlas/bake.rs:100`,
      `src/host/winit/input/mod.rs:14`. Three of them use it as a
      divide-by-zero guard (`.max(f32::EPSILON)`), which is a different
      question again and has no named helper at all.

- [ ] `zoom::clamp` (`src/input/zoom.rs:26`) documents itself as bringing a
      product "back into the invertible `f32` range", and its two comparisons
      are both false for NaN — so a NaN product falls through to
      `product as f32` and comes out NaN. `combine` and `from_wheel` state the
      screen as a `debug_assert!`, so the release build carries none.

- [ ] `src/host/winit/input/mod.rs:14` clamps the incoming scale factor with
      `max(f32::EPSILON)` instead of the shared `display::scale_factor_is_valid`
      that both `OffscreenHost::frame_offscreen` (`src/host/offscreen.rs:203`)
      and `FrameCycle` (`src/ui/frame_cycle.rs:87`) assert against. The windowed
      host also never validates the `f64` it stores from `ScaleFactorChanged`, so
      a bad value produces absurd logical coordinates before panicking several
      layers later, while the offscreen host rejects it at its door.

- [ ] `src/primitives/color/mod.rs:206`, `:211`, `:345`, `:366` and
      `src/primitives/brush/gradient/stops/mod.rs:35` — five spellings of
      0..1-float-to-u8, two of which round differently: three use
      `(x.clamp(0,1) * 255.0).round()`, one uses `(c.r * 255.0 + 0.5) as u8` with
      no clamp, one uses `(offset.clamp(0,1) * 255.0 + 0.5) as u8`.

- [ ] `src/primitives/approx/mod.rs:105` and `:121` — `noop_f16_bits` and
      `opaque_f16_bits` each recompute `EPS_BITS`, and `Corners::approx_zero`
      (`src/primitives/corners/mod.rs:95`) computes it a third time. These are
      `F16x4`'s domain, not that of a module documented as f32 comparisons.

- [ ] `src/primitives/stroke.rs:47`, `src/primitives/shadow.rs:104`,
      `src/primitives/brush/gradient/linear.rs:34` — the exact and visual
      canonicalization policies are mixed *within* a single hash: `Stroke::hash`
      folds `width` through `canon_bits` (visual) but `color` through
      `Color::hash`, which uses `eq_bits` (exact). So a colour differing by 1e-5
      fragments a "visual" cache key while a stroke width differing by the same
      amount does not. `Shadow::hash` and the three gradient hashes have the same
      split.

- [ ] `src/layout/axis.rs:38` — `Axis::main_b` exists and answers "pick this
      axis's lane out of a `BVec2`", but `ScrollSpec::pans`
      (`src/layout/types/layout_mode.rs:253`), `ScrollSpec::contributes`
      (`:277`) and `scrollbars::axis_rects`
      (`src/layout/scrollbars/mod.rs:202`) each spell the match out by hand.

- [ ] `src/layout/types/align.rs:176` and `src/layout/types/overlay/mod.rs:113`
      — "offset a box inside a slot per alignment" exists twice.
      `Align::place_in` computes `Center => (outer - content) * 0.5`,
      `Right/Bottom => outer - content`, else `0`, floored at zero; `align_cross`
      computes the same offsets over `AxisAlign` instead of `HAlign`/`VAlign`,
      and clamps to a `bounds` rect rather than flooring. The first is consumed
      outside layout entirely (`src/renderer/frontend/encoder/layer_ctx.rs:215`,
      `src/widgets/text_edit/text_geometry.rs:118`, `src/text/probe/mod.rs`) and
      its doc claims it is "one definition for all of them" — true for text, not
      for the layout pass beside it.

- [ ] `src/widgets/slider/mod.rs:175` vs `src/widgets/splitter/mod.rs:252` —
      `pointer_to_fraction` and `pointer_to_ratio` independently map a
      container-local pointer coordinate to a 0..1 share minus a reserved centre
      band. The "tolerate a reversed `[min, max]`, then clamp" idiom appears
      three times (`slider/mod.rs:190`, `drag_value/mod.rs:45` and `:82`).

- [ ] Two axis-aligned-bounds vocabularies. `primitives::rect::aabb::Aabb` folds
      points into a `Rect` and signals failure with the named `Rect::NAN`
      sentinel; `primitives::bezier::CurveBounds` is a bare `{ lo, hi }` pair,
      and both `cubic_bezier_bbox` (`src/primitives/bezier/mod.rs:56`) and
      `arc_bbox` (`src/primitives/arc/mod.rs:32`) open-code the "screen the
      inputs, return a NaN pair" idiom that `Aabb` exists to own.

- [ ] `src/primitives/bezier/mod.rs:93` and `:94` — `solve_quadratic` gates on
      two unnamed `1.0e-12` literals, in a crate where every other tolerance
      carries a name and a reason.

---

## Invariants a type states but does not enforce

Each of these is a property the code depends on and documents at length, held
in place by a comment and by every current caller happening to respect it. The
compiler checks none of them, and the failure mode in each case is silent.

- [ ] `GradientStops` (`src/primitives/brush/gradient/stops/mod.rs:75`) states
      ascending offset order as "an invariant of the type, not a step the bake
      does", and `bake_stops` (`src/renderer/gradient_atlas/bake.rs:16`)
      `debug_assert!`s it. `GradientStops::new` sorts;
      `GradientStops::deserialize` (`:168`) builds `Self(values)` from the
      parsed array without sorting. Logged in `.notes/ISSUES.md`.

- [ ] `RasterAtlas`'s free-slab-index rule is "a freed index has
      `alloc == None`", which is what stops `ClockSweep`
      (`src/renderer/backend/raster_atlas/clock_sweep.rs:51`) from picking an
      index already on the free list and pushing it a second time. Three
      functions depend on it — `retire_slot`, `retire_unallocated`, and the
      sweep — and none of them names it; `retire_slot`'s `slot.alloc.take()`
      is load-bearing for a reason its own doc gives as generation-bumping.
      A duplicate free index hands one slab slot to two live cache keys.

- [ ] `src/renderer/frontend/composer/session.rs:237` — the `PaintSink`
      impl's `pop_clip` body is `self.pop_clip()`. It terminates only because
      an inherent `ComposeSession::pop_clip` exists at `:1167` and inherent
      methods win method resolution. Rename or move that inherent method and
      this compiles unchanged into unbounded recursion. `push_clip` at `:234`
      leans on the same shadowing, saved only by taking an argument.

- [ ] `Ramp::color_at` (`src/renderer/gradient_atlas/bake.rs:83`) documents
      "**Callers must pass a non-decreasing `t`**" and reads a cursor that
      cannot walk back. Its `while self.stops[self.upper].offset() < t` loop
      is bounded by an earlier `t >= stops[last].offset()` return — which is
      only a bound while the stops are sorted, the invariant above.

---

## Untrusted host input screened in one place only

- [ ] `InputState::on_input` (`src/input/input_state/mod.rs:383`) screens
      `InputEvent::Zoom` against `zoom::is_valid` and drops an invalid one at
      the door. No other variant is screened, and neither is the winit
      translation that mints them (`src/host/winit/input/mod.rs`):
      `ScrollPixels` / `ScrollLines` / `PointerMoved` pass whatever the OS
      reported straight through. A non-finite scroll delta reaches
      `ScrollState::offset` (`src/widgets/scroll/state.rs:167`), where
      `f32::clamp` passes NaN through, and then `ScrollState::transform`
      (`:188`), whose `TranslateScale::new` is a **release** `assert!`. The
      crate's own rule is that untrusted data is never an assert. Logged in
      `.notes/ISSUES.md`.

---

## Contracts carried by convention instead of by a type

Each of these is a set of types or modules that all answer the same shaped
question, held together by prose and by the reviewer noticing. Adding a member
means finding every site by hand.

- [ ] The layout driver contract is prose plus three parallel matches. Seven
      driver modules (`stack`, `wrapstack`, `zstack`, `canvas`, `grid`, `scroll`,
      `scrollbars`) each export a `measure` / `arrange` / `intrinsic` triple, and
      the contract is spelled out in a forty-line doc comment on
      `LayoutPass::measure_dispatch` (`src/layout/pass.rs:328`) which ends by
      listing the three places a new driver must be added by hand:
      `measure_dispatch` (`:372`), `LayoutPass::arrange` (`:419`), and
      `intrinsic::content_intrinsic` (`src/layout/intrinsic/mod.rs:287`). The
      three matches also disagree on argument order and on which of them takes
      the pass.

- [ ] Four GPU pipelines implement the same unnamed five-method shape —
      `new(device)`, `build_variants(...)`, `upload(...)`, `bind(...)`,
      `draw_*(...)`, plus a free `*_instance_layout()` beside each:
      `QuadPipeline` (`src/renderer/backend/quad_pipeline.rs`), `MeshPipeline`,
      `ImagePipeline`, `CurvePipeline`. The names drift already — `upload`
      against `upload_instances`, `draw_range` against `draw_batch` against
      `draw`.

- [ ] Eight widgets rebuild the same five-line `LookPlan` by hand from the same
      four theme-slot fields: `src/widgets/button.rs:74`,
      `checkbox/mod.rs:72`, `radio/mod.rs:82`, `switch.rs:72`,
      `combo_box/mod.rs:105`, `drag_value/mod.rs:367`, `text_edit/mod.rs:386`,
      `context_menu/menu_item.rs:103`. Every one reads
      `slot.pick(&response[, state]).to_animated(&theme.text)`, `slot.padding`,
      `slot.margin`, `slot.anim` — a `padding`/`margin`/`anim`/`looks` quartet
      that six different theme structs each declare independently.

- [ ] `Checkbox::show`, `RadioButton::show` and `Switch::show` share an
      identical twenty-line preamble (widget open, response probe, click latch,
      theme slot read, `LookPlan`, `ToggleChrome` construction, `record_row`) and
      differ only in the toggle semantics and the box child. `ToggleChrome`
      absorbed the row scaffolding but stopped short of the preamble that feeds
      it.

- [ ] `src/renderer/backend/raster_atlas/counters.rs:21` declares `AtlasCounts`
      and `src/renderer/gradient_atlas/counters.rs:44` declares
      `GradientAtlasCounts`; neither name appears anywhere else in the crate.
      They are reachable only through `counter_snapshot!`'s expansion, so a
      grep for either type finds one hit and tells the reader nothing.

---

## Structural duplication a payload parameter would collapse

- [ ] The three gradient kinds are the same type written three times.
      `src/primitives/brush/gradient/{linear,radial,conic}.rs` are 123, 123 and
      124 lines that each declare a struct of `geometry + stops + spread +
      interp`, a hand-written `Hash` folding geometry through `canon_bits` then
      `gradient_tag` then stops, `builder()`, `new()`, a `two_stop*` shorthand,
      `axis()`, a `…Builder` struct repeating the geometry fields beside a
      `GradientBuilderCore`, `build()`, `From<Builder>`, and `NanCheck`. Only the
      geometry payload and the default `Interp` differ. The two existing macros
      (`gradient_common!`, `gradient_builder_common!`) cover the two modifier
      setters and nothing else.

- [ ] `GradientBuilderCore::push` (`src/primitives/brush/gradient/mod.rs:124`)
      asserts against `MAX_STOPS`, and `GradientStops::new`
      (`stops/mod.rs:100`) asserts against it again over the same values.

- [ ] `CurveBrush` (`src/primitives/brush/mod.rs:38`) is a two-variant
      restriction of `Brush` and re-spells its `From<Color>` /
      `From<ColorU8>` / `From<LinearGradient>` / `From<LinearGradientBuilder>`
      ladder, its `is_noop`, and its `NanCheck` — with `TRANSPARENT` at
      `pub(crate)` on one and `pub` on the other.

- [ ] Two solvers answer "distribute leftover by weight, clamped to
      `[floor, cap]`, freezing violators". `stack::freeze_distribute`
      (`src/layout/stack/mod.rs:53`) freezes every violator per pass;
      `AxisScratch::resolve_axis` Phase 3
      (`src/layout/grid/axis_scratch.rs:255`) freezes one per pass. They
      converge differently for mixed min/max violations, which
      `cross_driver_tests/fill_solvers.rs` pins — so `Sizing::fill` means one
      thing inside a `Panel` and another inside a `Grid`, and the difference is
      not documented anywhere a caller reads.

- [ ] `src/layout/grid/axis_scratch.rs:256`–`:277` — the Phase-3 clamp loop
      computes `weight`, `candidate` and `lo` inside the `position` closure and
      then recomputes all three in the `Some(k)` arm, from the same `tracks[i]`.

---

## Policies that spend more than their doc claims

- [ ] `build_mask_plan` (`src/renderer/backend/schedule/mod.rs:29`)
      deduplicates a group's stencil mask chain against **the previous group
      only**. A group with no scissor resets `previous_chain` to empty, so two
      groups sharing one rounded-clip chain with any scissor-less group
      between them each stage their own copy of the mask quads, and the
      schedule then clears and re-stamps the chain between them. Nothing in
      the module says the dedup is one-deep.

- [ ] `RasterAtlas::evict_one`
      (`src/renderer/backend/raster_atlas/mod.rs:664`) latches
      `Side::dry_frame` for the rest of the frame the moment one clock
      rotation finds no victim. `allocate` (`:593`) can grow the side after
      that latch, which adds slots the clock could sweep — but the latch is
      keyed on the frame, not on the side's generation, so no further
      eviction runs until the next frame however much the atlas changed.

- [ ] `bar_geometry` (`src/layout/scrollbars/mod.rs:150`) floors
      `thumb_size` at `1.0` and then computes
      `thumb_offset.min(viewport.floor() - thumb_size)` with no lower bound.
      Logged in `.notes/ISSUES.md`.

---

## Types that outgrew their file

- [ ] `Ui` (`src/ui/mod.rs`) is 1,655 lines and **91 methods** on one type,
      spanning recording, input reads, focus, window open/close, vsync, cursor,
      clipboard, image registration, icon loading, GPU views, debug overlay,
      per-widget state, animation, text probing and theming. It is pinned at
      5,256 bytes by `hot_struct_sizes_are_pinned`, and the pin's own comment
      says every pass walks `&mut Ui` to reach five different engines. The file
      carries no `//!`.

- [ ] `src/input/response.rs` (443 lines) declares eight public types —
      `InputDelta`, `Drag`, `ButtonPhase`, `ButtonState`, `ScrollDelta`,
      `ResponseState`, `PointerAction`, `PointerEdge` — against the project's
      one-major-struct-per-file rule. `src/scene/node/mod.rs` (781 lines)
      similarly holds `NodeMode`, `Salt`, `Node`, `ConfigureNode` and the two
      traits `Configure` and `ThemeDefaults`; `src/input/keyboard.rs` holds
      `Key`, `Modifiers`, `TextChunk`, `KeyPress` and `KeyboardEvent`.

- [ ] `WgpuBackend::submit` (`src/renderer/backend/mod.rs:357`) is roughly 190
      lines, over half of them multi-paragraph block comments explaining the
      pass structure, the belt, the clear-alpha rule and the timestamp resolve —
      orientation material with no `//!` to live in, since the module doc
      (`:1`) is two lines about what the module contains.

---

## Dead surface

- [ ] `ClipMode::is_rounded` (`src/layout/types/clip_mode.rs:31`) has no caller
      anywhere in the crate.

- [ ] `IconSet::nominal` (`src/icons/icon_set.rs:110`) has no caller and returns
      exactly `self.handle(icon).view_box`, which `IconHandle` already carries as
      a public field.
