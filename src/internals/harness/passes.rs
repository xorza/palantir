//! The values one frame's record passes returned.

use crate::ui::frame_report::FrameReport;

/// The values one frame's record closure returned, one per record pass
/// in the order the passes ran, warmup excluded.
///
/// Pass A is the input-observing pass: it sees the one-frame edges
/// (`clicked`, `drag.started()`). Pass B, when there is one, runs after
/// the edges were drained. Reading the right pass is the whole reason
/// this type exists — a value captured into an outer variable keeps
/// whichever pass ran last, which is pass B on exactly the frames an
/// action made interesting.
#[derive(Debug)]
pub struct Passes<R> {
    values: Vec<R>,
    report: FrameReport,
}

impl<R> Passes<R> {
    pub(crate) const fn new(values: Vec<R>, report: FrameReport) -> Self {
        Self { values, report }
    }

    /// What the frame reported to its host.
    pub fn report(&self) -> &FrameReport {
        &self.report
    }

    /// Pass A's value.
    ///
    /// # Panics
    ///
    /// Panics when the frame ran no record pass
    /// (`FrameProcessing::PaintOnly`).
    pub fn a(&self) -> &R {
        self.values.first().expect(NO_PASS)
    }

    /// Pass B's value, `None` on a single-pass frame.
    pub fn b(&self) -> Option<&R> {
        self.values.get(1)
    }

    /// How many record passes ran, warmup excluded: 0, 1 or 2.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the frame ran no record pass at all.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// How many passes returned a value matching `pred`, so a signal
    /// both passes reported reads as the double fire it is.
    pub fn count_where(&self, pred: impl Fn(&R) -> bool) -> usize {
        self.values.iter().filter(|value| pred(value)).count()
    }

    /// Pass A's value, by value; `None` on a frame with no record pass.
    pub fn into_a(self) -> Option<R> {
        self.values.into_iter().next()
    }
}

const NO_PASS: &str = "the frame ran no record pass — FrameProcessing::PaintOnly. A \
                       paint-anim wake was the frame's only cause (a focused TextEdit's \
                       caret blink is enough). Feed an input, request a repaint, or check \
                       `Passes::is_empty`.";
