//! The assembled color picker: the panel, its retained state, and the rule
//! deciding which control's write reaches the bound color.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::layout::track::Track;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::color_coords::ColorCoords;
use crate::primitives::paint::color::color_model::ColorModel;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::theme::color_picker::ColorPickerTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureDefaults;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::color_field::ColorField;
use crate::widgets::color_picker::history::History;
use crate::widgets::color_picker::swatch_row::SwatchRow;
use crate::widgets::color_strip::ColorStrip;
use crate::widgets::color_surface;
use crate::widgets::color_swatch::ColorSwatch;
use crate::widgets::drag_value::DragValue;
use crate::widgets::grid::Grid;
use crate::widgets::panel::Panel;
use crate::widgets::radio_group::RadioGroup;
use crate::widgets::text::Text;
use crate::widgets::text_edit::TextEdit;
use std::num::NonZeroU16;
use std::rc::Rc;

mod history;
pub(crate) mod swatch_row;

/// A color picker: the saturation/value field, a hue bar, an optional alpha
/// bar, a preview chip, the channel values, the model switch and a swatch row.
///
/// Arranges the family's other widgets; an app wanting a different layout
/// builds it from [`ColorField`], [`ColorStrip`] and [`ColorSwatch`].
///
/// # What writes what
///
/// Each control writes only the part of the color it owns; the picker never
/// rebuilds the rest from its axes. A wedge of sRGB around pure blue lies
/// outside the Okhsv cube (see [`ColorModel::Okhsv`]), so rebuilding on an
/// opacity change would shift `#0000ff` to `#0037ff`.
///
/// # Retained state
///
/// The axes and history live in the response map, and the hex field's draft in the field's own row, keyed off this
/// widget's id. The axes are retained because black has no hue and gray no
/// saturation; they are re-derived when the bound color changes from outside.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct ColorPicker<'a> {
    widget: Widget,
    body: PickerBody<'a>,
    style: Option<&'a ColorPickerTheme>,
}

/// What the panel's rows record from, besides the retained state.
#[derive(Debug)]
struct PickerBody<'a> {
    color: &'a mut RgbaF32,
    alpha_on: bool,
    pinned: Option<ColorModel>,
    swatches: SwatchRow<'a>,
    texel_size: u32,
}

/// What one picker keeps between frames.
#[derive(Debug, Default)]
struct PickerState {
    coords: ColorCoords,
    /// The color this picker last wrote or seeded from. Any other value in the
    /// binding is an outside edit and re-seeds the axes.
    written: Option<RgbaF32>,
    history: History,
}

impl<'a> ColorPicker<'a> {
    /// A picker bound to `color`.
    #[track_caller]
    pub fn new(color: &'a mut RgbaF32) -> Self {
        Self {
            widget: Widget::vstack(),
            body: PickerBody {
                color,
                alpha_on: false,
                pinned: None,
                swatches: SwatchRow::None,
                texel_size: color_surface::TEXEL_SIZE,
            },
            style: None,
        }
    }

    /// Show the alpha bar and the opacity value. Off by default.
    pub const fn alpha(mut self, alpha: bool) -> Self {
        self.body.alpha_on = alpha;
        self
    }

    /// Pin the model instead of letting the user switch it; the switch hides.
    pub const fn model(mut self, model: ColorModel) -> Self {
        self.body.pinned = Some(model);
        self
    }

    /// The swatch row under the panel. [`SwatchRow::None`] by default.
    pub const fn swatches(mut self, row: SwatchRow<'a>) -> Self {
        self.body.swatches = row;
        self
    }

    /// The edge of one texel of the field and bars, in physical pixels; see
    /// [`ColorField::texel_size`].
    ///
    /// # Panics
    ///
    /// Panics unless `n` is a power of two from 1 to 16.
    #[track_caller]
    pub const fn texel_size(mut self, n: u32) -> Self {
        self.body.texel_size = domain::power_of_two_in(n, color_surface::MAX_TEXEL_SIZE);
        self
    }

