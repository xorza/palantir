mod action;
#[cfg(feature = "bench")]
pub(crate) mod bench;
mod edit_state;
mod editor;
mod input;
mod menu;
mod unicode;
mod view;

use crate::input::key_class::KeyFilter;
use crate::input::response::ResponseState;
use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::layout_mode::ScrollSpec;
use crate::primitives::approx::noop_f32;
use crate::primitives::rect::Rect;
use crate::primitives::spacing::Spacing;
use crate::scene::node::Node;
use crate::ui::Ui;
use crate::widgets::response::{Response, ResponseSnapshot};
use crate::widgets::text_edit::edit_state::EditState;
use crate::widgets::text_edit::input::{AcceptPolicy, InputResult, run_input};
use crate::widgets::text_edit::view::{
    CaretPaint, GeometryInput, InteractionState, LayoutInput, PaintInput, ViewState,
    ViewUpdateInput,
};
use crate::widgets::theme::WidgetTheme;
use crate::widgets::theme::text_edit::TextEditTheme;
use crate::widgets::widget::Widget;
use glam::Vec2;
use std::borrow::Cow;

#[derive(Clone, Default, Debug)]
struct TextEditState {
    edit: EditState,
    interaction: InteractionState,
    view: ViewState,
    /// Selection wash for the painter, refilled every frame and retained
    /// so a held drag across a long multi-line selection allocates once and
    /// never again (`long_multiline_selection_alloc_free`).
    ///
    /// On the row rather than inside [`ViewState`] because the pass fills it
    /// while `ViewState::update` runs: as sibling fields the two borrows are
    /// disjoint, where a field of `view` would have to be moved out and put
    /// back around every call that touches the rest of the view.
    selection_rects: Vec<Rect>,
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
    /// Whether Escape belongs to the container rather than to this field —
    /// see [`TextEdit::escape_falls_through`].
    escape_falls_through: bool,
}

