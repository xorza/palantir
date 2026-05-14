use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::align::{Align, HAlign};
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::primitives::widget_id::WidgetId;
use crate::shape::{Shape, TextWrap};
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::popup::{ClickOutside, Popup, PopupHandle, PopupResponse};

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
///     .show(ui, |ui, popup| { MenuItem::new("…").show(ui, popup); });
/// ```
///
/// For programmatic opens (keyboard shortcut, custom gesture) call
/// [`Self::open`] before [`Self::for_id`]`(id).show(...)`.
///
/// Closes on outside-click, on Esc, when any [`MenuItem`] inside
/// reports `clicked()`, or when a [`MenuItem`]'s declared
/// [`Shortcut`] matches a keypress this frame.
///
/// Implements [`Configure`] — chain `.max_size(...)`, `.min_size(...)`,
/// `.padding(...)`, `.gap(...)`, `.background(...)`, etc. on the menu
/// body. Theme-driven defaults fill in any field the caller leaves
/// untouched (`chrome`, `padding`, `min_size.w`).
pub struct ContextMenu {
    for_id: WidgetId,
    element: Element,
    chrome: Option<Background>,
}

impl ContextMenu {
    pub fn for_id(for_id: WidgetId) -> Self {
        let mut element = Element::new(LayoutMode::VStack);
        element.sense = Sense::CLICK;
        Self {
            for_id,
            element,
            chrome: None,
        }
    }

    /// Paint chrome (fill / stroke / corner radius / shadow). `None`
    /// is the default; theme fallback in [`Self::show`] fills it in
    /// from `ui.theme.context_menu.panel` when unset.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
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
    pub fn show(
        &self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &PopupHandle),
    ) -> ContextMenuResponse {
        if ui.escape_pressed() {
            ContextMenu::close(ui, self.for_id);
        }

        let Some(raw_anchor) = ui.state_mut::<ContextMenuState>(self.for_id).anchor else {
            return ContextMenuResponse::default();
        };

        let surface = ui.display().logical_rect();
        let theme = ui.theme.context_menu.clone();
        let body_id = self.for_id.with("ctx_menu_body");

        // First open has no prior cascade entry — record raw and
        // request a relayout so pass B can clamp against measured size.
        let prev_size = ui.response_for(body_id).rect.map(|r| r.size);
        let clamped = clamp_anchor(raw_anchor, prev_size, surface);
        let first_open = prev_size.is_none();

        let mut e = self.element;
        e.set_id(body_id);
        if e.padding == Spacing::ZERO {
            e.padding = theme.padding;
        }
        if e.min_size.w <= 0.0 {
            e.min_size.w = theme.min_width;
        }

        let mut popup = Popup::anchored_to(clamped).click_outside(ClickOutside::Dismiss);
        *popup.element_mut() = e;
        popup.chrome = Some(self.chrome.unwrap_or(theme.panel));
        let PopupResponse {
            dismissed,
            close_requested: item_clicked,
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

#[derive(Clone, Copy, Debug, Default)]
pub struct ContextMenuResponse {
    pub dismissed: bool,
    pub item_clicked: bool,
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
/// [`PopupHandle::close`] on click so the parent `ContextMenu`
/// auto-closes without the caller threading state.
///
/// If [`Self::shortcut`] is set, the row also intercepts that
/// shortcut from this frame's key events: matching keypresses
/// synthesize a click (so `if item.clicked() { … }` fires) AND
/// close the menu, mirroring native menu behaviour. Disabled rows
/// don't intercept.
pub struct MenuItem {
    element: Element,
    label: Cow<'static, str>,
    shortcut: Option<Shortcut>,
}

impl MenuItem {
    #[track_caller]
    pub fn new(label: impl Into<Cow<'static, str>>) -> Self {
        let mut element = Element::new(LayoutMode::HStack);
        element.sense = Sense::CLICK;
        Self {
            element,
            label: label.into(),
            shortcut: None,
        }
    }

    /// Attach a keyboard shortcut. Renders the right-aligned hint
    /// using the platform's native form (`⌘C` / `Ctrl+C`) and
    /// intercepts that keypress while the menu is open. Glyph-only
    /// hints (no modifier, e.g. `Backspace → ⌫`) are expressed as
    /// `Shortcut::new(Mods::NONE, Key::Backspace)`.
    pub fn shortcut(mut self, s: Shortcut) -> Self {
        self.shortcut = Some(s);
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
        element.sense = Sense::NONE;
        // Hug+Stretch (not Fill) — avoids leaking INF width up to the Hug menu container. See `docs/popups.md`.
        element.size = (Sizing::Hug, Sizing::Fixed(1.0)).into();
        element.align = Align::h(HAlign::Stretch);
        element.margin = Spacing::xy(0.0, 4.0);
        let chrome = Background {
            fill: ui.theme.context_menu.separator.into(),
            stroke: Stroke::ZERO,
            radius: Corners::ZERO,
            shadow: Shadow::NONE,
        };
        let id = element.id;
        let node = ui.node_with_chrome(element, chrome, |_| {});
        let state = ui.response_for(id);
        Response { node, id, state }
    }

    pub fn show(self, ui: &mut Ui, popup: &PopupHandle) -> Response {
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
        // Hug+Stretch+SpaceBetween: row hugs content, arrange stretches to widest row, label/shortcut pin to opposite edges. Fill would leak INF — see `docs/popups.md`.
        element.size = (Sizing::Hug, Sizing::Hug).into();
        element.align = Align::h(HAlign::Stretch);
        element.justify = Justify::SpaceBetween;
        element.padding = padding;
        element.gap = 16.0;

        let label = self.label;
        let shortcut = self.shortcut;
        // Shortcut intercept: while the menu is open, a matching
        // keypress synthesizes a click and closes the menu. Resolved
        // before the node records so we don't pay for the label
        // resolution on rows with no shortcut.
        let shortcut_fired = shortcut.is_some_and(|s| !disabled && ui.shortcut_pressed(s));
        let shortcut_label = shortcut.map(|s| s.label());

        let family = text_style.family;
        let body = |ui: &mut Ui| {
            let mut label_el = Element::new(LayoutMode::Leaf);
            label_el.set_id(id.with("label"));
            label_el.size = (Sizing::Hug, Sizing::Hug).into();
            ui.node(label_el, |ui| {
                ui.add_shape(Shape::Text {
                    local_origin: None,
                    text: label,
                    brush: label_color.into(),
                    font_size_px,
                    line_height_px,
                    wrap: TextWrap::Single,
                    align: crate::layout::types::align::Align::default(),
                    family,
                });
            });
            if let Some(s) = shortcut_label {
                let mut sh_el = Element::new(LayoutMode::Leaf);
                sh_el.set_id(id.with("shortcut"));
                sh_el.size = (Sizing::Hug, Sizing::Hug).into();
                ui.node(sh_el, |ui| {
                    ui.add_shape(Shape::Text {
                        local_origin: None,
                        text: s,
                        brush: shortcut_color.into(),
                        font_size_px,
                        line_height_px,
                        wrap: TextWrap::Single,
                        align: crate::layout::types::align::Align::default(),
                        family,
                    });
                });
            }
        };
        let node = match look_bg {
            Some(c) => ui.node_with_chrome(element, c, body),
            None => ui.node(element, body),
        };

        let mut state = ui.response_for(id);
        if shortcut_fired {
            state.clicked = true;
        }
        let resp = Response { node, id, state };
        if resp.clicked() {
            popup.close();
        }
        resp
    }
}

impl Configure for MenuItem {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
