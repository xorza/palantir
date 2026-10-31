//! The editable text leaf and the machinery one edit costs: buffer, selection and undo
//! ([`editor`], [`edit_state`], [`history`]), keys and gestures ([`action`], [`input_pass`]),
//! the input method's composition ([`composition`]),
//! placement and caret mapping ([`text_layout`], [`text_geometry`], [`shape_ctx`],
//! [`paint_input`]), retained scroll and focus ([`view_state`]), and
//! grapheme/word walks ([`unicode`]).

mod action;
#[cfg(feature = "internals")]
pub(crate) mod bench;
mod composition;
pub(crate) mod edit_end;
mod edit_state;
mod editor;
mod history;
mod input_pass;
mod paint_input;
mod shape_ctx;
mod text_geometry;
mod text_layout;
mod text_nav;
mod unicode;
mod view_state;

use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::text::text_input::TextInput;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::text_edit::TextEditTheme;
use crate::theme::text_style_overrides::TextStyleOverrides;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::{Response, ResponseSnapshot};
use crate::widget_core::widget::Widget;
use crate::widgets::text_edit::composition::{Composition, CompositionPass, Shown};
use crate::widgets::text_edit::edit_end::EditEnd;
use crate::widgets::text_edit::edit_state::EditState;
use crate::widgets::text_edit::editor::{EditRules, Editor};
use crate::widgets::text_edit::history::EditKind;
use crate::widgets::text_edit::input_pass::{AcceptPolicy, InputPass, InputResult};
use crate::widgets::text_edit::paint_input::{CaretPaint, PaintInput};
use crate::widgets::text_edit::text_geometry::{GeometryInput, TextGeometry};
use crate::widgets::text_edit::text_layout::{LayoutInput, TextLayout};
use crate::widgets::text_edit::view_state::{FocusEdges, ViewState, ViewUpdateInput};
use glam::Vec2;
use std::mem;

#[derive(Clone, Default, Debug)]
struct TextEditState {
    edit: EditState,
    view: ViewState,
    /// Selection wash for the painter, retained so a held drag allocates once
    /// (`long_multiline_selection_alloc_free`). On the row, not in [`ViewState`], so its
    /// borrow stays disjoint from `ViewState::update`.
    selection_rects: Vec<Rect>,
    /// An interned placeholder's characters, copied out of the arena so the pass can measure them
    /// while holding `&mut Ui`.
    placeholder: String,
    /// The focus session has an uncommitted result; stops a blur after Enter committing twice.
    commit_pending: bool,
    composition: Composition,
    /// A [buffered](TextEdit::buffered) field's text, which the field edits in place of a bound
    /// `String`.
    draft: String,
    /// A buffered field's current value, copied out so an interned one is read while the pass
    /// holds `&mut Ui`.
    current: String,
}

#[derive(Clone, Copy, Debug)]
struct CommitPass {
    focus: FocusEdges,
    changed: bool,
    ended: Option<EditEnd>,
    disabled: bool,
}

impl TextEditState {
    const fn roll_commit(&mut self, pass: CommitPass) -> bool {
        if pass.disabled || matches!(pass.ended, Some(EditEnd::Canceled)) {
            self.commit_pending = false;
            return false;
        }
        if pass.focus.gained || pass.changed {
            self.commit_pending = true;
        }
        let committed = matches!(pass.ended, Some(EditEnd::Submitted))
            || (pass.focus.lost && self.commit_pending);
        if committed || pass.focus.lost {
            self.commit_pending = false;
        }
        committed
    }
}

