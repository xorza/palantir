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
use crate::widgets::theme::context_menu::{ContextMenuTheme, MenuItemTheme, MenuSeparatorTheme};
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::{Response, ResponseSnapshot, enter_widget};

use crate::primitives::interned_str::TextInput;
use glam::Vec2;

/// Cross-frame state for one context-menu site, keyed off the trigger
/// widget's id in [`crate::ui::state::StateMap`]. `anchor = Some` is
/// the single source of truth for "menu open".
#[derive(Default, Clone, Copy, Debug)]
struct ContextMenuState {
    anchor: Option<Vec2>,
}

/// A right-click / programmatically-opened popup menu attached to a
/// trigger widget. State lives in `StateMap` keyed off the trigger
/// id, so opening / dismissing survives across frames without the
/// caller threading a flag.
///
/// Typical usage chains [`Self::attach`] off a trigger's `Response`,
/// which auto-opens at the pointer on a right-click (`right.clicked()`):
///
/// ```
/// # use palantir::{Button, ContextMenu, MenuItem, Ui};
/// # fn demo(ui: &mut Ui) {
/// let trigger = Button::new().label("…").show(ui).snapshot();
/// ContextMenu::attach(ui, &trigger)
///     .max_size((280.0, 400.0))
///     .show(ui, |ui, popup| { MenuItem::new("Delete").show(ui, popup); });
/// # }
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
/// `min_size.w`, `gap`). Identity and input behavior remain owned by the trigger.
///
/// [`Self::style`] swaps the whole [`ContextMenuTheme`] for one instance.
/// It restyles the *panel* only — the rows are recorded by the caller's
/// body closure, so pass the matching sub-themes down to them
/// ([`MenuItem::style`], [`MenuSeparator::style`]).
#[derive(Debug)]
pub struct ContextMenu<'a> {
    for_id: WidgetId,
    node: Node,
    chrome: Option<Background>,
    gap: Option<f32>,
    style: Option<&'a ContextMenuTheme>,
}

impl<'a> ContextMenu<'a> {
    pub fn for_id(for_id: WidgetId) -> Self {
        let mut node = Node::vstack();
        node.flags.set_sense(Sense::CLICK);
        Self {
            for_id,
            node,
            chrome: None,
            gap: None,
            style: None,
        }
    }

    /// Per-instance theme override. `None` (the default) reads
    /// [`crate::Theme::context_menu`].
    pub fn style(mut self, s: &'a ContextMenuTheme) -> Self {
        self.style = Some(s);
        self
    }

    /// Paint chrome (fill / stroke / corner radius / shadow). `None`
    /// is the default; theme fallback in [`Self::show`] fills it in
    /// from the resolved theme's `panel` when unset. Pass
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

    /// Vertical gutter between rows. Unset inherits the resolved
    /// theme's `gap`.
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = Some(gap);
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
        body: impl FnOnce(&mut Ui, &PopupHandle),
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

        // Borrow the sub-theme to copy out the three scalars and the single
        // `Background` we keep — avoids cloning the whole `ContextMenuTheme`
        // (including the unused per-item looks) every open frame.
        let ctx = self.style.unwrap_or(&ui.theme.context_menu);
        let theme_padding = ctx.padding;
        let theme_min_width = ctx.min_width;
        let gap = self.gap.unwrap_or(ctx.gap);
        let panel = self.chrome.unwrap_or_else(|| ctx.panel.clone());

        let mut e = self.node.id(body_id);
        e.padding.get_or_insert(theme_padding);
        e.min_size.get_or_insert(Size::new(theme_min_width, 0.0));
        e.gaps.set_gap(gap);

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
    shortcut: MenuShortcut,
    style: Option<&'a MenuItemTheme>,
}

#[derive(Clone, Copy, Debug)]
enum MenuShortcut {
    None,
    Hint(Shortcut),
    Activate(Shortcut),
}

