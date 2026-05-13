use crate::forest::element::{Configure, Element, LayoutMode};
use crate::forest::widget_id::WidgetId;
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, HAlign};
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizing;
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
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct ContextMenuState {
    pub(crate) anchor: Option<Vec2>,
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
/// ContextMenu::attach(ui, &trigger)
///     .max_size((280.0, 400.0))
///     .show(ui, |ui| { … });
/// ```
///
/// For programmatic opens (keyboard shortcut, custom gesture) call
/// [`Self::open`] before [`Self::for_id`]`(id).show(...)`.
///
/// Closes on outside-click, on Esc, or when any [`MenuItem`] inside
/// reports `clicked()`.
///
/// Implements [`Configure`] — chain `.max_size(...)`, `.min_size(...)`,
/// `.padding(...)`, `.gap(...)`, `.background(...)`, etc. on the menu
/// body. Theme-driven defaults fill in any field the caller leaves
/// untouched (`chrome`, `padding`, `min_size.w`).
pub struct ContextMenu {
    for_id: WidgetId,
    element: Element,
}

impl ContextMenu {
    pub fn for_id(for_id: WidgetId) -> Self {
        let mut element = Element::new(LayoutMode::VStack);
        element.sense = Sense::CLICK;
        Self { for_id, element }
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
        ContextMenu::for_id(resp.widget_id())
    }

    /// Record the menu and return per-frame outcome. The body closure
    /// records [`MenuItem`]s inside `Layer::Popup`; the menu auto-
    /// closes on outside-click, Esc, or an item click.
    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> ContextMenuResponse {
        if ui.escape_pressed() {
            ContextMenu::close(ui, self.for_id);
        }

        let Some(raw_anchor) = ui.state_mut::<ContextMenuState>(self.for_id).anchor else {
            return ContextMenuResponse::default();
        };

        let surface = ui.display().logical_rect();
        let theme = ui.theme.context_menu.clone();
        let body_id = self.for_id.with("ctx_menu_body");

        // Use the body's most recent arranged rect to clamp the
        // anchor inside the surface. Cascade runs in `post_record`,
        // so on a re-record pass this returns pass A's rect — no
        // one-frame bleed. On the very first frame the menu opens
        // there's no prior cascade entry; we record at the raw
        // anchor and `request_relayout` so pass B can clamp.
        let prev_size = ui.response_for(body_id).rect.map(|r| r.size);
        let clamped = clamp_anchor(raw_anchor, prev_size, surface);
        let first_open = prev_size.is_none();

        // Apply theme defaults onto our element where the caller
        // didn't override. Mirrors Popup's `panel_background`
        // sentinel pattern.
        let mut e = self.element;
        e.id = body_id;
        e.id_source = crate::forest::seen_ids::IdSource::Explicit;
        if e.chrome.is_none() {
            e.chrome = Some(theme.panel);
        }
        if e.padding == Spacing::ZERO {
            e.padding = theme.padding;
        }
        if e.min_size.w <= 0.0 {
            e.min_size.w = theme.min_width;
        }

        let mut popup = Popup::anchored_to(clamped)
            .click_outside(ClickOutside::Dismiss)
            .owned_by(self.for_id);
        *popup.element_mut() = e;
        let PopupResponse {
            dismissed,
            close_requested: item_clicked,
            ..
        } = popup.show(ui, body);

        if dismissed || item_clicked {
            ContextMenu::close(ui, self.for_id);
        } else if first_open {
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

impl Configure for ContextMenu {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
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
        // `Hug` width + `Stretch` align (NOT `Fill`): same reasoning
        // as `MenuItem::show` — `Fill` would leak `INF` width up
        // through the Hug menu container and the menu would span the
        // surface. Hug-with-Stretch arranges to the body's inner.w
        // without growing the body during measure.
        element.size = (Sizing::Hug, Sizing::Fixed(1.0)).into();
        element.align = Align::h(HAlign::Stretch);
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
        // Hug both axes so each row measures to its natural width
        // (label + gap + shortcut). Stretch on the cross axis makes
        // arrange widen the row to the parent VStack's inner width
        // (= widest row, with `Hug` ancestors). `SpaceBetween` then
        // pushes label to the left edge and shortcut to the right
        // edge of the stretched row during arrange, since both
        // children are Hug-sized with leftover space between them.
        // Using `Fill` anywhere here would leak `INF` width up to
        // the Hug menu container and the menu would span the
        // surface — see `docs/popups.md`.
        element.size = (Sizing::Hug, Sizing::Hug).into();
        element.align = Align::h(HAlign::Stretch);
        element.justify = Justify::SpaceBetween;
        element.padding = padding;
        element.chrome = look_bg;
        element.gap = 16.0;

        let label = self.label;
        let shortcut = self.shortcut;

        let node = ui.node(element, |ui| {
            // Label cell — Hug width (NOT Fill, see comment on the
            // row element above). `Justify::SpaceBetween` on the row
            // pushes the label to the left during arrange.
            let mut label_el = Element::new(LayoutMode::Leaf);
            label_el.id = id.with("label");
            label_el.id_source = crate::forest::seen_ids::IdSource::Explicit;
            label_el.size = (Sizing::Hug, Sizing::Hug).into();
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
