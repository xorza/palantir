//! The suite's own theme, written out whole over two suite-owned palettes, so no change to a
//! recipe, `Palette` or `Metrics` moves a golden. A new `Theme` field fails to compile until the
//! theme holds it.

use std::f32::consts::{FRAC_PI_2, PI};
use std::time::Duration;

use palantir::glam::Vec2;
use palantir::{
    AnimationSpec, Border, ButtonTheme, Chrome, ClipMode, CloseShown, ColorPickerTheme,
    ColorScheme, ComboBoxTheme, Corners, DockTheme, DragValueTheme, Easing, ExpanderTheme,
    FocusRingTheme, FontFamily, FontSlant, FontWeight, MenuBarTheme, MenuItemTheme, MenuTheme,
    ModalTheme, Palette, PopupTheme, ProgressBarTheme, RgbaF32, ScrollbarTheme, SegmentedTheme,
    SeparatorTheme, Shadow, Size, SliderColors, SliderTheme, SlotDefaults, Spacing, SpinnerTheme,
    SplitterTheme, StatefulLook, TabsTheme, TextEditTheme, TextStyle, TextStyleOverrides, Theme,
    ToggleTheme, TooltipTheme, WidgetLook,
};

use crate::visual_rig::{FIXTURE_PALETTE, LIGHT_FIXTURE_PALETTE};

/// The theme every fixture renders under in `scheme`.
pub(crate) fn fixture(scheme: ColorScheme) -> Theme {
    theme(&match scheme {
        ColorScheme::Dark => FixtureScheme {
            palette: FIXTURE_PALETTE,
            motion: Motion::Moving,
            material_fallback: RgbaF32::hexa(0x26263af0),
        },
        ColorScheme::Light => FixtureScheme {
            palette: LIGHT_FIXTURE_PALETTE,
            motion: Motion::Still,
            material_fallback: RgbaF32::hexa(0xeeeef4e0),
        },
    })
}

/// What the theme reads in one scheme: its palette, its widgets' motion, and the surface a
/// material falls back to.
#[derive(Debug)]
struct FixtureScheme {
    palette: Palette,
    motion: Motion,
    material_fallback: RgbaF32,
}

/// Whether the widgets move between their looks: the dark scheme's do, the light one's snap, so
/// its widgets show at rest.
#[derive(Clone, Copy, Debug)]
enum Motion {
    Moving,
    Still,
}

impl Motion {
    /// A look's change: 120 ms out of a cubic, or none.
    const fn widget(self) -> AnimationSpec {
        match self {
            Self::Moving => AnimationSpec::duration(Duration::from_millis(120), Easing::OutCubic),
            Self::Still => AnimationSpec::SNAP,
        }
    }

    /// A thumb's travel: a spring of stiffness 170 and damping 26, or none.
    fn spring(self) -> AnimationSpec {
        match self {
            Self::Moving => AnimationSpec::spring(170.0, 26.0),
            Self::Still => AnimationSpec::SNAP,
        }
    }
}

/// `color` at the alpha of `byte`, as `RgbaF32::hexa` reads one.
const fn alpha(color: RgbaF32, byte: u8) -> RgbaF32 {
    color.with_alpha(byte as f32 / 255.0)
}

