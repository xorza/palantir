//! The hover tooltip: the widget, its per-trigger hover clock, and the app-global
//! state letting a second tooltip skip the delay.

use crate::input::keyboard::key::Key;
use crate::input::pointer::PointerEvent;
use crate::input::sense::Sense;
use crate::input::watch::PointerWake;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::text::text_input::TextInput;
use crate::scene::layer::Layer;
use crate::text::wrap::TextWrap;
use crate::theme::tooltip::TooltipTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::overlay_scope::{Backdrop, OverlayScope, OverlaySpec};
use crate::widget_core::response::ResponseSnapshot;
use crate::widget_core::widget::Widget;
use crate::widgets::text::Text;
use std::rc::Rc;
use std::time::Duration;

/// Per-trigger state; times are Ui-time, immune to the `MAX_DT` clamp on idle wakes.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
enum TooltipState {
    /// The pointer is off the trigger.
    #[default]
    Idle,
    /// The pointer rests on the trigger, since then, and the delay is not over.
    Hovering {
        since: Duration,
    },
    Visible,
    /// A press or Escape hid the bubble, which stays hidden until the pointer leaves, as Qt's,
    /// Win32's and a browser's `title` tooltips hide on a press, and WCAG 1.4.13 asks a tooltip
    /// to dismiss without a pointer move.
    Dismissed,
}

/// Singleton holding when any tooltip was last visible; a cold tooltip within
/// `theme.warmup` of it skips its delay.
#[derive(Default, Clone, Copy, Debug)]
struct TooltipGlobal {
    last_visible_at: Option<Duration>,
}

/// Hover-driven text bubble attached to a trigger widget, recorded into
/// [`Layer::Tooltip`] after the pointer rests
/// for the theme's delay; the theme's warmup keeps later tooltips instant.
///
/// ```
/// # use palantir::{Button, Tooltip, Ui};
/// # fn demo(ui: &mut Ui) {
/// let r = Button::new().label("Save").show(ui).snapshot();
/// Tooltip::on(&r, "Persist changes (Ctrl+S)").show(ui);
/// # }
/// ```
///
/// Pointer-driven only, and skipped on disabled triggers unless
/// `.when_disabled(true)`. Implements [`Configure`]; the bubble's id defaults to
/// [`Self::bubble_id`] of the trigger's.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Tooltip<'a> {
    snapshot: ResponseSnapshot,
    label: TextInput<'a>,
    delay: Option<Duration>,
    when_disabled: bool,
    widget: Widget,
    chrome: Option<Chrome>,
    style: Option<&'a TooltipTheme>,
}

impl<'a> Tooltip<'a> {
    /// Attach a tooltip showing `text` to a trigger's response snapshot
    /// (`trigger.snapshot()` releases the `&Ui` borrow). An empty `text` records no
    /// bubble.
    #[track_caller]
    pub fn on(snapshot: &ResponseSnapshot, text: impl Into<TextInput<'a>>) -> Self {
        // The bubble must never claim hover, or it would shadow its trigger.
        let widget = Widget::vstack().sense(Sense::NONE);
        Self {
            snapshot: *snapshot,
            label: text.into(),
            delay: None,
            when_disabled: false,
            widget,
            chrome: None,
            style: None,
        }
    }

