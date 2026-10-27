//! Pivot-anchored zoom configuration for a `Scroll::both`, with its two axes [`ZoomModifier`] and [`ZoomPivot`].

use crate::primitives::math::domain;
use std::ops::RangeInclusive;

/// What kind of input triggers a zoom step. See [`ZoomConfig::with_modifier`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ZoomModifier {
    /// The primary command modifier + wheel (default): Cmd on macOS, Ctrl elsewhere, as
    /// [`Modifiers::ctrl`](crate::Modifiers::ctrl) reads it. A bare wheel pans.
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
#[derive(Copy, Clone, Debug, PartialEq)]
#[must_use]
pub struct ZoomConfig {
    min: f32,
    max: f32,
    step: f32,
    modifier: ZoomModifier,
    pivot: ZoomPivot,
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
            min: a.min(b),
            max: a.max(b),
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

    /// The smallest zoom, positive.
    pub const fn min(self) -> f32 {
        self.min
    }

    /// The largest zoom, at least [`Self::min`].
    pub const fn max(self) -> f32 {
        self.max
    }

    /// The factor one wheel notch multiplies the zoom by, positive.
    pub const fn step(self) -> f32 {
        self.step
    }

    /// What input triggers a zoom step; see [`Self::with_modifier`].
    pub const fn modifier(self) -> ZoomModifier {
        self.modifier
    }

    /// Where a zoom step pivots; see [`Self::with_pivot`].
    pub const fn pivot(self) -> ZoomPivot {
        self.pivot
    }

    /// `zoom` held inside the range.
    pub(super) const fn clamp(self, zoom: f32) -> f32 {
        zoom.clamp(self.min, self.max)
    }
}

impl Default for ZoomConfig {
    fn default() -> Self {
        Self::new(0.1..=10.0, 1.03)
    }
}
