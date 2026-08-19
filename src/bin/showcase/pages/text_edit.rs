//! TextEdit — editable text leaves. Single-line fields with a focus
//! policy toggle, a right-aligned multi-line editor, and a 3×3 grid
//! covering every `(HAlign, VAlign)` combination.
//!
//! Buffer storage: one [`Page`] row under a non-widget id, lent to the whole
//! page body by `Ui::with_state`, so the buffers survive page switches and
//! the editors can take `&mut String` straight out of it.

use crate::support::{note_style, row, section};
use palantir::{
    Align, Button, Configure, FocusPolicy, HAlign, Panel, Sizing, Text, TextEdit, Ui, VAlign,
    WidgetId, fmt,
};

/// Everything this page keeps across frames. One row rather than four,
/// because they are one page's worth of state and nothing outside reads them.
#[derive(Debug, Default)]
struct Page {
    a: String,
    b: String,
    multiline: String,
    policy: FocusPolicy,
}

pub(crate) fn build(ui: &mut Ui) {
    let page_id = WidgetId::from_hash("showcase::text_edit::page");
    ui.with_state::<Page, _>(page_id, page);
}

fn page(ui: &mut Ui, state: &mut Page) {
    ui.set_focus_policy(state.policy);

    section(
        ui,
        "single line — the default ClearOnMiss policy drops focus on an outside \
         click; toggle to PreserveOnMiss for sticky focus",
        |ui| {
            row(ui, |ui| {
                TextEdit::new(&mut state.a)
                    .id_salt("editor_a")
                    .placeholder("first field")
                    .size((Sizing::FILL, Sizing::HUG))
                    .min_size((180.0, 32.0))
                    .show(ui);
                TextEdit::new(&mut state.b)
                    .id_salt("editor_b")
                    .placeholder("second field")
                    .size((Sizing::FILL, Sizing::HUG))
                    .min_size((180.0, 32.0))
                    .show(ui);
            });
            row(ui, |ui| {
                let label = match state.policy {
                    FocusPolicy::ClearOnMiss => "policy: ClearOnMiss",
                    FocusPolicy::PreserveOnMiss => "policy: PreserveOnMiss",
                };
                if Button::new()
                    .id_salt("policy_toggle")
                    .label(label)
                    .min_size((220.0, 32.0))
                    .show(ui)
                    .left
                    .clicked()
                {
                    state.policy = match state.policy {
                        FocusPolicy::ClearOnMiss => FocusPolicy::PreserveOnMiss,
                        FocusPolicy::PreserveOnMiss => FocusPolicy::ClearOnMiss,
                    };
                }
                if Button::new()
                    .id_salt("clear")
                    .label("clear both")
                    .min_size((140.0, 32.0))
                    .show(ui)
                    .left
                    .clicked()
                {
                    state.a.clear();
                    state.b.clear();
                }
            });
            let a = fmt!(ui, "buffer A ({:>2} bytes): {}", state.a.len(), state.a);
            Text::new(a).style(&note_style()).show(ui);
            let b = fmt!(ui, "buffer B ({:>2} bytes): {}", state.b.len(), state.b);
            Text::new(b).style(&note_style()).show(ui);
        },
    );

    section(
        ui,
        "multi-line — Enter inserts a newline, Up/Down navigate visual lines, \
         selection spans newlines, paste preserves a multi-line clipboard",
        |ui| {
            TextEdit::new(&mut state.multiline)
                .id_salt("editor_ml")
                .multiline(true)
                .text_align(Align::RIGHT)
                .align(Align::RIGHT)
                .placeholder("paste a paragraph here")
                .size((Sizing::FILL, Sizing::fixed(110.0)))
                .min_size((180.0, 80.0))
                .show(ui);
        },
    );

    // Editors are taller than their text line so the vertical placement
    // is obvious; the caret tracks the glyphs regardless of where the
    // text sits inside the rect.
    section(
        ui,
        "text alignment — one editor per (HAlign, VAlign) combination",
        align_grid,
    );
}

fn align_grid(ui: &mut Ui) {
    const ROWS: [(VAlign, &str); 3] = [
        (VAlign::Top, "top"),
        (VAlign::Center, "center"),
        (VAlign::Bottom, "bottom"),
    ];
    const COLS: [(HAlign, &str); 3] = [
        (HAlign::Left, "left"),
        (HAlign::Center, "center"),
        (HAlign::Right, "right"),
    ];

    Panel::vstack()
        .id_salt("align-grid")
        .gap(8.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for (v, vname) in ROWS {
                Panel::hstack()
                    .id_salt(vname)
                    .gap(8.0)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        for (h, hname) in COLS {
                            // Hashed from the parts: `WidgetId::from_hash`
                            // takes anything `Hash`, so building a `String`
                            // to name the cell allocated once per cell per
                            // frame to describe two `&'static str`s.
                            let key = ("textedit_align", vname, hname);
                            let buf_id = WidgetId::from_hash(key);
                            // Seeded on the first frame only — keyed on the
                            // row not existing yet, rather than on the buffer
                            // being empty, so a cell the user clears stays
                            // cleared and its placeholder can actually show.
                            let fresh = ui.try_state::<String>(buf_id).is_none();
                            ui.with_state::<String, _>(buf_id, |ui, buf| {
                                if fresh {
                                    *buf = format!("{vname}-{hname}");
                                }
                                let empty = buf.is_empty();
                                let mut edit = TextEdit::new(buf)
                                    .id_salt(key)
                                    .text_align(Align::new(h, v))
                                    .size((Sizing::FILL, Sizing::fixed(56.0)))
                                    .min_size((140.0, 56.0));
                                // Composed only when it can be seen:
                                // `placeholder` takes an owned `Cow`, so
                                // building one every frame would allocate per
                                // cell for text that shows only while a cell
                                // is empty.
                                if empty {
                                    edit = edit.placeholder(format!("{vname} / {hname}"));
                                }
                                edit.show(ui);
                            });
                        }
                    });
            }
        });
}
