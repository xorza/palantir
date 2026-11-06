# Review: `Ui`, `Input` and the frame lifecycle

Status: waits for a go-ahead, because it changes the public API. This file replaces
`input-stage-proposal.md`. Delete an item when it lands, and delete this file with the last one.
Keep each lasting decision in the doc comment of the type it governs.

# Part 1: Proposal

## Result

Each stage of a frame has one type, and each type offers only what its stage may do.

| Stage | Who runs it | Type | Owns |
|---|---|---|---|
| Frame | The host | `FrameLoop` (crate-private) | `Ui`, the three engines, the window token |
| Non-input work | `App::update` | `&Ui` | No change |
| Input | `App::input` | `InputCtx<'_>` | Nothing: borrows `Input` and the state rows from `Ui` |
| Record | `App::record` | `&mut Ui` | `Input`, `Forest`, `Memory`, theme, resources, `FrameRuntime`, `HostSeam`, overlays |

```rust
pub trait App {
    fn update(&mut self, window: WindowToken, ui: &Ui) {}              // no change
    fn input(&mut self, window: WindowToken, ctx: InputCtx<'_>) {}     // once a frame
    fn record(&mut self, window: WindowToken, ui: &mut Ui);            // no change
    fn settle_policy(&self) -> SettlePolicy { SettlePolicy::OnAction } // new
    fn is_busy(&self) -> bool { false }                                // no change
}

pub struct Input { pointer, keyboard, focus, /* ... */ arranged: Arranged }
pub struct InputCtx<'a> { pub input: &'a mut Input, state: &'a mut StateMap, display, now }
pub struct Ui { input: Input, forest: Forest, /* ... */ }

pub(crate) struct FrameLoop { ui: Ui, layout: LayoutEngine, cascade: CascadeEngine, /* ... */ }
```

An app with only `record`, or a closure, changes nothing. `Button::new().show(input)` does not
compile, as `show` takes `&mut Ui` and an `Input` holds no `Ui`.

## Root causes

1. **`Ui` has three roles on one type.** It is the authoring handle, the retained frame data and the
   host's port. `impl Ui` has 89 public and about 40 crate-private methods in a 1677-line file, and 16
   of them only a host calls (`on_input`, `set_window_facts`, `drain_window_output`, `submit_parts`,
   `frame`, ...). Any widget can call `ui.frame(..)`. `FrameEngines` sits outside `Ui` to keep it from
   widgets, but the host port does not.
2. **No type owns a frame.** `Ui::frame` passes `&mut self` and `&mut FrameEngines` into `FrameCycle`,
   a struct that lives for one call and only names the method. `WindowDriver` owns the two halves in
   another module. This is the split between `Ui` and the lifecycle.
3. **`InputState` cannot answer alone.** It reads the last arranged frame, so 27 signatures in
   `input_state` take a `&Cascade`, an `InputCtx` or an `ArrangedFrame`, and `Ui` adds them to each
   call. So an input stage needed a view over the whole `Ui`.
4. **No stage runs input.** The prepass runs inside `record`, so it replays in pass B and the warmup,
   and pass B runs for each action whoever handled it.

**The size of the struct is not a root cause.** `Ui` has 10 fields, and each is a cohesive subsystem.
egui's `ContextImpl` has about 25, as egui's `Context` also holds fonts, textures and input. The flat
public surface stays too: a facade of one-line calls is the established form, as `egui::Context` and
GPUI's `Window` show.

## The problem with pass B

- `FrameCycle::run` records again for each click, key press, drag latch and drag stop
  (`EventOutcome::settles`), wherever the app handled it. Each typed key in a Darkroom text field
  records the whole editor twice.
- Darkroom keeps guards for the replay: `GraphUi::appearing` allows for two passes with one frame
  id, and `Session::frame` asserts that a pass left no requests.
- `App::update` cannot do this work: it takes `&Ui`, and a key read takes `&mut` to keep its watch.
  It also sees focus before this frame's Tab moves it, and pass A sees another focus.

## Decisions

