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
      `src/primitives/brush/gradient/mod.rs:142` — the exact and visual
      canonicalization policies are mixed *within* a single hash: `Stroke::hash`
      folds `width` through `canon_bits` (visual) but `color` through
      `Color::hash`, which uses `eq_bits` (exact). So a colour differing by 1e-5
      fragments a "visual" cache key while a stroke width differing by the same
      amount does not. `Shadow::hash` and `Gradient::hash` have the same split.

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
