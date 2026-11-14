//! [`TabCarry`]: the tab a pointer drag carries, from the chip's drag start to its release.

use crate::primitives::input::keyboard::key::Key;
use crate::ui::Ui;

/// The tab a drag carries, by `K`, the key its owner finds it by: a strip's reorder and a dock's
/// move keep one each, so the lifecycle is written once. A carry ends at its release, at Escape,
/// or when its tab leaves the owner, as no chip is then left to report the release.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TabCarry<K>(Option<K>);

impl<K: Copy + PartialEq> TabCarry<K> {
    /// `carried`, kept from last frame.
    pub(crate) const fn new(carried: Option<K>) -> Self {
        Self(carried)
    }

    /// The tab carried.
    pub(crate) const fn carried(self) -> Option<K> {
        self.0
    }

    /// Carry `started`, a chip whose drag started, when nothing is carried; whether it began.
    pub(crate) const fn begin(&mut self, started: Option<K>) -> bool {
        if self.0.is_some() {
            return false;
        }
        self.0 = started;
        started.is_some()
    }

    /// End the carry at Escape.
    pub(crate) fn cancel(&mut self, ui: &mut Ui) {
        if self.0.is_some() && ui.shortcut_pressed(Key::Escape) {
            self.0 = None;
        }
    }

    /// End the carry when its tab is gone: `present` no longer finds it.
    pub(crate) fn end_if_gone(&mut self, present: impl FnOnce(K) -> bool) {
        if self.0.is_some_and(|tab| !present(tab)) {
            self.0 = None;
        }
    }

    /// The tab released, where `stopped` says its drag stopped, ending the carry.
    pub(crate) fn release(&mut self, stopped: impl FnOnce(K) -> bool) -> Option<K> {
        let tab = self.0.filter(|&tab| stopped(tab))?;
        self.0 = None;
        Some(tab)
    }
}

#[cfg(test)]
mod tests {
    use crate::widgets::tabs::tab_carry::TabCarry;

    /// A carry begins only from none, ends where its tab is gone, and releases only where its
    /// drag stopped, once.
    #[test]
    fn a_carry_begins_from_none_and_ends_once() {
        let mut carry = TabCarry::new(None);
        assert!(!carry.begin(None), "no drag started");
        assert!(carry.begin(Some(1)));
        assert!(!carry.begin(Some(2)), "one tab at a time");
        assert_eq!(carry.carried(), Some(1));

        assert_eq!(carry.release(|_| false), None, "its drag has not stopped");
        assert_eq!(carry.carried(), Some(1));
        carry.end_if_gone(|tab| tab == 1);
        assert_eq!(carry.carried(), Some(1), "its tab is still there");
        assert_eq!(carry.release(|tab| tab == 1), Some(1));
        assert_eq!(carry.carried(), None);
        assert_eq!(carry.release(|_| true), None, "released once");

        let mut gone = TabCarry::new(Some(3));
        gone.end_if_gone(|tab| tab != 3);
        assert_eq!(gone.carried(), None);
    }
}
