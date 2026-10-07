//! The indeterminate activity spinner, rotating on the paint clock.

use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::tree::paint_anims::curves;
use crate::scene::tree::paint_anims::paint_animation::PaintAnimation;
use crate::scene::tree::paint_anims::paint_animation::PaintRepeat;
use crate::shape::Shape;
use crate::shape::style::LineCap;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widgets::theme::spinner::SpinnerTheme;
use glam::Vec2;
use std::f32::consts::TAU;
use std::time::Duration;

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
    thickness: Option<f32>,
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
            thickness: None,
            style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `spinner`; [`Self::color`],
    /// [`Self::diameter`] and [`Self::thickness`] still win.
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

    /// Arc colour (the comet's head), defaulting to the theme's.
    ///
    /// # Panics
    ///
    /// Panics unless `c` is a [colour](crate::widget::domain::color).
    #[track_caller]
    pub const fn color(mut self, c: RgbaF32) -> Self {
        self.color = Some(domain::color(c));
        self
    }

    /// Stroke width in logical px, defaulting to a diameter-derived width.
    ///
    /// # Panics
    ///
    /// Panics unless `px` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn thickness(mut self, px: f32) -> Self {
        self.thickness = Some(domain::length(px));
        self
    }

    /// Records the spinner.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let theme = self.style.unwrap_or(&ui.theme().spinner);
        let diameter = domain::length_at_least(self.diameter.unwrap_or(theme.diameter), 1.0);
        let width = self
            .thickness
            .unwrap_or((diameter * theme.thickness_ratio).max(theme.min_thickness));
        let color = self.color.unwrap_or(theme.color);
        let sweep = theme.sweep;
        let speed = theme.speed;
        self.widget
            .default_size((Sizing::fixed(diameter), Sizing::fixed(diameter)))
            .show(ui, None, |ui| {
                let ArcGeometry { center, radius } = arc_geometry(diameter, width);
                ui.add_shape_animated(
                    Shape::arc(center, radius, 0.0, sweep, Stroke::new(color, width))
                        .ramp(comet())
                        .cap(LineCap::Round),
                    PaintAnimation::turn(0.0, 1.0)
                        .with_period(Duration::from_secs_f32(TAU / speed))
                        .with_repeat(PaintRepeat::Forever)
                        .with_curve(curves::linear),
                );
            })
            .response
    }
}

impl Configure for Spinner<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[derive(Debug, PartialEq)]
struct ArcGeometry {
    center: Vec2,
    radius: f32,
}

/// Insets the trace circle by half the stroke width so round caps stay inside.
fn arc_geometry(diameter: f32, width: f32) -> ArcGeometry {
    ArcGeometry {
        center: Vec2::splat(diameter * 0.5),
        radius: (diameter - width).max(0.0) * 0.5,
    }
}

/// Comet-trail ramp, transparent at the tail and opaque at the head. White, so
/// it multiplies the stroke colour; shared across themes.
fn comet() -> ColorRamp {
    ColorRamp::two_stop(RgbaF32::WHITE.with_alpha(0.0), RgbaF32::WHITE)
}

#[cfg(test)]
mod tests;