/// Editable text leaf: typing (see [`KeyText`](crate::KeyText)), caret motion,
/// drag-select, multi-line, cut/copy/paste, undo/redo, escape-to-blur and
/// click-to-place-caret. [`Self::new`] borrows `&'a mut String`, the host's storage, and edits it
/// live; [`Self::buffered`] edits a draft of its own and hands the result back on commit.
///
/// While focused it asks for IME text ([`Ui::request_ime`]): the composition shows
/// at the caret and the bound `String` changes only on commit. Commits type as keys
/// do, so control characters (newlines included) are dropped.
///
/// The wheel pans only along the axis the text overflows (x single-line, y for
/// [`Self::multiline`]) and reaches the container behind on the other.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct TextEdit<'a> {
    widget: Widget,
    /// The text the field edits; `show` takes it.
    text: Option<Bound<'a>>,
    style: Option<&'a TextEditTheme>,
    overrides: TextStyleOverrides,
    placeholder: TextInput<'a>,
    /// Enter inserts `\n`, paste keeps newlines, and text soft-wraps. Set via [`Self::multiline`].
    multiline: bool,
    /// Caller alignment of the text in the inner rect; `None` picks by mode.
    text_align: Option<Align>,
    /// Max characters (Unicode scalar values); `None` is unbounded. Overflowing input is dropped.
    max_chars: Option<usize>,
    /// Select the whole buffer when focus arrives without a same-frame press, so the first
    /// keystroke replaces it.
    select_all_on_focus: bool,
    escape_falls_through: bool,
}

