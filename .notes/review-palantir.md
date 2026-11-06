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
│  │  ├─ resources: UiResources (7 separate refcounted fields)
│  │  ├─ frontend: Frontend { encoder, composer (Rc<SharedDevice>), buffer }
│  │  └─ render_owners
│  └─ device: DeviceCore
│     ├─ spec: BackendSpec { image_store (shared), timing }
│     └─ backend: WgpuBackend (19 fields, image_store (shared))
├─ windows: WindowSet
│  └─ Window { window: Arc, surface, driver, translator, clock, remote, … }
│     └─ driver: WindowDriver
│        ├─ frames: FrameLoop (pub fields)
│        │  ├─ ui: Ui
│        │  │  ├─ forest, window_theme, memory, overlays, frame_runtime, host
│        │  │  ├─ resources: UiResources (clone)
│        │  │  └─ input: Input { arranged: { cascade, layout }, … }
│        │  └─ layout, cascade, damage (engines)
│        ├─ stream: RenderStream
│        ├─ scissors, damage_base, target, target_outlined, pane_captures
│        └─ screenshot_requests, captures
├─ frame_log: Arc<FrameLog>   (also inside handle)
└─ handle: HostHandle<T>
```

One painted frame, 10 named frames deep to the app:

```text
WinitRuntime::draw
└─ WindowSet::run
   └─ Window::frame
      ├─ DeviceCore::record(cpu, driver, spec, app)
      │  └─ CpuCore::record(driver, spec, app, FrameDepth::Paint(placements))
      │     └─ WindowDriver::cpu_frame(frontend, stamp, app, depth)
      │        ├─ FrameLoop::frame(stamp, base, app)
      │        │  └─ record_pass(app) → record_user(app) → App::record
      │        └─ finish_cpu_frame(frontend, placements, report)
      │           └─ build, plan_cutouts → RenderStream → CutoutPlan
      └─ Window::present(core, cpu_frame)
         └─ acquire_and_present(core, mode)
            └─ DeviceCore::submit(cpu, driver, target, mode)
               └─ WindowDriver::render_to_texture(frontend, backend, target, mode)
                  └─ WgpuBackend::submit → upload_frame, passes
```

What rides down that stack, counted as the signatures that take it:

| Value | Today | Target | What changes |
|---|---|---|---|
| `app` | 7 | 7 | Nothing: the app is the frame's input |
| `frontend` | 3 | 3 | It rides in one `BuildCtx`, and only after the record ends |
| `placements` | 6, two of them inside an enum | 4 | It rides in the same `BuildCtx` |
| `mode`, `scissors`, the buffer | 5, and two fields | 4 | One `BuiltFrame` holds what three places held |
| `cpu` at submit | 1, which only hands over `frontend` | 0 | Removed |
| `FrameDepth` | 2 | 0 | Removed, with `HeadlessDepth` |

The hop count falls little, and that is the honest result: most of these values reach real work. The gain is in the signatures and the types:

| Signature today | Target |
|---|---|
| `cpu_frame(frontend, stamp, app, depth)` | `record(spec, app)`, `compose(frontend, report)`, `build_paint(ctx, report)` |
| `CpuCore::record(driver, spec, app, depth)` | Removed |
| `DeviceCore::submit(cpu, driver, target, mode)` | `submit(driver, target, built)` |
| `render_to_texture(frontend, backend, target, mode)` | `submit(backend, target, built)` |
| `list_panes(tree, layout, lc, display_scale)` | `list_panes(source, lc)` |
| `ToggleFace::paint(ui, id, slot, chrome, checked)` | `paint(ui, toggle_box)` |
| `double_click` in five `Input` signatures | A field on `Input` |

## Root causes

1. **One call runs three stages.** `cpu_frame` records, decides the present path and builds, under a `FrameDepth` parameter. So `frontend` and `placements` ride through `CpuCore::record` and `cpu_frame`, which need them only after the record ends. Four enums hold one fact: `Depth`, `HarnessPaint`, `HeadlessDepth`, `FrameDepth`.
2. **A built frame has no owner.** Its parts are `PresentPath` on the host's stack, `WindowDriver::scissors`, `Frontend::buffer` and the encoder's gradient epoch. Five `debug_assert!`s in `render_to_texture` and one convention hold them together. A host that records window B before it submits window A paints B's list into A's target, and no type stops it. `damage_base` has six write sites.
3. **Escalation is a sequence of resets.** `finish_cpu_frame` is 107 lines. It holds `mode` and `plan` as two variables that must agree, calls `path` four times, `decide_panes` four times and `build` twice. Each escalation is one way, from partial to whole, so there are two end states and not five.
4. **Hosts reach through the window's layers.** 71 non-test lines in `host` name `.frames.`, and 38 of them reach `.frames.ui`. `FrameLoop`'s engines are `pub(crate)` fields. A screenshot makes four stops on its way through, two on `Ui` and two on the driver, and `WindowDriver` mixes that lane with the present decision in 842 lines.
5. **One shared concept holds many refcounts, and some facts have two routes.** `UiResources` clones seven handles for one idea, the host's shared state. `Composer` holds an `Rc<SharedDevice>` for a limit that `Frontend::attach_device` already receives. `WinitRuntime` holds `frame_log` beside the `handle` that contains it.
6. **The module order has no check.** `AGENTS.md` states the order, and `ui/mod.rs` imports `widget_core::overlay_stack` against it. That one edge is the only shipped violation, found by a scan of each `crate::` path. `OverlayStack` imports only `primitives`.
7. **Two orchestrators outgrew their stages.** `FrameLoop::frame` is 199 lines and `WgpuBackend::submit` is 299. `frame_runtime.begin_pass` is a per-pass reset outside `pre_record`, the place whose doc says each such reset goes.

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
│  ├─ backend.prepare_format, driver.take_device_news(backend)
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

A harness below `Depth::Gpu` calls `WindowDriver::record`, then `WindowDriver::compose` at `Depth::Compose`. `FrameDepth` and `HeadlessDepth` go, and `CpuCore::record` goes with them.

### The built frame

```rust
/// What a frame's GPU half does. It borrows the frontend, so no other window can build until
/// this one is submitted or dropped.
pub(super) enum BuiltFrame<'f> {
    /// Nothing to draw, and the target holds the last frame.
    Still,
    /// Nothing to draw, and the target takes the backbuffer's copy.
    CopyBackbuffer,
    Paint(PaintJob<'f>),
}

