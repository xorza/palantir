//! [`RemoteEnd`]: the window side of the remote protocol, which a window of the winit host and a
//! test's loop both drive.

use crate::driver::error::PlayError;
use crate::host::winit::frame_summary::FrameSummary;
use crate::host::winit::remote::ShotAsk;
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
    /// How many events a remote sent through the handle delivered.
    sent_inputs: u64,
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

    /// Note that a sent event reached the window's `Ui`.
    pub(crate) const fn note_input(&mut self) {
        self.sent_inputs += 1;
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
            sent_inputs: self.sent_inputs,
            repaint_requested: report.repaint_requested,
            repaint_at: report.repaint_at,
            busy,
        }
    }
}
