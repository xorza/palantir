//! The disclosure control: a header that reveals or hides a body.

use crate::animation::animation_slot::AnimationSlot;
use crate::input::interaction::response_state::ResponseState;
use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::text::text_input::TextInput;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widget_core::widget_look::theme_slot::ThemeSlot;
use crate::widgets::arrow::Arrow;
use crate::widgets::text::Text;
use crate::widgets::theme::expander::ExpanderTheme;
use std::rc::Rc;

/// A header that reveals or hides a body, like HTML `<details>`.
///
/// ```
/// # use palantir::{Expander, Text, Ui};
/// # fn demo(ui: &mut Ui) {
/// Expander::new("Advanced")
///     .start_open(false)
///     .show(ui, |ui| {
///         Text::new("hidden until asked for").show(ui);
///     });
/// # }
/// ```
///
/// **The body does not record while closed**, so cross-frame rows inside it (a
/// [`TextEdit`](crate::TextEdit)'s unsent edit, a [`Scroll`](crate::Scroll)'s
/// offset) are swept; a section holding any wants [`Self::keep_body`], at the price
/// of a full record every frame. The open flag lives on the widget's id and is
/// `false` until [`Self::start_open`] says otherwise, so an untouched section keeps
/// no state row; an application owning the flag binds it with [`Self::open`].
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Expander<'a> {
    widget: Widget,
    label: TextInput<'a>,
    start_open: bool,
    open: Option<&'a mut bool>,
    keep_body: bool,
    style: Option<&'a ExpanderTheme>,
}

const SLOT_OPEN: AnimationSlot = AnimationSlot::new("open");

impl<'a> Expander<'a> {
    /// A header labelled `label`, closed on its first frame; the widget owns the
    /// open state until [`Self::open`] takes it over.
    #[track_caller]
    pub fn new(label: impl Into<TextInput<'a>>) -> Self {
        Self {
            widget: Widget::vstack().size((Sizing::FILL, Sizing::HUG)),
            label: label.into(),
            start_open: false,
            open: None,
            keep_body: false,
            style: None,
        }
    }

    /// Whether the section starts open; read on the first frame only, ignored when
    /// [`Self::open`] binds the flag.
    pub const fn start_open(mut self, open: bool) -> Self {
        self.start_open = open;
        self
    }

    /// Bind the open flag to the caller's own `bool`; wins over
    /// [`Self::start_open`], and every toggle is written back.
    pub const fn open(mut self, open: &'a mut bool) -> Self {
        self.open = Some(open);
        self
    }

