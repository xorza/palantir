//! The assembled colour picker: the panel, what it retains between frames,
//! and the rule that decides which control's write reaches the bound colour.

use crate::layout::types::grid_cell::GridCell;
use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::primitives::color::RgbaF32;
use crate::primitives::color::color_coords::ColorCoords;
use crate::primitives::color::color_model::ColorModel;
use crate::primitives::color::srgba_u8::SrgbaU8;
use crate::primitives::num::F32Ext;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::color_field::ColorField;
use crate::widgets::color_picker::history::History;
use crate::widgets::color_strip::ColorStrip;
use crate::widgets::color_surface;
use crate::widgets::color_swatch::ColorSwatch;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ConfigureWidget;
use crate::widgets::drag_value::DragValue;
use crate::widgets::grid::Grid;
use crate::widgets::panel::Panel;
use crate::widgets::radio::RadioButton;
use crate::widgets::response::Response;
use crate::widgets::text::Text;
use crate::widgets::text_edit::TextEdit;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use crate::widgets::value_response::ValueResponse;
use crate::widgets::widget::Widget;
use std::fmt::Write as _;
use std::rc::Rc;

mod history;

/// A colour picker: the saturation/value field, a hue bar, an optional alpha
/// bar, a preview chip, the channel values, the model switch and a swatch
/// row.
///
/// Arranges the family's four other widgets and owns nothing they do not — an
/// app wanting a different layout takes [`ColorField`], [`ColorStrip`] and
/// [`ColorSwatch`] and builds it.
///
/// # What writes what
///
/// Each control writes only the part of the colour it owns, and the picker
/// never rebuilds the rest from its axes. That matters at more than the
/// margins: a small wedge of sRGB around pure blue is outside the Okhsv cube
/// (see [`Okhsv`](crate::Okhsv)), so a picker that rebuilt the colour every
/// time the opacity moved would quietly shift `#0000ff` to `#0038ff`.
///
/// # Retained state
///
/// The axes, the hex text and the history live in the response map, keyed off
/// this widget's id. The axes are retained rather than re-derived because
/// black has no hue and grey has no saturation: a picker that read them back
/// from the colour every frame would lose the hue the moment the value
/// reached zero. They *are* re-derived when the bound colour changes from
/// outside, which is how a caller's own edit moves the handles.
#[derive(Debug)]
pub struct ColorPicker<'a> {
    widget: Widget,
    color: &'a mut RgbaF32,
    alpha: bool,
    model: Option<ColorModel>,
    swatches: Swatches<'a>,
    downsample: u32,
    style: Option<&'a ColorPickerTheme>,
}

/// Where the swatch row's colours come from, if it shows at all.
///
/// One field with two setters rather than two fields with a precedence rule:
/// the last call wins and no combination can conflict.
#[derive(Debug)]
enum Swatches<'a> {
    /// No row. The default.
    Hidden,
    /// The picker keeps its own, seeded with presets.
    Owned,
    /// The app owns the row and the picker only reads it.
    Given(&'a [RgbaF32]),
}

/// What one picker keeps between frames.
#[derive(Debug, Default)]
struct PickerState {
    coords: ColorCoords,
    /// The colour this picker last wrote. Any other value in the binding is
    /// an edit from outside, and re-seeds the axes.
    written: RgbaF32,
    /// Whether `written` has ever been written. Without it a picker bound to
    /// a transparent black would read its own default as a match and never
    /// seed.
    seeded: bool,
    /// The hex field's buffer, rewritten from the colour whenever the field
    /// does not hold focus.
    hex: String,
    history: History,
}

impl<'a> ColorPicker<'a> {
    /// A picker bound to `color`.
    #[track_caller]
    pub fn new(color: &'a mut RgbaF32) -> Self {
        Self {
            widget: Widget::vstack(),
            color,
            alpha: false,
            model: None,
            swatches: Swatches::Hidden,
            downsample: color_surface::DOWNSAMPLE,
            style: None,
        }
    }

    /// Show the alpha bar and the opacity value. Off by default: most colours
    /// an app picks are opaque, and a bar for an axis nobody moves is one
    /// more thing to read past.
    pub fn alpha(mut self, on: bool) -> Self {
        self.alpha = on;
        self
    }

    /// Pin the model instead of letting the user switch it. The switch itself
    /// only shows when the model is not pinned.
    pub fn model(mut self, model: ColorModel) -> Self {
        self.model = Some(model);
        self
    }

