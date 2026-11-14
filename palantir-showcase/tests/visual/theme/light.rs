use std::f32::consts::{FRAC_PI_2, PI};
use std::time::Duration;

use palantir::glam::Vec2;
use palantir::{
    AnimationSpec, Border, ButtonTheme, Chrome, ClipMode, CloseShown, ColorPickerTheme,
    ComboBoxTheme, Corners, DockTheme, DragValueTheme, ExpanderTheme, FocusRingTheme, FontFamily,
    FontSlant, FontWeight, MenuBarTheme, MenuItemTheme, MenuTheme, ModalTheme, PopupTheme,
    ProgressBarTheme, RgbaF32, ScrollbarTheme, SegmentedTheme, SeparatorTheme, Shadow, Size,
    SliderColors, SliderTheme, SlotDefaults, Spacing, SpinnerTheme, SplitterTheme, StatefulLook,
    TabsTheme, TextEditTheme, TextStyle, TextStyleOverrides, Theme, ToggleTheme, TooltipTheme,
    WidgetLook,
};

/// The light theme, with no motion, so its widgets show at rest.
pub(super) fn theme() -> Theme {
    Theme {
        button: ButtonTheme {
            unselected: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xffffff), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            selected: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xffffff), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            defaults: SlotDefaults {
                padding: Spacing::xy(12.0, 4.0),
                margin: Spacing::all(0.0),
                transition: AnimationSpec::SNAP,
                press: None,
            },
        },
        checkbox: ToggleTheme {
            unchecked: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f687359), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f687359), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xffffff), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            checked: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xa66509), Corners::all(4.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xa66509), Corners::all(4.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xa66509), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0xa6650973), Corners::all(4.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            indicator: RgbaF32::hex(0xffffff),
            unchecked_indicator: RgbaF32::hex(0x3d4248),
            box_size: 16.0,
            indicator_width: 2.0,
            check_points: [
                Vec2::new(0.21875, 0.53125),
                Vec2::new(0.4375, 0.75),
                Vec2::new(0.78125, 0.28125),
            ],
            indicator_inset: 4.0,
            gap: 8.0,
            track_aspect: 1.0,
            defaults: SlotDefaults {
                padding: Spacing::all(0.0),
                margin: Spacing::all(0.0),
                transition: AnimationSpec::SNAP,
                press: None,
            },
        },
        radio: ToggleTheme {
            unchecked: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(8.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f687359), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(8.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f687359), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(8.0))
                        .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xffffff), Corners::all(8.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            checked: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xa66509), Corners::all(8.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xa66509), Corners::all(8.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xa66509), Corners::all(8.0))
                        .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0xa6650973), Corners::all(8.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            indicator: RgbaF32::hex(0xffffff),
            unchecked_indicator: RgbaF32::hex(0x3d4248),
            box_size: 16.0,
            indicator_width: 2.0,
            check_points: [
                Vec2::new(0.21875, 0.53125),
                Vec2::new(0.4375, 0.75),
                Vec2::new(0.78125, 0.28125),
            ],
            indicator_inset: 4.0,
            gap: 8.0,
            track_aspect: 1.0,
            defaults: SlotDefaults {
                padding: Spacing::all(0.0),
                margin: Spacing::all(0.0),
                transition: AnimationSpec::SNAP,
                press: None,
            },
        },
        segmented: SegmentedTheme {
            track: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0)),
            track_padding: Spacing::all(2.0),
            segment: ButtonTheme {
                unselected: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::all(2.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x5f6873)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(2.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x3d4248)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xfcfcfc), Corners::all(2.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x3d4248)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::all(2.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x9aa1aa)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                selected: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xfcfcfc), Corners::all(2.0))
                            .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x3d4248)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xfcfcfc), Corners::all(2.0))
                            .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x3d4248)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xfcfcfc), Corners::all(2.0))
                            .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x3d4248)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xfcfcfc), Corners::all(2.0))
                            .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x9aa1aa)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                defaults: SlotDefaults {
                    padding: Spacing::xy(8.0, 2.0),
                    margin: Spacing::all(0.0),
                    transition: AnimationSpec::SNAP,
                    press: None,
                },
            },
        },
        switch: ToggleTheme {
            unchecked: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(10.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f687359), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(10.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f687359), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(10.0))
                        .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xffffff), Corners::all(10.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            checked: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xa66509), Corners::all(10.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xa66509), Corners::all(10.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xa66509), Corners::all(10.0))
                        .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0xa6650973), Corners::all(10.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            indicator: RgbaF32::hex(0xffffff),
            unchecked_indicator: RgbaF32::hex(0x3d4248),
            box_size: 20.0,
            indicator_width: 2.0,
            check_points: [
                Vec2::new(0.21875, 0.53125),
                Vec2::new(0.4375, 0.75),
                Vec2::new(0.78125, 0.28125),
            ],
            indicator_inset: 3.0,
            gap: 8.0,
            track_aspect: 1.75,
            defaults: SlotDefaults {
                padding: Spacing::all(0.0),
                margin: Spacing::all(0.0),
                transition: AnimationSpec::SNAP,
                press: None,
            },
        },
        scrollbar: ScrollbarTheme {
            thickness: 8.0,
            gap: 4.0,
            min_thumb: 24.0,
            track: RgbaF32::hexa(0x00000000),
            thumb: RgbaF32::hexa(0x5f687373),
            thumb_hovered: RgbaF32::hexa(0x5f6873a6),
            thumb_active: RgbaF32::hexa(0x5f6873d9),
        },
        text_edit: TextEditTheme {
            looks: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xffffff), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            placeholder: RgbaF32::hex(0x5f6873),
            caret: RgbaF32::hex(0x3d4248),
            caret_width: 1.5,
            selection: RgbaF32::hexa(0xa665093d),
            defaults: SlotDefaults {
                padding: Spacing::xy(8.0, 4.0),
                margin: Spacing::all(0.0),
                transition: AnimationSpec::SNAP,
                press: None,
            },
        },
        drag_value: DragValueTheme {
            looks: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xffffff), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            defaults: SlotDefaults {
                padding: Spacing::xy(12.0, 4.0),
                margin: Spacing::all(0.0),
                transition: AnimationSpec::SNAP,
                press: None,
            },
            caret: RgbaF32::hex(0x3d4248),
            caret_width: 1.5,
            selection: RgbaF32::hexa(0xa665093d),
            placeholder: RgbaF32::hex(0x5f6873),
        },
        menu: MenuTheme {
            material: None,
            panel: Chrome::rounded(RgbaF32::hex(0xffffff), Corners::all(10.0))
                .with_border(Border::new(RgbaF32::hexa(0x5f687338), 1.0))
                .with_shadow(Shadow::drop(
                    RgbaF32::hexa(0x00000024),
                    Vec2::new(0.0, 8.0),
                    16.0,
                )),
            padding: Spacing::all(4.0),
            min_width: 208.0,
            gap: 0.0,
            item: MenuItemTheme {
                unselected: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                        text: TextStyleOverrides::NONE,
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x9aa1aa)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                selected: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                        text: TextStyleOverrides::NONE.with_color(RgbaF32::hex(0xa66509)),
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0)),
                        text: TextStyleOverrides::NONE.with_color(RgbaF32::hex(0xa66509)),
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0)),
                        text: TextStyleOverrides::NONE.with_color(RgbaF32::hex(0xa66509)),
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x9aa1aa)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                shortcut: RgbaF32::hex(0x5f6873),
                gap: 24.0,
                defaults: SlotDefaults {
                    padding: Spacing::xy(12.0, 8.0),
                    margin: Spacing::all(0.0),
                    transition: AnimationSpec::SNAP,
                    press: None,
                },
            },
            separator: SeparatorTheme {
                color: RgbaF32::hexa(0x5f68732e),
                thickness: 1.0,
                margin: Spacing::xy(0.0, 4.0),
            },
        },
        menu_bar: MenuBarTheme {
            title: ButtonTheme {
                unselected: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::all(4.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x5f6873)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::all(4.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x9aa1aa)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                selected: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0)),
                        text: TextStyleOverrides::NONE,
                    },
                },
                defaults: SlotDefaults {
                    padding: Spacing::xy(12.0, 8.0),
                    margin: Spacing::all(0.0),
                    transition: AnimationSpec::SNAP,
                    press: None,
                },
            },
            bar: Chrome::new(RgbaF32::hex(0xeff1f3)),
            bar_padding: Spacing::xy(8.0, 4.0),
            gap: 4.0,
        },
        combo_box: ComboBoxTheme {
            gap: 12.0,
            arrow_size: Size::new(10.0, 6.0),
            arrow_width: 1.5,
        },
        modal: ModalTheme {
            panel_material: None,
            scrim_material: None,
            panel: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(10.0))
                .with_border(Border::new(RgbaF32::hexa(0x5f687338), 1.0))
                .with_shadow(Shadow::drop(
                    RgbaF32::hexa(0x00000024),
                    Vec2::new(0.0, 8.0),
                    16.0,
                )),
            scrim: Chrome::new(RgbaF32::hexa(0x00000080)),
            padding: Spacing::all(24.0),
            min_width: 280.0,
        },
        popup: PopupTheme {
            material: None,
            panel: Chrome::rounded(RgbaF32::hex(0xffffff), Corners::all(10.0))
                .with_border(Border::new(RgbaF32::hexa(0x5f687338), 1.0))
                .with_shadow(Shadow::drop(
                    RgbaF32::hexa(0x00000024),
                    Vec2::new(0.0, 8.0),
                    16.0,
                )),
            padding: Spacing::all(8.0),
        },
        color_picker: ColorPickerTheme {
            field_width: 208.0,
            field_height: 160.0,
            bar_thickness: 14.0,
            chip_size: 38.0,
            swatch_size: 18.0,
            handle_radius: 6.0,
            handle_width: 1.5,
            handle_outer: RgbaF32::hexa(0x000000bf),
            handle_inner: RgbaF32::hexa(0xfffffff2),
            checker_light: RgbaF32::hex(0xe4e6e8),
            checker_dark: RgbaF32::hex(0xffffff),
            checker_cell: 6.0,
            chip: ButtonTheme {
                unselected: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::NONE.with_border(Border::new(RgbaF32::hex(0xd9dbdd), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::NONE.with_border(Border::new(RgbaF32::hex(0x5f6873), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    active: WidgetLook {
                        chrome: Chrome::NONE.with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::NONE.with_border(Border::new(RgbaF32::hex(0xffffff), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                },
                selected: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::NONE.with_border(Border::new(RgbaF32::hex(0xd9dbdd), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::NONE.with_border(Border::new(RgbaF32::hex(0x5f6873), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    active: WidgetLook {
                        chrome: Chrome::NONE.with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::NONE.with_border(Border::new(RgbaF32::hex(0xffffff), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                },
                defaults: SlotDefaults {
                    padding: Spacing::all(0.0),
                    margin: Spacing::all(0.0),
                    transition: AnimationSpec::SNAP,
                    press: None,
                },
            },
            disabled_veil: RgbaF32::hexa(0xffffff80),
            gap: 6.0,
            popup: PopupTheme {
                material: None,
                panel: Chrome::rounded(RgbaF32::hex(0xffffff), Corners::all(10.0))
                    .with_border(Border::new(RgbaF32::hexa(0x5f687338), 1.0))
                    .with_shadow(Shadow::drop(
                        RgbaF32::hexa(0x00000024),
                        Vec2::new(0.0, 8.0),
                        16.0,
                    )),
                padding: Spacing::all(8.0),
            },
            value: DragValueTheme {
                looks: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0))
                            .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0))
                            .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0))
                            .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xffffff), Corners::all(4.0))
                            .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x9aa1aa)),
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                defaults: SlotDefaults {
                    padding: Spacing::all(5.0),
                    margin: Spacing::all(0.0),
                    transition: AnimationSpec::SNAP,
                    press: None,
                },
                caret: RgbaF32::hex(0x3d4248),
                caret_width: 1.5,
                selection: RgbaF32::hexa(0xa665093d),
                placeholder: RgbaF32::hex(0x5f6873),
            },
            hex: TextEditTheme {
                looks: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0))
                            .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0))
                            .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0))
                            .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0xffffff), Corners::all(4.0))
                            .with_border(Border::new(RgbaF32::hexa(0x5f68732e), 1.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0x9aa1aa)),
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                placeholder: RgbaF32::hex(0x5f6873),
                caret: RgbaF32::hex(0x3d4248),
                caret_width: 1.5,
                selection: RgbaF32::hexa(0xa665093d),
                defaults: SlotDefaults {
                    padding: Spacing::all(5.0),
                    margin: Spacing::all(0.0),
                    transition: AnimationSpec::SNAP,
                    press: None,
                },
            },
            label: TextStyleOverrides {
                color: Some(RgbaF32::hex(0x5f6873)),
                font_size: Some(10.0),
                family: Some(FontFamily::MONO),
                ..TextStyleOverrides::NONE
            },
        },
        tooltip: TooltipTheme {
            material: None,
            panel: Chrome::rounded(RgbaF32::hex(0xffffff), Corners::all(4.0))
                .with_border(Border::new(RgbaF32::hexa(0x5f687338), 1.0))
                .with_shadow(Shadow::drop(
                    RgbaF32::hexa(0x00000024),
                    Vec2::new(0.0, 8.0),
                    16.0,
                )),
            text: TextStyleOverrides {
                font_size: Some(12.0),
                ..TextStyleOverrides::NONE
            },
            padding: Spacing::xy(12.0, 4.0),
            max_size: Size::new(280.0, f32::INFINITY),
            delay: Duration::from_millis(500),
            warmup: Duration::from_millis(1000),
            gap: 8.0,
        },
        progress_bar: ProgressBarTheme {
            track: RgbaF32::hex(0xe4e6e8),
            fill: RgbaF32::hex(0xa66509),
            thickness: 6.0,
        },
        separator: SeparatorTheme {
            color: RgbaF32::hexa(0x5f68732e),
            thickness: 1.0,
            margin: Spacing::all(0.0),
        },
        slider: SliderTheme {
            enabled: SliderColors {
                track: RgbaF32::hex(0xe4e6e8),
                fill: RgbaF32::hex(0xa66509),
                knob: RgbaF32::hex(0x3d4248),
            },
            disabled: SliderColors {
                track: RgbaF32::hex(0xffffff),
                fill: RgbaF32::hex(0x9aa1aa),
                knob: RgbaF32::hex(0x9aa1aa),
            },
            knob_size: 18.0,
            track_thickness: 4.0,
        },
        spinner: SpinnerTheme {
            color: RgbaF32::hex(0xa66509),
            diameter: 24.0,
            sweep: 1.5 * PI,
            period: Duration::from_millis(1400),
            width_ratio: 0.12,
            min_stroke_width: 1.5,
        },
        splitter: SplitterTheme {
            grab_thickness: 6.0,
            rule: RgbaF32::hexa(0x5f68732e),
            rule_thickness: 1.0,
            rule_hovered: RgbaF32::hex(0xe4e6e8),
            rule_active: RgbaF32::hexa(0xa6650999),
        },
        tabs: TabsTheme {
            unselected: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::top(6.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x5f6873)),
                        ..TextStyleOverrides::NONE
                    },
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::top(6.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::top(6.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::top(6.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            selected: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xfcfcfc), Corners::top(6.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xfcfcfc), Corners::top(6.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xfcfcfc), Corners::top(6.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xfcfcfc), Corners::top(6.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            cap_focused: RgbaF32::hex(0xa66509),
            cap_unfocused: RgbaF32::hex(0xd9dbdd),
            cap_thickness: 2.0,
            strip: Chrome::new(RgbaF32::hex(0xeff1f3)),
            strip_padding: Spacing::new(8.0, 4.0, 8.0, 0.0),
            gap: 4.0,
            rule: RgbaF32::hexa(0x5f68732e),
            rule_thickness: 0.0,
            radius: 6.0,
            chip_padding: Spacing::xy(12.0, 8.0),
            trailing_inset: 4.0,
            min_width: 64.0,
            max_width: 200.0,
            close: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x5f6873)),
                        ..TextStyleOverrides::NONE
                    },
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0))
                        .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            close_size: 16.0,
            close_glyph_span: 8.0,
            close_glyph_width: 1.5,
            close_shown: CloseShown::SelectedOrHovered,
            caret: RgbaF32::hex(0xa66509),
            caret_width: 2.0,
            label_gap: 8.0,
            defaults: SlotDefaults {
                padding: Spacing::all(0.0),
                margin: Spacing::all(0.0),
                transition: AnimationSpec::SNAP,
                press: None,
            },
        },
        dock: DockTheme {
            preview: Chrome::rounded(RgbaF32::hexa(0xa665092e), Corners::all(2.0))
                .with_border(Border::new(RgbaF32::hex(0xa66509), 1.5)),
            ghost: WidgetLook {
                chrome: Chrome::rounded(RgbaF32::hex(0xeff1f3), Corners::all(6.0))
                    .with_border(Border::new(RgbaF32::hex(0xa66509), 1.0)),
                text: TextStyleOverrides::NONE,
            },
            ghost_padding: Spacing::xy(12.0, 8.0),
            ghost_offset: Vec2::new(12.0, 16.0),
        },
        expander: ExpanderTheme {
            looks: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xe4e6e8), Corners::all(4.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd9dbdd), Corners::all(4.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9aa1aa)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            arrow_size: Size::new(11.0, 11.0),
            arrow_radius: 1.5,
            arrow_closed_angle: -FRAC_PI_2,
            arrow_open_angle: 0.0,
            gap: 8.0,
            indent: 19.0,
            body_padding: Spacing::xy(0.0, 4.0),
            defaults: SlotDefaults {
                padding: Spacing::all(4.0),
                margin: Spacing::all(0.0),
                transition: AnimationSpec::SNAP,
                press: None,
            },
        },
        focus_ring: FocusRingTheme {
            color: RgbaF32::hex(0xa66509),
            width: 2.0,
        },
        text: TextStyle {
            font_size: 14.0,
            color: RgbaF32::hex(0x3d4248),
            line_height_factor: 1.2,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            slant: FontSlant::Normal,
        },
        window_bg: RgbaF32::hex(0xfcfcfc),
        material_fallback: RgbaF32::hexa(0xeeeef4e0),
        transition: AnimationSpec::THEME,
        panel_chrome: None,
        panel_clip: ClipMode::None,
    }
}
