use std::f32::consts::{FRAC_PI_2, PI};
use std::time::Duration;

use glam::Vec2;
use palantir::{
    AnimationSpec, ButtonTheme, Chrome, ClipMode, CloseShown, ColorPickerTheme, ComboBoxTheme,
    ContextMenuTheme, Corners, DockTheme, DragValueTheme, Easing, ExpanderTheme, FocusRingTheme,
    FontFamily, FontSlant, FontWeight, MenuBarTheme, MenuItemTheme, ModalTheme, PopupTheme,
    ProgressBarTheme, RgbaF32, ScrollbarTheme, SeparatorTheme, Shadow, Size, SliderTheme,
    SlotDefaults, Spacing, SpinnerTheme, SplitterTheme, StatefulLook, Stroke, TabsTheme,
    TextEditTheme, TextStyle, TextStyleOverrides, Theme, ToggleTheme, TooltipTheme, WidgetLook,
};

/// The dark theme, in the hue-coded colors of `FIXTURE_PALETTE`, with motion.
pub(super) fn theme() -> Theme {
    Theme {
        button: ButtonTheme {
            unselected: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            selected: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            defaults: SlotDefaults {
                padding: Spacing::xy(12.0, 4.0),
                margin: Spacing::all(0.0),
                animation: Some(AnimationSpec::duration(
                    Duration::from_millis(120),
                    Easing::OutCubic,
                )),
            },
        },
        checkbox: ToggleTheme {
            unchecked: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a059), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a059), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            checked: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd23f7a), Corners::all(4.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd23f7a), Corners::all(4.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd23f7a), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0xd23f7a73), Corners::all(4.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            indicator: RgbaF32::hex(0x14141a),
            unchecked_indicator: RgbaF32::hex(0xf2f2f2),
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
                animation: None,
            },
        },
        radio: ToggleTheme {
            unchecked: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(8.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a059), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(8.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a059), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(8.0))
                        .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(8.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            checked: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd23f7a), Corners::all(8.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd23f7a), Corners::all(8.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd23f7a), Corners::all(8.0))
                        .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0xd23f7a73), Corners::all(8.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            indicator: RgbaF32::hex(0x14141a),
            unchecked_indicator: RgbaF32::hex(0xf2f2f2),
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
                animation: None,
            },
        },
        switch: ToggleTheme {
            unchecked: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(10.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a059), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(10.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a059), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(10.0))
                        .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(10.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            checked: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd23f7a), Corners::all(10.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd23f7a), Corners::all(10.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0xd23f7a), Corners::all(10.0))
                        .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0xd23f7a73), Corners::all(10.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            indicator: RgbaF32::hex(0x14141a),
            unchecked_indicator: RgbaF32::hex(0xf2f2f2),
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
                animation: Some(AnimationSpec::spring(170.0, 26.0)),
            },
        },
        scrollbar: ScrollbarTheme {
            thickness: 8.0,
            gap: 4.0,
            min_thumb: 24.0,
            track: RgbaF32::hexa(0x00000000),
            thumb: RgbaF32::hexa(0x9ad2a073),
            thumb_hovered: RgbaF32::hexa(0x9ad2a0a6),
            thumb_active: RgbaF32::hexa(0x9ad2a0d9),
        },
        text_edit: TextEditTheme {
            looks: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            placeholder: RgbaF32::hex(0x9ad2a0),
            caret: RgbaF32::hex(0xf2f2f2),
            caret_width: 1.5,
            selection: RgbaF32::hexa(0xd23f7a3d),
            defaults: SlotDefaults {
                padding: Spacing::xy(8.0, 4.0),
                margin: Spacing::all(0.0),
                animation: None,
            },
        },
        drag_value: DragValueTheme {
            chip: ButtonTheme {
                unselected: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0xd9a05e)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                selected: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0xd9a05e)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                defaults: SlotDefaults {
                    padding: Spacing::xy(12.0, 4.0),
                    margin: Spacing::all(0.0),
                    animation: Some(AnimationSpec::duration(
                        Duration::from_millis(120),
                        Easing::OutCubic,
                    )),
                },
            },
            editor: TextEditTheme {
                looks: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0xd9a05e)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                placeholder: RgbaF32::hex(0x9ad2a0),
                caret: RgbaF32::hex(0xf2f2f2),
                caret_width: 1.5,
                selection: RgbaF32::hexa(0xd23f7a3d),
                defaults: SlotDefaults {
                    padding: Spacing::xy(12.0, 4.0),
                    margin: Spacing::all(0.0),
                    animation: Some(AnimationSpec::duration(
                        Duration::from_millis(120),
                        Easing::OutCubic,
                    )),
                },
            },
        },
        context_menu: ContextMenuTheme {
            panel: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(10.0))
                .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a038), 1.0))
                .with_shadow(Shadow::drop(
                    RgbaF32::hexa(0x00000080),
                    Vec2::new(0.0, 8.0),
                    16.0,
                )),
            padding: Spacing::all(4.0),
            min_width: 208.0,
            gap: 0.0,
            item: MenuItemTheme {
                looks: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::fill(RgbaF32::hexa(0x00000000)),
                        text: TextStyleOverrides::NONE,
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0)),
                        text: TextStyleOverrides::NONE,
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::fill(RgbaF32::hexa(0x00000000)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0xd9a05e)),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                shortcut: RgbaF32::hex(0x9ad2a0),
                gap: 24.0,
                defaults: SlotDefaults {
                    padding: Spacing::xy(12.0, 8.0),
                    margin: Spacing::all(0.0),
                    animation: Some(AnimationSpec::duration(
                        Duration::from_millis(120),
                        Easing::OutCubic,
                    )),
                },
            },
            separator: SeparatorTheme {
                color: RgbaF32::hexa(0x9ad2a02e),
                thickness: 1.0,
                margin: Spacing::xy(0.0, 4.0),
            },
        },
        menu_bar: MenuBarTheme {
            title: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::all(4.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9ad2a0)),
                        ..TextStyleOverrides::NONE
                    },
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::all(4.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            open: WidgetLook {
                chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0)),
                text: TextStyleOverrides::NONE,
            },
            bar: Chrome::fill(RgbaF32::hex(0x3a1a14)),
            bar_padding: Spacing::xy(8.0, 4.0),
            gap: 4.0,
            defaults: SlotDefaults {
                padding: Spacing::xy(12.0, 8.0),
                margin: Spacing::all(0.0),
                animation: Some(AnimationSpec::duration(
                    Duration::from_millis(120),
                    Easing::OutCubic,
                )),
            },
        },
        combo_box: ComboBoxTheme {
            gap: 12.0,
            arrow_size: Vec2::new(10.0, 6.0),
            arrow_width: 1.5,
        },
        modal: ModalTheme {
            panel: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(10.0))
                .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a038), 1.0))
                .with_shadow(Shadow::drop(
                    RgbaF32::hexa(0x00000080),
                    Vec2::new(0.0, 8.0),
                    16.0,
                )),
            backdrop: RgbaF32::hexa(0x00000080),
            padding: Spacing::all(24.0),
            min_width: 280.0,
        },
        popup: PopupTheme {
            panel: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(10.0))
                .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a038), 1.0))
                .with_shadow(Shadow::drop(
                    RgbaF32::hexa(0x00000080),
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
            checker_light: RgbaF32::hex(0x1e4048),
            checker_dark: RgbaF32::hex(0x2e1f38),
            checker_cell: 6.0,
            border: RgbaF32::hex(0x3d4f1e),
            border_width: 1.0,
            gap: 6.0,
            popup: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(10.0))
                .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a038), 1.0))
                .with_shadow(Shadow::drop(
                    RgbaF32::hexa(0x00000080),
                    Vec2::new(0.0, 8.0),
                    16.0,
                )),
            popup_padding: Spacing::all(8.0),
            value: DragValueTheme {
                chip: ButtonTheme {
                    unselected: StatefulLook {
                        normal: WidgetLook {
                            chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0))
                                .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                            text: TextStyleOverrides {
                                font_size: Some(12.0),
                                family: Some(FontFamily::MONO),
                                ..TextStyleOverrides::NONE
                            },
                        },
                        hovered: WidgetLook {
                            chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                                .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                            text: TextStyleOverrides {
                                font_size: Some(12.0),
                                family: Some(FontFamily::MONO),
                                ..TextStyleOverrides::NONE
                            },
                        },
                        active: WidgetLook {
                            chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                                .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                            text: TextStyleOverrides {
                                font_size: Some(12.0),
                                family: Some(FontFamily::MONO),
                                ..TextStyleOverrides::NONE
                            },
                        },
                        disabled: WidgetLook {
                            chrome: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(4.0))
                                .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                            text: TextStyleOverrides {
                                color: Some(RgbaF32::hex(0xd9a05e)),
                                font_size: Some(12.0),
                                family: Some(FontFamily::MONO),
                                ..TextStyleOverrides::NONE
                            },
                        },
                    },
                    selected: StatefulLook {
                        normal: WidgetLook {
                            chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                                .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                            text: TextStyleOverrides::NONE,
                        },
                        hovered: WidgetLook {
                            chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                                .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                            text: TextStyleOverrides::NONE,
                        },
                        active: WidgetLook {
                            chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                                .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                            text: TextStyleOverrides::NONE,
                        },
                        disabled: WidgetLook {
                            chrome: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(4.0))
                                .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                            text: TextStyleOverrides {
                                color: Some(RgbaF32::hex(0xd9a05e)),
                                ..TextStyleOverrides::NONE
                            },
                        },
                    },
                    defaults: SlotDefaults {
                        padding: Spacing::all(5.0),
                        margin: Spacing::all(0.0),
                        animation: Some(AnimationSpec::duration(
                            Duration::from_millis(120),
                            Easing::OutCubic,
                        )),
                    },
                },
                editor: TextEditTheme {
                    looks: StatefulLook {
                        normal: WidgetLook {
                            chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0))
                                .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                            text: TextStyleOverrides {
                                font_size: Some(12.0),
                                family: Some(FontFamily::MONO),
                                ..TextStyleOverrides::NONE
                            },
                        },
                        hovered: WidgetLook {
                            chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                                .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                            text: TextStyleOverrides {
                                font_size: Some(12.0),
                                family: Some(FontFamily::MONO),
                                ..TextStyleOverrides::NONE
                            },
                        },
                        active: WidgetLook {
                            chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                                .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                            text: TextStyleOverrides {
                                font_size: Some(12.0),
                                family: Some(FontFamily::MONO),
                                ..TextStyleOverrides::NONE
                            },
                        },
                        disabled: WidgetLook {
                            chrome: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(4.0))
                                .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                            text: TextStyleOverrides {
                                color: Some(RgbaF32::hex(0xd9a05e)),
                                font_size: Some(12.0),
                                family: Some(FontFamily::MONO),
                                ..TextStyleOverrides::NONE
                            },
                        },
                    },
                    placeholder: RgbaF32::hex(0x9ad2a0),
                    caret: RgbaF32::hex(0xf2f2f2),
                    caret_width: 1.5,
                    selection: RgbaF32::hexa(0xd23f7a3d),
                    defaults: SlotDefaults {
                        padding: Spacing::all(5.0),
                        margin: Spacing::all(0.0),
                        animation: Some(AnimationSpec::duration(
                            Duration::from_millis(120),
                            Easing::OutCubic,
                        )),
                    },
                },
            },
            hex: TextEditTheme {
                looks: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(4.0))
                            .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a02e), 1.0)),
                        text: TextStyleOverrides {
                            color: Some(RgbaF32::hex(0xd9a05e)),
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                placeholder: RgbaF32::hex(0x9ad2a0),
                caret: RgbaF32::hex(0xf2f2f2),
                caret_width: 1.5,
                selection: RgbaF32::hexa(0xd23f7a3d),
                defaults: SlotDefaults {
                    padding: Spacing::all(5.0),
                    margin: Spacing::all(0.0),
                    animation: None,
                },
            },
            label: TextStyleOverrides {
                color: Some(RgbaF32::hex(0x9ad2a0)),
                font_size: Some(10.0),
                family: Some(FontFamily::MONO),
                ..TextStyleOverrides::NONE
            },
        },
        tooltip: TooltipTheme {
            panel: Chrome::rounded(RgbaF32::hex(0x2e1f38), Corners::all(4.0))
                .with_border(Stroke::new(RgbaF32::hexa(0x9ad2a038), 1.0))
                .with_shadow(Shadow::drop(
                    RgbaF32::hexa(0x00000080),
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
            track: RgbaF32::hex(0x1e4048),
            fill: RgbaF32::hex(0xd23f7a),
            thickness: 6.0,
        },
        separator: SeparatorTheme {
            color: RgbaF32::hexa(0x9ad2a02e),
            thickness: 1.0,
            margin: Spacing::all(0.0),
        },
        slider: SliderTheme {
            track: RgbaF32::hex(0x1e4048),
            fill: RgbaF32::hex(0xd23f7a),
            knob: RgbaF32::hex(0xf2f2f2),
            knob_size: 18.0,
            track_thickness: 4.0,
        },
        spinner: SpinnerTheme {
            color: RgbaF32::hex(0xd23f7a),
            diameter: 24.0,
            sweep: 1.5 * PI,
            period: Duration::from_millis(1400),
            width_ratio: 0.12,
            min_width: 1.5,
        },
        splitter: SplitterTheme {
            grab_thickness: 6.0,
            rule: RgbaF32::hexa(0x9ad2a02e),
            rule_thickness: 1.0,
            hovered: RgbaF32::hex(0x1e4048),
            active: RgbaF32::hexa(0xd23f7a99),
        },
        tabs: TabsTheme {
            selected: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x14141a), Corners::top(6.0)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x14141a), Corners::top(6.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x14141a), Corners::top(6.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x14141a), Corners::top(6.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            unselected: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::top(6.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9ad2a0)),
                        ..TextStyleOverrides::NONE
                    },
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::top(6.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::top(6.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::top(6.0)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            cap_focused: RgbaF32::hex(0xd23f7a),
            cap_unfocused: RgbaF32::hex(0x3d4f1e),
            cap_thickness: 2.0,
            strip: Chrome::fill(RgbaF32::hex(0x3a1a14)),
            strip_padding: Spacing::new(8.0, 4.0, 8.0, 0.0),
            gap: 4.0,
            rule: RgbaF32::hexa(0x9ad2a02e),
            rule_thickness: 0.0,
            radius: 6.0,
            chip_padding: Spacing::xy(12.0, 8.0),
            trailing_inset: 4.0,
            min_width: 64.0,
            max_width: 200.0,
            close: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::fill(RgbaF32::hexa(0x00000000)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0x9ad2a0)),
                        ..TextStyleOverrides::NONE
                    },
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0))
                        .with_border(Stroke::new(RgbaF32::hex(0x2f6fd0), 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::fill(RgbaF32::hexa(0x00000000)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            close_size: 16.0,
            close_glyph_span: 8.0,
            close_glyph_width: 1.5,
            close_shown: CloseShown::SelectedOrHovered,
            caret: RgbaF32::hex(0xd23f7a),
            caret_width: 2.0,
            label_gap: 8.0,
            defaults: SlotDefaults {
                padding: Spacing::all(0.0),
                margin: Spacing::all(0.0),
                animation: Some(AnimationSpec::duration(
                    Duration::from_millis(120),
                    Easing::OutCubic,
                )),
            },
        },
        dock: DockTheme {
            preview_fill: RgbaF32::hexa(0xd23f7a2e),
            preview_stroke: Stroke::new(RgbaF32::hex(0xd23f7a), 1.5),
            preview_radius: 2.0,
            ghost: WidgetLook {
                chrome: Chrome::rounded(RgbaF32::hex(0x3a1a14), Corners::all(6.0))
                    .with_border(Stroke::new(RgbaF32::hex(0xd23f7a), 1.0)),
                text: TextStyleOverrides::NONE,
            },
            ghost_padding: Spacing::xy(12.0, 8.0),
            ghost_offset: Vec2::new(12.0, 16.0),
        },
        expander: ExpanderTheme {
            looks: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::fill(RgbaF32::hexa(0x00000000)),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x1e4048), Corners::all(4.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hex(0x3d4f1e), Corners::all(4.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::fill(RgbaF32::hexa(0x00000000)),
                    text: TextStyleOverrides {
                        color: Some(RgbaF32::hex(0xd9a05e)),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            arrow_size: Vec2::new(11.0, 11.0),
            arrow_radius: 1.5,
            arrow_closed_angle: -FRAC_PI_2,
            arrow_open_angle: 0.0,
            gap: 8.0,
            indent: 19.0,
            body_padding: Spacing::xy(0.0, 4.0),
            defaults: SlotDefaults {
                padding: Spacing::all(4.0),
                margin: Spacing::all(0.0),
                animation: None,
            },
        },
        focus_ring: FocusRingTheme {
            color: RgbaF32::hex(0xd23f7a),
            width: 2.0,
        },
        text: TextStyle {
            font_size: 14.0,
            color: RgbaF32::hex(0xf2f2f2),
            line_height_factor: 1.2,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            slant: FontSlant::Normal,
        },
        window_clear: RgbaF32::hex(0x14141a),
        panel_chrome: None,
        panel_clip: ClipMode::None,
    }
}
