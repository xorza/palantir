//! [`LayerScope`] — the builder [`Ui::layer`] hands out.

use crate::layout::types::overlay::OverlayPosition;
use crate::layout::types::placement::Placement;
use crate::primitives::size::Size;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use glam::Vec2;

/// A side layer being configured, terminated by [`Self::show`].
///
/// [`Self::at`] fixes the body's top-left. [`Self::anchored`] resolves
/// the origin from the body's measured size instead — that is what lets
/// a popup flip above its anchor when it would not fit below, and it is
/// why `Popup`, `ContextMenu` and `Tooltip` place themselves rather than
/// taking a point from the caller. [`Self::max_size`] caps either one,
/// so no setter here depends on the order the others ran in. Setting
/// neither anchors at the surface origin with the whole surface
/// available, which is what every full-surface layer wants.
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

    /// Place the body's top-left at `anchor`. Without a
    /// [`Self::max_size`] the available extent runs from here to the
    /// surface's bottom-right.
    pub fn at(mut self, anchor: Vec2) -> Self {
        self.placement = self.placement.with_anchor(anchor);
        self
    }

    /// Cap the available extent at `size`, still clamped to the surface
    /// so an oversized cap can't bleed past the viewport. The root's own
    /// `Sizing` (Hug / Fill / Fixed) governs the painted size within it.
    pub fn max_size(mut self, size: impl Into<Size>) -> Self {
        self.placement = self.placement.with_size(size.into());
        self
    }

    /// Resolve the body's origin from its measured size against
    /// `position`'s anchor, flipping or shifting it to fit the surface.
    ///
    /// The form every anchored overlay wants — a dropdown under its
    /// trigger, a menu at the pointer, a tooltip beside the thing it
    /// describes. Replaces an origin set by [`Self::at`] and keeps a cap
    /// set by [`Self::max_size`].
    pub fn anchored(mut self, position: OverlayPosition) -> Self {
        self.placement = self.placement.with_overlay(position);
        self
    }

    /// Record `body` into the layer and hand back its value.
    ///
    /// Forwarding the value is load-bearing, not a convenience: an
    /// `input_scope` declared inside the body has to be read from inside
    /// it too, which is how an overlay records its capture against the
    /// layer it actually lives on.
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
