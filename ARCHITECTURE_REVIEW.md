# Aperture structure and dependency review

## Executive summary

Aperture's core architecture is sound. The immediate-mode authoring model, per-layer
`Forest`, two-pass layout, retained cascade/damage state, CPU renderer frontend, and
wgpu backend form a coherent pipeline. The data-oriented choices (`Soa`, packed command
buffers, retained scratch, bounded caches) are consistently applied, and the large
algorithm files are generally large because they keep one hot invariant in one place,
not because unrelated subsystems were casually combined.

The main simplification opportunity is dependency direction. Several downstream
artifacts are currently placed under `ui`, the CPU renderer receives the entire `Ui`,
and `Ui` imports a capability bundle owned by `host`. Those choices obscure the
otherwise clean pass boundaries and allow renderer/host code to reach through the
orchestrator instead of consuming named inputs.

The most important state-ownership problem is scrolling. `ScrollLayoutState` combines
widget interaction state with layout output and is mutated by both the widget and
`LayoutEngine`. Splitting it would restore the layout engine's own documented rule that
it produces output without retaining widget state.

Dependency removal is otherwise limited: nearly every direct crate is genuinely used.
`encase` is the one immediate removal candidate. Larger build-graph reductions require
isolating `winit` and the system clipboard behind features.

## Scope

The review covered every production Rust module under `src/`, the
`aperture-anim-derive` subcrate, both manifests, the documented architecture, and the
normal dependency tree. Tests and benchmarks were consulted only to understand pinned
contracts and validation coverage; they were not reviewed as production structure.

## Current structure and data flow

| Area | Current responsibility | Assessment |
| --- | --- | --- |
| `scene/` | Per-layer recorded tree, retained bulk payloads, derived cascade state, hit index, paint bounds, and cross-frame damage | Cohesive recorded and derived scene ownership |
| `layout/` | Measure/arrange drivers, intrinsic queries, cache, text reuse, and scroll rows | Strong except for mixed scroll interaction state |
| `input/` | Native events, routing, capture/focus, subscriptions, and responses | Cohesive state machine; `winit` translation is misplaced |
| `renderer/frontend/` | Scene encoding and CPU composition into `RenderBuffer` | Good CPU/GPU split; input contract is too broad |
| `renderer/backend/` | wgpu resources, scheduling, uploads, passes, and presentation support | Cohesive despite its size |
| `host/` | Winit/offscreen drivers, shared resource root, per-window orchestration | Correct composition root; currently leaks types downward into `Ui` |
| `widgets/`, `text/`, `animation/`, `primitives/` | Authoring API and focused domain services | Generally well isolated |

The current frame flow is:

1. `WinitHost` translates a `WindowEvent` and pushes native events into the window's
   `InputState` (`src/host/winit/mod.rs:638`, `src/host/winit/mod.rs:651`).
2. `WindowDriver` builds `Display` and calls `Ui::frame`
   (`src/host/window_driver.rs:333`, `src/host/window_driver.rs:431`).
3. A full UI frame records `scene::Forest` plus its `RecordStore`, runs layout and cascade, settles
   input/state, and computes damage (`src/ui/mod.rs:159`, `src/ui/mod.rs:518`,
   `src/ui/mod.rs:561`).
4. `FrameReport` carries a private renderer-owned `RenderPlan`; `WindowDriver` selects
   a presentation mode (`src/renderer/plan.rs`, `src/host/window_driver.rs:438`).
5. `Frontend` reads an immutable `FrameScene`, encodes commands, and composes `RenderBuffer`
   (`src/renderer/frontend/mod.rs:57`, `src/renderer/frontend/encoder/mod.rs:143`).
6. `WgpuBackend` submits the buffer, after which `WindowDriver` updates its owned
   output-validity state (`src/host/window_driver.rs:510`, `src/host/window_driver.rs:524`).

The target dependency direction should be:

```text
authoring vocabulary + primitives
              ↓
     scene::{Forest, RecordPayloads}
              ↓
     layout → scene cascade → damage
              ↓
      renderer frontend → backend
              ↓
             host
```

