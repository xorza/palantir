# Text architecture and API review

The split between window-local identity reuse (`TextSystem`) and the
app-global shaped-buffer cache (`TextShaper`) is sound: widget identities are
not window-namespaced, while the cosmic-text buffers are safely shareable.
The remaining structural problem is the absence of one cohesive ownership
boundary. Shaping and identity reuse now have one canonical operation, but wrap
sizing policy is still interpreted in multiple layout paths, global maintenance
is window-driven, and editor-only geometry occupies the shaping/cache root
module.

Current flow: layout converts raw `ShapeRecord` fields directly into a
canonical `TextShapeRequest` and calls `TextSystem::shape` with the run identity,
wrap policy, alignment, and optional available width. `TextSystem` owns the
unbounded identity lookup, derives any bounded request after reading
`intrinsic_min`, and returns the resolved measurement without exposing a cache
borrow. `TextShapeKey` owns validation, quantization, normalization, and
decoding; cosmic shaping and backend reconstruction consume the same
`text + key` pair. `TextEdit` bypasses the identity cache and borrows the same
global shaper directly for caret, hit-test, and selection geometry.

## High: wrap sizing policy remains split across layout paths

- [ ] **`TextWrap` sizing semantics are decomposed independently by desired-size and intrinsic-size code.** `TextSystem::shape` now owns the shaping interpretation of all six public policies (`src/text/mod.rs:312`, `src/text/mod.rs:352`), but desired-size layout still special-cases `Scroll` (`src/layout/engine.rs:873`) while intrinsic layout separately maps every policy to min-content and max-content behavior (`src/layout/intrinsic/mod.rs:296`). A policy change can therefore alter shaped glyphs without the matching sizing behavior, producing disagreement between measurement, intrinsic constraints, and layout demand.

## Medium: boundaries expose or own the wrong responsibilities

- [ ] **The validated text-metrics invariant is discarded between recording and layout, forcing repeated validation in the trusted hot path.** `Shapes::add` drops every no-op shape before storage (`src/scene/shapes/mod.rs:71`), and text no-op detection already validates font size and line height (`src/shape/mod.rs:304`), yet `ShapeRecord` and `TextShapeInput` store those values as raw `f32`s again (`src/scene/shapes/record.rs:104`, `src/layout/support.rs:27`). `TextShapeInput::shape_request` must therefore call the fallible `TextShapeRequest::unbounded` constructor and immediately `expect` success for every text run (`src/layout/support.rs:45`), including both desired-size and intrinsic-size paths (`src/layout/engine.rs:854`, `src/layout/intrinsic/mod.rs:289`). Malformed metrics are correctly filtered at authoring, but the proof is lost before layout, so finite/epsilon checks and an impossible branch remain in the hot path.

- [ ] **Editor-only caret, hit-test, and selection geometry occupies the shaping/cache root module.** `TextLayoutProbe`, `CursorPos`, `SelectionRects`, cursor conversion, mono hit-testing, and selection-rectangle construction account for the geometry block in `src/text/mod.rs` (`src/text/mod.rs:65`, `src/text/mod.rs:148`, `src/text/mod.rs:395`, `src/text/mod.rs:495`, `src/text/mod.rs:893`, `src/text/mod.rs:954`), but their only production consumer is `TextEdit` (`src/widgets/text_edit/view.rs:13`, `src/widgets/text_edit/input.rs:120`). This makes the generic text cache depend on editor interaction semantics and `unicode-segmentation` (`src/text/mod.rs:35`), while the root still mixes public vocabulary, cache ownership, shaping dispatch, editor geometry, placement, and fallback measurement.

- [ ] **Window-local finalization owns maintenance of an app-global buffer cache.** Every `Ui::finalize_frame` calls its own `TextSystem::end_frame` (`src/ui/mod.rs:423`, `src/ui/mod.rs:426`), which immediately runs `TextShaper::end_frame` before sweeping the window-local identity rows (`src/text/mod.rs:284`); that shaper handle is clone-shared by every window and the backend (`src/host/window_driver.rs:219`, `src/host/window_driver.rs:222`, `src/host/shared.rs:34`). Global LRU maintenance is therefore scheduled according to each window's independent frame cadence and interleaved with local widget eviction, so the same global resource can be budgeted multiple times during one multi-window host cycle and its ownership remains split between layout and host/backend lifetimes.

## Low: avoidable hot-path work remains

- [ ] **Test-only dispatch accounting is stored and mutated in production shaping state.** `TextShaper::inner` is `pub(crate)` specifically for test observability, and `ShaperInner` unconditionally carries `measure_calls` (`src/text/mod.rs:122`, `src/text/mod.rs:167`); direct layout probes, identity refreshes, and bounded misses all increment it (`src/text/mod.rs:261`, `src/text/mod.rs:328`, `src/text/mod.rs:377`). Production text work pays the shared mutable-state write and carries a wider internal visibility boundary solely for cache-effectiveness tests.
