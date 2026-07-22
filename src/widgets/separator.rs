use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::scene::element::{Configure, ConfigureElement, Element};
use crate::ui::Ui;
use crate::widgets::Response;

/// A thin divider rule between content. [`Separator::horizontal`]
/// stretches across the parent's width as a `thickness`-tall line;
/// [`Separator::vertical`] is a `thickness`-wide column rule.
///
/// Sized `Hug + Stretch` on its long axis so it fills the parent's cross
/// extent without leaking an infinite size up to a `Hug` ancestor. An
/// explicit [`Configure::size`] replaces that default entirely — the
/// given size describes the rule's box and `thickness` is ignored.
/// Visuals come from [`crate::SeparatorTheme`] (theme slot `separator`).
#[derive(Debug)]
pub struct Separator {
    element: Element,
    horizontal: bool,
    thickness: Option<f32>,
    color: Option<Color>,
}

impl Separator {
    /// A horizontal rule (stretches across the parent's width).
    #[track_caller]
    pub fn horizontal() -> Self {
        Self::axis(true)
    }

    /// A vertical rule (stretches down the parent's height).
    #[track_caller]
    pub fn vertical() -> Self {
        Self::axis(false)
    }

    #[track_caller]
    fn axis(horizontal: bool) -> Self {
        Self {
            element: Element::leaf(),
            horizontal,
            thickness: None,
            color: None,
        }
    }

    /// Line thickness in logical px. `None` (default) inherits
    /// [`crate::Theme::separator`].
    pub fn thickness(mut self, px: f32) -> Self {
        self.thickness = Some(px);
        self
    }

    /// Line color. `None` (default) inherits [`crate::Theme::separator`].
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> Response<'_> {
        let theme = &ui.theme.separator;
        let t = self.thickness.unwrap_or(theme.thickness).max(0.0);
        let default_size = if self.horizontal {
            (Sizing::HUG, Sizing::fixed(t)).into()
        } else {
            (Sizing::fixed(t), Sizing::HUG).into()
        };
        if self.element.configured().size().is_none() {
            self.element.size = default_size;
            self.element.align = if self.horizontal {
                Align::h(HAlign::Stretch)
            } else {
                Align::v(VAlign::Stretch)
            };
        }
        let chrome = Background::fill(self.color.unwrap_or(theme.color));
        let id = ui.widget_id(&self.element);
        ui.node(id, self.element, Some(&chrome), |_| {});
        // Decorative: skip the eager `response_for` probe.
        Response::lazy(id, ui)
    }
}

impl Configure for Separator {
    fn element_mut(&mut self) -> ConfigureElement<'_> {
        self.element.element_mut()
    }
}

#[cfg(test)]
mod tests {
    use crate::Ui;
    use crate::layout::types::sizing::Sizing;
    use crate::scene::element::Configure;
    use crate::scene::layer::Layer;
    use crate::widgets::panel::Panel;
    use crate::widgets::separator::Separator;
    use glam::UVec2;

    /// Explicit `.size(...)` replaces the Hug+Stretch/thickness default
    /// entirely, and an untouched horizontal rule still stretches across
    /// the 400-wide FILL column at the theme thickness of 1.
    #[test]
    fn explicit_size_overrides_stretch_default() {
        let mut ui = Ui::for_test();
        let (mut sized, mut hug, mut default) = (None, None, None);
        ui.run_at_without_baseline(UVec2::new(400, 300), |ui| {
            let col = Panel::vstack().auto_id().size((Sizing::FILL, Sizing::FILL));
            col.show(ui, |ui| {
                sized = Some(
                    Separator::horizontal()
                        .size((Sizing::fixed(50.0), Sizing::fixed(3.0)))
                        .show(ui)
                        .node(),
                );
                hug = Some(
                    Separator::horizontal()
                        .size((Sizing::HUG, Sizing::HUG))
                        .show(ui)
                        .node(),
                );
                default = Some(Separator::horizontal().show(ui).node());
            });
        });
        let rects = &ui.layout[Layer::Main].rect;
        let s = rects[sized.unwrap().idx()];
        assert_eq!((s.size.w, s.size.h), (50.0, 3.0), "explicit size");
        let h = rects[hug.unwrap().idx()];
        assert_eq!((h.size.w, h.size.h), (0.0, 0.0), "explicit hug");
        let d = rects[default.unwrap().idx()];
        assert_eq!((d.size.w, d.size.h), (400.0, 1.0), "untouched default");
    }
}
