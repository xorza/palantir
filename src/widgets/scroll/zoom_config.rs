//! Pivot-anchored zoom configuration for a `Scroll::both`, with its two axes [`ZoomModifier`] and [`ZoomPivot`].

use crate::primitives::math::domain;
use std::ops::RangeInclusive;

/// What kind of input triggers a zoom step. See [`ZoomConfig::with_modifier`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ZoomModifier {
    /// `Ctrl` + wheel (default); bare wheel pans. Cmd on macOS is not honored, matching the shortcut layer.
    Ctrl,
    /// Plain wheel always zooms (image viewers without pan).
    Always,
    /// Wheel pans; only pinch zooms (touch-first).
    PinchOnly,
}

/// Where the zoom step pivots: the point that stays fixed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ZoomPivot {
    /// Pointer position in widget-local coords (default).
    Pointer,
    /// Viewport center.
    Center,
}

/// Per-widget zoom configuration for a `Scroll::both`; see [`Scroll::zoomable`](crate::Scroll::zoomable) and [`Scroll::zoom_config`](crate::Scroll::zoom_config).
#[derive(Clone, Debug)]
#[must_use]
pub struct ZoomConfig {
    pub(super) range: RangeInclusive<f32>,
    pub(super) step: f32,
    pub(super) modifier: ZoomModifier,
    pub(super) pivot: ZoomPivot,
}

impl ZoomConfig {
    /// The inclusive zoom range and multiplicative wheel factor; a reversed range is ordered.
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
