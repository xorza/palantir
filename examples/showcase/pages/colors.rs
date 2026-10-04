//! The colour family in one composition: the whole panel, the parts on their
//! own, and the chip that opens a panel of its own.
//!
//! What to look at — the field and both bars are exact per texel, so the
//! Okhsv square keeps one brightness right across the hue circle where the
//! HSV one does not. Switch the model under the panel to see the difference.

use crate::support::{api, note, row, section};
use palantir::{
    ColorButton, ColorCoords, ColorField, ColorModel, ColorPicker, ColorStrip, ColorSwatch,
    Configure, Panel, RgbaF32, Sizing, Ui, WidgetId,
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
            parts: ColorCoords::new(ColorModel::Okhsv, RgbaF32::hex(0xd9ff57), 0.0),
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

/// How many colours the app-owned swatch row keeps.
const RECENT: usize = 12;

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::colors::state");
    ui.with_state::<State, _>(state_id, |ui, state| {
        section(
            ui,
            "Picker",
            &[api!(ColorPicker::alpha), api!(ColorPicker::history)],
            |ui| {
                note(
                    ui,
                    "The whole panel: field, hue and alpha bars, preview, channel values, the \
                     model switch, and a swatch row the picker keeps itself. The second panel \
                     shows an app-owned swatch row instead, filled by each commit of the first.",
                );
                row(ui, |ui| {
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
                    ColorPicker::new(&mut state.accent)
                        .swatches(&state.recent)
                        .show(ui);
                });
            },
        );

        section(
            ui,
            "Parts",
            &[
                api!(ColorField::new),
                api!(ColorStrip::for_hue),
                api!(ColorField::texel_size),
            ],
            |ui| {
                note(
                    ui,
                    "The same widgets on their own, for a layout of your own. The field and the \
                     bar share one set of colour coordinates, so the bar's hue is the field's. The lower \
                     field has a texel size of 16, for the difference it makes.",
                );
                row(ui, |ui| {
                    ColorField::new(&mut state.parts).show(ui);
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
            },
        );

        section(ui, "Chip", &[api!(ColorButton::new)], |ui| {
            note(
                ui,
                "A chip that opens the panel in a popup, beside a swatch that echoes what it \
                 holds. Click outside or press Escape to close it.",
            );
            row(ui, |ui| {
                ColorButton::new(ColorPicker::new(&mut state.port).alpha(true).history(true))
                    .show(ui);
                ColorSwatch::new(state.port).show(ui);
            });
        });
    });
}
