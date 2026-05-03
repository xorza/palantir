# Proposed features

Systems-level / under-the-hood gaps not in `todo.md`. One-to-two-sentence
sketches; promote into `todo.md` when motivated by a workload.

## Layout / measurement caching

- **Cross-frame measure short-circuit.** Key `(WidgetId, available, sizing) → desired` and skip measure for unchanged subtrees, the way WPF does with `_previousAvailableSize` and Masonry does with `MeasurementCache`. Composes directly with damage rendering: if measure didn't fire, encode cache stays valid.
- **Subtree-granularity encode cache.** Replay a contiguous range when no descendant is dirty, instead of N per-node slice replays. Cheaper memcpy and pairs with a Vello-style flat stream representation.
- **Pixel-snapping audit at fractional scales.** Yoga shipped accumulating 1px gaps at scale=1.5; Taffy fixed it (commit aa5b296). Add tests at 1.25 / 1.5 / 1.75 to pin behavior.

## Invalidation model

- **Property tracker / fine-grained dirty propagation.** Hash each widget's input bag per frame so the encode cache can decide invalidation without a full equality check on `(NodeHash, cascade row)`. Distinct from damage rects — this tracks data-input change, not screen-rect change.
- **`request_discard` equivalent for first-frame size mismatch.** When measure produces a different size than last frame (text reflow, cosmic shape miss), re-run the frame invisibly the way egui does. First-frame text widths are likely wrong today.

## Renderer / GPU

- **Instance buffer capacity-retention audit.** Confirm encode → compose → backend retains `Vec` capacity across frames; "alloc-free per frame" is a stated goal but not currently enforced. Iced, quirky, and makepad all keep typed instance buffers across frames.
- **wgpu staging belt / upload pool.** Replace ad-hoc `queue.write_buffer` calls with `wgpu::util::StagingBelt` to batch instance + uniform uploads.
- **Offscreen render targets / mask layer.** No render-to-texture path today, which blocks real drop shadows beyond SDF, blur, masked compositing, and tab transitions. Mark as a known fork point in `DESIGN.md`.
- **Color management discipline.** The glyphon-sRGB-vs-linear concern in `todo.md` applies to every shape — verify surface format matches shader assumptions and pin a test.
- **Push constants vs shared UBO for camera/scissor.** Open question from `references/SUMMARY.md §12.5`. UBO works on stock wgpu (quirky proves it); document the choice.
- ~~**Vello-style flat tag-encoded stream.**~~ Done — `RenderCmdBuffer` (SoA: kinds + starts + u32 arena).

## Input / hit-test

- **Spatial index for hit-test at high N.** `HitIndex` is O(1) by-id but pointer→node walks the cascade table; quad-tree / BVH only matters at thousands of nodes but the data is there. Profile-gated.
- **Focus subsystem.** Tab order, focus ring, keyboard navigation, focus-on-disabled rules — separate concern from the `Id → Any` state map. Masonry has a dedicated focus pass for this.
- **Event coalescing / key repeat / double-click timing.** winit delivers raw events; UI conventions (250ms double-click window, OS key-repeat rate, mouse-motion coalescing) need a centralized layer.
- **IME + clipboard plumbing.** Both required for `TextEdit`; `todo.md` mentions IME but not clipboard.
- **Drag-and-drop with MIME-typed payloads.** Distinct from drag-tracking-with-`Active`-capture — needs payload typing, drop targets, OS file drops.

## Layering / composition

- **Overlay / popup layer.** Tooltips, dropdowns, context menus, and modals must draw outside their parent's clip and above siblings regardless of pre-order. Typically a separate "always on top" tree merged into the encoder pass.
- **Explicit z-order beyond pre-order.** Clay's `zIndex` field on render commands is the model; becomes relevant once popups exist.
- **Multi-window / multi-viewport.** egui's `Viewport` + per-surface `IdMap<PaintList>` is the reference design. Single-surface today.

## Long-list / scroll

- **Scroll region as a subsystem.** State-map persistence is the gate, but the scroll widget itself (clip + child transform + content-size negotiation + over-scroll + momentum) is a real chunk of work.
- **Virtualization / windowed children.** Once scroll exists. Prefer a "virtual children" hook on a single node yielding measured children for the visible window over Flutter's heavyweight sliver protocol.

## Accessibility / i18n

- **accesskit integration.** SUMMARY quotes Masonry: "one week if planned now, a month if not." Per-widget `accessibility_role` + dedicated tree pass.
- **RTL / mirroring.** cosmic-text handles BiDi glyph-side, but stack/grid arrangement and alignment defaults need an LTR/RTL flag.
- **HiDPI / scale-factor change handling.** Per-monitor DPI changes mid-session must invalidate atlas, text shape cache, and the proposed layout cache.

## Tooling / discipline

- **Per-frame allocation audit harness.** Run N frames under `dhat` / a `cap_alloc` global allocator and assert zero allocs after warmup. Makes the alloc-free rule binding instead of aspirational.
- **Profiling spans (tracy or puffin).** One-line `profile_function!` per pass; cheap and the "optimize aggressively" posture wants per-pass timings on demand.
- **Snapshot / golden-image renderer tests.** Pixel-diff each showcase tab against a checked-in reference; catches renderer regressions unit tests miss.
- **Per-frame scratch arena.** A project-wide `bumpalo` for things that are genuinely per-frame transient, instead of every pass solving capacity-retention separately.

## Tree topology

- **Contiguous children slices.** Clay's `children.elements: int32_t*` into a shared array beats linked-list children for cache locality and BFS. SUMMARY §5 marks this as "strictly better, defer until profiling justifies."

## Promote-first shortlist

If picking three to start now, motivated by the alloc-free + optimization-as-craft posture and the in-flight damage work: **(1) cross-frame measure cache** (composes with damage), **(2) per-frame allocation test harness** (makes the rule enforceable), **(3) overlay/popup layer** (unblocks a wide range of widgets; showcase feels half-built without it). accesskit deserves the "do it now or pay 4× later" callout even if deferred.
