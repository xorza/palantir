//! Locks that panic on poison: a poisoned lock means a panic left its data half-changed, so the
//! thread that takes it panics too, as the Darkroom workspace's `common::MutexExt` does.
#![expect(
    clippy::disallowed_methods,
    reason = "the one place a std lock is taken and its poison checked"
)]

#[cfg(test)]
use std::sync::TryLockError;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

pub(crate) const POISONED: &str =
    "a holder of this lock panicked: the panic above left its data half-changed";

/// [`Mutex`]'s calls, each panicking on a poisoned lock.
pub(crate) trait MutexExt<T: ?Sized> {
    /// [`Mutex::lock`].
    ///
    /// # Panics
    ///
    /// When a panic poisoned the lock.
    fn lock_unpoisoned(&self) -> MutexGuard<'_, T>;

    /// [`Mutex::try_lock`]; `None` while another holds it.
    ///
    /// # Panics
    ///
    /// When a panic poisoned the lock.
    #[cfg(test)]
    fn try_lock_unpoisoned(&self) -> Option<MutexGuard<'_, T>>;
}

impl<T: ?Sized> MutexExt<T> for Mutex<T> {
    fn lock_unpoisoned(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|_| panic!("{POISONED}"))
    }

    #[cfg(test)]
    fn try_lock_unpoisoned(&self) -> Option<MutexGuard<'_, T>> {
        match self.try_lock() {
            Ok(guard) => Some(guard),
            Err(TryLockError::WouldBlock) => None,
            Err(TryLockError::Poisoned(_)) => panic!("{POISONED}"),
        }
    }
}

/// [`Condvar`]'s wait, panicking on the poisoned lock it takes back.
pub(crate) trait CondvarExt {
    /// [`Condvar::wait_timeout_while`], which the guard's value tells met or timed out.
    ///
    /// # Panics
    ///
    /// When a panic poisoned the lock.
    fn wait_timeout_while_unpoisoned<'a, T>(
        &self,
        guard: MutexGuard<'a, T>,
        timeout: Duration,
        condition: impl FnMut(&mut T) -> bool,
    ) -> MutexGuard<'a, T>;
}

impl CondvarExt for Condvar {
    fn wait_timeout_while_unpoisoned<'a, T>(
        &self,
        guard: MutexGuard<'a, T>,
        timeout: Duration,
        condition: impl FnMut(&mut T) -> bool,
    ) -> MutexGuard<'a, T> {
        let (guard, _) = self
            .wait_timeout_while(guard, timeout, condition)
            .unwrap_or_else(|_| panic!("{POISONED}"));
        guard
    }
}

#[cfg(test)]
mod tests {
    use crate::common::sync::{CondvarExt, MutexExt, POISONED};
    use std::panic::{self, AssertUnwindSafe};
    use std::sync::{Condvar, Mutex};
    use std::time::Duration;

    /// The message `call` panics with, which must be one.
    fn panic_message(call: impl FnOnce()) -> String {
        let payload = panic::catch_unwind(AssertUnwindSafe(call)).expect_err("the call panics");
        payload
            .downcast_ref::<String>()
            .cloned()
            .expect("a formatted message")
    }

    /// A healthy lock gives its value, a held one `None` to `try_lock`, and a timed wait with no
    /// wake gives back the guard unchanged; once a panic poisons the lock, every call panics with
    /// [`POISONED`].
    #[test]
    fn a_poisoned_lock_panics_on_every_call() {
        let m = Mutex::new(3);
        let wake = Condvar::new();
        assert_eq!(*m.lock_unpoisoned(), 3);
        {
            let _held = m.lock_unpoisoned();
            assert!(m.try_lock_unpoisoned().is_none());
        }
        let guard =
            wake.wait_timeout_while_unpoisoned(m.lock_unpoisoned(), Duration::ZERO, |_| true);
        assert_eq!(*guard, 3);
        drop(guard);

        let unwound = panic::catch_unwind(AssertUnwindSafe(|| {
            let _held = m.lock_unpoisoned();
            panic!("poison the lock");
        }));
        assert!(unwound.is_err());
        assert_eq!(panic_message(|| drop(m.lock_unpoisoned())), POISONED);
        assert_eq!(panic_message(|| drop(m.try_lock_unpoisoned())), POISONED);
    }
}
