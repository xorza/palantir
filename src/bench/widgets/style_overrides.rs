//! Construction and full-record costs for inherited and borrowed widget styles.

use crate::display::Display;
use crate::forest::element::Configure;
use crate::ui::Ui;
use crate::ui::frame::FrameStamp;
use crate::widgets::button::Button;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::combo_box::ComboBox;
use crate::widgets::drag_value::DragValue;
use crate::widgets::panel::Panel;
use crate::widgets::radio::RadioButton;
use crate::widgets::switch::Switch;
use crate::widgets::text_edit::TextEdit;
use crate::widgets::theme::Theme;
use criterion::Criterion;
use glam::UVec2;
use std::hint::black_box;
use std::time::Duration;

const OPTIONS: [&str; 2] = ["one", "two"];
const SETS_PER_FRAME: usize = 32;
const CONSTRUCTION_SETS: usize = 64;
const DISPLAY_SIZE: UVec2 = UVec2::new(1280, 800);

#[derive(Debug)]
struct WidgetValues {
    checked: bool,
    switched: bool,
    selected: usize,
    number: i64,
    radio: u8,
    text: String,
}

impl Default for WidgetValues {
    fn default() -> Self {
        Self {
            checked: false,
            switched: true,
            selected: 0,
            number: 42,
            radio: 0,
            text: String::from("text"),
        }
    }
}

fn construct_inherited(values: &mut WidgetValues) {
    for _ in 0..CONSTRUCTION_SETS {
        black_box(Button::new().label("button"));
        black_box(Checkbox::new(&mut values.checked).label("checkbox"));
        black_box(Switch::new(&mut values.switched).label("switch"));
        black_box(ComboBox::new(&mut values.selected, &OPTIONS));
        black_box(DragValue::new(&mut values.number));
        black_box(RadioButton::new(&mut values.radio, 1).label("radio"));
        black_box(TextEdit::new(&mut values.text));
    }
}

fn construct_custom(values: &mut WidgetValues, theme: &Theme) {
    for _ in 0..CONSTRUCTION_SETS {
        black_box(Button::new().label("button").style(&theme.button));
        black_box(
            Checkbox::new(&mut values.checked)
                .label("checkbox")
                .style(&theme.checkbox),
        );
        black_box(
            Switch::new(&mut values.switched)
                .label("switch")
                .style(&theme.switch),
        );
        black_box(ComboBox::new(&mut values.selected, &OPTIONS).style(&theme.button));
        black_box(DragValue::new(&mut values.number).style(&theme.drag_value));
        black_box(
            RadioButton::new(&mut values.radio, 1)
                .label("radio")
                .style(&theme.radio),
        );
        black_box(TextEdit::new(&mut values.text).style(&theme.text_edit));
    }
}

fn render_widgets(ui: &mut Ui, values: &mut WidgetValues, styles: Option<&Theme>) {
    Panel::vstack().auto_id().show(ui, |ui| {
        for i in 0..SETS_PER_FRAME {
            let button = Button::new().id_salt(("button", i)).label("button");
            match styles {
                Some(theme) => button.style(&theme.button).show(ui),
                None => button.show(ui),
            };

            let checkbox = Checkbox::new(&mut values.checked)
                .id_salt(("checkbox", i))
                .label("checkbox");
            match styles {
                Some(theme) => checkbox.style(&theme.checkbox).show(ui),
                None => checkbox.show(ui),
            };

            let switch = Switch::new(&mut values.switched)
                .id_salt(("switch", i))
                .label("switch");
            match styles {
                Some(theme) => switch.style(&theme.switch).show(ui),
                None => switch.show(ui),
            };

            let combo = ComboBox::new(&mut values.selected, &OPTIONS).id_salt(("combo", i));
            match styles {
                Some(theme) => combo.style(&theme.button).show(ui),
                None => combo.show(ui),
            };

            let drag = DragValue::new(&mut values.number).id_salt(("drag", i));
            match styles {
                Some(theme) => drag.style(&theme.drag_value).show(ui),
                None => drag.show(ui),
            };

            let radio = RadioButton::new(&mut values.radio, 1)
                .id_salt(("radio", i))
                .label("radio");
            match styles {
                Some(theme) => radio.style(&theme.radio).show(ui),
                None => radio.show(ui),
            };

            let edit = TextEdit::new(&mut values.text).id_salt(("edit", i));
            match styles {
                Some(theme) => edit.style(&theme.text_edit).show(ui),
                None => edit.show(ui),
            };
        }
    });
}

fn warm_show(ui: &mut Ui, values: &mut WidgetValues, styles: Option<&Theme>) {
    let display = Display::from_physical(DISPLAY_SIZE, 1.0);
    for frame in 0..4 {
        ui.record_acked(
            FrameStamp::new(display, Duration::from_millis(frame)),
            |ui| render_widgets(ui, values, styles),
        );
    }
}

pub fn bench(c: &mut Criterion) {
    let theme = Theme::default();

    {
        let mut inherited = WidgetValues::default();
        let mut custom = WidgetValues::default();
        let mut group = c.benchmark_group("widget_styles/construction");
        group.bench_function("inherited", |b| {
            b.iter(|| construct_inherited(&mut inherited));
        });
        group.bench_function("custom", |b| {
            b.iter(|| construct_custom(&mut custom, &theme));
        });
        group.finish();
    }

    {
        let mut ui = Ui::default();
        let mut values = WidgetValues::default();
        let mut frame = 4_u64;
        let display = Display::from_physical(DISPLAY_SIZE, 1.0);
        warm_show(&mut ui, &mut values, None);
        c.bench_function("widget_styles/show/inherited", |b| {
            b.iter(|| {
                frame = frame.wrapping_add(1);
                black_box(ui.record_acked(
                    FrameStamp::new(display, Duration::from_millis(frame)),
                    |ui| render_widgets(ui, &mut values, None),
                ));
            });
        });
    }

    {
        let mut ui = Ui::default();
        let mut values = WidgetValues::default();
        let mut frame = 4_u64;
        let display = Display::from_physical(DISPLAY_SIZE, 1.0);
        warm_show(&mut ui, &mut values, Some(&theme));
        c.bench_function("widget_styles/show/custom", |b| {
            b.iter(|| {
                frame = frame.wrapping_add(1);
                black_box(ui.record_acked(
                    FrameStamp::new(display, Duration::from_millis(frame)),
                    |ui| render_widgets(ui, &mut values, Some(&theme)),
                ));
            });
        });
    }
}
