use crate::forest::element::{Configure, Element, LayoutMode};
use crate::forest::element::Salt;
use crate::forest::tree::Layer;
use crate::input::sense::Sense;
use crate::primitives::background::Background;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::text::Text;
use glam::Vec2;
use std::borrow::Cow;

/// Result of `place_anchor` — anchor point and a flag noting whether
/// the bubble was flipped above the trigger instead of below.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlacedAnchor {
    pub(crate) anchor: Vec2,
    pub(crate) flipped_above: bool,
}

/// Pure positioning math: pick a top-left anchor for a `bubble`-sized
/// bubble next to a `trigger` rect, inside `viewport`. Default = below;
/// flip above when below would clip; horizontally clamp so the bubble
/// stays on-screen. `gap` is the breathing room between trigger and
/// bubble.
pub(crate) fn place_anchor(trigger: Rect, bubble: Size, viewport: Rect, gap: f32) -> PlacedAnchor {
    let below_y = trigger.min.y + trigger.size.h + gap;
    let above_y = trigger.min.y - gap - bubble.h;
    let viewport_bottom = viewport.min.y + viewport.size.h;
    let viewport_right = viewport.min.x + viewport.size.w;
    let fits_below = below_y + bubble.h <= viewport_bottom;
    let (y, flipped_above) = if fits_below || above_y < viewport.min.y {
        (below_y, false)
    } else {
        (above_y, true)
    };
    let x = trigger.min.x.clamp(
        viewport.min.x,
        (viewport_right - bubble.w).max(viewport.min.x),
    );
    PlacedAnchor {
        anchor: Vec2::new(x, y),
        flipped_above,
    }
}

/// Per-trigger tooltip state. `hover_started_at` is Ui-time at first
/// hovered frame; elapsed = `now - hover_started_at`, immune to
/// `Ui::dt`'s `MAX_DT` clamp on idle wakes. `last_size` caches the
/// previous frame's bubble extent for anchor flip/clamp.
#[derive(Default, Clone, Copy)]
pub(crate) struct TooltipState {
    pub(crate) hover_started_at: Option<f32>,
    pub(crate) visible: bool,
    pub(crate) last_size: Size,
}

/// Singleton tracking the most recent moment any tooltip was visible.
/// Cold-start tooltips within `theme.warmup` of `last_visible_at`
/// skip their delay (egui-style toolbar warmup).
#[derive(Default, Clone, Copy)]
pub(crate) struct TooltipGlobal {
    pub(crate) last_visible_at: Option<f32>,
}

static GLOBAL_STATE_ID: std::sync::LazyLock<WidgetId> =
    std::sync::LazyLock::new(|| WidgetId::from_hash("palantir.tooltip.global"));

/// Hover-driven text bubble attached to a trigger widget. Records into
/// [`crate::forest::tree::Layer::Tooltip`] after the pointer has rested
/// on the trigger for [`crate::widgets::theme::tooltip::TooltipTheme::delay`]
/// seconds. A short warmup window (configured on the theme) keeps
/// subsequent tooltips instant after one was dismissed, so scanning a
/// row of buttons doesn't re-delay on every move.
///
/// Two-line attachment:
///
/// ```ignore
/// let r = Button::new().label("Save").show(ui);
/// Tooltip::for_(&r).text("Persist changes (Ctrl+S)").show(ui);
/// ```
///
/// Tooltips are pointer-driven only and skip recording on disabled
/// triggers by default. Pass `.show_when_disabled(true)` to opt in for
/// "why is this disabled?" hints.
pub struct Tooltip<'r> {
    response: &'r Response,
    text: Cow<'static, str>,
    delay: Option<f32>,
    show_when_disabled: bool,
    element: Element,
    chrome: Option<Background>,
}

