//! One axis's arranged extent and its alignment offset.

use crate::layout::axis_align_pair::AxisAlignPair;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::{Align, AxisAlign};
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::layout_core::LayoutCore;
use glam::{BVec2, Vec2};

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

#[derive(Debug)]
pub(super) struct AxisPlacement {
    pub(super) size: f32,
    pub(super) offset: f32,
}

impl AxisPlacement {
    /// Resolves the outer extent and alignment offset for one axis. `Fixed`
    /// keeps its measured extent; `Fill` and `Stretch` grow to the slot. In a
    /// smaller slot a child gives way, not below its floor ([`give_way_floor`]).
    pub(super) fn arrange(
        axis: Axis,
        align: AxisAlign,
        child: &LayoutCore,
        bounds: &BoundsExtras,
        measured: Placed,
        slot: f32,
    ) -> Self {
        let margin = axis.spacing(child.margin);
        let min = axis.main(bounds.min_size) + margin;
        let max = axis.main(bounds.max_size) + margin;
        let floor = give_way_floor(child, axis.main(measured.floor));
        let sizing = axis.main_sizing(child.size);
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
        Self { size, offset }
    }

    /// A child placed into `slot` on both axes under `align`. Grid stretches an
    /// `Auto` axis to the cell ([`AxisAlignPair::or_stretch_if_auto`]), ZStack pins it.
    pub(super) fn arrange_rect(
        align: AxisAlignPair,
        child: &LayoutCore,
        bounds: &BoundsExtras,
        measured: Placed,
        slot: Rect,
    ) -> Rect {
        let x = Self::arrange(Axis::X, align.h, child, bounds, measured, slot.size.w);
        let y = Self::arrange(Axis::Y, align.v, child, bounds, measured, slot.size.h);
        Rect {
            min: slot.min + Vec2::new(x.offset, y.offset),
            size: Size::new(x.size, y.size),
        }
    }

    /// Outer size arranged into `slot` with no alignment, for callers that place by other means.
    pub(super) fn arrange_size(
        child: &LayoutCore,
        bounds: &BoundsExtras,
        measured: Placed,
        slot: Size,
    ) -> Size {
        Self::arrange_rect(
            AxisAlignPair::AUTO,
            child,
            bounds,
            measured,
            Rect {
                min: Vec2::ZERO,
                size: slot,
            },
        )
        .size
    }

    /// Cross-axis placement for a child of a main-axis stack, one source for Stack and WrapStack.
    pub(super) fn cross(
        main_axis: Axis,
        child: &LayoutCore,
        bounds: &BoundsExtras,
        parent_child_align: Align,
        measured: Placed,
        inner_cross: f32,
    ) -> Self {
        let cross_axis = main_axis.other();
        let cross_align = AxisAlignPair::resolve_axis(cross_axis, child, parent_child_align);
        Self::arrange(
            cross_axis,
            cross_align,
            child,
            bounds,
            measured,
            inner_cross,
        )
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
