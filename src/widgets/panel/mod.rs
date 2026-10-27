//! The container widget: every stack, wrap and canvas layout, over the one node layout drivers dispatch on.

use crate::primitives::layout::axis::Axis;
use crate::primitives::paint::chrome::Chrome;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::response::InnerResponse;
use crate::widget_core::widget::Widget;
use std::rc::Rc;

/// The container widget: an h-, v- or z-stack chosen by constructor, with
/// optional chrome ([`Self::chrome`]) and clip. Both default to `None`;
/// `theme.panel_chrome` and `theme.panel_clip` supply fallbacks.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Panel {
    widget: Widget,
    chrome: Option<Chrome>,
}

impl Panel {
    const fn auto(widget: Widget) -> Self {
        Self {
            widget,
            chrome: None,
        }
    }

    /// Records the panel and its `body`.
    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R> {
        let theme = Rc::clone(ui.theme());
        let widget = self.widget.default_clip(theme.panel_clip);
        let chrome = self.chrome.as_ref().or(theme.panel_chrome.as_ref());
        widget.show(ui, chrome, body)
    }

    /// Children left to right on one line.
    #[track_caller]
    pub fn hstack() -> Self {
        Self::auto(Widget::hstack())
    }

    /// Children top to bottom in one column.
    #[track_caller]
    pub fn vstack() -> Self {
        Self::auto(Widget::vstack())
    }

    /// Children in one line along `axis`, for a direction picked at run time.
    #[track_caller]
    pub fn stack(axis: Axis) -> Self {
        Self::auto(Widget::stack(axis))
    }

    /// HStack that wraps to a new row when the next child won't fit. `.gap(g)`
    /// spaces siblings, `.line_gap(g)` rows. Main-axis `Sizing::fill` is treated as `Hug`.
    #[track_caller]
    pub fn wrap_hstack() -> Self {
        Self::auto(Widget::wrap_hstack())
    }

    /// VStack that wraps into new columns; `wrap_hstack` with axes swapped.
    #[track_caller]
    pub fn wrap_vstack() -> Self {
        Self::auto(Widget::wrap_vstack())
    }

    /// A wrapping stack along `axis`, for a direction picked at run time.
    #[track_caller]
    pub fn wrap_stack(axis: Axis) -> Self {
        Self::auto(Widget::wrap_stack(axis))
    }

    /// Layered children at the parent's inner top-left; the last paints on top.
    #[track_caller]
    pub fn zstack() -> Self {
        Self::auto(Widget::zstack())
    }

    /// Children at their `.pos(Vec2)`; hugs the bounding box of the placed children.
    #[track_caller]
    pub fn canvas() -> Self {
        Self::auto(Widget::canvas())
    }
}

impl Panel {
    /// Paints `chrome`. Unset, the theme's `panel_chrome` fills in; [`Chrome::NONE`] suppresses that.
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

    /// Paints `chrome` unless the caller set one, for a wrapper theming a
    /// widget after the caller's setters; an explicit [`Self::chrome`] wins in either order.
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

impl Configure for Panel {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::layout::axis::Axis;
    use crate::widgets::panel::Panel;

    impl Panel {
        /// [`Panel::wrap_hstack`] or [`Panel::wrap_vstack`], packing along `axis`.
        #[track_caller]
        pub(crate) fn wrap_stack_on(axis: Axis) -> Self {
            match axis {
                Axis::X => Self::wrap_hstack(),
                Axis::Y => Self::wrap_vstack(),
            }
        }
    }
}

#[cfg(test)]
mod tests;
