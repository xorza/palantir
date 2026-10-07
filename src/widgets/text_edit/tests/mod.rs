use crate::widgets::text_edit::TextEditState;
use crate::widgets::text_edit::action::EditAction;
use crate::widgets::text_edit::edit_state::EditState;
use crate::widgets::text_edit::editor::Editor;
use crate::widgets::text_edit::input_pass::{KeyOutcome, apply_key as apply_editor_key};
use crate::widgets::text_edit::unicode::{
    next_grapheme_boundary, next_word_boundary, prev_grapheme_boundary, prev_word_boundary,
    word_range_at,
};

use crate::Spacing;
use crate::Ui;
use crate::common::clipboard::Clipboard;
use crate::common::platform::{PLATFORM, Platform};
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::modifiers::Modifiers;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::shape::record::ShapeRecord;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::text_edit::TextEdit;
use glam::{UVec2, Vec2};
use std::iter;

fn apply_key(text: &mut String, state: &mut EditState, kp: KeyPress) -> bool {
    let clipboard = Clipboard::memory();
    apply_key_with_clipboard(text, state, kp, &clipboard)
}

fn apply_key_with_clipboard(
    text: &mut String,
    state: &mut EditState,
    kp: KeyPress,
    clipboard: &Clipboard,
) -> bool {
    let mut ed = Editor::new(text, state, false, None);
    let blur = match EditAction::from_keypress(kp) {
        Some(action) => {
            action.execute(&mut ed, clipboard);
            false
        }
        None => apply_editor_key(&mut ed, kp) == KeyOutcome::Blur,
    };
    ed.observe_text();
    blur
}

/// Every shape a widget paints, descendants included: a [`Text`](crate::Text) paints on its own leaf, a [`TextEdit`] on the block child carrying its alignment ([`block_of`]).
fn painted_shapes(ui: &Ui, node: NodeId) -> impl Iterator<Item = &ShapeRecord> + '_ {
    let tree = ui.tree(Layer::Main);
    iter::once(node)
        .chain(tree.children(node).map(|child| child.id))
        .flat_map(move |n| tree.shapes_of(n))
}

/// The block child a field records its shapes on. The run, selection wash and caret go on a child whose placement inside the inner rect is the text alignment, so layout resolves it against the freshly arranged rect. See [`PaintInput::record`](crate::widgets::text_edit::paint_input::PaintInput::record).
fn block_of(ui: &Ui, field: NodeId) -> NodeId {
    ui.tree(Layer::Main)
        .children(field)
        .next()
        .expect("the field records one block child")
        .id
}

fn press(key: Key) -> KeyPress {
    KeyPress::with(key, Modifiers::NONE)
}

const SMALL: UVec2 = UVec2::new(200, 80);
const WIDE: UVec2 = UVec2::new(400, 80);
const NARROW: UVec2 = UVec2::new(300, 80);

fn editor_only(buf: &mut String) -> impl FnMut(&mut Ui) + '_ {
    |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("editor"))
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    }
}

fn shift(key: Key) -> KeyPress {
    KeyPress::with(key, Modifiers::SHIFT)
}

/// Primary-modifier + key, the chord for select-all / copy / cut / paste. `Modifiers::ctrl` is the platform command bit (Cmd on macOS).
fn ctrl_press(key: Key) -> KeyPress {
    KeyPress::with(key, Modifiers::CTRL)
}

fn ctrl_shift_press(key: Key) -> KeyPress {
    let mut kp = ctrl_press(key);
    kp.mods.shift = true;
    kp
}

fn editor_and_button(buf: &mut String) -> impl FnMut(&mut Ui) + '_ {
    use crate::widgets::button::Button;
    |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("editor"))
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
            // Not focusable: these tests press a widget a click doesn't focus, and `Button` is focusable by default.
            Button::new()
                .id(WidgetId::from_hash("plain"))
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .focusable(false)
                .show(ui);
        });
    }
}

fn editor_at(buf: &mut String, padding: Option<Spacing>) -> impl FnMut(&mut Ui) + '_ {
    move |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            let mut e = TextEdit::new(buf)
                .id(WidgetId::from_hash("ed"))
                .size((Sizing::fixed(280.0), Sizing::fixed(40.0)));
            if let Some(p) = padding {
                e = e.padding(p);
            }
            e.show(ui);
        });
    }
}

/// Multi-line flag: `Enter` inserts `\n`, paste preserves newlines, cursor navigation is 2D. Driven via `apply_key`; the show()+layout path is covered by `align_per_line::multiline_widget_right_aligns_each_line`.
fn multiline_editor(buf: &mut String) -> impl FnMut(&mut Ui) + '_ {
    |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("ml-ed"))
                .multiline(true)
                .size((Sizing::fixed(200.0), Sizing::fixed(120.0)))
                .show(ui);
        });
    }
}

mod align;
mod align_per_line;
mod apply_key;
mod blink;
mod click;
mod context_menu;
mod grapheme;
mod ime;
mod measure;
mod multi_click;
mod multiline;
mod response;
mod scroll;
mod selection;
mod theme;
mod undo;
mod word_nav;
