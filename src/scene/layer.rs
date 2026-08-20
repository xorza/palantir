//! Layer ordering and fixed per-layer storage.

use std::array;
use strum::EnumCount as _;

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
    /// Transient overlays anchored to a trigger — dropdowns, context menus.
    Popup = 1,
    /// Dialogs that take the whole window, above popups.
    Modal = 2,
    /// Hover bubbles, above modals so they can annotate a dialog.
    Tooltip = 3,
    /// Diagnostics overlays. Painted last, hit-tested first.
    Debug = 4,
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

/// Fixed-size `[T; Layer::COUNT]` indexed by [`Layer`].
///
/// Three ways in, one per question the caller is asking: `Index<Layer>`
/// / `IndexMut<Layer>` for a known layer, [`Self::iter`] /
/// [`Self::iter_mut`] when the layer doesn't matter, and
/// [`Self::iter_paint_order`] when it does. The backing array is private
/// so those stay the only spellings.
#[derive(Debug)]
#[repr(transparent)]
pub(crate) struct PerLayer<T>([T; Layer::COUNT]);

impl<T: Default> Default for PerLayer<T> {
    fn default() -> Self {
        Self(array::from_fn(|_| T::default()))
    }
}

impl<T> PerLayer<T> {
    /// Every layer's slot, order unspecified — for folds that don't care
    /// which layer a value came from.
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }

    pub(crate) fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.0.iter_mut()
    }

    /// Iterate `(Layer, &T)` in [`Layer::PAINT_ORDER`] — bottom-up
    /// (under-first). Reverse for topmost-first hit-test traversal.
    pub(crate) fn iter_paint_order(&self) -> impl Iterator<Item = (Layer, &T)> {
        Layer::PAINT_ORDER
            .iter()
            .copied()
            .map(move |layer| (layer, &self.0[layer.idx()]))
    }
}

impl<T> std::ops::Index<Layer> for PerLayer<T> {
    type Output = T;
    #[inline]
    fn index(&self, layer: Layer) -> &T {
        &self.0[layer.idx()]
    }
}

impl<T> std::ops::IndexMut<Layer> for PerLayer<T> {
    #[inline]
    fn index_mut(&mut self, layer: Layer) -> &mut T {
        &mut self.0[layer.idx()]
    }
}
