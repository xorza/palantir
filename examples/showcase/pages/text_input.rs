//! TextEdit: single-line fields and focus-session edges, a multi-line editor, alignment, and IME composition.
//!
//! Buffers live in one [`Page`] row lent by `Ui::with_state`, so they survive page switches.

use crate::scaffold::api;
use crate::support;
use crate::support::{Kit, Section};
use palantir::widget::Span;
use palantir::{
    Align, Configure, EditEnd, HAlign, Sizing, Text, TextEdit, TextEditResponse, Ui, VAlign,
    WidgetId, fmt,
};

#[derive(Debug)]
struct Page {
    a: String,
    b: String,
    events_field: String,
    events: EventLog,
    multiline: String,
    aligned: [String; 3],
    ime: String,
    /// A copy of the live composition for the readout, refilled to avoid reallocating.
    preedit: String,
    preedit_cursor: Option<Span>,
    plain: String,
    plain_edits: u32,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            a: String::new(),
            b: String::new(),
            events_field: String::from("edit me"),
            events: EventLog::default(),
            multiline: String::new(),
            aligned: ["left".into(), "center".into(), "right".into()],
            ime: String::new(),
            preedit: String::new(),
            preedit_cursor: None,
            plain: String::new(),
            plain_edits: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Edge {
    FocusGained,
    Changed,
    Submitted,
    Canceled,
    Committed,
    FocusLost,
}

impl Edge {
    const fn name(self) -> &'static str {
        match self {
            Self::FocusGained => "focus_gained",
            Self::Changed => "changed",
            Self::Submitted => "submitted",
            Self::Canceled => "canceled",
            Self::Committed => "committed",
            Self::FocusLost => "focus_lost",
        }
    }
}

/// The last [`EventLog::LEN`] edges in a fixed ring.
#[derive(Debug, Default)]
struct EventLog {
    entries: [Option<(u32, Edge)>; Self::LEN],
    /// Next edge's sequence number; its slot is `next % LEN`.
    next: u32,
}

impl EventLog {
    const LEN: usize = 8;

    fn record(&mut self, r: &TextEditResponse<'_>) {
        for (fired, edge) in [
            (r.focus_gained, Edge::FocusGained),
            (r.changed, Edge::Changed),
            (r.ended == Some(EditEnd::Submitted), Edge::Submitted),
            (r.ended == Some(EditEnd::Canceled), Edge::Canceled),
            (r.committed, Edge::Committed),
            (r.focus_lost, Edge::FocusLost),
        ] {
            if fired {
                self.entries[self.next as usize % Self::LEN] = Some((self.next, edge));
                self.next += 1;
            }
        }
    }

    /// Newest first.
    fn iter(&self) -> impl Iterator<Item = (u32, Edge)> + '_ {
        (0..Self::LEN).filter_map(move |back| {
            let seq = self.next.checked_sub(1 + back as u32)?;
            self.entries[seq as usize % Self::LEN]
        })
    }
}

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    let page_id = WidgetId::from_hash("showcase::text_input::page");
    ui.with_state::<Page, _>(page_id, |ui, s| page(ui, k, s));
}

