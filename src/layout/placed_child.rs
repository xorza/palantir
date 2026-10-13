//! A child as its parent places it: the extent and alignment offset a slot gives it.

use crate::layout::axis_align_pair::AxisAlignPair;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::{Align, AxisAlign};
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::layout_core::LayoutCore;
use glam::{BVec2, Vec2};

/// What a child measured to, as a parent places it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Placed {
    pub(super) desired: Size,
    pub(super) floor: Size,
}

impl Placed {
    /// A child that measured to `desired` over `floor`, placed. **Width in,
    /// height out:** a child's width is decided before it measures, so arrange
    /// never takes a width back or text would paint shaped for a wider box.
    /// Only a height gives way after measure.
    pub(super) const fn of(desired: Size, floor: Size) -> Self {
        Self {
            desired,
            floor: Size::new(desired.w, floor.h),
        }
    }

    pub(super) const fn rigid_on(self, rigid: BVec2) -> Self {
        Self {
            desired: self.desired,
            floor: self.desired.select(rigid, self.floor),
        }
    }
}

/// A child about to be placed: its box and what it measured to.
#[derive(Clone, Copy, Debug)]
pub(super) struct PlacedChild<'a> {
    pub(super) layout: &'a LayoutCore,
    pub(super) bounds: &'a BoundsExtras,
    pub(super) measured: Placed,
}

/// One axis's arranged extent and its alignment offset.
#[derive(Debug)]
pub(super) struct AxisPlacement {
    pub(super) size: f32,
    pub(super) offset: f32,
}

impl PlacedChild<'_> {
    /// This child with no give on the axes `rigid` sets.
    pub(super) const fn rigid_on(self, rigid: BVec2) -> Self {
        Self {
            measured: self.measured.rigid_on(rigid),
            ..self
        }
    }

    /// Resolves the outer extent and alignment offset for one axis. `Fixed`
    /// keeps its measured extent; `Fill` and `Stretch` grow to the slot. In a
    /// smaller slot a child gives way, not below its floor ([`give_way_floor`]).
    pub(super) fn arrange(self, axis: Axis, align: AxisAlign, slot: f32) -> AxisPlacement {
        let Self {
            layout,
            bounds,
            measured,
        } = self;
        let margin = axis.spacing(layout.margin);
        let min = axis.main(bounds.min_size) + margin;
        let max = axis.main(bounds.max_size) + margin;
        let floor = give_way_floor(layout, axis.main(measured.floor));
        let sizing = axis.main_sizing(layout.size);
        let stretch = sizing.fill_weight().is_some()
            || matches!(align, AxisAlign::Stretch) && sizing.fixed_value().is_none();
        let size = if stretch {
            slot.max(floor)
        } else {
            axis.main(measured.desired).min(slot.max(floor))
        }
        .clamp(min, max);
        // Floored at zero like `Align::place_in`: oversized content pins to the leading edge.
        let offset = align.offset_in(slot, size).max(0.0);
        AxisPlacement { size, offset }
    }

    /// Placed into `slot` on both axes under `align`. Grid stretches an `Auto`
    /// axis to the cell ([`AxisAlignPair::or_stretch_if_auto`]), ZStack pins it.
    pub(super) fn arrange_rect(self, align: AxisAlignPair, slot: Rect) -> Rect {
        let x = self.arrange(Axis::X, align.h, slot.size.w);
        let y = self.arrange(Axis::Y, align.v, slot.size.h);
        Rect {
            min: slot.min + Vec2::new(x.offset, y.offset),
            size: Size::new(x.size, y.size),
        }
    }

    /// Outer size arranged into `slot` with no alignment, for callers that place by other means.
    pub(super) fn arrange_size(self, slot: Size) -> Size {
        self.arrange_rect(
            AxisAlignPair::AUTO,
            Rect {
                min: Vec2::ZERO,
                size: slot,
            },
        )
        .size
    }

    /// Cross-axis placement in a main-axis stack, one source for Stack and WrapStack.
    pub(super) fn cross(
        self,
        main_axis: Axis,
        parent_child_align: Align,
        inner_cross: f32,
    ) -> AxisPlacement {
        let cross_axis = main_axis.other();
        let cross_align = AxisAlignPair::resolve_axis(cross_axis, self.layout, parent_child_align);
        self.arrange(cross_axis, cross_align, inner_cross)
    }
}

/// What a child gives way to in a slot smaller than it measured to: its floor
/// (see [`Placed::of`]). Content that cannot shrink overflows its parent; a
/// stack, grid or scroll that can shrink shares the slot. A `Scroll` viewport
/// gives way fully on both axes, since it clips.
fn give_way_floor(child: &LayoutCore, floor: f32) -> f32 {
    match LayoutMode::from(child.meta) {
        LayoutMode::Scroll(_) => 0.0,
        _ => floor,
    }
}
