//! The color family in one composition: the whole panel, the parts alone, and the chip that opens its own panel.
//!
//! The field and both bars are exact per texel, so the Okhsv square keeps one brightness across the hue circle where HSV does not; switch the model under the panel to compare.

use crate::appearance::Appearance;
use crate::scaffold::api;
use crate::support;
use crate::support::{Kit, Section, Swatches};
use palantir::{
    Align, Button, ColorButton, ColorCoords, ColorField, ColorModel, ColorPicker, ColorStrip,
    ColorSwatch, Configure, Metrics, Panel, RgbaF32, Sizing, SwatchRow, Ui, VAlign, WidgetId,
};

const M: Metrics = Metrics::DEFAULT;

#[derive(Debug)]
struct State {
    picked: RgbaF32,
    port: RgbaF32,
    accent: RgbaF32,
    parts: ColorCoords,
    recent: Vec<RgbaF32>,
}

impl State {
    /// The pickers open on the tour's specimen colors, as the scheme that first shows solves them.
    fn new(swatch: Swatches) -> Self {
        Self {
            picked: swatch.teal,
            port: swatch.orange,
            accent: swatch.violet,
            parts: ColorCoords::from_color(swatch.lime, ColorModel::Okhsv, 0.0),
            recent: swatch.all().to_vec(),
        }
    }
}

/// How many colors the app-owned swatch row keeps.
const RECENT: usize = 12;

pub(crate) fn build(ui: &mut Ui, k: Kit, look: &mut Appearance) {
    let state_id = WidgetId::from_hash("showcase::colors::state");
    ui.with_state::<Option<State>, _>(state_id, |ui, state| {
        let state = state.get_or_insert_with(|| State::new(k.swatch));
        Section::new("Picker").apis(&[api!(ColorPicker::alpha), api!(ColorPicker::swatches)]).show(ui, k, |ui| {
                support::note(ui, k,
                    "The whole panel: field, hue and alpha bars, preview, channel values, the \
                     model switch, and a swatch row the picker keeps itself. The second panel \
                     shows an app-owned swatch row instead, filled by each commit of the first.",
                );
                panels(ui, |ui| {
                    panel(ui, k, |ui| {
                        let picked = ColorPicker::new(&mut state.picked)
                            .alpha(true)
                            .swatches(SwatchRow::History)
                            .show(ui);
                        if picked.committed {
                            if state.recent.len() == RECENT {
                                state.recent.pop();
                            }
                            state.recent.insert(0, state.picked);
                        }
                    });
                    panel(ui, k, |ui| {
                        ColorPicker::new(&mut state.accent)
                            .swatches(SwatchRow::Fixed(&state.recent))
                            .show(ui);
                    });
                });
                // The first panel's color as the tour's accent seed, solved per scheme.
                support::row(ui, |ui| {
                    if Button::new().label("Use as accent").show(ui).clicked() {
                        look.set_seed(Some(state.picked));
                    }
                    if Button::new()
                        .label("Stock accent")
                        .disabled(look.seed().is_none())
                        .show(ui)
                        .clicked()
                    {
                        look.set_seed(None);
                    }
                });
            });

        Section::new("Parts").apis(&[
                api!(ColorField::new),
                api!(ColorStrip::hue),
                api!(ColorField::texel_size),
            ]).show(ui, k, |ui| {
                support::note(ui, k,
                    "The same widgets on their own, for a layout of your own. The field and the \
                     bar share one set of color coordinates, so the bar's hue is the field's. The lower \
                     field has a texel size of 16, for the difference it makes.",
                );
                panels(ui, |ui| {
                    panel(ui, k, |ui| {
                        ColorField::new(&mut state.parts).show(ui);
                    });
                    panel(ui, k, |ui| {
                        Panel::vstack()
                            .gap(M.space.sm)
                            .size((Sizing::fixed(180.0), Sizing::HUG))
                            .show(ui, |ui| {
                                ColorStrip::hue(&mut state.parts)
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

        Section::new("Chip").apis(&[api!(ColorButton::new)]).show(ui, k, |ui| {
            support::note(ui, k,
                "A chip that opens the panel in a popup, beside a swatch that echoes what it \
                 holds. Click outside or press Escape to close it.",
            );
            support::row(ui, |ui| {
                ColorButton::new(ColorPicker::new(&mut state.port).alpha(true).swatches(SwatchRow::History))
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

/// The surface a picker sits on: the tour's raised card, padded as the chip's popup pads the
/// panel. Refused: the popup's own chrome, whose float material would capture and blur the page
/// under a panel that does not float. Without a surface a field's and a bar's colors run into the
/// page's.
#[track_caller]
fn panel(ui: &mut Ui, k: Kit, body: impl FnOnce(&mut Ui)) {
    let padding = ui.theme().color_picker.popup.padding;
    Panel::vstack()
        .auto_id()
        .padding(padding)
        .chrome(support::raised_chrome(k))
        .show(ui, body);
}
