//! Layer ordering and fixed per-layer storage.

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Which recording arena a widget lands in. Each layer is an independent
/// tree; they are painted bottom-up in declaration order and hit-tested
/// top-down, so a popup rejects a pointer before the content beneath it
/// ever sees the event — no per-node z-index anywhere.
///
/// Switch arenas with [`Ui::layer`](crate::Ui::layer). Widgets that manage
/// their own overlay ([`Popup`](crate::Popup), [`Modal`](crate::Modal),
/// [`Tooltip`](crate::Tooltip)) do this for you.
pub enum Layer {
    /// Ordinary content. Everything lands here unless it asks otherwise.
    #[default]
    Main = 0,
    /// Transient overlays anchored to a trigger — dropdowns, pickers, a
    /// form standing beside what it is about.
    Popup = 1,
    /// Dialogs that take the whole window, above popups.
    Modal = 2,
    /// Context menus, above both — because a menu is raised *from* something,
    /// and a popup and a dialog are both things it can be raised from.
    ///
    /// Its own rank rather than sharing the popup's, and that is what a layer
    /// is *for*: an overlay must paint above the scope that raised it, and one
    /// that shared a rank with its own parent could not — see
    /// `Forest::push_layer`. A right-click in a field of a popup is the
    /// ordinary case, and a shared rank leaves it with nowhere to go.
    Menu = 3,
    /// Hover bubbles, above every one of those, so they can annotate a menu
    /// item as readily as a dialog.
    Tooltip = 4,
    /// Diagnostics overlays. Painted last, hit-tested first.
    Debug = 5,
}

impl Layer {
    pub(crate) const COUNT: usize = 6;

    /// Every layer, back to front. Hit order is this reversed.
    ///
    /// Paint order *is* declaration order — the discriminants are the
    /// paint sequence — and the assertion below holds this table to them.
    /// An array rather than a slice so callers keep iterating by value.
    pub(crate) const PAINT_ORDER: [Layer; Layer::COUNT] = [
        Layer::Main,
        Layer::Popup,
        Layer::Modal,
        Layer::Menu,
        Layer::Tooltip,
        Layer::Debug,
    ];

    #[inline]
    pub(crate) const fn idx(self) -> usize {
        self as usize
    }
}

// Entry `i` of the paint order has discriminant `i`, and the last
// variant closes the count.
const _: () = {
    let mut i = 0;
    while i < Layer::COUNT {
        assert!(
            Layer::PAINT_ORDER[i].idx() == i,
            "Layer::PAINT_ORDER must list every discriminant in order",
        );
        i += 1;
    }
    assert!(Layer::Debug.idx() + 1 == Layer::COUNT);
};
