use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::widget_core::configure::Configure;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::radio::RadioButton;
use crate::widgets::switch::Switch;
use glam::{UVec2, Vec2};

/// All three toggles resolve their box through `WidgetTheme::plan`,
/// so [`crate::ToggleTheme`]'s `padding` / `margin` reach the row and
/// an explicit builder value still wins — the same contract `Button`
/// and `TextEdit` hold to.
///
/// One test over all three because they are now one code path: a
/// regression that reached only `Switch` would mean `Switch` had
/// stopped sharing it.
///
/// Each toggle gets its **own** spacing, so this also pins which slot
/// each one reads. `toggle_row` is shared but the slots are not —
/// restyling `checkbox` must leave `radio` and `switch` alone — and the
/// three name their slot exactly once, at their own `style` use.
/// Writing one value to all three slots could not tell them apart, so a
/// toggle reading its neighbour's slot passed.
#[test]
fn theme_spacing_reaches_every_toggle_row_and_explicit_wins() {
    #[track_caller]
    fn check(label: &str, h: &UiHarness, nodes: [NodeId; 2], padding: Spacing, margin: Spacing) {
        let layouts = h.ui.tree(Layer::Main).records.layout();
        let explicit = layouts[nodes[0].idx()];
        let inherited = layouts[nodes[1].idx()];
        assert_eq!(explicit.padding, Spacing::ZERO, "{label}: explicit padding");
        assert_eq!(explicit.margin, Spacing::ZERO, "{label}: explicit margin");
        assert_eq!(inherited.padding, padding, "{label}: theme padding");
        assert_eq!(inherited.margin, margin, "{label}: theme margin");
    }

    // Asymmetric, and different from each other, so neither a
    // padding/margin swap nor an axis swap can read as a pass — and
    // distinct per toggle, so nor can a slot mix-up.
    let spacing = |n: f32| (Spacing::xy(n, n + 2.0), Spacing::xy(n + 4.0, n + 6.0));
    let (cb_padding, cb_margin) = spacing(7.0);
    let (rb_padding, rb_margin) = spacing(23.0);
    let (sw_padding, sw_margin) = spacing(41.0);

    let mut h = UiHarness::new(UVec2::new(400, 300));
    let theme = h.ui.theme_mut();
    for (slot, (padding, margin)) in [
        (&mut theme.checkbox, (cb_padding, cb_margin)),
        (&mut theme.radio, (rb_padding, rb_margin)),
        (&mut theme.switch, (sw_padding, sw_margin)),
    ] {
        slot.defaults.padding = padding;
        slot.defaults.margin = margin;
    }

    let (mut a, mut b) = (false, false);
    let rows = h.frame_value(|ui| {
        [
            Checkbox::new(&mut a)
                .id(WidgetId::from_hash("cb-explicit"))
                .padding(Spacing::ZERO)
                .margin(Spacing::ZERO)
                .show(ui)
                .response
                .node(),
            Checkbox::new(&mut b)
                .id(WidgetId::from_hash("cb-inherited"))
                .show(ui)
                .response
                .node(),
        ]
    });
    check("Checkbox", &h, rows, cb_padding, cb_margin);

    let (mut c, mut d) = (0_u8, 0_u8);
    let rows = h.frame_value(|ui| {
        [
            RadioButton::new(&mut c, 1)
                .id(WidgetId::from_hash("rb-explicit"))
                .padding(Spacing::ZERO)
                .margin(Spacing::ZERO)
                .show(ui)
                .response
                .node(),
            RadioButton::new(&mut d, 1)
                .id(WidgetId::from_hash("rb-inherited"))
                .show(ui)
                .response
                .node(),
        ]
    });
    check("RadioButton", &h, rows, rb_padding, rb_margin);

    let (mut e, mut f) = (false, false);
    let rows = h.frame_value(|ui| {
        [
            Switch::new(&mut e)
                .id(WidgetId::from_hash("sw-explicit"))
                .padding(Spacing::ZERO)
                .margin(Spacing::ZERO)
                .show(ui)
                .response
                .node(),
            Switch::new(&mut f)
                .id(WidgetId::from_hash("sw-inherited"))
                .show(ui)
                .response
                .node(),
        ]
    });
    check("Switch", &h, rows, sw_padding, sw_margin);
}

#[derive(Clone, Copy, Debug)]
enum Toggle {
    Checkbox,
    Radio,
    Switch,
}

/// Record one `kind` bound to `value`, at the origin, and report
/// `(clicked, changed)` — what its response says this frame. Every toggle
/// reports `changed` exactly when the bound value moved, and commits it at
/// once.
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

/// Two clicks on each toggle, enabled and disabled. A checkbox and a
/// switch flip on every click, so `clicked()` is their change edge: true
/// exactly on the frames the value moved. A radio latches, so its second
/// click is `clicked()` without `changed`. A disabled toggle takes the
/// click on the same spot and moves nothing.
#[test]
fn toggles_answer_clicks_and_ignore_them_disabled() {
    let at = Vec2::new(8.0, 8.0);
    for kind in [Toggle::Checkbox, Toggle::Switch, Toggle::Radio] {
        for disabled in [false, true] {
            let mut h = UiHarness::new(UVec2::new(300, 100));
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
                h.click_at(at);
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

/// Space activates a focused toggle as a click does: a checkbox and a
/// switch flip, a radio selects, and each reports a committed change. An
/// unfocused toggle ignores the key.
#[test]
fn space_activates_a_focused_toggle() {
    use crate::input::keyboard::key::Key;

    for kind in [Toggle::Checkbox, Toggle::Switch, Toggle::Radio] {
        for focused in [true, false] {
            let mut h = UiHarness::new(UVec2::new(300, 100));
            let mut value = false;
            h.frame(|ui| {
                record_toggle(ui, kind, &mut value, false);
            });
            if focused {
                h.set_focus(WidgetId::from_hash("toggle"));
            }
            h.key(Key::Char(' '));
            let [_, changed] = h.frame_value(|ui| record_toggle(ui, kind, &mut value, false));
            assert_eq!(
                (value, changed),
                (focused, focused),
                "{kind:?} focused {focused}"
            );
        }
    }
}