    /// Show a swatch row the picker fills itself: the preset colours, with
    /// each committed pick moving to the front. Replaces
    /// [`swatches`](Self::swatches).
    pub fn history(mut self, on: bool) -> Self {
        self.swatches = if on {
            Swatches::Owned
        } else {
            Swatches::Hidden
        };
        self
    }

    /// Show a swatch row the app owns. Clicking one picks it; the picker
    /// never writes to the slice. Replaces [`history`](Self::history).
    pub fn swatches(mut self, colors: &'a [RgbaF32]) -> Self {
        self.swatches = Swatches::Given(colors);
        self
    }

    /// How far below the display's resolution the field and bars are built.
    /// See [`ColorField::downsample`].
    ///
    /// # Panics
    ///
    /// Panics unless `n` is a power of two from 1 to 16.
    pub fn downsample(mut self, n: u32) -> Self {
        self.downsample = color_surface::checked_downsample(n);
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `color_picker`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    pub fn style(mut self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the panel and report what it did to the bound colour.
    pub fn show(self, ui: &mut Ui) -> ValueResponse<'_> {
        // An `Rc` bump on the theme bundle, so the rows can borrow their
        // styles out of it across the `&mut Ui` the record below takes.
        let theme = Rc::clone(ui.theme());
        let slot = self.style.unwrap_or(&theme.color_picker);
        let gap = slot.gap.themed_length(0.0);

        // The panel is as wide as its field and no wider. Every row below is
        // `FILL` inside that, which is what keeps the value grid's columns a
        // fixed width instead of one the digits inside them push around.
        let mut widget = self.widget.gap(gap).default_size((
            Sizing::fixed(slot.field_width.themed_length(1.0)),
            Sizing::HUG,
        ));
        let response = widget.response(ui);
        let id = widget.resolve(ui);

        let color = self.color;
        let alpha_on = self.alpha;
        let pinned = self.model;
        let swatches = self.swatches;
        let downsample = self.downsample;

        let mut edit = Edit::default();
        widget.record(ui, None, |ui| {
            ui.with_state::<PickerState, _>(id, |ui, state| {
                edit = body(
                    ui,
                    state,
                    Inputs {
                        id,
                        theme: slot,
                        color,
                        alpha_on,
                        pinned,
                        swatches,
                        downsample,
                    },
                );
            });
        });
        ValueResponse {
            response: Response::eager(id, ui, response),
            changed: edit.changed,
            committed: edit.committed,
        }
    }
}

impl Configure for ColorPicker<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

/// Everything the panel body needs that is not the retained state.
///
/// A bundle rather than seven arguments: the body is one function because
/// the state borrow has to span every row.
#[derive(Debug)]
struct Inputs<'a> {
    id: WidgetId,
    theme: &'a ColorPickerTheme,
    color: &'a mut RgbaF32,
    alpha_on: bool,
    pinned: Option<ColorModel>,
    swatches: Swatches<'a>,
    downsample: u32,
}

/// Columns the value grid is built on. Four, so the hex field spans two and
/// every channel box lands on the same width as the one above it.
const VALUE_COLUMNS: usize = 4;

/// Space between a channel's caption and its value.
const LABEL_GAP: f32 = 2.0;

/// What one frame of the panel — or one cell of it — did to the colour.
#[derive(Debug, Default)]
struct Edit {
    changed: bool,
    committed: bool,
}

/// Which control wrote, and so which part of the colour to rebuild.
#[derive(Debug, Default)]
struct Writes {
    /// The field, the hue bar, or the H / S values moved.
    axes: bool,
    /// The alpha bar or the opacity value moved.
    alpha: f32,
    alpha_moved: bool,
    /// The hex field, an RGB value or a swatch named a colour outright.
    exact: Option<RgbaF32>,
    committed: bool,
}

