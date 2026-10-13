//! A settled `TextEdit` frame with real shaping: a single-line field, and a
//! multiline one whose whole text is selected, so the selection geometry is
//! rebuilt every frame.

use crate::bench::fixture::{Backend, Kind, Scale};
use crate::internals::ui_harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widgets::text_edit::{TextEdit, TextEditState};
use criterion::Criterion;
use glam::UVec2;
use std::hint::black_box;

fn editor_id() -> WidgetId {
    WidgetId::from_hash("text-edit-bench")
}

/// One bench case: the editor's mode and what is selected once it settles.
#[derive(Debug)]
struct Case {
    name: &'static str,
    text: &'static str,
    multiline: bool,
    selection: Selection,
}

#[derive(Clone, Copy, Debug)]
enum Selection {
    Nothing,
    All,
}

fn run_frame(h: &mut UiHarness, text: &mut String, case: &Case) {
    black_box(h.frame(|ui| {
        TextEdit::new(text)
            .id(editor_id())
            .multiline(case.multiline)
            .size((Sizing::fixed(480.0), Sizing::fixed(160.0)))
            .show(ui);
    }));
}

/// A settled editor over `text`, selected as `case` says.
fn settled(text: &mut String, case: &Case) -> UiHarness {
    let mut h = UiHarness::with_text(UVec2::new(800, 300));
    for _ in 0..3 {
        run_frame(&mut h, text, case);
    }
    match case.selection {
        Selection::Nothing => {}
        Selection::All => {
            h.set_focus(editor_id());
            h.ui.with_state::<TextEditState, _>(editor_id(), |_, state| {
                state.edit.selection = Some(0);
                state.edit.caret = text.len();
            });
            run_frame(&mut h, text, case);
        }
    }
    h
}

/// `atomic/text_edit/cpu/<case>`: one frame of a settled editor with real shaping.
pub(crate) fn atomic(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "text_edit", Scale::Sample);
    let cases = [
        Case {
            name: "stable_single_line",
            text: "A stable single-line editor with enough text to exercise shaping.",
            multiline: false,
            selection: Selection::Nothing,
        },
        Case {
            name: "stable_multiline_selection",
            text: "First selected line with enough text to wrap across the editor.\n\
                   Second selected line keeps selection geometry in the shared probe.",
            multiline: true,
            selection: Selection::All,
        },
    ];
    for case in &cases {
        group.bench_function(Backend::Cpu.case(case.name), |bencher| {
            let mut text = String::from(case.text);
            let mut h = settled(&mut text, case);
            bencher.iter(|| run_frame(&mut h, &mut text, case));
        });
    }
    group.finish();
}
