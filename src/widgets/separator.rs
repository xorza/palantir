use crate::forest::element::{Configure, Element, LayoutMode};
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::sizing::{Sizes, Sizing};
use crate::primitives::background::Background;
use crate::primitives::color::Color;
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
            element: Element::new(LayoutMode::Leaf),
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
        let theme = ui.theme.separator;
        let t = self.thickness.unwrap_or(theme.thickness).max(0.0);
        // `Sizes::default()` (Hug×Hug) = "caller didn't set a size" —
        // the same sentinel convention as theme padding/margin.
        if self.element.size == Sizes::default() {
            if self.horizontal {
                self.element.size = (Sizing::Hug, Sizing::Fixed(t)).into();
                self.element.align = Align::h(HAlign::Stretch);
            } else {
                self.element.size = (Sizing::Fixed(t), Sizing::Hug).into();
                self.element.align = Align::v(VAlign::Stretch);
            }
        }
        let chrome = Background::fill(self.color.unwrap_or(theme.color));
        let id = ui.widget_id(&self.element);
        ui.node(id, self.element, Some(&chrome), |_| {});
        // Decorative: skip the eager `response_for` probe.
        Response::lazy(id, ui)
    }
}

impl Configure for Separator {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
