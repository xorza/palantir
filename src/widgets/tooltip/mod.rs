//! The hover tooltip: the widget, the per-trigger hover clock it needs to
//! honour a delay, and the app-global state that lets a second tooltip
//! appear without re-serving the delay.

use crate::input::sense::Sense;
use crate::primitives::identity::widget_id::WidgetId;
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

/// Per-trigger tooltip state. `hover_started_at` is Ui-time at first
/// hovered frame; elapsed = `now - hover_started_at`, immune to
/// the frame runtime's `MAX_DT` clamp on idle wakes.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
struct TooltipState {
    hover_started_at: Option<Duration>,
    visible: bool,
}

/// Singleton tracking the most recent moment any tooltip was visible.
/// Cold-start tooltips within `theme.warmup` of `last_visible_at`
/// skip their delay (egui-style toolbar warmup).
#[derive(Default, Clone, Copy, Debug)]
struct TooltipGlobal {
    last_visible_at: Option<Duration>,
}

/// Row key for the process-wide warmup state shared by every tooltip.
/// Hashed on call rather than held in a `LazyLock`: hashing a short
/// literal costs less than the lazy cell's init check, and only a
/// hovered trigger asks for it at all.
fn global_state_id() -> WidgetId {
    WidgetId::from_hash("palantir.tooltip.global")
}

