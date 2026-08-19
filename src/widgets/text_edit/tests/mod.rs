use crate::common::hash;
use crate::widgets::text_edit::TextEditState;
use crate::widgets::text_edit::edit_state::EditState;
use crate::widgets::text_edit::editor::Editor;
use crate::widgets::text_edit::input::{
    KeyOutcome, apply_key as apply_editor_key, dispatch_action,
};
use crate::widgets::text_edit::unicode::{
    next_grapheme_boundary, next_word_boundary, prev_grapheme_boundary, prev_word_boundary,
    word_range_at,
};

fn apply_key(text: &mut String, state: &mut EditState, kp: KeyPress) -> bool {
    let clipboard = Clipboard::default();
    apply_key_with_clipboard(text, state, kp, &clipboard)
}

fn apply_key_with_clipboard(
    text: &mut String,
    state: &mut EditState,
    kp: KeyPress,
    clipboard: &Clipboard,
) -> bool {
    let mut ed = Editor::new(text, state, false, None);
    let blur = !dispatch_action(&mut ed, kp, clipboard)
        && apply_editor_key(&mut ed, kp) == KeyOutcome::Blur;
    let text_hash = hash::hash_str(ed.text);
    ed.state.observe_text_hash(text_hash);
    blur
}
use crate::Spacing;
use crate::Ui;
use crate::common::clipboard::Clipboard;
use crate::common::platform::{PLATFORM, Platform};
use crate::input::input_event::InputEvent;
use crate::input::keyboard::{Key, KeyPress, Modifiers};
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::panel::Panel;
use crate::widgets::text_edit::TextEdit;
use glam::{UVec2, Vec2};

/// Every shape a widget paints, its descendants' included.
///
/// A [`Text`](crate::Text) paints on its own leaf; a [`TextEdit`] paints on the
/// block child that carries its alignment — see [`block_of`] — so a reader
/// asking "what did this widget draw" has to look through the subtree rather
/// than at one node. Written once here because several tests below hand the
/// same closure both kinds.
fn painted_shapes(
    ui: &Ui,
    node: crate::scene::tree::node_id::NodeId,
) -> impl Iterator<Item = &crate::scene::shapes::record::ShapeRecord> + '_ {
    let tree = &ui.forest.trees[Layer::Main];
    std::iter::once(node)
        .chain(tree.children(node).map(|child| child.id))
        .flat_map(move |n| tree.shapes_of(n))
}

/// The block child a field records its shapes on.
///
/// A field paints nothing itself but its chrome: the run, the selection wash
/// and the caret go on a child whose placement inside the inner rect *is* the
/// text alignment, so that the layout engine resolves it against the rect it
/// has just arranged rather than the widget guessing from last frame's. See
/// [`PaintInput::record`](crate::widgets::text_edit::paint_input::PaintInput::record).
fn block_of(
    ui: &Ui,
    field: crate::scene::tree::node_id::NodeId,
) -> crate::scene::tree::node_id::NodeId {
    ui.forest.trees[Layer::Main]
        .children(field)
        .next()
        .expect("the field records one block child")
        .id
}

fn press(key: Key) -> KeyPress {
    KeyPress {
        key,
        mods: Modifiers::NONE,
        repeat: false,
        physical: Key::Other,
    }
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
    KeyPress {
        key,
        mods: Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
        repeat: false,
        physical: Key::Other,
    }
}

/// Primary-modifier + key — the chord under which shortcuts like
/// select-all / copy / cut / paste fire. `Modifiers::ctrl` is the
/// platform-normalized command bit (Cmd on macOS, Ctrl elsewhere), so
/// tests just set `ctrl`.
fn ctrl_press(key: Key) -> KeyPress {
    KeyPress {
        key,
        mods: Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
        repeat: false,
        physical: Key::Other,
    }
}

fn ctrl_shift_press(key: Key) -> KeyPress {
    let mut kp = ctrl_press(key);
    kp.mods.shift = true;
    kp
}

fn editor_and_button<'a>(buf: &'a mut String) -> impl FnMut(&mut Ui) + 'a {
    use crate::widgets::button::Button;
    |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("editor"))
                .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                .show(ui);
            Button::new()
                .id(WidgetId::from_hash("plain"))
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
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

/// `ui_at_no_cosmic` constructs a Ui without cosmic, so the mono
/// fallback drives caret-x (8 px/char at 16 px font) — predictable
/// widths the click-positioning tests rely on.
fn ui_at_no_cosmic(size: UVec2) -> UiHarness {
    UiHarness::new(size)
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
                .id(WidgetId::from_hash("ml-ed"))
                .multiline(true)
                .size((Sizing::fixed(200.0), Sizing::fixed(120.0)))
                .show(ui);
        });
    }
}

mod align;
mod apply_key;
mod blink;
mod click;
mod context_menu;
mod grapheme;
mod measure;
mod multi_click;
mod multiline;
mod response;
mod scroll;
mod selection;
mod theme;
mod undo;
mod word_nav;