pub(super) struct PaintJob<'f> {
    route: Route,
    plan: RenderPlan,
    scissors: RepaintScissors,
    frontend: &'f mut Frontend,
}
```

- `PresentPath` goes, as `Route` plus `RenderPlan` hold the same two facts.
- `WindowDriver::scissors` goes. `PaintJob::new` is the one place that pairs a plan with its scissors, which replaces a `debug_assert!`.
- `submit` consumes the value, so a stale `mode` from an earlier frame cannot exist.
- Dropping a `BuiltFrame` on a failed acquire or a lost device is correct. The `Discarded` base already forces the next frame whole.
- The move costs one copy of about 656 B a frame (40 × 16 B `URect`s and a length), which is negligible.

### The present decision

```rust
fn build_paint<'f>(&mut self, mut ctx: BuildCtx<'f>, report: FrameReport) -> BuiltFrame<'f> {
    // Forced frames and screenshots shape the plan first.
    let Some(plan) = self.effective_plan(report) else { return self.still_frame() };
    let built = match self.try_partial(&mut ctx, plan) {
        Ok(built) => built,
        Err(escalation) => self.whole(&mut ctx, plan.to_full(), escalation),
    };
    // Attached last: a `Result` that held the frontend's borrow would not release it on `Err`.
    BuiltFrame::Paint(built.into_job(ctx.frontend))
}
```

- `try_partial` runs each refusal once: the promote share, a stale backbuffer, too many captures, a kept target, too many scissors, a stale cutout census.
- `whole` cannot refuse. It asserts `Census::Cur`.
- `Escalation` keeps the route a refusal asks for, so a retained target that cannot keep its pixels still goes through the backbuffer, as it does today. Each current outcome holds.
- The rule of a second build stays where it is needed: a stale census is the one refusal after a build.

The pure parts move to `host/window_driver/present_decision.rs`: `promote_share`, `density_share`, `rows_per_mpx`, `Escalation`, the route choice.

### Module layout

- `host/window_driver/mod.rs`: the driver and its stages.
- `host/window_driver/present_decision.rs`: the pure decision above.
- `host/window_driver/screenshot_lane.rs`: `ScreenshotLane { requests, pending }`, with `take_from`, `is_capturing`, `deliver`, `capture_all`, `requeue_lost`, `drop_unpainted`.
- `ui/overlay_stack.rs`: `OverlayStack`, moved out of `widget_core`.
- `ui/resources/mod.rs`: `UiResources(Rc<SharedResources>)`.

### Shared ownership

| Holder | Verdict | Why |
|---|---|---|
| `UiResources.settings: Rc<AppSettings>` | **Replace** with a field of `SharedResources` | One `Rc` serves |
| `UiResources.device: Rc<SharedDevice>` | **Replace** with a field of `SharedResources` | One `Rc` serves |
| `WindowDirectory.tokens: Rc<RefCell<_>>` | **Replace** with `RefCell` in `SharedResources` | Only `UiResources` holds it |
| `Composer.device: Rc<SharedDevice>` | **Replace** with a `TextureLimit` value | `Frontend::attach_device` already carries the limit |
| `WinitRuntime.frame_log: Arc<FrameLog>` | **Replace** with `handle.frame_log` | The runtime holds that handle |
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

`UiResources` falls from 64 B to 8 B on `Ui`. A settings read still takes one pointer chase, so `Ui::animate` pays nothing new. A text read takes two in place of one, on a path that then shapes text.

### Refused

- **A `HostCore` method for each step**: `OffscreenHost` and `Harness` hold `cpu` inside `HeadlessWindow` and `device` beside it, so `HostCore` is not on their path.
- **A `Frame<'a>` context that holds `app`, `frontend` and `backend` for the whole stack**: it hides which stage reads what, which is the defect in root cause 1.
- **Typestate markers on `WindowDriver`** (`WindowDriver<Recorded>`): a driver lives in a `Vec` across frames, and a consumed value gives the same guarantee at no cost to its type.
- **Merging `WindowDriver` into `FrameLoop`**: `FrameLoop` sits in `ui`, left of `host`, and it needs nothing from the device.
- **Merging the text clock and `DisplayFrames`**: they look alike, but a `TextShaper` can span two hosts and a backend cannot, so each keeps its own round.
- **A trait for the `Ui`, `UpdateCtx` and `Input` reads**: a caller must import it, and `const fn` goes.

