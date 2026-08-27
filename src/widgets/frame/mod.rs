//! A decorated rectangle: background, size and margin around a body, with
//! none of the interaction the other containers carry.

use crate::primitives::background::Background;
use crate::scene::node::Node;
use crate::ui::Ui;
use crate::widgets::response::Response;

/// A simple decorated rectangle: optional background / size / margin
/// plus an optional `Sense`. Used directly for dividers / hit-areas /
/// bg swatches. Chrome + clip behavior come from
/// [`Self::background`] / [`Configure::clip_rect`](crate::Configure::clip_rect) /
/// [`Configure::clip_rounded`](crate::Configure::clip_rounded).
#[derive(Debug)]
pub struct Frame {
    node: Node,
    chrome: Option<Background>,
}

impl Frame {
    #[track_caller]
    pub fn new() -> Self {
        Self {
            node: Node::leaf(),
            chrome: None,
        }
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let chrome = self.chrome;
        ui.widget(self.node)
            .show(ui, chrome.as_ref(), |_| {})
            .response
    }
}

impl_background!(
    Frame,
    "`Frame` is the unthemed container: there is no slot to fall back to, \
     so an unset background paints nothing.",
);
impl_configure!(Frame);

#[cfg(test)]
mod tests;