/// The suite's theme in `scheme`: a literal in structure, every color a role of its palette.
fn theme(scheme: &FixtureScheme) -> Theme {
    let p = &scheme.palette;
    Theme {
        button: ButtonTheme {
            unselected: StatefulLook {
                normal: WidgetLook::new(
                    Chrome::rounded(p.element_mid, Corners::all(4.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                ),
                hovered: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(4.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                ),
                active: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(4.0))
                        .with_border(Border::new(p.border_focused, 1.0)),
                ),
                disabled: WidgetLook {
                    chrome: Chrome::rounded(p.element, Corners::all(4.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            selected: StatefulLook {
                normal: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(4.0))
                        .with_border(Border::new(p.border_focused, 1.0)),
                ),
                hovered: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(4.0))
                        .with_border(Border::new(p.border_focused, 1.0)),
                ),
                active: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(4.0))
                        .with_border(Border::new(p.border_focused, 1.0)),
                ),
                disabled: WidgetLook {
                    chrome: Chrome::rounded(p.element, Corners::all(4.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            defaults: SlotDefaults {
                padding: Spacing::xy(12.0, 4.0),
                margin: Spacing::all(0.0),
                transition: scheme.motion.widget(),
                press: None,
            },
        },
        checkbox: ToggleTheme {
            unchecked: StatefulLook {
                normal: WidgetLook::new(
                    Chrome::rounded(p.element_mid, Corners::all(4.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x59), 1.0)),
                ),
                hovered: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(4.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x59), 1.0)),
                ),
                active: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(4.0))
                        .with_border(Border::new(p.border_focused, 1.0)),
                ),
                disabled: WidgetLook {
                    chrome: Chrome::rounded(p.element, Corners::all(4.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            checked: StatefulLook {
                normal: WidgetLook::new(Chrome::rounded(p.accent, Corners::all(4.0))),
                hovered: WidgetLook::new(Chrome::rounded(p.accent, Corners::all(4.0))),
                active: WidgetLook::new(
                    Chrome::rounded(p.accent, Corners::all(4.0))
                        .with_border(Border::new(p.border_focused, 1.0)),
                ),
                disabled: WidgetLook {
                    chrome: Chrome::rounded(alpha(p.accent, 0x73), Corners::all(4.0)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            indicator: p.on_accent,
            indicator_unchecked: p.text,
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
                normal: WidgetLook::new(
                    Chrome::rounded(p.element_mid, Corners::all(8.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x59), 1.0)),
                ),
                hovered: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(8.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x59), 1.0)),
                ),
                active: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(8.0))
                        .with_border(Border::new(p.border_focused, 1.0)),
                ),
                disabled: WidgetLook {
                    chrome: Chrome::rounded(p.element, Corners::all(8.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            checked: StatefulLook {
                normal: WidgetLook::new(Chrome::rounded(p.accent, Corners::all(8.0))),
                hovered: WidgetLook::new(Chrome::rounded(p.accent, Corners::all(8.0))),
                active: WidgetLook::new(
                    Chrome::rounded(p.accent, Corners::all(8.0))
                        .with_border(Border::new(p.border_focused, 1.0)),
                ),
                disabled: WidgetLook {
                    chrome: Chrome::rounded(alpha(p.accent, 0x73), Corners::all(8.0)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            indicator: p.on_accent,
            indicator_unchecked: p.text,
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
            track: Chrome::rounded(p.element_mid, Corners::all(4.0)),
            track_padding: Spacing::all(2.0),
            pill: Chrome::rounded(p.accent, Corners::all(2.0)),
            slide: AnimationSpec::SNAP,
            gap: 4.0,
            segment: ButtonTheme {
                unselected: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::all(2.0)),
                        text: TextStyleOverrides {
                            color: Some(p.text_muted),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(p.element_strong, Corners::all(2.0)),
                        text: TextStyleOverrides {
                            color: Some(p.text),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(p.window_bg, Corners::all(2.0)),
                        text: TextStyleOverrides {
                            color: Some(p.text),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::all(2.0)),
                        text: TextStyleOverrides {
                            color: Some(p.text_disabled),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                selected: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::all(2.0)),
                        text: TextStyleOverrides {
                            color: Some(p.on_accent),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::all(2.0)),
                        text: TextStyleOverrides {
                            color: Some(p.on_accent),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::all(2.0)),
                        text: TextStyleOverrides {
                            color: Some(p.on_accent),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::all(2.0)),
                        text: TextStyleOverrides {
                            color: Some(alpha(p.on_accent, 0x80)),
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
                normal: WidgetLook::new(
                    Chrome::rounded(p.element_mid, Corners::all(10.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x59), 1.0)),
                ),
                hovered: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(10.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x59), 1.0)),
                ),
                active: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(10.0))
                        .with_border(Border::new(p.border_focused, 1.0)),
                ),
                disabled: WidgetLook {
                    chrome: Chrome::rounded(p.element, Corners::all(10.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            checked: StatefulLook {
                normal: WidgetLook::new(Chrome::rounded(p.accent, Corners::all(10.0))),
                hovered: WidgetLook::new(Chrome::rounded(p.accent, Corners::all(10.0))),
                active: WidgetLook::new(
                    Chrome::rounded(p.accent, Corners::all(10.0))
                        .with_border(Border::new(p.border_focused, 1.0)),
                ),
                disabled: WidgetLook {
                    chrome: Chrome::rounded(alpha(p.accent, 0x73), Corners::all(10.0)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            indicator: p.on_accent,
            indicator_unchecked: p.text,
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
                transition: scheme.motion.spring(),
                press: None,
            },
        },
        scrollbar: ScrollbarTheme {
            thickness: 8.0,
            gap: 4.0,
            min_thumb: 24.0,
            track: RgbaF32::hexa(0x00000000),
            thumb: alpha(p.text_muted, 0x73),
            thumb_hovered: alpha(p.text_muted, 0xa6),
            thumb_active: alpha(p.text_muted, 0xd9),
        },
        text_edit: TextEditTheme {
            looks: StatefulLook {
                normal: WidgetLook::new(
                    Chrome::rounded(p.element_mid, Corners::all(4.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                ),
                hovered: WidgetLook::new(
                    Chrome::rounded(p.element_mid, Corners::all(4.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                ),
                active: WidgetLook::new(
                    Chrome::rounded(p.element_mid, Corners::all(4.0))
                        .with_border(Border::new(p.border_focused, 1.0)),
                ),
                disabled: WidgetLook {
                    chrome: Chrome::rounded(p.element, Corners::all(4.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            placeholder: p.text_muted,
            caret: p.text,
            caret_width: 1.5,
            selection: alpha(p.accent, 0x3d),
            defaults: SlotDefaults {
                padding: Spacing::xy(8.0, 4.0),
                margin: Spacing::all(0.0),
                transition: AnimationSpec::SNAP,
                press: None,
            },
        },
        drag_value: DragValueTheme {
            looks: StatefulLook {
                normal: WidgetLook::new(
                    Chrome::rounded(p.element_mid, Corners::all(4.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                ),
                hovered: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(4.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                ),
                active: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(4.0))
                        .with_border(Border::new(p.border_focused, 1.0)),
                ),
                disabled: WidgetLook {
                    chrome: Chrome::rounded(p.element, Corners::all(4.0))
                        .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            defaults: SlotDefaults {
                padding: Spacing::xy(12.0, 4.0),
                margin: Spacing::all(0.0),
                transition: scheme.motion.widget(),
                press: None,
            },
            caret: p.text,
            caret_width: 1.5,
            selection: alpha(p.accent, 0x3d),
            placeholder: p.text_muted,
            range_fill: alpha(p.accent, 0x59),
            range_fill_disabled: alpha(p.text_disabled, 0x59),
        },
        menu: MenuTheme {
            material: None,
            panel: Chrome::rounded(p.element, Corners::all(10.0))
                .with_border(Border::new(alpha(p.text_muted, 0x38), 1.0))
                .with_shadow(Shadow::drop(p.shadow, Vec2::new(0.0, 8.0), 16.0)),
            padding: Spacing::all(4.0),
            min_width: 208.0,
            gap: 0.0,
            item: MenuItemTheme {
                unselected: StatefulLook {
                    normal: WidgetLook::new(Chrome::new(RgbaF32::hexa(0x00000000))),
                    hovered: WidgetLook::new(Chrome::rounded(p.element_mid, Corners::all(4.0))),
                    active: WidgetLook::new(Chrome::rounded(p.element_mid, Corners::all(4.0))),
                    disabled: WidgetLook {
                        chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                        text: TextStyleOverrides {
                            color: Some(p.text_disabled),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                selected: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                        text: TextStyleOverrides::NONE.with_color(p.accent),
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(p.element_mid, Corners::all(4.0)),
                        text: TextStyleOverrides::NONE.with_color(p.accent),
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(p.element_mid, Corners::all(4.0)),
                        text: TextStyleOverrides::NONE.with_color(p.accent),
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                        text: TextStyleOverrides {
                            color: Some(p.text_disabled),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                shortcut: TextStyleOverrides::NONE.with_color(p.text_muted),
                gap: 24.0,
                check_size: 14.0,
                check_width: 2.0,
                check_points: [
                    Vec2::new(2.5 / 14.0, 7.5 / 14.0),
                    Vec2::new(5.5 / 14.0, 10.5 / 14.0),
                    Vec2::new(11.5 / 14.0, 3.5 / 14.0),
                ],
                check_gap: 8.0,
                defaults: SlotDefaults {
                    padding: Spacing::xy(12.0, 8.0),
                    margin: Spacing::all(0.0),
                    transition: scheme.motion.widget(),
                    press: None,
                },
            },
            separator: SeparatorTheme {
                color: alpha(p.text_muted, 0x2e),
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
                            color: Some(p.text_muted),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    hovered: WidgetLook::new(Chrome::rounded(p.element_mid, Corners::all(4.0))),
                    active: WidgetLook::new(Chrome::rounded(p.element_strong, Corners::all(4.0))),
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::all(4.0)),
                        text: TextStyleOverrides {
                            color: Some(p.text_disabled),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                selected: StatefulLook {
                    normal: WidgetLook::new(Chrome::rounded(p.element_mid, Corners::all(4.0))),
                    hovered: WidgetLook::new(Chrome::rounded(p.element_mid, Corners::all(4.0))),
                    active: WidgetLook::new(Chrome::rounded(p.element_mid, Corners::all(4.0))),
                    disabled: WidgetLook::new(Chrome::rounded(p.element_mid, Corners::all(4.0))),
                },
                defaults: SlotDefaults {
                    padding: Spacing::xy(12.0, 8.0),
                    margin: Spacing::all(0.0),
                    transition: scheme.motion.widget(),
                    press: None,
                },
            },
            bar: Chrome::new(p.surround),
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
            panel: Chrome::rounded(p.element_mid, Corners::all(10.0))
                .with_border(Border::new(alpha(p.text_muted, 0x38), 1.0))
                .with_shadow(Shadow::drop(p.shadow, Vec2::new(0.0, 8.0), 16.0)),
            scrim: Chrome::new(RgbaF32::hexa(0x00000080)),
            padding: Spacing::all(24.0),
            min_width: 280.0,
        },
        popup: PopupTheme {
            material: None,
            panel: Chrome::rounded(p.element, Corners::all(10.0))
                .with_border(Border::new(alpha(p.text_muted, 0x38), 1.0))
                .with_shadow(Shadow::drop(p.shadow, Vec2::new(0.0, 8.0), 16.0)),
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
            checker_light: p.element_mid,
            checker_dark: p.element,
            checker_cell: 6.0,
            chip: ButtonTheme {
                unselected: StatefulLook {
                    normal: WidgetLook::new(
                        Chrome::NONE.with_border(Border::new(p.element_strong, 1.0)),
                    ),
                    hovered: WidgetLook::new(
                        Chrome::NONE.with_border(Border::new(p.text_muted, 1.0)),
                    ),
                    active: WidgetLook::new(
                        Chrome::NONE.with_border(Border::new(p.border_focused, 1.0)),
                    ),
                    disabled: WidgetLook::new(
                        Chrome::NONE.with_border(Border::new(p.element, 1.0)),
                    ),
                },
                selected: StatefulLook {
                    normal: WidgetLook::new(
                        Chrome::NONE.with_border(Border::new(p.element_strong, 1.0)),
                    ),
                    hovered: WidgetLook::new(
                        Chrome::NONE.with_border(Border::new(p.text_muted, 1.0)),
                    ),
                    active: WidgetLook::new(
                        Chrome::NONE.with_border(Border::new(p.border_focused, 1.0)),
                    ),
                    disabled: WidgetLook::new(
                        Chrome::NONE.with_border(Border::new(p.element, 1.0)),
                    ),
                },
                defaults: SlotDefaults {
                    padding: Spacing::all(0.0),
                    margin: Spacing::all(0.0),
                    transition: AnimationSpec::SNAP,
                    press: None,
                },
            },
            disabled_veil: alpha(p.element, 0x80),
            gap: 6.0,
            popup: PopupTheme {
                material: None,
                panel: Chrome::rounded(p.element, Corners::all(10.0))
                    .with_border(Border::new(alpha(p.text_muted, 0x38), 1.0))
                    .with_shadow(Shadow::drop(p.shadow, Vec2::new(0.0, 8.0), 16.0)),
                padding: Spacing::all(8.0),
            },
            value: DragValueTheme {
                looks: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(p.element_mid, Corners::all(4.0))
                            .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(p.element_strong, Corners::all(4.0))
                            .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(p.element_strong, Corners::all(4.0))
                            .with_border(Border::new(p.border_focused, 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(p.element, Corners::all(4.0))
                            .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                        text: TextStyleOverrides {
                            color: Some(p.text_disabled),
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                defaults: SlotDefaults {
                    padding: Spacing::all(5.0),
                    margin: Spacing::all(0.0),
                    transition: scheme.motion.widget(),
                    press: None,
                },
                caret: p.text,
                caret_width: 1.5,
                selection: alpha(p.accent, 0x3d),
                placeholder: p.text_muted,
                range_fill: alpha(p.accent, 0x59),
                range_fill_disabled: alpha(p.text_disabled, 0x59),
            },
            hex: TextEditTheme {
                looks: StatefulLook {
                    normal: WidgetLook {
                        chrome: Chrome::rounded(p.element_mid, Corners::all(4.0))
                            .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    hovered: WidgetLook {
                        chrome: Chrome::rounded(p.element_mid, Corners::all(4.0))
                            .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    active: WidgetLook {
                        chrome: Chrome::rounded(p.element_mid, Corners::all(4.0))
                            .with_border(Border::new(p.border_focused, 1.0)),
                        text: TextStyleOverrides {
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                    disabled: WidgetLook {
                        chrome: Chrome::rounded(p.element, Corners::all(4.0))
                            .with_border(Border::new(alpha(p.text_muted, 0x2e), 1.0)),
                        text: TextStyleOverrides {
                            color: Some(p.text_disabled),
                            font_size: Some(12.0),
                            family: Some(FontFamily::MONO),
                            ..TextStyleOverrides::NONE
                        },
                    },
                },
                placeholder: p.text_muted,
                caret: p.text,
                caret_width: 1.5,
                selection: alpha(p.accent, 0x3d),
                defaults: SlotDefaults {
                    padding: Spacing::all(5.0),
                    margin: Spacing::all(0.0),
                    transition: AnimationSpec::SNAP,
                    press: None,
                },
            },
            label: TextStyleOverrides {
                color: Some(p.text_muted),
                font_size: Some(10.0),
                family: Some(FontFamily::MONO),
                ..TextStyleOverrides::NONE
            },
        },
        tooltip: TooltipTheme {
            material: None,
            panel: Chrome::rounded(p.element, Corners::all(4.0))
                .with_border(Border::new(alpha(p.text_muted, 0x38), 1.0))
                .with_shadow(Shadow::drop(p.shadow, Vec2::new(0.0, 8.0), 16.0)),
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
            track: p.element_mid,
            fill: p.accent,
            thickness: 6.0,
        },
        separator: SeparatorTheme {
            color: alpha(p.text_muted, 0x2e),
            thickness: 1.0,
            margin: Spacing::all(0.0),
        },
        slider: SliderTheme {
            enabled: SliderColors {
                track: p.element_mid,
                fill: p.accent,
                knob: p.text,
            },
            disabled: SliderColors {
                track: p.element,
                fill: p.text_disabled,
                knob: p.text_disabled,
            },
            knob_diameter: 18.0,
            track_thickness: 4.0,
        },
        spinner: SpinnerTheme {
            color: p.accent,
            diameter: 24.0,
            sweep: 1.5 * PI,
            period: Duration::from_millis(1400),
            width_ratio: 0.12,
            min_stroke_width: 1.5,
        },
        splitter: SplitterTheme {
            grab_thickness: 6.0,
            rule: alpha(p.text_muted, 0x2e),
            rule_thickness: 1.0,
            rule_hovered: p.element_mid,
            rule_active: alpha(p.accent, 0x99),
        },
        tabs: TabsTheme {
            unselected: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::top(6.0)),
                    text: TextStyleOverrides {
                        color: Some(p.text_muted),
                        ..TextStyleOverrides::NONE
                    },
                },
                hovered: WidgetLook::new(Chrome::rounded(p.element_mid, Corners::top(6.0))),
                active: WidgetLook::new(Chrome::rounded(p.element_strong, Corners::top(6.0))),
                disabled: WidgetLook {
                    chrome: Chrome::rounded(RgbaF32::hexa(0x00000000), Corners::top(6.0)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            selected: StatefulLook {
                normal: WidgetLook::new(Chrome::rounded(p.window_bg, Corners::top(6.0))),
                hovered: WidgetLook::new(Chrome::rounded(p.window_bg, Corners::top(6.0))),
                active: WidgetLook::new(Chrome::rounded(p.window_bg, Corners::top(6.0))),
                disabled: WidgetLook {
                    chrome: Chrome::rounded(p.window_bg, Corners::top(6.0)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            cap_focused: p.accent,
            cap_unfocused: p.element_strong,
            cap_thickness: 2.0,
            strip: Chrome::new(p.surround),
            strip_padding: Spacing::new(8.0, 4.0, 8.0, 0.0),
            gap: 4.0,
            rule: alpha(p.text_muted, 0x2e),
            rule_thickness: 0.0,
            chip_padding: Spacing::xy(12.0, 8.0),
            trailing_inset: 4.0,
            min_width: 64.0,
            max_width: 200.0,
            close: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                    text: TextStyleOverrides {
                        color: Some(p.text_muted),
                        ..TextStyleOverrides::NONE
                    },
                },
                hovered: WidgetLook::new(Chrome::rounded(p.element_strong, Corners::all(4.0))),
                active: WidgetLook::new(
                    Chrome::rounded(p.element_strong, Corners::all(4.0))
                        .with_border(Border::new(p.border_focused, 1.0)),
                ),
                disabled: WidgetLook {
                    chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
                        ..TextStyleOverrides::NONE
                    },
                },
            },
            close_size: 16.0,
            close_glyph_span: 8.0,
            close_glyph_width: 1.5,
            close_shown: CloseShown::SelectedOrHovered,
            drop_mark: p.accent,
            drop_mark_width: 2.0,
            label_gap: 8.0,
            defaults: SlotDefaults {
                padding: Spacing::all(0.0),
                margin: Spacing::all(0.0),
                transition: scheme.motion.widget(),
                press: None,
            },
        },
        dock: DockTheme {
            preview: Chrome::rounded(alpha(p.accent, 0x2e), Corners::all(2.0))
                .with_border(Border::new(p.accent, 1.5)),
            ghost: WidgetLook::new(
                Chrome::rounded(p.surround, Corners::all(6.0))
                    .with_border(Border::new(p.accent, 1.0)),
            ),
            ghost_padding: Spacing::xy(12.0, 8.0),
            ghost_offset: Vec2::new(12.0, 16.0),
        },
        expander: ExpanderTheme {
            looks: StatefulLook {
                normal: WidgetLook::new(Chrome::new(RgbaF32::hexa(0x00000000))),
                hovered: WidgetLook::new(Chrome::rounded(p.element_mid, Corners::all(4.0))),
                active: WidgetLook::new(Chrome::rounded(p.element_strong, Corners::all(4.0))),
                disabled: WidgetLook {
                    chrome: Chrome::new(RgbaF32::hexa(0x00000000)),
                    text: TextStyleOverrides {
                        color: Some(p.text_disabled),
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
            color: p.accent,
            width: 2.0,
        },
        text: TextStyle {
            font_size: 14.0,
            color: p.text,
            line_height_factor: 1.2,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            slant: FontSlant::Normal,
        },
        window_bg: p.window_bg,
        material_fallback: scheme.material_fallback,
        transition: AnimationSpec::THEME,
        panel_chrome: None,
        panel_clip: ClipMode::None,
    }
}