### Decided: the mirrored reads stay

- **The 16 `Input` reads stay on `Ui`, and `Ui` gets no public `input()`.** The 17 `UpdateCtx` reads stay too. All 33 wrappers are sugar under the one-route rule: they add no behavior, and call sites read better. Refused: a public `Ui::input() -> &mut Input` that removes the 16 wrappers, as egui's `ui.input(|i| …)` reads, which moves every `ui.focus()` caller in darkroom and the showcase and makes each call longer. Cost: a new read is written up to three times.

# Part 2: Plan

Each batch leaves the build and the tests green. Whoever addresses an item deletes it. No batch changes the public API, so `.notes/API_SURFACE.md` stays as it is.

## Batch 1: Push the depth up

Files: `src/host/window_driver/mod.rs`, `src/host/cpu_core.rs`, `src/host/device_core.rs`, `src/host/headless_window.rs`, `src/host/harness/mod.rs`, `src/host/offscreen/mod.rs`, `src/host/core.rs`, `AGENTS.md`.

- [ ] `WindowDriver::cpu_frame`: it takes `frontend` and `FrameDepth` and runs the record below them. Split it into `record(spec, app) -> FrameReport`, `compose(frontend, report)` and `build_paint`.
- [ ] `CpuCore::record`: it only forwards. Delete it, and move `note_target` and `stamp` into `WindowDriver::record`.
- [ ] `FrameDepth`: delete it. `DeviceCore::record` becomes `DeviceCore::frame`, the one painted sequence.
- [ ] `HeadlessDepth` and `HeadlessFrame`: they mirror `HarnessPaint`. Give `HeadlessWindow` `record_frame`, `compose_frame` and `paint_frame`, and let `HarnessPaint` match at its own level. `OffscreenHost` calls `paint_frame` alone.
- [ ] `WindowDriver::plan_cutouts`: a one-line forward. Call `RenderStream::plan_cutouts` directly.
- [ ] `src/host/core.rs` tests: the `FrameSpec` literal repeats. Add one `spec_of(&target)` fixture.
- [ ] `AGENTS.md`, **Harness**: update `DeviceCore::record` to the new name.

## Batch 2: One escalation

