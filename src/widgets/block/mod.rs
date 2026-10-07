//! A decorated leaf rectangle: background, size, margin, no body.

use crate::primitives::paint::background::Background;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;

/// A leaf rectangle with optional background, size, margin and `Sense`: dividers, hit areas, swatches, spacers. Content inside needs a [`Panel`](crate::Panel).
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Block {
    widget: Widget,
    chrome: Option<Background>,
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
    /// Paint `background`. `Block` is unthemed, so unset paints nothing.
    ///
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background) lists.
    #[track_caller]
    pub const fn background(mut self, background: Background) -> Self {
        background.validate();
        self.chrome = Some(background);
        self
    }

    /// Paint `background` unless the caller set one; for wrappers that theme a held widget after the caller's setters. An explicit [`Self::background`] wins in either order.
    ///
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background) lists.
    #[track_caller]
    pub const fn default_background(mut self, background: Background) -> Self {
        background.validate();
        if self.chrome.is_none() {
            self.chrome = Some(background);
        }
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
