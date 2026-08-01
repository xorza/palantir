use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::scene::node::Node;
use crate::scene::node::ThemeDefaults;
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

    /// Borrow a theme override for this rule. The default inherits
    /// [`crate::Theme::separator`]; [`crate::MenuSeparator`] passes
    /// `theme.context_menu.separator` instead. Per-field
    /// [`Self::color`] / [`Self::thickness`] still win over whichever
    /// bundle is in play.
    pub fn style(mut self, s: &'a SeparatorTheme) -> Self {
        self.style = Some(s);
        self
    }

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
        let theme = self.style.unwrap_or(&ui.theme.separator);
        let t = self.thickness.unwrap_or(theme.thickness).max(0.0);
        let margin = theme.margin;
        let default_size = if self.horizontal {
            (Sizing::HUG, Sizing::fixed(t)).into()
        } else {
            (Sizing::fixed(t), Sizing::HUG).into()
        };
        if self.node.size.is_none() {
            self.node.size = Some(default_size);
            self.node.align = if self.horizontal {
                Align::h(HAlign::Stretch)
            } else {
                Align::v(VAlign::Stretch)
            };
        }
        let chrome = Background::fill(self.color.unwrap_or(theme.color));
        // Theme margin fills in only where the caller stayed silent —
        // the menu slot holds its rule off the rows above and below,
        // the in-flow slot leaves it at zero.
        let node = self.node.default_margin(margin);
        let widget = ui.widget(node);
        widget.record(ui, Some(&chrome), |_| {});
        // Decorative: skip the eager `response_for` probe.
        widget.response(ui)
    }
}

impl_configure!(Separator<'_>);

#[cfg(test)]
mod tests {
    use crate::ui::harness::UiHarness;

    use crate::layout::types::sizing::Sizing;
    use crate::primitives::spacing::Spacing;
    use crate::scene::layer::Layer;
    use crate::scene::node::Configure;
    use crate::widgets::panel::Panel;
    use crate::widgets::separator::Separator;
    use crate::widgets::theme::separator::SeparatorTheme;
    use glam::UVec2;

    /// `Separator` gained the per-instance `.style(&SeparatorTheme)`
    /// every other themed widget already had — which is what lets
    /// `MenuSeparator` hand its slot down whole instead of unpacking it
    /// field by field.
    ///
    /// `margin` came with it, so the bundle also has to fill in where
    /// the builder stayed silent and lose where it didn't: the menu slot
    /// holds its rule off the rows around it, the in-flow slot leaves it
    /// at zero, and a caller who says `.margin(...)` beats both.
    #[test]
    fn instance_style_beats_the_global_slot_and_explicit_margin_beats_both() {
        let styled = SeparatorTheme {
            thickness: 3.0,
            margin: Spacing::xy(0.0, 5.0),
            ..SeparatorTheme::default()
        };
        let mut h = UiHarness::new(UVec2::new(400, 300));
        // Loudly different global slot — a styled rule must not reach it.
        h.ui.theme.separator.thickness = 11.0;
        h.ui.theme.separator.margin = Spacing::all(9.0);

        let (mut inherited, mut explicit, mut global) = (None, None, None);
        h.frame(|ui| {
            let col = Panel::vstack().auto_id().size((Sizing::FILL, Sizing::FILL));
            col.show(ui, |ui| {
                inherited = Some(Separator::horizontal().style(&styled).show(ui).node());
                explicit = Some(
                    Separator::horizontal()
                        .style(&styled)
                        .margin(Spacing::ZERO)
                        .show(ui)
                        .node(),
                );
                global = Some(Separator::horizontal().show(ui).node());
            });
        });

        let layouts = h.ui.forest.trees[Layer::Main].records.layout();
        let rects = &h.ui.layout[Layer::Main].rect;
        assert_eq!(
            layouts[inherited.unwrap().idx()].margin,
            Spacing::xy(0.0, 5.0),
            "the styled bundle's margin fills in",
        );
        assert_eq!(
            rects[inherited.unwrap().idx()].size.h,
            3.0,
            "the styled bundle's thickness wins over the global slot's 11",
        );
        assert_eq!(
            layouts[explicit.unwrap().idx()].margin,
            Spacing::ZERO,
            "an explicit margin beats the styled bundle",
        );
        assert_eq!(
            layouts[global.unwrap().idx()].margin,
            Spacing::all(9.0),
            "an unstyled rule still reads the global slot",
        );
    }

    /// Explicit `.size(...)` replaces the Hug+Stretch/thickness default
    /// entirely, and an untouched horizontal rule still stretches across
    /// the 400-wide FILL column at the theme thickness of 1.
    #[test]
    fn explicit_size_overrides_stretch_default() {
        let mut h = UiHarness::new(UVec2::new(400, 300));
        let (mut sized, mut hug, mut default) = (None, None, None);
        h.frame(|ui| {
            let col = Panel::vstack().auto_id().size((Sizing::FILL, Sizing::FILL));
            col.show(ui, |ui| {
                sized = Some(
                    Separator::horizontal()
                        .size((Sizing::fixed(50.0), Sizing::fixed(3.0)))
                        .show(ui)
                        .node(),
                );
                hug = Some(
                    Separator::horizontal()
                        .size((Sizing::HUG, Sizing::HUG))
                        .show(ui)
                        .node(),
                );
                default = Some(Separator::horizontal().show(ui).node());
            });
        });
        let rects = &h.ui.layout[Layer::Main].rect;
        let s = rects[sized.unwrap().idx()];
        assert_eq!((s.size.w, s.size.h), (50.0, 3.0), "explicit size");
        let h = rects[hug.unwrap().idx()];
        assert_eq!((h.size.w, h.size.h), (0.0, 0.0), "explicit hug");
        let d = rects[default.unwrap().idx()];
        assert_eq!((d.size.w, d.size.h), (400.0, 1.0), "untouched default");
    }
}
