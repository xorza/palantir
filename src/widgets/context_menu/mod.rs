use crate::input::response::ButtonPhase;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::align::{Align, HAlign};
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizes;
use crate::primitives::background::Background;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::{Configure, ConfigureNode, Node};
use crate::ui::Ui;
use crate::widgets::popup::{ClickOutside, Popup, PopupHandle};
use crate::widgets::separator::Separator;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::{Response, ResponseSnapshot, enter_widget};

use crate::primitives::interned_str::TextInput;
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
/// which auto-opens at the pointer on a right-click (`right.clicked()`):
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
/// Chain `.size(...)`, `.max_size(...)`, `.min_size(...)`, `.padding(...)`,
/// `.gap(...)`, and `.background(...)` to configure the menu body. Theme-driven
/// defaults fill in any field the caller leaves untouched (`chrome`, `padding`,
/// `min_size.w`). Identity and input behavior remain owned by the trigger.
#[derive(Debug)]
pub struct ContextMenu {
    for_id: WidgetId,
    node: Node,
    chrome: Option<Background>,
}

impl ContextMenu {
    pub fn for_id(for_id: WidgetId) -> Self {
        let mut node = Node::vstack();
        node.flags.set_sense(Sense::CLICK);
        Self {
            for_id,
            node,
            chrome: None,
        }
    }

