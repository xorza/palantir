//! WrapStack driver: HStack/VStack with overflow wrap. Children flow on the
//! main axis and wrap to a new line when the next would not fit the remaining
//! budget. Cross extent = sum of line cross-extents plus line gaps.
//!
//! `gap` is within-line spacing; `line_gap` is between lines. Main-axis
//! `Sizing::fill` is treated as `Hug`: consuming row leftover would need a
//! per-line distribution this driver does not define. Cross-axis Fill works as
//! in Stack: each line's cross size is the max child cross, and Fill children
//! grow to it without shrinking below their measured content.

use crate::layout::depth_scratch::DepthScratch;
use crate::layout::drivers::LayoutDriver;
use crate::layout::intrinsic::intrinsic_pass::IntrinsicPass;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::justify_offsets::{JustifyOffsets, JustifyRow};
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::{Axis, AxisExtent};
use crate::primitives::math::num::F32Px;
use crate::scene::tree::node_id::NodeId;

/// One child's measured contribution to the current line.
#[derive(Clone, Copy, Debug)]
struct ChildPack {
    main: f32,
    cross: f32,
}

impl ChildPack {
    #[inline]
    const fn on(axis: Axis, d: Size) -> Self {
        Self {
            main: axis.main(d),
            cross: axis.cross(d),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct LinePack {
    main: f32,
    cross: f32,
    occupied: bool,
}

/// The line being filled, and what it breaks against. The wrap decision and
/// line-extent arithmetic live here so measure and arrange cannot drift.
#[derive(Debug)]
struct LinePacker {
    line: LinePack,
    gap: f32,
    /// The main-axis budget a line breaks against, on the whole-pixel grid the
    /// measure cache keys `available_q` on.
    ///
    /// Where a line breaks is this driver's one discontinuous output: a quarter
    /// pixel decides between one line and two. The cache restores a subtree under
    /// a whole-pixel key, so the break must use the same grid or a warm frame and
    /// a cold one disagree for the same surface. Text wrapping pays the same
    /// through [`F32Px::canonical_px`].
    ///
    /// Read once per node by measure and arrange alike so they break identically.
    /// Justify keeps the raw extent: leftover space is continuous.
    budget: f32,
}

impl LinePacker {
    #[inline]
    fn new(axis: Axis, gap: f32, inner: Size) -> Self {
        Self {
            line: LinePack::default(),
            gap,
            budget: axis.main(inner).canonical_px(),
        }
    }

    /// True iff appending a child would push the line past [`Self::budget`]. The
    /// first child on an empty line never wraps.
    ///
    /// The extent is quantized like the budget: a Hug stack is arranged at its
    /// widest line, which rounds to its budget, so a raw 200.4 against 200 would
    /// break in arrange where measure kept it whole.
    #[inline]
    fn would_wrap(&self, child_main: f32) -> bool {
        self.line.occupied && (self.line.main + self.gap + child_main).canonical_px() > self.budget
    }

    /// Advance the line by one child. On a wrap, `complete_line(line_main,
    /// line_cross)` runs for the finished line and a fresh line starts with this
    /// child.
    #[inline]
    fn push(&mut self, pack: ChildPack, mut complete_line: impl FnMut(f32, f32)) {
        let ChildPack { main, cross } = pack;
        if self.would_wrap(main) {
            complete_line(self.line.main, self.line.cross);
            self.line = LinePack {
                main,
                cross,
                occupied: true,
            };
        } else {
            let line = &mut self.line;
            if line.occupied {
                line.main += self.gap;
            }
            line.main += main;
            line.cross = line.cross.max(cross);
            line.occupied = true;
        }
    }
}

/// The wrapping stack's line buffer: the children of the line being filled.
pub(crate) type WrapScratch = DepthScratch<NodeId>;

#[derive(Clone, Copy, Debug)]
pub(super) struct WrapStack {
    node: NodeId,
    axis: Axis,
}

impl WrapStack {
    pub(super) const fn new(node: NodeId, axis: Axis) -> Self {
        Self { node, axis }
    }
}

impl LayoutDriver for WrapStack {
    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = true;

    /// Pack children into lines; return (max-line-main, sum line-cross plus
    /// line-gaps). Arrange repeats the packing on the same `desired` values,
    /// so both passes agree.
    fn measure(self, pass: &mut LayoutPass<'_>, inner_avail: Size) -> Measured {
        let Self { node, axis } = self;
        let tree = pass.tree;
        let panel = tree.panel(node);
        let line_gap = panel.gaps.line_gap();
        let cross_avail = axis.cross(inner_avail);

        // Each non-collapsed child is measured once, with `INF` on main and
        // the committed cross (as Stack pass-1), so wrap text shapes against
        // `cross_avail`.
        let mut max_line_main = 0.0f32;
        let mut total_cross = 0.0f32;
        let mut packer = LinePacker::new(axis, panel.gaps.gap(), inner_avail);
        let mut line_count = 0usize;

        let mut complete_line = |line_main: f32, line_cross: f32| {
            max_line_main = max_line_main.max(line_main);
            total_cross += line_cross;
            line_count += 1;
        };
        // Every child is offered an unbounded main, so only the cross carries ranges.
        let mut stable_cross = 0.0f32;
        for c in tree.active_children(node) {
            let d = pass.measure(c, axis.compose_size(f32::INFINITY, cross_avail));
            stable_cross = stable_cross.max(axis.cross(d.stable_from));
            packer.push(ChildPack::on(axis, d.size), &mut complete_line);
        }
        if packer.line.occupied {
            complete_line(packer.line.main, packer.line.cross);
        }
        total_cross += line_gap.gaps_between(line_count);
        // One line stays one line under every budget past its length; a break
        // was taken at this budget and moves with it.
        let stable_main = if line_count <= 1 {
            max_line_main.min(axis.main(inner_avail))
        } else {
            Measured::AT_OFFER_ONLY
        };

        // Arrange packs again against the extent it is handed, so a wrap stack
        // placed smaller than it measured would break elsewhere: it gives way
        // to nothing on either axis.
        let size = axis.compose_size(max_line_main, total_cross);
        Measured {
            size,
            floor: size,
            stable_from: axis.compose_size(stable_main, stable_cross),
        }
    }

    fn arrange(self, pass: &mut LayoutPass<'_>, inner: Size) {
        let Self { node, axis } = self;
        let tree = pass.tree;
        let panel = tree.panel(node);
        let gap = panel.gaps.gap();
        let line_gap = panel.gaps.line_gap();
        let justify = panel.justify;
        let parent_child_align = panel.child_align;
        // Lines break against the budget, so measure, arrange and the cache
        // agree; justify hands out the continuous leftover of the arranged
        // rect.
        let main_avail = axis.main(inner);
        let mut packer = LinePacker::new(axis, gap, inner);

        // Same packing as `measure`, with lookahead: a child cannot be placed
        // until the row's `line_main` (justify) and `line_cross` are known. The
        // row's node IDs buffer in the wrap scratch at this depth and flush on
        // overflow or end of children.
        let line_start = pass.wrap_scratch_mut().mark();
        let mut cross_cursor = 0.0;
        let mut first_line = true;

        let place_line = |pass: &mut LayoutPass<'_>,
                          line_main: f32,
                          line_cross: f32,
                          cross_cursor: &mut f32,
                          first_line: &mut bool| {
            let line_end = pass.wrap_scratch_mut().mark();
            if line_end == line_start {
                return;
            }
            if !*first_line {
                *cross_cursor += line_gap;
            }
            *first_line = false;

            let count = line_end - line_start;
            let leftover = (main_avail - line_main).max(0.0);
            let JustifyOffsets {
                start: start_offset,
                gap: eff_gap,
            } = JustifyOffsets::new(
                justify,
                JustifyRow {
                    leftover,
                    gap,
                    count,
                },
            );
            let mut main_cursor = start_offset;
            // Index loop: copy each `NodeId` out, as `pass.arrange` needs `&mut`.
            for i in line_start..line_end {
                let c = pass.wrap_scratch_mut().at(i);
                if i > line_start {
                    main_cursor += eff_gap;
                }
                let main_size = axis.main(pass.desired(c));
                // Cross axis within the line's extent, as Stack: Fill stretches
                // to line_cross, Hug aligns per child.
                let cross_p = pass.placed(c).cross(axis, parent_child_align, line_cross);
                let child_rect = axis.compose_rect(
                    AxisExtent::new(main_cursor, main_size),
                    AxisExtent::new(*cross_cursor + cross_p.offset, cross_p.size),
                );
                pass.arrange(c, child_rect);
                main_cursor += main_size;
            }
            *cross_cursor += line_cross;
            // Drop our line from the pool. Recursive arranges may have extended
            // and re-truncated it past `line_end`; reset to our depth's start.
            pass.wrap_scratch_mut().truncate(line_start);
        };

        // Collapsed children are zeroed at the cursor; active ones pack and
        // flush on overflow.
        for child in tree.children(node) {
            let c = child.id;
            if child.visibility.is_collapsed() {
                // Anchored at the cursor with zero size: no visual or input contribution.
                pass.zero_subtree(c, axis.compose_point(0.0, cross_cursor));
                continue;
            }

            let pack = ChildPack::on(axis, pass.desired(c));
            // On wrap, `push` places the finished line (emptying the pool to this
            // depth's start); the triggering child then starts the next.
            packer.push(pack, |line_main, line_cross| {
                place_line(
                    pass,
                    line_main,
                    line_cross,
                    &mut cross_cursor,
                    &mut first_line,
                );
            });
            pass.wrap_scratch_mut().push(c);
        }
        if packer.line.occupied {
            place_line(
                pass,
                packer.line.main,
                packer.line.cross,
                &mut cross_cursor,
                &mut first_line,
            );
        }
    }

    /// Intrinsic size on the query's axis. Main-axis answers are exact;
    /// cross-axis answers approximate a single line (the conservative
    /// max-content shape) since the full packing is not run.
    ///
    /// - **MinContent** on main: max child intrinsic (the widest child sets
    ///   the floor).
    /// - **MaxContent** on main: sum plus within-line gaps.
    /// - Cross axis: max child intrinsic.
    fn intrinsic(self, pass: &mut IntrinsicPass<'_>, query: IntrinsicQuery) -> IntrinsicRange {
        if self.axis != query.axis() {
            // Real wrapped cross depends on the resolved main width, which is
            // not computed here; conservative for toolbar/badge use.
            return pass.children_max_at_origin(self.node, query);
        }
        let tree = pass.tree();
        let mut range = IntrinsicRange::ZERO;
        let mut count = 0_usize;
        for c in tree.active_children(self.node) {
            let child = pass.query(c, query);
            for (req, slot) in range.requested(query) {
                *slot = match req {
                    // The widest single child is the floor.
                    LenReq::MinContent => slot.max(child.min),
                    // Everything on one line.
                    LenReq::MaxContent => *slot + child.max,
                };
            }
            count += 1;
        }
        // Within-line gaps exist only on the single-line (max) reading.
        if query.includes(LenReq::MaxContent) {
            range.max += tree.panel(self.node).gaps.gap().gaps_between(count);
        }
        range
    }
}

#[cfg(test)]
mod tests;
