mod action;
#[cfg(feature = "internals")]
pub(crate) mod bench;
mod input;
mod menu;
pub(crate) mod model;
mod view;

use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::approx::noop_f32;
use crate::primitives::spacing::Spacing;
use crate::scene::node::{Configure, ConfigureNode, Node};
use crate::text::{SELECTION_RECTS_INLINE_CAPACITY, SelectionRects};
use crate::ui::Ui;
use crate::widgets::text_edit::input::{InputResult, handle_input};
use crate::widgets::text_edit::menu::MenuResult;
use crate::widgets::text_edit::model::EditState;
use crate::widgets::text_edit::view::{
    CaretPaint, GeometryInput, InteractionState, LayoutInput, PaintInput, ViewState,
    ViewUpdateInput,
};
use crate::widgets::theme::resolve_look;
use crate::widgets::theme::text_edit::TextEditTheme;
use crate::widgets::{Response, ResponseSnapshot};
use glam::Vec2;
use std::borrow::Cow;

#[derive(Clone, Default, Debug)]
pub(crate) struct TextEditState {
    pub(crate) edit: EditState,
    pub(crate) interaction: InteractionState,
    pub(crate) view: ViewState,
}

/// Editable text leaf. Supports typing (`KeyDown` printable chars or
/// IME `Text` commits), backspace/delete, left/right (+ shift / home /
/// end), drag-select, multi-line, cut/copy/paste, undo+redo
/// (Cmd/Ctrl+Z, Cmd/Ctrl+Shift+Z), escape-to-blur, click-to-place-caret.
///
/// Borrows `&'a mut String` for the buffer — host owns the storage and
/// the widget retains only semantic and view state. Host-side buffer
/// mutations between frames are visible immediately; persisted offsets
/// are repaired before each input pass.
#[derive(Debug)]
pub struct TextEdit<'a> {
    node: Node,
    text: &'a mut String,
    style: Option<&'a TextEditTheme>,
    placeholder: Cow<'static, str>,
    /// When `true`, Enter inserts `\n`, paste/IME preserve newlines,
    /// click hit-test + caret + selection render in 2D, and text
    /// soft-wraps to the editor's inner width via cosmic-text. v1
    /// single-line behaviour is the default — flip via [`Self::multiline`].
    multiline: bool,
    /// Caller-supplied alignment of the text inside the editor's
    /// inner rect. `None` means "pick the mode-appropriate default" —
    /// `Align::LEFT` (left + vcenter) for single-line, `Align::TOP_LEFT`
    /// for multi-line. Caret and selection rects derive from the same
    /// offset, so any alignment keeps them tracking the glyphs.
    text_align: Option<Align>,
    /// Max characters (Unicode scalar values) the buffer may hold.
    /// `None` = unbounded. Enforced at every insertion path (typing,
    /// IME/text, paste, newline): input that would overflow is dropped.
    max_chars: Option<usize>,
    /// Select the whole buffer when the field gains focus without a
    /// same-frame press (e.g. focus handed off programmatically, as
    /// [`crate::DragValue`] does on click-to-edit) so the first keystroke
    /// replaces it. A press that focuses the field still places the caret.
    select_all_on_focus: bool,
}

impl<'a> TextEdit<'a> {
    #[track_caller]
    pub fn new(text: &'a mut String) -> Self {
        let mut node = Node::leaf();
        node.flags.set_sense(Sense::CLICK);
        node.flags.set_focusable(true);
        // Clip glyphs, caret, and selection wash to the editor's own
        // rect so a `Fixed`-sized editor with long content doesn't
        // bleed over its neighbours. Chrome (background) draws before
        // the clip, so the editor's surround still paints normally.
        node.clip = Some(ClipMode::Rect);
        // `Node::padding` left at zero — `show()` substitutes
        // `theme.text_edit.padding` when the user didn't call
        // `.padding(...)`. Same renderer semantics as before; the
        // value just lives on the theme instead of hard-coded here.
        Self {
            node,
            text,
            style: None,
            placeholder: Cow::Borrowed(""),
            multiline: false,
            text_align: None,
            max_chars: None,
            select_all_on_focus: false,
        }
    }

    /// Select the whole buffer the moment the field gains focus without a
    /// same-frame pointer press — so a value handed to it (via `request_focus`)
    /// is replaced by the first keystroke. Clicking into the field still
    /// places the caret at the hit. Default off.
    pub fn select_all_on_focus(mut self) -> Self {
        self.select_all_on_focus = true;
        self
    }

    /// Cap the buffer at `n` characters. Insertions are truncated to
    /// what fits; content already longer than `n` is left alone (the
    /// cap only gates growth). `n == 0` rejects every insertion.
    pub fn max_chars(mut self, n: usize) -> Self {
        self.max_chars = Some(n);
        self
    }

