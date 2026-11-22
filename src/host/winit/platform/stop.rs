//! [`Stop`]: what a watch's owner raises on drop and the watch's thread races its work against.

use std::future;
use std::pin::pin;
use std::sync::Mutex;
use std::task::{Poll, Waker};

use crate::common::sync::MutexExt as _;

/// A stop an owner raises once, which ends the work a thread runs [`until`](Self::until) it.
#[derive(Debug, Default)]
pub(super) struct Stop(Mutex<State>);

#[derive(Debug, Default)]
struct State {
    raised: bool,
    /// The task waiting in [`Stop::until`], woken by the raise.
    waker: Option<Waker>,
}

impl Stop {
    /// Raise the stop, waking the task that waits on it.
    pub(super) fn raise(&self) {
        let waker = {
            let mut state = self.0.lock_unpoisoned();
            state.raised = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// `work`'s output, or `None` once the stop is raised, which drops `work` unfinished; a stop
    /// raised before the first poll never polls it.
    pub(super) async fn until<T>(&self, work: impl Future<Output = T>) -> Option<T> {
        let mut work = pin!(work);
        future::poll_fn(|cx| {
            {
                let mut state = self.0.lock_unpoisoned();
                if state.raised {
                    return Poll::Ready(None);
                }
                // Filed before `work` is polled, so a raise between the two still wakes this task.
                state.waker = Some(cx.waker().clone());
            }
            work.as_mut().poll(cx).map(Some)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::future;
    use std::sync::{Arc, mpsc};
    use std::task::Poll;
    use std::thread;

    use crate::common::executor;
    use crate::host::winit::platform::stop::Stop;

    /// Work that finishes gives its output; a stop raised first never polls the work; a stop
    /// raised from another thread while the work waits wakes the waiting task, which drops the
    /// work, so what the work holds is gone once `until` returns.
    #[test]
    fn a_raised_stop_ends_and_drops_the_work() {
        assert_eq!(
            executor::block_on(Stop::default().until(async { 7 })),
            Some(7)
        );

        let early = Stop::default();
        early.raise();
        let mut polled = false;
        let work = future::poll_fn(|_| {
            polled = true;
            Poll::<()>::Pending
        });
        assert_eq!(executor::block_on(early.until(work)), None);
        assert!(!polled, "a stop raised first polls nothing");

        let stop = Arc::new(Stop::default());
        let held = Arc::new(());
        let (waiting, waited) = mpsc::channel();
        let raiser = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                waited.recv().unwrap();
                stop.raise();
            })
        };
        let work = {
            let held = Arc::clone(&held);
            async move {
                waiting.send(()).unwrap();
                future::pending::<()>().await;
                drop(held);
            }
        };
        assert_eq!(executor::block_on(stop.until(work)), None);
        raiser.join().unwrap();
        assert_eq!(Arc::strong_count(&held), 1, "the work is dropped");
    }
}