fn body(ui: &mut Ui, state: &mut PickerState, inputs: Inputs<'_>) -> Edit {
    let Inputs {
        id,
        theme,
        color,
        alpha_on,
        pinned,
        swatches,
        downsample,
    } = inputs;
    let gap = theme.gap.themed_length(0.0);
    let bar = theme.bar_thickness.themed_length(1.0);
    let chip = theme.chip_size.themed_length(1.0);

    // An edit from outside moves the handles; the picker's own writes do not
    // come back through here, which is what lets black keep its hue.
    if !state.seeded || *color != state.written {
        let model = pinned.unwrap_or_else(|| state.coords.model());
        state.coords = ColorCoords::new(model, *color, state.coords.hue());
        state.written = *color;
        state.seeded = true;
    }
    if let Some(model) = pinned {
        state.coords = state.coords.with_model(model);
    }

    let mut writes = Writes {
        alpha: color.a,
        ..Writes::default()
    };

    let field = ColorField::new(&mut state.coords)
        .downsample(downsample)
        .id(id.with("field"))
        .show(ui);
    writes.axes |= field.changed;
    writes.committed |= field.committed;

    let preview = state.coords.to_color().with_alpha(writes.alpha);
    Panel::hstack()
        .id(id.with("bars"))
        .gap(gap)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            ColorSwatch::new(preview)
                .id(id.with("preview"))
                .size((Sizing::fixed(chip), Sizing::fixed(chip)))
                .show(ui);
            Panel::vstack()
                .id(id.with("bar-stack"))
                .gap(gap)
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui, |ui| {
                    let hue = ColorStrip::hue(&mut state.coords)
                        .downsample(downsample)
                        .id(id.with("hue"))
                        .size((Sizing::FILL, Sizing::fixed(bar)))
                        .show(ui);
                    writes.axes |= hue.changed;
                    writes.committed |= hue.committed;
                    if alpha_on {
                        let mut working = preview;
                        let strip = ColorStrip::alpha(&mut working)
                            .downsample(downsample)
                            .id(id.with("alpha"))
                            .size((Sizing::FILL, Sizing::fixed(bar)))
                            .show(ui);
                        if strip.changed {
                            writes.alpha = working.a;
                            writes.alpha_moved = true;
                        }
                        writes.committed |= strip.committed;
                    }
                });
        });

    values_grid(ui, state, id, theme, alpha_on, gap, &mut writes);
    if pinned.is_none() {
        model_switch(ui, state, id, gap);
    }
    swatch_row(ui, state, id, &swatches, gap, &mut writes);

    apply(state, color, writes)
}

/// Fold this frame's writes into the bound colour, rebuilding only the part
/// the control that moved actually owns.
fn apply(state: &mut PickerState, color: &mut RgbaF32, writes: Writes) -> Edit {
    let next = if let Some(exact) = writes.exact {
        exact
    } else if writes.axes {
        state.coords.to_color().with_alpha(writes.alpha)
    } else if writes.alpha_moved {
        color.with_alpha(writes.alpha)
    } else {
        *color
    };
    let changed = next != *color;
    if changed {
        *color = next;
        state.written = next;
        if writes.exact.is_some() {
            let model = state.coords.model();
            state.coords = ColorCoords::new(model, next, state.coords.hue());
        }
    }
    if writes.committed {
        state.history.push(*color);
    }
    Edit {
        changed,
        committed: writes.committed,
    }
}

/// The hex field and the six channel values, in one grid of four equal
/// columns.
///
/// A grid rather than two rows of flexed cells, because the columns have to
/// agree between the rows: a value box that changed width under a drag —
/// or sat a gap's width off the one above it — would make a drag read as the
/// row rearranging itself rather than as one number changing. The mono face
/// the theme puts on them finishes the job at the digit level.
fn values_grid(
    ui: &mut Ui,
    state: &mut PickerState,
    id: WidgetId,
    theme: &ColorPickerTheme,
    alpha_on: bool,
    gap: f32,
    writes: &mut Writes,
) {
    let color = state.coords.to_color().with_alpha(writes.alpha);
    let quantized = color.to_srgba_u8();
    let hex_id = id.with("hex");
    if ui.focused_id() != Some(hex_id) {
        state.hex.clear();
        let _ = write!(
            state.hex,
            "#{:02X}{:02X}{:02X}",
            quantized.r, quantized.g, quantized.b,
        );
    }

    let mut rgb = [
        i64::from(quantized.r),
        i64::from(quantized.g),
        i64::from(quantized.b),
    ];
    let mut opacity = (writes.alpha * 100.0).round() as i64;
    let mut hue = (state.coords.hue() * 360.0).round() as i64;
    let mut sat = (state.coords.sat() * 100.0).round() as i64;

    Grid::new()
        .id(id.with("values"))
        .cols([Track::FILL; VALUE_COLUMNS])
        .rows([Track::HUG; 2])
        .gap(gap)
        .line_gap(gap)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            Panel::vstack()
                .id(id.with("hex-cell"))
                .gap(LABEL_GAP)
                .grid_cell(GridCell::at(0, 0).span(1, 2))
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui, |ui| {
                    Text::new("HEX")
                        .style(&theme.label)
                        .id(id.with("hex-label"))
                        .show(ui);
                    let hex = TextEdit::new(&mut state.hex)
                        .max_chars(7)
                        .style(&theme.hex)
                        .id(hex_id)
                        .size((Sizing::FILL, Sizing::HUG))
                        .show(ui);
                    if !hex.cancelled
                        && (hex.submitted || hex.lost_focus)
                        && let Ok(parsed) = state.hex.trim().parse::<RgbaF32>()
                    {
                        writes.exact = Some(parsed.with_alpha(writes.alpha));
                        writes.committed = true;
                    }
                });

            if alpha_on {
                let cell = GridCell::at(0, 2);
                let r = value_cell(ui, id, theme, "A %", cell, &mut opacity, 100.0);
                if r.changed {
                    writes.alpha = opacity as f32 / 100.0;
                    writes.alpha_moved = true;
                }
                writes.committed |= r.committed;
            }

            let r = value_cell(ui, id, theme, "H °", GridCell::at(0, 3), &mut hue, 360.0);
            if r.changed {
                state.coords.set_hue(hue as f32 / 360.0);
                writes.axes = true;
            }
            writes.committed |= r.committed;

            for (column, name) in ["R", "G", "B"].into_iter().enumerate() {
                let cell = GridCell::at(1, column as u16);
                let r = value_cell(ui, id, theme, name, cell, &mut rgb[column], 255.0);
                if r.changed {
                    let channel = |v: i64| v.clamp(0, 255) as u8;
                    writes.exact = Some(
                        RgbaF32::from_srgba(SrgbaU8::rgb(
                            channel(rgb[0]),
                            channel(rgb[1]),
                            channel(rgb[2]),
                        ))
                        .with_alpha(writes.alpha),
                    );
                }
                writes.committed |= r.committed;
            }

            let r = value_cell(ui, id, theme, "S %", GridCell::at(1, 3), &mut sat, 100.0);
            if r.changed {
                state.coords.set_sat(sat as f32 / 100.0);
                writes.axes = true;
            }
            writes.committed |= r.committed;
        });
}

