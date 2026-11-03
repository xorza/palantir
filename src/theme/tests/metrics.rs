use glam::Vec2;

use crate::animation::animation_spec::AnimationSpec;

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::metrics::{
    Metrics, RadiusSteps, ShadowGeometry, SpaceSteps, StrokeSteps, TextSteps,
};
use crate::theme::palette::Palette;
use crate::theme::{FACES_ERROR, Theme};

/// A text step is a font size: a file's `body: 0.0` is refused, and so is a theme built from it,
/// as every ambient text would paint nothing.
#[test]
fn a_text_step_is_a_font_size() {
    use crate::internals::panic_probe;
    use crate::primitives::math::domain;

    let valid = super::pretty(&Metrics::DEFAULT);
    let zero = valid.replacen("body: 14.0", "body: 0.0", 1);
    assert_ne!(zero, valid, "`body: 14.0` is not in the metrics");
    let error = ron::from_str::<Metrics>(&zero).expect_err("a zero body");
    assert!(
        error.to_string().contains(domain::FONT_LENGTH_RULE),
        "unexpected serde error: {error}"
    );

    let metrics = Metrics {
        text: TextSteps {
            body: 0.0,
            ..Metrics::DEFAULT.text
        },
        ..Metrics::DEFAULT
    };
    panic_probe::assert_panics_with(FACES_ERROR, || {
        Theme::from_parts(Palette::for_scheme(ColorScheme::Dark), &metrics)
    });
}

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
    float_shadow: ShadowGeometry {
        offset: Vec2::new(19.0, 21.0),
        blur: 23.0,
    },
    transition: AnimationSpec::MEDIUM,
};

#[test]
fn every_recipe_reads_its_geometry_from_the_metrics() {
    // A shadow color no stock palette has, so the cast shows it comes from the palette.
    let palette = Palette {
        shadow: RgbaF32::hex(0x123456),
        ..Palette::DARK
    };
    let t = Theme::from_parts(&palette, &DISTINCT);
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
            t.drag_value.defaults.padding,
            Spacing::xy(5.0, 1.0),
        ),
        ("menu", t.menu.padding, Spacing::all(1.0)),
        ("menu item", t.menu.item.defaults.padding, row),
        (
            "menu separator",
            t.menu.separator.margin,
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
            t.color_picker.popup.padding,
            Spacing::all(3.0),
        ),
        ("popup", t.popup.padding, Spacing::all(3.0)),
        (
            "picker value",
            t.color_picker.value.defaults.padding,
            Spacing::all(1.0),
        ),
        (
            "picker hex",
            t.color_picker.hex.defaults.padding,
            Spacing::all(1.0),
        ),
    ];
    for (slot, got, want) in paddings {
        assert_eq!(got, want, "{slot} padding");
    }
    let lengths = [
        ("menu hint gap", t.menu.item.gap, 9.0),
        ("tooltip gap", t.tooltip.gap, 3.0),
        ("tab gap", t.tabs.gap, 1.0),
        ("tab label gap", t.tabs.label_gap, 3.0),
        ("tab trailing inset", t.tabs.trailing_inset, 1.0),
        ("checkbox gap", t.checkbox.gap, 3.0),
        ("expander gap", t.expander.gap, 3.0),
        ("expander indent", t.expander.indent, 11.0 + 3.0),
        ("combo gap", t.combo_box.gap, 5.0),
        ("scrollbar gap", t.scrollbar.gap, 1.0),
        (
            "dock preview radius",
            t.dock.preview.corners.to_array()[0],
            11.0,
        ),
        ("tab radius", t.tabs.radius, 15.0),
        ("separator", t.separator.thickness, 0.25),
        ("splitter rule", t.splitter.rule_thickness, 0.25),
        (
            "picker chip ring",
            t.color_picker.chip.unselected.normal.chrome.border.width,
            0.25,
        ),
        ("picker gap", t.color_picker.gap, 3.0),
        ("caret", t.text_edit.caret_width, 0.5),
        ("dock preview", t.dock.preview.border.width, 0.5),
        ("tab close", t.tabs.close_glyph_width, 0.5),
        ("combo arrow", t.combo_box.arrow_width, 0.5),
        ("spinner floor", t.spinner.min_stroke_width, 0.5),
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
    let captions = [
        ("tooltip", t.tooltip.text.font_size),
        ("picker label", t.color_picker.label.font_size),
        (
            "picker value",
            t.color_picker.value.looks.normal.text.font_size,
        ),
        (
            "picker hex",
            t.color_picker.hex.looks.disabled.text.font_size,
        ),
    ];
    for (slot, size) in captions {
        assert_eq!(size, Some(25.0), "{slot} is the caption tier");
    }
    let chromes = [
        ("button", &t.button.unselected.normal.chrome, 13.0, 0.25),
        ("text edit", &t.text_edit.looks.normal.chrome, 13.0, 0.25),
        ("checkbox", &t.checkbox.unchecked.normal.chrome, 13.0, 0.25),
        ("tooltip", &t.tooltip.panel, 13.0, 0.25),
        ("menu", &t.menu.panel, 17.0, 0.25),
        ("picker popup", &t.color_picker.popup.panel, 17.0, 0.25),
        ("modal", &t.modal.panel, 17.0, 0.25),
        ("dock ghost", &t.dock.ghost.chrome, 15.0, 0.25),
    ];
    for (slot, chrome, radius, border) in chromes {
        assert_eq!(chrome.corners, Corners::all(radius), "{slot} corners");
        assert_eq!(chrome.border.width, border, "{slot} border");
    }
    assert_eq!(
        t.menu.item.unselected.hovered.chrome.corners,
        Corners::all(13.0)
    );
    assert_eq!(t.tabs.close.hovered.chrome.corners, Corners::all(13.0));
    assert_eq!(t.tabs.selected.normal.chrome.corners, Corners::top(15.0));
    for (slot, animation) in [
        ("button", t.button.defaults.transition),
        ("menu bar title", t.menu_bar.defaults.transition),
        ("menu item", t.menu.item.defaults.transition),
        ("tab chip", t.tabs.defaults.transition),
        ("drag value", t.drag_value.defaults.transition),
    ] {
        assert_eq!(animation, DISTINCT.transition, "{slot} transition");
    }
    for (slot, shadow) in [
        ("tooltip", t.tooltip.panel.shadow),
        ("menu", t.menu.panel.shadow),
        ("picker popup", t.color_picker.popup.panel.shadow),
        ("modal", t.modal.panel.shadow),
    ] {
        assert_eq!(
            shadow,
            Shadow::drop(RgbaF32::hex(0x123456), Vec2::new(19.0, 21.0), 23.0),
            "{slot} shadow"
        );
    }
}