- **Two plain structures, and `Ui` owns `Input`.** `Ui` has the field `input: Input`. Refused: `Input<'a>` over `&mut Ui` with `Deref`, which needs a lifetime in
  each helper and 15 forwards. Refused: `UiCtx` with `Ui(UiCtx)` and `DerefMut`, as egui's `Ui` derefs
  to `Context`, which moves most public methods and gives the stage writes it does not need. Refused:
  `&mut Ui` with a panic, as GPUI's `debug_assert_paint` checks its phases, which compiles wrong code,
  and a `request_cursor` there is lost with no error.
- **`Input` owns `Arranged`, the `Layout` and the `Cascade`.** An input event means something only
  against the frame on screen, so this is the data `Input` reads. The 27 threaded parameters go away,
  and `ArrangedFrame` and most of `InputCtx` go with them. `Ui` falls to 8 fields. The engines write
  `ui.input.arranged`, and `FrameScene` and `DamageInput` borrow from it. Refused: a public
  `InputState`, which exposes `on_input` and `next_frame`. Cost: the encoder's data sits under a type
  named for input. `Arranged` keeps its own name and doc for this reason.
- **`Input` is one flat struct.** `InputState` became `Input`, with `arranged` as a field, so no read
  takes a cascade. Refused: `Input { state, arranged }`, which keeps the parameters between the two
  halves. The warmup takes the arranged frame into its scratch and brings it back.
- **`Input` has the canonical input methods, and `Ui` has one-line calls to them.** The `Ui` calls
  exist now, so a record's call sites do not change. Two reads differ by position:
  `ui.key_pressed(k)` reads at the record position, and `input.key_pressed(k)` reads as the window's
  root, on the main plane, so a modal's scope silences it. `ui.input()` gives a record the `&mut Input`
  that a shared helper takes.
- **The input phase has a view, `InputCtx`, and `Ui` is the record context.** The view holds disjoint
  parts of `Ui`: `Input`, the state rows, the display and the time. Masonry's contexts hold references
  to shared state in the same way. Refused: the state rows on `Input`, whose logic never reads them, a
  hack that a build of it showed; a `Ui<'a>` of references too, as Masonry has, which changes about
  1500 signatures for no gain to a caller; a shared core with `Deref`; a `DockDrag` the caller owns,
  which leaves the phase with no display or time.
- **A read that a scope routes is on the type that knows where it reads from.** `InputCtx` holds a
  reader: the window's root in `App::input`, the record position from `Ui::input_ctx`. `Input` keeps
  only what holds anywhere. Refused: a root read on `Input`, as `DockView::run` inside an overlay then
  lost its Esc, which a test showed.
- **A field belongs where its logic reads it.** So `Input` keeps `Arranged`, as 27 of its reads need
  it, and `Memory` keeps the state rows.
- **`App::input` is a second hook, and `update` stays.** A `&Ui` and an `InputCtx` cannot exist at one
  time, so one hook cannot hold both. Darkroom's `update` reads `window_geometry`, `clipboard` and
  `close_requested`, which are not input.
- **The input vocabulary is in `primitives::input`, and `crate::input` is the machine.** One top-level
  folder is input, and `Input` sits in a module of its name. Refused: a plain merge, a cycle through
  `scene`, which imports `Sense`, and `cascade`, which the machine imports; two folders under any
  names. Public paths do not change.
- **`FrameLoop` owns a frame.** It replaces `Ui::frame`, `FrameCycle`, `FrameEngines` and `FrameInput`.
  The 16 host methods move to it, so a widget cannot reach them, since it holds only `&mut Ui`. It is a
  child module of `ui`, so it reads `Ui`'s private fields. This is the shape of Masonry's `RenderRoot`,
  "the composition root", which a host gives events and asks to redraw, and of egui's one call
  `Context::run_ui`. Refused: moving the frame logic to `WindowDriver`, as `host` is at the far right of
  the layer order and needs nothing here beyond the port.
