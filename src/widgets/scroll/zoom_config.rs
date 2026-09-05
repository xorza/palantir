//! Pivot-anchored zoom configuration for a `Scroll::both`.
//!
//! [`ZoomModifier`] and [`ZoomPivot`] are [`ZoomConfig`]'s own axes —
//! neither means anything without it — so all three share a file.

use crate::input::zoom_factor::ZoomFactor;
use std::ops::RangeInclusive;

/// What kind of input triggers a zoom step. See [`ZoomConfig::modifier`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ZoomModifier {
    /// Hold `Ctrl` and turn the wheel. Default. Bare wheel pans as
    /// today. Ctrl is the zoom modifier on every platform (macOS Cmd
    /// is not honored — matches the shortcut layer).
    Ctrl,
    /// Plain wheel always zooms (rare; for image viewers without pan).
    Always,
    /// Wheel always pans; only pinch gestures zoom. Touch-first apps.
    PinchOnly,
}

/// Where the zoom step pivots — the point that stays fixed across the
/// scale change.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ZoomPivot {
    /// Pointer position (in widget-local coords). Default — the point
    /// under the cursor stays put across the zoom step.
    Pointer,
    /// Viewport center.
    Center,
}

/// Per-widget zoom configuration. Attach to a `Scroll::both` via
/// [`Scroll::with_zoom`](crate::Scroll::with_zoom) / [`Scroll::with_zoom_config`](crate::Scroll::with_zoom_config).
#[derive(Clone, Debug)]
pub struct ZoomConfig {
    pub(super) range: RangeInclusive<f32>,
    pub(super) step: f32,
    /// Wheel-vs-pinch routing. Default [`ZoomModifier::Ctrl`].
    pub modifier: ZoomModifier,
    /// Where the zoom step pivots. Default [`ZoomPivot::Pointer`].
    pub pivot: ZoomPivot,
}

const ZOOM_RANGE_ERROR: &str = "zoom range must satisfy 0 < min <= max with finite bounds";
const ZOOM_STEP_ERROR: &str = "zoom step must be finite and positive";

impl ZoomConfig {
    /// Configure the inclusive zoom range and multiplicative wheel factor.
    ///
    /// # Panics
    ///
    /// Panics unless both range bounds are finite, `0 < min <= max`, and
    /// `step` is finite and positive.
    #[track_caller]
    pub fn new(range: RangeInclusive<f32>, step: f32) -> Self {
        let min = *range.start();
        let max = *range.end();
        assert!(
            ZoomFactor::new(min).is_some() && ZoomFactor::new(max).is_some() && min <= max,
            "{ZOOM_RANGE_ERROR}"
        );
        assert!(ZoomFactor::new(step).is_some(), "{ZOOM_STEP_ERROR}");
        Self {
            range,
            step,
            modifier: ZoomModifier::Ctrl,
            pivot: ZoomPivot::Pointer,
        }
    }
}

impl Default for ZoomConfig {
    fn default() -> Self {
        Self::new(0.1..=10.0, 1.03)
    }
}
