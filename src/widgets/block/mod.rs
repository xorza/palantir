//! A decorated rectangle with nothing inside it: background, size and
//! margin, and none of the interaction a container carries.

use crate::primitives::background::Background;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ConfigureWidget;
use crate::widgets::response::Response;
use crate::widgets::widget::Widget;

/// A leaf rectangle: optional background / size / margin plus an optional
/// `Sense`. Dividers, hit areas, colour swatches, spacers. Chrome + clip
/// behavior come from [`Self::background`] /
/// [`Configure::clip_rect`](crate::Configure::clip_rect) /
/// [`Configure::clip_rounded`](crate::Configure::clip_rounded).
///
/// **It takes no body.** A decorated rectangle *around* content is a
/// [`Panel`](crate::Panel) with a background.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Block {
    widget: Widget,
    chrome: Option<Background>,
}

impl Block {
    /// An empty block. It occupies space, and paints nothing until
    /// [`Self::background`] gives it something to paint.
    #[track_caller]
    pub fn new() -> Self {
        Self {
            widget: Widget::leaf(),
            chrome: None,
        }
    }

    /// Record the rectangle. The [`Response`] answers only where the
    /// `Sense` given through [`Configure::sense`] lets it.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let chrome = self.chrome;
        self.widget.show(ui, chrome.as_ref(), |_| {}).response
    }
}

impl Block {
    /// Paint `bg` as this widget's background.
    ///
    /// `Block` is unthemed: there is no slot to fall back to, so an unset
    /// background paints nothing.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }
}

impl Configure for Block {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
