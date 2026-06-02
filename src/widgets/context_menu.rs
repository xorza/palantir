use crate::forest::element::{Configure, Element, LayoutMode, Salt};
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::align::{Align, HAlign};
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::popup::{ClickOutside, Popup, PopupHandle, PopupResponse};
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::{Response, ResponseSnapshot, WidgetEntry, enter_widget};

use crate::primitives::interned_str::InternedStr;
use glam::Vec2;

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
        element.flags.set_sense(Sense::CLICK);
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

    /// Derive `for_id` from a trigger widget's response snapshot, and
    /// auto-open at the current pointer position if the trigger
    /// reported `secondary_clicked` this frame. Pass via
    /// `trigger.snapshot()` to detach from the trigger's `&Ui`
    /// borrow before attaching the menu.
    pub fn attach(ui: &mut Ui, snapshot: &ResponseSnapshot) -> Self {
        if snapshot.secondary_clicked()
            && let Some(p) = ui.pointer_pos()
        {
            ContextMenu::open(ui, snapshot.widget_id(), p);
        }
        ContextMenu::for_id(snapshot.widget_id())
    }

    /// Record the menu and return per-frame outcome. The body closure
    /// records [`MenuItem`]s inside `Layer::Popup`; the menu auto-
    /// closes on outside-click, Esc, or an item click.
    pub fn show(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &PopupHandle),
    ) -> ContextMenuResponse {
        if ui.escape_pressed() {
            ContextMenu::close(ui, self.for_id);
        }

        let Some(raw_anchor) = ui.state_mut::<ContextMenuState>(self.for_id).anchor else {
            return ContextMenuResponse::default();
        };

        let body_id = self.for_id.with("ctx_menu_body");

        // Borrow the sub-theme to copy out the two scalars and the single
        // `Background` we keep — avoids cloning the whole `ContextMenuTheme`
        // (including the unused per-item looks) every open frame.
        let ctx = &ui.theme.context_menu;
        let theme_padding = ctx.padding;
        let theme_min_width = ctx.min_width;
        let panel = self.chrome.unwrap_or_else(|| ctx.panel.clone());

        // Id is derived from `for_id` so per-site state pairs with the
        // trigger; a caller-supplied `.id_salt(...)` would be silently
        // dropped — hard-assert instead (mirrors `Tooltip`).
        assert!(
            matches!(self.element.salt, Salt::Auto(_)),
            "ContextMenu does not honor `.id(...)` / `.id_salt(...)` — its id is \
             derived from the trigger so per-site state stays paired. Drop the override.",
        );

        let mut e = self.element;
        e.salt = Salt::Verbatim(body_id);
        if e.padding == Spacing::ZERO {
            e.padding = theme_padding;
        }
        if e.min_size.w <= 0.0 {
            e.min_size.w = theme_min_width;
        }

        // `Popup::show` handles surface-aware placement (flip when
        // overflowing, clamp as a last resort, one-shot relayout on
        // first open). ContextMenu just hands the raw anchor through.
        let mut popup = Popup::anchored_to(raw_anchor)
            .click_outside(ClickOutside::Dismiss)
            .background(panel);
        *popup.element_mut() = e;
        let PopupResponse {
            dismissed,
            close_requested: item_clicked,
        } = popup.show(ui, body);

        if dismissed || item_clicked {
            ContextMenu::close(ui, self.for_id);
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
    label: InternedStr,
    shortcut: Option<Shortcut>,
}

impl MenuItem {
    #[track_caller]
    pub fn new(label: impl Into<InternedStr>) -> Self {
        let mut element = Element::new(LayoutMode::HStack);
        element.flags.set_sense(Sense::CLICK);
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
        self.element.flags.set_disabled(!e);
        self
    }

    /// Thin horizontal divider — no label, no input. Free function in
    /// disguise: chain `.show(ui)` and ignore the response.
    #[track_caller]
    pub fn separator(ui: &mut Ui) -> Response<'_> {
        let mut element = Element::new(LayoutMode::Leaf);
        element.flags.set_sense(Sense::NONE);
        // Hug+Stretch (not Fill) — avoids leaking INF width up to the Hug menu container. See `docs/popups.md`.
        element.size = (Sizing::Hug, Sizing::Fixed(1.0)).into();
        element.align = Align::h(HAlign::Stretch);
        element.margin = Spacing::xy(0.0, 4.0);
        let chrome = Background {
            fill: ui.theme.context_menu.separator.into(),
            stroke: Stroke::ZERO,
            corners: Corners::ZERO,
            shadow: Shadow::NONE,
        };
        let id = ui.make_persistent_id(element.salt);
        ui.node(id, element, Some(&chrome), |_| {});
        // Decorative separator: response is almost always discarded.
        Response::lazy(id, ui)
    }

    pub fn show<'ui>(self, ui: &'ui mut Ui, popup: &PopupHandle) -> Response<'ui> {
        // Single `response_for` probe via the shared entry helper: the
        // row's body records only decorative `Text` leaves, so the state
        // is identical before and after `ui.node`. `merged` ORs the row's
        // own disabled flag onto the cascaded one (a disabled ancestor
        // still disables the row); `raw` stays pristine for the returned
        // `Response`.
        let WidgetEntry {
            id,
            raw: raw_state,
            merged: picked_state,
        } = enter_widget(ui, &self.element);
        let disabled = picked_state.disabled;

        // Borrow the per-item theme and copy out only what the row paints
        // — avoids cloning the whole `MenuItemTheme` (three looks) per
        // row, per frame. `pick` returns a borrow, so read everything off
        // it before the borrow ends.
        let item = &ui.theme.context_menu.item;
        let look = item.pick(picked_state);
        let look_bg = look.background.clone();
        let text_style = look.text.unwrap_or(ui.theme.text);
        let padding = item.padding;
        // Shortcut hint reads muted — same style as the label but the
        // theme's `shortcut` color.
        let shortcut_style = TextStyle {
            color: item.shortcut,
            ..text_style
        };

        let mut element = self.element;
        // Hug+Stretch+SpaceBetween: row hugs content, arrange stretches to widest row, label/shortcut pin to opposite edges. Fill would leak INF — see `docs/popups.md`.
        element.size = (Sizing::Hug, Sizing::Hug).into();
        element.align = Align::h(HAlign::Stretch);
        element.justify = Justify::SpaceBetween;
        element.padding = padding;
        element.gaps.set_gap(16.0);

        let label = self.label;
        let shortcut = self.shortcut;
        // Shortcut intercept: while the menu is open, a matching
        // keypress synthesizes a click and closes the menu. Resolved
        // before the node records so we don't pay for the label
        // resolution on rows with no shortcut.
        let shortcut_fired = shortcut.is_some_and(|s| !disabled && ui.key_pressed(s));
        let shortcut_label = shortcut.map(|s| s.label());

        // Label + optional right-aligned shortcut hint as `Text` leaves;
        // the row's `SpaceBetween` pins them to opposite edges. Both
        // hug their content (Text defaults to `Hug × Hug` and a
        // `SingleLine` wrap), matching what the row layout expects.
        let body = |ui: &mut Ui| {
            Text::new(label)
                .id(id.with("label"))
                .style(text_style)
                .show(ui);
            if let Some(s) = shortcut_label {
                Text::new(s)
                    .id(id.with("shortcut"))
                    .style(shortcut_style)
                    .show(ui);
            }
        };
        ui.node(id, element, look_bg.as_ref(), body);

        let mut state = raw_state;
        if shortcut_fired {
            state.clicked = true;
        }
        // Eager: `state` folds in the synthesized shortcut click, which
        // a lazy re-probe would drop.
        let resp = Response::eager(id, ui, state);
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
