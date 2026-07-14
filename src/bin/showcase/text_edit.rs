//! TextEdit — editable text leaves. Single-line fields with focus
//! policy, a right-aligned multi-line editor, and a 3×3 grid covering
//! every `(HAlign, VAlign)` combination.
//!
//! Buffer storage: stashed in `Ui::state_mut::<String>` under
//! non-widget ids, so buffers survive across showcase tab switches.
//! The widget takes `&mut String`, so we `mem::take` out of the state
//! map for the body and put it back at the end — two moves of a small
//! `String` per buffer.

use crate::support;
use crate::support::{row, section};
use aperture::{
    Align, Button, Configure, FocusPolicy, HAlign, Panel, Sizing, Text, TextEdit, Ui, VAlign,
    WidgetId,
};

pub fn build(ui: &mut Ui) {
    let buf_a_id = WidgetId::from_hash("textedit_showcase__buffer_a");
    let buf_b_id = WidgetId::from_hash("textedit_showcase__buffer_b");
    let buf_ml_id = WidgetId::from_hash("textedit_showcase__buffer_ml");
    let policy_id = WidgetId::from_hash("textedit_showcase__policy");

    let mut buf_a = std::mem::take(ui.state_mut::<String>(buf_a_id));
    let mut buf_b = std::mem::take(ui.state_mut::<String>(buf_b_id));
    let mut buf_ml = std::mem::take(ui.state_mut::<String>(buf_ml_id));
    let policy = *ui.state_mut::<FocusPolicy>(policy_id);
    ui.set_focus_policy(policy);

    support::page(ui, |ui| {
        support::header(
            ui,
            "TextEdit — click to focus, type to insert; arrows / Home / End / \
             Backspace / Delete navigate, Escape blurs.",
        );

        section(
            ui,
            "single",
            "single-line — default focus policy is ClearOnMiss (a click elsewhere drops focus); \
             toggle to PreserveOnMiss for sticky focus",
            |ui| {
                row(ui, "editors", |ui| {
                    TextEdit::new(&mut buf_a)
                        .id_salt("editor_a")
                        .placeholder("first field")
                        .size((Sizing::FILL, Sizing::Hug))
                        .min_size((180.0, 32.0))
                        .show(ui);
                    TextEdit::new(&mut buf_b)
                        .id_salt("editor_b")
                        .placeholder("second field")
                        .size((Sizing::FILL, Sizing::Hug))
                        .min_size((180.0, 32.0))
                        .show(ui);
                });
                row(ui, "controls", |ui| {
                    let label = match policy {
                        FocusPolicy::ClearOnMiss => "policy: ClearOnMiss",
                        FocusPolicy::PreserveOnMiss => "policy: PreserveOnMiss",
                    };
                    if Button::new()
                        .id_salt("policy_toggle")
                        .label(label)
                        .min_size((220.0, 32.0))
                        .show(ui)
                        .left.clicked()
                    {
                        let next = match policy {
                            FocusPolicy::ClearOnMiss => FocusPolicy::PreserveOnMiss,
                            FocusPolicy::PreserveOnMiss => FocusPolicy::ClearOnMiss,
                        };
                        *ui.state_mut::<FocusPolicy>(policy_id) = next;
                    }
                    if Button::new()
                        .id_salt("clear")
                        .label("clear both")
                        .min_size((140.0, 32.0))
                        .show(ui)
                        .left.clicked()
                    {
                        buf_a.clear();
                        buf_b.clear();
                    }
                });
                Text::new(format!("buffer A ({:>2} bytes): {}", buf_a.len(), buf_a))
                    .auto_id()
                    .show(ui);
                Text::new(format!("buffer B ({:>2} bytes): {}", buf_b.len(), buf_b))
                    .auto_id()
                    .show(ui);
            },
        );

        section(
            ui,
            "multiline",
            "multi-line, right-aligned — Enter inserts \\n, Up/Down navigate visual lines, \
             selection spans newlines, paste preserves multi-line clipboard",
            |ui| {
                TextEdit::new(&mut buf_ml)
                    .id_salt("editor_ml")
                    .multiline(true)
                    .text_align(Align::RIGHT)
                    .align(Align::RIGHT)
                    .placeholder("paste a paragraph here")
                    .size((Sizing::FILL, Sizing::Fixed(110.0)))
                    .min_size((180.0, 80.0))
                    .show(ui);
            },
        );

        // Editors are taller than their text line so the vertical
        // placement is obvious; the caret tracks the glyphs regardless
        // of where the text sits inside the rect.
        section(
            ui,
            "align",
            "alignment — one editor per (HAlign, VAlign) combination",
            align_grid,
        );
    });

    *ui.state_mut::<String>(buf_a_id) = buf_a;
    *ui.state_mut::<String>(buf_b_id) = buf_b;
    *ui.state_mut::<String>(buf_ml_id) = buf_ml;
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
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            for (v, vname) in ROWS {
                Panel::hstack()
                    .id_salt(vname)
                    .gap(8.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        for (h, hname) in COLS {
                            let key = format!("textedit_align__{vname}_{hname}");
                            let buf_id = WidgetId::from_hash(key.as_str());
                            let mut buf = std::mem::take(ui.state_mut::<String>(buf_id));
                            if buf.is_empty() {
                                buf = format!("{vname}-{hname}");
                            }
                            TextEdit::new(&mut buf)
                                .id_salt(key.as_str())
                                .text_align(Align::new(h, v))
                                .placeholder(format!("{vname} / {hname}"))
                                .size((Sizing::FILL, Sizing::Fixed(56.0)))
                                .min_size((140.0, 56.0))
                                .show(ui);
                            *ui.state_mut::<String>(buf_id) = buf;
                        }
                    });
            }
        });
}
