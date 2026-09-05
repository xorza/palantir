//! The thin divider rule, on either axis.

use crate::layout::axis::Axis;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::num::F32Ext;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ThemeDefaults;
use crate::widgets::response::Response;
use crate::widgets::theme::separator::SeparatorTheme;
use crate::widgets::widget::Widget;

/// A thin divider rule between content. [`Separator::horizontal`]
/// stretches across the parent's width as a `thickness`-tall line;
/// [`Separator::vertical`] is a `thickness`-wide column rule.
///
/// Sized `Hug + Stretch` on its long axis so it fills the parent's cross
/// extent without leaking an infinite size up to a `Hug` ancestor. An
/// explicit [`Configure::size`] replaces that default entirely — the
/// given size describes the rule's box and `thickness` is ignored — and
/// an explicit [`Configure::align`] replaces it on the axis it names,
/// leaving the other axis to the default.
/// Visuals come from [`crate::SeparatorTheme`] (theme slot `separator`).
#[derive(Debug)]
pub struct Separator<'a> {
    widget: Widget,
    axis: Axis,
    thickness: Option<f32>,
    color: Option<RgbaF32>,
    style: Option<&'a SeparatorTheme>,
}

impl<'a> Separator<'a> {
    /// A horizontal rule (stretches across the parent's width).
    #[track_caller]
    pub fn horizontal() -> Self {
        Self::along(Axis::X)
    }

    /// A vertical rule (stretches down the parent's height).
    #[track_caller]
    pub fn vertical() -> Self {
        Self::along(Axis::Y)
    }

    #[track_caller]
    fn along(axis: Axis) -> Self {
        Self::over(Widget::leaf(), axis)
    }

    /// A rule on `axis` over a node the caller already built, so
    /// [`crate::MenuSeparator`] forwards the `Configure` calls that
    /// landed on it — identity included, which is why this takes the
    /// node rather than building one at *this* call site.
    pub(crate) fn over(widget: Widget, axis: Axis) -> Self {
        Self {
            widget,
            axis,
            thickness: None,
            color: None,
            style: None,
        }
    }

    style_setter!(
        'a,
        SeparatorTheme,
        separator,
        "[`crate::MenuSeparator`] passes `theme.context_menu.separator` here \
         instead. Per-field [`Self::color`] / [`Self::thickness`] still win \
         over whichever bundle is in play.",
    );

    /// Line thickness in logical px, defaulting to
    /// [`crate::Theme::separator`]'s. One-axis hatch over the resolved bundle — see [`crate::Theme`].
    pub fn thickness(mut self, px: f32) -> Self {
        self.thickness = Some(px);
        self
    }

    /// Line color, defaulting to [`crate::Theme::separator`]'s.
    /// One-axis hatch over the resolved bundle — see [`crate::Theme`].
    pub fn color(mut self, c: RgbaF32) -> Self {
        self.color = Some(c);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let theme = self.slot(ui.theme());
        let t = self.thickness.unwrap_or(theme.thickness).themed_length(0.0);
        let (default_size, stretch) = match self.axis {
            Axis::X => ((Sizing::HUG, Sizing::fixed(t)), Align::h(HAlign::Stretch)),
            Axis::Y => ((Sizing::fixed(t), Sizing::HUG), Align::v(VAlign::Stretch)),
        };
        // The stretch belongs to the `Hug` default, not to the rule: it
        // is what spans the parent, and applying it over an explicit
        // size would override the extent the caller gave.
        let widget = match self.widget.authored_size() {
            Some(_) => self.widget,
            None => self.widget.size(default_size).default_align(stretch),
        };
        let chrome = Background::fill(self.color.unwrap_or(theme.color));
        // Theme margin fills in only where the caller stayed silent —
        // the menu slot holds its rule off the rows above and below,
        // the in-flow slot leaves it at zero.
        let widget = widget.default_margin(theme.margin);
        widget.show(ui, Some(&chrome), |_| {}).response
    }
}

impl_configure!(Separator<'_>);

#[cfg(test)]
mod tests;
