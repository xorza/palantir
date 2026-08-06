use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::scene::node::Node;
use crate::ui::Ui;
use crate::widgets::chrome;
use crate::widgets::response::Response;
use crate::widgets::theme::progress_bar::ProgressBarTheme;

/// Determinate progress bar: a rounded `track` with an accent fill
/// spanning `fraction` (clamped to `0..=1`) of its width.
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

    /// Borrow a theme override for this bar. The default inherits
    /// [`crate::Theme::progress_bar`].
    pub fn style(mut self, s: &'a ProgressBarTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let theme = self.style.unwrap_or(&ui.theme().progress_bar);
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
                chrome::leaf(ui, id.with("fill"), (fill, Sizing::FILL), Some(&fill_bg));
                // Remainder spacer — its `Fill` weight pushes the fill to the
                // correct fraction of the track width.
                chrome::leaf(ui, id.with("rest"), (spacer, Sizing::FILL), None);
            })
            .response
    }
}

impl_configure!(ProgressBar<'_>);

#[cfg(test)]
mod tests {
    use crate::ui::harness::UiHarness;

    use crate::layout::types::sizing::Sizing;
    use crate::scene::layer::Layer;
    use crate::scene::node::Configure;
    use crate::widgets::panel::Panel;
    use crate::widgets::progress_bar::ProgressBar;
    use glam::UVec2;

    /// Explicit `.size(...)` wins over the widget's `Fill × theme.height`
    /// default, and an untouched bar still gets that default (400-wide FILL
    /// column → 400 × theme height 6).
    #[test]
    fn explicit_size_overrides_fill_default() {
        let mut h = UiHarness::new(UVec2::new(400, 300));
        let (mut sized, mut hug, mut default) = (None, None, None);
        h.frame(|ui| {
            let col = Panel::vstack().auto_id().size((Sizing::FILL, Sizing::FILL));
            col.show(ui, |ui| {
                sized = Some(
                    ProgressBar::new(0.3)
                        .size((Sizing::fixed(80.0), Sizing::fixed(10.0)))
                        .show(ui)
                        .node(),
                );
                hug = Some(
                    ProgressBar::new(0.3)
                        .size((Sizing::HUG, Sizing::HUG))
                        .show(ui)
                        .node(),
                );
                default = Some(ProgressBar::new(0.3).show(ui).node());
            });
        });
        let rects = &h.ui.layout[Layer::Main].rect;
        let s = rects[sized.unwrap().idx()];
        assert_eq!((s.size.w, s.size.h), (80.0, 10.0), "explicit size");
        let h = rects[hug.unwrap().idx()];
        assert_eq!((h.size.w, h.size.h), (0.0, 0.0), "explicit hug");
        let d = rects[default.unwrap().idx()];
        assert_eq!((d.size.w, d.size.h), (400.0, 6.0), "untouched default");
    }

    #[test]
    fn endpoint_segments_collapse_without_invalid_fill_weights() {
        for (fraction, expected) in [(0.0, [0.0, 100.0]), (1.0, [100.0, 0.0])] {
            let mut h = UiHarness::new(UVec2::new(100, 20));
            let root = h.frame_value(|ui| {
                ProgressBar::new(fraction)
                    .size((Sizing::fixed(100.0), Sizing::fixed(10.0)))
                    .show(ui)
                    .node()
            });
            let widths: Vec<_> = h
                .main_child_rects(root)
                .into_iter()
                .map(|rect| rect.size.w)
                .collect();
            assert_eq!(widths, expected, "fraction {fraction}");
        }
    }
}
