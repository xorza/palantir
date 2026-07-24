# Text architecture and API review

The split between window-local identity reuse (`TextSystem`) and the
app-global shaped-buffer cache (`TextShaper`) is sound: widget identities are
not window-namespaced, while the cosmic-text buffers are safely shareable.
The remaining structural problem is the absence of one cohesive ownership
boundary. Shaping now has one canonical request/key representation, but cache
sequencing still leaks into layout, wrap policy is interpreted in several
places, global maintenance is window-driven, and editor-only geometry occupies
the shaping/cache root module.

Current flow: layout converts raw `ShapeRecord` fields directly into a
canonical `TextShapeRequest`, creates an unbounded identity-cached run, and may
derive a bounded request in a second shape step. `TextShapeKey` owns validation,
quantization, normalization, and decoding; cosmic shaping and backend
reconstruction consume the same `text + key` pair. `TextEdit` bypasses the
identity cache and borrows the same global shaper directly for caret, hit-test,
and selection geometry.

## High: shaping policy and cache sequencing still leak into layout

- [ ] **The identity-cache API models one shape as a staged, borrow-bearing protocol with four coordinator entities.** `TextRunIdentity`, `PreparedTextRun`, `PreparedTextRunState`, and `TextReuseEntry` jointly represent one lookup (`src/text/mod.rs:156`, `src/text/mod.rs:163`, `src/text/mod.rs:171`, `src/text/mod.rs:1017`); `prepare` returns a value whose hidden optional state distinguishes empty text and holds a mutable map entry alive (`src/text/mod.rs:330`), and `shape_bounded` must consume that value to complete the request (`src/text/mod.rs:381`). The bounded production caller has to copy `unbounded` before conditionally consuming `prepared` (`src/layout/engine.rs:860`, `src/layout/engine.rs:901`). This protocol inflates entity count and exposes cache sequencing to layout code, so changes to a single shaping operation propagate through several lifetime-coupled types and call-site phases.

- [ ] **`TextWrap` semantics are decomposed independently by shaping, desired-size, and intrinsic-size code.** Six public policies live in `TextWrap` (`src/text/wrap.rs:11`), layout separately maps them to `LineFit`, decides whether shaping is bounded, adjusts the target width, and special-cases scroll desired size (`src/layout/engine.rs:880`, `src/layout/engine.rs:887`, `src/layout/engine.rs:893`, `src/layout/engine.rs:927`), while intrinsic layout performs another exhaustive interpretation (`src/layout/intrinsic/mod.rs:306`). A policy change can therefore alter painted glyphs without the matching min-content, max-content, or desired-size behavior, producing disagreement between measurement and rendering.

## Medium: boundaries expose or own the wrong responsibilities

- [ ] **The validated text-metrics invariant is discarded between recording and layout, forcing repeated validation in the trusted hot path.** `Shapes::add` drops every no-op shape before storage (`src/scene/shapes/mod.rs:71`), and text no-op detection already validates font size and line height (`src/shape/mod.rs:304`), yet `ShapeRecord` and `TextShapeInput` store those values as raw `f32`s again (`src/scene/shapes/record.rs:104`, `src/layout/support.rs:27`). `TextShapeInput::shape_request` must therefore call the fallible `TextShapeRequest::unbounded` constructor and immediately `expect` success for every text run (`src/layout/support.rs:45`), including both desired-size and intrinsic-size paths (`src/layout/engine.rs:852`, `src/layout/intrinsic/mod.rs:291`). Malformed metrics are correctly filtered at authoring, but the proof is lost before layout, so finite/epsilon checks and an impossible branch remain in the hot path.

- [ ] **Editor-only caret, hit-test, and selection geometry occupies the shaping/cache root module.** `TextLayoutProbe`, `CursorPos`, `SelectionRects`, cursor conversion, mono hit-testing, and selection-rectangle construction account for the geometry block in `src/text/mod.rs` (`src/text/mod.rs:61`, `src/text/mod.rs:147`, `src/text/mod.rs:413`, `src/text/mod.rs:480`, `src/text/mod.rs:860`, `src/text/mod.rs:997`), but their only production consumer is `TextEdit` (`src/widgets/text_edit/view.rs:13`, `src/widgets/text_edit/input.rs:120`). This makes the generic text cache depend on editor interaction semantics and `unicode-segmentation` (`src/text/mod.rs:34`), while the root still mixes public vocabulary, cache ownership, shaping dispatch, editor geometry, placement, and fallback measurement.

- [ ] **Window-local finalization owns maintenance of an app-global buffer cache.** Every `Ui::finalize_frame` calls its own `TextSystem::end_frame` (`src/ui/mod.rs:423`, `src/ui/mod.rs:426`), which immediately runs `TextShaper::end_frame` before sweeping the window-local identity rows (`src/text/mod.rs:303`); that shaper handle is clone-shared by every window and the backend (`src/host/window_driver.rs:219`, `src/host/window_driver.rs:222`, `src/host/shared.rs:34`). Global LRU maintenance is therefore scheduled according to each window's independent frame cadence and interleaved with local widget eviction, so the same global resource can be budgeted multiple times during one multi-window host cycle and its ownership remains split between layout and host/backend lifetimes.

## Low: avoidable hot-path work remains

- [ ] **Test-only dispatch accounting is stored and mutated in production shaping state.** `TextShaper::inner` is `pub(crate)` specifically for test observability, and `ShaperInner` unconditionally carries `measure_calls` (`src/text/mod.rs:121`, `src/text/mod.rs:181`); direct layout probes, identity refreshes, and bounded misses all increment it (`src/text/mod.rs:275`, `src/text/mod.rs:347`, `src/text/mod.rs:403`). Production text work pays the shared mutable-state write and carries a wider internal visibility boundary solely for cache-effectiveness tests.