- **`FrameRuntime` stays on `Ui`.** A record reads its clock and writes its requests and wakes. Refused:
  moving the plan state alone to `FrameLoop`, a split that removes no defect.
- **`CpuCore`, `DeviceCore` and `HostCore` stay.** `AGENTS.md`, "Harness", holds that decision, and
  this review found no defect in it.
- **`SettlePolicy` is an `App` method.** It is a fact of how the app is written, as `is_busy` is, so no
  host, window or test can forget it. Refused: `Ui::set_settle_policy` beside `set_input_policy`, a
  per-window state that each harness must set.
- **Under `OnRequest`, pass A is the painted pass, and a write in the record calls
  `request_relayout`.** Refused: claiming an edge when `input` reads it, as Darkroom's geometry walk
  reads every port's response for its rect, so a claim would come with no intent. Refused: egui's
  extra frame, "Each request results in two repaints", which paints the stale frame first and costs
  the same record.
- **A runtime switch checks that contract.** Under `OnRequest`, on an action frame with no request,
  the check runs pass B anyway and compares the rollup hashes with pass A. A difference is an error
  that names the frame. `Harness` turns it on, and a window leaves it off. It is a switch and not a
  debug check, as a diagnostic must be.
- **On the first frame, `update` runs before the warmup, and the input stage after it.** An app sets up
  in `update`, and a record needs that, which a test showed when the warmup ran first. The input stage
  opens after the warmup, so a press or a Tab before frame 1 meets its frame. Each later frame opens
  input before `update`, so `update` reads pass A's focus.
- **`request_relayout` stays record-only.** An app that learns of the need in `input` keeps it for its
  record, as Darkroom does now with `Relayout`.
- **Refused: retained handlers** (`on_click` closures or messages, as in iced, GPUI and Flutter). Their
  order is the same: an event meets the last layout, the state changes, then the build runs. In
  palantir they allocate per frame and add a second way for each widget. An id and `response_for` give
  the same order, as egui's `read_response` does.

## What stays

- **Pass B for a sizing need.** A node's first record has no port rects, so its wires need a second
  pass, as egui's `Grid` takes through `request_discard`.
- **Record-time reads.** A menu row or an inspector field can keep its read in the record. An edit it
  raises asks for pass B, and a click that raises nothing costs one pass.

## Ownership

