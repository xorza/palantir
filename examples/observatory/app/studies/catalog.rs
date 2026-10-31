//! The catalog study: every object as a table that filters, sorts and moves by keyboard, its rows
//! virtualized, so only the visible ones record.

use palantir::glam::Vec2;
use palantir::{
    Align, Block, Chrome, Configure, ContextMenu, EditEnd, EditableLabel, Key, MenuItem, Panel,
    Scroll, Sense, Shortcut, Sizing, Text, TextEdit, Ui, VAlign, WidgetId, fmt,
};

use crate::app::model::Observatory;
use crate::app::model::catalog::{Catalog, ObjectId};
use crate::app::model::sky::{Dms, Hms};
use crate::app::model::view::Column;
use crate::app::scaffold::{StudyHeader, Tokens, api};

/// Every row's height, so a scroll offset names the first visible row.
pub(crate) const ROW_HEIGHT: f32 = 26.0;
/// The rows' scroll.
pub(crate) fn rows_id() -> WidgetId {
    WidgetId::from_hash("observatory::catalog::rows")
}

/// The search field, which Ctrl+F focuses.
fn search_id() -> WidgetId {
    WidgetId::from_hash("observatory::catalog::search")
}

/// The columns, their title, width and alignment, numbers to the right so digits line up; the
/// note takes what is left.
const COLUMNS: [(Column, &str, f32, Align); 5] = [
    (Column::Name, "Name", 110.0, Align::LEFT),
    (Column::Kind, "Type", 90.0, Align::LEFT),
    (Column::Mag, "Mag", 70.0, Align::RIGHT),
    (Column::Ra, "RA", 130.0, Align::RIGHT),
    (Column::Dec, "Dec", 110.0, Align::RIGHT),
];
/// The gap after each column.
const CELL_GAP: f32 = 12.0;

pub(crate) fn header(ui: &mut Ui, t: Tokens) {
    StudyHeader {
        title: "Catalog",
        blurb: "Every object of the generated sky as a table: type to filter, click a header to \
                sort, move with the keyboard. Only the rows in view record, so the 4,060 rows \
                cost what a screenful does.",
        keys: &[
            "Ctrl+F",
            "Esc",
            "↑↓",
            "PgUp",
            "PgDn",
            "Home",
            "End",
            "F2",
            "Ctrl+C",
            "right-click",
        ],
        apis: &[
            api!(type palantir::TextEdit),
            api!(Scroll::offset),
            api!(EditableLabel::edit),
            api!(type palantir::ContextMenu),
            api!(type palantir::Clipboard),
        ],
        showcase: &[
            "text input",
            "scroll & split",
            "overlays",
            "focus & keyboard",
        ],
        source: "examples/observatory/app/studies/catalog.rs",
    }
    .show(ui, t);
}

/// What the table asks of the model this frame, applied once it has recorded.
#[derive(Debug)]
enum Action {
    Select(ObjectId),
    Plan(ObjectId),
    Copy(ObjectId),
    Note(ObjectId, String),
}

pub(crate) fn show(ui: &mut Ui, model: &mut Observatory, t: Tokens) {
    search(ui, model, t);
    Panel::hstack()
        .id_salt("columns")
        .size((Sizing::FILL, Sizing::HUG))
        // The rows' inset plus the table's border, so each title sits over its column.
        .padding((9.0, 0.0))
        .show(ui, |ui| {
            let sort = model.view.sort();
            for (column, title, width, align) in COLUMNS {
                let mark = match (sort.column == column, sort.descending) {
                    (false, _) => "",
                    (true, false) => " \u{25b2}",
                    (true, true) => " \u{25bc}",
                };
                let clicked = Text::new(fmt!(ui, "{title}{mark}"))
                    .id_salt(title)
                    .style(&t.dim())
                    .sense(Sense::CLICK)
                    .text_align(align)
                    .size((Sizing::fixed(width), Sizing::HUG))
                    .padding((0.0, 0.0, CELL_GAP, 0.0))
                    .show(ui)
                    .clicked();
                if clicked {
                    model.view.sort_by(column);
                }
            }
            Text::new("Note").style(&t.dim()).show(ui);
        });
    if let Some(action) = table(ui, model, t) {
        apply(ui, model, action);
    }
}

