//! [`Handoff`]: a value a watch's thread opens and its owner's drop closes, whichever comes first.

use std::mem;
use std::sync::Mutex;

use crate::common::sync::MutexExt as _;

/// Where a value a thread opens for its owner stands: the thread files it once open, and the
/// owner's drop closes it, taking it to end what reads it. A close before the file refuses the
/// file, so the thread ends what it opened itself and nothing outlives the owner.
#[derive(Debug)]
pub(super) struct Handoff<T>(Mutex<State<T>>);

#[derive(Debug)]
enum State<T> {
    Opening,
    Open(T),
    Closed,
}

impl<T> Handoff<T> {
    /// A handoff whose value the thread has not opened yet.
    pub(super) const fn new() -> Self {
        Self(Mutex::new(State::Opening))
    }

    /// File `value` as open; `Err` hands it back when the owner closed first, for the thread to
    /// end.
    pub(super) fn file(&self, value: T) -> Result<(), T> {
        let mut state = self.0.lock_unpoisoned();
        if matches!(*state, State::Closed) {
            return Err(value);
        }
        debug_assert!(
            matches!(*state, State::Opening),
            "a thread files its value once"
        );
        *state = State::Open(value);
        Ok(())
    }

    /// Close the handoff: the value filed, for the owner to end, or `None` where none was; a later
    /// file is refused.
    pub(super) fn close(&self) -> Option<T> {
        match mem::replace(&mut *self.0.lock_unpoisoned(), State::Closed) {
            State::Open(value) => Some(value),
            State::Opening | State::Closed => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::host::winit::platform::handoff::Handoff;

    /// A value filed before the close goes to the owner; one filed after goes back to the thread.
    #[test]
    fn the_close_takes_a_filed_value_and_refuses_a_late_one() {
        let filed = Handoff::new();
        assert_eq!(filed.file(7), Ok(()));
        assert_eq!(filed.close(), Some(7));
        assert_eq!(filed.close(), None, "closed once");

        let late = Handoff::new();
        assert_eq!(late.close(), None, "nothing filed yet");
        assert_eq!(late.file(7), Err(7));
    }
}
