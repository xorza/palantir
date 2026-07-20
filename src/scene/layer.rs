//! Layer ordering and fixed per-layer storage.

use std::array;
use strum::EnumCount as _;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, strum::EnumCount)]
pub enum Layer {
    #[default]
    Main = 0,
    Popup = 1,
    Modal = 2,
    Tooltip = 3,
    Debug = 4,
}

impl Layer {
    pub(crate) const PAINT_ORDER: [Layer; <Layer as strum::EnumCount>::COUNT] = [
        Layer::Main,
        Layer::Popup,
        Layer::Modal,
        Layer::Tooltip,
        Layer::Debug,
    ];

    #[inline]
    pub(crate) const fn idx(self) -> usize {
        self as usize
    }
}

const _: () = {
    let mut i = 0;
    while i < Layer::PAINT_ORDER.len() {
        assert!(
            Layer::PAINT_ORDER[i] as usize == i,
            "Layer::PAINT_ORDER must match the discriminant order",
        );
        i += 1;
    }
};

/// Fixed-size `[T; Layer::COUNT]` indexed by [`Layer`]. Implements
/// `Index<Layer>` / `IndexMut<Layer>` for the natural sugar,
/// `IntoIterator` for `&` and `&mut` so `for t in &per` works, plus
/// [`Self::iter_paint_order`] for layer-tagged iteration. Order-blind
/// slice access goes through the `pub(crate)` `.0` array directly.
#[derive(Debug)]
#[repr(transparent)]
pub(crate) struct PerLayer<T>(pub(crate) [T; Layer::COUNT]);

impl<T: Default> Default for PerLayer<T> {
    fn default() -> Self {
        Self(array::from_fn(|_| T::default()))
    }
}

impl<T> PerLayer<T> {
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
