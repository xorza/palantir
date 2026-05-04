# Todo


## Damage rendering

- **Multi-rect damage.** Replace the single union rect with N disjoint regions (clustered from the per-node dirty set). Avoids the 50% heuristic tripping when two unrelated corners change.
- **Incremental hit-index rebuild.** Only update `HitIndex` entries for dirty nodes (and any whose cascade row changed) instead of walking every node every frame.
- **Debug overlay.** Toggleable mode that flashes dirty nodes red and outlines the damage rect — trivial once the per-node dirty set has a real consumer.
- **Tighter damage on parent-transform animation.** A dedicated transform-cascade pass to collapse deep-subtree damage to a tight bound; only worth it if profiling shows the current union is too coarse.
- **Manual damage verification.** Visual A/B against `damage = None` to catch the case where the diff misses something.

## Text

- **Layer B — `CosmicMeasure.cache` eviction.** Refcount `TextCacheKey` by live `WidgetId`s; sweep via `SeenIds.removed()` so the shaped-buffer table doesn't leak. Defer until a string-churn workload demonstrates the leak.
- **Wallclock bench for the reuse cache.** `benches/layout.rs` runs without cosmic, so it can't see the Layer A win. Add a cosmic-enabled variant with N=100 static labels and quote real µs/frame numbers.
- **`Shape::Text.text: String` allocs.** Each `Text::show` clones into the shape every frame. Move to `Cow<'static, str>` for static labels; intern dynamic strings via `Arc<str>` keyed on `text_hash`. Profile-gate before shipping.
- **Editable text.** `TextEdit` widget with one `cosmic_text::Editor` per `WidgetId`, glyph-level hit-test (`Buffer::hit`), IME plumbing through `winit`, selection rendering as sibling `RoundedRect` shapes. Blocked on the persistent `Id → Any` state map.
- **Color-space verification.** Glyphon outputs sRGB; confirm text doesn't look faded on a linear surface format and document the rule.
- **Atlas eviction under multi-font / multi-size load.** Verify `atlas.trim()` + glyphon's shelf overflow holds up over a long session.

## Persistent-state consumers

`Ui::state_mut::<T>(id)` is shipped (`src/ui/state.rs`). Pending consumers:

- **ScrollView v1.** Plan in `docs/scrollview.md`. `InputEvent::Scroll` from winit, vertical-only widget, offset stored via `state_mut`.
- **Drag tracking.** Build on the existing `Active`-capture so `drag_delta` works rect-independent (pointer can leave the originating widget mid-drag). Pre-req for scrollbars + touch-drag.
- **Focus subsystem.** Tab order, focus ring, keyboard-nav. Distinct from the state map — needs its own pass.
- **`TextEdit` widget.** One `cosmic_text::Editor` per `WidgetId` via `state_mut_with` (add the public API when this lands), glyph-level hit-test, IME, selection rendering as sibling shapes.

## Measure cache (`src/layout/measure-cache.md`)

- **Cross-frame intrinsic-query cache.** `LayoutEngine::intrinsic` is intra-frame only. A second column keyed on `subtree_hash + axis + req` would compose cleanly. Skip until a workload proves it matters.
- **Real-workload validation.** Bench numbers are synthetic. The showcase doesn't push against the 400 µs ceiling, so the cache's user-visible win is unverified.
- **Cold-cache mitigations.** If a workload ever shows resize-frame jank, candidates: skip snapshot writes for collapsed subtrees, gate writes by subtree-size threshold, amortize compact across frames. Speculative.
- **Coarser `available` quantization (measure side).** Currently 1 logical px. If jittery `Fill` children show cache misses on sub-pixel parent drift, bump granularity. Wait for evidence.

## Encode cache (`src/renderer/frontend/encoder/encode-cache.md`)

Listed in rough order of bang-for-buck.

1. **Composer cache.** Shipped. See `src/renderer/frontend/composer/compose-cache.md`. Cascade-keyed (variant A), bracketed by in-stream `EnterSubtree`/`ExitSubtree` markers, fast-forwards past cached cmd ranges via patched `exit_idx`. Bench: 141× speedup on `nested/compose_only`.
2. **Hit-hint propagation.** Both caches key on `(WidgetId, subtree_hash, available_q)` and sweep on the same `removed` list, so a measure-cache hit implies an encode-cache hit. Layout writes a `Vec<bool>` (or packed bit on `LayoutResult`) marking measure-cache-hit roots; encoder reads the bit and skips its own `FxHashMap::get`. Saves one hash lookup per cached subtree. Tiny per-call, only sound while the two caches stay eviction-locked. Profile-driven.
3. **Damage-aware encode replay.** Currently `damage_filter.is_some()` bypasses the cache entirely, so animated frames don't benefit. The cached cmds are already correct (full subtree, damage-independent); gate the replay on `screen_rect ∩ damage = ∅`. Closer to a damage optimization than a cache one, but composes naturally.
4. **SIMD `bump_rect_min`.** Replay loop reads/writes 2× f32 per rect-bearing cmd (~12 800 ops on the nested workload). Precompute a bit-per-cmd "rect-bearing" mask alongside the kinds array; `bump_rect_min` then vectorizes over rect payloads. Only worth it if profiles show this loop hot.
5. **Tiny-subtree threshold.** Caching a 1–2-cmd subtree costs more in hashmap probe + `write_subtree` bookkeeping than it saves. Add a `min_cmds_for_cache` (≈4) gate before `write_subtree`. Speculative — needs a profile.
6. **Coarser `available_q` quantization (encode side).** 1-logical-px granularity may bust the cache on sub-pixel parent drift. Bump to 2 px or 4 px if a profile shows hash-match / avail-mismatch as a frequent miss path.
