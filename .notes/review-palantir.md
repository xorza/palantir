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
      `src/host/winit/input/mod.rs:13`. Three of them use it as a
      divide-by-zero guard (`.max(f32::EPSILON)`), which is a different
      question again and has no named helper at all.

- [ ] `src/host/winit/input/mod.rs:13` clamps the incoming scale factor with
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
