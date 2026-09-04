//! The colour family in one composition: the whole panel, the parts on their
//! own, and the chip that opens a panel of its own.
//!
//! What to look at — the field and both bars are exact per texel, so the
//! Okhsv square keeps one brightness right across the hue circle where the
//! HSV one does not. Switch the model under the panel to see the difference.

use crate::support::{note_style, row, section};
use palantir::{
    RgbaF32, ColorButton, ColorCoords, ColorField, ColorModel, ColorPicker, ColorStrip, ColorSwatch,
    Configure, Panel, Sizing, Text, Ui, WidgetId,
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

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::colors::state");
    ui.with_state::<State, _>(state_id, |ui, state| {
        section(ui, "PICKER", |ui| {
            Text::new(
                "The whole panel: field, hue and alpha bars, preview, channel values, the \
                 model switch and a swatch row the picker keeps itself.",
            )
            .style(&note_style())
            .show(ui);
            row(ui, |ui| {
                let picked = ColorPicker::new(&mut state.picked)
                    .alpha(true)
                    .history(true)
                    .id(state_id.with("panel"))
                    .show(ui);
                if picked.committed {
                    state.recent.push(state.picked);
                    state.recent.truncate(12);
                }
                Panel::vstack()
                    .id(state_id.with("app-row"))
                    .gap(8.0)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        Text::new("An app-owned swatch row instead of the picker's own:")
                            .style(&note_style())
                            .show(ui);
                        ColorPicker::new(&mut state.accent)
                            .swatches(&state.recent)
                            .id(state_id.with("app-panel"))
                            .show(ui);
                    });
            });
        });

        section(ui, "PARTS", |ui| {
            Text::new(
                "The same widgets on their own, for a layout of your own. The field and the \
                 bar share one ColorCoords, so the bar's hue is the field's.",
            )
            .style(&note_style())
            .show(ui);
            row(ui, |ui| {
                ColorField::new(&mut state.parts)
                    .id(state_id.with("field"))
                    .show(ui);
                Panel::vstack()
                    .id(state_id.with("part-bars"))
                    .gap(8.0)
                    .size((Sizing::fixed(180.0), Sizing::HUG))
                    .show(ui, |ui| {
                        ColorStrip::hue(&mut state.parts)
                            .id(state_id.with("hue"))
                            .size((Sizing::FILL, Sizing::fixed(14.0)))
                            .show(ui);
                        ColorSwatch::new(state.parts.to_color())
                            .id(state_id.with("part-chip"))
                            .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
                            .show(ui);
                        Text::new("A downsample of 16, for the difference it makes:")
                            .style(&note_style())
                            .show(ui);
                        ColorField::new(&mut state.parts)
                            .downsample(16)
                            .id(state_id.with("coarse"))
                            .size((Sizing::FILL, Sizing::fixed(80.0)))
                            .show(ui);
                    });
            });
        });

        section(ui, "CHIP", |ui| {
            Text::new(
                "A chip that opens the panel in a popup. Click outside or press Escape to \
                 close it.",
            )
            .style(&note_style())
            .show(ui);
            row(ui, |ui| {
                ColorButton::new(&mut state.port)
                    .alpha(true)
                    .id(state_id.with("chip"))
                    .show(ui);
                ColorSwatch::new(state.port)
                    .id(state_id.with("chip-echo"))
                    .show(ui);
                Text::new("the chip and an echo of what it holds")
                    .style(&note_style())
                    .show(ui);
            });
        });
    });
}
