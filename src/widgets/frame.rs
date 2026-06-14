use crate::forest::element::{Configure, Element, LayoutMode};
use crate::primitives::background::Background;
use crate::ui::Ui;
use crate::widgets::Response;

/// A simple decorated rectangle: optional background / size / margin
/// plus an optional `Sense`. Used directly for dividers / hit-areas /
/// bg swatches, and as the rendering primitive inside `Button`.
/// Chrome + clip behavior come from
/// [`Self::background`] / [`Configure::clip_rect`] /
/// [`Configure::clip_rounded`].
pub struct Frame {
    element: Element,
    chrome: Option<Background>,
}

impl Frame {
    #[allow(clippy::new_without_default)]
    #[track_caller]
    pub fn new() -> Self {
        Self {
            element: Element::new(LayoutMode::Leaf),
            chrome: None,
        }
    }

    /// Paint chrome (fill / stroke / corner radius / shadow).
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let id = ui.widget_id(&self.element);
        ui.node(id, self.element, self.chrome.as_ref(), |_| {});
        // Decorative: skip eager `response_for`.
        Response::lazy(id, ui)
    }
}

impl Configure for Frame {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
