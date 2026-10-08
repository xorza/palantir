//! The min-and-median summary drivers print beside criterion's estimate.

use std::fmt;
use std::time::Duration;

/// The fastest and the middle of a run of per-frame samples. The minimum is the
/// keep-or-revert signal: the upper half measures interference from the rest of
/// the machine.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Summary {
    pub(crate) min: Duration,
    pub(crate) median: Duration,
}

impl Summary {
    /// Sorts `samples` in place; `None` when there are none.
    pub(crate) fn of(samples: &mut [Duration]) -> Option<Self> {
        samples.sort_unstable();
        Some(Self {
            min: *samples.first()?,
            median: samples[samples.len() / 2],
        })
    }
}

/// `min=… median=…`, each to three decimals of the unit that fits it.
impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "min={:.3?} median={:.3?}", self.min, self.median)
    }
}
