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

1. **Harness state and code sit where they do by history.** `impl Harness` is split across `play.rs`, a
   frame counter counts other frames than the remote's, and three settings are stored in three structs.

## Target design

- **Data.** `Harness::frames` goes: both players count `record_frame_id`.
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
  compile error, and Darkroom's `Capture::take` is its generic caller.
- **`Remote::sent` stays per remote.** A second remote can settle early, which its doc says. A sequence
  shared through the handle fixes a case no caller has, at the cost of a lock across each send.
- **`RemoteStage` and `HarnessStage` stay.** A direct `impl Stage for Remote` makes `settle` ambiguous
  beside `AgentWindow::settle` at each call.
- **`play` keeps `&[Action<X>]`, and `shot` and `widgets` keep their `Option`.** Each borrows, and `None`
  reads as "the whole window" and "no filter".
- **No `#[non_exhaustive]` on the errors.** Darkroom's exhaustive match on `PlayError` is what makes a new
  case get its hint.
- **`Harness::state`, `rect` and `clipboard_text` stay.** They are sugar over `Ui`, and tests read better.

# Part 2: plan

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
