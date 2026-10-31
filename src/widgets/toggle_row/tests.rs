use crate::host::harness::Harness;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::radio::RadioButton;
use crate::widgets::switch::Switch;
use glam::{UVec2, Vec2};

/// Each toggle lays its row out from its own slot: `padding`, `margin` and `gap` reach the row,
/// and the box centers on the cross axis; an explicit builder value wins each.
///
/// Spacing is asymmetric and distinct per toggle, so no lane, axis or slot swap passes. The row is
/// 120 tall, so a centered box `h` tall sits `(120 - pad.top - pad.bottom - h) / 2` below the
/// padding, and a top-aligned one at the padding.
#[test]
fn theme_spacing_reaches_every_toggle_row_and_explicit_wins() {
    let spacing = |n: f32| {
        (
            Spacing::xy(n, n + 2.0),
            Spacing::xy(n + 4.0, n + 6.0),
            n + 8.0,
        )
    };
    let mut h = Harness::mono(UVec2::new(400, 300));
    let theme = h.ui().theme_mut();
    let kinds = [
        (Toggle::Checkbox, &mut theme.checkbox, spacing(7.0)),
        (Toggle::Radio, &mut theme.radio, spacing(23.0)),
        (Toggle::Switch, &mut theme.switch, spacing(41.0)),
    ]
    .map(|(kind, slot, (padding, margin, gap))| {
        slot.defaults.padding = padding;
        slot.defaults.margin = margin;
        slot.gap = gap;
        (kind, padding, margin, gap)
    });

    for (kind, padding, margin, gap) in kinds {
        for explicit in [false, true] {
            let id = WidgetId::from_hash(("row", explicit));
            let mut value = false;
            h.frame(|ui| {
                record_spaced(ui, kind, id, &mut value, explicit);
            });
            let (padding, margin, gap, center) = if explicit {
                (Spacing::ZERO, Spacing::ZERO, 0.0, false)
            } else {
                (padding, margin, gap, true)
            };
            let node = h.ui().forest().node_for_widget_id(Layer::Main, id);
            let layout = h.ui().tree(Layer::Main).records.layout()[node.idx()];
            let label = format!("{kind:?} explicit {explicit}");
            assert_eq!(layout.padding, padding, "{label}: padding");
            assert_eq!(layout.margin, margin, "{label}: margin");

            let row = h.arranged(id);
            let boxed = h.arranged(id.with("box"));
            let text = h.arranged(id.with("label"));
            assert_eq!(text.min.x - boxed.max().x, gap, "{label}: gap");
            let [_, top, _, bottom] = padding.to_array();
            let content_h = 120.0 - top - bottom;
            let below = if center {
                (content_h - boxed.size.h) / 2.0
            } else {
                0.0
            };
            assert_eq!(boxed.min.y - row.min.y - top, below, "{label}: child align");
        }
    }
}

/// One `kind` 120 px tall under `id`; `explicit` sets every spacing the theme supplies.
fn record_spaced(ui: &mut Ui, kind: Toggle, id: WidgetId, value: &mut bool, explicit: bool) {
    fn spaced<C: Configure>(row: C, id: WidgetId, explicit: bool) -> C {
        let row = row.id(id).size((Sizing::HUG, Sizing::fixed(120.0)));
        if explicit {
            row.padding(Spacing::ZERO)
                .margin(Spacing::ZERO)
                .gap(0.0)
                .child_align(Align::v(VAlign::Top))
        } else {
            row
        }
    }
    match kind {
        Toggle::Checkbox => spaced(Checkbox::new(value).label("t"), id, explicit).show(ui),
        Toggle::Radio => spaced(RadioButton::new(value, true).label("t"), id, explicit).show(ui),
        Toggle::Switch => spaced(Switch::new(value).label("t"), id, explicit).show(ui),
    };
}

#[derive(Clone, Copy, Debug)]
enum Toggle {
    Checkbox,
    Radio,
    Switch,
}

/// Record one `kind` bound to `value` at the origin; report `(clicked, changed)`.
fn record_toggle(ui: &mut Ui, kind: Toggle, value: &mut bool, disabled: bool) -> [bool; 2] {
    let id = WidgetId::from_hash("toggle");
    let before = *value;
    let r = match kind {
        Toggle::Checkbox => Checkbox::new(value)
            .id(id)
            .label("t")
            .disabled(disabled)
            .show(ui),
        Toggle::Switch => Switch::new(value)
            .id(id)
            .label("t")
            .disabled(disabled)
            .show(ui),
        Toggle::Radio => RadioButton::new(value, true)
            .id(id)
            .label("t")
            .disabled(disabled)
            .show(ui),
    };
    let (clicked, changed, committed) = (r.response.left.clicked(), r.changed, r.committed);
    assert_eq!(changed, *value != before, "{kind:?}: changed is the move");
    assert_eq!(committed, changed, "{kind:?}: a pick commits at once");
    [clicked, changed]
}

/// Two clicks per toggle, enabled and disabled. Checkbox and switch flip every click; radio latches, so its second click is `clicked()` without `changed`. A disabled toggle moves nothing.
#[test]
fn toggles_answer_clicks_and_ignore_them_disabled() {
    let at = Vec2::new(8.0, 8.0);
    for kind in [Toggle::Checkbox, Toggle::Switch, Toggle::Radio] {
        for disabled in [false, true] {
            let mut h = Harness::mono(UVec2::new(300, 100));
            let mut value = false;
            h.frame(|ui| {
                record_toggle(ui, kind, &mut value, disabled);
            });
            assert_eq!(
                h.hit_at(at),
                Some(WidgetId::from_hash("toggle")),
                "{kind:?} disabled {disabled}: the click lands",
            );

            let second_changes = !matches!(kind, Toggle::Radio);
            let want = if disabled {
                [([false, false], false), ([false, false], false)]
            } else {
                [
                    ([true, true], true),
                    ([true, second_changes], !second_changes),
                ]
            };
            for (n, (edges, after)) in want.into_iter().enumerate() {
                h.click(at);
                let got = h.frame_value(|ui| record_toggle(ui, kind, &mut value, disabled));
                assert_eq!(got, edges, "{kind:?} disabled {disabled}: click {n} edges");
                assert_eq!(
                    value, after,
                    "{kind:?} disabled {disabled}: value after click {n}"
                );
            }
            let quiet = h.frame_value(|ui| record_toggle(ui, kind, &mut value, disabled));
            assert_eq!(quiet, [false, false], "{kind:?}: the edges are one-shot");
        }
    }
}

/// Space activates a focused toggle as a click, which its response reports, and commits; an
/// unfocused one ignores it, and Enter, which WAI-ARIA leaves to the form, moves none.
#[test]
fn space_activates_a_focused_toggle() {
    use crate::input::keyboard::key::Key;

    for kind in [Toggle::Checkbox, Toggle::Switch, Toggle::Radio] {
        for (key, focused, acts) in [
            (Key::Char(' '), true, true),
            (Key::Char(' '), false, false),
            (Key::Enter, true, false),
        ] {
            let mut h = Harness::mono(UVec2::new(300, 100));
            let mut value = false;
            h.frame(|ui| {
                record_toggle(ui, kind, &mut value, false);
            });
            if focused {
                h.set_focus(WidgetId::from_hash("toggle"));
            }
            h.key(key);
            let edges = h.frame_value(|ui| record_toggle(ui, kind, &mut value, false));
            assert_eq!(
                (value, edges),
                (acts, [acts, acts]),
                "{kind:?} {key:?} focused {focused}"
            );
        }
    }
}
