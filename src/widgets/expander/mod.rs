//! The disclosure control: a header that reveals or hides a body.

use crate::animation::anim_slot::AnimSlot;
use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::response::response_state::ResponseState;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::chevron::Chevron;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::text_input::TextInput;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use crate::shape::style::{LineCap, LineJoin};
use crate::ui::Ui;
use crate::widgets::response::Response;
use crate::widgets::text::Text;
use crate::widgets::theme::expander::ExpanderTheme;
use crate::widgets::theme::widget_look::theme_slot::ThemeSlot;
use std::rc::Rc;

/// What one pass over an [`Expander`] produced.
#[derive(Debug)]
pub struct ExpanderResponse<'a, R> {
    /// The header's response — the whole row is the hit target.
    pub response: Response<'a>,
    /// What the body closure returned, or `None` on a frame the body did
    /// not record. A collapsed [`Expander::keep_body`] section still
    /// records, so it still answers `Some`.
    pub inner: Option<R>,
    /// The header was activated this frame, by click or by key.
    pub toggled: bool,
    /// `0.0` closed, `1.0` open, in between while the reveal animates.
    pub openness: f32,
}

/// A header that reveals or hides a body — `<details>` / `<summary>` in
/// HTML, an `Expander` in WPF and GTK, a `CollapsingHeader` in egui.
///
/// ```
/// # use palantir::{Expander, Text, Ui};
/// # fn demo(ui: &mut Ui) {
/// Expander::new("Advanced")
///     .default_open(false)
///     .show(ui, |ui| {
///         Text::new("hidden until asked for").show(ui);
///     });
/// # }
/// ```
///
/// **The body does not record while closed**, so every cross-frame row
/// inside it is swept — a [`TextEdit`](crate::TextEdit)'s unsent edit, a
/// [`Scroll`](crate::Scroll)'s offset, a nested expander's own flag. A
/// section holding any of those wants [`Self::keep_body`], which records
/// it collapsed instead: live ids and zero size, at the price of a full
/// record on every frame.
///
/// The open flag lives on the widget's own id and is `false` until
/// [`Self::default_open`] says otherwise, so a section nobody touches
/// keeps no state row at all. An application that owns the flag itself —
/// a restored layout, an "expand all" — binds it with [`Self::open`]
/// instead.
#[derive(Debug)]
pub struct Expander<'a> {
    node: Node,
    label: TextInput<'a>,
    default_open: bool,
    open: Option<&'a mut bool>,
    keep_body: bool,
    style: Option<&'a ExpanderTheme>,
}

/// The reveal's `0..1` tween, on the header's id.
const SLOT_OPEN: AnimSlot = AnimSlot::new("open");

impl<'a> Expander<'a> {
    #[track_caller]
    pub fn new(label: impl Into<TextInput<'a>>) -> Self {
        Self {
            node: Node::vstack().size((Sizing::FILL, Sizing::HUG)),
            label: label.into(),
            default_open: false,
            open: None,
            keep_body: false,
            style: None,
        }
    }

    /// Whether the section starts open. Read on the first frame only —
    /// after that the widget's own flag answers. Ignored entirely when
    /// [`Self::open`] binds the flag.
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Bind the open flag to the caller's own `bool`, for an application
    /// that persists it or drives it from elsewhere. Wins over
    /// [`Self::default_open`], and the widget writes every toggle back
    /// through it.
    pub fn open(mut self, open: &'a mut bool) -> Self {
        self.open = Some(open);
        self
    }

    /// Record the body even while closed, under
    /// [`Visibility::Collapsed`](crate::Visibility::Collapsed) — laid out
    /// as if absent, painted and hit-tested not at all.
    ///
    /// The reason to pay for it is state: Palantir sweeps the
    /// cross-frame row of any widget that stops being recorded, so a
    /// skipped body loses everything inside it. Default `false`, because
    /// costing nothing while closed is what the control is for.
    pub fn keep_body(mut self, keep: bool) -> Self {
        self.keep_body = keep;
        self
    }

