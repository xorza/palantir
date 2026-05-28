use crate::forest::element::{Configure, Element, LayoutMode, Salt};
use crate::input::ResponseState;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::interned_str::InternedStr;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::text::Text;
use crate::widgets::theme::widget_look::AnimatedLook;

/// Shared `HStack [box, label]` scaffolding behind [`crate::Checkbox`]
/// and [`crate::RadioButton`] — the two differ only in the toggle
/// semantics (resolved by the caller before this runs), the indicator
/// glyph (`paint_indicator`), and the box's corner radius (baked into
/// `look.background.corners` by the caller, so the radio can force a
/// pill). Everything structural — the row gap / cross-centering, the
/// `Fixed×Fixed` box leaf with its chrome, the label leaf — lives here.
///
/// `element` is the row's `HStack` (sense + salt already set), `id` its
/// resolved [`WidgetId`], and `raw_state` the un-merged response handed
/// back to the caller via [`Response::eager`]. `paint_indicator` runs
/// inside the box leaf and is responsible for its own checked/selected
/// gate.
#[allow(clippy::too_many_arguments)]
pub(crate) fn toggle_row(
    ui: &mut Ui,
    id: WidgetId,
    mut element: Element,
    raw_state: ResponseState,
    look: AnimatedLook,
    box_size: f32,
    row_gap: f32,
    label: InternedStr,
    paint_indicator: impl FnOnce(&mut Ui),
) -> Response<'_> {
    element.gaps.set_gap(row_gap);
    element.child_align = Align::v(VAlign::Center);

    ui.node(id, element, |ui| {
        let box_id = id.with("box");
        let mut box_elem = Element::new(LayoutMode::Leaf);
        box_elem.salt = Salt::Verbatim(box_id);
        box_elem.size = (Sizing::Fixed(box_size), Sizing::Fixed(box_size)).into();
        ui.node_with_chrome(box_id, box_elem, &look.background, paint_indicator);

        if !label.is_empty() {
            Text::new(label)
                .id(id.with("label"))
                .style(look.text)
                .text_align(Align::v(VAlign::Center))
                .show(ui);
        }
    });

    Response::eager(id, ui, raw_state)
}