    /// Per-instance override of [`Theme::color_picker`](crate::Theme::color_picker).
    pub fn style(mut self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// The bound color before `show`, for a [`ColorButton`](crate::ColorButton) chip.
    pub(crate) const fn color(&self) -> RgbaF32 {
        *self.body.color
    }

    /// Record the panel and report what it did to the bound color.
    pub fn show(self, ui: &mut Ui) -> ValueResponse {
        // An `Rc` bump so rows can borrow styles across the `&mut Ui` record.
        let theme = Rc::clone(ui.theme());
        let slot = self.style.unwrap_or(&theme.color_picker);
        let gap = domain::length_at_least(slot.gap, 0.0);

        // As wide as the field; every row is `FILL` inside, keeping the value
        // grid's columns a fixed width.
        let Self { widget, body, .. } = self;
        let mut widget = widget.default_gap(gap).default_size((
            Sizing::fixed(domain::length_at_least(slot.field_width, 1.0)),
            Sizing::HUG,
        ));
        let response = widget.response(ui);
        let id = response.id;
        let panel = PanelLook { theme: slot, gap };
        let edits = widget.record(ui, None, |ui| {
            ui.with_state::<PickerState, _>(id, |ui, state| body.record(ui, state, id, panel))
        });
        ValueResponse {
            response,
            changed: edits.changed,
            committed: edits.committed,
        }
    }
}

impl Configure for ColorPicker<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// What the panel's rows did to the bound color this frame.
#[derive(Clone, Copy, Debug)]
struct PanelEdits {
    changed: bool,
    committed: bool,
}

/// The theme every row wears, and its gap, checked once.
#[derive(Clone, Copy, Debug)]
struct PanelLook<'a> {
    theme: &'a ColorPickerTheme,
    gap: f32,
}

/// Columns the value grid is built on. Four, so the hex field spans two and
/// every channel box matches the one above it.
const VALUE_COLS: usize = 4;

/// Space between a channel's caption and its value.
const LABEL_GAP: f32 = 2.0;

/// Which control wrote, and so which part of the color to rebuild.
#[derive(Clone, Copy, Debug, Default)]
struct Writes {
    /// The field, the hue bar, or the H / S / V values moved.
    axes: bool,
    /// The alpha bar or the opacity value moved.
    alpha: f32,
    alpha_moved: bool,
    /// The hex field, an RGB value or a swatch named a color outright.
    exact: Option<RgbaF32>,
    committed: bool,
    /// A pick the history keeps: a released pointer gesture, a committed typed
    /// value, a swatch click. Not a keyboard nudge, as sixteen arrow presses
    /// would evict the row with near-identical shades.
    remember: bool,
}

impl Writes {
    /// A pointer surface's frame: its commit, which the history keeps when a release made it.
    const fn gestured(&mut self, edit: &ValueResponse) {
        self.committed |= edit.committed;
        self.remember |= edit.committed && edit.response.left.released();
    }

    /// A channel value's commit; the history keeps it only as a pick.
    const fn typed(&mut self, cell: &CellWrite) {
        self.committed |= cell.edit.committed;
        self.remember |= cell.edit.committed && cell.pick;
    }
}

/// A channel value's frame: what it reported, and whether a commit in it is a pick the history
/// keeps, a scrub's release or the editor's commit, rather than a key's nudge on the chip.
#[derive(Clone, Copy, Debug)]
struct CellWrite {
    edit: ValueResponse,
    pick: bool,
}

/// One channel's grid cell: its caption, where it sits, and the top of its
/// `0..=top` drag. The caption carries the unit (`A %`, `H °`) so the number gets
/// the column's whole width.
#[derive(Clone, Copy, Debug)]
struct ValueCell {
    caption: &'static str,
    cell: GridCell,
    top: f64,
}

const OPACITY: ValueCell = ValueCell {
    caption: "A %",
    cell: GridCell::at(0, 2),
    top: 100.0,
};

const VALUE: ValueCell = ValueCell {
    caption: "V %",
    cell: GridCell::at(0, 2),
    top: 100.0,
};

const HUE: ValueCell = ValueCell {
    caption: "H °",
    cell: GridCell::at(0, 3),
    top: 360.0,
};

const SATURATION: ValueCell = ValueCell {
    caption: "S %",
    cell: GridCell::at(1, 3),
    top: 100.0,
};

