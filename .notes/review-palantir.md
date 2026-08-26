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
      the crate are this one test"; `src/primitives/approx/mod.rs:105` and `:121`
      hold two more.

- [ ] `src/primitives/span.rs:7` — directs readers to the `Range<u32>`
      conversions; three of `Span`'s four `From` impls (`:35`, `:55`, `:62`)
      have no callers.

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
      `src/text/wrap/mod.rs`.

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

- [ ] `src/primitives/approx/mod.rs:105` and `:121` — `noop_f16_bits` and
      `opaque_f16_bits` each recompute `EPS_BITS`, and `Corners::approx_zero`
      (`src/primitives/corners/mod.rs:124`) computes it a third time with an inline
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
