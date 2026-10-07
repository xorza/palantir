//! Layer ordering and fixed per-layer storage.

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Which recording arena a widget lands in. Layers paint bottom-up in
/// declaration order and hit-test top-down; there is no per-node z-index.
///
/// Switch with [`Ui::layer`](crate::Ui::layer); `Popup`, `Modal` and
/// `Tooltip` do it for you.
pub enum Layer {
    /// Ordinary content; the default.
    #[default]
    Main = 0,
    /// Overlays anchored to a trigger: dropdowns, pickers.
    Popup = 1,
    /// Dialogs that take the whole window, above popups.
    Modal = 2,
    /// Context menus, above both. Own rank so a menu raised from a popup paints above it (see `Forest::push_layer`).
    Menu = 3,
    /// Hover bubbles, above everything else.
    Tooltip = 4,
    /// Diagnostics overlays. Painted last, hit-tested first.
    Debug = 5,
}

impl Layer {
    pub(crate) const COUNT: usize = 6;

    /// Every layer, back to front. Hit order is reversed. Held to the discriminants by the assertion below.
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

// Entry `i` has discriminant `i`.
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
