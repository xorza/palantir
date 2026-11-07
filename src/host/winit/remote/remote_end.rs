//! [`RemoteEnd`]: the window side of the remote protocol, which a window of the winit host and a
//! test's loop both drive.

use crate::driver::error::PlayError;
use crate::host::winit::frame_summary::FrameSummary;
use crate::host::winit::remote::{QueryTask, ShotAsk};
use crate::ui::Ui;
use crate::ui::frame_report::FrameReport;
use crate::window::screenshot::{Screenshot, ScreenshotId};
use std::sync::mpsc::SyncSender;

/// What one window answers its remotes with: the screenshots they asked that have not landed,
/// and how many sent events it applied, which each frame's summary counts. Each screenshot
/// answers in the frame its image lands in, which reads it before a later frame replaces it; a
/// window that closes drops the asks, which its remotes read as [`PlayError::Disconnected`].
/// Refused: the shots in one type and the count and summary in each player of the window, where
/// a test's loop wrote its own copy of the window's side.
#[derive(Debug, Default)]
pub(crate) struct RemoteEnd {
    pending: Vec<PendingShot>,
    /// How many events a remote sent through the handle delivered, inputs and queries.
    sent_events: u64,
}

#[derive(Debug)]
struct PendingShot {
    id: ScreenshotId,
    reply: SyncSender<Result<Screenshot, PlayError>>,
}

impl RemoteEnd {
    /// Ask `ui` for the screenshot `ask` names; a region with no pixel of the window answers at
    /// once.
    pub(crate) fn ask_shot(&mut self, ui: &mut Ui, ask: ShotAsk) {
        match ui.request_screenshot(ask.region) {
            Some(id) => self.pending.push(PendingShot {
                id,
                reply: ask.reply,
            }),
            None => {
                ask.reply.send(Err(PlayError::EmptyRegion)).ok();
            }
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

    /// Note that a sent event, an input or a query, reached the window's `Ui`.
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
        self.pending
            .retain(|pending| match ui.screenshot_taken(pending.id) {
                Some(shot) => {
                    pending.reply.send(Ok(shot.clone())).ok();
                    false
                }
                None => true,
            });
        FrameSummary {
            record_frame_id: ui.record_frame_id(),
            sent_events: self.sent_events,
            repaint_requested: report.repaint_requested,
            busy,
        }
    }
}