`Ui` should orchestrate the middle passes, but renderer and host code should not depend
on `Ui` as a data container.

## Batch 1 — Restore explicit frame and pass boundaries (high priority)

- [x] **Consolidate recorded and derived scene ownership.** `scene` replaces the old
  top-level `forest` module and owns `Forest`, its element/tree/shape vocabulary,
  retained `RecordStore`, frozen cascades, and cross-frame damage state. This avoids
  introducing overlapping `forest` and `scene` domains while removing input and
  renderer dependencies on the `ui` orchestrator. `RenderPlan`/`RenderKind` live in
  `renderer/plan.rs`; public `FrameReport`, `FramePaint`, and `FrameProcessing` remain
  in `ui/frame_report.rs`. The colocated tests and internals benchmarks mirror the new
  hierarchy. `rg 'crate::ui::(cascade|damage)' src/input src/renderer src/host` and
  `rg 'crate::ui::frame_report::Render' src/renderer src/host` return no production
  imports.

- [x] **Give the renderer a named immutable frame input instead of `&Ui`.**
  `Frontend::build` accepts the entire recorder and directly borrows its payloads,
  display, and clock (`src/renderer/frontend/mod.rs:59`), while `Encoder::encode`
  reaches into forest, layout, cascades, text, gradients, GPU views, and collision
  records (`src/renderer/frontend/encoder/mod.rs:145`,
  `src/renderer/frontend/encoder/mod.rs:157`). Add a `FrameScene<'a>` next to
  `Frontend` containing only `&Forest`, `&Layout`, `&Cascades`,
  `&RecordPayloads`, `&TextShaper`, the gradient handle, GPU views, `Display`, and
  frame time. Construct it after `Ui::frame` freezes those values in
  `WindowDriver::finish_cpu_frame` (`src/host/window_driver.rs:438`). This makes
  encode/compose dependencies visible in the type system, prevents new incidental UI
  reach-through, and lets encoder tests build the smallest fixture they need.
  Validate with encoder/composer tests, the allocation-free benchmarks, and an import
  check showing no production file under `renderer/` imports `crate::ui::Ui`.

- [x] **Move presented-output validity to `WindowDriver`, the component that can
  observe presentation.** Target-owning adapters detect size/format changes and
  invalidate `WindowDriver`; the driver stores `output_valid` beside
  `backbuffer_fresh`, passes its prior value into UI frame classification as a named
  frame-entry field, marks it pending before acquire, and restores it only for a
  valid skip/copy or successful submit. The exact cases cover first frame, target
  change, failed acquire, successful submit, `SkipNoop`, and `SkipCopy`; the next CPU
  frame after any failed paint escalates to full damage.

- [ ] **Move recorder capabilities below the host composition root and narrow them.**
  The host documentation says recorder vocabulary must not depend on host machinery
  (`src/host/mod.rs:9`), but `Ui` imports both `HostShared` and `UiShared`
  (`src/ui/mod.rs:19`) and constructs its default through `HostShared`
  (`src/ui/mod.rs:1259`). Define `UiResources` in `ui/resources.rs`; move the shared
  window directory to `window.rs` and diagnostics handles to a neutral
  `diagnostics/` module; let `HostShared` assemble and clone those lower-level
  capabilities. Also split `RenderAssets`: UI authoring needs only the image registry
  and texture-ID source (`src/ui/mod.rs:862`, `src/ui/mod.rs:899`), while gradients
  are renderer-only (`src/renderer/assets.rs:15`,
  `src/renderer/frontend/encoder/mod.rs:163`). Give UI, frontend, and backend
  capability-specific views instead of cloning the full bundle into each. Validate
  the multi-window sharing tests and confirm no production `ui/` file imports
  `crate::host`.

## Batch 2 — Separate scroll interaction state from layout output (high priority)

