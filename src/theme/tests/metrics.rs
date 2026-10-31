use glam::Vec2;

use crate::animation::animation_spec::AnimationSpec;

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::theme::Theme;
use crate::theme::metrics::{Metrics, RadiusSteps, SpaceSteps, StrokeSteps, TextSteps};
use crate::theme::palette::Palette;

/// Steps no two of which are equal, and none equal to a default step, so each read below names
/// the one step its field takes.
const DISTINCT: Metrics = Metrics {
    text: TextSteps {
        caption: 25.0,
        body: 27.0,
        title: 29.0,
    },
    space: SpaceSteps {
        xxs: 0.125,
        xs: 1.0,
        sm: 3.0,
        md: 5.0,
        lg: 7.0,
        xl: 9.0,
    },
    radius: RadiusSteps {
        mark: 11.0,
        control: 13.0,
        card: 15.0,
        float: 17.0,
    },
    stroke: StrokeSteps {
        hairline: 0.25,
        fine: 0.5,
        line: 0.75,
    },
    float_shadow: Shadow::drop(RgbaF32::hex(0x123456), Vec2::new(19.0, 21.0), 23.0),
    transition: Some(AnimationSpec::MEDIUM),
};

#[test]
fn every_recipe_reads_its_geometry_from_the_metrics() {
    let t = Theme::from_parts(&Palette::DEFAULT, &DISTINCT);
    let row = Spacing::xy(5.0, 3.0);
    let paddings = [
        ("button", t.button.defaults.padding, Spacing::xy(5.0, 1.0)),
        (
            "text edit",
            t.text_edit.defaults.padding,
            Spacing::xy(3.0, 1.0),
        ),
        (
            "drag value",
            t.drag_value.chip.defaults.padding,
            Spacing::xy(5.0, 1.0),
        ),
        ("menu", t.context_menu.padding, Spacing::all(1.0)),
        ("menu item", t.context_menu.item.defaults.padding, row),
        (
            "menu separator",
            t.context_menu.separator.margin,
            Spacing::xy(0.0, 1.0),
        ),
        ("tooltip", t.tooltip.padding, Spacing::xy(5.0, 1.0)),
        ("modal", t.modal.padding, Spacing::all(9.0)),
        ("tab chip", t.tabs.chip_padding, row),
        (
            "tab strip",
            t.tabs.strip_padding,
            Spacing::new(3.0, 1.0, 3.0, 0.0),
        ),
        ("dock ghost", t.dock.ghost_padding, row),
        ("menu bar", t.menu_bar.bar_padding, Spacing::xy(3.0, 1.0)),
        ("expander", t.expander.defaults.padding, Spacing::all(1.0)),
        (
            "expander body",
            t.expander.body_padding,
            Spacing::new(0.0, 1.0, 0.0, 1.0),
        ),
        (
            "picker popup",
            t.color_picker.popup_padding,
            Spacing::all(3.0),
        ),
    ];
    for (slot, got, want) in paddings {
        assert_eq!(got, want, "{slot} padding");
    }
    let lengths = [
        ("menu hint gap", t.context_menu.item.gap, 9.0),
        ("tooltip gap", t.tooltip.gap, 3.0),
        ("tab gap", t.tabs.gap, 1.0),
        ("tab label gap", t.tabs.label_gap, 3.0),
        ("tab trailing inset", t.tabs.trailing_inset, 1.0),
        ("checkbox gap", t.checkbox.gap, 3.0),
        ("expander gap", t.expander.gap, 3.0),
        // The body lines up under the label: the arrow's 9 px and the gap.
        ("expander indent", t.expander.indent, 9.0 + 3.0),
        ("combo gap", t.combo_box.gap, 5.0),
        ("scrollbar gap", t.scrollbar.gap, 1.0),
        ("dock preview radius", t.dock.preview_radius, 11.0),
        ("tab radius", t.tabs.radius, 15.0),
        ("separator", t.separator.thickness, 0.25),
        ("splitter rule", t.splitter.rule_thickness, 0.25),
        ("picker border", t.color_picker.border_width, 0.25),
        ("caret", t.text_edit.caret_width, 0.5),
        ("dock preview", t.dock.preview_stroke.width, 0.5),
        ("tab close", t.tabs.close_glyph_width, 0.5),
        ("combo arrow", t.combo_box.arrow_width, 0.5),
        ("picker handle", t.color_picker.handle_width, 0.5),
        ("focus ring", t.focus_ring.width, 0.75),
        ("tab cap", t.tabs.cap_thickness, 0.75),
        ("tab caret", t.tabs.caret_width, 0.75),
        ("check mark", t.checkbox.indicator_width, 0.75),
    ];
    for (slot, got, want) in lengths {
        assert_eq!(got, want, "{slot}");
    }
    assert_eq!(t.dock.ghost_offset, Vec2::new(5.0, 7.0));
    assert_eq!(t.text.font_size, 27.0, "the ambient text is the body tier");
    assert_eq!(
        t.tooltip.text.font_size,
        Some(25.0),
        "a tooltip is the caption tier"
    );
    let chromes = [
        ("button", &t.button.unselected.normal.chrome, 13.0, 0.25),
        ("text edit", &t.text_edit.looks.normal.chrome, 13.0, 0.25),
        ("checkbox", &t.checkbox.unchecked.normal.chrome, 13.0, 0.25),
        ("tooltip", &t.tooltip.panel, 13.0, 0.25),
        ("menu", &t.context_menu.panel, 17.0, 0.25),
        ("picker popup", &t.color_picker.popup, 17.0, 0.25),
        ("modal", &t.modal.panel, 17.0, 0.25),
        ("dock ghost", &t.dock.ghost.chrome, 15.0, 0.25),
    ];
    for (slot, chrome, radius, border) in chromes {
        assert_eq!(chrome.corners, Corners::all(radius), "{slot} corners");
        assert_eq!(chrome.border.width, border, "{slot} border");
    }
    assert_eq!(
        t.context_menu.item.looks.hovered.chrome.corners,
        Corners::all(13.0)
    );
    assert_eq!(t.tabs.close.hovered.chrome.corners, Corners::all(13.0));
    assert_eq!(t.tabs.selected.normal.chrome.corners, Corners::top(15.0));
    for (slot, animation) in [
        ("button", t.button.defaults.animation),
        ("menu bar title", t.menu_bar.defaults.animation),
        ("menu item", t.context_menu.item.defaults.animation),
        ("tab chip", t.tabs.defaults.animation),
        ("drag value", t.drag_value.chip.defaults.animation),
    ] {
        assert_eq!(animation, DISTINCT.transition, "{slot} transition");
    }
    for (slot, shadow) in [
        ("tooltip", t.tooltip.panel.shadow),
        ("menu", t.context_menu.panel.shadow),
        ("picker popup", t.color_picker.popup.shadow),
        ("modal", t.modal.panel.shadow),
    ] {
        assert_eq!(shadow, DISTINCT.float_shadow, "{slot} shadow");
    }
}
