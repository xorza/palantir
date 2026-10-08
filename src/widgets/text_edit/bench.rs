//! A settled `TextEdit` frame with real shaping: a single-line field, and a
//! multiline one whose whole text is selected, so the selection geometry is
//! rebuilt every frame.

use crate::bench::Run;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widgets::text_edit::{TextEdit, TextEditState};
use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion};
use glam::UVec2;
use std::hint::black_box;

fn editor_id() -> WidgetId {
    WidgetId::from_hash("text-edit-bench")
}

fn run_frame(h: &mut UiHarness, text: &mut String, multiline: bool) {
    black_box(h.frame(|ui| {
        TextEdit::new(text)
            .id(editor_id())
            .multiline(multiline)
            .size((Sizing::fixed(480.0), Sizing::fixed(160.0)))
            .show(ui);
    }));
}

fn bench_stable(
    group: &mut BenchmarkGroup<'_, WallTime>,
    leaf: &str,
    text: String,
    multiline: bool,
    selected: bool,
) {
    let mut h = UiHarness::with_text(UVec2::new(800, 300));
    let mut text = text;
    for _ in 0..3 {
        run_frame(&mut h, &mut text, multiline);
    }
    if selected {
        h.set_focus(editor_id());
        h.ui.with_state::<TextEditState, _>(editor_id(), |_, state| {
            state.edit.selection = Some(0);
            state.edit.caret = text.len();
        });
        run_frame(&mut h, &mut text, multiline);
    }
    group.bench_function(leaf, |bencher| {
        bencher.iter(|| run_frame(&mut h, &mut text, multiline));
    });
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let mut group = run.group(c);
    bench_stable(
        &mut group,
        "stable_single_line",
        String::from("A stable single-line editor with enough text to exercise shaping."),
        false,
        false,
    );
    bench_stable(
        &mut group,
        "stable_multiline_selection",
        String::from(
            "First selected line with enough text to wrap across the editor.\n\
             Second selected line keeps selection geometry in the shared probe.",
        ),
        true,
        true,
    );
    group.finish();
}
