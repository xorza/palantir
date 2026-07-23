use crate::display::Display;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Configure;
use crate::ui::Ui;
use crate::widgets::text_edit::{TextEdit, TextEditState};
use criterion::Criterion;
use glam::UVec2;
use std::hint::black_box;
use std::time::Duration;

fn display() -> Display {
    Display::from_physical(UVec2::new(800, 300), 1.0)
}

fn editor_id() -> WidgetId {
    WidgetId::from_hash("text-edit-bench")
}

fn run_frame(ui: &mut Ui, text: &mut String, multiline: bool) {
    black_box(
        ui.record_test_frame_without_baseline(display(), Duration::ZERO, |ui| {
            TextEdit::new(text)
                .id(editor_id())
                .multiline(multiline)
                .size((Sizing::fixed(480.0), Sizing::fixed(160.0)))
                .show(ui);
        }),
    );
}

fn bench_stable(c: &mut Criterion, name: &str, text: String, multiline: bool, selected: bool) {
    let mut ui = Ui::for_test_text();
    let mut text = text;
    for _ in 0..3 {
        run_frame(&mut ui, &mut text, multiline);
    }
    if selected {
        ui.request_focus(Some(editor_id()));
        let state = ui.state_mut::<TextEditState>(editor_id());
        state.edit.selection = Some(0);
        state.edit.caret = text.len();
        run_frame(&mut ui, &mut text, multiline);
    }
    c.bench_function(name, |bencher| {
        bencher.iter(|| run_frame(&mut ui, &mut text, multiline));
    });
}

pub fn bench(c: &mut Criterion) {
    bench_stable(
        c,
        "text_edit/stable_single_line",
        String::from("A stable single-line editor with enough text to exercise shaping."),
        false,
        false,
    );
    bench_stable(
        c,
        "text_edit/stable_multiline_selection",
        String::from(
            "First selected line with enough text to wrap across the editor.\n\
             Second selected line keeps selection geometry in the shared probe.",
        ),
        true,
        true,
    );
}
