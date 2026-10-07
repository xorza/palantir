//! One app-global setting, and the signal that says it moved.

use std::cell::Cell;

/// An app-global value any window may write, with a flag for whether it changed since the host last asked: windows that didn't record are asleep, so the write raises the signal and [`Self::take_change`] lowers it. Shared through an `Rc` in [`UiResources`](crate::ui::resources::UiResources).
#[derive(Debug, Default)]
pub(crate) struct AppSetting<T: Copy + PartialEq> {
    value: Cell<T>,
    changed: Cell<bool>,
}

impl<T: Copy + PartialEq> AppSetting<T> {
    #[inline]
    pub(crate) const fn get(&self) -> T {
        self.value.get()
    }

    /// Writing the held value is not a change, so a per-frame assign doesn't repaint every window.
    #[inline]
    pub(crate) fn set(&self, value: T) {
        if self.value.replace(value) != value {
            self.changed.set(true);
        }
    }

    #[cfg(any(test, feature = "winit"))]
    #[inline]
    pub(crate) const fn take_change(&self) -> bool {
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