- [ ] **Split `ScrollLayoutState` into widget state, ephemeral configuration, and
  layout metrics.** The current row holds interaction fields (`offset`, `zoom`,
  `drag_anchor`), current widget configuration (`content_margin`), and layout outputs
  (`viewport`, `outer`, `content`, `overflow`, `seen`) in one struct
  (`src/layout/scroll/mod.rs:36`, `src/layout/scroll/mod.rs:69`). `Scroll::show`
  mutates the row directly through `ui.layout_engine.scroll_states`
  (`src/widgets/scroll/mod.rs:619`), while measure and arrange write it later
  (`src/layout/scroll/mod.rs:347`, `src/layout/scroll/mod.rs:393`). Move
  `ScrollState { offset, zoom, drag_anchor }` and its input mutation logic to
  `widgets/scroll/state.rs` and store it in `StateMap`; pass `content_margin` as
  current record configuration; keep `ScrollMetrics { viewport, outer, content,
  overflow, seen }` as layout-owned output. The widget should read the previous
  metrics snapshot and mutate only `ScrollState`; layout should consume the recorded
  transform and write only metrics. This restores one-way data flow and the
  `LayoutEngine` contract that finalized output is caller-owned
  (`src/layout/engine.rs:123`). Validate all scroll interaction/layout tests, add a
  test proving a layout run cannot change offset/zoom, and re-run allocation checks
  because both maps are steady-state hot paths.

- [ ] **Remove the global scroll-map fold from the cascade fingerprint after the
  split.** `cascade_fingerprint` scans every retained scroll row and hashes
  offset/zoom separately (`src/scene/cascade/mod.rs:574`,
  `src/scene/cascade/mod.rs:637`), but `Scroll::show` records those values into the
  viewport element's transform (`src/widgets/scroll/mod.rs:794`) and
  `PanelExtras::hash` already includes that transform in the node/subtree hash
  (`src/scene/element/columns.rs:98`, `src/scene/tree/mod.rs:199`). The extra fold
  is redundant, makes cascade reuse depend on stale/unrelated map rows, and is the
  only reason cascade knows about layout-engine scroll storage. Remove the
  `ScrollStates` argument and add pin tests showing offset/zoom changes alter the
  recorded subtree fingerprint while changes to unused/stale metrics do not.

## Batch 3 — Isolate platform adapters and reduce dependencies (medium priority)

- [x] **Move all winit event translation to `host/winit/input.rs`.** `InputEvent`
  claims to be toolkit-independent (`src/input/mod.rs:156`) but its inherent impl
  accepts `winit::event::WindowEvent` (`src/input/mod.rs:264`), and core keyboard
  vocabulary contains three winit mapping functions
  (`src/input/keyboard.rs:199`, `src/input/keyboard.rs:241`,
  `src/input/keyboard.rs:313`). Move the mapper, modifier normalization, physical-key
  mapping, and text-chunk fan-out to the winit adapter; leave `input/` with native
  vocabulary and the routing state machine. `WinitHost::window_event` is already the
  only production caller (`src/host/winit/mod.rs:638`). Move the translation tests
  with the adapter and validate that `rg 'winit::' src/input` is empty.

- [x] **Replace the process-global clipboard with an injected UI capability.**
  `common/clipboard.rs` owns a global `OnceLock<Mutex<...>>` and chooses arboard or
  memory fallback process-wide (`src/common/clipboard.rs:1`,
  `src/common/clipboard.rs:111`, `src/common/clipboard.rs:140`). The supposedly
  buffer/state-focused text-edit model calls it directly for cut/copy/paste
  (`src/widgets/text_edit/model.rs:249`, `src/widgets/text_edit/model.rs:308`), and
  the context menu probes it independently (`src/widgets/text_edit/mod.rs:762`).
  Add a cloneable clipboard capability to `UiResources`; construct the OS-backed
  instance at the windowed host boundary and a memory-backed instance for
  `OffscreenHost`/`Ui::default`. Pass the capability explicitly to edit commands so
  clipboard success still gates destructive cut. This removes hidden global state,
  makes multiple hosts/tests isolatable, and gives headless mode deterministic
  behavior. Validate OS-failure fallback, cross-window sharing within one host,
  isolation between independently constructed hosts, and cut-not-delete-on-write
  failure.

