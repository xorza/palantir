# Review: the agent driver, `Harness` and `Remote`

Scope: `src/driver`, `src/host/harness`, `src/host/winit/remote`, `frame_log.rs`, `frame_summary.rs`,
the handle's remote part, and how Darkroom's `src/driver` and the showcase use them. The tests of the
scope run in 0.55 s (harness, 38 tests), 0.00 s (driver, 6) and 0.79 s (remote, 4).

Public API changes are marked **API**, and Darkroom changes **Darkroom**.

# Part 1: proposal

The main shape is canonical, and it stays. Locators name what the window shows and refuse what a person
could not reach, as Playwright's locators and actionability checks do. A settle follows Flutter's
`pumpAndSettle`. A remote queries between two frames, as Flutter's driver extension and Slint's MCP
server do. One set of rules plays over a `Stage`, so both players agree, and a test holds them to it.

## Root causes

1. **`PlayError::NoFrame` holds three causes.** A held thread, a hidden window and a plain timeout read
   the same, and its text says "show it" for all of them. So Darkroom asks the probes again after the
   error, which can give another answer by then.
2. **A remote has one door to the window, and that door records.** `query` takes `&mut` the app, asks a
   frame and counts a sent event. `find`, `widgets`, `point` and the double-click read all go through it,
   so each read runs a whole frame, and a `find` on a hidden window makes the next settle fail. The
   harness runs no frame for the same reads. `AgentWindow` has no such verb at all, so a driver goes
   around the trait, and Darkroom writes `with_app` and `shot` twice.
3. **`FrameLog` does not model an open window.** "No summary" means both "no frame yet" and "closed", so
   a settle before the first frame reports that the window closed, and `is_open` reads false for an open
   window.
4. **Harness state and code sit where they do by history.** `impl Harness` is split across `play.rs`, a
   frame counter counts other frames than the remote's, and three settings are stored in three structs.

## Target design

- **Data.** `PlayError` has one case per cause. `FrameLog` holds an entry from a window's open to its
  close, so `Disconnected` means closed. `FrameSummary` loses `repaint_at`, which nothing reads.
  `Harness::frames` goes: both players count `record_frame_id`.
- **Interfaces.** `AgentWindow` gains `read` and `query`, so a driver writes a step once for both hosts,
  as the trait's doc already claims. `Locate` carries its own `Clone + Debug + Send + 'static` bound.
- **Module layout.** `harness/play.rs` goes. The verbs and `HarnessStage` move to `harness_session.rs`,
  and the rest of `impl Harness` to `mod.rs`, as one major struct holds its file.

## Ownership

Every shared-ownership case in the scope:

| Case | Verdict |
|---|---|
| `Arc<FrameLog>` on the host and each handle | Kept. A handle lives on any thread for as long as it likes, so no single owner exists. |
| `Mutex<Vec<LoggedFrame>>` and `Condvar` in `FrameLog` | Kept. The loop writes and any thread reads, and a few windows make a scan under one lock cheap. |
| `Arc<AtomicBool>` `given_up` in `Remote::query` | Kept. The task crosses to the window's thread, and a channel cannot tell a sender that its receiver left without a send, which must not run `f`. |
| `Box<dyn Fn() -> bool + Send>` `blocked` | Kept. The caller's probe is of a type palantir cannot name, and a generic parameter would reach `Remote` in every signature. |
| `Arc<WakeSignal>` in `HeadlessWindow` | Kept. A `Waker` crosses to a worker thread. |

## Looked at and kept

- **`AgentWindow` stays a trait with generic methods.** It has no `dyn` caller, and Darkroom's `Backend`
  enum stays, as the two backends differ in more than the verbs. The trait still makes a missing verb a
  compile error, and batch 4 gives it a real generic caller.
- **`Remote::sent` stays per remote.** A second remote can settle early, which its doc says. A sequence
  shared through the handle fixes a case no caller has, at the cost of a lock across each send.
- **`RemoteStage` and `HarnessStage` stay.** A direct `impl Stage for Remote` makes `settle` ambiguous
  beside `AgentWindow::settle` at each call.
- **`play` keeps `&[Action<X>]`, and `shot` and `widgets` keep their `Option`.** Each borrows, and `None`
  reads as "the whole window" and "no filter".
- **No `#[non_exhaustive]` on the errors.** Darkroom's exhaustive match on `PlayError` is what makes a new
  case get its hint.
