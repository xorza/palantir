use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::scene::node::ThemeDefaults;
use crate::scene::node::{Configure, Node};
use crate::ui::Ui;
use crate::widgets::response::Response;
use crate::widgets::theme::separator::SeparatorTheme;

/// A thin divider rule between content. [`Separator::horizontal`]
/// stretches across the parent's width as a `thickness`-tall line;
/// [`Separator::vertical`] is a `thickness`-wide column rule.
///
/// Sized `Hug + Stretch` on its long axis so it fills the parent's cross
/// extent without leaking an infinite size up to a `Hug` ancestor. An
/// explicit [`Configure::size`] replaces that default entirely — the
/// given size describes the rule's box and `thickness` is ignored.
/// Visuals come from [`crate::SeparatorTheme`] (theme slot `separator`).
#[derive(Debug)]
pub struct Separator<'a> {
    node: Node,
    horizontal: bool,
    thickness: Option<f32>,
    color: Option<Color>,
    style: Option<&'a SeparatorTheme>,
}

impl<'a> Separator<'a> {
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
            node: Node::leaf(),
            horizontal,
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
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> Response<'_> {
        let theme = self.slot(ui.theme());
        let t = self.thickness.unwrap_or(theme.thickness).max(0.0);
        let margin = theme.margin;
        let default_size = if self.horizontal {
            (Sizing::HUG, Sizing::fixed(t))
        } else {
            (Sizing::fixed(t), Sizing::HUG)
        };
        if self.node.size.is_none() {
            // `Node` is `Copy`, so the chain reads back into the field.
            self.node = self.node.size(default_size).align(if self.horizontal {
                Align::h(HAlign::Stretch)
            } else {
                Align::v(VAlign::Stretch)
            });
        }
        let chrome = Background::fill(self.color.unwrap_or(theme.color));
        // Theme margin fills in only where the caller stayed silent —
        // the menu slot holds its rule off the rows above and below,
        // the in-flow slot leaves it at zero.
        let node = self.node.default_margin(margin);
        ui.widget(node).show(ui, Some(&chrome), |_| {}).response
    }
}

impl_configure!(Separator<'_>);

#[cfg(test)]
mod tests;