    /// Record the body even while closed, under
    /// [`Visibility::Collapsed`](crate::Visibility::Collapsed). Palantir sweeps the
    /// cross-frame row of any widget no longer recorded, so this keeps the body's
    /// state; default `false`, since costing nothing while closed is the point.
    pub const fn keep_body(mut self, keep: bool) -> Self {
        self.keep_body = keep;
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `expander`.
    pub fn style(mut self, s: impl Into<Option<&'a ExpanderTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the header, and the body under it while open.
    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> ExpanderResponse<'_, R> {
        let theme = Rc::clone(ui.theme());
        let t = self.style.unwrap_or(&theme.expander);
        let ambient = theme.text;
        let Self {
            mut widget,
            label,
            start_open,
            open,
            keep_body,
            style: _,
        } = self;

        let id = widget.resolve(ui);
        let header_id = id.with("header");
        let body_id = id.with("body");
        let stored = ui.state::<ExpanderState>(header_id).copied();
        let was_open = match &open {
            Some(flag) => **flag,
            None => stored.map_or(start_open, |s| s.open),
        };
        let height = stored.and_then(|s| s.height);

        let mut pass = Pass {
            header: ResponseState::default(),
            toggled: false,
            openness: 0.0,
            inner: None,
        };
        widget.record(ui, None, |ui| {
            let mut header = Widget::hstack()
                .id(header_id)
                .size((Sizing::FILL, Sizing::HUG))
                .gap(t.gap)
                .child_align(Align::v(VAlign::Center))
                .sense(Sense::CLICK)
                .focusable(true)
                // Enter and Space classify as `KeyClass::Text`, so taking them
                // claims that class; right for a focused header, which is no typing
                // target.
                .input_scope(KeyFilter::TEXT);
            let state = header.response(ui);
            let look = t.plan(&state, (), ambient).apply(ui, &mut header);

            // A disabled widget's button slices are already empty, but key events
            // skip that fold.
            let activated = state.clicked() || (!state.disabled && activation_key(ui, &mut header));
            let now_open = was_open != activated;
            // With no measured height there is nothing to clip against: snap, then
            // animate every later reveal.
            let spec = if now_open && height.is_none() {
                None
            } else {
                t.defaults.animation
            };
            let openness = ui.animate(header_id, SLOT_OPEN, f32::from(now_open), spec);
            let showing = openness > 0.0;
            let [a, b, c] =
                Arrow { size: t.arrow_size }.rounded(t.arrow_radius, t.arrow_angle(openness));
            let text = look.text;
            let label = ui.intern(label);
            header.record(ui, Some(&look.background), |ui| {
                let arrow_box = Widget::leaf()
                    .id(header_id.with("arrow"))
                    .size((Sizing::fixed(t.arrow_size.x), Sizing::fixed(t.arrow_size.y)));
                arrow_box.record(ui, None, |ui| {
                    ui.add_shape(
                        Shape::triangle(a, b, c)
                            .fill(text.color)
                            .radius(t.arrow_radius),
                    );
                });
                Text::new(label)
                    .id(header_id.with("label"))
                    .style(&text)
                    .show(ui);
            });

            if showing || keep_body {
                // The body records whole inside a wrapper whose clip reveals it;
                // laying it out at a fraction of its height would reflow its text
                // each tween frame and clip its own rect, so no frame could measure
                // the height the next clips against.
                let mut reveal = Widget::vstack()
                    .id(body_id.with("reveal"))
                    .size((Sizing::FILL, Sizing::HUG))
                    .margin(Spacing::new(t.indent, 0.0, 0.0, 0.0));
                if !showing {
                    reveal = reveal.collapsed();
                } else if let Some(full) = height.filter(|_| openness < 1.0) {
                    reveal = reveal
                        .max_size(Size::new(f32::INFINITY, openness * full))
                        .clip_rect();
                }
                let body_panel = Widget::vstack()
                    .id(body_id)
                    .size((Sizing::FILL, Sizing::HUG))
                    .padding(t.body_padding);
                pass.inner = Some(reveal.record(ui, None, |ui| body_panel.record(ui, None, body)));
            }
            pass.header = state;
            pass.toggled = activated;
            pass.openness = openness;
        });

        let now_open = was_open != pass.toggled;
        if let Some(flag) = open {
            *flag = now_open;
        }
        // The body lays out whole whenever it shows, so last frame's rect is its
        // height; a collapsed or unrecorded one has none.
        let measured = stored
            .is_some_and(|s| s.shown)
            .then(|| ui.response_for(body_id).layout_rect.map(|r| r.size.h))
            .flatten();
        let row = ExpanderState {
            open: now_open,
            height: measured.or(height),
            shown: pass.openness > 0.0,
        };
        // Written only on a change, so an unopened section mints no row (the
        // probe-don't-insert path `ComboBox` takes); an absent row *is* the
        // resolved state, hence the comparison against it.
        let current = stored.unwrap_or(ExpanderState {
            open: was_open,
            height: None,
            shown: false,
        });
        if current != row {
            ui.with_state::<ExpanderState, _>(header_id, |_, s| *s = row);
        }

        ExpanderResponse {
            response: Response::new(header_id, ui, pass.header),
            inner: pass.inner,
            changed: pass.toggled,
            openness: pass.openness,
        }
    }
}

impl Configure for Expander<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[derive(Debug)]
struct Pass<R> {
    header: ResponseState,
    toggled: bool,
    openness: f32,
    inner: Option<R>,
}

/// The open flag and the body height the reveal clips against. The height lives on
/// the *header*, recorded every frame, because a skipped body has no response;
/// `shown` is whether the body laid out showing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ExpanderState {
    open: bool,
    height: Option<f32>,
    shown: bool,
}

/// Whether an activation key fired on the focused header; both are sampled, not
/// short-circuited, as `key_pressed` keeps the chord subscribed for the wake gate.
fn activation_key(ui: &mut Ui, header: &mut Widget) -> bool {
    let id = header.resolve(ui);
    if !ui.is_focus_within(id) {
        return false;
    }
    let space = header.key_pressed(ui, Shortcut::key(Key::Char(' ')));
    let enter = header.key_pressed(ui, Shortcut::key(Key::Enter));
    space || enter
}

/// What one pass over an [`Expander`] produced.
#[derive(Debug)]
pub struct ExpanderResponse<'a, R> {
    /// The header's response; the whole row is the hit target.
    pub response: Response<'a>,
    /// What the body closure returned, or `None` on a frame the body did not
    /// record; a collapsed [`keep_body`](crate::Expander::keep_body) section still
    /// records.
    pub inner: Option<R>,
    /// The header was activated this frame, by click or key, flipping the section.
    pub changed: bool,
    /// `0.0` closed, `1.0` open, in between while the reveal animates.
    pub openness: f32,
}

#[cfg(test)]
mod tests;