    /// Position of the text inside the editor's inner rect (the rect
    /// minus padding). Defaults: `Align::LEFT` (left + vcenter) for
    /// single-line, `Align::TOP_LEFT` for multi-line. Overflow clamps
    /// the offset to zero on each axis so caret + horizontal scroll
    /// keep working when the text exceeds the inner rect. Distinct
    /// from [`Configure::align`], which positions the *widget* inside
    /// its parent's stack slot.
    pub fn text_align(mut self, a: Align) -> Self {
        self.text_align = Some(a);
        self
    }

    /// Switch to multi-line mode. Enter inserts `\n` (instead of
    /// blurring), paste / IME-text preserve newlines, text soft-wraps
    /// to the editor's inner width, and click/caret/selection all
    /// route through cosmic-text's 2D layout.
    pub fn multiline(mut self, on: bool) -> Self {
        self.multiline = on;
        self
    }

    pub fn placeholder(mut self, s: impl Into<Cow<'static, str>>) -> Self {
        self.placeholder = s.into();
        self
    }

    /// Borrow a whole TextEdit theme override — all-or-nothing. To tweak
    /// one axis, build and share a bundle:
    /// `TextEditTheme { caret: red, ..ui.theme.text_edit.clone() }`. Buffer
    /// font/leading/color live on the per-state `text` slot (a
    /// [`crate::TextStyle`]) — `None` inherits [`crate::Theme::text`]
    /// like every other text-rendering widget.
    pub fn style(mut self, s: &'a TextEditTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> TextEditResponse<'_> {
        let mut widget = ui.widget(self.node);
        let id = widget.id();
        let mut is_focused = ui.input.focused == Some(id);
        // Pick the per-state look + animate its visual components.
        // Disabled wins over focus — a disabled editor that still
        // happens to hold focus paints with its disabled visuals
        // (mirrors Button). State.disabled comes from the cascade
        // (one-frame stale); OR self-disabled in for lag-free
        // response to a freshly toggled `.disabled(true)`.
        let mut response = ui.response_for(id);
        response.disabled |= self.node.flags.is_disabled();
        // A disabled editor must not keep keyboard focus — it would
        // paint disabled while still routing typing / paste / undo
        // into the host's buffer. Kick focus out (mirrors `DragValue`'s
        // click-to-edit path) and run this frame unfocused, so the
        // same frame's keystrokes are dropped and no caret paints.
        if is_focused && response.disabled {
            ui.request_focus(None);
            is_focused = false;
        }
        // `resolve_look` also substitutes theme padding/margin where
        // the builder left those fields unconfigured. The renderer
        // reads `node.padding` to deflate the buffer layout, and
        // the caret hit-test reads it back below — both see the
        // resolved value.
        let look = resolve_look(ui, id, &mut self.node, &response, self.style, |t| {
            &t.text_edit
        });
        // State-independent scalars off the same style source, copied
        // out so no theme borrow (or whole-theme clone) survives.
        let style = self.style.unwrap_or(&ui.theme.text_edit);
        let caret_color = style.caret;
        let caret_width = style.caret_width;
        let selection_color = style.selection;
        let placeholder_color = style.placeholder;
        if !look.text.metrics_valid() {
            let was_focused = {
                let state = ui.state_mut::<TextEditState>(id);
                let was_focused = state.view.prev_focused;
                state.view.prev_focused = is_focused;
                was_focused
            };
            let chrome = look.background;
            widget.node = self.node;
            widget.record(ui, Some(&chrome), |_| {});
            let state = ui.response_for(id);
            return TextEditResponse {
                response: Response::eager(id, ui, state),
                changed: false,
                submitted: false,
                gained_focus: is_focused && !was_focused,
                lost_focus: was_focused && !is_focused,
            };
        }
        let font_size = look.text.font_size_px;
        let line_height_px = look.text.line_height_for(font_size);
        // `Tree::open_node` folds chrome stroke width into the stored
        // padding so children sit inside the painted stroke ring (see
        // `forest/tree/mod.rs::open_node`). Encoder's clip mask is
        // `rect.deflated_by(post-inflate padding)`, so glyph + caret
        // coordinates must use the same effective value — otherwise
        // the top row of glyphs sits above the clip and gets scissored
        // away. The node's own padding stays at the pre-inflate
        // value so Tree's fold reproduces the same effective padding.
        let stroke_w = if noop_f32(look.background.stroke.width) {
            0.0
        } else {
            look.background.stroke.width
        };
        let padding =
            Spacing::from_array(self.node.padding.unwrap().as_array().map(|v| v + stroke_w));
        let previous_block_offset = ui
            .try_state::<TextEditState>(id)
            .map_or(Vec2::ZERO, |state| state.view.block_offset);
        let layout = view::resolve_layout(LayoutInput {
            response_rect: response.layout_rect,
            padding,
            caret_width,
            font_size,
            line_height_px,
            family: look.text.family,
            weight: look.text.weight,
            multiline: self.multiline,
            text_align: self.text_align,
            previous_block_offset,
        });
        let ctx = layout.ctx;
        let InputResult {
            caret_moved,
            was_focused,
            blur: blur_after,
            submitted,
            edited,
        } = handle_input(
            ui,
            id,
            is_focused,
            self.text,
            &ctx,
            self.max_chars,
            self.select_all_on_focus,
        );
        if blur_after {
            ui.request_focus(None);
            is_focused = false;
        }
        let gained_focus = is_focused && !was_focused;
        let lost_focus = was_focused && !is_focused;

        let snapshot = ResponseSnapshot {
            id,
            state: ui.response_for(id),
        };
        let MenuResult {
            edited: menu_edited,
            caret_moved: menu_caret_moved,
        } = menu::show(ui, id, &snapshot, self.text, ctx.multiline, self.max_chars);
        let changed = edited || menu_edited;
        let caret_moved = caret_moved || menu_caret_moved;
        let (caret_byte, selection) = {
            let state = ui.state_mut::<TextEditState>(id);
            (state.edit.caret, state.edit.sel_range())
        };

        let mut retained = ui
            .state_mut::<TextEditState>(id)
            .view
            .selection_rects
            .take();
        let mut inline = SelectionRects::new();
        let selection_rects = retained.as_deref_mut().unwrap_or(&mut inline);
        let geometry = view::resolve_geometry(
            &ui.resources.text,
            GeometryInput {
                layout,
                text: self.text,
                placeholder: &self.placeholder,
                caret: caret_byte,
                selection: is_focused.then_some(selection).flatten(),
            },
            selection_rects,
        );
        let layout = geometry.layout;
        let caret_pos = geometry.caret_pos;
        ui.state_mut::<TextEditState>(id)
            .edit
            .observe_text_hash(geometry.text_hash);
        let now = ui.frame_runtime.time;
        let view = ui
            .state_mut::<TextEditState>(id)
            .view
            .update(ViewUpdateInput {
                response_rect: response.layout_rect,
                ctx,
                caret_pos,
                caret_width,
                content_width: layout.content_width,
                focused: is_focused,
                caret_moved,
                edited: changed,
                gained_focus,
                now,
                block_offset: layout.ctx.block_offset,
            });
        let text_color = look.text.color;
        let placeholder = self.placeholder;
        view::record(
            ui,
            widget,
            PaintInput {
                node: self.node,
                chrome: look.background,
                text: self.text,
                placeholder: &placeholder,
                layout,
                selection_rects,
                selection_color,
                text_color,
                placeholder_color,
                scroll: view.scroll,
                caret: is_focused.then_some(CaretPaint {
                    pos: caret_pos,
                    width: caret_width,
                    color: caret_color,
                    anim: view.caret_anim,
                }),
            },
        );
        if retained.is_none() && inline.len() > SELECTION_RECTS_INLINE_CAPACITY {
            retained = Some(Box::new(inline));
        }
        ui.state_mut::<TextEditState>(id).view.selection_rects = retained;

        let state = ui.response_for(id);
        TextEditResponse {
            response: Response::eager(id, ui, state),
            changed,
            submitted,
            gained_focus,
            lost_focus,
        }
    }
}

impl Configure for TextEdit<'_> {
    fn node_mut(&mut self) -> ConfigureNode<'_> {
        self.node.node_mut()
    }
}

/// What [`TextEdit::show`] returns: the widget's [`Response`] plus the
/// edit-specific signals computed *inside* `show()`. Callers read
/// commit/focus state from here instead of re-polling `ui` for focus
/// and key presses, which is both terser and authoritative (the editor
/// knows what it did with the input this frame).
#[derive(Debug)]
pub struct TextEditResponse<'a> {
    /// The widget's pointer/click/hover [`Response`].
    pub response: Response<'a>,
    /// The buffer was edited this frame (characters inserted or removed).
    pub changed: bool,
    /// The user pressed Enter in a single-line editor — the conventional
    /// "accept" signal. Always `false` in multi-line mode (Enter inserts `\n`).
    pub submitted: bool,
    /// The editor took focus this frame.
    pub gained_focus: bool,
    /// The editor lost focus this frame (clicked away, another widget focused,
    /// or Escape) — the conventional "commit on blur" signal.
    pub lost_focus: bool,
}

#[cfg(test)]
mod tests;
