//! A background job: a worker thread that counts through its steps and wakes the window after
//! each, so a frame shows its progress when there is progress to show, and none is spent polling.

use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use palantir::Waker;

/// A worker counting to [`Job::STEPS`], one step a [`Job::STEP`].
#[derive(Debug)]
pub(crate) struct Job {
    /// Steps the worker finished; it writes, the frame reads.
    done: Arc<AtomicU16>,
    worker: Option<JoinHandle<()>>,
}

impl Job {
    pub(crate) const STEPS: u16 = 40;
    const STEP: Duration = Duration::from_millis(50);

    /// Start the worker; `waker` asks the window for a frame after each step.
    pub(crate) fn start(waker: Waker) -> Self {
        Self::stepping(waker, Self::STEP)
    }

    fn stepping(waker: Waker, step: Duration) -> Self {
        let done = Arc::new(AtomicU16::new(0));
        let counted = Arc::clone(&done);
        let worker = thread::spawn(move || {
            for finished in 1..=Self::STEPS {
                thread::sleep(step);
                counted.store(finished, Ordering::Release);
                waker.wake();
            }
        });
        Self {
            done,
            worker: Some(worker),
        }
    }

    /// The steps the worker finished.
    pub(crate) fn done(&self) -> u16 {
        self.done.load(Ordering::Acquire)
    }

    /// Whether the worker's thread still runs; one that stopped short reads as stopped, so a
    /// settle never waits on it.
    pub(crate) fn is_running(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
    }

    /// Join the worker once its thread ended, so a panic in it surfaces here.
    pub(crate) fn reap(&mut self) {
        if !self.is_running()
            && let Some(worker) = self.worker.take()
        {
            worker.join().expect("the worker counts and never panics");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use palantir::Harness;
    use palantir::glam::UVec2;

    use crate::job::Job;

    /// The worker counts every step, and the job reads done once the last step lands.
    #[test]
    fn the_worker_counts_every_step() {
        let mut h = Harness::builder(UVec2::new(16, 16)).mono().build();
        let mut job = Job::stepping(h.ui().waker(), Duration::ZERO);
        let worker = job.worker.take().expect("a started job holds its worker");
        worker.join().expect("the worker counts and never panics");
        assert_eq!(job.done(), Job::STEPS);
        assert!(!job.is_running());
    }
}