fn search(ui: &mut Ui, model: &mut Observatory, t: Tokens) {
    if ui.shortcut_pressed(Shortcut::ctrl('F')) {
        ui.set_focus(search_id());
    }
    Panel::hstack()
        .id_salt("search")
        .size((Sizing::FILL, Sizing::HUG))
        .gap(12.0)
        .show(ui, |ui| {
            let field = TextEdit::new(&mut model.view.query)
                .id(search_id())
                .placeholder("Search names and types")
                .size((Sizing::fixed(320.0), Sizing::HUG))
                .show(ui);
            let (changed, canceled) = (field.changed, field.ended == Some(EditEnd::Canceled));
            if canceled {
                model.view.query.clear();
            }
            if changed || canceled {
                model.view.mark_stale();
            }
            let shown = model.view.rows(&model.catalog).len();
            Text::new(fmt!(ui, "{shown} of {}", model.catalog.len()))
                .style(&t.caption())
                .show(ui);
        });
}

/// The rows in view between two spacers, and the keys that move the selection.
#[expect(
    clippy::cast_sign_loss,
    reason = "a viewport height and a scroll offset are not negative"
)]
fn table(ui: &mut Ui, model: &mut Observatory, t: Tokens) -> Option<Action> {
    let viewport = ui
        .response_for(rows_id())
        .rect
        .map_or(ROW_HEIGHT * 20.0, |rect| rect.size.h);
    let visible = (viewport / ROW_HEIGHT).ceil() as usize + 1;
    let count = model.view.rows(&model.catalog).len();
    let selected = model
        .selection
        .and_then(|id| model.view.position(&model.catalog, id));
    let mut offset = Scroll::offset(ui, rows_id()).y;
    // A selection made elsewhere (the chart, the plan) scrolls its row into view once.
    let revealed = WidgetId::from_hash("observatory::catalog::revealed");
    let moved_elsewhere = ui.with_state::<Option<ObjectId>, _>(revealed, |_, last| {
        let moved = *last != model.selection;
        *last = model.selection;
        moved
    });
    if moved_elsewhere && let Some(idx) = selected {
        offset = into_view(offset, viewport, idx);
        Scroll::set_offset(ui, rows_id(), Vec2::new(0.0, offset));
    }
    let mut action = None;
    let rows = model.view.rows(&model.catalog);
    Scroll::vertical()
        .id(rows_id())
        .size((Sizing::FILL, Sizing::FILL))
        .chrome(t.card())
        .show(ui, |ui| {
            if let Some(next) = moved(ui, selected, count, visible.saturating_sub(2)) {
                action = Some(Action::Select(rows[next]));
                offset = into_view(offset, viewport, next);
                Scroll::set_offset(ui, rows_id(), Vec2::new(0.0, offset));
            }
            let first = ((offset / ROW_HEIGHT).floor().max(0.0) as usize).min(count);
            let last = (first + visible).min(count);
            Panel::vstack()
                .id_salt("rows")
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui, |ui| {
                    spacer(ui, "above", first);
                    for (idx, &id) in rows.iter().enumerate().take(last).skip(first) {
                        if let Some(row_action) =
                            row(ui, &model.catalog, id, selected == Some(idx), t)
                        {
                            action = Some(row_action);
                        }
                    }
                    spacer(ui, "below", count - last);
                });
            if let Some(idx) = selected {
                let id = rows[idx];
                if ui.shortcut_pressed(Shortcut::ctrl('C')) {
                    action = Some(Action::Copy(id));
                }
                if ui.key_pressed(Key::F2) {
                    EditableLabel::edit(ui, note_id(id));
                }
            }
        });
    action
}

/// The row the arrows, the page keys, Home and End move the selection to, if one was pressed.
fn moved(ui: &mut Ui, selected: Option<usize>, count: usize, page: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let cur = selected.unwrap_or(0);
    let last = count - 1;
    let next = if ui.key_pressed(Key::ArrowDown) {
        selected.map_or(0, |idx| (idx + 1).min(last))
    } else if ui.key_pressed(Key::ArrowUp) {
        cur.saturating_sub(1)
    } else if ui.key_pressed(Key::PageDown) {
        (cur + page.max(1)).min(last)
    } else if ui.key_pressed(Key::PageUp) {
        cur.saturating_sub(page.max(1))
    } else if ui.key_pressed(Key::Home) {
        0
    } else if ui.key_pressed(Key::End) {
        last
    } else {
        return None;
    };
    Some(next)
}

