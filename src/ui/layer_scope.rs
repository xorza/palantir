//! [`LayerScope`] — the builder [`Ui::layer`] hands out.

use crate::primitives::geometry::size::Size;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::placement::Placement;
use crate::primitives::math::domain::{self, vec2};
use crate::scene::layer::Layer;
use crate::ui::Ui;
use glam::Vec2;

/// A side layer being configured, terminated by [`Self::show`]. With neither [`Self::fixed_at`] nor
/// [`Self::anchor`], the body sits at the surface origin with the whole surface available.
#[derive(Debug)]
#[must_use = "a layer records nothing until `show`"]
pub struct LayerScope<'a> {
    ui: &'a mut Ui,
    layer: Layer,
    placement: Placement,
}

impl<'a> LayerScope<'a> {
    pub(super) fn new(ui: &'a mut Ui, layer: Layer) -> Self {
        Self {
            ui,
            layer,
            placement: Placement::default(),
        }
    }

    /// Pins the body's top-left at `point`. [`Self::anchor`] instead keeps the body on screen.
    ///
    /// # Panics
    ///
    /// Panics unless both axes of `point` are [offsets](crate::widget::domain::offset).
    #[track_caller]
    pub const fn fixed_at(mut self, point: Vec2) -> Self {
        self.placement = self.placement.with_fixed(vec2::offset(point));
        self
    }

    /// Resolves the origin from the body's measured size against `anchor`, flipping or shifting to fit.
    pub const fn anchor(mut self, anchor: Anchor) -> Self {
        self.placement = self.placement.with_anchored(anchor);
        self
    }

    /// Caps the available extent at `size`, still clamped to the surface.
    ///
    /// # Panics
    ///
    /// Panics unless both axes are [extents](crate::widget::domain::extent).
    #[track_caller]
    pub fn max_size(mut self, size: impl Into<Size>) -> Self {
        let size = size.into();
        domain::extent(size.w);
        domain::extent(size.h);
        self.placement = self.placement.with_max_size(size);
        self
    }

    /// Records `body` into the layer and returns its value: an `input_scope` declared inside the
    /// body must be read from inside it.
    pub fn show<R>(self, body: impl FnOnce(&mut Ui) -> R) -> R {
        let Self {
            ui,
            layer,
            placement,
        } = self;
        ui.forest.push_layer(layer, placement);
        let result = body(ui);
        ui.forest.pop_layer();
        result
    }
}
