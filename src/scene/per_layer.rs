//! One value per [`Layer`].

use crate::scene::layer::Layer;
use std::array;
use std::ops;
use std::slice;
use strum::EnumCount as _;

/// Fixed-size `[T; Layer::COUNT]` indexed by [`Layer`].
///
/// Three ways in, one per question the caller is asking: `Index<Layer>`
/// / `IndexMut<Layer>` for a known layer, [`Self::iter`] /
/// [`Self::iter_mut`] when the layer doesn't matter, and
/// [`Self::iter_paint_order`] when it does. The backing array is private
/// so those stay the only spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub(crate) fn iter(&self) -> slice::Iter<'_, T> {
        self.0.iter()
    }

    pub(crate) fn iter_mut(&mut self) -> slice::IterMut<'_, T> {
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

impl<T> ops::Index<Layer> for PerLayer<T> {
    type Output = T;
    #[inline]
    fn index(&self, layer: Layer) -> &T {
        &self.0[layer.idx()]
    }
}

impl<T> ops::IndexMut<Layer> for PerLayer<T> {
    #[inline]
    fn index_mut(&mut self, layer: Layer) -> &mut T {
        &mut self.0[layer.idx()]
    }
}
