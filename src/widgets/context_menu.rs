use crate::forest::element::{Configure, Element, LayoutMode};
use crate::forest::widget_id::WidgetId;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx::EPS;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::shape::{Shape, TextWrap};
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::popup::{ClickOutside, Popup, PopupResponse};

use glam::Vec2;
use std::borrow::Cow;

/// Cross-frame state for one context-menu site, keyed off the trigger
/// widget's id in [`crate::ui::state::StateMap`]. `anchor = Some` is
/// the single source of truth for "menu open".
///
/// `last_size` is the menu container's measured size from the most
/// recent open frame, reused to clamp future-frame anchors so the
/// menu never spills off the surface. On the first open after a
/// content change `ContextMenu::show` calls
/// [`Ui::request_relayout`] so the discard-and-rerecord pass paints
/// the clamped anchor without a one-frame flicker — same machinery
/// that scroll content-size changes rely on.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct ContextMenuState {
    pub(crate) anchor: Option<Vec2>,
    pub(crate) last_size: Option<Size>,
}

/// A right-click / programmatically-opened popup menu attached to a
/// trigger widget. State lives in `StateMap` keyed off the trigger
/// id, so opening / dismissing survives across frames without the
/// caller threading a flag.
///
/// Typical usage chains [`Self::attach`] off a trigger's `Response`,
/// which auto-opens at the pointer on `secondary_clicked`:
///
/// ```ignore
/// let trigger = Button::new().label("…").show(ui);
/// ContextMenu::attach(ui, &trigger).show(ui, |ui| { … });
/// ```
///
/// For programmatic opens (keyboard shortcut, custom gesture) call
/// [`Self::open`] before [`Self::for_id`]`(id).show(...)`.
///
/// Closes on outside-click, on Esc, or when any [`MenuItem`] inside
/// reports `clicked()`.
pub struct ContextMenu {
    for_id: WidgetId,
}

impl ContextMenu {
    pub fn for_id(for_id: WidgetId) -> Self {
        Self { for_id }
    }

    /// Derive `for_id` from a trigger widget's response, and auto-open
    /// at the current pointer position if the trigger reported
    /// `secondary_clicked` this frame.
    pub fn attach(ui: &mut Ui, resp: &Response) -> Self {
        if resp.secondary_clicked()
            && let Some(p) = ui.pointer_pos()
        {
            ContextMenu::open(ui, resp.widget_id(), p);
        }
        Self {
            for_id: resp.widget_id(),
        }
    }