impl<'a> MenuItem<'a> {
    #[track_caller]
    pub fn new(label: impl Into<TextInput<'a>>) -> Self {
        let mut node = Node::hstack();
        node.flags.set_sense(Sense::CLICK);
        Self {
            node,
            label: label.into(),
            shortcut: MenuShortcut::None,
            style: None,
        }
    }

    /// Per-instance theme override. `None` (the default) reads
    /// [`crate::Theme::context_menu`]'s `item`.
    pub fn style(mut self, s: &'a MenuItemTheme) -> Self {
        self.style = Some(s);
        self
    }

    /// Attach a keyboard shortcut. Renders the right-aligned hint
    /// using the platform's native form (`⌘C` / `Ctrl+C`) and
    /// intercepts that keypress while the menu is open. Glyph-only
    /// hints (no modifier, e.g. `Backspace → ⌫`) are expressed as
    /// `Shortcut::new(Mods::NONE, Key::Backspace)`.
    pub fn shortcut(mut self, s: Shortcut) -> Self {
        self.shortcut = MenuShortcut::Activate(s);
        self
    }

    pub(crate) fn shortcut_hint(mut self, shortcut: Shortcut) -> Self {
        self.shortcut = MenuShortcut::Hint(shortcut);
        self
    }

    pub fn enabled(self, e: bool) -> Self {
        self.disabled(!e)
    }

    /// Thin horizontal divider between groups — no label, no input.
    /// Chain `.show(ui)` and ignore the response. See
    /// [`MenuSeparator`].
    pub fn separator<'s>() -> MenuSeparator<'s> {
        MenuSeparator { style: None }
    }

    pub fn show<'ui>(self, ui: &'ui mut Ui, popup: &PopupHandle) -> Response<'ui> {
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
        let item = self.style.unwrap_or(&ui.theme.context_menu.item);
        let look = item.pick(&entry.state);
        let look_bg = look.background.clone();
        let text_style = look.text.as_ref().unwrap_or(&ui.theme.text).clone();
        let padding = item.padding;
        let gap = item.gap;
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
        node.gaps.set_gap(gap);

        let label = ui.intern(self.label);
        // Passive hints watch for wake-up while their parent owns dispatch.
        let mut shortcut_fired = false;
        let shortcut = match self.shortcut {
            MenuShortcut::None => None,
            MenuShortcut::Hint(shortcut) => {
                ui.watch_key(shortcut);
                Some(shortcut)
            }
            MenuShortcut::Activate(shortcut) => {
                shortcut_fired = !disabled && popup.key_pressed(ui, shortcut);
                Some(shortcut)
            }
        };
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

/// The rule [`MenuItem::separator`] records between menu groups: a
/// [`crate::Separator`] wearing [`MenuSeparatorTheme`] instead of the
/// app-wide `theme.separator`.
///
/// ```
/// # use palantir::{MenuItem, Ui};
/// # fn demo(ui: &mut Ui) {
/// MenuItem::separator().show(ui);
/// # }
/// ```
#[derive(Debug)]
pub struct MenuSeparator<'a> {
    style: Option<&'a MenuSeparatorTheme>,
}

impl<'a> MenuSeparator<'a> {
    /// Per-instance theme override. `None` (the default) reads
    /// [`crate::Theme::context_menu`]'s `separator`.
    pub fn style(mut self, s: &'a MenuSeparatorTheme) -> Self {
        self.style = Some(s);
        self
    }

    #[track_caller]
    pub fn show<'ui>(self, ui: &'ui mut Ui) -> Response<'ui> {
        let sep = self.style.unwrap_or(&ui.theme.context_menu.separator);
        let color = sep.color;
        let thickness = sep.thickness;
        let margin = sep.margin;
        Separator::horizontal()
            .color(color)
            .thickness(thickness)
            .margin(margin)
            .show(ui)
    }
}

#[cfg(test)]
mod tests;