/// Hover-driven text bubble attached to a trigger widget. Records into
/// [`crate::scene::layer::Layer::Tooltip`] after the pointer has rested
/// on the trigger for [`crate::widgets::theme::tooltip::TooltipTheme::delay`]
/// seconds. A short warmup window (configured on the theme) keeps
/// subsequent tooltips instant after one was dismissed, so scanning a
/// row of buttons doesn't re-delay on every move.
///
/// Two-line attachment:
///
/// ```
/// # use palantir::{Button, Tooltip, Ui};
/// # fn demo(ui: &mut Ui) {
/// let r = Button::new().label("Save").show(ui).snapshot();
/// Tooltip::on(&r, "Persist changes (Ctrl+S)").show(ui);
/// # }
/// ```
///
/// Tooltips are pointer-driven only and skip recording on disabled
/// triggers by default. Pass `.when_disabled(true)` to opt in for
/// "why is this disabled?" hints.
///
/// Implements [`Configure`], so the bubble takes `.padding(...)`,
/// `.max_size(...)`, `.size(...)`, `.margin(...)` and the rest like any
/// other widget. Identity defaults to the trigger's id — a tooltip has
/// no call site of its own worth keying on — but an explicit `.id(...)`
/// / `.id_salt(...)` wins.
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
    /// Attach a tooltip showing `text` to the given trigger response
    /// snapshot. The snapshot carries the trigger's `WidgetId` and
    /// last-frame rect — both drive timer keying and anchor computation.
    /// Pass via `trigger.snapshot()` to detach from the trigger's `&Ui`
    /// borrow before recording the tooltip body.
    ///
    /// `text` is the bubble's whole content, so it is required. An empty
    /// one records no bubble at all.
    #[track_caller]
    pub fn on(snapshot: &'a ResponseSnapshot, text: impl Into<TextInput<'a>>) -> Self {
        // Bubble must never claim hover — would shadow its own trigger.
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

    /// Per-instance override of [`crate::Theme`]'s `tooltip`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    ///
    /// Per-field [`Self::background`] / [`Self::delay`] still win over it.
    pub fn style(mut self, s: impl Into<Option<&'a TooltipTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Override the per-tooltip delay. Falls back to
    /// [`crate::widgets::theme::tooltip::TooltipTheme::delay`] when unset.
    pub const fn delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// Allow the tooltip to fire on disabled triggers. Off by default —
    /// most disabled tooltips would be UX noise.
    pub const fn when_disabled(mut self, yes: bool) -> Self {
        self.when_disabled = yes;
        self
    }

    /// Tick the hover timer, update visibility, and (when visible)
    /// record the bubble into `Layer::Tooltip` anchored next to the
    /// trigger.
    pub fn show(self, ui: &mut Ui) -> TooltipResponse {
        // Handle, not a borrow: the bundle may point into the `Ui`'s own
        // theme, and the record below reborrows `ui` mutably.
        let ui_theme = Rc::clone(ui.theme());
        let theme = self.style.unwrap_or(&ui_theme.tooltip);
        let delay = self.delay.unwrap_or(theme.delay);
        let warmup = theme.warmup;
        let gap = theme.gap;

        let trigger_id = self.snapshot.id;
        let bubble_id = trigger_id.with("bubble");

        // The observation, not the reaction: the two agree for an enabled
        // trigger, and a disabled one is never hovered — which is the
        // trigger `when_disabled` exists for.
        let pointer_over = self.snapshot.state.pointer_over;
        let trigger_disabled = self.snapshot.state.disabled;
        let trigger_rect = self.snapshot.state.rect;
        // An empty label is inactive rather than an empty bubble, and
        // inactive early enough that the hover timer never arms and no
        // wake is queued for a tooltip that could never appear.
        let active_trigger =
            pointer_over && !self.label.is_empty() && (!trigger_disabled || self.when_disabled);

        let now = ui.now();

        // A tooltip attaches to a trigger that is idle on almost every
        // frame it is recorded, so nothing here may touch the state map
        // unconditionally: the read probes without materialising a row, the
        // warmup singleton is only asked for by a hovered trigger, and the
        // write-back below is gated on an actual change.
        let prev: TooltipState = ui
            .state::<TooltipState>(trigger_id)
            .copied()
            .unwrap_or_default();
        let mut state = prev;

        if active_trigger {
            let warmup_active = ui
                .state::<TooltipGlobal>(global_state_id())
                .and_then(|global| global.last_visible_at)
                .is_some_and(|t| now.saturating_sub(t) < warmup);
            let started = if let Some(t) = state.hover_started_at {
                t
            } else {
                state.hover_started_at = Some(now);
                // One wake at the threshold is enough — the queue
                // remembers it. If the user moves off before then
                // the wake still fires into a no-op frame; cheap.
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
            ui.state_or_default::<TooltipGlobal>(global_state_id())
                .last_visible_at = Some(now);
            let anchor = Anchor::below(trigger_rect).with_gap(gap);
            let label = self.label;
            let chrome = self.chrome.as_ref().unwrap_or(&theme.panel);
            let text = theme.text.apply(&ui_theme.text);
            // Theme fills in whatever the caller left alone. Identity
            // derives from the trigger, because that is the only thing a
            // tooltip *has* — but a caller-set id wins like any other
            // explicit value.
            let mut bubble = self
                .widget
                .default_id(bubble_id)
                .default_padding(theme.padding)
                .default_max_size(theme.max_size);
            // `Backdrop::None`: a tooltip annotates rather than
            // interrupts, and it is recorded every frame it is up — a
            // scope would cut off every layer below it for as long.
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
            *ui.state_or_default::<TooltipState>(trigger_id) = state;
        }
        TooltipResponse {
            visible: state.visible,
        }
    }
}

impl Tooltip<'_> {
    /// Paint `bg` as this widget's background.
    ///
    /// `None` is the default; theme fallback in [`Self::show`] fills it in
    /// from `ui.theme().tooltip.panel` when unset. Pass
    /// [`Background::NONE`] to suppress the themed bubble chrome.
    pub const fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    /// Paint `bg` as this widget's background unless the caller set one —
    /// the chrome peer of
    /// [`ThemeDefaults::default_padding`](crate::widget::ThemeDefaults::default_padding),
    /// for a wrapper that themes a widget it holds after the caller's own
    /// setters ran. An explicit [`Self::background`] wins in either order.
    pub const fn default_background(mut self, bg: Background) -> Self {
        if self.chrome.is_none() {
            self.chrome = Some(bg);
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

/// What one pass over a [`Tooltip`] produced.
///
/// No [`Response`](crate::Response) here, unlike the other widget results:
/// the bubble senses nothing and does not record at all on the frames it is
/// down, so there is no node an application would ask about. Whether it is
/// up is the whole answer — for a trigger that wants to paint differently
/// while its hint is showing.
#[derive(Debug, Clone, Copy)]
pub struct TooltipResponse {
    /// The bubble recorded this frame.
    pub visible: bool,
}

#[cfg(test)]
mod tests;
