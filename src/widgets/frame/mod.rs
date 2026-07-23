use crate::primitives::background::Background;
use crate::scene::node::{Configure, ConfigureNode, Node};
use crate::ui::Ui;
use crate::widgets::Response;

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
        let widget = ui.widget(self.node);
        widget.record(ui, self.chrome.as_ref(), |_| {});
        // Decorative: skip eager `response_for`.
        widget.response(ui)
    }
}

impl Configure for Frame {
    fn node_mut(&mut self) -> ConfigureNode<'_> {
        self.node.node_mut()
    }
}

#[cfg(test)]
mod tests;
