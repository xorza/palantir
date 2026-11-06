# Palantir: call graph and ownership review

The shape of palantir is canonical down to the host layer. The host layer between `Window::frame` and `FrameLoop::frame` is the part to change.

Raw argument count is not the defect. Of 5,578 shipped functions, 11 take five or more parameters and one takes six. The defects are pass-through layers and frame state that no one value owns.

No behavior bug came from the traced paths: frame, warmup, present decision, failed acquire, device loss, window close. The test lane is green: 2,528 tests, and the library suite takes 1.26 s.

# Part 1: Proposal

## What stays

Each item below is the established shape. The plan does not touch it.

- **`FrameLoop` is the composition root**, as Masonry's [`RenderRoot`](https://docs.rs/masonry_core/latest/masonry_core/app/struct.RenderRoot.html) owns the tree and runs the passes a host asks for. It runs layout, then what derives from layout, then paint, as Flutter's [`PipelineOwner`](https://api.flutter.dev/flutter/rendering/PipelineOwner-class.html) flushes its phases.
- **`&mut Ui` in each widget signature stays.** The 163 sites are immediate mode's one context. egui's [`Context`](https://docs.rs/egui/latest/egui/struct.Context.html) holds its state behind an `RwLock`, where a recursive lock can deadlock. An exclusive borrow cannot.
- **`UpdateCtx` as a view of disjoint fields stays.** Rust has no [view types](https://smallcultfollowing.com/babysteps/blog/2021/11/05/view-types/) yet ([project goal](https://rust-lang.github.io/rust-project-goals/2026/roadmap-borrow-checker-within.html)), so a struct of field borrows is the available form.
- **Recursive walks take one context**: `LayoutPass`, `PlaneCtx`, `PlaneWalk`, `DrawCtx`, `GpuCtx`. This is the correct answer to argument threading.
- **`Input` keeps `Arranged`.** One owner is correct, and the refused choice threaded it through 27 signatures. The cost is one `mem::take` in the warmup and 14 `input.arranged` paths outside `input`.
- **`&Gpu` stays a parameter** at 98 sites. Each is a builder or a helper that makes one GPU object, so a context would only rename it.
- **`CpuCore` and `DeviceCore` stay two types**, as `AGENTS.md` decides, and the harness below `Depth::Gpu` holds the first alone.
- **The warmup's scratch `Input` stays.** A new field drops by default, which is the safe default for a pass that must see no input.

## The graphs today

Ownership, with each `Rc` that crosses a window boundary marked `(shared)`:

```text
WinitRuntime<T>
├─ app: T
├─ surfaces: SurfaceManager
├─ core: HostCore
│  ├─ cpu: CpuCore
│  │  ├─ resources: UiResources (one Rc<SharedResources>)
│  │  ├─ frontend: Frontend { encoder, composer, buffer }
│  │  └─ render_owners
│  └─ device: DeviceCore
│     ├─ spec: BackendSpec { image_store (shared), timing }
│     └─ backend: WgpuBackend (19 fields, image_store (shared))
├─ windows: WindowSet
│  └─ Window { window: Arc, surface, driver, translator, clock, remote, … }
│     └─ driver: WindowDriver
│        ├─ frames: FrameLoop (private engines)
│        │  ├─ ui: Ui
│        │  │  ├─ forest, window_theme, memory, overlays, frame_runtime, host
│        │  │  ├─ resources: UiResources (clone)
│        │  │  └─ input: Input { arranged: { cascade, layout }, … }
│        │  └─ layout, cascade, damage (engines)
│        ├─ stream: RenderStream
│        ├─ damage_base, target, target_outlined, pane_captures
│        └─ screenshots: ScreenshotLane { requests, pending }
└─ handle: HostHandle<T> { frame_log: Arc<FrameLog>, … }
```

One painted frame, 9 named frames deep to the app:

```text
WinitRuntime::draw
└─ WindowSet::run
   └─ Window::frame
      ├─ DeviceCore::frame(cpu, driver, spec, app)
      │  ├─ WindowDriver::record(spec, app)
      │  │  └─ FrameLoop::frame(stamp, base, app)
      │  │     └─ record_pass(app) → record_user(app) → App::record
      │  └─ WindowDriver::build_paint(BuildCtx { frontend, placements }, report)
      │     └─ build, RenderStream::plan_cutouts → CutoutPlan
      └─ Window::present(device, cpu_frame)
         └─ acquire_and_present(device, built) → submit_and_present
            └─ DeviceCore::submit(driver, target, built)
               └─ WindowDriver::submit(backend, target, built) → submit_paint(job)
                  └─ WgpuBackend::submit → upload_frame, passes
```

What rides down that stack, counted as the signatures that take it. The hop count stays, as each value reaches real work:

| Value | Signatures | Where it stops |
|---|---|---|
| `app` | 6 | `App::record`, the frame's input |
| `frontend` | 4, three inside `BuildCtx` | `Frontend::build` |
| `placements` | 5, three inside `BuildCtx` | `CutoutPlan::build` |
| `built` | 4 | `WindowDriver::submit`, one value in place of `mode`, `scissors` and the buffer |

The remaining gain is in the signatures:

| Signature today | Target |
|---|---|
| `list_panes(tree, layout, lc, display_scale)` | `list_panes(source, lc)` |
| `ToggleFace::paint(ui, id, slot, chrome, checked)` | `paint(ui, toggle_box)` |
| `double_click` in five `Input` signatures | A field on `Input` |

## Root causes

1. **The module order has no check.** `AGENTS.md` states the order, and `ui/mod.rs` imports `widget_core::overlay_stack` against it. That one edge is the only shipped violation, found by a scan of each `crate::` path. `OverlayStack` imports only `primitives`.
2. **Two orchestrators outgrew their stages.** `FrameLoop::frame` is 199 lines and `WgpuBackend::submit` is 299. `frame_runtime.begin_pass` is a per-pass reset outside `pre_record`, the place whose doc says each such reset goes.

## Target design

### Rules

These rules make the design one design, and each later change tests against them.

- **A stage is a method on the owner of its output, and it takes only that stage's inputs.** The caller picks the stages, as [branches gather at the top](https://matklad.github.io/2023/11/15/push-ifs-up-and-fors-down.html). No stage takes a depth or a mode.
- **A product moves out of the stage that makes it and into the next.** It does not wait in a field. Masonry's `redraw` hands back its scene by value in the same way.
- **A step that must not repeat consumes its value**, as [typestate](https://cliffle.com/blog/rust-typestate/) does and as `Widget::record` and `SurfaceTexture::present` already do.
- **A bundle names a stage's whole input.** It holds no value derived from another of its fields, and a recursive walk has one context.
- **A host talks to a window through `WindowDriver`**, one level down.
- **One `Rc` holds what a host shares.** A separate refcount exists only for a handle an app holds or drops, or for a thread or a wgpu callback.

### Frame call graph

```text
Window::frame
├─ DeviceCore::frame(cpu, driver, spec, app) -> CpuFrame<'cpu>
│  ├─ backend.prepare_format, driver.deliver_screenshot, driver.forget_evicted_panes
│  ├─ WindowDriver::record(spec, app) -> FrameReport
│  │  └─ FrameLoop::frame(stamp, base, app)
│  │     ├─ open_frame(stamp, base) -> FramePlan
│  │     ├─ record_frame(app, plan) -> FrameProcessing
│  │     └─ close_frame(plan, processing) -> FrameReport
│  └─ WindowDriver::build_paint(BuildCtx { frontend, placements }, report) -> BuiltFrame<'cpu>
└─ Window::present(device, cpu_frame)
   └─ DeviceCore::submit(driver, target, built)
      └─ WindowDriver::submit(backend, target, built)
```

A harness below `Depth::Gpu` calls `WindowDriver::record`, then `WindowDriver::compose` at `Depth::Compose`, then `WindowDriver::finish_unpainted`.

### Module layout

- `host/window_driver/mod.rs`: the driver and its stages.
- `host/window_driver/present_facts.rs`: the pure decision: `PresentFacts`, `Escalation`, the promote share.
- `host/window_driver/screenshot_lane.rs`: `ScreenshotLane`, each screenshot from the ask to the image.
- `ui/overlay_stack.rs`: `OverlayStack`, moved out of `widget_core`.
- `ui/resources/shared_device.rs`: `SharedDevice`, moved out of `renderer`, as only `UiResources` holds it now.

### Shared ownership

| Holder | Verdict | Why |
|---|---|---|
| `UiResources(Rc<SharedResources>)` | Keep | `&mut Ui` has no lifetime to lend from, and every window shares it |
| `Rc<Theme>`, 21 `Rc::clone(ui.theme())` | Keep | An immutable value too costly to copy, and it must outlive a `&mut Ui` reborrow |
| `TextShaper(Rc<Shared>)`, `RefCell` | Keep | Public, and two hosts may share one through `OffscreenHostBuilder::shaper` |
| `FamilyKeys(Rc<RefCell<_>>)` | Keep | Each window's `Forest` keys text as it records, with no reach to the shaper |
| `IconRegistry`, `IconSet`, `Weak`, `Rc<IconTable>` | Keep | An app drops an `IconSet` at any time |
| `ImageRegistry`, `ImageHandle`, `Rc<ImageToken>` | Keep | An app drops an `ImageHandle` at any time, and `&mut` cannot reach the store then |
| `Rc<WgpuImageStore>`, `RefCell` | Keep | Recorders write from `&Ui` and the backend reads at draw |
| `GpuPaintRef(Rc<RefCell<dyn GpuPaint>>)` | Keep | Public: the app and the view both hold it. No borrow overlaps, as the app is off the stack at submit |
| `Clipboard(Rc<RefCell<_>>)` | Keep | Public handle, read inside a walk that already holds `&mut Ui` |
| `Waker`, `WakeSignal`, `WakeLatch`, `FrameLog` (`Arc`) | Keep | Other threads hold them |
| `Arc<WinitWindow>`, `Arc<dyn InstanceDisplay>` | Keep | wgpu's `'static` surface asks for it, and `retire` asserts the last share |
| `MapState`, `DeviceHealth`, timing `done` flags, `gpu.rs` `polling` and `mapped` | Keep | Each is written by a wgpu callback, which has no owner to borrow |
| `Gpu.pipeline_turn: Arc<Mutex<()>>` | Keep | Decided in `AGENTS.md`, **Pipeline builds** |
| `Remote` `given_up: Arc<AtomicBool>` | Keep | Two threads share it |
| `common::id_counter` atomics | Keep | Ids stay unique across hosts in a process |

### Refused

- **A `HostCore` method for each step**: `OffscreenHost` and `Harness` hold `cpu` inside `HeadlessWindow` and `device` beside it, so `HostCore` is not on their path.
- **A `Frame<'a>` context that holds `app`, `frontend` and `backend` for the whole stack**: it hides which stage reads what, which is the defect the stage split removed.
- **Typestate markers on `WindowDriver`** (`WindowDriver<Recorded>`): a driver lives in a `Vec` across frames, and a consumed value gives the same guarantee at no cost to its type.
- **Moving a screenshot's wake out of `FrameReport`**: `OffscreenHost::frame` returns the report, and its caller has no other sign that the window owes a frame.
- **Merging `WindowDriver` into `FrameLoop`**: `FrameLoop` sits in `ui`, left of `host`, and it needs nothing from the device.
- **Merging the text clock and `DisplayFrames`**: they look alike, but a `TextShaper` can span two hosts and a backend cannot, so each keeps its own round.
- **A trait for the `Ui`, `UpdateCtx` and `Input` reads**: a caller must import it, and `const fn` goes.

### Decided: the mirrored reads stay

- **The 16 `Input` reads stay on `Ui`, and `Ui` gets no public `input()`.** The 17 `UpdateCtx` reads stay too. All 33 wrappers are sugar under the one-route rule: they add no behavior, and call sites read better. Refused: a public `Ui::input() -> &mut Input` that removes the 16 wrappers, as egui's `ui.input(|i| …)` reads, which moves every `ui.focus()` caller in darkroom and the showcase and makes each call longer. Cost: a new read is written up to three times.

# Part 2: Plan

Each batch leaves the build and the tests green. Whoever addresses an item deletes it. No batch changes the public API, so `.notes/API_SURFACE.md` stays as it is.

## Batch 6: Frame stages

Files: `src/ui/frame_loop.rs`, `src/ui/frame_runtime/mod.rs`, `src/input/mod.rs`, `src/input/capture.rs`.

- [ ] `FrameLoop::frame`: 199 lines of five stages. Split it into `open_frame`, `record_frame` and `close_frame`, with each stage's comments beside it.
- [ ] `frame_runtime.begin_pass`: move it into `pre_record`, as that fn's doc says a per-pass reset goes there. Extend `src/ui/tests/frames/passes.rs` to hold that a first frame still runs its warmup and at most two record passes.
- [ ] `Input::on_input`, `next_frame`, `apply`, `apply_held`, `on_pointer_pressed`: a `double_click` setting rides five signatures. Hold it as a field on `Input`, which `FrameLoop` refreshes at its two entries, `on_input` and `frame`.
- [ ] `src/ui/mod.rs:636`: the comment names a seam that moved to `FrameLoop`. Delete it.

## Batch 7: Backend stages

Files: `src/gpu/wgpu_backend/mod.rs`, `src/gpu/pipeline/format_pipelines.rs`.

- [ ] `WgpuBackend`: `PipelineSources` is written out twice, as a helper on `&self` would borrow all 19 fields. Group `quad`, `mesh`, `image`, `curve` and `raster` in a `Programs` field with `sources()`.
- [ ] `WgpuBackend::submit`: 299 lines. Split it at its seams: `prepare_target -> FrameTarget`, `ensure_pipelines`, `run_steps`.
- [ ] `FrameUpload`: `clear` and `dims_undamaged` derive from its own `submission`. Delete both fields, and read them from `submission`.

## Batch 8: Layers and small signatures

Files: `src/widget_core/overlay_stack.rs`, `src/ui/mod.rs`, `src/widget_core/overlay_scope.rs`, `src/widgets/modal/mod.rs`, `src/internals/`, `src/cascade/engine.rs`, `src/widgets/toggle_row/mod.rs`, `src/widgets/checkbox/mod.rs`, `src/widgets/radio_group/mod.rs`, `src/widgets/switch/mod.rs`, `src/widgets/checkerboard.rs`, `src/widgets/dock/dock_view.rs`, `src/gpu/backdrop/backdrops/mod.rs`, `src/gpu/backdrop/backdrop_blur/mod.rs`, `src/layout/drivers/grid/`.

This batch has no dependency, so it can run first.

- [ ] `OverlayStack`: move it to `src/ui/overlay_stack.rs`, which removes the `ui → widget_core` edge.
- [ ] Add a test under `internals` that scans each `crate::` path in `src/` against one table of the order, with `golden`, `internals`, `bench` and bench files outside it, as `AGENTS.md` says. It proves it can fail on a seeded line, so a clean run says something.
- [ ] `cascade::engine::list_panes` and `narrowest_side`: `tree`, `layout` and `display_scale` are the fields of `TreeSource` in the same file. Take `TreeSource`.
- [ ] `ToggleFace::paint`: five parameters, and two of four impls ignore two. Take one `ToggleBox<'_> { id, slot, chrome, checked }`.
- [ ] `Checkerboard::paint_chip`, `DockView::drop_target`, `composite_group`, `Backdrops::sampled`, `grid::distribute_span`, the grid `of`: each takes five or six. Give each a param struct under the four-parameter rule.

## Batch 9: Stale names

Files: `src/window/window_directory/mod.rs`, `src/host/window_driver/mod.rs`, `src/host/device_core.rs`, `src/host/winit/window/mod.rs`, `src/host/mod.rs`, `src/ui/frame_loop.rs`.

- [ ] `WindowDirectory`'s doc, `WindowDriver::new`'s doc and `DeviceCore::close`'s doc: each says the driver registers and retires a token. `FrameLoop::new` and `FrameLoop`'s `Drop` do it.
- [ ] `src/host/winit/window/mod.rs`, in `Window::frame`: the comment names `Ui::set_window_facts`, now on `FrameLoop`.
- [ ] `src/host/mod.rs`: the module doc names two drivers, and `harness` is a third.
- [ ] `src/ui/frame_loop.rs`: the Tracy zones read `Ui::frame` and `Ui::record_pass`. Rename them, and check that `.notes/PERF.md` and the showcase's bench notes name no old zone.
