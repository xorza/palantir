//! One recording arena's address: a [`Layer`] and the nesting depth inside it.

use crate::scene::layer::Layer;

/// One recording arena: a [`Layer`] and how deeply its overlays nest in it. An overlay
/// opened while its layer's plane has a node open records one plane up, so it paints above the
/// overlay that opened it, as HTML's top layer and egui's sublayers stack one above its opener.
///
/// Paint order is layer, then depth (the derived `Ord`); hit order is the reverse. A plane is a
/// depth, not one overlay root as egui's areas are, so its identity, which the retained caches key
/// on, stays put while other overlays open and close.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Plane {
    pub(crate) layer: Layer,
    pub(crate) depth: u8,
}

impl Plane {
    /// Ordinary content: the only plane outside every overlay.
    pub(crate) const MAIN: Self = Self::base(Layer::Main);

    /// The plane a top-level overlay of `layer` records into.
    pub(crate) const fn base(layer: Layer) -> Self {
        Self { layer, depth: 0 }
    }
}

/// What tests name a plane by: a `Layer` is its base plane.
#[cfg(test)]
pub(crate) mod internals {
    use crate::scene::layer::Layer;
    use crate::scene::plane::Plane;

    impl From<Layer> for Plane {
        fn from(layer: Layer) -> Self {
            Self::base(layer)
        }
    }
}