impl<'a> TextEdit<'a> {
    #[track_caller]
    pub fn new(text: &'a mut String) -> Self {
        // **A scrolling viewport over one child**, which is what a field is.
        //
        // A child, because where the text sits inside the inner rect is an
        // alignment and an alignment wants the arranged rect — so the engine
        // resolves it, and a field's first frame paints its text where its
        // second frame keeps it. A *leaf* arranges no children, so this cannot
        // be one; `Layered` scroll arranges like a ZStack and honours the
        // block's align.
        //
        // Scrolling, because that is what makes the field able to shrink under
        // its own text: a panned axis reports no min-content, so a `Fill` field
        // in a container narrower than its buffer scrolls rather than refusing
        // to fit. The leaf said the same thing through `TextWrap::Scroll`; a
        // child with a width of its own needs the viewport to say it instead.
        let mut node = Node::scroll(ScrollSpec::BOTH);
        // `SCROLL` as well as `CLICK`: the node is a scroll viewport, so
        // an editor whose content overflows has somewhere to go, and
        // without the sense the wheel routed straight past it to
        // whatever container sat behind — a multi-line editor could only
        // be panned by moving the caret.
        node.flags.set_sense(Sense::CLICK | Sense::SCROLL);
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
            escape_falls_through: false,
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

    /// Drop `ESCAPE` from the focused field's scope, so Escape resolves to
    /// whatever encloses it instead.
    ///
    /// For a field that *filters* its container rather than editing a value
    /// of its own — a palette's search box, a picker's type-ahead. There,
    /// one Escape is expected to close the whole overlay; a field that
    /// takes it first blurs itself and leaves the overlay open around a
    /// search box the user can no longer type in.
    ///
    /// Off by default, because the other archetype is the common one: an
    /// inline rename or a value editor, where Escape *is* the cancel and
    /// must not reach past the field to close the surface behind it.
    pub fn escape_falls_through(mut self) -> Self {
        self.escape_falls_through = true;
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
    /// from [`Configure::align`](crate::Configure::align), which positions the *widget* inside
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
    /// `TextEditTheme { caret: red, ..ui.theme().text_edit.clone() }`. Buffer
    /// font/leading/color live on the per-state `text` slot (a
    /// [`crate::TextStyle`]) — `None` inherits [`crate::Theme::text`]
    /// like every other text-rendering widget.
    pub fn style(mut self, s: &'a TextEditTheme) -> Self {
        self.style = Some(s);
        self
    }

    /// Take over the placement half of `from` — where the widget sits in
    /// its parent, not what it looks like.
    ///
    /// For a widget that *becomes* a `TextEdit` partway through a
    /// gesture: [`crate::DragValue`] swaps its scrub chip for an inline
    /// editor on click, and without this the field visibly moved and
    /// resized on the edit frame because padding, margin, alignment,
    /// grid placement and canvas position all vanished with the chip.
    ///
    /// The destructure below is exhaustive on purpose. A new `Node`
    /// field has to be given a policy here rather than silently
    /// disappearing across the swap — that silent loss is the whole
    /// defect this exists to prevent, and an elided `..` would let it
    /// back in.
    pub(crate) fn adopt_placement(mut self, from: Node) -> Self {
        let Node {
            // Identity and layout mode belong to the editor: it
            // deliberately shares the chip's resolved `WidgetId` (same
            // focus, same state row), and it is a scrolling text field
            // whatever the chip was.
            salt: _,
            mode: _,
            // Sizing is resolved by the caller, which pins the width to
            // the chip's last rect so a long value scrolls instead of
            // growing the row. Forwarding the raw values here would undo
            // that.
            size: _,
            min_size: _,
            max_size: _,
            // `TextEdit::new` pins `ClipMode::Rect` so glyphs cannot
            // spill past the field; a caller's clip choice must not
            // relax that.
            clip: _,
            // Box parity across the swap is the *theme's* job —
            // `DragValueTheme::from_chip` mirrors the chip's padding onto
            // the editor for exactly that reason — and the chip itself
            // resolves its padding from the theme rather than from this
            // node. Forwarding it would make the editor honour a value
            // the chip ignores, which is a new divergence rather than a
            // fix, and it perturbs the editor's intrinsic height. Margin
            // below is different: nothing else carries it, so without
            // this the field visibly shifts.
            padding: _,
            // Interior child configuration — the editor owns its one
            // child.
            gaps: _,
            justify: _,
            child_align: _,
            // The editor senses as a text field, not as a click/drag
            // chip.
            flags: _,
            // `TextEdit` wraps a `Scroll`, which overwrites `transform`
            // with its pan offset (see `scroll::scroll_wrappers`), so
            // forwarding one would read as supported while doing nothing.
            transform: _,
            // Everything below places the widget inside its parent or
            // sets its box metrics. These are what must survive.
            margin,
            align,
            position,
            grid,
            visibility,
        } = from;

        // `None` means "no caller opinion" — leave this field's own theme
        // default standing rather than overwriting it with zero.
        if let Some(margin) = margin {
            self.node.margin = Some(margin);
        }
        self.node.align = align;
        self.node.position = position;
        self.node.grid = grid;
        self.node.visibility = visibility;
        self
    }

    pub fn show(self, ui: &mut Ui) -> TextEditResponse<'_> {
        // Identity resolves on its own: `self.node` keeps being written
        // below (key filter, then `WidgetTheme::resolve`'s spacing defaults), so
        // a copy staged now would be stale by the time it records.
        let widget = ui.widget(self.node);
        let id = widget.id();
        // **The state row is moved out for the whole pass and moved back
        // after.** Every stage of the pass wants it, and the stages are
        // separated by `&mut Ui` calls — the keyboard drain, the shape
        // probe, the context menu, the record — so a borrow taken from
        // `ui` cannot survive between them. Owning it costs two moves of
        // a small struct (no allocation: the undo buffers move with it)
        // and collapses the seven per-stage lookups into one.
        //
        // [`Self::pass`] exists to make the write-back unconditional: it
        // early-returns on an unstyled editor, and a `mem::take` whose
        // write-back only runs on *some* paths silently resets the caret.
        let mut state = std::mem::take(ui.state_mut::<TextEditState>(id));
        let signals = self.pass(ui, widget, &mut state);
        *ui.state_mut::<TextEditState>(id) = state;

        TextEditResponse {
            // The pass already probed this id and tracked the one field that
            // can move under it mid-pass (focus), so it hands the state back
            // rather than paying a second cascade + layout lookup here.
            response: Response::eager(id, ui, signals.state),
            changed: signals.changed,
            submitted: signals.submitted,
            cancelled: signals.cancelled,
            gained_focus: signals.gained_focus,
            lost_focus: signals.lost_focus,
        }
    }

    /// One record pass over a state row the caller owns — see
    /// [`Self::show`] for why it is passed in rather than looked up.
    /// Returns the borrow-free half of [`TextEditResponse`].
    fn pass(mut self, ui: &mut Ui, mut widget: Widget, state: &mut TextEditState) -> EditSignals {
        let id = widget.id();
        let mut is_focused = ui.focused_id() == Some(id);
        // The pass's one probe, and what `show` hands back at the end.
        // Nothing below can move a cascade or layout answer — both are frozen
        // for the pass — so the only field kept current is `focused`, updated
        // wherever the pass moves focus.
        let mut probed = ui.response_for(id);
        // Pick the per-state look + animate its visual components.
        // Disabled wins over focus — a disabled editor that still
        // happens to hold focus paints with its disabled visuals
        // (mirrors Button). State.disabled comes from the cascade
        // (one-frame stale); OR self-disabled in for lag-free
        // response to a freshly toggled `.disabled(true)`.
        let mut response = probed;
        response.disabled |= self.node.flags.is_disabled();
        // A disabled editor must not keep keyboard focus — it would
        // paint disabled while still routing typing / paste / undo
        // into the host's buffer. Kick focus out (mirrors `DragValue`'s
        // click-to-edit path) and run this frame unfocused, so the
        // same frame's keystrokes are dropped and no caret paints.
        if is_focused && response.disabled {
            ui.request_focus(None);
            is_focused = false;
            probed.focused = false;
        }
        // A focused editor takes the classes it edits with, so an
        // app-level Ctrl+Z undoes *this buffer* rather than the document
        // behind it, and Delete removes a character rather than the
        // selected node. `ACCEL` stays out (see `KeyFilter::TEXT_FIELD`)
        // — Ctrl+S still saves mid-edit, which is exactly what an
        // exclusive capture would break.
        //
        // The same value gates the drain in `run_input`, so what this
        // field tells other readers it takes and what it acts on are one
        // fact rather than two that can disagree.
        let mut filter = KeyFilter::TEXT_FIELD;
        filter.set(KeyFilter::ESCAPE, !self.escape_falls_through);
        if is_focused {
            self.node.flags.set_key_filter(filter);
        }
        // `WidgetTheme::resolve` also substitutes theme padding/margin where
        // the builder left those fields unconfigured. The renderer
        // reads `node.padding` to deflate the buffer layout, and
        // the caret hit-test reads it back below — both see the
        // resolved value.
        let look = WidgetTheme::resolve(ui, id, &mut self.node, &response, (), self.style, |t| {
            &t.text_edit
        });
        // State-independent scalars off the same style source, copied
        // out so no theme borrow (or whole-theme clone) survives.
        let style = self.style.unwrap_or(&ui.theme().text_edit);
        let caret_color = style.caret;
        let caret_width = style.caret_width;
        let selection_color = style.selection;
        let placeholder_color = style.placeholder;
        if !look.text.metrics_valid() {
            let was_focused = state.view.prev_focused;
            state.view.prev_focused = is_focused;
            let chrome = look.background;
            widget.node = self.node;
            widget.record(ui, Some(&chrome), |_| {});
            return EditSignals {
                changed: false,
                submitted: false,
                cancelled: false,
                gained_focus: is_focused && !was_focused,
                lost_focus: was_focused && !is_focused,
                state: probed,
            };
        }
        let font = look.text.font();
        // `Tree::open_node` folds chrome stroke width into the stored
        // padding so children sit inside the painted stroke ring (see
        // `scene/tree/mod.rs::open_node`). Encoder's clip mask is
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
        let previous_block_offset = state.view.block_offset;
        let layout = view::resolve_layout(LayoutInput {
            response_rect: response.layout_rect,
            padding,
            caret_width,
            font,
            multiline: self.multiline,
            text_align: self.text_align,
            previous_block_offset,
        });
        let ctx = layout.ctx;
        // Pre-input caret snapshot, taken before `run_input`'s clamp so
        // an external buffer shrink that displaces the caret still reads
        // as motion (blink reset). Compared once, after the menu pass:
        // the keyboard drain and the menu drain are mutually exclusive
        // — an open popup's scope silences the layer below it — but
        // either can move the caret, and one bracket covers both.
        let caret_before = state.edit.caret;
        let sel_before = state.edit.selection;
        let InputResult {
            was_focused,
            blur: blur_after,
            submitted,
            edited,
        } = run_input(
            ui,
            &response,
            is_focused,
            self.text,
            &layout,
            AcceptPolicy {
                max_chars: self.max_chars,
                select_all_on_focus: self.select_all_on_focus,
                filter,
            },
            state,
        );
        if blur_after {
            ui.request_focus(None);
            is_focused = false;
            probed.focused = false;
        }
        let gained_focus = is_focused && !was_focused;
        let lost_focus = was_focused && !is_focused;

        let snapshot = ResponseSnapshot { id, state: probed };
        let menu_edited = menu::show(
            ui,
            &snapshot,
            self.text,
            ctx.multiline,
            self.max_chars,
            &mut state.edit,
        );
        let changed = edited || menu_edited;
        let caret_moved = caret_before != state.edit.caret || sel_before != state.edit.selection;
        let caret_byte = state.edit.caret;
        let selection = state.edit.sel_range();

        // Same pixel + line fold `Scroll` applies, against the editor's
        // own line height rather than the theme's default text size:
        // one notch should advance this editor by its own lines.
        let wheel = if response.disabled {
            Vec2::ZERO
        } else {
            response.scroll.pixels + response.scroll.lines * ctx.font.line_height_px
        };

        let geometry = view::resolve_geometry(
            ui,
            GeometryInput {
                layout,
                text: self.text,
                placeholder: &self.placeholder,
                caret: caret_byte,
                selection: is_focused.then_some(selection).flatten(),
            },
            &mut state.selection_rects,
        );
        let caret_pos = geometry.caret_pos;
        state.edit.observe_text_hash(geometry.text_hash);
        let now = ui.frame_runtime.time;
        let view = state.view.update(ViewUpdateInput {
            response_rect: response.layout_rect,
            ctx,
            caret_pos,
            caret_width,
            content_size: geometry.content_size,
            wheel,
            caret_byte,
            focused: is_focused,
            caret_moved,
            edited: changed,
            gained_focus,
            now,
            block_offset: geometry.block_offset,
        });
        let text_color = look.text.color;
        let placeholder = self.placeholder;
        widget.node = self.node;
        view::record(
            ui,
            widget,
            PaintInput {
                chrome: look.background,
                block_id: id.with("text-block"),
                text: self.text,
                placeholder: &placeholder,
                geometry,
                selection_rects: &state.selection_rects,
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
        EditSignals {
            changed,
            submitted,
            cancelled: blur_after,
            gained_focus,
            lost_focus,
            state: probed,
        }
    }
}

impl_configure!(TextEdit<'_>);

/// [`TextEditResponse`] minus its `Response` — what one pass can report
/// while the state row is still out on loan. `show` reunites the two
/// once the row is home and it can borrow `ui` again.
#[derive(Clone, Copy, Debug)]
struct EditSignals {
    changed: bool,
    submitted: bool,
    cancelled: bool,
    gained_focus: bool,
    lost_focus: bool,
    /// The response the pass probed, with `focused` as the pass left it —
    /// what `show` hands to [`Response::eager`] instead of re-probing.
    /// Every other field is frozen for the pass, so this is the same answer
    /// a second `response_for` would give.
    state: ResponseState,
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
    /// The user pressed Escape with no selection left to collapse — the
    /// conventional "cancel" signal.
    ///
    /// Escape also blurs, so [`Self::lost_focus`] fires alongside it. A
    /// commit-on-blur caller has to test this **first**, or a cancel is
    /// indistinguishable from clicking away.
    pub cancelled: bool,
    /// The editor took focus this frame.
    pub gained_focus: bool,
    /// The editor lost focus this frame (clicked away, another widget focused,
    /// or Escape) — the conventional "commit on blur" signal.
    pub lost_focus: bool,
}

#[cfg(test)]
mod tests;
