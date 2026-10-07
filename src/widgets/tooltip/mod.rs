//! The hover tooltip: the widget, its per-trigger hover clock, and the app-global
//! state letting a second tooltip skip the delay.

use crate::input::sense::Sense;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::paint::background::Background;
use crate::primitives::text::text_input::TextInput;
use crate::scene::layer::Layer;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::overlay_scope::{Backdrop, OverlayScope};
use crate::widget_core::response::ResponseSnapshot;
use crate::widget_core::widget::Widget;
use crate::widgets::text::Text;
use crate::widgets::theme::tooltip::TooltipTheme;
use std::rc::Rc;
use std::time::Duration;

/// Per-trigger state; `hover_started_at` is Ui-time, immune to the `MAX_DT` clamp
/// on idle wakes.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
struct TooltipState {
    hover_started_at: Option<Duration>,
    visible: bool,
}

/// Singleton holding when any tooltip was last visible; a cold tooltip within
/// `theme.warmup` of it skips its delay.
#[derive(Default, Clone, Copy, Debug)]
struct TooltipGlobal {
    last_visible_at: Option<Duration>,
}

/// Hover-driven text bubble attached to a trigger widget, recorded into
/// [`Layer::Tooltip`](crate::scene::layer::Layer::Tooltip) after the pointer rests
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
/// `.when_disabled(true)`. Implements [`Configure`]; identity defaults to the
/// trigger's id.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Tooltip<'a> {
    snapshot: &'a ResponseSnapshot,
    label: TextInput<'a>,
    delay: Option<Duration>,
    when_disabled: bool,
    widget: Widget,
    chrome: Option<Background>,
    style: Option<&'a TooltipTheme>,
}

impl<'a> Tooltip<'a> {
    /// Attach a tooltip showing `text` to a trigger's response snapshot
    /// (`trigger.snapshot()` releases the `&Ui` borrow). An empty `text` records no
    /// bubble.
    #[track_caller]
    pub fn on(snapshot: &'a ResponseSnapshot, text: impl Into<TextInput<'a>>) -> Self {
        // The bubble must never claim hover, or it would shadow its trigger.
        let widget = Widget::vstack().sense(Sense::NONE);
        Self {
            snapshot,
            label: text.into(),
            delay: None,
            when_disabled: false,
            widget,
            chrome: None,
            style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `tooltip`.
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
    pub const fn when_disabled(mut self, yes: bool) -> Self {
        self.when_disabled = yes;
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
        let bubble_id = trigger_id.with("bubble");

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
        let mut state = prev;

        if active_trigger {
            let warmup_active = ui
                .singleton::<TooltipGlobal>()
                .and_then(|global| global.last_visible_at)
                .is_some_and(|t| now.saturating_sub(t) < warmup);
            let started = if let Some(t) = state.hover_started_at {
                t
            } else {
                state.hover_started_at = Some(now);
                // One wake at the threshold is enough; a stale one is a cheap no-op
                // frame.
                ui.request_repaint_after(delay);
                now
            };
            let elapsed = now.saturating_sub(started);
            if warmup_active || elapsed >= delay {
                state.visible = true;
            }
        } else {
            state.hover_started_at = None;
            state.visible = false;
        }

        if state.visible
            && let Some(trigger_rect) = trigger_rect
        {
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
            // `Backdrop::None`: a tooltip annotates, and a scope recorded every
            // frame would cut off every layer below.
            let scope = OverlayScope::claim(
                ui,
                Layer::Tooltip,
                Some(anchor),
                Backdrop::None,
                &mut bubble,
            );
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
        TooltipResponse {
            visible: state.visible,
        }
    }
}

impl Tooltip<'_> {
    /// Paint `background` as this widget's background; unset falls back to the
    /// theme's. [`Background::NONE`] suppresses the chrome.
    ///
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background)
    /// lists.
    #[track_caller]
    pub const fn background(mut self, background: Background) -> Self {
        background.validate();
        self.chrome = Some(background);
        self
    }

    /// Paint `background` unless the caller set one, for a wrapper theming a widget
    /// after the caller's setters.
    ///
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background)
    /// lists.
    #[track_caller]
    pub const fn default_background(mut self, background: Background) -> Self {
        background.validate();
        if self.chrome.is_none() {
            self.chrome = Some(background);
        }
        self
    }
}

impl Configure for Tooltip<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
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
mod tests;
