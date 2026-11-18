//! [`SegmentOption`]: one option of a segmented group, a glyph and a label centered on a click
//! target, lit as the pill covers it.

use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_slot::AnimationSlot;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::align::VAlign;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::text::text_input::TextInput;
use crate::text::wrap::TextWrap;
use crate::theme::look::WidgetLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::segmented::SegmentedTheme;
use crate::theme::selection::Selection;
use crate::ui::Ui;
use crate::widget_core::activation::Activation;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::label_glyph::Glyph;
use crate::widgets::radio_group::Pick;
use crate::widgets::text::Text;
use crate::widgets::toggle_row::{Settled, ToggleFace};
use std::rc::Rc;

/// The row a segment's resting ink animates in, so a hover moves its ink as it moves its fill.
const REST_INK: AnimationSlot = AnimationSlot::new("rest_ink");

/// One option of a segmented group: its cell of the track's grid, an equal share along its axis,
/// and a focusable click target over its glyph and label, centered. It wears
/// [`SegmentedTheme::segment`] over the pill, `selected` while checked, and its label's ink is its
/// resting ink blended toward its checked ink by the share of it the pill covers,
/// `1 − |pill − index|` clamped to `[0, 1]`.
#[derive(Debug)]
pub(super) struct SegmentOption<'o> {
    pub(super) widget: Widget,
    /// The text after the glyph; empty records no text child.
    pub(super) label: TextInput<'o>,
    slot: &'o SegmentedTheme,
    place: SegmentPlace,
}

/// Where a segment sits on its track: the track's axis, which its grid's cells are counted on,
/// the segment's index along it, and where the pill is, in segments from the first, `None` while
/// it draws none.
#[derive(Clone, Copy, Debug)]
pub(super) struct SegmentPlace {
    pub(super) axis: Axis,
    pub(super) idx: u32,
    pub(super) pill_at: Option<f32>,
}

impl<'o> SegmentOption<'o> {
    /// An option at `place` on a track in `slot`.
    #[track_caller]
    pub(super) fn new(slot: &'o SegmentedTheme, place: SegmentPlace) -> Self {
        let mut widget = Widget::hstack().sense(Sense::CLICK).focusable(true);
        widget
            .configure()
            .default_size(place.axis.compose_sizing(Sizing::FILL, Sizing::FILL));
        Self {
            widget,
            label: TextInput::default(),
            slot,
            place,
        }
    }

    /// Records the option with `glyph` before its label, settling `pick` on a click or Space.
    pub(super) fn show<T: PartialEq>(
        self,
        ui: &mut Ui,
        pick: Pick<'_, T>,
        glyph: impl Glyph,
    ) -> ValueResponse {
        let Self {
            mut widget,
            label,
            slot,
            place: SegmentPlace { axis, idx, pill_at },
        } = self;
        let response = widget.activation_response(ui, Activation::Toggle);
        let id = response.id;
        let Settled { checked, changed } = pick.settle(response.clicked());
        // An `Rc` bump, so the slot's ambient text outlives the `&mut Ui` the record takes.
        let theme = Rc::clone(ui.theme());
        let selection = Selection::from(checked);
        let plan = slot.segment.plan(&response, selection, theme.text);
        let mut look = widget.wear(ui, plan);
        let ink = |look: &WidgetLook| look.text.color.unwrap_or(theme.text.color);
        let rest = ink(slot.segment.look(&response, Selection::Unselected));
        let rest = ui
            .animate(
                AnimationRequest::new(id, REST_INK, rest),
                slot.segment.defaults.transition,
            )
            .value;
        let lit_ink = ink(slot.segment.look(&response, Selection::Selected));
        look.text.color = Lerp::lerp(&rest, &lit_ink, coverage(pill_at, idx, checked));
        let [row, col] = axis.rows_cols(idx, 0);
        let cell = |at: u32| u16::try_from(at).expect("a track holds under 2^16 options");
        widget
            .configure()
            .grid_cell(GridCell::at(cell(row), cell(col)))
            .default_gap(slot.gap)
            .default_justify(Justify::Center)
            .default_child_align(VAlign::Center);
        widget.record(ui, Some(&look.chrome), |ui| {
            glyph.record(ui, id.with("icon"), &look.text);
            if !label.is_empty() {
                Text::new(label)
                    .id(id.with("label"))
                    .style(&look.text)
                    .text_wrap(TextWrap::Truncate)
                    .show(ui);
            }
        });
        ValueResponse::pick(response, changed)
    }
}

/// The share of segment `idx` the pill covers, the pill at `pill_at` segments from the first:
/// `1 − |pill_at − idx|`, clamped to `[0, 1]`, as the pill and the segment are one share wide.
/// With no pill, the checked segment is whole and the rest bare.
pub(super) fn coverage(pill_at: Option<f32>, idx: u32, checked: bool) -> f32 {
    match pill_at {
        Some(at) => (1.0 - (at - idx as f32).abs()).clamp(0.0, 1.0),
        None => f32::from(checked),
    }
}
