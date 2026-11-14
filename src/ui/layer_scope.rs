//! [`LayerScope`] — the builder [`Ui::layer`] hands out.

use crate::primitives::geometry::size::Size;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::placement::Placement;
use crate::primitives::math::domain::{self, vec2};
use crate::scene::layer::Layer;
use crate::ui::Ui;
use glam::Vec2;

/// A side layer being configured, terminated by [`Self::show`]. With neither [`Self::at`] nor
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

    /// Pins the body's top-left at `p`, where it stays though it overflows the surface;
    /// [`Self::anchor`] instead flips or shifts the body to fit.
    ///
    /// # Panics
    ///
    /// Panics unless both axes of `p` are [offsets](crate::widget::domain::is_offset).
    #[track_caller]
    pub fn at(mut self, p: impl Into<Vec2>) -> Self {
        self.placement = self.placement.with_fixed(vec2::offset(p.into()));
        self
    }

    /// Resolves the origin from the body's measured size against `anchor`, flipping or shifting to fit.
    pub const fn anchor(mut self, anchor: Anchor) -> Self {
        self.placement = self.placement.with_anchored(anchor);
        self
    }

    /// Caps the available extent at `s`, never past the room the origin leaves: the surface from a
    /// fixed point to its edge, or, for an anchor, the surface across its side and the room its
    /// roomier side leaves along it.
    ///
    /// # Panics
    ///
    /// Panics unless both axes are [extents](crate::widget::domain::is_extent).
    #[track_caller]
    pub fn max_size(mut self, s: impl Into<Size>) -> Self {
        let s = s.into();
        domain::extent(s.w);
        domain::extent(s.h);
        self.placement = self.placement.with_max_size(s);
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