/// One grid cell: the channel's caption over the value it names, a drag from
/// zero to `top`, so the number gets the column's whole width.
///
/// The caption carries the unit — `A %`, `H °` — rather than the value
/// carrying a suffix. A three-digit number and a suffix do not both fit a
/// quarter of the panel, and the unit is the half that never changes.
fn value_cell(
    ui: &mut Ui,
    id: WidgetId,
    theme: &ColorPickerTheme,
    caption: &'static str,
    cell: GridCell,
    value: &mut i64,
    top: f64,
) -> Edit {
    // Keyed on the channel letter alone: the caption carries the unit too,
    // and an id that moved when a unit changed would drop the widget's state.
    let cell_id = id.with(&caption[..1]);
    let mut edit = Edit::default();
    Panel::vstack()
        .id(cell_id)
        .gap(LABEL_GAP)
        .grid_cell(cell)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            Text::new(caption)
                .style(&theme.label)
                .id(cell_id.with("label"))
                .show(ui);
            let r = DragValue::new(value)
                .range(0.0..=top)
                .style(&theme.value)
                .id(cell_id.with("value"))
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui);
            edit.changed = r.changed;
            edit.committed = r.committed;
        });
    edit
}

/// The two models, as one either-or choice.
fn model_switch(ui: &mut Ui, state: &mut PickerState, id: WidgetId, gap: f32) {
    let mut model = state.coords.model();
    Panel::hstack()
        .id(id.with("models"))
        .gap(gap)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for choice in ColorModel::ALL {
                RadioButton::new(&mut model, choice)
                    .label(choice.label())
                    .id(id.with(choice.label()))
                    .show(ui);
            }
        });
    state.coords = state.coords.with_model(model);
}

/// The preset or history row. Clicking a swatch picks it outright.
fn swatch_row(
    ui: &mut Ui,
    state: &mut PickerState,
    id: WidgetId,
    swatches: &Swatches<'_>,
    gap: f32,
    writes: &mut Writes,
) {
    let colors: &[RgbaF32] = match swatches {
        Swatches::Hidden => return,
        Swatches::Owned => state.history.colors(),
        Swatches::Given(given) => given,
    };
    let mut picked = None;
    Panel::wrap_hstack()
        .id(id.with("swatches"))
        .gap(gap)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for (index, color) in colors.iter().enumerate() {
                if ColorSwatch::new(*color)
                    .id(id.with("swatch").with(index))
                    .show(ui)
                    .left
                    .clicked()
                {
                    picked = Some(*color);
                }
            }
        });
    if let Some(color) = picked {
        writes.exact = Some(color);
        writes.alpha = color.a;
        writes.committed = true;
    }
}

#[cfg(test)]
mod tests;
