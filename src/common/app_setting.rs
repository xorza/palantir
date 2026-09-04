//! One app-global setting, and the signal that says it moved.

use std::cell::Cell;

/// A value every window's frames read and any one window's recorder may
/// write, plus whether it changed since the host last asked.
///
/// **The signal is what makes app-global work.** A write in one window has
/// to reach the others, and nothing else tells the host that: the windows
/// that did not record are asleep, and the one that did has no way to wake
/// them. So the write raises the signal, [`Self::take_change`] lowers it,
/// and the host holds no copy of its own to keep in step — nor asks
/// anything of a setting on a loop tick that moved none.
///
/// Held behind an `Rc` by [`UiResources`](crate::ui::resources::UiResources)
/// so every recorder shares the one cell.
#[derive(Debug, Default)]
pub(crate) struct AppSetting<T: Copy + PartialEq> {
    value: Cell<T>,
    changed: Cell<bool>,
}

impl<T: Copy + PartialEq> AppSetting<T> {
    #[inline]
    pub(crate) fn get(&self) -> T {
        self.value.get()
    }

    /// Writing the value already held is not a change: an app that
    /// assigns the same one every frame must not repaint every window
    /// every frame.
    #[inline]
    pub(crate) fn set(&self, value: T) {
        if self.value.replace(value) != value {
            self.changed.set(true);
        }
    }

    /// Whether the value changed since this was last asked, clearing the
    /// signal. Gated with the windowed runtime, its only caller: it is the
    /// one that has other windows to repaint.
    #[cfg(any(test, feature = "winit"))]
    #[inline]
    pub(crate) fn take_change(&self) -> bool {
        self.changed.replace(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_move_signals_once_and_a_repeat_signals_not_at_all() {
        let setting = AppSetting::<u32>::default();
        assert_eq!(setting.get(), 0);
        assert!(!setting.take_change(), "nothing has written yet");

        setting.set(2);
        assert_eq!(setting.get(), 2);
        assert!(setting.take_change());
        assert!(!setting.take_change(), "the ask lowers the signal");

        setting.set(2);
        assert!(
            !setting.take_change(),
            "re-asserting the held value is not a change",
        );
    }
}