Files: `src/host/window_driver/mod.rs`, new `src/host/window_driver/present_decision.rs`, `src/host/window_driver/tests.rs`.

- [ ] `finish_cpu_frame`: replace the sequence of resets with `effective_plan`, `try_partial -> Result<_, Escalation>` and an infallible `whole`, after **The present decision**.
- [ ] `path` closure: it takes `backbuffer_fresh` as a parameter for a value that holds for the whole call. Read it once.
- [ ] Move the pure decision functions to `present_decision.rs`.
- [ ] Test: a table over strategy, partial, capturing, fresh, pane overflow, scissor overflow and stale census, which asserts route, damage kind and build count for each row, with the expected row worked out by hand.
- [ ] Keep `src/host/offscreen/tests/promote.rs` and `forced_frame.rs` unchanged, as they pin today's outcomes.

## Batch 3: The built frame

Files: `src/host/window_driver/mod.rs`, `src/host/device_core.rs`, `src/host/headless_window.rs`, `src/host/winit/window/mod.rs`, `src/gpu/frame/submission.rs`.

- [ ] Add `BuiltFrame<'f>` and `PaintJob<'f>`. Delete `PresentPath` and `WindowDriver::scissors`.
- [ ] `CpuFrame`: give it the lifetime and `built`. `DeviceCore::submit` loses `cpu`.
- [ ] `render_to_texture`: rename it `submit`, and delete the `debug_assert!` that `PaintJob::new` replaces.
- [ ] `damage_base`: three arms of `render_to_texture` set `Prev`. Write it once at the end of `submit`, which leaves four: `invalidate_target_contents`, the painted frame's `Discarded`, that one `Prev`, and the `Prev` of a frame below paint.
- [ ] `Window::frame`: destructure `HostCore` into `cpu` and `device`, so `present` takes `device` alone.
- [ ] `Window::acquire_and_present`: the `Ready` and `Suboptimal` arms repeat. Merge them.
- [ ] `HeadlessWindow::paint_frame` and `Window::present`: each spells the lost-device skip its own way. Keep one spelling through `DeviceCore`.
- [ ] Test: dropping a `BuiltFrame` unsubmitted leaves the next frame `Damage::Full`.

## Batch 4: One level to the window

Files: `src/ui/frame_loop.rs`, `src/host/window_driver/mod.rs`, new `src/host/window_driver/screenshot_lane.rs`, `src/host/winit/*`, `src/host/harness/*`, `src/host/offscreen/mod.rs`.

- [ ] `FrameLoop`: make `layout`, `cascade` and `damage` private. Add `dirty_panes()`, and give the harness its reads under `internals`.
- [ ] `WindowDriver.frames`: make it private. Add `ui()`, `ui_mut()`, `token()` and the few host calls, and move the 71 `.frames.` lines to them.
- [ ] `FrameLoop::frame_scene` and `scene_of`: `scene_of` exists for one bench reach. Keep one of them.
- [ ] Add `ScreenshotLane`, and move `screenshot_requests`, `captures`, `deliver_screenshot` and the capture loop into it.
- [ ] `finish_cpu_frame`: it writes `report.repaint_requested` after the report exists. Return the lane's need beside the report, and let `FrameReport` stay what the frame said.

## Batch 5: One shared owner

Files: `src/ui/resources/mod.rs`, `src/ui/resources/app_settings.rs`, `src/window/window_directory/mod.rs`, `src/renderer/frontend/mod.rs`, `src/renderer/frontend/composer/mod.rs`, `src/renderer/frontend/composer/session.rs`, `src/host/cpu_core.rs`, `src/host/winit/runtime.rs`, `src/internals/hot_struct_sizes.rs`.

- [ ] `UiResources`: seven refcounted fields for one concept. Make it `UiResources(Rc<SharedResources>)`, and hold `AppSettings`, `SharedDevice` and `WindowDirectory` by value inside it.
- [ ] `WindowDirectory.tokens`: drop the `Rc`.
- [ ] `Composer.device`: replace it with a `TextureLimit` set through `Frontend::attach_device`, and take the `Rc` off `Frontend::new`. Test: an attach moves the `GpuView` ceiling in the next build.
- [ ] `WinitRuntime.frame_log`: read `self.handle.frame_log`.
- [ ] `hot_struct_sizes`: update `UI_SIZE` and `FRAME_LOOP_SIZE`.

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
