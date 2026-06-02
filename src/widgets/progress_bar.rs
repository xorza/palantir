use crate::forest::element::{Configure, Element, LayoutMode, Salt};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::theme::progress_bar::ProgressBarTheme;

/// Determinate progress bar: a rounded `track` with an accent fill
/// spanning `fraction` (clamped to `0..=1`) of its width.
///
/// The fill / remainder split is two `Fill`-weighted leaves
/// (`Fill(fraction)` and `Fill(1 − fraction)`), so the fill tracks the
/// resolved track width without the widget knowing it at record time.
/// Visuals come from [`crate::ProgressBarTheme`] (theme slot
/// `progress_bar`).
pub struct ProgressBar {
    element: Element,
    fraction: f32,
    style: Option<ProgressBarTheme>,
}

impl ProgressBar {
    #[track_caller]
    pub fn new(fraction: f32) -> Self {
        Self {
            element: Element::new(LayoutMode::HStack),
            fraction,
            style: None,
        }
    }

    /// Override the theme for this bar. `None` (default) inherits
    /// [`crate::Theme::progress_bar`].
    pub fn style(mut self, s: ProgressBarTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let theme = self
            .style
            .clone()
            .unwrap_or_else(|| ui.theme.progress_bar.clone());
        let WeightSplit { fill, spacer } = fill_weights(self.fraction);
        let height = theme.height.max(0.0);
        let radius = Corners::all(height * 0.5);

        let mut element = self.element;
        element.size = (Sizing::FILL, Sizing::Fixed(height)).into();
        let track = Background {
            corners: radius,
            ..Background::fill(theme.track)
        };
        let fill_bg = Background {
            corners: radius,
            ..Background::fill(theme.fill)
        };

        let id = ui.make_persistent_id(element.salt);
        ui.node(id, element, Some(&track), |ui| {
            let fill_id = id.with("fill");
            let mut fill_el = Element::new(LayoutMode::Leaf);
            fill_el.salt = Salt::Verbatim(fill_id);
            fill_el.size = (Sizing::Fill(fill), Sizing::FILL).into();
            ui.node(fill_id, fill_el, Some(&fill_bg), |_| {});

            // Remainder spacer — its `Fill` weight pushes the fill to the
            // correct fraction of the track width.
            let rest_id = id.with("rest");
            let mut rest = Element::new(LayoutMode::Leaf);
            rest.salt = Salt::Verbatim(rest_id);
            rest.size = (Sizing::Fill(spacer), Sizing::FILL).into();
            ui.node(rest_id, rest, None, |_| {});
        });
        Response::lazy(id, ui)
    }
}

impl Configure for ProgressBar {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

struct WeightSplit {
    fill: f32,
    spacer: f32,
}

/// Clamp `fraction` to `0..=1` and split it into the fill / remainder
/// `Fill` weights. `0 → (0, 1)`, `1 → (1, 0)`, out-of-range clamps.
fn fill_weights(fraction: f32) -> WeightSplit {
    let f = fraction.clamp(0.0, 1.0);
    WeightSplit {
        fill: f,
        spacer: 1.0 - f,
    }
}

#[cfg(test)]
mod tests {
    use super::fill_weights;

    #[test]
    fn fill_weights_clamp_and_split() {
        let cases = [
            (0.0, 0.0, 1.0),
            (0.25, 0.25, 0.75),
            (0.5, 0.5, 0.5),
            (1.0, 1.0, 0.0),
            (-0.3, 0.0, 1.0), // below range clamps to empty
            (1.7, 1.0, 0.0),  // above range clamps to full
        ];
        for (input, want_fill, want_spacer) in cases {
            let w = fill_weights(input);
            assert!(
                (w.fill - want_fill).abs() < 1e-6 && (w.spacer - want_spacer).abs() < 1e-6,
                "fraction {input}: got ({}, {}), want ({want_fill}, {want_spacer})",
                w.fill,
                w.spacer,
            );
            // The two weights always partition 1.0 so the fill lands at
            // exactly `fraction` of the track.
            assert!((w.fill + w.spacer - 1.0).abs() < 1e-6);
        }
    }
}
