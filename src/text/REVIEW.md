# Text module simplification review

Scope: production code under `aperture/src/text` (`mod.rs`, `key.rs`, `system.rs`,
`probe.rs`, `render.rs`, `cosmic.rs`, `mono.rs`, `wrap.rs`), traced through its
layout, record-store, `TextEdit`, and renderer consumers. Tests, benches, fixtures,
and gated `test_support` code are excluded except where they prove that a production
construct exists only to serve them.

Supersedes the previous revision of this file. Its single High finding — the
Clip/Ellipsis path as a partial second layout engine — has been investigated and is
resolved or corrected: the "Cosmic Text has no equivalent" premise was wrong
(cosmic-text 0.19 ships `Ellipsize::End`, deliberately not adopted because
`LineFit::Clip` is the default policy and has no native counterpart), the cut is now
cluster-safe, and the standalone-vs-contextual ellipsis advance is no longer a
correctness concern because a verify-and-back-off loop re-checks the shaped result.
What survives from that finding is folded in below as the `ellipsis_cache` item.

## Summary

~1,600 production lines carrying roughly 25 distinct types. The render-side
vocabulary in `render.rs` earns its keep — every type there has a real consumer in
`renderer/backend/text/`, and the `TextRenderSession` lease is what stops cosmic
types escaping the module. The surplus is elsewhere, in three themes:

1. A test-only shaping fallback is modelled as an `Option` plus an `INVALID` key
   sentinel, propagating a "may not exist" case through production types that never
   observe it — and forcing a duplicated key field into every per-window reuse row.
2. `TextMeasurement` is one struct serving two roles (unbounded root vs. resolved
   result), with per-field validity documented in prose rather than expressed in the
   type, so a large fraction of every cached bounded result is fields nothing reads.
3. Three caches live here, each with its own hand-rolled eviction discipline, its own
   tuning constant, and its own retained scratch.

Ownership path:

```text
RecordedText -> TextShapeInput -> TextSystem reuse map -> TextShaper
             -> ShaperInner -> CosmicMeasure buffer cache
             -> TextRenderSession -> TextBackend
```

## Withdrawn: "TextEdit bypasses the reuse layer"

An earlier revision listed the two shaping entry points as a High finding, on the
grounds that `TextEdit` calls `TextShaper::layout` (`widgets/text_edit/view.rs:224`,
`view.rs:238`, `input.rs:123`, `input.rs:281`) rather than `TextSystem::measure`,
against a benchmark showing 0.92 µs versus 2.33 µs for the reuse layer
(`system.rs:6-14`). That reasoning does not hold and the item is withdrawn:

- The benchmark's 2.33 µs figure is the *wrapped* case, where a run costs two
  dispatches a frame — the unbounded root plus the width resolve — which the module
  doc states outright (`system.rs:11-14`). `ShapeCtx::request`
  (`widgets/text_edit/view.rs:119-131`) builds the final key directly, so `TextEdit`
  pays one dispatch. The mechanism the win comes from is absent on its path.
- All four sites need geometry (`cursor_xy`, `byte_at_xy`, `selection_rects`), not a
  size. `TextSystem::measure` returns a `TextMeasurement`; the probe is the point.
- The probe resolves the shaped buffer by key regardless (`probe.rs:47`). A reuse slot
  would trade one hashmap lookup for another and leave that one in place.
- `TextSystem` is owned by `LayoutEngine`, while `TextEdit` runs during record/input
  holding only `ui.resources.text`. Routing it through the reuse layer would mean
  exposing per-window layout state to widget code — widening API, not narrowing it.

