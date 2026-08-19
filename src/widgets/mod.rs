/// Implement the `background` builder for container widgets that keep
/// their override in a field called `chrome: Option<Background>`.
///
/// Eight widgets offer one and every body is the same assignment over
/// the same field — the *resolution* differs (a theme slot to fall back
/// to, or none at all, and whether a clip default rides along), but
/// taking the caller's value never does. Invoke it **in the widget's own
/// file**, next to the type.
///
/// Only for the plain case: `Grid` carries generic parameters the macro
/// can't take, and writes the builder by hand — same exemption
/// `impl_configure!` below has for the same type.
macro_rules! impl_background {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl $ty {
                /// Paint `bg` as this widget's background.
                ///
                /// What an unset one falls back to is each widget's own —
                /// a theme slot for most, nothing at all for `Frame`.
                pub fn background(mut self, bg: $crate::primitives::background::Background) -> Self {
                    self.chrome = Some(bg);
                    self
                }
            }
        )+
    };
}

/// Implement [`Configure`](crate::scene::node::Configure) for widget
/// builders that keep their [`Node`](crate::scene::node::Node) in a
/// field called `node`.
///
/// The trait has one required method and every widget's answer is the
/// same expression over the same field, so spelling the block out per
/// widget is pure noise. Invoke it **in the widget's own file**, next to
/// the type — the impl still lives where the struct does.
///
/// Only for the plain case: `Grid` and `RadioButton` carry generic
/// parameters the macro can't take, and write the impl by hand.
///
/// Declared above the module list so textual scoping reaches every
/// widget file, same as `gradient_common!` in `primitives::brush`.
macro_rules! impl_configure {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl $crate::scene::node::Configure for $ty {
                #[inline]
                fn node_mut(&mut self) -> $crate::scene::node::ConfigureNode<'_> {
                    $crate::scene::node::Configure::node_mut(&mut self.node)
                }
            }
        )+
    };
}

pub(crate) mod button;
pub(crate) mod checkbox;
mod chrome;
pub(crate) mod combo_box;
pub(crate) mod context_menu;
pub(crate) mod drag_value;
pub(crate) mod frame;
pub(crate) mod gpu_view;
pub(crate) mod grid;
pub(crate) mod modal;
mod overlay_scope;
pub(crate) mod panel;
pub(crate) mod popup;
pub(crate) mod progress_bar;
pub(crate) mod radio;
pub(crate) mod response;
pub(crate) mod scroll;
pub(crate) mod separator;
pub(crate) mod slider;
pub(crate) mod spinner;
pub(crate) mod splitter;
pub(crate) mod switch;
pub(crate) mod text;
pub(crate) mod text_edit;
pub(crate) mod theme;
pub(crate) mod toggle;
pub(crate) mod tooltip;
pub(crate) mod widget;
