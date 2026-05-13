use super::{
    TextEditState, next_grapheme_boundary, next_word_boundary, prev_grapheme_boundary,
    prev_word_boundary, word_range_at,
};

/// Test wrapper: single-line `apply_key` with the vertical-motion
/// out-param ignored. Single-line tests never exercise Up/Down so the
/// motion sink is always `None`. Clipboard handling is always on
/// here; menu-intercept gating is exercised end-to-end via the
/// integration tests instead.
fn apply_key(text: &mut String, state: &mut TextEditState, kp: KeyPress) -> bool {
    let mut vert = None;
    super::apply_key(text, state, kp, false, true, &mut vert)
}
use crate::Spacing;
use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::input::keyboard::{Key, KeyPress, Modifiers};
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::support::testing::{
    click_at, run_at, run_at_acked, secondary_click_at, shapes_of, ui_with_text,
};
use crate::widgets::panel::Panel;
use crate::widgets::text_edit::TextEdit;
use glam::{UVec2, Vec2};

fn press(key: Key) -> KeyPress {
    KeyPress {
        key,
        mods: Modifiers::NONE,
        repeat: false,
    }
}

const SMALL: UVec2 = UVec2::new(200, 80);
const WIDE: UVec2 = UVec2::new(400, 80);
const NARROW: UVec2 = UVec2::new(300, 80);

fn editor_only(buf: &mut String) -> impl FnMut(&mut Ui) + '_ {
    |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id_salt("editor")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    }
}

fn shift(key: Key) -> KeyPress {
    KeyPress {
        key,
        mods: Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
        repeat: false,
    }
}

/// `Cmd+key` on macOS, `Ctrl+key` elsewhere — the platform primary
/// modifier under which shortcuts like select-all / copy / cut /
/// paste fire.
fn cmd_press(key: Key) -> KeyPress {
    let mods = if cfg!(target_os = "macos") {
        Modifiers {
            meta: true,
            ..Modifiers::NONE
        }
    } else {
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        }
    };
    KeyPress {
        key,
        mods,
        repeat: false,
    }
}

fn cmd_shift_press(key: Key) -> KeyPress {
    let mut kp = cmd_press(key);
    kp.mods.shift = true;
    kp
}

fn editor_and_button<'a>(buf: &'a mut String) -> impl FnMut(&mut Ui) + 'a {
    use crate::widgets::button::Button;
    |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id_salt("editor")
                .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                .show(ui);
            Button::new()
                .id_salt("plain")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    }
}

fn editor_at(buf: &mut String, padding: Option<Spacing>) -> impl FnMut(&mut Ui) + '_ {
    move |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            let mut e = TextEdit::new(buf)
                .id_salt("ed")
                .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)));
            if let Some(p) = padding {
                e = e.padding(p);
            }
            e.show(ui);
        });
    }
}

/// `ui_at_no_cosmic` constructs a Ui without cosmic, so the mono
/// fallback drives caret-x (8 px/char at 16 px font) — predictable
/// widths the click-positioning tests rely on.
fn ui_at_no_cosmic(size: UVec2) -> Ui {
    use crate::layout::types::display::Display;
    let mut ui = Ui::new();
    ui.display = Display::from_physical(size, 1.0);
    ui
}

/// Multi-line builder flag: `Enter` inserts `\n` (instead of being
/// ignored), `Cmd/Ctrl+V` preserves clipboard newlines, and cursor
/// navigation works in 2D. Driven via `apply_key` directly for the
/// state-machine assertions; the full show()+layout path is exercised
/// separately by `multiline_renders_multiple_visual_lines`.
fn multiline_editor(buf: &mut String) -> impl FnMut(&mut Ui) + '_ {
    |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id_salt("ml-ed")
                .multiline(true)
                .size((Sizing::Fixed(200.0), Sizing::Fixed(120.0)))
                .show(ui);
        });
    }
}

mod apply_key;
mod blink;
mod click;
mod context_menu;
mod grapheme;
mod multi_click;
mod multiline;
mod scroll;
mod selection;
mod theme;
mod undo;
mod word_nav;
