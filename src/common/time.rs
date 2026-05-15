//! Shared timing constants used across animation, repaint
//! scheduling, and frame pacing.

use std::time::Duration;

/// Fixed substep used by the spring integrator and the `Ui::dt`
/// accumulator. Stability requires `dt·√k < ~1`; 1/240 s keeps the
/// product < 0.3 for `k ≤ 5000`. The `Ui` accumulator spends one
/// step per crossed threshold so each spent step is a single, stable
/// substep.
pub(crate) const ANIM_SUBSTEP_DT: f32 = 1.0 / 240.0;

/// Minimum gap between two scheduled repaint wakes. Wakes whose
/// deadline lands within this window of an existing entry collapse
/// onto the later wake — caps host wake-up rate at ~120 Hz under
/// bursts of `request_repaint_after`.
pub(crate) const REPAINT_COALESCE_DT: Duration = Duration::from_nanos(1_000_000_000 / 120);
