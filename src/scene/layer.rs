//! Layer ordering and fixed per-layer storage.

#[repr(u8)]
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    strum::EnumCount,
    strum::VariantArray,
)]
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
    /// Every layer, back to front. Hit order is this reversed.
    ///
    /// Copied out of the derived `VARIANTS` at const-eval rather than
    /// written out, because paint order *is* declaration order — the
    /// discriminants are the paint sequence. A hand-written table could
    /// only ever agree with the enum or be wrong, which is why it used
    /// to need a `const` block asserting `PAINT_ORDER[i] as usize == i`.
    /// An array rather than the slice so callers keep iterating by
    /// value.
    pub(crate) const PAINT_ORDER: [Layer; <Layer as strum::EnumCount>::COUNT] = {
        let mut out = [Layer::Main; <Layer as strum::EnumCount>::COUNT];
        let mut i = 0;
        while i < out.len() {
            out[i] = <Layer as strum::VariantArray>::VARIANTS[i];
            i += 1;
        }
        out
    };

    #[inline]
    pub(crate) const fn idx(self) -> usize {
        self as usize
    }
}
