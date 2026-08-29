//! The data-entry cards: the request form, the settings grid, the
//! property table, and the notes field. Between them they carry every
//! value-bound widget in the fixture and both `Grid` flavours — the
//! statically-sized one in [`settings_card`] and the row-count-driven one
//! in [`properties_card`].

use crate::frame_fixture::FrameFixture;
use crate::frame_fixture::tokens;
use crate::layout::types::align::Align;
use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::scene::node::configure::Configure;
use crate::scene::visibility::Visibility;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widgets::button::Button;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::combo_box::ComboBox;
use crate::widgets::drag_value::DragValue;
use crate::widgets::frame::Frame;
use crate::widgets::grid::Grid;
use crate::widgets::panel::Panel;
use crate::widgets::progress_bar::ProgressBar;
use crate::widgets::radio::RadioButton;
use crate::widgets::separator::Separator;
use crate::widgets::slider::Slider;
use crate::widgets::switch::Switch;
use crate::widgets::text::Text;
use crate::widgets::text_edit::TextEdit;

pub(super) fn request_card(state: &mut FrameFixture, ui: &mut Ui) {
    tokens::card(ui, "request", "REQUEST", Sizing::HUG, |ui| {
        Panel::hstack()
            .id_salt("form-row")
            .gap(8.0)
            .child_align(Align::CENTER)
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                TextEdit::new(&mut state.name)
                    .id_salt("edit-name")
                    .size((Sizing::fill(2.0), Sizing::HUG))
                    .show(ui);
                Checkbox::new(&mut state.enabled)
                    .id_salt("enabled")
                    .label("enabled")
                    .show(ui);
                for v in 0u8..3 {
                    RadioButton::new(&mut state.role, v)
                        .id_salt(("role", v))
                        .label(["read", "write", "admin"][v as usize])
                        .show(ui);
                }
                Button::new().id_salt("submit").label("Submit").show(ui);
            });
        // `Collapsed`, not `Hidden`: the validation line a real form keeps
        // recorded but out of the way while the input is valid. `Hidden`
        // would still reserve its row and leave a gap under the controls;
        // the paint-skip path it covers is exercised in the stat strip,
        // where a ZStack sibling can hide without moving anything.
        Text::new("Name is required")
            .id_salt("form-error")
            .style(&tokens::caption_style().with_color(tokens::WARN))
            .visibility(Visibility::Collapsed)
            .show(ui);
    });
}

/// Settings as a two-column `Grid` (label | control) rather than loose
/// rows, so the controls align on a real track edge — and so the fixture
/// carries a second, statically-sized Grid alongside the dynamic one in
/// [`properties_card`].
pub(super) fn settings_card(state: &mut FrameFixture, ui: &mut Ui) {
    tokens::card(ui, "settings", "SETTINGS", Sizing::HUG, |ui| {
        let rows = [Track::HUG; 6];
        Grid::new()
            .id_salt("settings-grid")
            .cols([Track::HUG.min(92.0), Track::FILL])
            .rows(rows)
            .gap_xy(8.0, 12.0)
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                Text::new("Appearance")
                    .id_salt("s-l0")
                    .style(&tokens::body_style())
                    .grid_cell((0, 0))
                    .show(ui);
                Panel::hstack()
                    .id_salt("s-c0")
                    .gap(10.0)
                    .child_align(Align::CENTER)
                    .size((Sizing::FILL, Sizing::HUG))
                    .grid_cell((0, 1))
                    .show(ui, |ui| {
                        Switch::new(&mut state.dark_mode)
                            .id_salt("dark-mode")
                            .label("dark mode")
                            .show(ui);
                        Separator::vertical().id_salt("s-vsep").show(ui);
                        let quality_opts = ["Low", "Medium", "High", "Ultra"];
                        ComboBox::new(&mut state.quality, &quality_opts)
                            .id_salt("quality")
                            .size((Sizing::fixed(140.0), Sizing::HUG))
                            .show(ui);
                    });

                // Full-width rule, drawn with a spanning cell so the grid
                // carries `grid_span` coverage in its simplest honest form.
                Frame::new()
                    .id_salt("s-rule")
                    .size((Sizing::FILL, Sizing::fixed(1.0)))
                    .background(Background {
                        fill: tokens::BORDER.into(),
                        ..Default::default()
                    })
                    .grid_cell((1, 0))
                    .grid_span((1, 2))
                    .show(ui);

                Text::new("Zoom")
                    .id_salt("s-l2")
                    .style(&tokens::body_style())
                    .grid_cell((2, 0))
                    .show(ui);
                DragValue::new(&mut state.zoom)
                    .id_salt("zoom")
                    .speed(0.5)
                    .range(0.0..=100.0)
                    .decimals(0)
                    .suffix("%")
                    .size((Sizing::fixed(90.0), Sizing::HUG))
                    .grid_cell((2, 1))
                    .show(ui);

                Text::new("Volume")
                    .id_salt("s-l3")
                    .style(&tokens::body_style())
                    .grid_cell((3, 0))
                    .show(ui);
                Slider::new(&mut state.volume, 0.0..=1.0)
                    .id_salt("volume")
                    .step(0.05)
                    .grid_cell((3, 1))
                    .show(ui);

                Text::new("Wet / dry")
                    .id_salt("s-l4")
                    .style(&tokens::body_style())
                    .grid_cell((4, 0))
                    .show(ui);
                Slider::new(&mut state.mix, 0.0..=1.0)
                    .id_salt("mix")
                    .grid_cell((4, 1))
                    .show(ui);

                Text::new("Indexing")
                    .id_salt("s-l5")
                    .style(&tokens::body_style())
                    .grid_cell((5, 0))
                    .show(ui);
                ProgressBar::new(0.62)
                    .id_salt("progress")
                    .grid_cell((5, 1))
                    .show(ui);
            });
    });
}