const CHANNELS: [ValueCell; 3] = [
    ValueCell {
        caption: "R",
        cell: GridCell::at(1, 0),
        top: 255.0,
    },
    ValueCell {
        caption: "G",
        cell: GridCell::at(1, 1),
        top: 255.0,
    },
    ValueCell {
        caption: "B",
        cell: GridCell::at(1, 2),
        top: 255.0,
    },
];

/// The color the panel shows: the bound one unless an axis moved this frame.
/// Built from the axes alone it would show their clamped reading (`#0000ff`
/// seeds Okhsv at s = v = 1, which is `#0037ff`).
fn shown(state: &PickerState, bound: RgbaF32, writes: &Writes) -> RgbaF32 {
    let color = if writes.axes {
        state.coords.to_color()
    } else {
        bound
    };
    color.with_alpha(writes.alpha)
}

/// Whether two colors have the same sRGB channels, alpha aside.
fn same_rgb(a: SrgbaU8, b: SrgbaU8) -> bool {
    (a.r, a.g, a.b) == (b.r, b.g, b.b)
}

impl PickerBody<'_> {
    /// Record the panel's rows over `state`, the picker `id`'s, and answer what they did to the
    /// bound color.
    fn record(
        self,
        ui: &mut Ui,
        state: &mut PickerState,
        id: WidgetId,
        PanelLook { theme, gap }: PanelLook<'_>,
    ) -> PanelEdits {
        let Self {
            color,
            alpha_on,
            pinned,
            swatches,
            texel_size,
        } = self;
        let bar = domain::length_at_least(theme.bar_thickness, 1.0);
        let chip = domain::length_at_least(theme.chip_size, 1.0);

        // An outside edit moves the handles; the picker's own writes do not come
        // back through here, which lets black keep its hue.
        if state.written != Some(*color) {
            let model = pinned.unwrap_or_else(|| state.coords.model());
            state.coords = ColorCoords::from_color(*color, model, state.coords.hue());
            state.written = Some(*color);
        }
        if let Some(model) = pinned {
            state.coords = state.coords.with_model(model);
        }

        let mut writes = Writes {
            alpha: color.a,
            ..Writes::default()
        };

        let field = ColorField::new(&mut state.coords)
            .texel_size(texel_size)
            .style(theme)
            .id(id.with("field"))
            .show(ui);
        writes.axes |= field.changed;
        writes.gestured(&field);

        let bound = *color;
        let preview = shown(state, bound, &writes);
        Panel::hstack()
            .id(id.with("bars"))
            .gap(gap)
            .size(SizeSpec::FILL_W)
            .show(ui, |ui| {
                ColorSwatch::new(preview)
                    .style(theme)
                    .id(id.with("preview"))
                    .sense(Sense::NONE)
                    .focusable(false)
                    .size((Sizing::fixed(chip), Sizing::fixed(chip)))
                    .show(ui);
                Panel::vstack()
                    .id(id.with("bar-stack"))
                    .gap(gap)
                    .size(SizeSpec::FILL_W)
                    .show(ui, |ui| {
                        let hue = ColorStrip::hue(&mut state.coords)
                            .texel_size(texel_size)
                            .style(theme)
                            .id(id.with("hue"))
                            .size((Sizing::FILL, Sizing::fixed(bar)))
                            .show(ui);
                        writes.axes |= hue.changed;
                        writes.gestured(&hue);
                        if alpha_on {
                            let mut working = preview;
                            let strip = ColorStrip::alpha(&mut working)
                                .texel_size(texel_size)
                                .style(theme)
                                .id(id.with("alpha"))
                                .size((Sizing::FILL, Sizing::fixed(bar)))
                                .show(ui);
                            if strip.changed {
                                writes.alpha = working.a;
                                writes.alpha_moved = true;
                            }
                            writes.gestured(&strip);
                        }
                    });
            });

        let readout = shown(state, bound, &writes);
        let mut rows = Rows {
            state,
            id,
            theme,
            gap,
            writes,
        };
        rows.values_grid(ui, alpha_on, readout);
        if pinned.is_none() {
            rows.model_switch(ui);
        }
        rows.swatch_row(ui, swatches);

        let Rows { state, writes, .. } = rows;
        PanelEdits {
            changed: apply(state, color, writes),
            committed: writes.committed,
        }
    }
}

