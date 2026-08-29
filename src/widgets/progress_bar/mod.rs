//! The determinate progress bar: a rounded track with an accent fill
//! sized to a 0..1 fraction.

use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::scene::node::Node;
use crate::ui::Ui;
use crate::widgets::response::Response;
use crate::widgets::theme::progress_bar::ProgressBarTheme;

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
    node: Node,
    fraction: f32,
    style: Option<&'a ProgressBarTheme>,
}

impl<'a> ProgressBar<'a> {
    #[track_caller]
    pub fn new(fraction: f32) -> Self {
        Self {
            node: Node::hstack(),
            fraction,
            style: None,
        }
    }

    style_setter!('a, ProgressBarTheme, progress_bar);

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let theme = self.slot(ui.theme());
        let [fill, spacer] = Sizing::split(self.fraction);
        let height = theme.height.max(0.0);
        let radius = Corners::all(height * 0.5);

        let mut node = self.node;
        node.size
            .get_or_insert((Sizing::FILL, Sizing::fixed(height)).into());
        let track = Background::rounded(theme.track, radius);
        let fill_bg = Background::rounded(theme.fill, radius);

        let widget = ui.widget(node);
        let id = widget.id();
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

impl_configure!(ProgressBar<'_>);

#[cfg(test)]
mod tests;