    /// Paint chrome (fill / stroke / corner radius / shadow). `None`
    /// is the default; theme fallback in [`Self::show`] fills it in
    /// from `ui.theme.context_menu.panel` when unset. Pass
    /// [`Background::NONE`] to suppress the themed menu chrome.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    pub fn size(mut self, size: impl Into<Sizes>) -> Self {
        self.node = self.node.size(size);
        self
    }

    pub fn min_size(mut self, size: impl Into<Size>) -> Self {
        self.node = self.node.min_size(size);
        self
    }

    pub fn max_size(mut self, size: impl Into<Size>) -> Self {
        self.node = self.node.max_size(size);
        self
    }

    pub fn padding(mut self, padding: impl Into<Spacing>) -> Self {
        self.node = self.node.padding(padding);
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.node = self.node.gap(gap);
        self
    }

    /// Derive `for_id` from a trigger widget's response snapshot, and
    /// auto-open at the current pointer position if the trigger
    /// reported a right-click this frame. Pass via
    /// `trigger.snapshot()` to detach from the trigger's `&Ui`
    /// borrow before attaching the menu.
    pub fn attach(ui: &mut Ui, snapshot: &ResponseSnapshot) -> Self {
        if snapshot.right.clicked()
            && let Some(p) = ui.pointer_pos()
        {
            ContextMenu::open(ui, snapshot.id, p);
        }
        ContextMenu::for_id(snapshot.id)
    }

    /// Record the menu and return per-frame outcome. The body closure
    /// records [`MenuItem`]s inside `Layer::Popup`; the menu auto-
    /// closes on outside-click, Esc, or an item click.
    pub fn show(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &PopupHandle<'_>),
    ) -> ContextMenuResponse {
        // Esc dismissal is owned by the `Dismiss` popup below — it folds into
        // `resp.closed()`, so no hand-rolled `escape_pressed` here.
        //
        // Read via `try_state` so a never-opened menu doesn't materialize a
        // StateMap row every frame `show` is called (matches `is_open`'s no-alloc
        // path); the row only needs to exist after `open`.
        let Some(raw_anchor) = ui
            .try_state::<ContextMenuState>(self.for_id)
            .and_then(|st| st.anchor)
        else {
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

        let mut e = self.node.id(body_id);
        e.padding.get_or_insert(theme_padding);
        e.min_size.get_or_insert(Size::new(theme_min_width, 0.0));

        // `Popup::show` resolves the current body against the surface.
        let mut popup = Popup::anchored_to(raw_anchor)
            .click_outside(ClickOutside::Dismiss)
            .background(panel);
        popup.node = e;
        let resp = popup.show(ui, body);
        if resp.closed() {
            ContextMenu::close(ui, self.for_id);
        }

        ContextMenuResponse {
            dismissed: resp.dismissed,
            item_clicked: resp.close_requested,
        }
    }

    /// Open the context menu keyed off `for_id` at surface-space
    /// `anchor`. Idempotent — repeated calls refresh the anchor.
    pub fn open(ui: &mut Ui, for_id: WidgetId, anchor: Vec2) {
        ui.state_mut::<ContextMenuState>(for_id).anchor = Some(anchor);
    }

    /// Close the context menu keyed off `for_id`. No-op if already closed.
    pub fn close(ui: &mut Ui, for_id: WidgetId) {
        if let Some(state) = ui.try_state_mut::<ContextMenuState>(for_id) {
            state.anchor = None;
        }
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

/// One row inside a [`ContextMenu`]. Label on the left, optional
/// right-aligned shortcut hint, theme-driven hover chrome. Reports
/// `Response` so callers branch on `clicked()`; the row also calls
/// [`PopupHandle::close`] on click so the parent `ContextMenu`
/// auto-closes without the caller threading state.
///
/// If [`Self::shortcut`] is set, the row also intercepts that
/// shortcut from this frame's key events: matching keypresses
/// synthesize a click (so `if item.left.clicked() { … }` fires) AND
/// close the menu, mirroring native menu behaviour. Disabled rows
/// don't intercept.
#[derive(Debug)]
pub struct MenuItem<'a> {
    node: Node,
    label: TextInput<'a>,
    shortcut: Option<Shortcut>,
    intercept_shortcut: bool,
}

impl<'a> MenuItem<'a> {
    #[track_caller]
    pub fn new(label: impl Into<TextInput<'a>>) -> Self {
        let mut node = Node::hstack();
        node.flags.set_sense(Sense::CLICK);
        Self {
            node,
            label: label.into(),
            shortcut: None,
            intercept_shortcut: false,
        }
    }

    /// Attach a keyboard shortcut. Renders the right-aligned hint
    /// using the platform's native form (`⌘C` / `Ctrl+C`) and
    /// intercepts that keypress while the menu is open. Glyph-only
    /// hints (no modifier, e.g. `Backspace → ⌫`) are expressed as
    /// `Shortcut::new(Mods::NONE, Key::Backspace)`.
    pub fn shortcut(mut self, s: Shortcut) -> Self {
        self.shortcut = Some(s);
        self.intercept_shortcut = true;
        self
    }

    pub(crate) fn shortcut_hint(mut self, shortcut: Shortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn enabled(self, e: bool) -> Self {
        self.disabled(!e)
    }

    /// Thin horizontal divider — no label, no input. Free function in
    /// disguise: chain `.show(ui)` and ignore the response. The
    /// [`crate::Separator`] widget, colored by
    /// `theme.context_menu.separator` and given a little breathing room.
    #[track_caller]
    pub fn separator(ui: &mut Ui) -> Response<'_> {
        Separator::horizontal()
            .color(ui.theme.context_menu.separator)
            .margin(Spacing::xy(0.0, 4.0))
            .show(ui)
    }

    pub fn show<'ui>(self, ui: &'ui mut Ui, popup: &PopupHandle<'_>) -> Response<'ui> {
        // Single `response_for` probe via the shared entry helper: the
        // row's body records only decorative `Text` leaves, so the state
        // is identical before and after the node records.
        let mut entry = enter_widget(ui, self.node);
        let id = entry.widget.id();
        let disabled = entry.state.disabled;

        // Borrow the per-item theme and copy out only what the row paints
        // — avoids cloning the whole `MenuItemTheme` (three looks) per
        // row, per frame. `pick` returns a borrow, so read everything off
        // it before the borrow ends.
        let item = &ui.theme.context_menu.item;
        let look = item.pick(&entry.state);
        let look_bg = look.background.clone();
        let text_style = look.text.as_ref().unwrap_or(&ui.theme.text).clone();
        let padding = item.padding;
        // Shortcut hint reads muted — same style as the label but the
        // theme's `shortcut` color.
        let shortcut_style = TextStyle {
            color: item.shortcut,
            ..text_style.clone()
        };

        let node = &mut entry.widget.node;
        // Hug+Stretch+SpaceBetween: row hugs content (the default
        // `Sizes` — respects an explicit `.size(...)`), arrange
        // stretches to widest row, label/shortcut pin to opposite
        // edges. Fill would leak INF.
        node.align = Align::h(HAlign::Stretch);
        node.justify = Justify::SpaceBetween;
        node.padding = Some(padding);
        node.gaps.set_gap(16.0);

        let label = ui.intern(self.label);
        let shortcut = self.shortcut;
        // Shortcut intercept: while the menu is open, a matching
        // keypress synthesizes a click and closes the menu. Resolved
        // before the node records so we don't pay for the label
        // resolution on rows with no shortcut.
        let shortcut_fired = shortcut.is_some_and(|s| {
            if self.intercept_shortcut {
                !disabled && popup.key_pressed(ui, s)
            } else {
                ui.subscribe_key(s);
                false
            }
        });
        let shortcut_label = shortcut.map(|s| ui.fmt(format_args!("{s}")));

        // Label + optional right-aligned shortcut hint as `Text` leaves;
        // the row's `SpaceBetween` pins them to opposite edges. Both
        // hug their content (Text defaults to `Hug × Hug` and a
        // `SingleLine` wrap), matching what the row layout expects.
        let body = |ui: &mut Ui| {
            Text::new(label)
                .id(id.with("label"))
                .style(&text_style)
                .show(ui);
            if let Some(s) = shortcut_label {
                Text::new(s)
                    .id(id.with("shortcut"))
                    .style(&shortcut_style)
                    .show(ui);
            }
        };
        entry.widget.record(ui, look_bg.as_ref(), body);

        if shortcut_fired {
            entry.state.left.phase = ButtonPhase::Up { click: Some(1) };
        }
        // Eager: `state` folds in the synthesized shortcut click, which
        // a lazy re-probe would drop.
        let resp = entry.into_response(ui);
        if resp.left.clicked() {
            popup.close();
        }
        resp
    }
}

impl Configure for MenuItem<'_> {
    fn node_mut(&mut self) -> ConfigureNode<'_> {
        self.node.node_mut()
    }
}

#[cfg(test)]
mod tests;