/// The rows under the bars, sharing the retained state and this frame's writes.
#[derive(Debug)]
struct Rows<'a> {
    state: &'a mut PickerState,
    id: WidgetId,
    theme: &'a ColorPickerTheme,
    gap: f32,
    writes: Writes,
}

impl Rows<'_> {
    /// The hex field and six channel values, in one grid of four equal columns, so
    /// columns agree between rows: a value box changing width under a drag would
    /// read as the row rearranging. The theme's mono face finishes the job.
    #[expect(
        clippy::cast_sign_loss,
        reason = "the channel is clamped to 0..=255 before the cast"
    )]
    fn values_grid(&mut self, ui: &mut Ui, alpha_on: bool, shown: RgbaF32) {
        let id = self.id;
        let theme = self.theme;
        let gap = self.gap;
        let quantized = shown.to_srgba8();
        let hex_id = id.with("hex");
        let shown_hex = ui.fmt(format_args!(
            "#{:02X}{:02X}{:02X}",
            quantized.r, quantized.g, quantized.b,
        ));

        let mut rgb = [
            i64::from(quantized.r),
            i64::from(quantized.g),
            i64::from(quantized.b),
        ];
        let mut opacity = (self.writes.alpha * 100.0).round() as i64;
        let mut hue = (self.state.coords.hue() * 360.0).round() as i64;
        let mut sat = (self.state.coords.saturation() * 100.0).round() as i64;
        let mut val = (self.state.coords.value() * 100.0).round() as i64;

        Grid::new()
            .id(id.with("values"))
            .cols([Track::FILL; VALUE_COLS])
            .rows([Track::HUG; 2])
            .gap(gap)
            .line_gap(gap)
            .size(SizeSpec::FILL_W)
            .show(ui, |ui| {
                Panel::vstack()
                    .id(id.with("hex-cell"))
                    .gap(LABEL_GAP)
                    .grid_cell(
                        GridCell::at(0, 0).with_span(NonZeroU16::MIN, NonZeroU16::new(2).unwrap()),
                    )
                    .size(SizeSpec::FILL_W)
                    .show(ui, |ui| {
                        Text::new("HEX")
                            .style(&theme.label.apply(&ui.theme().text))
                            .id(id.with("hex-label"))
                            .show(ui);
                        let hex = TextEdit::buffered(shown_hex)
                            .max_chars(7)
                            .style(&theme.hex)
                            .id(hex_id)
                            .size(SizeSpec::FILL_W)
                            .show(ui);
                        // A draft parsing to the color already shown is no edit, as `#0000ff`
                        // under `#0000FF`.
                        if let Some(typed) = hex.committed_text
                            && let Ok(parsed) = typed.trim().parse::<RgbaF32>()
                            && !same_rgb(parsed.to_srgba8(), quantized)
                        {
                            self.writes.exact = Some(parsed.with_alpha(self.writes.alpha));
                            self.writes.committed = true;
                            self.writes.remember = true;
                        }
                    });

                // The cell beside the hex field shows opacity where alpha is
                // edited and the value axis otherwise, completing H, S and V.
                if alpha_on {
                    let r = self.value_cell(ui, OPACITY, &mut opacity);
                    if r.edit.changed {
                        self.writes.alpha = opacity as f32 / 100.0;
                        self.writes.alpha_moved = true;
                    }
                    self.writes.typed(&r);
                } else {
                    let r = self.value_cell(ui, VALUE, &mut val);
                    if r.edit.changed {
                        self.state.coords = self.state.coords.with_value(val as f32 / 100.0);
                        self.writes.axes = true;
                    }
                    self.writes.typed(&r);
                }

                let r = self.value_cell(ui, HUE, &mut hue);
                if r.edit.changed {
                    self.state.coords = self.state.coords.with_hue(hue as f32 / 360.0);
                    self.writes.axes = true;
                }
                self.writes.typed(&r);

                for (channel, cell) in CHANNELS.into_iter().enumerate() {
                    let r = self.value_cell(ui, cell, &mut rgb[channel]);
                    if r.edit.changed {
                        let byte = |v: i64| v.clamp(0, 255) as u8;
                        self.writes.exact = Some(
                            RgbaF32::from_srgba8(SrgbaU8::rgb(
                                byte(rgb[0]),
                                byte(rgb[1]),
                                byte(rgb[2]),
                            ))
                            .with_alpha(self.writes.alpha),
                        );
                    }
                    self.writes.typed(&r);
                }

                let r = self.value_cell(ui, SATURATION, &mut sat);
                if r.edit.changed {
                    self.state.coords = self.state.coords.with_saturation(sat as f32 / 100.0);
                    self.writes.axes = true;
                }
                self.writes.typed(&r);
            });
    }

    /// One grid cell: the channel's caption over its value, a drag from zero to the
    /// cell's top.
    fn value_cell(&self, ui: &mut Ui, cell: ValueCell, value: &mut i64) -> CellWrite {
        let theme = self.theme;
        // Keyed on the channel letter alone: an id that moved with the unit would
        // drop the widget's state.
        let cell_id = self.id.with(&cell.caption[..1]);
        let value_id = cell_id.with("value");
        // Read before the show, which closes an editor on its commit.
        let editing = DragValue::is_editing(ui, value_id);
        let edit = Panel::vstack()
            .id(cell_id)
            .gap(LABEL_GAP)
            .grid_cell(cell.cell)
            .size(SizeSpec::FILL_W)
            .show(ui, |ui| {
                Text::new(cell.caption)
                    .style(&theme.label.apply(&ui.theme().text))
                    .id(cell_id.with("label"))
                    .show(ui);
                DragValue::new(value)
                    .range(0.0..=cell.top)
                    .style(&theme.value)
                    .id(value_id)
                    .size(SizeSpec::FILL_W)
                    .show(ui)
            })
            .inner;
        CellWrite {
            edit,
            pick: editing || edit.response.left.released(),
        }
    }

    /// The two models, as one either-or choice.
    fn model_switch(&mut self, ui: &mut Ui) {
        let id = self.id;
        let gap = self.gap;
        let mut model = self.state.coords.model();
        RadioGroup::new(&mut model)
            .axis(Axis::X)
            .id(id.with("models"))
            .gap(gap)
            .size(SizeSpec::FILL_W)
            .show(ui, |ui, options| {
                for choice in ColorModel::ALL {
                    options.option(choice).label(model_label(choice)).show(ui);
                }
            });
        self.state.coords = self.state.coords.with_model(model);
    }

    /// The preset or history row. Clicking a swatch picks it outright.
    fn swatch_row(&mut self, ui: &mut Ui, swatches: SwatchRow<'_>) {
        let id = self.id;
        let theme = self.theme;
        let colors: &[RgbaF32] = match swatches {
            SwatchRow::None => return,
            SwatchRow::History => self.state.history.colors(),
            SwatchRow::Fixed(colors) => colors,
        };
        let mut picked = None;
        Panel::wrap_hstack()
            .id(id.with("swatches"))
            .gap(self.gap)
            .size(SizeSpec::FILL_W)
            .show(ui, |ui| {
                for (idx, color) in colors.iter().enumerate() {
                    if ColorSwatch::new(*color)
                        .style(theme)
                        .id(id.with("swatch").with(idx))
                        .show(ui)
                        .left
                        .clicked()
                    {
                        picked = Some(*color);
                    }
                }
            });
        if let Some(color) = picked {
            self.writes.exact = Some(color);
            self.writes.alpha = color.a;
            self.writes.committed = true;
            self.writes.remember = true;
        }
    }
}

/// Fold this frame's writes into the bound color, rebuilding only the part
/// the moved control owns; whether the color moved.
fn apply(state: &mut PickerState, color: &mut RgbaF32, writes: Writes) -> bool {
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
        state.written = Some(next);
        if writes.exact.is_some() {
            let model = state.coords.model();
            state.coords = ColorCoords::from_color(next, model, state.coords.hue());
        }
    }
    if writes.remember {
        state.history.push(*color);
    }
    changed
}

/// What the model switch calls `model`: the picker's text, not the color model's.
const fn model_label(model: ColorModel) -> &'static str {
    match model {
        ColorModel::Okhsv => "Okhsv",
        ColorModel::Hsv => "HSV",
    }
}

#[cfg(test)]
mod tests;
