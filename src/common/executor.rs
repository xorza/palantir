//! [`block_on`]: one future run to its end on the calling thread, which parks between polls and
//! wakes on the future's waker, as std's [`Wake`] documents a minimal executor and as `pollster`
//! runs it. For wgpu's adapter and device requests and a portal thread's watch, each a thread with
//! nothing else to run meanwhile.

use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

/// Wakes the thread that blocks on a future.
struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Poll `future` on this thread until it is ready, parked between polls; a wake that lands before
/// the park leaves its token, so the park returns at once and nothing is missed.
pub(crate) fn block_on<F: IntoFuture>(future: F) -> F::Output {
    let mut future = pin!(future.into_future());
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
        thread::park();
    }
}

#[cfg(test)]
mod tests {
    use crate::common::executor;
    use std::future;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::{Poll, Waker};
    use std::thread;

    /// A ready future gives its output; one that waits gives it once another thread sets its flag
    /// and wakes it, having been polled exactly twice: before the wake and after it.
    #[test]
    fn a_future_runs_to_its_output_and_a_wake_from_another_thread_resumes_it() {
        assert_eq!(executor::block_on(async { 7 }), 7);

        let done = Arc::new(AtomicBool::new(false));
        let mut polls = 0;
        let mut waker: Option<Waker> = None;
        let mut waking = None;
        let out = executor::block_on(future::poll_fn(|cx| {
            polls += 1;
            if done.load(Ordering::Acquire) {
                return Poll::Ready("woken");
            }
            if waker.is_none() {
                let wake = cx.waker().clone();
                waker = Some(wake.clone());
                let done = Arc::clone(&done);
                waking = Some(thread::spawn(move || {
                    done.store(true, Ordering::Release);
                    wake.wake();
                }));
            }
            Poll::Pending
        }));
        waking
            .expect("the first poll spawned the waker")
            .join()
            .unwrap();
        assert_eq!(out, "woken");
        assert!(
            polls >= 2,
            "polled before and after the wake, a spurious unpark aside"
        );
    }
}
