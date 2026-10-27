//! One value per [`Plane`].

use crate::scene::layer::Layer;
use crate::scene::plane::Plane;
use std::array;
use std::iter;
use std::ops;
use std::slice;

/// One `T` per [`Plane`]: each [`Layer`] holds its depths, the base plane always.
///
/// `Index<Plane>`/`IndexMut<Plane>` for a known plane, [`Self::iter`]/[`Self::iter_mut`] when the
/// plane doesn't matter, [`Self::iter_paint_order`] when it does. Planes only grow, so a
/// retained table keeps its slots and a steady frame allocates nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PerPlane<T> {
    layers: [Vec<T>; Layer::COUNT],
}

impl<T: Default> Default for PerPlane<T> {
    fn default() -> Self {
        Self {
            layers: array::from_fn(|_| vec![T::default()]),
        }
    }
}

impl<T> PerPlane<T> {
    /// Every plane's slot, order unspecified.
    pub(crate) fn iter(&self) -> iter::Flatten<slice::Iter<'_, Vec<T>>> {
        self.layers.iter().flatten()
    }

    /// [`Self::iter`], mutably.
    pub(crate) fn iter_mut(&mut self) -> iter::Flatten<slice::IterMut<'_, Vec<T>>> {
        self.layers.iter_mut().flatten()
    }

    /// Every plane this table holds, bottom-up; reverse for topmost-first hit-test.
    pub(crate) fn planes(&self) -> impl Iterator<Item = Plane> + '_ {
        Layer::PAINT_ORDER.into_iter().flat_map(move |layer| {
            (0..self.layers[layer.idx()].len()).map(move |depth| Plane {
                layer,
                depth: depth as u8,
            })
        })
    }

    /// `(Plane, &T)` bottom-up; reverse for topmost-first hit-test.
    pub(crate) fn iter_paint_order(&self) -> impl Iterator<Item = (Plane, &T)> {
        self.planes().map(move |plane| (plane, &self[plane]))
    }
}

impl<T: Default> PerPlane<T> {
    /// Grow to hold `plane` and every depth below it in its layer.
    pub(crate) fn ensure(&mut self, plane: Plane) {
        let layer = &mut self.layers[plane.layer.idx()];
        let len = usize::from(plane.depth) + 1;
        if layer.len() < len {
            layer.resize_with(len, T::default);
        }
    }

    /// Grow to hold every plane `other` holds, so a table derived from it indexes the same planes.
    pub(crate) fn cover<U>(&mut self, other: &PerPlane<U>) {
        for (layer, theirs) in self.layers.iter_mut().zip(&other.layers) {
            if layer.len() < theirs.len() {
                layer.resize_with(theirs.len(), T::default);
            }
        }
    }
}

impl<'a, T> IntoIterator for &'a PerPlane<T> {
    type Item = &'a T;
    type IntoIter = iter::Flatten<slice::Iter<'a, Vec<T>>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut PerPlane<T> {
    type Item = &'a mut T;
    type IntoIter = iter::Flatten<slice::IterMut<'a, Vec<T>>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<T> ops::Index<Plane> for PerPlane<T> {
    type Output = T;
    #[inline]
    fn index(&self, plane: Plane) -> &T {
        &self.layers[plane.layer.idx()][usize::from(plane.depth)]
    }
}

impl<T> ops::IndexMut<Plane> for PerPlane<T> {
    #[inline]
    fn index_mut(&mut self, plane: Plane) -> &mut T {
        &mut self.layers[plane.layer.idx()][usize::from(plane.depth)]
    }
}

#[cfg(test)]
mod tests {
    use crate::scene::layer::Layer;
    use crate::scene::per_plane::PerPlane;
    use crate::scene::plane::Plane;

    const fn plane(layer: Layer, depth: u8) -> Plane {
        Plane { layer, depth }
    }

    /// A fresh table holds each layer's base plane; `ensure` grows a layer to a depth and every
    /// depth below it, `cover` grows to another table's planes, and neither shrinks. Planes list
    /// layer, then depth: paint order, which is also `Plane`'s order.
    #[test]
    fn planes_grow_per_layer_and_list_in_paint_order() {
        let mut table = PerPlane::<u32>::default();
        assert_eq!(
            table.planes().collect::<Vec<_>>(),
            Layer::PAINT_ORDER.map(Plane::base),
        );

        table.ensure(plane(Layer::Modal, 2));
        table.ensure(plane(Layer::Modal, 1));
        table[plane(Layer::Modal, 2)] = 7;
        let planes: Vec<Plane> = table.planes().collect();
        assert_eq!(
            planes,
            [
                plane(Layer::Main, 0),
                plane(Layer::Popup, 0),
                plane(Layer::Modal, 0),
                plane(Layer::Modal, 1),
                plane(Layer::Modal, 2),
                plane(Layer::Menu, 0),
                plane(Layer::Tooltip, 0),
                plane(Layer::Debug, 0),
            ],
        );
        assert!(planes.is_sorted(), "paint order is `Plane`'s order");
        assert_eq!(table.iter().count(), 8);
        assert_eq!(
            table
                .iter_paint_order()
                .map(|(_, v)| *v)
                .collect::<Vec<_>>(),
            [0, 0, 0, 0, 7, 0, 0, 0],
        );

        let mut derived = PerPlane::<String>::default();
        derived.ensure(plane(Layer::Popup, 1));
        derived.cover(&table);
        assert_eq!(
            derived.planes().collect::<Vec<_>>(),
            [
                plane(Layer::Main, 0),
                plane(Layer::Popup, 0),
                plane(Layer::Popup, 1),
                plane(Layer::Modal, 0),
                plane(Layer::Modal, 1),
                plane(Layer::Modal, 2),
                plane(Layer::Menu, 0),
                plane(Layer::Tooltip, 0),
                plane(Layer::Debug, 0),
            ],
            "cover keeps its own deeper planes and takes the other's",
        );
    }
}