- [x] **Feature-gate platform integrations after their types are isolated.**
  `winit` and `arboard` are unconditional dependencies
  (`Cargo.toml:25`, `Cargo.toml:39`); on Linux they pull the window-system graph even
  for the supported offscreen entry point (`src/host/offscreen.rs:1`). Add
  `winit-host` and `system-clipboard` features, conditionally compile the
  corresponding host/backend and root re-exports (`src/lib.rs:63`), and make the
  showcase require `winit-host`. Defaults may retain today's batteries-included
  behavior, while `--no-default-features` should still build native input, CPU UI,
  and `OffscreenHost` with the memory clipboard. Validate both the normal full suite
  and `cargo check --no-default-features`; inspect `cargo tree -e normal
  --no-default-features` to confirm neither `winit` nor `arboard` remains.

- [x] **Gate the winit adapter as a module, not core entities item by item.**
  The feature dependencies are optional, but winit-only fields, methods, imports,
  and helper variants remain scattered through backend-agnostic modules
  (`src/host/window_driver.rs`, `src/renderer/backend/mod.rs`, `src/window.rs`,
  `src/host/shared.rs`, and `src/common/clipboard.rs`). Keep `WindowDriver` as the
  target-agnostic UI/render core and move swapchain ownership, configure tracking,
  occlusion timing, acquire/present recovery, cursor/command draining, and
  `FramePresent` scheduling into `host/winit/window.rs`. Construct the system
  clipboard at that host boundary, and compile shared vocabulary unconditionally.
  The `winit-host` condition should remain only on the `host::winit` module edge and
  its public `lib.rs` export, rather than changing the shape of core structs and
  impls. Validate default and `--no-default-features` builds, then audit that
  `rg '#\[cfg(_attr)?\([^\]]*winit-host' src` reports only those module/API edges.

- [x] **Remove `encase` for the single eight-byte viewport immediate.** The crate
  uses `encase` only to serialize `ViewportPush { size: Vec2 }`
  (`src/renderer/backend/viewport.rs:89`, `src/renderer/backend/viewport.rs:104`),
  yet it is a direct dependency and also enables glam's `encase` integration
  (`Cargo.toml:28`, `Cargo.toml:32`). Make the immediate an explicit `#[repr(C)]`
  Pod record (or `[f32; 2]`), encode it with `bytemuck`, and retain a compile-time
  eight-byte size assertion beside the shader offset. Remove both the direct
  dependency and glam feature. Validate the exact emitted bytes and confirm
  `cargo tree -i encase` has no result.

## Batch 4 — Put local types and behavior beside their true owners (medium priority)

- [x] **Convert `shape.rs` into a directory module and separate authoring types from
  wire types.** The file combines the public `Shape` enum and its builders
  (`src/shape.rs:27`, `src/shape.rs:234`), public stroke styles
  (`src/shape.rs:559`, `src/shape.rs:607`), an internal record-storage tag
  (`src/shape.rs:649`), renderer Pod wrappers (`src/shape.rs:577`,
  `src/shape.rs:662`), and text-layout policy (`src/shape.rs:695`). Move the whole
  `Shape` definition plus all inherent impls together to `shape/mod.rs`, public
  `LineCap`/`LineJoin` to `shape/style.rs`, `TextWrap` to `text/wrap.rs`, the
  storage-only `ColorMode` beside `ShapeRecord`, and the three Pod wrappers beside
  command payloads (`src/renderer/frontend/cmd_buffer/payload.rs:1`). Keep the flat
  public API through `lib.rs` re-exports. This follows the crate rule that inherent
  impls stay with their struct while removing renderer/storage concerns from the
  authoring file. Validate hot-struct size tests, shape hashing, command-buffer
  round trips, and text-wrap layout tests.

