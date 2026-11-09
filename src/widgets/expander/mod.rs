//! The disclosure control: a header that reveals or hides a body.

use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_slot::AnimationSlot;
use crate::animation::animation_spec::AnimationSpec;
use crate::primitives::geometry::arrow;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::input::interaction::response_state::ResponseState;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::text::text_input::TextInput;
use crate::shape::Shape;
use crate::theme::expander::ExpanderTheme;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::ui::Ui;
use crate::widget_core::activation::Activation;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use crate::widgets::text::Text;
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
/// of a full record every frame. The open flag lives in the state map on the widget's id and
/// is `false` until [`Self::start_open`] says otherwise, so an untouched closed section keeps no
/// state row; [`Self::open`], [`Self::close`] and [`Self::is_open`] drive it from outside.
///
/// Its parts are the header, [`Self::header_id`], with `arrow` and `label` under it, and the
/// body, `id.with("body")`, inside `id.with("body").with("reveal")`.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Expander<'a> {
    widget: Widget,
    label: TextInput<'a>,
    start_open: bool,
    keep_body: bool,
    style: Option<&'a ExpanderTheme>,
}

const SLOT_OPEN: AnimationSlot = AnimationSlot::new("open");

impl<'a> Expander<'a> {
    /// A header labeled `label`, closed on its first frame unless [`Self::start_open`] says
    /// otherwise.
    #[track_caller]
    pub fn new(label: impl Into<TextInput<'a>>) -> Self {
        Self {
            widget: Widget::vstack(),
            label: label.into(),
            start_open: false,
            keep_body: false,
            style: None,
        }
    }

    /// Whether the section starts open; read until the section's first toggle, by the user
    /// or by [`Self::open`] and [`Self::close`].
    pub const fn start_open(mut self, start_open: bool) -> Self {
        self.start_open = start_open;
        self
    }

    /// Record the body even while closed, under
    /// [`Visibility::Collapsed`](crate::Visibility::Collapsed). Palantir sweeps the
    /// cross-frame row of any widget no longer recorded, so this keeps the body's
    /// state; default `false`, since costing nothing while closed is the point.
    pub const fn keep_body(mut self, keep_body: bool) -> Self {
        self.keep_body = keep_body;
        self
    }

    /// Per-instance override of [`Theme::expander`](crate::Theme::expander).
    pub fn style(mut self, s: impl Into<Option<&'a ExpanderTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// The header of the expander `expander`: the focusable click target that holds the open
    /// state and reports [`ExpanderResponse::response`].
    pub fn header_id(expander: WidgetId) -> WidgetId {
        expander.with("header")
    }

    /// Record the header, and the body under it while open.
    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> ExpanderResponse<R> {
        let theme = Rc::clone(ui.theme());
        let t = self.style.unwrap_or(&theme.expander);
        let ambient = theme.text;
        let Self {
            mut widget,
            label,
            start_open,
            keep_body,
            style: _,
        } = self;

        widget.configure().default_size((Sizing::FILL, Sizing::HUG));
        let id = widget.resolve(ui);
        let header_id = Self::header_id(id);
        let body_id = id.with("body");
        let stored = ui.state::<ExpanderState>(header_id).copied();
        let was_open = stored.map_or(start_open, |s| s.open);
        let height = stored.and_then(|s| s.height);

        let mut pass = Pass {
            header: Response {
                id: header_id,
                state: ResponseState::default(),
            },
            toggled: false,
            openness: 0.0,
            showing: false,
            inner: None,
        };
        widget.record(ui, None, |ui| {
            let mut header = Widget::hstack()
                .id(header_id)
                .size((Sizing::FILL, Sizing::HUG))
                .gap(t.gap)
                .child_align(Align::v(VAlign::Center))
                .sense(Sense::CLICK)
                .focusable(true);
            let state = header.activation_response(ui, Activation::Button);
            let plan = t.plan(&state, (), ambient);
            let look = header.wear(ui, plan);

            let activated = state.clicked();
            let now_open = was_open != activated;
            // With no measured height there is nothing to clip against: snap, then
            // animate every later reveal.
            let spec = if now_open && height.is_none() {
                AnimationSpec::SNAP
            } else {
                t.defaults.transition
            };
            let swing = ui.animate(
                AnimationRequest::new(header_id, SLOT_OPEN, f32::from(now_open)),
                spec,
            );
            // A spring swings past its ends, so the clamp keeps the arrow and the clip in range,
            // and a close shows the body until it rests at 0, not while its swing dips under 0.
            let openness = swing.value.clamp(0.0, 1.0);
            let showing = now_open || !swing.settled;
            let [a, b, c] = arrow::rotated(t.arrow_size, t.arrow_angle(openness));
            let text = look.text;
            let label = ui.intern(label);
            header.record(ui, Some(&look.chrome), |ui| {
                let arrow_box = Widget::leaf()
                    .id(header_id.with("arrow"))
                    .size(t.arrow_size);
                arrow_box.record(ui, None, |ui| {
                    ui.add_shape(Shape::triangle(a, b, c).radius(t.arrow_radius), text.color);
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
            pass.showing = showing;
        });

        let now_open = was_open != pass.toggled;
        // The body lays out whole whenever it shows, so last frame's rect is its
        // height; a collapsed or unrecorded one has none.
        let measured = stored
            .is_some_and(|s| s.shown)
            .then(|| ui.response_for(body_id).layout_rect.map(|r| r.size.h))
            .flatten();
        let row = ExpanderState {
            open: now_open,
            height: measured.or(height),
            shown: pass.showing,
        };
        // Written on a change, so an unopened section mints no row (the probe-don't-insert path
        // `ComboBox` takes). An open one always keeps a row, which `is_open` reads, as a section
        // `start_open` opens has none of its own yet.
        let cur = stored.unwrap_or(ExpanderState {
            open: false,
            height: None,
            shown: false,
        });
        if cur != row {
            ui.with_state::<ExpanderState, _>(header_id, |_, s| *s = row);
        }

        ExpanderResponse {
            response: pass.header,
            inner: pass.inner,
            changed: pass.toggled,
            openness: pass.openness,
        }
    }
}

/// Driving an expander from outside, by its id.
impl Expander<'static> {
    /// Open the expander `id`; its next `show` reveals the body.
    pub fn open(ui: &mut Ui, id: WidgetId) {
        ui.with_state::<ExpanderState, _>(Self::header_id(id), |_, s| s.open = true);
    }

    /// Close the expander `id`; its next `show` hides the body.
    pub fn close(ui: &mut Ui, id: WidgetId) {
        ui.with_state::<ExpanderState, _>(Self::header_id(id), |_, s| s.open = false);
    }

    /// Whether the expander `id` is open, as its last `show` or a call above left it. Allocates
    /// no row.
    pub fn is_open(ui: &Ui, id: WidgetId) -> bool {
        ui.state::<ExpanderState>(Self::header_id(id))
            .is_some_and(|s| s.open)
    }
}

impl Configure for Expander<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[derive(Debug)]
struct Pass<R> {
    header: Response,
    toggled: bool,
    openness: f32,
    showing: bool,
    inner: Option<R>,
}

/// The open flag and the body height the reveal clips against. The height lives on
/// the *header*, recorded every frame, because a skipped body has no response;
/// `shown` is whether the body laid out expanded, so its last rect is its height.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ExpanderState {
    open: bool,
    height: Option<f32>,
    shown: bool,
}

/// What one pass over an [`Expander`] produced.
#[derive(Clone, Copy, Debug)]
pub struct ExpanderResponse<R> {
    /// The header's response; the whole row is the hit target.
    pub response: Response,
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
