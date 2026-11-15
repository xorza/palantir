//! [`RemoteEnd`]: the window side of the remote protocol, which a window of the winit host and a
//! test's loop both drive.

use crate::driver::error::PlayError;
use crate::gpu::device::gpu::Gpu;
use crate::host::winit::frame_summary::FrameSummary;
use crate::host::winit::user_event::{Asked, LoseTask, QueryTask, ShotAsk};
use crate::ui::Ui;
use crate::ui::frame_report::FrameReport;
use crate::window::screenshot::{Screenshot, ScreenshotId};

/// What one window answers its remotes with: the screenshots they asked that have not landed,
/// one at most for each remote that still waits, as one a remote gave up on goes at the next
/// frame, and how many sent events it applied, which each frame's summary counts. Each screenshot
/// answers in the frame its image lands in, which reads it before a later frame replaces it; a
/// window that closes drops the asks, which its remotes read as [`PlayError::Disconnected`].
/// Refused: the shots in one type and the count and summary in each player of the window, where
/// a test's loop wrote its own copy of the window's side.
#[derive(Debug, Default)]
pub(crate) struct RemoteEnd {
    pending: Vec<PendingShot>,
    /// How many events a remote sent through the handle delivered: inputs, queries, device losses.
    sent_events: u64,
}

#[derive(Debug)]
struct PendingShot {
    id: ScreenshotId,
    asked: Asked<Result<Screenshot, PlayError>>,
}

impl RemoteEnd {
    /// Ask `ui` for the screenshot `ask` names; a refused ask answers at once, and one its remote
    /// gave up on asks nothing.
    pub(crate) fn ask_shot(&mut self, ui: &mut Ui, ask: ShotAsk) {
        let ShotAsk { region, asked } = ask;
        if asked.is_given_up() {
            return;
        }
        match ui.request_screenshot(region) {
            Ok(id) => self.pending.push(PendingShot { id, asked }),
            Err(refusal) => asked.answer(|| Err(PlayError::from(refusal))),
        }
    }

    /// Run a query's `task` with `&mut` `app` and `ui`, between two frames, and ask for a frame
    /// that records, as the task may have changed the app; the query counts as a sent event, so a
    /// remote's settle after it waits for that frame.
    pub(crate) fn run_query<T>(&mut self, app: &mut T, ui: &mut Ui, task: QueryTask<T>) {
        task(app, ui);
        ui.request_repaint();
        self.note_sent();
    }

    /// Run `task` with `gpu`, the device the window paints with, which loses it.
    pub(crate) fn run_lose_device(&mut self, gpu: &Gpu, task: LoseTask) {
        task(gpu);
        self.note_sent();
    }

    /// Note that a sent event, an input, a query or a device loss, reached the window's `Ui`.
    pub(crate) const fn note_sent(&mut self) {
        self.sent_events += 1;
    }

    /// Close the frame `ui` just ran, which reported `report`, the app `busy` or not: hand each
    /// screenshot that landed to the remote that asked, and sum the frame up for its remotes.
    pub(crate) fn after_frame(
        &mut self,
        ui: &Ui,
        report: &FrameReport,
        busy: bool,
    ) -> FrameSummary {
        self.pending.retain(|pending| {
            if pending.asked.is_given_up() {
                return false;
            }
            let Some(shot) = ui.screenshot_taken(pending.id) else {
                return true;
            };
            pending.asked.reply.send(Ok(shot.clone())).ok();
            false
        });
        FrameSummary {
            record_frame_idx: ui.record_frame_idx(),
            sent_events: self.sent_events,
            repaint_requested: report.repaint_requested,
            busy,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::host::harness::Harness;
    use crate::host::winit::remote::remote_end::RemoteEnd;
    use crate::host::winit::user_event::{Asked, ShotAsk};
    use glam::UVec2;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;

    /// A shot its remote gave up on asks nothing, and one given up on after it was asked leaves
    /// the window's list at the next frame, so a remote that retries leaves one ask at most.
    #[test]
    fn a_shot_given_up_on_leaves_the_pending_list() {
        let mut h = Harness::mono(UVec2::new(40, 30));
        let mut end = RemoteEnd::default();
        let (reply, answer) = mpsc::sync_channel(1);
        let ask = |given_up: &Arc<AtomicBool>| ShotAsk {
            region: None,
            asked: Asked {
                reply: reply.clone(),
                given_up: Arc::clone(given_up),
            },
        };

        let abandoned = Arc::new(AtomicBool::new(true));
        end.ask_shot(h.ui_mut(), ask(&abandoned));
        assert_eq!(end.pending.len(), 0, "an ask given up on asks nothing");

        let waiting = Arc::new(AtomicBool::new(false));
        end.ask_shot(h.ui_mut(), ask(&waiting));
        let report = h.frame(|_| {});
        let _ = end.after_frame(h.ui(), &report, false);
        assert_eq!(end.pending.len(), 1, "a record-only frame lands no image");

        waiting.store(true, Ordering::Release);
        let _ = end.after_frame(h.ui(), &report, false);
        assert_eq!(end.pending.len(), 0, "given up on after its ask");
        assert!(
            answer.try_recv().is_err(),
            "no answer went to a remote that left"
        );
    }
}
