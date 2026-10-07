//! The thin divider rule, on either axis.

use crate::primitives::layout::align::{Align, HAlign, VAlign};
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widgets::theme::separator::SeparatorTheme;

/// A thin divider rule: `thickness` tall across the parent's width, or wide as a column. Sized `Hug` plus
/// Stretch on its long axis so it fills the cross extent without leaking an infinite size to a `Hug`
/// ancestor; an explicit [`Configure::size`] replaces that default and ignores `thickness`, an explicit
/// [`Configure::align`] replaces it on its axis only. Visuals: [`crate::SeparatorTheme`].
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Separator<'a> {
    widget: Widget,
    axis: Axis,
    thickness: Option<f32>,
    color: Option<RgbaF32>,
    style: Option<&'a SeparatorTheme>,
}

impl<'a> Separator<'a> {
    #[track_caller]
    /// A horizontal rule.
    pub fn horizontal() -> Self {
        Self::along(Axis::X)
    }

    #[track_caller]
    /// A vertical rule.
    pub fn vertical() -> Self {
        Self::along(Axis::Y)
    }

    #[track_caller]
    fn along(axis: Axis) -> Self {
        Self {
            widget: Widget::leaf(),
            axis,
            thickness: None,
            color: None,
            style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `separator`; [`Self::color`] and [`Self::thickness`] still win.
    pub fn style(mut self, s: impl Into<Option<&'a SeparatorTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Line thickness in logical px, defaulting to the theme's.
    ///
    /// # Panics
    ///
    /// Panics unless `px` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn thickness(mut self, px: f32) -> Self {
        self.thickness = Some(domain::length(px));
        self
    }

    /// Line color, defaulting to the theme's.
    ///
    /// # Panics
    ///
    /// Panics unless `c` is a [colour](crate::widget::domain::color).
    #[track_caller]
    pub const fn color(mut self, c: RgbaF32) -> Self {
        self.color = Some(domain::color(c));
        self
    }

    /// Records the rule.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let theme = self.style.unwrap_or(&ui.theme().separator);
        let t = domain::length_at_least(self.thickness.unwrap_or(theme.thickness), 0.0);
        let (default_size, stretch) = match self.axis {
            Axis::X => ((Sizing::HUG, Sizing::fixed(t)), Align::h(HAlign::Stretch)),
            Axis::Y => ((Sizing::fixed(t), Sizing::HUG), Align::v(VAlign::Stretch)),
        };
        // The stretch belongs to the `Hug` default; over an explicit size it would override the caller's extent.
        let widget = match self.widget.authored_size() {
            Some(_) => self.widget,
            None => self.widget.size(default_size).default_align(stretch),
        };
        let chrome = Background::fill(self.color.unwrap_or(theme.color));
        // Theme margin fills in only where the caller stayed silent.
        let widget = widget.default_margin(theme.margin);
        widget.show(ui, Some(&chrome), |_| {}).response
    }
}

impl Configure for Separator<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
