//! The determinate progress bar: a rounded track with an accent fill
//! sized to a 0..1 fraction.

use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::num::F32Ext;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ConfigureWidget;
use crate::widgets::response::Response;
use crate::widgets::theme::progress_bar::ProgressBarTheme;
use crate::widgets::widget::Widget;

/// Determinate progress bar: a rounded `track` with an accent fill
/// spanning `fraction` (clamped to `0..=1`) of its width.
///
/// A `fraction` that names no share — the `0 / 0` of a job with nothing
/// to do — reads as empty. `Sizing::split` owns that screen, so app code
/// may divide without guarding the divisor first.
///
/// The fill / remainder split is two weighted leaves, so the fill tracks the
/// resolved track width without the widget knowing it at record time.
/// Visuals come from [`crate::ProgressBarTheme`] (theme slot
/// `progress_bar`).
#[derive(Debug)]
pub struct ProgressBar<'a> {
    widget: Widget,
    fraction: f32,
    style: Option<&'a ProgressBarTheme>,
}

impl<'a> ProgressBar<'a> {
    #[track_caller]
    pub fn new(fraction: f32) -> Self {
        Self {
            widget: Widget::hstack(),
            fraction,
            style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `progress_bar`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    pub fn style(mut self, s: impl Into<Option<&'a ProgressBarTheme>>) -> Self {
        self.style = s.into();
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let theme = self.style.unwrap_or(&ui.theme().progress_bar);
        let [fill, spacer] = Sizing::split(self.fraction);
        let thickness = theme.thickness.themed_length(0.0);
        let radius = Corners::all(thickness * 0.5);

        let mut widget = self
            .widget
            .default_size((Sizing::FILL, Sizing::fixed(thickness)));
        let track = Background::rounded(theme.track, radius);
        let fill_bg = Background::rounded(theme.fill, radius);

        let id = widget.resolve(ui);
        widget
            .show(ui, Some(&track), |ui| {
                ui.chrome_leaf(id.with("fill"), (fill, Sizing::FILL), Some(&fill_bg));
                // Remainder spacer — its `Fill` weight pushes the fill to the
                // correct fraction of the track width.
                ui.chrome_leaf(id.with("rest"), (spacer, Sizing::FILL), None);
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