    /// Record the menu and return per-frame outcome. The body closure
    /// records [`MenuItem`]s inside `Layer::Popup`; the menu auto-
    /// closes on outside-click, Esc, or an item click.
    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> ContextMenuResponse {
        if ui.escape_pressed() {
            ContextMenu::close(ui, self.for_id);
        }

        let st = *ui.state_mut::<ContextMenuState>(self.for_id);
        let Some(raw_anchor) = st.anchor else {
            return ContextMenuResponse::default();
        };

        let surface = ui.display().logical_rect();
        let clamped = clamp_anchor(raw_anchor, st.last_size, surface);

        let theme = ui.theme.context_menu.clone();
        let body_id = self.for_id.with("ctx_menu_body");

        let popup = Popup::anchored_to(clamped)
            .id(body_id)
            .click_outside(ClickOutside::Dismiss)
            .owned_by(self.for_id)
            .background(theme.panel)
            .padding(theme.padding)
            .min_size(Size::new(theme.min_width, 0.0));
        let PopupResponse {
            body: body_resp,
            dismissed,
            close_requested: item_clicked,
        } = popup.show(ui, body);

        // Record measured size for next-frame clamp. If this is the
        // first frame the menu opened (last_size was None) or if the
        // body grew/shrank, ask for a re-record so the clamp lands in
        // the same visible frame (same machinery scroll uses).
        let measured = body_resp.state.rect.map(|r| r.size);
        let need_relayout = match (measured, st.last_size) {
            (Some(now), Some(prev)) => (now.w - prev.w).abs() > EPS || (now.h - prev.h).abs() > EPS,
            (Some(_), None) => true,
            _ => false,
        };
        let st_mut = ui.state_mut::<ContextMenuState>(self.for_id);
        if let Some(m) = measured {
            st_mut.last_size = Some(m);
        }

        if dismissed || item_clicked {
            ContextMenu::close(ui, self.for_id);
        } else if need_relayout {
            ui.request_relayout();
        }

        ContextMenuResponse {
            dismissed,
            item_clicked,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ContextMenuResponse {
    pub dismissed: bool,
    pub item_clicked: bool,
}

/// Host-facing lifecycle for context menus, keyed off a trigger
/// `WidgetId`. Cross-frame state lives in [`Ui::state`]; these are the
/// only entrypoints — `ContextMenu::show` is the per-frame recorder.
impl ContextMenu {
    /// Open the context menu keyed off `for_id` at surface-space
    /// `anchor`. Idempotent — repeated calls refresh the anchor.
    pub fn open(ui: &mut Ui, for_id: WidgetId, anchor: Vec2) {
        ui.state_mut::<ContextMenuState>(for_id).anchor = Some(anchor);
    }

    /// Close the context menu keyed off `for_id`. No-op if already closed.
    pub fn close(ui: &mut Ui, for_id: WidgetId) {
        ui.state_mut::<ContextMenuState>(for_id).anchor = None;
    }

    /// `true` while the menu keyed off `for_id` has an active anchor.
    /// Cheap immutable probe — no row is allocated for triggers that
    /// have never been opened.
    pub fn is_open(ui: &Ui, for_id: WidgetId) -> bool {
        ui.try_state::<ContextMenuState>(for_id)
            .is_some_and(|st| st.anchor.is_some())
    }
}

pub(crate) fn clamp_anchor(raw: Vec2, size: Option<Size>, surface: Rect) -> Vec2 {
    let Some(s) = size else {
        return raw;
    };
    let max_x = (surface.min.x + surface.size.w - s.w).max(surface.min.x);
    let max_y = (surface.min.y + surface.size.h - s.h).max(surface.min.y);
    Vec2::new(
        raw.x.min(max_x).max(surface.min.x),
        raw.y.min(max_y).max(surface.min.y),
    )
}

// ── MenuItem ────────────────────────────────────────────────────────

/// One row inside a [`ContextMenu`]. Label on the left, optional
/// right-aligned shortcut hint, theme-driven hover chrome. Reports
/// `Response` so callers branch on `clicked()`; the row also calls
/// [`Popup::request_close`] on click so the parent `ContextMenu`
/// auto-closes without the caller threading state.
pub struct MenuItem {
    element: Element,
    label: Cow<'static, str>,
    shortcut: Option<Cow<'static, str>>,
}

impl MenuItem {
    #[track_caller]
    pub fn new(label: impl Into<Cow<'static, str>>) -> Self {
        let mut element = Element::new(LayoutMode::HStack);
        element.id = WidgetId::auto_stable();
        element.id_source = crate::forest::seen_ids::IdSource::Auto;
        element.sense = Sense::CLICK;
        Self {
            element,
            label: label.into(),
            shortcut: None,
        }
    }

    pub fn shortcut(mut self, s: impl Into<Cow<'static, str>>) -> Self {
        self.shortcut = Some(s.into());
        self
    }

    pub fn enabled(mut self, e: bool) -> Self {
        self.element.disabled = !e;
        self
    }

    /// Thin horizontal divider — no label, no input. Free function in
    /// disguise: chain `.show(ui)` and ignore the response.
    #[track_caller]
    pub fn separator(ui: &mut Ui) -> Response {
        let mut element = Element::new(LayoutMode::Leaf);
        element.id = WidgetId::auto_stable();
        element.id_source = crate::forest::seen_ids::IdSource::Auto;
        element.sense = Sense::NONE;
        element.size = (Sizing::FILL, Sizing::Fixed(1.0)).into();
        element.margin = Spacing::xy(0.0, 4.0);
        element.chrome = Some(Background {
            fill: ui.theme.context_menu.separator.into(),
            stroke: Stroke::ZERO,
            radius: Corners::ZERO,
        });
        let id = element.id;
        let node = ui.node(element, |_| {});
        let state = ui.response_for(id);
        Response { node, id, state }
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let id = self.element.id;
        let disabled = self.element.disabled;
        let mut raw_state = ui.response_for(id);
        raw_state.disabled = disabled;

        let theme = ui.theme.context_menu.item.clone();
        let look = theme.pick(raw_state);
        let look_bg = look.background;
        let text_style = look.text.unwrap_or(ui.theme.text);
        let label_color = text_style.color;
        let font_size_px = text_style.font_size_px;
        let line_height_px = text_style.line_height_for(font_size_px);
        let shortcut_color = theme.shortcut;
        let padding = theme.padding;

        let mut element = self.element;
        element.size = (Sizing::FILL, Sizing::Hug).into();
        element.padding = padding;
        element.chrome = look_bg;
        element.gap = 16.0;

        let label = self.label;
        let shortcut = self.shortcut;

        let node = ui.node(element, |ui| {
            // Label cell — Fill grabs all leftover; shortcut hugs.
            let mut label_el = Element::new(LayoutMode::Leaf);
            label_el.id = id.with("label");
            label_el.id_source = crate::forest::seen_ids::IdSource::Explicit;
            label_el.size = (Sizing::FILL, Sizing::Hug).into();
            ui.node(label_el, |ui| {
                ui.add_shape(Shape::Text {
                    local_rect: None,
                    text: label,
                    brush: label_color.into(),
                    font_size_px,
                    line_height_px,
                    wrap: TextWrap::Single,
                    align: crate::layout::types::align::Align::default(),
                });
            });
            if let Some(s) = shortcut {
                let mut sh_el = Element::new(LayoutMode::Leaf);
                sh_el.id = id.with("shortcut");
                sh_el.id_source = crate::forest::seen_ids::IdSource::Explicit;
                sh_el.size = (Sizing::Hug, Sizing::Hug).into();
                ui.node(sh_el, |ui| {
                    ui.add_shape(Shape::Text {
                        local_rect: None,
                        text: s,
                        brush: shortcut_color.into(),
                        font_size_px,
                        line_height_px,
                        wrap: TextWrap::Single,
                        align: crate::layout::types::align::Align::default(),
                    });
                });
            }
        });

        let state = ui.response_for(id);
        let resp = Response { node, id, state };
        if resp.clicked() {
            Popup::request_close(ui);
        }
        resp
    }
}

impl Configure for MenuItem {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
