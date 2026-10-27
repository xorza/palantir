//! Layer ordering.

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Which layer a widget records in, and what an overlay is. Layers paint bottom-up
/// in declaration order and hit-test top-down; there is no per-node z-index. An
/// overlay opened inside another one records above it, in the higher of the two
/// layers (see [`Ui::layer`](crate::Ui::layer)).
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
    /// Context menus, above both.
    Menu = 3,
    /// Hover bubbles, above everything else.
    Tooltip = 4,
    /// Diagnostics overlays. Painted last, hit-tested first.
    Debug = 5,
}

impl Layer {
    pub(crate) const COUNT: usize = 6;

    /// Every layer, back to front. Hit order is reversed. Held to the discriminants by the assertion below.
    pub(crate) const PAINT_ORDER: [Self; Self::COUNT] = [
        Self::Main,
        Self::Popup,
        Self::Modal,
        Self::Menu,
        Self::Tooltip,
        Self::Debug,
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
