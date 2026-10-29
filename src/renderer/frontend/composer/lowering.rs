//! Where a draw lands on the surface: the walk transform and the display, which turn a draw's
//! logical px into the physical px and whole pixels it covers.

use crate::display::Display;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::geometry::urect::URect;
use crate::primitives::paint::antialias;
use crate::renderer::frontend::composer::geometry;
use crate::renderer::frontend::composer::transform_stack::TransformStack;
use crate::renderer::frontend::payload::spin::Spin;
use glam::{UVec2, Vec2};

/// The walk transform and the display one compose session lowers its draws under.
#[derive(Clone, Copy, Debug)]
pub(super) struct Lowering<'a> {
    pub(super) transform: &'a TransformStack,
    pub(super) display: &'a Display,
}

/// A rect in physical px, and the whole pixels it reaches.
#[derive(Debug)]
pub(super) struct ScaledRect {
    pub(super) physical: Rect,
    pub(super) urect: URect,
}

impl ScaledRect {
    pub(super) fn from_physical(physical: Rect, viewport: UVec2) -> Self {
        Self {
            physical,
            urect: geometry::urect_from_physical(physical.min, physical.max(), viewport),
        }
    }
}

impl Lowering<'_> {
    /// Apply the walk transform to a logical rect and scale it to physical px with integer bounds,
    /// once, for cull and instance alike.
    pub(super) fn scaled_rect(&self, rect: Rect) -> ScaledRect {
        self.scaled_rect_with(rect, self.display.edge_rounding())
    }

    pub(super) fn scaled_rect_with(&self, rect: Rect, edges: EdgeRounding) -> ScaledRect {
        let world = self.transform.apply_rect(rect);
        let physical = world.scaled_by(self.display.scale_factor(), edges);
        ScaledRect::from_physical(physical, self.display.physical)
    }

    /// `spin`, its pivot in the encoder's logical frame, about the physical pivot it maps to.
    pub(super) fn physical_spin(&self, spin: Spin) -> Spin {
        Spin {
            pivot: self.transform.physical_map(Vec2::ZERO).point(spin.pivot),
            angle: spin.angle,
        }
    }

    /// `physical` moved so its center is turned by `spin` about the physical pivot, which a
    /// rotation by `spin.angle` about that center then completes: together a turn of the whole
    /// rect about the pivot. Its pixels are those the shader shades:
    /// [`antialias::turned_reach`] of it, as the occlusion pruner reads them.
    pub(super) fn turned_rect(&self, physical: Rect, spin: Spin) -> ScaledRect {
        let physical_spin = self.physical_spin(spin);
        let center = physical.center();
        let moved = Rect {
            min: physical.min + physical_spin.rotor().apply(center) - center,
            size: physical.size,
        };
        let reach = antialias::turned_reach(moved, spin.angle);
        ScaledRect {
            physical: moved,
            urect: geometry::urect_from_physical(reach.min, reach.max(), self.display.physical),
        }
    }
}
