//! The indeterminate activity spinner, rotating on the paint clock.

use crate::animation::motion_role::MotionRole;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::line_style::LineCap;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::shape::paint_animation::PaintAnimation;
use crate::shape::paint_animation::PaintRepeat;
use crate::theme::spinner::SpinnerTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureDefaults;
use crate::widget_core::widget::Widget;
use glam::Vec2;

/// Indeterminate activity spinner: a rounded arc rotating with the frame
/// clock, its tail fading out. The recorded [`Shape::arc`] is identical every
/// frame (phase 0), so measure and cascade skip the subtree and the composer
/// shifts the angles; the spin's wake keeps the host repainting only while recorded.
///
/// Sized as any leaf: a [fixed](Sizing::fixed) [`size`](Configure::size) on both axes draws the
/// arc across its shorter side, centered; otherwise the arc takes the theme's diameter, and so
/// does the node unless the caller sizes it.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Spinner<'a> {
    widget: Widget,
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
            color: None,
            width: None,
            style: None,
        }
    }

    /// Per-instance override of [`Theme::spinner`](crate::Theme::spinner); [`Self::color`], [`Self::stroke_width`]
    /// and a fixed size still win.
    pub fn style(mut self, s: impl Into<Option<&'a SpinnerTheme>>) -> Self {
        self.style = s.into();
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
    pub const fn stroke_width(mut self, px: f32) -> Self {
        self.width = Some(domain::length(px));
        self
    }

    /// Records the spinner.
    pub fn show(self, ui: &mut Ui) -> Response {
        let theme = self.style.unwrap_or(&ui.theme().spinner);
        let fixed = self
            .widget
            .authored()
            .size()
            .and_then(|size| Some(Vec2::new(size.w().fixed_value()?, size.h().fixed_value()?)));
        let diameter =
            domain::length_at_least(fixed.map_or(theme.diameter, Vec2::min_element), 1.0);
        let extent = fixed.unwrap_or_else(|| Vec2::splat(diameter));
        let width = self
            .width
            .unwrap_or_else(|| (diameter * theme.width_ratio).max(theme.min_stroke_width));
        let color = self.color.unwrap_or(theme.color);
        let sweep = theme.sweep;
        let period = theme.period;
        self.widget
            .default_size((Sizing::fixed(diameter), Sizing::fixed(diameter)))
            .show(ui, None, |ui| {
                let SpinnerCircle { center, radius } = SpinnerCircle::new(extent, diameter, width);
                // An arc of no length, of no sweep or no radius, paints its cap as a dot in the
                // comet's color at its start, the transparent tail: it would show nothing and
                // turn forever, so it is not drawn.
                if is_invisible(radius) || is_invisible(sweep.abs()) {
                    return;
                }
                let circle = Rect::new(
                    center.x - radius,
                    center.y - radius,
                    2.0 * radius,
                    2.0 * radius,
                );
                if let Some(shape) = ui.add_shape(
                    Shape::arc(0.0, sweep).at(circle),
                    Stroke::new(color, width)
                        .with_ramp(comet())
                        .with_cap(LineCap::Round),
                ) {
                    shape.animate(
                        PaintAnimation::turn(0.0, 1.0)
                            .with_period(period)
                            .with_repeat(PaintRepeat::Forever)
                            .with_motion(MotionRole::Essential),
                    );
                }
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
struct SpinnerCircle {
    center: Vec2,
    radius: f32,
}

impl SpinnerCircle {
    /// A circle of `diameter` centered in `extent`, its trace inset by half the stroke width so
    /// round caps stay inside.
    fn new(extent: Vec2, diameter: f32, width: f32) -> Self {
        Self {
            center: extent * 0.5,
            radius: (diameter - width).max(0.0) * 0.5,
        }
    }
}

/// Comet-trail ramp, transparent at the tail and opaque at the head. White, so
/// it multiplies the stroke color; shared across themes.
fn comet() -> ColorRamp {
    ColorRamp::from([RgbaF32::WHITE.with_alpha(0.0), RgbaF32::WHITE])
}

#[cfg(test)]
mod tests;
