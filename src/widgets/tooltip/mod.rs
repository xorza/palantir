//! The hover tooltip: the widget, the per-trigger hover clock it needs to
//! honour a delay, and the app-global state that lets a second tooltip
//! appear without re-serving the delay.

use crate::input::sense::Sense;
use crate::layout::types::overlay::OverlayPosition;
use crate::primitives::background::Background;
use crate::primitives::text_input::TextInput;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::Node;
use crate::scene::node::theme_defaults::ThemeDefaults;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widgets::overlay_scope::{Backdrop, OverlayScope};
use crate::widgets::response::ResponseSnapshot;
use crate::widgets::text::Text;
use crate::widgets::theme::tooltip::TooltipTheme;
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
/// Tooltip::on(&r).text("Persist changes (Ctrl+S)").show(ui);
/// # }
/// ```
///
/// Tooltips are pointer-driven only and skip recording on disabled
/// triggers by default. Pass `.show_when_disabled(true)` to opt in for
/// "why is this disabled?" hints.
///
/// Implements [`Configure`](crate::Configure), so the bubble takes `.padding(...)`,
/// `.max_size(...)`, `.size(...)`, `.margin(...)` and the rest like any
/// other widget. Identity defaults to the trigger's id — a tooltip has
/// no call site of its own worth keying on — but an explicit `.id(...)`
/// / `.id_salt(...)` wins.
#[derive(Debug)]
pub struct Tooltip<'r, 'a> {
    snapshot: &'r ResponseSnapshot,
    text: TextInput<'a>,
    delay: Option<Duration>,
    show_when_disabled: bool,
    node: Node,
    chrome: Option<Background>,
    /// Keyed to `'r` (the snapshot's lifetime), not `'a`: [`Self::text`]
    /// rebinds `'a` to whatever the new text borrows from, and the theme
    /// has to survive that swap.
    style: Option<&'r TooltipTheme>,
}

impl<'r> Tooltip<'r, 'static> {
    /// Attach a tooltip to the given trigger response snapshot. The
    /// snapshot carries the trigger's `WidgetId` and last-frame rect
    /// — both drive timer keying and anchor computation. Pass via
    /// `trigger.snapshot()` to detach from the trigger's `&Ui`
    /// borrow before recording the tooltip body.
    #[track_caller]
    pub fn on(snapshot: &'r ResponseSnapshot) -> Self {
        let mut node = Node::vstack();
        // Bubble must never claim hover — would shadow its own trigger.
        node.flags.set_sense(Sense::empty());
        Self {
            snapshot,
            text: TextInput::default(),
            delay: None,
            show_when_disabled: false,
            node,
            chrome: None,
            style: None,
        }
    }
}

impl<'r, 'a> Tooltip<'r, 'a> {
    style_setter!(
        'r,
        TooltipTheme,
        tooltip,
        "Per-field [`Self::background`] / [`Self::delay`] still win over it.",
    );

    pub fn text<'text>(self, text: impl Into<TextInput<'text>>) -> Tooltip<'r, 'text> {
        Tooltip {
            snapshot: self.snapshot,
            text: text.into(),
            delay: self.delay,
            show_when_disabled: self.show_when_disabled,
            node: self.node,
            chrome: self.chrome,
            style: self.style,
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
        // Handle, not a borrow: the bundle may point into the `Ui`'s own
        // theme, and the record below reborrows `ui` mutably.
        let ui_theme = ui.theme().clone();
        let theme = self.slot(&ui_theme);
        let delay = self.delay.unwrap_or(theme.delay);
        let warmup = theme.warmup;
        let gap = theme.gap;

        let trigger_id = self.snapshot.id;
        let bubble_id = trigger_id.with("bubble");

        let trigger_hovered = self.snapshot.state.hovered;
        let trigger_disabled = self.snapshot.state.disabled;
        let trigger_rect = self.snapshot.state.rect;
        let active_trigger = trigger_hovered && (!trigger_disabled || self.show_when_disabled);

        let now = ui.now();

        // A tooltip attaches to a trigger that is idle on almost every
        // frame it is recorded, so nothing here may touch the state map
        // unconditionally: the read probes without materialising a row, the
        // warmup singleton is only asked for by a hovered trigger, and the
        // write-back below is gated on an actual change.
        let prev: TooltipState = ui
            .try_state::<TooltipState>(trigger_id)
            .copied()
            .unwrap_or_default();
        let mut state = prev;

        if active_trigger {
            let warmup_active = ui
                .try_state::<TooltipGlobal>(global_state_id())
                .and_then(|global| global.last_visible_at)
                .is_some_and(|t| now.saturating_sub(t) < warmup);
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
            ui.state_mut::<TooltipGlobal>(global_state_id())
                .last_visible_at = Some(now);
            let position = OverlayPosition::below(trigger_rect, gap);
            let text = self.text;
            let chrome = self.chrome.as_ref().unwrap_or(&theme.panel);
            // Theme fills in whatever the caller left alone. Identity
            // derives from the trigger, because that is the only thing a
            // tooltip *has* — but a caller-set id wins like any other
            // explicit value.
            let mut node = self
                .node
                .default_id(bubble_id)
                .default_padding(theme.padding)
                .default_max_size(theme.max_size);
            // `Backdrop::None`: a tooltip annotates rather than
            // interrupts, and it is recorded every frame it is up — a
            // scope would cut off every layer below it for as long.
            let scope = OverlayScope::claim(
                bubble_id,
                Layer::Tooltip,
                position,
                Backdrop::None,
                &mut node,
            );
            scope.record(ui, |ui| {
                ui.widget(node).record(ui, Some(chrome), |ui| {
                    Text::new(text)
                        .style(&theme.text)
                        .text_wrap(TextWrap::Wrap)
                        .show(ui);
                });
            });
        }

        if state != prev {
            *ui.state_mut::<TooltipState>(trigger_id) = state;
        }
    }
}

impl_background!(
    Tooltip<'_, '_>,
    "`None` is the default; theme fallback in [`Self::show`] fills it in from \
     `ui.theme().tooltip.panel` when unset. Pass [`Background::NONE`] to \
     suppress the themed bubble chrome.",
);
impl_configure!(Tooltip<'_, '_>);

#[cfg(test)]
mod tests;
