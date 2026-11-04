//! The thin divider rule, on either axis.

use crate::primitives::layout::align::{Align, HAlign, VAlign};
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::math::domain;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::separator::SeparatorTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;

/// A thin divider rule: the theme's `thickness` tall across the parent's width, or wide as a
/// column. Its long axis is `Hug` plus Stretch, so it fills the cross extent without leaking an
/// infinite size to a `Hug` ancestor. An explicit [`Configure::size`] replaces the default
/// size, and the stretch stays while its long axis is `Hug`, so
/// `.size((Sizing::HUG, Sizing::fixed(3.0)))` is a 3 px horizontal rule across the parent. An
/// explicit [`Configure::align`] replaces the stretch on its axis only. Visuals:
/// [`crate::SeparatorTheme`].
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Separator<'a> {
    widget: Widget,
    axis: Axis,
    color: Option<RgbaF32>,
    style: Option<&'a SeparatorTheme>,
}

impl<'a> Separator<'a> {
    /// A horizontal rule.
    #[track_caller]
    pub fn horizontal() -> Self {
        Self::along(Axis::X)
    }

    /// A vertical rule.
    #[track_caller]
    pub fn vertical() -> Self {
        Self::along(Axis::Y)
    }

    /// A rule whose long axis is `axis`: [`Axis::X`] is [`Self::horizontal`].
    #[track_caller]
    pub fn along(axis: Axis) -> Self {
        Self {
            widget: Widget::leaf(),
            axis,
            color: None,
            style: None,
        }
    }

    /// Per-instance override of [`Theme::separator`](crate::Theme::separator); [`Self::color`] and
    /// [`Configure::size`] still win.
    pub fn style(mut self, s: impl Into<Option<&'a SeparatorTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Line color, defaulting to the theme's.
    ///
    /// # Panics
    ///
    /// Panics unless `color` is a [color](crate::widget::domain::color).
    #[track_caller]
    pub const fn color(mut self, color: RgbaF32) -> Self {
        self.color = Some(domain::color(color));
        self
    }

    /// Records the rule.
    pub fn show(self, ui: &mut Ui) -> Response {
        let theme = self.style.unwrap_or(&ui.theme().separator);
        let t = domain::length_at_least(theme.thickness, 0.0);
        let (default_size, stretch) = match self.axis {
            Axis::X => (
                SizeSpec::new(Sizing::HUG, Sizing::fixed(t)),
                Align::h(HAlign::Stretch),
            ),
            Axis::Y => (
                SizeSpec::new(Sizing::fixed(t), Sizing::HUG),
                Align::v(VAlign::Stretch),
            ),
        };
        let size = self.widget.authored_size().unwrap_or(default_size);
        let long = match self.axis {
            Axis::X => size.w(),
            Axis::Y => size.h(),
        };
        // The stretch serves a `Hug` long axis alone; over a sized one it would override the
        // caller's extent.
        let mut widget = self.widget.size(size);
        if long.is_hug() {
            widget = widget.default_align(stretch);
        }
        let chrome = Chrome::fill(self.color.unwrap_or(theme.color));
        // Theme margin fills in only where the caller stayed silent.
        let widget = widget.default_margin(theme.margin);
        widget.show(ui, Some(&chrome), |_| {}).response
    }
}

impl Configure for Separator<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
