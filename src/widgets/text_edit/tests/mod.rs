use crate::widgets::text_edit::TextEditState;
use crate::widgets::text_edit::action::EditAction;
use crate::widgets::text_edit::edit_state::EditState;
use crate::widgets::text_edit::editor::{CaretMove, EditRules, Editor};
use crate::widgets::text_edit::input_pass::{
    KeyOutcome, ProbeMotion, ProbeTarget, apply_key as apply_editor_key,
};
use crate::widgets::text_edit::text_nav::TextNav;
use crate::widgets::text_edit::unicode::{
    next_grapheme_boundary, next_word_boundary, prev_grapheme_boundary, prev_word_boundary,
    word_range_at,
};

use crate::common::clipboard::Clipboard;
use crate::common::platform::{PLATFORM, Platform};
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::modifiers::Modifiers;
use crate::internals::ui_harness::UiHarness;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::shape::record::ShapeRecord;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::text_edit::TextEdit;
use glam::{UVec2, Vec2};
use std::iter;

/// The build's own chords, for the cases that do not test them.
const NAV: TextNav = TextNav::new(PLATFORM);

fn apply_key(text: &mut String, state: &mut EditState, kp: KeyPress) -> bool {
    FieldKeys::new(Clipboard::memory()).apply(text, state, kp)
}

/// A single-line field's key handling outside a frame: the clipboard its
/// commands reach, and the platform whose chords it binds.
#[derive(Debug)]
struct FieldKeys {
    clipboard: Clipboard,
    platform: Platform,
}

impl FieldKeys {
    /// The build's own platform.
    fn new(clipboard: Clipboard) -> Self {
        Self {
            clipboard,
            platform: PLATFORM,
        }
    }

    fn on(platform: Platform) -> Self {
        Self {
            clipboard: Clipboard::memory(),
            platform,
        }
    }

    /// Whether `kp` blurred the field.
    fn apply(&self, text: &mut String, state: &mut EditState, kp: KeyPress) -> bool {
        let mut ed = Editor::new(
            text,
            state,
            EditRules {
                multiline: false,
                max_chars: None,
            },
        );
        let blur = match EditAction::from_keypress(kp) {
            Some(action) => {
                action.execute(&mut ed, &self.clipboard);
                false
            }
            None => apply_editor_key(&mut ed, kp, TextNav::new(self.platform)) == KeyOutcome::Blur,
        };
        ed.observe_text();
        blur
    }
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

/// The field most tests record: one `TextEdit` in a row, with the knobs those tests turn.
#[derive(Clone, Copy, Debug)]
struct Field {
    id: &'static str,
    size: Vec2,
    multiline: bool,
    padding: Option<Spacing>,
    /// A button beside the field, which a press can land on without taking focus.
    button: bool,
}

impl Field {
    /// A 180 × 40 single-line field.
    const EDITOR: Self = Self {
        id: "editor",
        size: Vec2::new(180.0, 40.0),
        multiline: false,
        padding: None,
        button: false,
    };

    /// A 280 × 40 single-line field, for tests that turn its padding.
    const PADDED: Self = Self {
        id: "ed",
        size: Vec2::new(280.0, 40.0),
        ..Self::EDITOR
    };

    /// A 200 × 120 multi-line field: Enter inserts `\n`, paste keeps newlines, the caret moves in 2D.
    const MULTILINE: Self = Self {
        id: "ml-ed",
        size: Vec2::new(200.0, 120.0),
        multiline: true,
        ..Self::EDITOR
    };

    const fn with_padding(self, padding: Option<Spacing>) -> Self {
        Self { padding, ..self }
    }

    const fn with_button(self) -> Self {
        Self {
            button: true,
            ..self
        }
    }

    fn scene(self, buf: &mut String) -> impl FnMut(&mut Ui) + '_ {
        use crate::widgets::button::Button;
        move |ui: &mut Ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                let mut field = TextEdit::new(buf)
                    .id(WidgetId::from_hash(self.id))
                    .multiline(self.multiline)
                    .size((Sizing::fixed(self.size.x), Sizing::fixed(self.size.y)));
                if let Some(p) = self.padding {
                    field = field.padding(p);
                }
                field.show(ui);
                if self.button {
                    // Not focusable: these tests press a widget a click doesn't focus, and `Button` is focusable by default.
                    Button::new()
                        .id(WidgetId::from_hash("plain"))
                        .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                        .focusable(false)
                        .show(ui);
                }
            });
        }
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
