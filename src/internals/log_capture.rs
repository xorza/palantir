//! [`captured`]: the log records one test's thread writes while a closure runs. `log` routes every
//! record of the process through one logger set once, so the logger is a process-wide static, as
//! the outside world forces; it keeps only the records of a thread that is capturing, so tests on
//! other threads neither see each other's records nor pay more than an atomic load while no thread
//! captures. A record is formatted outside the lock, as its arguments may log in turn.

use crate::common::sync::MutexExt;
use log::{Level, LevelFilter, Log, Metadata, Record};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, Once};
use std::thread::{self, ThreadId};

/// One record a capturing thread wrote: its level and its formatted message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CapturedRecord {
    pub(crate) level: Level,
    pub(crate) message: String,
}

/// The capturing threads and what each has written so far, and how many there are, read before the
/// lock.
#[derive(Debug)]
struct LogCapture {
    threads: Mutex<Vec<(ThreadId, Vec<CapturedRecord>)>>,
    capturing: AtomicUsize,
}

static CAPTURE: LogCapture = LogCapture {
    threads: Mutex::new(Vec::new()),
    capturing: AtomicUsize::new(0),
};

impl Log for LogCapture {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &Record<'_>) {
        // Counted before the thread is named: a thread-local destructor that logs as the process
        // ends runs after the thread's own data is gone, which `thread::current` panics on.
        if self.capturing.load(Ordering::Acquire) == 0 {
            return;
        }
        let me = thread::current().id();
        if !self
            .threads
            .lock_unpoisoned()
            .iter()
            .any(|(id, _)| *id == me)
        {
            return;
        }
        let captured = CapturedRecord {
            level: record.level(),
            message: record.args().to_string(),
        };
        let mut threads = self.threads.lock_unpoisoned();
        let (_, records) = threads
            .iter_mut()
            .find(|(id, _)| *id == me)
            .expect("only its own capture ends a thread's capture");
        records.push(captured);
    }

    fn flush(&self) {}
}

/// Run `f`, and return what it returns with every record this thread logged meanwhile, at every
/// level, in order.
pub(crate) fn captured<R>(f: impl FnOnce() -> R) -> (R, Vec<CapturedRecord>) {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        log::set_logger(&CAPTURE).expect("the tests set no other logger");
        log::set_max_level(LevelFilter::Trace);
    });
    let me = thread::current().id();
    CAPTURE.threads.lock_unpoisoned().push((me, Vec::new()));
    CAPTURE.capturing.fetch_add(1, Ordering::Release);
    let out = f();
    CAPTURE.capturing.fetch_sub(1, Ordering::Release);
    let mut threads = CAPTURE.threads.lock_unpoisoned();
    let at = threads
        .iter()
        .position(|(id, _)| *id == me)
        .expect("this thread is capturing");
    (out, threads.swap_remove(at).1)
}

#[cfg(test)]
mod tests {
    use crate::internals::log_capture::{CapturedRecord, captured};
    use log::Level;
    use std::fmt;
    use std::thread;

    /// An argument that logs while it is formatted.
    struct LogsWhenShown;

    impl fmt::Display for LogsWhenShown {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            log::info!("inner");
            f.write_str("outer")
        }
    }

    /// A capture holds this thread's records in order, at their levels, and none of another
    /// thread's, which logs at the same time outside any capture; a record whose argument logs
    /// while it is formatted holds the inner record first.
    #[test]
    fn a_capture_holds_this_threads_records_alone() {
        let ((), records) = captured(|| {
            log::warn!("first {}", 1);
            thread::scope(|s| {
                s.spawn(|| log::error!("another thread"));
            });
            log::trace!("second");
        });
        assert_eq!(
            records,
            [
                CapturedRecord {
                    level: Level::Warn,
                    message: "first 1".to_owned(),
                },
                CapturedRecord {
                    level: Level::Trace,
                    message: "second".to_owned(),
                },
            ]
        );
        let ((), after) = captured(|| {});
        assert!(after.is_empty(), "a capture ends with its closure");

        let ((), nested) = captured(|| log::info!("{LogsWhenShown}"));
        let messages: Vec<&str> = nested.iter().map(|r| r.message.as_str()).collect();
        assert_eq!(
            messages,
            ["inner", "outer"],
            "an argument that logs does not deadlock"
        );
    }
}
