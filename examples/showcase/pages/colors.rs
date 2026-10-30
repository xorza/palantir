//! The color family in one composition: the whole panel, the parts alone, and the chip that opens its own panel.
//!
//! The field and both bars are exact per texel, so the Okhsv square keeps one brightness across the hue circle where HSV does not; switch the model under the panel to compare.

use crate::support;
use crate::support::{Section, api};
use palantir::{
    Align, Chrome, ColorButton, ColorCoords, ColorField, ColorModel, ColorPicker, ColorStrip,
    ColorSwatch, Configure, Panel, RgbaF32, Shadow, Sizing, Ui, VAlign, WidgetId,
};

#[derive(Debug)]
struct State {
    picked: RgbaF32,
    port: RgbaF32,
    accent: RgbaF32,
    parts: ColorCoords,
    recent: Vec<RgbaF32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            picked: RgbaF32::hex(0x4cd3ff),
            port: RgbaF32::hex(0xffa63d),
            accent: RgbaF32::hex(0xd897ff),
            parts: ColorCoords::from_color(RgbaF32::hex(0xd9ff57), ColorModel::Okhsv, 0.0),
            recent: vec![
                RgbaF32::hex(0x4cd3ff),
                RgbaF32::hex(0xffa63d),
                RgbaF32::hex(0xd9ff57),
                RgbaF32::hex(0xd897ff),
                RgbaF32::hex(0xff5e44),
            ],
        }
    }
}

/// How many colors the app-owned swatch row keeps.
const RECENT: usize = 12;

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::colors::state");
    ui.with_state::<State, _>(state_id, |ui, state| {
        Section::new("Picker").apis(&[api!(ColorPicker::alpha), api!(ColorPicker::history)]).show(ui, |ui| {
                support::note(
                    ui,
                    "The whole panel: field, hue and alpha bars, preview, channel values, the \
                     model switch, and a swatch row the picker keeps itself. The second panel \
                     shows an app-owned swatch row instead, filled by each commit of the first.",
                );
                panels(ui, |ui| {
                    panel(ui, |ui| {
                        let picked = ColorPicker::new(&mut state.picked)
                            .alpha(true)
                            .history(true)
                            .show(ui);
                        if picked.committed {
                            if state.recent.len() == RECENT {
                                state.recent.pop();
                            }
                            state.recent.insert(0, state.picked);
                        }
                    });
                    panel(ui, |ui| {
                        ColorPicker::new(&mut state.accent)
                            .swatches(&state.recent)
                            .show(ui);
                    });
                });
            });

        Section::new("Parts").apis(&[
                api!(ColorField::new),
                api!(ColorStrip::for_hue),
                api!(ColorField::texel_size),
            ]).show(ui, |ui| {
                support::note(
                    ui,
                    "The same widgets on their own, for a layout of your own. The field and the \
                     bar share one set of color coordinates, so the bar's hue is the field's. The lower \
                     field has a texel size of 16, for the difference it makes.",
                );
                panels(ui, |ui| {
                    panel(ui, |ui| {
                        ColorField::new(&mut state.parts).show(ui);
                    });
                    panel(ui, |ui| {
                        Panel::vstack()
                            .gap(8.0)
                            .size((Sizing::fixed(180.0), Sizing::HUG))
                            .show(ui, |ui| {
                                ColorStrip::for_hue(&mut state.parts)
                                    .size((Sizing::FILL, Sizing::fixed(14.0)))
                                    .show(ui);
                                ColorSwatch::new(state.parts.to_color())
                                    .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
                                    .show(ui);
                                ColorField::new(&mut state.parts)
                                    .texel_size(16)
                                    .size((Sizing::FILL, Sizing::fixed(80.0)))
                                    .show(ui);
                            });
                    });
                });
            });

        Section::new("Chip").apis(&[api!(ColorButton::new)]).show(ui, |ui| {
            support::note(
                ui,
                "A chip that opens the panel in a popup, beside a swatch that echoes what it \
                 holds. Click outside or press Escape to close it.",
            );
            support::row(ui, |ui| {
                ColorButton::new(ColorPicker::new(&mut state.port).alpha(true).history(true))
                    .show(ui);
                ColorSwatch::new(state.port).show(ui);
            });
        });
    });
}

/// A row of [`panel`]s, top-aligned, so panels of different heights start on one line.
#[track_caller]
fn panels(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::hstack()
        .auto_id()
        .gap(support::TILE_GAP)
        .child_align(Align::v(VAlign::Top))
        .show(ui, body);
}

/// The surface a picker sits on: the panel the chip's popup wears, less its shadow, as the picker
/// lies on the page rather than floats over it. Without it a field's and a bar's colors run into
/// the page's.
#[track_caller]
fn panel(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    let theme = &ui.theme().color_picker;
    let chrome = Chrome {
        shadow: Shadow::NONE,
        ..theme.popup.clone()
    };
    let padding = theme.popup_padding;
    Panel::vstack()
        .auto_id()
        .padding(padding)
        .chrome(chrome)
        .show(ui, body);
}
