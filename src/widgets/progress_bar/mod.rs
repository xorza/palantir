//! The determinate progress bar: a rounded track with an accent fill sized to a 0..1 fraction.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::background::Background;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widgets::theme::progress_bar::ProgressBarTheme;

/// Determinate progress bar: a rounded `track` with an accent fill spanning `fraction` (clamped to `0..=1`) of its width.
///
/// A fraction naming no share (`0 / 0`) reads as empty; `Sizing::split` owns that, so app code may divide unguarded.
///
/// Fill and remainder are two weighted leaves, so the fill tracks the resolved track width. Visuals come from [`crate::ProgressBarTheme`] (slot `progress_bar`).
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct ProgressBar<'a> {
    widget: Widget,
    fraction: f32,
    style: Option<&'a ProgressBarTheme>,
}

impl<'a> ProgressBar<'a> {
    /// A bar filled to `fraction` of its width. Total over every `f32`: out of range reads as the nearer end, non-finite as empty. Resolved at `show` by [`Sizing::split`].
    #[track_caller]
    pub fn new(fraction: f32) -> Self {
        Self {
            widget: Widget::hstack(),
            fraction,
            style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `progress_bar`.
    pub fn style(mut self, s: impl Into<Option<&'a ProgressBarTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the bar. Senses nothing by default.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let theme = self.style.unwrap_or(&ui.theme().progress_bar);
        let [fill, spacer] = Sizing::split(self.fraction);
        let thickness = domain::length_at_least(theme.thickness, 0.0);
        let radius = Corners::all(thickness * 0.5);

        let mut widget = self
            .widget
            .default_size((Sizing::FILL, Sizing::fixed(thickness)));
        let track = Background::rounded(theme.track, radius);
        let fill_bg = Background::rounded(theme.fill, radius);

        let id = widget.resolve(ui);
        widget
            .show(ui, Some(&track), |ui| {
                Widget::leaf()
                    .id(id.with("fill"))
                    .size((fill, Sizing::FILL))
                    .record(ui, Some(&fill_bg), |_| {});
                // Remainder spacer: its `Fill` weight pushes the fill to the right fraction.
                Widget::leaf()
                    .id(id.with("rest"))
                    .size((spacer, Sizing::FILL))
                    .record(ui, None, |_| {});
            })
            .response
    }
}

impl Configure for ProgressBar<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
