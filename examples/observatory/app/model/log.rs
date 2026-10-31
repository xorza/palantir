//! [`Log`]: what happened in the session, newest last.

use std::collections::VecDeque;

/// The lines the log keeps.
pub(crate) const CAPACITY: usize = 200;

/// One event, stamped with the step it happened at.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LogLine {
    /// The count of events before this one, not a clock, so a run reads the same each time.
    pub(crate) step: u64,
    pub(crate) text: String,
}

/// The last [`CAPACITY`] events.
#[derive(Debug, Default)]
pub(crate) struct Log {
    lines: VecDeque<LogLine>,
    next_step: u64,
}

impl Log {
    /// Record `text` as the next event, dropping the oldest past [`CAPACITY`].
    pub(crate) fn push(&mut self, text: String) {
        if self.lines.len() == CAPACITY {
            self.lines.pop_front();
        }
        self.lines.push_back(LogLine {
            step: self.next_step,
            text,
        });
        self.next_step += 1;
    }

    pub(crate) fn lines(&self) -> impl DoubleEndedIterator<Item = &LogLine> + ExactSizeIterator {
        self.lines.iter()
    }
}

#[cfg(test)]
mod tests {
    use crate::app::model::log::{CAPACITY, Log};

    /// Steps count every event; past the capacity the oldest goes, so the first kept line is the
    /// one pushed `CAPACITY` events before the last.
    #[test]
    fn the_log_keeps_the_last_lines_with_their_steps() {
        let mut log = Log::default();
        for idx in 0..CAPACITY + 5 {
            log.push(format!("event {idx}"));
        }
        assert_eq!(log.lines().len(), CAPACITY);
        let first = log.lines().next().unwrap();
        assert_eq!((first.step, first.text.as_str()), (5, "event 5"));
        let last = log.lines().next_back().unwrap();
        assert_eq!(last.step, (CAPACITY + 4) as u64);
    }
}
