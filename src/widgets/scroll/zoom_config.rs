//! Pivot-anchored zoom configuration for a `Scroll::both`.
//!
//! [`ZoomModifier`] and [`ZoomPivot`] are [`ZoomConfig`]'s own axes —
//! neither means anything without it — so all three share a file.

use crate::primitives::math::domain;
use std::ops::RangeInclusive;

/// What kind of input triggers a zoom step. See [`ZoomConfig::with_modifier`].
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
/// [`Scroll::zoomable`](crate::Scroll::zoomable) / [`Scroll::zoomable_with`](crate::Scroll::zoomable_with).
#[derive(Clone, Debug)]
#[must_use]
pub struct ZoomConfig {
    pub(super) range: RangeInclusive<f32>,
    pub(super) step: f32,
    pub(super) modifier: ZoomModifier,
    pub(super) pivot: ZoomPivot,
}

impl ZoomConfig {
    /// Configure the inclusive zoom range and multiplicative wheel factor.
    /// Both range ends and `step` are *positive*; a reversed range is
    /// ordered.
    ///
    /// # Panics
    ///
    /// Panics unless both range ends and `step` are
    /// [positive](crate::widget::domain::positive).
    #[track_caller]
    pub const fn new(range: RangeInclusive<f32>, step: f32) -> Self {
        let a = domain::positive(*range.start());
        let b = domain::positive(*range.end());
        Self {
            range: a.min(b)..=a.max(b),
            step: domain::positive(step),
            modifier: ZoomModifier::Ctrl,
            pivot: ZoomPivot::Pointer,
        }
    }

    /// Wheel-vs-pinch routing. Default [`ZoomModifier::Ctrl`].
    pub const fn with_modifier(mut self, modifier: ZoomModifier) -> Self {
        self.modifier = modifier;
        self
    }

    /// Where the zoom step pivots. Default [`ZoomPivot::Pointer`].
    pub const fn with_pivot(mut self, pivot: ZoomPivot) -> Self {
        self.pivot = pivot;
        self
    }
}

impl Default for ZoomConfig {
    fn default() -> Self {
        Self::new(0.1..=10.0, 1.03)
    }
}
