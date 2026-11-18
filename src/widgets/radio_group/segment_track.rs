//! [`SegmentTrack`]: a segmented group's track, its options and the pill that slides under the
//! checked one.

use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_slot::AnimationSlot;
use crate::animation::motion_role::MotionRole;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::paint::Paint;
use crate::scene::node::node_mode::NodeMode;
use crate::theme::segmented::SegmentedTheme;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use crate::widgets::radio_group::{GroupMemory, PillMemory};

/// The row the pill's place animates in, on the group's id.
const SLOT: AnimationSlot = AnimationSlot::new("pill");
/// The row the pill's fade animates in while motion is reduced.
const FADE: AnimationSlot = AnimationSlot::new("pill_fade");
/// Where the move count wraps, under 2^24 so every count is an exact `f32`; a wrap leaves one
/// fade unseen.
const MOVES_WRAP: u32 = 1 << 20;

/// A segmented group's track: a layer of the pill under a stack of the options, the track's chrome
/// and inset around both. The pill takes an equal share of the track at a fractional place, as
/// [`Sizing::split`] places a progress bar's fill: a spacer of weight `x`, the pill of weight 1
/// and a spacer of weight `count − 1 − x`, so the layout puts it in the pass that sizes the
/// segments, and a resize or a theme change never leaves it a frame behind. Refused: placing it
/// at last frame's option rects, a frame stale on a resize; a layout mode that places a node
/// between two siblings' rects, for one widget.
#[derive(Clone, Copy, Debug)]
pub(super) struct SegmentTrack<'s> {
    pub(super) id: WidgetId,
    pub(super) slot: &'s SegmentedTheme,
    pub(super) axis: Axis,
}

impl SegmentTrack<'_> {
    /// Records the track over `widget`, the group's node, with the options `options` records;
    /// `options` gets where the pill is, and returns whether a pick changed the value.
    ///
    /// The pill heads for the option last frame showed checked, as the options record after it,
    /// to paint over it. When this frame's options differ, by a pick or by a count, the frame
    /// records once more, the idiomatic answer for a parent that draws from what its children
    /// report, as a click asks a second pass already.
    pub(super) fn record(
        self,
        ui: &mut Ui,
        mut widget: Widget,
        memory: &mut GroupMemory,
        options: impl FnOnce(&mut Ui, &mut GroupMemory, Option<f32>) -> bool,
    ) -> bool {
        let Self { id, slot, axis } = self;
        let count = memory.disabled.len();
        let headed = memory.checked;
        let pill = headed.map(|target| self.pill(ui, &mut memory.pill, target, count));
        widget.node.mode = NodeMode::Resolved(LayoutMode::ZStack);
        widget.configure().default_padding(slot.track_padding);
        let changed = widget.record(ui, Some(&slot.track), |ui| {
            if let Some(Pill { at, chrome }) = &pill {
                let rest = (count - 1) as f32;
                let along = |weight: f32| axis.compose_sizing(Sizing::fill(weight), Sizing::FILL);
                Widget::stack(axis)
                    .id(id.with("pill_row"))
                    .size(axis.compose_sizing(Sizing::FILL, Sizing::FILL))
                    .record(ui, None, |ui| {
                        Widget::leaf()
                            .id(id.with("pill_before"))
                            .size(along(*at))
                            .record(ui, None, |_| {});
                        Widget::leaf().id(id.with("pill")).size(along(1.0)).record(
                            ui,
                            Some(chrome),
                            |_| {},
                        );
                        Widget::leaf()
                            .id(id.with("pill_after"))
                            .size(along(rest - at))
                            .record(ui, None, |_| {});
                    });
            }
            // One Fill track per option, so the options take equal shares even in a track that hugs
            // them, each the widest one's, as CSS Grid's `1fr` tracks size in an auto container.
            // At least one, as a grid that places cells has tracks; a count that moved records
            // the frame again below.
            memory.tracks.clear();
            memory.tracks.resize(count.max(1), Track::FILL);
            let [rows, cols] = axis.rows_cols(memory.tracks.as_slice(), &[Track::FILL][..]);
            let mut row = Widget::grid();
            row.configure()
                .id(id.with("options"))
                .size(axis.compose_sizing(Sizing::FILL, Sizing::FILL));
            row.grid_tracks(ui, rows, cols);
            row.record(ui, None, |ui| {
                options(ui, memory, pill.as_ref().map(|pill| pill.at))
            })
        });
        if memory.checked != headed || memory.disabled.len() != count {
            ui.request_relayout();
        }
        changed
    }

    /// The pill heading for segment `target` of `count`: its place, between the first segment and
    /// the last, and its chrome, faded in after a move while motion is reduced.
    pub(super) fn pill(
        self,
        ui: &mut Ui,
        memory: &mut PillMemory,
        target: u32,
        count: usize,
    ) -> Pill {
        let Self { id, slot, .. } = self;
        let last = (count.max(1) - 1) as f32;
        let at = ui
            .animate(AnimationRequest::new(id, SLOT, target as f32), slot.slide)
            .value
            .clamp(0.0, last);
        if memory.at.is_some_and(|at| at != target) {
            memory.moves = (memory.moves + 1) % MOVES_WRAP;
        }
        memory.at = Some(target);
        // Every frame, so the row runs on when motion turns reduced; read only while it is. It
        // heads one move on, so it starts a move behind and its distance to go is the fade.
        let moves = memory.moves as f32;
        let faded = ui
            .animate(
                AnimationRequest::new(id, FADE, moves).with_motion(MotionRole::Essential),
                slot.slide,
            )
            .value;
        let shown = if ui.reductions().motion.is_reduced() {
            (1.0 - (moves - faded)).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let chrome = if shown < 1.0 {
            let hidden = Chrome {
                corners: slot.pill.corners,
                paint: Paint::NONE,
            };
            Lerp::lerp(&hidden, &slot.pill, shown)
        } else {
            slot.pill.clone()
        };
        Pill { at, chrome }
    }
}

/// Where the pill draws this frame, in segments from the first, and in what.
#[derive(Debug)]
pub(super) struct Pill {
    pub(super) at: f32,
    pub(super) chrome: Chrome,
}