pub(super) fn properties_card(state: &mut FrameFixture, ui: &mut Ui, rows: usize) {
    tokens::card(ui, "props", "PROPERTIES", Sizing::HUG, |ui| {
        Grid::new()
            .id_salt("props-grid")
            .cols([Track::HUG.min(92.0), Track::FILL, Track::fixed(60.0)])
            .rows(state.grid_rows.as_slice())
            .gap_xy(2.0, 8.0)
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                const LABELS: [&str; 8] = [
                    "Name",
                    "Description",
                    "Author",
                    "License",
                    "Created",
                    "Modified",
                    "Tags",
                    "Notes",
                ];
                const VALUES: [&str; 4] = [
                    "the quick brown fox jumps over the lazy dog",
                    "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor",
                    "Jane Doe and a long author name to force wrapping",
                    "MIT-or-Apache-2.0",
                ];
                for row in 0..rows {
                    let r = row as u16;
                    // Zebra band: a full-width cell under the row's three
                    // cells. Grid children may share a cell, so this both
                    // reads as a table and covers `grid_span`.
                    if row % 2 == 0 {
                        Frame::new()
                            .id_salt(("pband", row))
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                fill: Color::hex(0x1f232d).into(),
                                corners: Corners::all(4.0),
                                ..Default::default()
                            })
                            .grid_cell((r, 0))
                            .grid_span((1, 3))
                            .show(ui);
                    }
                    Text::new(LABELS[row % LABELS.len()])
                        .id_salt(("plbl", row))
                        .style(&tokens::caption_style())
                        .margin(4.0)
                        .grid_cell((r, 0))
                        .show(ui);
                    Text::new(VALUES[row % VALUES.len()])
                        .id_salt(("pval", row))
                        .style(&tokens::body_style())
                        .text_wrap(TextWrap::Wrap)
                        .margin(4.0)
                        .grid_cell((r, 1))
                        .show(ui);
                    Button::new()
                        .id_salt(("pact", row))
                        .label("Edit")
                        .grid_cell((r, 2))
                        .show(ui);
                }
            });
    });
}

pub(super) fn notes_card(state: &mut FrameFixture, ui: &mut Ui) {
    tokens::card(ui, "notes", "NOTES", Sizing::HUG, |ui| {
        TextEdit::new(&mut state.notes)
            .id_salt("notes-edit")
            .size((Sizing::FILL, Sizing::fixed(56.0)))
            .show(ui);
    });
}