/// What a [`TextEdit`] edits.
#[derive(Debug)]
enum Bound<'a> {
    /// The caller's `String`, edited in place.
    Live(&'a mut String),
    /// A draft the field keeps, filled from this value while the field is idle.
    Buffered(TextInput<'a>),
}

impl<'a> TextEdit<'a> {
    /// A single-line editor over `text`, edited in place.
    #[track_caller]
    pub fn new(text: &'a mut String) -> Self {
        Self::over(Bound::Live(text))
    }

    /// A single-line editor of a draft the field keeps under its id, for a value the caller
    /// stores in another form or changes only on a whole edit. While the field is idle, focused
    /// neither this frame nor the last, the draft takes `current`; Escape puts `current` back;
    /// and the frame that commits, on Enter or on the blur that ends the session, hands the draft
    /// over as [`TextEditResponse::committed_text`] when it differs from `current`. The frame
    /// focus leaves is not idle, so the draft lives to commit. The id must be stable across
    /// frames, as the draft is kept under it.
    #[track_caller]
    pub fn buffered(current: impl Into<TextInput<'a>>) -> Self {
        Self::over(Bound::Buffered(current.into()))
    }

    #[track_caller]
    fn over(text: Bound<'a>) -> Self {
        // A scrolling viewport over one child. A child, because text placement is an
        // alignment and needs the arranged rect. Scrolling, because a panned axis reports no
        // min-content, so a `Fill` field in a narrower container scrolls instead of
        // refusing to fit. `SCROLL` as well as `CLICK` so an overflowing editor has
        // somewhere to go; `pass` narrows it to the overflow axis before the node records.
        let widget = Widget::scroll(ScrollAxes::BOTH)
            .sense(Sense::CLICK | Sense::SCROLL)
            .focusable(true)
            .clip_rect();
        Self {
            widget,
            text: Some(text),
            style: None,
            overrides: TextStyleOverrides::NONE,
            placeholder: TextInput::Borrowed(""),
            multiline: false,
            text_align: None,
            max_chars: None,
            select_all_on_focus: false,
            escape_falls_through: false,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `text_edit`; all-or-nothing. To
    /// tweak one axis, share a bundle with `..ui.theme().text_edit.clone()`. Buffer
    /// font/leading/color live on the per-state `text` overrides
    /// ([`crate::TextStyleOverrides`]); unset axes inherit [`crate::Theme::text`].
    pub fn style(mut self, s: impl Into<Option<&'a TextEditTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Fill color for the buffer, overriding the resolved look's.
    ///
    /// # Panics
    ///
    /// Panics unless `color` is a [color](crate::widget::domain::color).
    #[track_caller]
    pub const fn color(mut self, color: RgbaF32) -> Self {
        self.overrides.color = Some(domain::color(color));
        self
    }

    /// Font size in logical px, a *length*, overriding the resolved look's.
    ///
    /// Named apart from [`Configure::size`], which is the widget's layout
    /// extent.
    ///
    /// # Panics
    ///
    /// Panics unless `px` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn font_size(mut self, px: f32) -> Self {
        self.overrides.font_size = Some(domain::length(px));
        self
    }

    /// Line height as a multiple of the font size, overriding the resolved
    /// look's `line_height_factor`. Sets the caret's height with it. `factor`:
    /// *positive*, as a theme file's is.
    ///
    /// # Panics
    ///
    /// Panics unless `factor` is [positive](crate::widget::domain::positive).
    #[track_caller]
    pub const fn line_height_factor(mut self, factor: f32) -> Self {
        self.overrides.line_height_factor = Some(domain::positive(factor));
        self
    }

    /// Family to shape against, overriding the resolved look's.
    pub const fn family(mut self, family: FontFamily) -> Self {
        self.overrides.family = Some(family);
        self
    }

    /// Weight to shape against, overriding the resolved look's.
    pub const fn weight(mut self, weight: FontWeight) -> Self {
        self.overrides.weight = Some(weight);
        self
    }

    /// Upright or italic, overriding the resolved look's.
    pub const fn slant(mut self, slant: FontSlant) -> Self {
        self.overrides.slant = Some(slant);
        self
    }

    /// Shape the buffer bold — [`Self::weight`] with [`FontWeight::BOLD`].
    pub const fn bold(mut self) -> Self {
        self.overrides.weight = Some(FontWeight::BOLD);
        self
    }

    /// Shape the buffer italic via [`Self::slant`]; `.bold().italic()` is bold italic.
    pub const fn italic(mut self) -> Self {
        self.overrides.slant = Some(FontSlant::Italic);
        self
    }

    /// Select the whole buffer when the field gains focus without a same-frame
    /// pointer press (e.g. via `set_focus`). Default off.
    pub const fn select_all_on_focus(mut self, on: bool) -> Self {
        self.select_all_on_focus = on;
        self
    }

    /// Drop `ESCAPE` from the field's scope so Escape resolves to the enclosing
    /// overlay. For a field that filters its container (a palette's search box); off by
    /// default because for a rename or value editor Escape is the cancel and must not
    /// reach past the field.
    pub const fn escape_falls_through(mut self, on: bool) -> Self {
        self.escape_falls_through = on;
        self
    }

    /// Cap the buffer at `n` characters. Insertions are truncated; longer existing content is left
    /// alone. `n == 0` rejects every insertion.
    pub const fn max_chars(mut self, n: usize) -> Self {
        self.max_chars = Some(n);
        self
    }

    /// Position of the text inside the inner rect. Defaults: `Align::LEFT`
    /// single-line, `Align::TOP_LEFT` multi-line. Distinct from [`Configure::align`],
    /// which positions the widget in its parent's slot.
    pub const fn text_align(mut self, a: Align) -> Self {
        self.text_align = Some(a);
        self
    }

    /// Switch to multi-line: Enter inserts `\n`, paste keeps newlines, text soft-wraps, and
    /// click/caret/selection use cosmic-text's 2D layout.
    pub const fn multiline(mut self, on: bool) -> Self {
        self.multiline = on;
        self
    }

    /// Text drawn in place of an empty buffer, focused or not; an empty `Hug` field sizes to it.
    pub fn placeholder(mut self, text: impl Into<TextInput<'a>>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Record the editor and run one frame of editing over its text.
    pub fn show(mut self, ui: &mut Ui) -> TextEditResponse<'_> {
        let id = self.widget.resolve(ui);
        let text = self
            .text
            .take()
            .expect("a field's text is taken once, by `show`");
        // The state row is held for the whole pass because its stages are separated by
        // `&mut Ui` calls; `with_state` moves it out and back without allocating.
        let (PassOutcome { edges, response }, committed_text) =
            ui.with_state::<TextEditState, _>(id, |ui, state| match text {
                Bound::Live(text) => (self.pass(ui, state, id, text), None),
                Bound::Buffered(current) => self.buffered_pass(ui, state, id, &current),
            });
        let EditEdges {
            changed,
            committed,
            ended,
            focus_gained,
            focus_lost,
        } = edges;
        TextEditResponse {
            response: Response::new(ui, response),
            changed,
            committed,
            committed_text,
            ended,
            focus_gained,
            focus_lost,
        }
    }

    /// [`Self::pass`] over the draft of a [buffered](Self::buffered) field: filled from `current`
    /// while idle and on Escape, and handed back on a commit that changed it.
    fn buffered_pass(
        self,
        ui: &mut Ui,
        state: &mut TextEditState,
        id: WidgetId,
        current: &TextInput<'_>,
    ) -> (PassOutcome, Option<String>) {
        let mut draft = mem::take(&mut state.draft);
        let mut shown = mem::take(&mut state.current);
        shown.clear();
        shown.push_str(match current {
            TextInput::Borrowed(text) => text,
            TextInput::Owned(text) => text,
            TextInput::Interned(text) => ui.text(*text),
        });
        let idle = ui.focus() != Some(id) && !state.view.was_focused();
        if idle && draft != shown {
            draft.clone_from(&shown);
        }
        let outcome = self.pass(ui, state, id, &mut draft);
        let edges = outcome.edges;
        let committed_text = (edges.committed && draft != shown).then(|| draft.clone());
        if edges.ended == Some(EditEnd::Canceled) {
            draft.clone_from(&shown);
        }
        state.draft = draft;
        state.current = shown;
        (outcome, committed_text)
    }

    fn pass(
        mut self,
        ui: &mut Ui,
        state: &mut TextEditState,
        id: WidgetId,
        text: &mut String,
    ) -> PassOutcome {
        let mut is_focused = ui.focus() == Some(id);
        // The pass's one probe, handed back by `show`. Through `Widget::response`, so
        // `disabled` is folded in; a copy taken before the fold would report a freshly
        // disabled field as live.
        let mut response = self.widget.response(ui);
        // A disabled editor must not keep focus or it would route typing / paste / undo into the
        // host's buffer. Kick it out and run this frame unfocused.
        if is_focused && response.disabled {
            ui.clear_focus();
            is_focused = false;
            response.state.focused = false;
        }
        // A focused editor takes the classes it edits with, so an app-level Ctrl+Z undoes
        // this buffer. `ACCEL` stays out (see `KeyFilter::TEXT_FIELD`) so Ctrl+S still
        // saves mid-edit. The same value gates the drain in the input pass.
        let mut filter = KeyFilter::TEXT_FIELD;
        filter.set(KeyFilter::ESCAPE, !self.escape_falls_through);
        if is_focused {
            self.widget.configure().input_scope(filter);
        }
        // `wear` substitutes theme padding/margin where unconfigured; the renderer and caret
        // hit-test both read `node.padding`.
        let theme = ui.theme();
        let slot = self.style.unwrap_or(&theme.text_edit);
        let caret_color = slot.caret;
        let caret_width = slot.caret_width;
        let selection_color = slot.selection;
        let placeholder_color = slot.placeholder;
        let plan = slot.plan(&response, (), theme.text);
        let mut look = self.widget.wear(ui, plan);
        // After the look animates, so a per-axis override outranks the theme in every state.
        look.text = self.overrides.apply(&look.text);
        // A face the shaper cannot be asked for shapes nothing (as `TextShape::is_noop`); caret and
        // selection derive from the face, so the layout below must not run.
        if !look.text.metrics_valid() {
            let focus = state.view.roll_focus(is_focused);
            let committed = state.roll_commit(CommitPass {
                focus,
                changed: false,
                ended: None,
                disabled: response.disabled,
            });
            let sense = self.widget.authored_sense();
            self.widget
                .configure()
                .sense(sense.difference(Sense::SCROLL));
            self.widget.record(ui, Some(&look.chrome), |_| {});
            return PassOutcome {
                edges: EditEdges {
                    changed: false,
                    committed,
                    ended: None,
                    focus_gained: focus.gained,
                    focus_lost: focus.lost,
                },
                response,
            };
        }
        let font = look.text.font();
        // `Tree::open_node` folds chrome stroke width into the stored padding, and the
        // encoder's clip mask is `rect.deflated_by(post-inflate padding)`, so glyph and
        // caret coordinates must use that value or the top glyph row is scissored away.
        let stroke_w = look.chrome.border_inset();
        let padding = Spacing::saturating(
            self.widget
                .authored_padding()
                .unwrap()
                .to_array()
                .map(|v| v + stroke_w),
        );
        let prev_block_offset = state.view.block_offset;
        let layout = TextLayout::resolve(LayoutInput {
            response_rect: response.layout_rect,
            padding,
            caret_width,
            font,
            multiline: self.multiline,
            text_align: self.text_align,
            prev_block_offset,
        });
        let ctx = layout.ctx;
        let rules = EditRules {
            multiline: self.multiline,
            max_chars: self.max_chars,
        };
        // Pre-input caret snapshot, before the input pass's clamp, so an external shrink that
        // displaces the caret still resets the blink.
        let caret_before = state.edit.caret;
        let selection_before = state.edit.selection;
        let InputResult { ended, edited } = InputPass {
            resp_state: &response,
            is_focused,
            text,
            layout: &layout,
            policy: AcceptPolicy {
                rules,
                select_all_on_focus: self.select_all_on_focus,
                filter,
            },
            edit: &mut state.edit,
            view: &state.view,
        }
        .run(ui);
        if ended == Some(EditEnd::Canceled) {
            ui.clear_focus();
            is_focused = false;
            response.state.focused = false;
        }
        let focus = state.view.roll_focus(is_focused);

        // One editing session for the whole menu pass, so one session reconciles undo
        // history once. A disabled editor offers no menu; not recording it closes any open
        // one, else Cut / Paste / Clear would keep executing.
        let menu_edited = !response.disabled && {
            let mut editor = Editor::new(text, &mut state.edit, rules);
            editor.show_menu(ui, &response, filter)
        };
        let preedit = if is_focused && !response.disabled {
            ui.ime_preedit()
        } else {
            None
        };
        let CompositionPass { started, cursor } = state.composition.roll(preedit);
        let composing = cursor.is_some();
        let cleared = started && {
            let mut editor = Editor::new(text, &mut state.edit, rules);
            let had_selection = editor.has_selection();
            if had_selection {
                editor.replace_selection("", EditKind::Delete);
            }
            had_selection
        };
        let changed = edited || menu_edited || cleared;
        let committed = state.roll_commit(CommitPass {
            focus,
            changed,
            ended,
            disabled: response.disabled,
        });
        let caret_moved =
            caret_before != state.edit.caret || selection_before != state.edit.selection;
        let caret_byte = state.edit.caret;
        let selection = state.edit.selection_range();

        let wheel = if response.disabled {
            Vec2::ZERO
        } else {
            response.scroll.pan(ctx.font.line_height)
        };

        let placeholder: &str = match &self.placeholder {
            TextInput::Borrowed(text) => text,
            TextInput::Owned(text) => text,
            TextInput::Interned(text) => {
                state.placeholder.clear();
                state.placeholder.push_str(ui.text(*text));
                &state.placeholder
            }
        };
        let Shown {
            text,
            caret: caret_byte,
            wash,
        } = match cursor {
            Some(cursor) => state.composition.splice(text, caret_byte, cursor),
            None => Shown {
                text,
                caret: caret_byte,
                wash: is_focused.then_some(selection).flatten(),
            },
        };
        let geometry = TextGeometry::resolve(
            ui,
            GeometryInput {
                layout,
                text,
                placeholder,
                caret: caret_byte,
                selection: wash,
            },
            &mut state.selection_rects,
        );
        // The probe hashed what is on show, which is not the buffer while composing.
        state.edit.observe_text_hash(if composing {
            Some(EditState::text_hash(text))
        } else {
            geometry.text_hash
        });
        let now = ui.now();
        let caret_animation = state.view.update(ViewUpdateInput {
            geometry,
            wheel,
            caret_byte,
            focused: is_focused,
            caret_moved,
            changed,
            focus_gained: focus.gained,
            now,
        });
        if is_focused
            && !response.disabled
            && let Some(layout_rect) = response.layout_rect
        {
            // The caret in screen space for the platform's candidate list, using last arrange's box
            // and block offset, and the scroll the caret follow just settled.
            let caret = geometry.caret_pos;
            let local = Rect::new(caret.x, caret.y_top, caret_width, caret.line_height);
            let scrolled = state.view.scroll.transform(Vec2::ZERO).apply_rect(local);
            let [left, top, _, _] = ctx.padding.to_array();
            let origin = layout_rect.min + Vec2::new(left, top) + geometry.block_offset;
            ui.request_ime(response.transform.apply_rect(Rect {
                min: scrolled.min + origin,
                size: scrolled.size,
            }));
        }
        // The wheel senses only the overflow axis; the other reaches the container behind.
        let sense = self.widget.authored_sense();
        self.widget
            .configure()
            .sense(sense.difference(Sense::SCROLL.difference(state.view.wheel_axes)));
        let text_color = look.text.color;
        PaintInput {
            chrome: look.chrome,
            block_id: id.with("text-block"),
            text,
            preedit_underline: composing.then_some(caret_width),
            placeholder,
            geometry,
            selection_rects: &state.selection_rects,
            selection_color,
            text_color,
            placeholder_color,
            scroll: state.view.scroll,
            caret: is_focused.then_some(CaretPaint {
                pos: geometry.caret_pos,
                width: caret_width,
                color: caret_color,
                animation: caret_animation,
            }),
        }
        .record(ui, self.widget);
        PassOutcome {
            edges: EditEdges {
                changed,
                committed,
                ended,
                focus_gained: focus.gained,
                focus_lost: focus.lost,
            },
            response,
        }
    }
}

impl Configure for TextEdit<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// What one pass did to the edit: [`TextEditResponse`]'s edges, without the `Response` that
/// borrows the `Ui`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EditEdges {
    changed: bool,
    committed: bool,
    ended: Option<EditEnd>,
    focus_gained: bool,
    focus_lost: bool,
}

/// What a pass reports while the state row is on loan.
#[derive(Clone, Copy, Debug)]
struct PassOutcome {
    edges: EditEdges,
    response: ResponseSnapshot,
}

/// What [`TextEdit::show`] returns: the widget's [`Response`] plus the edit signals computed inside
/// `show()`.
#[derive(Debug)]
pub struct TextEditResponse<'a> {
    /// The widget's pointer/click/hover [`Response`].
    pub response: Response<'a>,
    /// The buffer was edited this frame.
    pub changed: bool,
    /// The edit finished this frame and the buffer holds its result, as
    /// [`ValueResponse::committed`](crate::ValueResponse::committed): on Enter in a
    /// single-line editor or on the blur ending a focus session, never on Escape or a
    /// blur because the editor turned disabled. A session commits once.
    pub committed: bool,
    /// A [buffered](TextEdit::buffered) field's draft, on the frame it commits a text that
    /// differs from the field's current value; `None` on every other frame, and always for a
    /// field over a bound `String`, whose buffer already holds the result.
    pub committed_text: Option<String>,
    /// How the frame ended the edit, if it did: [`EditEnd::Submitted`] never in multi-line
    /// mode, and [`EditEnd::Canceled`] blurs, so [`Self::focus_lost`] fires and
    /// [`Self::committed`] does not.
    pub ended: Option<EditEnd>,
    /// The editor took focus this frame.
    pub focus_gained: bool,
    /// The editor lost focus this frame (clicked away, another widget focused, or Escape).
    pub focus_lost: bool,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::widgets::text_edit::{EditEdges, TextEditResponse};

    impl TextEditResponse<'_> {
        /// The edges, copied out of the record pass whose `ui` borrow the response holds.
        pub(super) const fn edges(&self) -> EditEdges {
            EditEdges {
                changed: self.changed,
                committed: self.committed,
                ended: self.ended,
                focus_gained: self.focus_gained,
                focus_lost: self.focus_lost,
            }
        }
    }
}

#[cfg(test)]
mod tests;