- **`Locator::point_for` walks the widgets twice per step.** One agent step does so, and it costs no
  frame budget.
- **`Harness::state`, `rect` and `clipboard_text` stay.** They are sugar over `Ui`, and tests read better.

# Part 2: plan

## Batch 2: one error case per cause

Files: `src/driver/error.rs`, `src/host/winit/remote/mod.rs`, `src/host/harness/play.rs`; **Darkroom**
`darkroom/src/driver/error.rs`, `window_backend.rs`, `window_link.rs`.

- [ ] **API** `PlayError::NoFrame`: split into `Blocked` (the probe said so, no time waited), `Hidden` and
  `NoFrame { waited }`, the timeout. `check_waiting` knows which. Each `Display` says its own fix.
- [ ] **Darkroom** `WindowBackend::error` asks `link.blocked()` again after the error, which can read
  differently by then. Target: map `Blocked` to `NativeDialog` and `Hidden` to `WindowHidden` in
  `DriverError::played`. `WindowLink::blocked` goes if nothing else calls it.
- [ ] `remote/mod.rs` `wait_frame(settled: bool)`: a bool that selects behavior. Target: an enum beside
  `Answer`, or two methods over one loop.
- [ ] `remote/mod.rs`: `u32::try_from(frames).unwrap_or(u32::MAX)` appears three times. Target: one
  `frames_since(first)`.
- [ ] Bug, `remote/mod.rs` `self.wait * BUSY_SETTLE_WAITS` and `play.rs` `self.background_wait *
  BUSY_SETTLE_WAITS`: `Duration * u32` panics on overflow, so `with_wait(Duration::MAX)` panics inside a
  wait. Target: `saturating_mul`, and a test with `Duration::MAX`.
- [ ] Bug, `play.rs` `Harness::shot`: every wait for the device timing out ends with `Unsettled`, whose
  text blames an animation. Target: return `GpuTimedOut` when the last wait timed out.
- [ ] Test gap: `Blocked`, `Hidden` and the timeout each get the exact case, in
  `a_remote_answers_a_hidden_window_and_settles_a_busy_app`'s fixture.

## Batch 3: `FrameLog` models an open window

Files: `src/host/winit/frame_log.rs`, `frame_summary.rs`, `runtime.rs`, `handle.rs`, `remote/mod.rs`,
`remote/remote_end.rs`, `remote/tests.rs`.

- [ ] Bug, `Remote::last_frame`: `None` before the first frame reads as `Disconnected`, so a settle made
  early says the window closed, and Darkroom's `running` ends the session on it. Target: `FrameLog::open`
  makes the entry as the window opens. `last_frame` answers "open, no frame yet" apart from "not open",
  and a wait goes on through the first.
- [ ] **API** `HostHandle::is_open` reads false before the first frame. Target: true from the open to the
  close, and its doc follows.
- [ ] Dead data: nothing reads `FrameSummary::repaint_at` but a test of the log. Target: delete it, and
  tell the test's two summaries apart by `record_frame_id`.
- [ ] `wait_frame` reads the summary and then locks to wait, so a write between the two waits a whole tick,
  50 ms, while the doc says a waiter wakes on each change. Target: a `FrameLog::wait_past(seen, timeout)`
  that checks a write counter under the wait's own lock.

## Batch 4: a read door, on the trait

Files: `src/driver/agent_window.rs`, `src/driver/locator.rs`, `remote/mod.rs`, `remote/remote_end.rs`,
`handle.rs`, `window/mod.rs`, `harness_session.rs`, `remote/tests.rs`; **Darkroom** `driver/backend.rs`,
`offscreen.rs`, `window_backend.rs`, `shot.rs`.