    style_setter!('a, ExpanderTheme, expander);

    /// Record the header, and the body under it while the section is
    /// open.
    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> ExpanderResponse<'_, R> {
        let theme = Rc::clone(ui.theme());
        let t = self.slot(&theme);
        let ambient = theme.text;
        let Self {
            node,
            label,
            default_open,
            open,
            keep_body,
            style: _,
        } = self;

        let widget = ui.widget(node);
        let id = widget.id();
        let header_id = id.with("header");
        let body_id = id.with("body");
        let stored = ui.try_state::<ExpanderState>(header_id).copied();
        let was_open = match &open {
            Some(flag) => **flag,
            None => stored.map_or(default_open, |s| s.open),
        };
        let height = stored.and_then(|s| s.height);

        let mut pass = Pass {
            header: ResponseState::default(),
            toggled: false,
            openness: 0.0,
            inner: None,
        };
        widget.record(ui, None, |ui| {
            let header_node = Node::hstack()
                .id(header_id)
                .size((Sizing::FILL, Sizing::HUG))
                .gap(t.gap)
                .child_align(Align::v(VAlign::Center))
                .sense(Sense::CLICK)
                .focusable(true)
                // Enter and Space classify as `KeyClass::Text`, so taking
                // them means claiming that class — which is right for a
                // focused header: it is not a typing target, and nothing
                // behind it should read the keys either.
                .input_scope(KeyFilter::TEXT);
            let mut header = ui.widget(header_node);
            let state = header.response(ui);
            let look = t.plan(&state, (), ambient).apply(ui, &mut header);

            let activated =
                !state.disabled && (state.left.clicked() || activation_key(ui, header_id));
            let now_open = was_open != activated;
            // No measured height yet, so a tween would have nothing to
            // clip against. Snap instead of guessing one, and animate
            // every reveal after it.
            let spec = if now_open && height.is_none() {
                None
            } else {
                t.defaults.anim
            };
            let openness = ui.animate(header_id, SLOT_OPEN, f32::from(now_open), spec);
            let arrow = Chevron { size: t.arrow_size }.rotated(t.arrow_angle(openness));
            let text = look.text;
            let label = ui.intern(label);
            header.record(ui, Some(&look.background), |ui| {
                let box_node = Node::leaf()
                    .id(header_id.with("arrow"))
                    .size((Sizing::fixed(t.arrow_size.x), Sizing::fixed(t.arrow_size.y)));
                ui.widget(box_node).record(ui, None, |ui| {
                    ui.add_shape(
                        Shape::polyline(&arrow, PolylineColors::Single(text.color), t.arrow_stroke)
                            .cap(LineCap::Round)
                            .join(LineJoin::Round),
                    );
                });
                Text::new(label)
                    .id(header_id.with("label"))
                    .style(&text)
                    .show(ui);
            });

            let showing = openness > 0.0;
            if showing || keep_body {
                let mut body_node = Node::vstack()
                    .id(body_id)
                    .size((Sizing::FILL, Sizing::HUG))
                    .margin(Spacing::new(t.indent, 0.0, 0.0, 0.0))
                    .padding(t.body_padding);
                if !showing {
                    body_node = body_node.collapsed();
                } else if let Some(full) = height.filter(|_| openness < 1.0) {
                    // The body records whole and the clip is what reveals
                    // it. Laying it out at a fraction of its height
                    // instead would reflow its text on every frame of the
                    // tween.
                    body_node = body_node
                        .max_size(Size::new(f32::INFINITY, openness * full))
                        .clip_rect();
                }
                pass.inner = Some(ui.widget(body_node).record(ui, None, body));
            }
            pass.header = state;
            pass.toggled = activated;
            pass.openness = openness;
        });

        let now_open = was_open != pass.toggled;
        if let Some(flag) = open {
            *flag = now_open;
        }
        // Measured only while the body is whole: a clipped or collapsed
        // one reports the height it was constrained to, not its own.
        let height = if pass.openness >= 1.0 {
            ui.response_for(body_id)
                .layout_rect
                .map(|r| r.size.h)
                .or(height)
        } else {
            height
        };
        let row = ExpanderState {
            open: now_open,
            height,
        };
        // Written only on a change, so a section nobody has opened mints
        // no row at all — the same probe-don't-insert path `ComboBox`
        // takes for its own open flag. An absent row *is* the state the
        // widget resolved from, which is why the comparison is against
        // that rather than against `ExpanderState::default`: a section
        // opened by `default_open` and left alone has nothing to record
        // either.
        let current = stored.unwrap_or(ExpanderState {
            open: was_open,
            height: None,
        });
        if current != row {
            *ui.state_mut::<ExpanderState>(header_id) = row;
        }

        ExpanderResponse {
            response: Response::eager(header_id, ui, pass.header),
            inner: pass.inner,
            toggled: pass.toggled,
            openness: pass.openness,
        }
    }
}

impl_configure!(Expander<'_>);

/// What the record pass hands back out of its closure.
#[derive(Debug)]
struct Pass<R> {
    header: ResponseState,
    toggled: bool,
    openness: f32,
    inner: Option<R>,
}

/// The open flag, and the body height the reveal clips against.
///
/// The height lives here rather than being re-read from the body's own
/// response because a skipped body has none: the row hangs off the
/// *header*, which is recorded on every frame, so a section reopened
/// later still animates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ExpanderState {
    open: bool,
    height: Option<f32>,
}

/// Whether an activation key fired on the focused header.
///
/// Both are sampled, never short-circuited: `key_pressed` also keeps the
/// chord subscribed for the wake gate, so one firing must not drop the
/// other's subscription that frame.
fn activation_key(ui: &mut Ui, header: WidgetId) -> bool {
    if !ui.focus_within(header) {
        return false;
    }
    let space = ui.key_pressed(Shortcut::key(Key::Char(' ')));
    let enter = ui.key_pressed(Shortcut::key(Key::Enter));
    space || enter
}

#[cfg(test)]
mod tests;
