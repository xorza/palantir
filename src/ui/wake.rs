//! One entry on the `Ui` repaint-wake queue.

use crate::ui::wake_reasons::WakeReasons;
use std::time::Duration;

/// One entry on the `Ui` repaint-wake queue.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Wake {
    pub(crate) deadline: Duration,
    pub(crate) reasons: WakeReasons,
}