- [ ] **API** `Remote::read(f: FnOnce(&T, &Ui) -> R)`: runs between two frames, asks no frame and counts no
  event. Target: `find`, `widgets`, `RemoteStage::point` and `space_press` use it. A find on a hidden
  window then leaves the next settle alone, and `record_frame_id` does not move.
- [ ] **API** `AgentWindow`: add `read` and `query`. `HarnessSession::read` calls `f` directly, and
  `query` also asks a repaint, as the remote's does.
- [ ] **Darkroom** `WindowBackend::shot` and `Offscreen::shot` are the same steps. Target: one generic
  `Capture::take(window: &mut impl AgentWindow<App>, region, max_side)`, the trait's first generic caller.
- [ ] **API** `X: Locate<A> + Clone + Send + 'static` repeats on each verb. Target: `Locate<A>: Clone +
  Debug + Send + 'static`, and the verbs bound `X: Locate<A>`.
- [ ] Test gap: a read asks no frame, as `record_frame_id` before and after shows, and a settle after a read
  on a hidden window returns `Ok`.

## Batch 5: locators reuse what they reason about

Files: `src/driver/locator.rs`, `src/cascade/mod.rs`, `src/driver/tests.rs`.

- [ ] Bug, `Locator::find` over `Locate::find_all`: an impl that returns `Ok` with nothing panics in a debug
  build, and in a release build reads `Ambiguous` with zero matches. Target: `NotFound` with no nearest in
  every build, and a test.
- [ ] `locator.rs` `topmost` writes `Cascade::hits_under` again over a copy of its rows, so "what the input
  reaches" has two walks. Target: a `Cascade::hit_row(pos, filter)` under `hit_test` and the locator.
  `reached` and `point_for` read it, and `Harness::point_in` already reads the same walk.

## Batch 6: the harness's shape

Files: `src/host/harness/*`.

- [ ] `harness/play.rs` holds `impl Harness` and the session's work, and the session's verbs only forward
  to `pub(super)` methods. Target: the verbs and `HarnessStage` in `harness_session.rs`, `wait_for_wake`,
  `has_pending_wake`, `capture` and the settle in `mod.rs`, `play.rs` deleted.
- [ ] **API** `settle` and `play` return different counts: the harness counts every frame, and a remote only
  those that record. `AgentWindow` says "the frames run" for both. Target: `record_frame_id` on both, the
  field `Harness::frames` deleted, and the doc says recorded frames. Update `Ok(11)` in the settle test
  if a paint-only frame moved it.
- [ ] `frame_dt`, `settle_frames` and `background_wait` live on `HarnessBuilder`, `Seal` and `Harness`.
  Target: one `SettlePace` struct on all, `Seal` reduced to it and the paint, or gone.
- [ ] `internals` `step_passes` keeps a `warmup` flag. Target: `ui.record_pass() != RecordPass::Warmup`, as
  `frame_value` reads it now.
- [ ] `mod.rs` writes `self.window.frames.ui()` 20 times beside `Self::ui`. Target: `self.ui()`.
- [ ] **API** `Harness` has no `double_click` while `Action::DoubleClick` exists, and tests write two clicks
  by hand. Target: `Harness::double_click(at)`.

## Batch 7: names and the modifier rule

Files: `src/driver/action.rs`, `src/driver/error.rs`, `src/driver/play.rs`, `harness/play.rs` or its
successor, and callers; **Darkroom** `app/src/mcp/params.rs`, `darkroom/src/driver`, the showcase's script.

- [ ] **API** `Action::Move` is `Harness::hover`, and the variant's own doc says "as a hover". Target:
  `Action::Hover`.
- [ ] **API** `ActError { ran, error }` reads as a sibling of `PlayError`, though it wraps one with an index,
  and `play` returns the one named after another verb. Target: `StepError { step, error }`.
- [ ] `play.rs` sends `ModifiersChanged(NONE)` around each step that holds none, so the remote sends events
  the harness's `Stage::input` must filter, and a person's held key is cleared on a live window. Target:
  `play` sends a change only for a step with modifiers, and `HarnessStage::input` loses its special case.
  A test that holds modifiers across a step must set them on the step.