fn page(ui: &mut Ui, k: Kit, s: &mut Page) {
    Section::new("Single line")
        .apis(&[api!(TextEdit::new)])
        .show(ui, k, |ui| {
            support::row(ui, |ui| {
                for (buf, hint) in [(&mut s.a, "first field"), (&mut s.b, "second field")] {
                    TextEdit::new(buf)
                        .id_salt(hint)
                        .placeholder(hint)
                        .size((Sizing::FILL, Sizing::HUG))
                        .max_size((320.0, f32::INFINITY))
                        .show(ui);
                }
            });
            let a = fmt!(ui, "{:?} ({} bytes)", s.a, s.a.len());
            support::readout(ui, k, "first", a);
            let b = fmt!(ui, "{:?} ({} bytes)", s.b, s.b.len());
            support::readout(ui, k, "second", b);
        });

    Section::new("Edit events")
        .apis(&[api!(TextEdit::show)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "The edges one focus session reports. Enter submits and commits, Escape \
                 cancels and blurs without a commit, and a blur after an edit commits.",
            );
            support::row(ui, |ui| {
                let r = TextEdit::new(&mut s.events_field)
                    .size((Sizing::fixed(240.0), Sizing::HUG))
                    .show(ui);
                s.events.record(&r);
            });
            support::well(ui, k, |ui| {
                let mut any = false;
                for (seq, edge) in s.events.iter() {
                    any = true;
                    let line = fmt!(ui, "#{seq:<4} {}", edge.name());
                    Text::new(line)
                        .id_salt(seq)
                        .style(&k.t.mono(k.t.ink))
                        .show(ui);
                }
                if !any {
                    Text::new("no edges yet — click the field")
                        .style(&k.t.caption())
                        .show(ui);
                }
            });
        });

    Section::new("Multi-line")
        .apis(&[api!(TextEdit::multiline)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Enter inserts a newline, ↑ and ↓ move by visual line, and a selection spans \
                 lines. A multi-line clipboard pastes as it is.",
            );
            TextEdit::new(&mut s.multiline)
                .multiline(true)
                .placeholder("paste a paragraph here")
                .size((Sizing::FILL, Sizing::fixed(110.0)))
                .show(ui);
        });

    Section::new("Alignment")
        .apis(&[api!(TextEdit::text_align)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "The text, and the caret with it, sit where text_align puts them in an editor \
                 taller and wider than its line.",
            );
            support::row(ui, |ui| {
                for (i, (buf, h)) in s
                    .aligned
                    .iter_mut()
                    .zip([HAlign::Left, HAlign::Center, HAlign::Right])
                    .enumerate()
                {
                    TextEdit::new(buf)
                        .id_salt(i)
                        .text_align(Align::new(h, VAlign::Center))
                        .size((Sizing::fill(1.0), Sizing::fixed(56.0)))
                        .show(ui);
                }
            });
        });

    ime(ui, k, s);
}

fn ime(ui: &mut Ui, k: Kit, s: &mut Page) {
    Section::new("IME composition")
        .apis(&[api!(Ui::request_ime), api!(Ui::ime_preedit)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "An input method composes in place: the text so far is drawn underlined at \
                 the caret, the candidate window opens beside the caret, and the bound \
                 String changes only when the composition commits. On macOS: add the \
                 Japanese – Romaji input source, switch to it, type nihon, then Space to \
                 pick a candidate and Enter to commit.",
            );
            TextEdit::new(&mut s.ime)
                .placeholder("type with an input method")
                .size((Sizing::FILL, Sizing::HUG))
                .max_size((420.0, f32::INFINITY))
                .show(ui);
            s.preedit.clear();
            s.preedit_cursor = None;
            if let Some(p) = ui.ime_preedit() {
                s.preedit.push_str(p.text);
                s.preedit_cursor = p.cursor;
            }
            support::well(ui, k, |ui| {
                let preedit = if s.preedit.is_empty() {
                    ui.intern("none")
                } else {
                    fmt!(ui, "{:?}", s.preedit)
                };
                support::readout(ui, k, "preedit", preedit);
                let cursor = match s.preedit_cursor {
                    Some(cursor) => fmt!(ui, "bytes {:?}", cursor.range()),
                    None => ui.intern("none"),
                };
                support::readout(ui, k, "cursor", cursor);
                let bound = fmt!(ui, "{:?}", s.ime);
                support::readout(ui, k, "bound value", bound);
            });
            support::checklist(
                ui,
                k,
                &[
                    "While composing, the preedit shows underlined at the caret",
                    "The candidate window opens next to the caret, and follows it",
                    "'bound value' stays the same until the composition commits",
                    "Enter commits the composition only; a second Enter submits the field",
                    "Escape while composing cancels the composition and keeps focus",
                ],
            );
        });

    Section::new("Plain keys with an input method on").show(ui, k, |ui| {
        support::note(
            ui,
            k,
            "On Windows and X11 an input method can pass a plain key through as a \
                 commit and as a key press both. Each key must type one character.",
        );
        support::row(ui, |ui| {
            let r = TextEdit::new(&mut s.plain)
                .placeholder("type abc")
                .size((Sizing::fixed(240.0), Sizing::HUG))
                .show(ui);
            if r.changed {
                s.plain_edits += 1;
            }
        });
        let count = fmt!(
            ui,
            "{} chars from {} edits",
            s.plain.chars().count(),
            s.plain_edits
        );
        support::readout(ui, k, "typed", count);
        support::checklist(
            ui,
            k,
            &["With an input method on in its Latin mode, typing abc shows abc, once"],
        );
    });
}
