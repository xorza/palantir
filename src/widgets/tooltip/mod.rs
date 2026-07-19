use crate::forest::element::Salt;
use crate::forest::element::{Configure, Element};
use crate::forest::layer::Layer;
use crate::input::sense::Sense;
use crate::layout::types::overlay::OverlayPosition;
use crate::primitives::background::Background;
use crate::primitives::interned_str::TextInput;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::shape::TextWrap;
use crate::ui::Ui;
use crate::widgets::ResponseSnapshot;
use crate::widgets::text::Text;
use std::sync::LazyLock;
use std::time::Duration;

/// Per-trigger tooltip state. `hover_started_at` is Ui-time at first
/// hovered frame; elapsed = `now - hover_started_at`, immune to
/// the frame runtime's `MAX_DT` clamp on idle wakes.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct TooltipState {
    pub(crate) hover_started_at: Option<Duration>,
    pub(crate) visible: bool,
}

/// Singleton tracking the most recent moment any tooltip was visible.
/// Cold-start tooltips within `theme.warmup` of `last_visible_at`
/// skip their delay (egui-style toolbar warmup).
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct TooltipGlobal {
    pub(crate) last_visible_at: Option<Duration>,
}

static GLOBAL_STATE_ID: LazyLock<WidgetId> =
    LazyLock::new(|| WidgetId::from_hash("aperture.tooltip.global"));

/// Hover-driven text bubble attached to a trigger widget. Records into
/// [`crate::forest::layer::Layer::Tooltip`] after the pointer has rested
/// on the trigger for [`crate::widgets::theme::tooltip::TooltipTheme::delay`]
/// seconds. A short warmup window (configured on the theme) keeps
/// subsequent tooltips instant after one was dismissed, so scanning a
/// row of buttons doesn't re-delay on every move.
///
/// Two-line attachment:
///
/// ```ignore
/// let r = Button::new().label("Save").show(ui);
/// Tooltip::on(&r).text("Persist changes (Ctrl+S)").show(ui);
/// ```
///
/// Tooltips are pointer-driven only and skip recording on disabled
/// triggers by default. Pass `.show_when_disabled(true)` to opt in for
/// "why is this disabled?" hints.
#[derive(Debug)]
pub struct Tooltip<'r, 'a> {
    snapshot: &'r ResponseSnapshot,
    text: TextInput<'a>,
    delay: Option<Duration>,
    show_when_disabled: bool,
    element: Element,
    chrome: Option<Background>,
}

impl<'r> Tooltip<'r, 'static> {
    /// Attach a tooltip to the given trigger response snapshot. The
    /// snapshot carries the trigger's `WidgetId` and last-frame rect
    /// — both drive timer keying and anchor computation. Pass via
    /// `trigger.snapshot()` to detach from the trigger's `&Ui`
    /// borrow before recording the tooltip body.
    #[track_caller]
    pub fn on(snapshot: &'r ResponseSnapshot) -> Self {
        let mut element = Element::vstack();
        // Bubble must never claim hover — would shadow its own trigger.
        element.flags.set_sense(Sense::empty());
        Self {
            snapshot,
            text: TextInput::default(),
            delay: None,
            show_when_disabled: false,
            element,
            chrome: None,
        }
    }
}

impl<'r, 'a> Tooltip<'r, 'a> {
    /// Paint chrome (fill / stroke / corner radius / shadow). `None`
    /// is the default; theme fallback in [`Self::show`] fills it in
    /// from `ui.theme.tooltip.panel` when unset.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    pub fn text<'text>(self, text: impl Into<TextInput<'text>>) -> Tooltip<'r, 'text> {
        Tooltip {
            snapshot: self.snapshot,
            text: text.into(),
            delay: self.delay,
            show_when_disabled: self.show_when_disabled,
            element: self.element,
            chrome: self.chrome,
        }
    }

    /// Override the per-tooltip delay. Falls back to
    /// [`crate::widgets::theme::tooltip::TooltipTheme::delay`] when unset.
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// Allow the tooltip to fire on disabled triggers. Off by default —
    /// most disabled tooltips would be UX noise.
    pub fn show_when_disabled(mut self, yes: bool) -> Self {
        self.show_when_disabled = yes;
        self
    }

    /// Tick the hover timer, update visibility, and (when visible)
    /// record the bubble into `Layer::Tooltip` anchored next to the
    /// trigger.
    pub fn show(self, ui: &mut Ui) {
        let delay = self.delay.unwrap_or(ui.theme.tooltip.delay);
        let warmup = ui.theme.tooltip.warmup;
        let gap = ui.theme.tooltip.gap;

        let trigger_id = self.snapshot.id;
        let bubble_id = trigger_id.with("tooltip.bubble");
        let g_id = *GLOBAL_STATE_ID;

        // Accepting an override would split bubble identity from the trigger-owned lifecycle.
        debug_assert!(
            matches!(self.element.salt, Salt::Auto(_)),
            "Tooltip does not honor `.id(...)` / `.id_salt(...)` — the id is \
             derived from the trigger's response so per-trigger state stays \
             paired. Drop the override.",
        );

        let trigger_hovered = self.snapshot.state.hovered;
        let trigger_disabled = self.snapshot.state.disabled;
        let trigger_rect = self.snapshot.state.rect;
        let active_trigger = trigger_hovered && (!trigger_disabled || self.show_when_disabled);

        let now = ui.now();

        let mut state: TooltipState = *ui.state_mut::<TooltipState>(trigger_id);
        let mut global: TooltipGlobal = *ui.state_mut::<TooltipGlobal>(g_id);

        let warmup_active = global
            .last_visible_at
            .is_some_and(|t| now.saturating_sub(t) < warmup);

        if active_trigger {
            let started = match state.hover_started_at {
                Some(t) => t,
                None => {
                    state.hover_started_at = Some(now);
                    // One wake at the threshold is enough — the queue
                    // remembers it. If the user moves off before then
                    // the wake still fires into a no-op frame; cheap.
                    ui.request_repaint_after(delay);
                    now
                }
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
            global.last_visible_at = Some(now);
            let position = OverlayPosition::below(trigger_rect, gap);
            let text = self.text;
            // Theme fallbacks: ZERO padding / INF max_size / None
            // chrome mean "inherit from theme.tooltip".
            let mut element = self.element;
            element.salt = Salt::Verbatim(bubble_id);
            let text_style = ui.theme.tooltip.text.clone();
            let chrome = self
                .chrome
                .unwrap_or_else(|| ui.theme.tooltip.panel.clone());
            if element.padding == Spacing::ZERO {
                element.padding = ui.theme.tooltip.padding;
            }
            if element.max_size == Size::INF {
                element.max_size = ui.theme.tooltip.max_size;
            }
            ui.overlay_layer(Layer::Tooltip, position, |ui| {
                ui.node(bubble_id, element, Some(&chrome), |ui| {
                    Text::new(text)
                        .style(&text_style)
                        .text_wrap(TextWrap::Wrap)
                        .show(ui);
                });
            });
        }

        *ui.state_mut::<TooltipState>(trigger_id) = state;
        *ui.state_mut::<TooltipGlobal>(g_id) = global;
    }
}

impl Configure for Tooltip<'_, '_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

#[cfg(test)]
mod tests;