    /// Per-instance override of [`Theme::tooltip`](crate::Theme::tooltip).
    pub fn style(mut self, s: impl Into<Option<&'a TooltipTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Override the delay; falls back to the theme's.
    pub const fn delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// Allow the tooltip on disabled triggers. Off by default.
    pub const fn when_disabled(mut self, when_disabled: bool) -> Self {
        self.when_disabled = when_disabled;
        self
    }

    /// Tick the hover timer and, when visible, record the bubble next to the
    /// trigger.
    pub fn show(self, ui: &mut Ui) -> TooltipResponse {
        // A handle, not a borrow: recording below reborrows `ui` mutably.
        let ui_theme = Rc::clone(ui.theme());
        let theme = self.style.unwrap_or(&ui_theme.tooltip);
        let delay = self.delay.unwrap_or(theme.delay);
        let warmup = theme.warmup;
        let gap = theme.gap;

        let trigger_id = self.snapshot.id;
        let bubble_id = Tooltip::bubble_id(trigger_id);

        // The observation, not the reaction: a disabled trigger is never hovered,
        // which `when_disabled` is for.
        let pointer_over = self.snapshot.state.pointer_over;
        let trigger_disabled = self.snapshot.state.disabled;
        let trigger_rect = self.snapshot.state.rect;
        let active_trigger =
            pointer_over && !self.label.is_empty() && (!trigger_disabled || self.when_disabled);

        let now = ui.now();

        // Idle on almost every frame, so nothing here touches the state map
        // unconditionally: reads probe without creating a row, writes are gated on
        // change.
        let prev: TooltipState = ui
            .state::<TooltipState>(trigger_id)
            .copied()
            .unwrap_or_default();
        // Read on every frame the pointer rests on the trigger, so the button watch it places
        // delivers the next frame's press.
        let dismissed = active_trigger && dismissed(ui, trigger_id);
        let state = if !active_trigger {
            TooltipState::Idle
        } else if dismissed && matches!(prev, TooltipState::Hovering { .. } | TooltipState::Visible)
        {
            TooltipState::Dismissed
        } else {
            let warmup_active = ui
                .singleton::<TooltipGlobal>()
                .and_then(|global| global.last_visible_at)
                .is_some_and(|t| now.saturating_sub(t) < warmup);
            let since = match prev {
                TooltipState::Dismissed | TooltipState::Visible => None,
                TooltipState::Hovering { since } => Some(since),
                TooltipState::Idle => Some(now),
            };
            match since {
                None => prev,
                Some(since) if warmup_active || now.saturating_sub(since) >= delay => {
                    TooltipState::Visible
                }
                Some(since) => {
                    if prev == TooltipState::Idle {
                        // One wake at the threshold is enough; a stale one is a cheap no-op
                        // frame.
                        ui.request_repaint_after(delay);
                    }
                    TooltipState::Hovering { since }
                }
            }
        };
        let visible = state == TooltipState::Visible;

        if visible && let Some(trigger_rect) = trigger_rect {
            ui.with_singleton::<TooltipGlobal, _>(|_, global| global.last_visible_at = Some(now));
            let anchor = Anchor::below(trigger_rect).with_gap(gap);
            let label = self.label;
            let chrome = self.chrome.as_ref().unwrap_or(&theme.panel);
            let text = theme.text.apply(&ui_theme.text);
            let mut bubble = self
                .widget
                .default_id(bubble_id)
                .default_padding(theme.padding)
                .default_max_size(theme.max_size);
            // `Backdrop::None`: a tooltip annotates, and a claim recorded every
            // frame would cut off every plane below.
            let spec = OverlaySpec {
                layer: Layer::Tooltip,
                anchor: Some(anchor),
                backdrop: Backdrop::None,
            };
            let scope = OverlayScope::claim(ui, spec, &mut bubble);
            let _ = scope.record(ui, |ui| {
                bubble.record(ui, Some(chrome), |ui| {
                    Text::new(label)
                        .style(&text)
                        .text_wrap(TextWrap::Wrap)
                        .show(ui);
                });
            });
        }

        if state != prev {
            ui.with_state::<TooltipState, _>(trigger_id, |_, s| *s = state);
        }
        TooltipResponse { visible }
    }
}

/// Whether a press while the pointer rests on the trigger, or an Escape granted to its scope,
/// dismisses the bubble this frame. Read only while the pointer rests on the trigger, so the
/// button watch wakes no frame elsewhere.
fn dismissed(ui: &mut Ui, trigger_id: WidgetId) -> bool {
    let pressed = ui
        .pointer_events(PointerWake::BUTTONS)
        .iter()
        .any(|event| matches!(event, PointerEvent::Pressed { .. }));
    pressed || ui.key_pressed_as(trigger_id, Key::Escape)
}

impl Tooltip<'_> {
    /// Paint `chrome` behind this widget; unset falls back to the
    /// theme's. [`Chrome::NONE`] suppresses the chrome.
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` holds the kinds [`Chrome`]
    /// lists.
    #[track_caller]
    pub const fn chrome(mut self, chrome: Chrome) -> Self {
        chrome.validate();
        self.chrome = Some(chrome);
        self
    }

    /// Paint `chrome` unless the caller set one, for a wrapper theming a widget
    /// after the caller's setters.
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` holds the kinds [`Chrome`]
    /// lists.
    #[track_caller]
    pub const fn default_chrome(mut self, chrome: Chrome) -> Self {
        chrome.validate();
        if self.chrome.is_none() {
            self.chrome = Some(chrome);
        }
        self
    }
}

/// The ids of a tooltip's parts.
impl Tooltip<'static> {
    /// The bubble of the tooltip on the trigger `trigger`, unless its `Configure` names another.
    pub fn bubble_id(trigger: WidgetId) -> WidgetId {
        trigger.with("bubble")
    }
}

impl Configure for Tooltip<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// What one pass over a [`Tooltip`] produced. No [`Response`](crate::Response): the
/// bubble senses nothing and records nothing while down.
#[derive(Debug, Clone, Copy)]
pub struct TooltipResponse {
    /// The bubble recorded this frame.
    pub visible: bool,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::widgets::tooltip::TooltipState;
    use std::time::Duration;

    impl TooltipState {
        pub(crate) const fn is_visible(&self) -> bool {
            matches!(self, Self::Visible)
        }

        /// When the pointer came to rest, while the delay runs.
        pub(crate) const fn hover_started_at(&self) -> Option<Duration> {
            match self {
                Self::Hovering { since } => Some(*since),
                Self::Idle | Self::Visible | Self::Dismissed => None,
            }
        }
    }
}

#[cfg(test)]
mod tests;
