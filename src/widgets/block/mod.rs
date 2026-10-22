//! A decorated leaf rectangle: chrome, size, margin, no body.

use crate::primitives::paint::chrome::Chrome;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;

/// A leaf rectangle with optional chrome, size, margin and `Sense`: dividers, hit areas, swatches, spacers. Content inside needs a [`Panel`](crate::Panel).
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Block {
    widget: Widget,
    chrome: Option<Chrome>,
}

impl Block {
    #[track_caller]
    /// A block.
    pub fn new() -> Self {
        Self {
            widget: Widget::leaf(),
            chrome: None,
        }
    }

    /// Records the block.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let chrome = self.chrome;
        self.widget.show(ui, chrome.as_ref(), |_| {}).response
    }
}

impl Block {
    /// Paint `chrome`. `Block` is unthemed, so unset paints nothing.
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` holds the kinds [`Chrome`] lists.
    #[track_caller]
    pub const fn chrome(mut self, chrome: Chrome) -> Self {
        chrome.validate();
        self.chrome = Some(chrome);
        self
    }

    /// Paint `chrome` unless the caller set one; for wrappers that theme a held widget after the caller's setters. An explicit [`Self::chrome`] wins in either order.
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` holds the kinds [`Chrome`] lists.
    #[track_caller]
    pub const fn default_chrome(mut self, chrome: Chrome) -> Self {
        chrome.validate();
        if self.chrome.is_none() {
            self.chrome = Some(chrome);
        }
        self
    }
}

impl Configure for Block {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
