use crate::forest::element::{Configure, Element, LayoutMode};
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::theme::palette;

/// A thin divider rule between content. [`Separator::horizontal`]
/// stretches across the parent's width as a `thickness`-tall line;
/// [`Separator::vertical`] is a `thickness`-wide column rule.
///
/// Sized `Hug + Stretch` on its long axis so it fills the parent's cross
/// extent without leaking an infinite size up to a `Hug` ancestor.
pub struct Separator {
    element: Element,
    horizontal: bool,
    thickness: f32,
    color: Color,
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
            thickness: 1.0,
            color: palette::TEXT_MUTED.with_alpha(0.18),
        }
    }

    /// Line thickness in logical px. Default `1.0`.
    pub fn thickness(mut self, px: f32) -> Self {
        self.thickness = px;
        self
    }

    /// Line color. Default a muted, low-alpha rule.
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> Response<'_> {
        let t = self.thickness.max(0.0);
        if self.horizontal {
            self.element.size = (Sizing::Hug, Sizing::Fixed(t)).into();
            self.element.align = Align::h(HAlign::Stretch);
        } else {
            self.element.size = (Sizing::Fixed(t), Sizing::Hug).into();
            self.element.align = Align::v(VAlign::Stretch);
        }
        let chrome = Background::fill(self.color);
        let id = ui.make_persistent_id(self.element.salt);
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
