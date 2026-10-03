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

/// What a child measured to, as its parent places it: the extent it
/// wants, and the floor it gives way to no further.
#[derive(Clone, Copy, Debug)]
pub(super) struct Placed {
    pub(super) desired: Size,
    pub(super) floor: Size,
}

impl Placed {
    /// A child that measured to `desired` over `floor`, placed.
    ///
    /// **Width in, height out.** A child's width is decided before it
    /// measures — its parent shares widths first, and its text shapes to
    /// the width it is given — so arrange never takes a width back: a
    /// child placed narrower than it measured would paint text shaped for
    /// the wider box. Its floor across is its width. Only a height, which
    /// is what a child measures *to*, gives way after measure.
    pub(super) const fn of(desired: Size, floor: Size) -> Self {
        Self {
            desired,
            floor: Size::new(desired.w, floor.h),
        }
    }

    /// This child with no give on the axes `rigid` sets: its floor there
    /// is what it measured to.
    pub(super) const fn rigid_on(self, rigid: BVec2) -> Self {
        Self {
            desired: self.desired,
            floor: self.desired.select(rigid, self.floor),
        }
    }
}

/// Per-axis placement: chosen extent + offset within the parent's inner span.
#[derive(Debug)]
pub(super) struct AxisPlacement {
    pub(super) size: f32,
    pub(super) offset: f32,
}

impl AxisPlacement {
    /// Resolve the outer extent and alignment offset for one arranged axis.
    /// `Fixed` always keeps its measured extent. `Fill` and explicit `Stretch`
    /// grow to their slot. In a slot smaller than what it measured to, a
    /// child gives way to the slot but not below its floor — see
    /// [`give_way_floor`]. The node's outer min/max bounds remain
    /// authoritative.
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
        // Through `offset_in`, the one definition of what an alignment
        // means as a number, floored at zero the way `Align::place_in`
        // floors it — oversized content pins to the leading edge rather
        // than backing out of the slot.
        let offset = align.offset_in(slot, size).max(0.0);
        Self { size, offset }
    }

    /// A child placed into `slot` on both axes: [`Self::arrange`] per axis
    /// under `align`, folded into the rect its parent hands
    /// `LayoutPass::arrange`.
    ///
    /// `slot` is the cell in the parent's own coordinates — a Grid's cell,
    /// a ZStack's whole inner rect — and the per-axis alignment offset moves
    /// the child inside it. The drivers differ only in the pair they pass:
    /// Grid stretches an `Auto` axis to the cell
    /// ([`AxisAlignPair::or_stretch_if_auto`]), ZStack pins it.
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

    /// Outer size of a node arranged into `slot` on both axes with no
    /// alignment — [`Self::arrange_rect`] under [`AxisAlignPair::AUTO`],
    /// keeping only the extents.
    ///
    /// The two callers that place a node without needing its alignment offset:
    /// `LayoutEngine::run` sizing a layer root against the surface, and
    /// `Canvas::arrange` sizing an absolutely-positioned child against its
    /// slot. Both position by other means (the root's `Placement`, the child's
    /// declared `pos`), so the offset the placement carries is dead to them.
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

    /// Cross-axis placement for a child of a main-axis stack (Stack /
    /// WrapStack): resolve the alignment cascade on the cross axis, then
    /// run [`Self::arrange`] against the child's cross sizing + desired +
    /// the parent's cross extent. Single source of truth so the cascade
    /// rule can't drift between Stack and WrapStack.
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

/// What a child gives way to in a slot smaller than what it measured
/// to: its floor — on its width the width itself, on its height its
/// measured floor (see [`Placed::of`]) — so content that cannot shrink
/// overflows its parent rather than its own rect, the contains-content
/// rule, while a stack, a grid or a scroll that can shrink takes the
/// slot and shares it among its own children at arrange.
///
/// A `Scroll` viewport gives way all the way, on both axes: it clips its
/// content, which is what it is for, so nothing it measured is a minimum
/// of the viewport's — a viewport larger than its slot would paint
/// outside the clip its own subtree is scissored to.
fn give_way_floor(child: &LayoutCore, floor: f32) -> f32 {
    match LayoutMode::from(child.meta) {
        LayoutMode::Scroll(_) => 0.0,
        _ => floor,
    }
}