/// The offset that shows row `idx` whole, moving the least from `offset`.
pub(crate) fn into_view(offset: f32, viewport: f32, idx: usize) -> f32 {
    let top = idx as f32 * ROW_HEIGHT;
    let bottom = top + ROW_HEIGHT;
    if top < offset {
        top
    } else if bottom > offset + viewport {
        bottom - viewport
    } else {
        offset
    }
}

/// The height of `rows` rows the table does not record.
fn spacer(ui: &mut Ui, salt: &'static str, rows: usize) {
    Block::new()
        .id_salt(salt)
        .size((Sizing::FILL, Sizing::fixed(rows as f32 * ROW_HEIGHT)))
        .show(ui);
}

pub(crate) fn row_id(id: ObjectId) -> WidgetId {
    WidgetId::from_hash(("observatory::catalog::row", id.0))
}

pub(crate) fn note_id(id: ObjectId) -> WidgetId {
    WidgetId::from_hash(("observatory::catalog::note", id.0))
}

fn row(ui: &mut Ui, catalog: &Catalog, id: ObjectId, selected: bool, t: Tokens) -> Option<Action> {
    let object = catalog.get(id);
    let chrome = if selected {
        Chrome::fill(t.accent.with_alpha(0.22))
    } else {
        Chrome::NONE
    };
    let mut action = None;
    let response = Panel::hstack()
        .id(row_id(id))
        .sense(Sense::CLICK)
        .focusable(true)
        .size((Sizing::FILL, Sizing::fixed(ROW_HEIGHT)))
        .padding((8.0, 0.0))
        .child_align(Align::v(VAlign::Center))
        .chrome(chrome)
        .show(ui, |ui| {
            let cells = [
                (fmt!(ui, "{}", object.name), false),
                (fmt!(ui, "{}", object.kind.label()), false),
                (fmt!(ui, "{:.2}", object.mag), true),
                (fmt!(ui, "{}", Hms(object.ra)), true),
                (fmt!(ui, "{}", Dms(object.dec)), true),
            ];
            for ((text, mono), (_, _, width, align)) in cells.into_iter().zip(COLUMNS) {
                let style = if mono { t.mono(t.ink) } else { t.body() };
                Text::new(text)
                    .style(&style)
                    .text_align(align)
                    .size((Sizing::fixed(width), Sizing::HUG))
                    .padding((0.0, 0.0, CELL_GAP, 0.0))
                    .show(ui);
            }
            // Only the selected row edits its note, so the table reads as text, not a form.
            if selected {
                let note = EditableLabel::new(object.note.as_str())
                    .id(note_id(id))
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui);
                if let Some(text) = note.committed_text {
                    action = Some(Action::Note(id, text));
                }
            } else {
                Text::new(object.note.as_str()).style(&t.dim()).show(ui);
            }
        })
        .response;
    if response.clicked() {
        action = Some(Action::Select(id));
    }
    let snapshot = response.snapshot();
    let menu = ContextMenu::on(&snapshot).show(ui, |ui, close| {
        let plan = MenuItem::new("Add to plan").show(ui, close).clicked();
        let copy = MenuItem::new("Copy row")
            .shortcut_hint(Shortcut::ctrl('C'))
            .show(ui, close)
            .clicked();
        if plan {
            Some(Action::Plan(id))
        } else if copy {
            Some(Action::Copy(id))
        } else {
            None
        }
    });
    menu.inner.flatten().or(action)
}

fn apply(ui: &Ui, model: &mut Observatory, action: Action) {
    match action {
        Action::Select(id) => model.select(id),
        Action::Plan(id) => {
            model.add_to_plan(id);
        }
        Action::Copy(id) => {
            ui.clipboard().set_text(&row_text(&model.catalog, id)).ok();
        }
        Action::Note(id, text) => model.catalog.get_mut(id).note = text,
    }
}

/// A row as tab-separated text, as a spreadsheet pastes it.
pub(crate) fn row_text(catalog: &Catalog, id: ObjectId) -> String {
    let o = catalog.get(id);
    format!(
        "{}\t{}\t{:.2}\t{}\t{}\t{}",
        o.name,
        o.kind.label(),
        o.mag,
        Hms(o.ra),
        Dms(o.dec),
        o.note
    )
}