impl<'r> Tooltip<'r> {
    /// Attach a tooltip to the given trigger response. The response
    /// carries the trigger's `WidgetId` and last-frame rect — both
    /// drive timer keying and anchor computation.
    #[track_caller]
    pub fn for_(response: &'r Response) -> Self {
        let mut element = Element::new(LayoutMode::VStack);
        // Bubble must never claim hover — would shadow its own trigger.
        element.set_sense(Sense::empty());
        Self {
            response,
            text: Cow::Borrowed(""),
            delay: None,
            show_when_disabled: false,
            element,
            chrome: None,
        }
    }

    /// Paint chrome (fill / stroke / corner radius / shadow). `None`
    /// is the default; theme fallback in [`Self::show`] fills it in
    /// from `ui.theme.tooltip.panel` when unset.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    pub fn text(mut self, t: impl Into<Cow<'static, str>>) -> Self {
        self.text = t.into();
        self
    }

    /// Override the per-tooltip delay (seconds). Falls back to
    /// [`crate::widgets::theme::tooltip::TooltipTheme::delay`] when unset.
    pub fn delay(mut self, secs: f32) -> Self {
        self.delay = Some(secs);
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

        let trigger_id = self.response.id;
        let state_id = trigger_id.with("tooltip");
        let bubble_id = trigger_id.with("tooltip.bubble");
        let g_id = *GLOBAL_STATE_ID;

        // State keying needs `bubble_id` for the StateMap row to live
        // alongside the trigger's lifecycle. A caller-supplied id via
        // `.id_salt(...)` would silently be overwritten — hard-assert
        // instead of swallowing the bug.
        assert!(
            matches!(self.element.salt, Salt::Auto(_)),
            "Tooltip does not honor `.id(...)` / `.id_salt(...)` — the id is \
             derived from the trigger's response so per-trigger state stays \
             paired. Drop the override.",
        );

        let trigger_hovered = self.response.state.hovered;
        let trigger_disabled = self.response.state.disabled;
        let trigger_rect = self.response.state.rect;
        let active_trigger = trigger_hovered && (!trigger_disabled || self.show_when_disabled);

        let now = ui.time.as_secs_f32();

        let mut state: TooltipState = *ui.state_mut::<TooltipState>(state_id);
        let mut global: TooltipGlobal = *ui.state_mut::<TooltipGlobal>(g_id);

        let warmup_active = global.last_visible_at.is_some_and(|t| (now - t) < warmup);

        if active_trigger {
            let started = match state.hover_started_at {
                Some(t) => t,
                None => {
                    state.hover_started_at = Some(now);
                    // One wake at the threshold is enough — the queue
                    // remembers it. If the user moves off before then
                    // the wake still fires into a no-op frame; cheap.
                    ui.request_repaint_after(std::time::Duration::from_secs_f32(delay));
                    now
                }
            };
            let elapsed = now - started;
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
            let viewport = ui.display().logical_rect();
            let placed = place_anchor(trigger_rect, state.last_size, viewport, gap);
            let text = self.text;
            // Theme fallbacks: ZERO padding / INF max_size / None
            // chrome mean "inherit from theme.tooltip".
            let mut element = self.element;
            element.salt = Salt::Verbatim(bubble_id);
            let text_style = ui.theme.tooltip.text;
            let chrome = self.chrome.unwrap_or(ui.theme.tooltip.panel);
            if element.padding == Spacing::ZERO {
                element.padding = ui.theme.tooltip.padding;
            }
            if element.max_size == Size::INF {
                element.max_size = ui.theme.tooltip.max_size;
            }
            ui.layer(Layer::Tooltip, placed.anchor, None, |ui| {
                ui.node_with_chrome(bubble_id, element, chrome, |ui| {
                    Text::new(text).style(text_style).wrapping().show(ui);
                });
            });
            if let Some(r) = ui.response_for(bubble_id).rect {
                state.last_size = r.size;
            }
        }

        *ui.state_mut::<TooltipState>(state_id) = state;
        *ui.state_mut::<TooltipGlobal>(g_id) = global;
    }
}

impl Configure for Tooltip<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
