//! `PerLayer<T>` — fixed-size `[T; Layer::COUNT]` keyed by [`Layer`]
//! directly so call sites don't sprinkle `.idx()`. Used by `Forest`
//! (trees), `Layout` (per-layer layout columns), and `Cascades`
//! (per-layer cascade rows + paint arenas).

use crate::forest::Layer;
use std::array;
use strum::EnumCount as _;

/// Fixed-size `[T; Layer::COUNT]` indexed by [`Layer`]. Implements
/// `Index<Layer>` / `IndexMut<Layer>` for the natural sugar,
/// `IntoIterator` for `&` and `&mut` so `for t in &per` works, plus
/// the project's two common iteration shapes (`iter_paint_order` and
/// the bare `iter`; mutable iteration goes through `&mut PerLayer`).
#[derive(Debug)]
#[repr(transparent)]
pub(crate) struct PerLayer<T>(pub(crate) [T; Layer::COUNT]);

impl<T: Default> Default for PerLayer<T> {
    fn default() -> Self {
        Self(array::from_fn(|_| T::default()))
    }
}

impl<T> PerLayer<T> {
    #[inline]
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
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

impl<'a, T> IntoIterator for &'a PerLayer<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut PerLayer<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}
