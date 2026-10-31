//! The indeterminate activity spinner, rotating on the paint clock.

use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::shape::paint_animation::PaintAnimation;
use crate::shape::paint_animation::PaintRepeat;
use crate::shape::paint_animation::curves;
use crate::shape::style::LineCap;
use crate::theme::spinner::SpinnerTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use glam::Vec2;

/// Indeterminate activity spinner: a rounded arc rotating with the frame
/// clock, its tail fading out. The recorded [`Shape::arc`] is identical every
/// frame (phase 0), so measure and cascade skip the subtree and the composer
/// shifts the angles; the spin's wake keeps the host repainting only while recorded.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Spinner<'a> {
    widget: Widget,
    diameter: Option<f32>,
    color: Option<RgbaF32>,
    width: Option<f32>,
    style: Option<&'a SpinnerTheme>,
}

impl<'a> Spinner<'a> {
    #[track_caller]
    /// A spinner.
    pub fn new() -> Self {
        Self {
            widget: Widget::leaf(),
            diameter: None,
            color: None,
            width: None,
            style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `spinner`; [`Self::color`],
    /// [`Self::diameter`] and [`Self::width`] still win.
    pub fn style(mut self, s: impl Into<Option<&'a SpinnerTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Diameter in logical px, defaulting to the theme's.
    ///
    /// # Panics
    ///
    /// Panics unless `px` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn diameter(mut self, px: f32) -> Self {
        self.diameter = Some(domain::length(px));
        self
    }

    /// Arc color (the comet's head), defaulting to the theme's.
    ///
    /// # Panics
    ///
    /// Panics unless `color` is a [color](crate::widget::domain::color).
    #[track_caller]
    pub const fn color(mut self, color: RgbaF32) -> Self {
        self.color = Some(domain::color(color));
        self
    }

    /// Stroke width in logical px, defaulting to a diameter-derived width.
    ///
    /// # Panics
    ///
    /// Panics unless `px` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn width(mut self, px: f32) -> Self {
        self.width = Some(domain::length(px));
        self
    }

    /// Records the spinner.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let theme = self.style.unwrap_or(&ui.theme().spinner);
        let diameter = domain::length_at_least(self.diameter.unwrap_or(theme.diameter), 1.0);
        let width = self
            .width
            .unwrap_or_else(|| (diameter * theme.width_ratio).max(theme.min_width));
        let color = self.color.unwrap_or(theme.color);
        let sweep = theme.sweep;
        let period = theme.period;
        self.widget
            .default_size((Sizing::fixed(diameter), Sizing::fixed(diameter)))
            .show(ui, None, |ui| {
                let ArcGeometry { center, radius } = ArcGeometry::new(diameter, width);
                let arc = Shape::arc(
                    CircularArc {
                        center,
                        radius,
                        start_angle: 0.0,
                        sweep,
                    },
                    Stroke::new(color, width),
                )
                .ramp(comet())
                .cap(LineCap::Round);
                // An arc of no length paints its cap as a dot, which a turn cannot rotate yet
                // (`.notes/TODO.md`), so it paints nothing, as before dots existed.
                if !arc.has_length() {
                    return;
                }
                ui.add_shape_animated(
                    arc,
                    PaintAnimation::turn(0.0, 1.0)
                        .with_period(period)
                        .with_repeat(PaintRepeat::Forever)
                        .with_curve(curves::linear),
                );
            })
            .response
    }
}

impl Configure for Spinner<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[derive(Debug, PartialEq)]
struct ArcGeometry {
    center: Vec2,
    radius: f32,
}

impl ArcGeometry {
    /// Insets the trace circle by half the stroke width so round caps stay inside.
    fn new(diameter: f32, width: f32) -> Self {
        Self {
            center: Vec2::splat(diameter * 0.5),
            radius: (diameter - width).max(0.0) * 0.5,
        }
    }
}

/// Comet-trail ramp, transparent at the tail and opaque at the head. White, so
/// it multiplies the stroke color; shared across themes.
fn comet() -> ColorRamp {
    ColorRamp::two_stop(RgbaF32::WHITE.with_alpha(0.0), RgbaF32::WHITE)
}

#[cfg(test)]
mod tests;
