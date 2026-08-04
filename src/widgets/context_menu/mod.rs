use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::align::{Align, HAlign};
use crate::layout::types::justify::Justify;
use crate::primitives::background::Background;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::ThemeDefaults;
use crate::scene::node::{Configure, ConfigureNode, Node};
use crate::ui::Ui;
use crate::widgets::popup::{ClickOutside, Popup, PopupHandle};
use crate::widgets::response::{Response, ResponseSnapshot};
use crate::widgets::separator::Separator;
use crate::widgets::text::Text;
use crate::widgets::theme::WidgetTheme;
use crate::widgets::theme::context_menu::ContextMenuTheme;
use crate::widgets::theme::context_menu::menu_item::MenuItemTheme;
use crate::widgets::theme::separator::SeparatorTheme;
use crate::widgets::theme::text_style::TextStyle;

use crate::primitives::interned_str::TextInput;
use glam::Vec2;

/// Cross-frame response for one context-menu site, keyed off the trigger
/// widget's id in [`crate::ui::response::StateMap`]. `anchor = Some` is
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
/// # use palantir::{Button, ContextMenu, Configure, MenuItem, Ui};
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
    /// The popup this menu *is*. It owns the body node from the start,
    /// so the caller's [`Configure`] calls land on the node that
    /// actually records — there is no second node to keep in sync or
    /// swap in at `show`. Its anchor is a placeholder until `show`
    /// re-places it (see [`Popup::anchored_at`]); a closed menu returns
    /// before recording, so the placeholder never places anything.
    popup: Popup,
    chrome: Option<Background>,
    style: Option<&'a ContextMenuTheme>,
}

impl<'a> ContextMenu<'a> {
    pub fn for_id(for_id: WidgetId) -> Self {
        Self {
            for_id,
            popup: Popup::anchored_to(Vec2::ZERO).click_outside(ClickOutside::Dismiss),
            chrome: None,
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

        let body_id = self.for_id.with("body");

        // Borrow the sub-theme to copy out the three scalars and the single
        // `Background` we keep — avoids cloning the whole `ContextMenuTheme`
        // (including the unused per-item looks) every open frame.
        let ctx = self.style.unwrap_or(&ui.theme.context_menu);
        let theme_padding = ctx.padding;
        let theme_min_width = ctx.min_width;
        let panel = self.chrome.unwrap_or_else(|| ctx.panel.clone());

        // The menu is the popup, configured: the caller's `Configure`
        // calls already landed on it, the menu theme fills in whatever
        // they left alone, and `Popup::show` resolves the result against
        // the surface. Identity falls back to the trigger's — a menu has
        // no call site of its own worth keying on.
        let resp = self
            .popup
            .anchored_at(raw_anchor)
            .background(panel)
            .default_id(body_id)
            .default_padding(theme_padding)
            .default_min_size(Size::new(theme_min_width, 0.0))
            .default_gap(ctx.gap)
            .show(ui, body);
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
        if let Some(response) = ui.try_state_mut::<ContextMenuState>(for_id) {
            response.anchor = None;
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

/// Forwards to the popup this menu wraps, so `.size(...)` /
/// `.padding(...)` / `.id(...)` configure the node that actually
/// records — the menu keeps no node of its own.
impl Configure for ContextMenu<'_> {
    fn node_mut(&mut self) -> ConfigureNode<'_> {
        self.popup.node_mut()
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
/// auto-closes without the caller threading response.
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
        // row's body records only decorative `Text` leaves, so the response
        // is identical before and after the node records.
        let mut widget = ui.widget(self.node);
        let mut response = widget.response(ui);
        let id = widget.id();
        let disabled = response.disabled;

        // Row-only scalars first, off a borrow that ends before
        // `WidgetTheme::resolve` reborrows `ui` mutably. Everything response-varying
        // — the four-response pick, the padding/margin defaults, the
        // transition — comes back from the shared resolver instead, so a
        // menu row picks and animates exactly like a Button.
        let item = self.style.unwrap_or(&ui.theme.context_menu.item);
        let shortcut_color = item.shortcut;
        let gap = item.gap;

        let look = WidgetTheme::resolve(ui, id, &mut widget.node, &response, (), self.style, |t| {
            &t.context_menu.item
        });
        // Already fallen back to `theme.text` by `WidgetLook::animate`.
        let text_style = look.text;
        // Shortcut hint reads muted — same style as the label but the
        // theme's `shortcut` color.
        let shortcut_style = TextStyle {
            color: shortcut_color,
            ..text_style.clone()
        };

        let node = &mut widget.node;
        // Hug+Stretch+SpaceBetween: row hugs content (the default
        // `Sizes` — respects an explicit `.size(...)`), arrange
        // stretches to widest row, label/shortcut pin to opposite
        // edges. Fill would leak INF.
        node.align = Align::h(HAlign::Stretch);
        node.justify = Justify::SpaceBetween;
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
                shortcut_fired = !disabled && ui.key_pressed(shortcut);
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
        widget.record(ui, Some(&look.background), body);

        if shortcut_fired {
            response.mark_clicked();
        }
        // Eager: `response` folds in the synthesized shortcut click, which
        // a lazy re-probe would drop.
        let resp = Response::eager(id, ui, response);
        if resp.left.clicked() {
            popup.close();
        }
        resp
    }
}

impl_configure!(MenuItem<'_>);

/// The rule [`MenuItem::separator`] records between menu groups: a
/// [`crate::Separator`] wearing [`crate::Theme::context_menu`]'s
/// `separator` slot instead of the app-wide `theme.separator`.
///
/// A menu rule *is* a separator — the two differ only in which
/// [`SeparatorTheme`] they read, so this hands the bundle straight down
/// rather than unpacking it field by field.
///
/// ```
/// # use palantir::{MenuItem, Ui};
/// # fn demo(ui: &mut Ui) {
/// MenuItem::separator().show(ui);
/// # }
/// ```
#[derive(Debug)]
pub struct MenuSeparator<'a> {
    style: Option<&'a SeparatorTheme>,
}

impl<'a> MenuSeparator<'a> {
    /// Per-instance theme override. `None` (the default) reads
    /// [`crate::Theme::context_menu`]'s `separator`.
    pub fn style(mut self, s: &'a SeparatorTheme) -> Self {
        self.style = Some(s);
        self
    }

    #[track_caller]
    pub fn show<'ui>(self, ui: &'ui mut Ui) -> Response<'ui> {
        let sep = self.style.unwrap_or(&ui.theme.context_menu.separator);
        // Cloned, not borrowed: `Separator::style` holds the reference
        // across `show`'s `&mut Ui`, and this one may point into
        // `ui.theme`.
        let sep = sep.clone();
        Separator::horizontal().style(&sep).show(ui)
    }
}

#[cfg(test)]
mod tests;
