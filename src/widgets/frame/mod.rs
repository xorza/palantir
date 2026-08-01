use crate::primitives::background::Background;
use crate::scene::node::Node;
use crate::ui::Ui;
use crate::widgets::response::Response;

/// A simple decorated rectangle: optional background / size / margin
/// plus an optional `Sense`. Used directly for dividers / hit-areas /
/// bg swatches. Chrome + clip behavior come from
/// [`Self::background`] / [`Configure::clip_rect`] /
/// [`Configure::clip_rounded`].
#[derive(Debug)]
pub struct Frame {
    node: Node,
    chrome: Option<Background>,
}

impl Frame {
    #[allow(clippy::new_without_default)]
    #[track_caller]
    pub fn new() -> Self {
        Self {
            node: Node::leaf(),
            chrome: None,
        }
    }

    /// Paint chrome (fill / stroke / corner radius / shadow).
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let chrome = self.chrome;
        ui.widget(self.node)
            .show(ui, chrome.as_ref(), |_| {})
            .response
    }
}

impl_configure!(Frame);

#[cfg(test)]
mod tests;