The two entry points do differ in contract (`TextMeasurement` versus a
`TextLayoutProbe` holding the shaper's exclusive borrow), but that difference is
load-bearing rather than accidental.

## Critical: the test-only mono fallback shapes production types

- [ ] **`ShaperInner` is a one-field production struct wrapping an `Option` whose
  `None` case is unreachable.** `cosmic: Option<CosmicMeasure>` (`mod.rs:149`) is
  `Some` for every production construction — `TextShaper::new` (`mod.rs:222`) is the
  only non-gated constructor, and `test_mono` (`mod.rs:421`) lives in `test_support`.
  The sibling field `measure_calls` is `#[cfg(any(test, feature = "internals"))]`
  (`mod.rs:157`), so a release build's `ShaperInner` has exactly one field and the
  real ownership chain is `Rc<RefCell<Option<CosmicMeasure>>>` with an always-`Some`
  `Option`. The unreachability is asserted by a cfg'd `unreachable!` arm in the
  dispatch match (`mod.rs:294`), and another site pays an `expect` to discharge it
  (`mod.rs:266`).

- [ ] **The per-window reuse row stores its key twice, and only the mono stub makes
  the two copies distinguishable.** `TextReuseEntry` holds both `key: TextShapeKey`
  and `unbounded: TextMeasurement` (`system.rs:146-151`), and `TextMeasurement` itself
  embeds a `key` (`mod.rs:306`). On the cosmic path the two are always equal —
  `measure_wrapped` and `measure_truncated` both return `key: request.key`, and empty
  text returns before any row is stored (`system.rs:88`). They diverge only under the
  mono fallback, which returns `TextShapeKey::INVALID` (`mono.rs:158`). `WrapReuse`
  repeats the pattern (`system.rs:161-165`). Measured: `TextShapeKey` is 24 B,
  `TextMeasurement` 40 B, `TextReuseEntry` 136 B — 48 B of every reuse row, 35%, is
  the duplicate keys. The freshness check at `system.rs:102` reads the outer copy, so
  agreement between the two fields is a convention with nothing enforcing it.

- [ ] **`mono.rs` splits one stub across gated and ungated code with `unreachable!`
  in the production arms.** `caret_x` (`mono.rs:19`) and `byte_at_x` (`mono.rs:36`)
  compile in every build because empty text is a real production case, but their
  non-empty bodies are `#[cfg]`-switched inside the function body to an
  `unreachable!` (`mono.rs:30`, `mono.rs:50`). The result is a 163-line file existing
  for a headless test path, with two functions that read as production API but are
  half-live.

## High: `TextMeasurement` carries fields meaningful in only one of its two roles

- [ ] **`intrinsic_min` and `single_line` are read exclusively off the unbounded
  root, yet ride on every bounded result.** The only readers are
  `LineFit::resolves_to_unbounded` (`wrap.rs:45`), `TextWrap::min_content`
  (`wrap.rs:100`), and `TextWrap::target_width` (`wrap.rs:125`) — all three take a
  parameter named `unbounded`. `TextWrap::content_size` (`wrap.rs:135`), the sole
  consumer of a resolved measurement, reads only `.size`. `cosmic` already relies on
  this: it hard-codes `intrinsic_min: 0.0` on the truncating path (`cosmic.rs:492`)
  and skips the segment scan for bounded wraps (`cosmic.rs:332-335`). The rule lives
  in prose on the field (`mod.rs:310-315`) rather than in the type, so
  `WrapReuse.result` (`system.rs:164`) stores fields no caller may legally read.

- [ ] **One struct is simultaneously the cache's stored value, the reuse row's
  payload, and the layout return type, which is why the key must travel inside it.**
  `CacheEntry` stores it whole "so a cache hit hands back the same value the shaping
  miss returned" (`cosmic.rs:103-105`); `LayerLayout` immediately unpacks it back into
  two fields (`layout/engine.rs:860-863`); `probe.rs:47` reads `measurement.key`
  rather than `request.key` specifically because the mono path needs the two to
  differ. Three consumers, three different subsets of the four fields.

## Medium: three caches, three eviction disciplines, five scratch buffers

- [ ] **Each cache in the module invented its own retention policy.**
  `CosmicMeasure::cache` is an LRU driven by a monotonic `use_gen` plus a per-entry
  `last_used`, evicted by copying every recency value into `evict_scratch` and
  running `select_nth_unstable` (`cosmic.rs:122-128`, `cosmic.rs:579-602`).
  `CosmicMeasure::ellipsis_cache` is bounded by wholesale clear at a fixed cap
  (`cosmic.rs:136`, `cosmic.rs:528-531`). `TextSystem::entries` is a clock sweep with
  a per-row `hot` bit and a power-of-two `sweep_limit` rebased each frame
  (`system.rs:55-68`, `system.rs:153-158`). Three policies, three tuning constants
  (`BUFFER_BUDGET` `mod.rs:174`, `ELLIPSIS_CACHE_CAP` `cosmic.rs:54`,
  `MIN_REUSE_SWEEP_LIMIT` `system.rs:44`), no shared vocabulary between them.

- [ ] **`ellipsis_cache` is a full second map — cap, clear policy, tuple key — storing
  one `f32` per entry.** It memoizes the `…` advance keyed on
  `(size_q, family as u8, weight as u8)` (`cosmic.rs:136`). Since the truncation path
  gained the verify-and-back-off loop, that advance is no longer load-bearing for
  correctness: the loop re-checks the shaped result against the budget regardless
  (`cosmic.rs:470-474`), so the memo now only determines how many retries fire, not
  whether the answer is right. It nonetheless carries the full apparatus of a bounded
  cache with an eviction rule shared with nothing else.

- [ ] **`CosmicMeasure` retains five separate scratch buffers alongside its two
  caches.** `evict_scratch` (`cosmic.rs:128`), `recycle_pool` (`cosmic.rs:131`),
  `truncate_scratch` (`cosmic.rs:143`), `break_scratch` (`cosmic.rs:146`), and
  `logical_order` (`cosmic.rs:150`). Each is individually justified by the
  alloc-free-steady-state rule, but they serve four mutually exclusive code paths —
  eviction, buffer acquisition, truncation, the unbounded segment scan — and are never
  live concurrently, so the struct's footprint is the union of paths rather than the
  peak of any one.

## Medium: width quantization is expressed twice, and the key's stated precision is unreachable

- [ ] **Two independent rounding functions must agree for the shape cache and the
  measure cache to stay in sync, with nothing but a test holding them together.**
  `wrap::canonical_wrap_width` (`wrap.rs:9-11`) and `layout::cache::quantize_axis`
  (`layout/cache/mod.rs:63-69`) both reduce to `fast_round`, in different modules,
  reached through different call chains. If they drift, a text run's shape key and its
  measure-cache `available_q` land on different grids and a cached subtree can be
  blitted against a shape measured at a different width.

- [ ] **`TextShapeKey::max_w_q` documents 1/64 px quantization but can only ever hold
  whole pixels.** The field comment states `max_width_px * 64, rounded`
  (`key.rs:39-40`), matching `size_q` and `lh_q`. The only constructor rounds to the
  whole-pixel grid first — `quantize_width(wrap::canonical_wrap_width(...))`
  (`key.rs:122`) — so the low six bits are always zero. The stated precision is
  illusory and the field's semantics differ from its two neighbours despite an
  identical description.

## Low: helpers placed by history rather than responsibility

- [ ] **`text_in_rect` is generic rect-alignment math living in the text-shaping
  module, and three unrelated subsystems depend on `text::probe` to reach it.** The
  function touches no text state — it offsets a `Size` inside a `Rect` per `Align`
  (`probe.rs:194-211`) — yet `probe.rs` is documented as "read-only geometry over one
  shaped text layout" (`probe.rs:1-4`). Consumers are the encoder
  (`renderer/frontend/encoder/mod.rs:354`), the scene shape record
  (`scene/shapes/record.rs:426`), and `TextEdit`'s view
  (`widgets/text_edit/view.rs:261`), none of which otherwise needs the probe.

- [ ] **The "widest glyph trailing edge" reduction is written twice.**
  `first_line_right` (`cosmic.rs:672`) and the per-run `line_right` inside
  `shaped_extent` (`cosmic.rs:706-712`) compute the same `max(g.x + g.w)` fold with
  the same RTL rationale; the duplication is acknowledged in the doc comment on
  `first_line_right` rather than removed.

- [ ] **`ShapedExtent` is returned in full to callers that use one of its three
  fields.** `measure_truncated` discards `intrinsic_min` and `single_line` at both
  call sites (`cosmic.rs:419`, `cosmic.rs:470`), passing `None` for the `breaks`
  scratch so `intrinsic_min` is statically `0.0`. Only `measure_wrapped`
  (`cosmic.rs:336-341`) reads all three.

- [ ] **`TextWrap`'s six variants are matched exhaustively in five methods that each
  collapse them to two or three behaviours.** `line_fit` (`wrap.rs:80`),
  `min_content` (`wrap.rs:92`), `max_content` (`wrap.rs:105`), `target_width`
  (`wrap.rs:123`), and `content_size` (`wrap.rs:135`) each re-enumerate all six, so a
  new policy means touching five matches and no single site states what one policy
  does.

## Open questions

- [x] **Resolved: the mono fallback cannot be reached from a production build.**
  `TextShaper::test_mono` lives inside the `#[cfg(any(test, feature = "internals"))]`
  `test_support` mod (`mod.rs:419`) and is the sole `ShaperInner::new(None)` caller;
  `internals` is not a default feature (`Cargo.toml:175`). Verified by compiling a
  default-feature example that names it: `E0599: no associated function ... test_mono`,
  while the same example builds under `--features internals`. So `ShaperInner.cosmic`
  is unconditionally `Some` in a released binary and `OffscreenHost` cannot be driven
  on placeholder metrics — the builder doc claiming otherwise
  (`host/offscreen.rs:88-90`) was the only thing suggesting a supported production
  mode, and has been corrected.

  Two consequences for the Critical group above. The `Option<CosmicMeasure>` axis is
  a compile-time-gated test/headless affordance, so its cost in production is the
  unreachable branches alone, not a live code path. And the duplicated key in
  `TextReuseEntry` is the price of testing through it: the reuse-layer tests run on
  the mono shaper (`TextSystem::default()`, `tests.rs:367`, `:438`, `:494`), which
  reports `INVALID`, so without the separate key those rows would miss every frame
  and the layer would be untestable. Expressing "no `Option` in production" in the
  type rather than the feature gate would need two cfg'd field definitions plus cfg'd
  accessors — more code for a guarantee the gate already provides.

- [ ] The reuse-layer benchmark cited at `system.rs:6-14` compares "slots" against
  "no slots". It does not isolate how much of that win survives once the duplicated
  keys and the two unread fields leave the row.

- [ ] `TextShaper::dispatch` and `TextSystem::shaper` remain `pub(crate)` with no
  production reader outside `src/text/`; both are reached only by `src/bench/text.rs`
  and the in-tree tests. The same is true of `ELLIPSIS_CACHE_CAP`, `ClusterGlyph`,
  `fitting_prefix` (`cosmic.rs`), `next_reuse_sweep_limit` (`system.rs`), and
  `cursor_from_byte` / `cursor_to_byte` (`probe.rs`), each of which is widened past
  its production need because the in-tree test module is a *sibling* of the file that
  declares it, not a descendant. Whether that is worth restructuring depends on
  whether `src/text/tests.rs` should be split per-file alongside its subjects.