- [x] **Move shared stroke paint bounds out of the render-buffer wire module.**
  Forest lowering and cascade currently import `HALF_FRINGE`/`stroked_bbox` upward
  from renderer storage (`src/scene/shapes/lower.rs:27`,
  `src/scene/cascade/mod.rs:31`), while the definitions live beside
  `CurveInstance` (`src/renderer/render_buffer/curve.rs:19`,
  `src/renderer/render_buffer/curve.rs:39`). Put CPU/shared semantics
  (`HALF_FRINGE`, `MITER_LIMIT`, `stroked_bbox`) in `shape/stroke_bounds.rs` beside
  cap/join vocabulary; leave segment counts, draw tags, and `CurveInstance` in
  `render_buffer/curve.rs`. Renderer/backend, composer, lowering, and scene cascade
  then all depend on the neutral shape rule. Validate bbox edge cases and keep
  compile/test pins proving CPU constants match specialized shader constants.

- [x] **Put frame clock/classification/wake behavior on `FrameRuntime`.**
  `FrameRuntime` owns the clock accumulator, prior stamp, wake queue, and repaint
  flags (`src/ui/frame.rs:71`), but `advance_clock`, `classify_frame`, and
  `schedule_wake` are methods on the much broader `Ui`
  (`src/ui/mod.rs:329`, `src/ui/mod.rs:367`, `src/ui/mod.rs:478`). Move those
  behaviors to `FrameRuntime` in `ui/frame.rs`; use a named
  `FrameClassifyInput` for the few external facts such as input policy/result,
  close request, display, and prior-output validity. Leave `Ui::frame` as the
  orchestration method. This reduces `ui/mod.rs` without splitting `impl Ui`
  arbitrarily and makes the frame-state machine testable without constructing a
  forest, text shaper, and renderer resources. Validate a table covering first
  frame, display change, real/animation/coalesced wakes, input policy, close request,
  and invalid prior output.

- [ ] **Split the frame-stat widget from backend-facing debug configuration.**
  `debug_overlay.rs` is both a backend-facing plain configuration type and a UI
  construction helper; as a result the module imports forest, layout, text, `Ui`,
  and widgets (`src/debug_overlay.rs:11`) even though the backend only needs
  `DebugOverlayConfig` (`src/renderer/backend/mod.rs:36`). Keep the configuration in
  the neutral `diagnostics/` module established by Batch 1 and move
  `record_frame_stats` (`src/debug_overlay.rs:53`) to `ui/frame_stats.rs`.
  This removes a misleading cross-layer module and lets backend diagnostics depend
  only on plain data. Validate all three overlay modes and the existing frame-stats
  damage test.

## Deliberate non-changes

- Do not merge the stack/grid/wrap layout drivers. Their duplication reflects
  different layout semantics, and the shared axis/support helpers already capture
  the reusable mechanics.
- Do not cache encode or compose output. The existing allocation-retaining,
  always-rebuild model matches the crate's documented invalidation strategy.
- Do not split `renderer/frontend/composer/mod.rs` merely because it is large. Its
  main match and retained overlap/batching scratch enforce one paint-order invariant;
  `higher_kind`, `occlusion`, and `text_grid` already contain the independently useful
  algorithms.
- Do not split `renderer/backend/mod.rs` by moving `WgpuBackend` impl blocks into
  unrelated files. The existing pipeline/schedule/resource submodules are the useful
  boundaries; the remaining methods coordinate one backend-owned state machine.
- Do not introduce a generic `types` or `utils` dumping ground to erase every module
  cycle. Move only vocabulary with a clear neutral owner, as described above.

## Open questions

- [x] **Direct custom-host driving is not a supported public API.** `Ui::frame` and
  `FrameStamp` are crate-private; `WinitHost` and `OffscreenHost` own the complete
  frame-entry/presentation protocol. The allocation harness uses an `internals`-only
  test-frame entry instead of exposing a partial production host contract.

- [x] **Headless-without-window-system is a supported build profile.**
  `winit-host` and `system-clipboard` remain default features for batteries-included
  builds, while `--no-default-features` keeps native input, CPU UI, `OffscreenHost`,
  and the memory clipboard without resolving either platform dependency.