| Shared value | Holders | Verdict |
|---|---|---|
| `Rc<Theme>` | App, `ThemePair`, `ThemeTransition` | Kept: an immutable value too costly to copy, and the public API takes it |
| `UiResources`: `TextShaper`, `ImageRegistry`, `IconRegistry`, `Clipboard`, `WindowDirectory`, `Rc<AppSettings>`, `Rc<SharedDevice>` | Each window's `Ui`, `CpuCore`, the frontend | Kept: `CpuCore` could own them, but `&mut Ui` has no lifetime, and lending them needs `Ui<'res>` at each of the 356 widget signatures. Windows also open and close at run time |
| `Rc<SharedDevice>` in `Composer` | `Frontend` beside `UiResources` | Kept: a device attach already reaches both, but `Composer` reads the limit per frame, and a parameter saves one `Rc` for one more thread of a value |
| `Rc<WgpuImageStore>` | `ImageRegistry`, `BackendSpec` | Kept: built before the device, and it outlives each backend across a device loss |
| `Arc<WakeSignal>`, `Waker` | The host, any thread | Kept: a wake crosses threads |
| `StateMap` lend (`with_state`) | One row at a time | Kept: the body takes `&mut Ui` and the row together, which no borrow can express |

## Evidence

A scratch copy removed the action trigger of pass B and changed nothing else. The copy is gone.

| Suite | Tests | Failures | What fails |
|---|---|---|---|
| palantir lib | 2430 | 21 | 9 assert the pass itself. 12 are widgets: splitter 4, menu bar 2, popup 2, tabs 1, color button 1, drag value 1, canvas drag 1 |
| darkroom lib | 344 | 7 | Same-frame results: viewer ask, menu bar, palette 2, wire end, sample run, view menu |

- I read the messages of the 12 widget failures, and I did not find each root cause.
- The Darkroom run still had its prepass in `record`, and it asked for no pass B after a record edit.
- Other failures in that run came from the copy: 3 need the `.fits` files it left out, and 1 reads
  free disk space.
- The review found no other bug in this scope. Tests: the lib suite takes about 5 s for 2425 tests.

## Not known yet

- **Time.** A bench did not run. `.notes/PERF.md` records that "An action frame records twice". The
  saving is one record, layout and cascade on click and key frames, which are the worst frames. Hover,
  pan, zoom and a drag in motion already record once.
- **The true method lists.** I did not build this. The lists come from my reading, and the first build
  of each batch shows the true ones.

## Public API change

```text
+ struct Input, struct InputCtx<'a>
+ Ui::input_ctx
+ App::input(&mut self, window: WindowToken, ctx: InputCtx<'_>)
+ App::settle_policy(&self) -> SettlePolicy
+ enum SettlePolicy { OnAction, OnRequest }
~ DockView::scan(ctx: &mut InputCtx<'_>, ..)        (was ui: &mut Ui)
```

# Part 2: Plan

## Batch 4: Darkroom moves its prepass

Files: `darkroom/src/gui/app/mod.rs`, `session/mod.rs`, `main_window/mod.rs`, `pane/graph_ui/`.

- [ ] `session/mod.rs:112`: navigation and the prepass run in `record`. Move them to `App::input`, with
  helpers on `&mut Input`.
- [ ] `main_window/mod.rs:109`: `GraphCtx::new` reads `ui.display()`. Read `ctx.display()`.
- [ ] `pane/graph_ui/mod.rs:287`: delete the guards for a replayed prepass, as `appearing`'s two-pass
  allowance.
- [ ] `pane/graph_ui/`: `CanvasGeometry` reads rects. Rebuild it in `input`, and again in pass B, as that
  pass reads pass A's rects.

Pass counts do not change in this batch.

## Batch 5: pass B on request

Files: `src/ui/app.rs`, `src/ui/frame_loop.rs`, `src/widgets/` (splitter, menu bar, popup, tabs, color
button, drag value), `darkroom/src/gui/app/`. Needs batch 4 for Darkroom.

- [ ] `ui/app.rs`: add `SettlePolicy` and `App::settle_policy`.
- [ ] `ui/frame_loop.rs`: run pass B on an action only under `OnAction`, and add the runtime check.
- [ ] The 12 widget failures above: find each cause. A widget that changes what an earlier record read
  asks for pass B. Run the widget tests under both policies.
- [ ] `darkroom/src/gui/app/mod.rs:710`: take `OnRequest`, and ask for pass B after an edit or a
  preference write in the record.
- [ ] `darkroom/src/gui/app/session/bench.rs`: add click and typing cases, and compare with the base
  commit.
- [ ] `palantir/.notes/TODO.md:10`: delete the line.

Tests: `OnAction` and `OnRequest` on one click give 2 and 1 passes, and 2 with a request. The check
reports a label before a button that writes it, and is silent when the write asks.

## Sources

- [Masonry: `RenderRoot`](https://docs.rs/masonry_core/latest/masonry_core/app/struct.RenderRoot.html)
- [egui: `Context`, `ContextImpl`, two repaints for each request](https://raw.githubusercontent.com/emilk/egui/master/crates/egui/src/context.rs)
- [egui: `Context::run_ui`, `request_discard`, `read_response`](https://docs.rs/egui/latest/egui/struct.Context.html)
- [egui: `InputState`](https://docs.rs/egui/latest/egui/struct.InputState.html)
- [egui: `Ui` derefs to `Context`](https://docs.rs/egui/latest/egui/struct.Ui.html)
- [eframe: `App::logic` and `App::ui`](https://docs.rs/eframe/latest/src/eframe/epi.rs.html)
- [iced: `UserInterface`](https://docs.rs/iced_runtime/latest/iced_runtime/user_interface/struct.UserInterface.html)
- [GPUI: `Window`](https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/src/window.rs)
