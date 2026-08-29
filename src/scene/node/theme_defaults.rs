//! The theme half of authoring: fill a node field only where the app left
//! it unset, so a builder's explicit value always wins.

use crate::layout::types::limits;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::scene::node::salt::Salt;

/// The *theme* half of [`Configure`]: fill a field in only where the
/// caller stayed silent.
///
/// This is the contract every themed widget states in prose — *explicit
/// wins, the theme fills in the rest*. `Configure`'s plain setters
/// always overwrite, so a widget resolving its defaults has to know
/// whether the caller already spoke, which those setters can't say.
/// These can.
///
/// **Deliberately `pub(crate)` and separate from `Configure`.** Theme
/// resolution is the framework's job, not the caller's: an app chaining
/// `.default_padding(…)` onto a `Button` would be overriding nothing and
/// shadowing a decision the widget makes for it. Keeping the family off
/// the public trait keeps it off every exported widget's method list.
///
/// Blanket-implemented for everything `Configure`, so it reaches a bare
/// [`Node`](crate::scene::node::Node) *and* a widget that wraps one —
/// `ContextMenu` resolves the
/// menu theme into the `Popup` it is built from, which an inherent
/// `Node` method could not do without one widget reaching into the
/// other's node.
pub(crate) trait ThemeDefaults: Configure {
    /// Identity to fall back on when the caller set none.
    ///
    /// "Set" means [`Configure::id`] / [`Configure::id_salt`] — a
    /// `#[track_caller]` auto id doesn't count, since every widget has
    /// one and counting it would make the fallback unreachable.
    fn default_id(mut self, id: WidgetId) -> Self {
        let node = self.node_mut().node;
        if !node.salt.is_explicit() {
            node.salt = Salt::Verbatim(id);
        }
        self
    }

    /// Padding to fall back on when the caller set none.
    fn default_padding(mut self, p: impl Into<Spacing>) -> Self {
        let node = self.node_mut().node;
        if node.padding.is_none() {
            node.padding = Some(p.into());
        }
        self
    }

    /// Margin to fall back on when the caller set none.
    fn default_margin(mut self, m: impl Into<Spacing>) -> Self {
        let node = self.node_mut().node;
        if node.margin.is_none() {
            node.margin = Some(m.into());
        }
        self
    }

    /// Sibling spacing to fall back on when the caller set none.
    fn default_gap(mut self, g: f32) -> Self {
        let node = self.node_mut().node;
        if !node.gaps.gap_is_set() {
            node.gaps.set_gap(g);
        }
        self
    }

    /// Lower size bound to fall back on when the caller set none.
    fn default_min_size(mut self, s: impl Into<Size>) -> Self {
        let node = self.node_mut().node;
        if node.min_size.is_none() {
            let value = s.into();
            limits::assert_valid_bounds(value, node.max_size.unwrap_or(Size::INF));
            node.min_size = Some(value);
        }
        self
    }

    /// Upper size bound to fall back on when the caller set none.
    fn default_max_size(mut self, s: impl Into<Size>) -> Self {
        let node = self.node_mut().node;
        if node.max_size.is_none() {
            let value = s.into();
            limits::assert_valid_bounds(node.min_size.unwrap_or(Size::ZERO), value);
            node.max_size = Some(value);
        }
        self
    }
}

impl<T: Configure> ThemeDefaults for T {}
