//! The chip row on its own: geometry, selection cap, close button, overflow, keyboard travel, drag sensing.

use crate::input::interaction::response_state::ResponseState;
use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::math::domain::EPS;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::layer::Layer;
use crate::shape::Shape;
use crate::shape::style::LineCap;
use crate::theme::close_shown::CloseShown;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::tab_selection::TabSelection;
use crate::theme::tab_strip_focus::TabStripFocus;
use crate::theme::tabs::TabsTheme;
use crate::theme::text_style::TextStyle;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::{Response, ResponseSnapshot};
use crate::widget_core::widget::Widget;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::popup::popup_trigger::PopupTrigger;
use crate::widgets::scroll::Scroll;
use crate::widgets::tabs::tab_chip::TabChip;
use crate::widgets::tabs::tab_item::TabItem;
use glam::Vec2;
use std::rc::Rc;

/// What a strip does with more chips than fit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabOverflow {
    /// Chips pan under the wheel. The default; no chrome of its own.
    #[default]
    Scroll,
    /// [`Self::Scroll`] plus a trailing chevron listing every chip. Read [`TabStripResponse::menu_picked`] beside [`TabStripResponse::clicked`], or the menu closes on a choice that does nothing.
    Menu,
}

/// What one pass over a strip found: the chip acted on, by slot. Every field is a one-frame edge.
///
/// Activation is reported by source: a caller that polls chips a phase earlier (the dock's navigation scan) acts on [`Self::keyed`] and [`Self::menu_picked`]; everyone else on [`Self::activated`].
#[derive(Debug)]
pub struct TabStripResponse<'a> {
    /// The strip's own [`Response`].
    pub response: Response<'a>,
    /// The chip a click activated.
    pub clicked: Option<usize>,
    /// The chip a keyboard move activated.
    pub keyed: Option<usize>,
    /// The chip chosen from the overflow menu.
    pub menu_picked: Option<usize>,
    /// The chip whose close button was clicked; wins over `clicked`.
    pub closed: Option<usize>,
    /// The chip whose drag latched this frame.
    pub drag_started: Option<usize>,
    /// The chip whose drag released this frame.
    pub drag_stopped: Option<usize>,
}

/// A row of tab chips, the shared half of [`TabbedView`](crate::TabbedView) and [`DockView`](crate::DockView) (same [`TabsTheme`], chip ids and overflow). Alone it is a plain segmented selector.
///
/// ```
/// # use palantir::{TabItem, TabStrip, Ui};
/// # fn demo(ui: &mut Ui, items: &[TabItem], active: usize) {
/// let hit = TabStrip::new(items).selected(active).show(ui, |ui, chip| chip.show_label(ui));
/// if let Some(i) = hit.activated() {
///     // activate items[i]
/// }
/// # }
/// ```
///
/// **Chip ids come from [`TabItem::key`], never the slot.** Derive them outside `show` with [`Self::chip_id`] and [`Self::close_id`].
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct TabStrip<'a> {
    widget: Widget,
    items: &'a [TabItem],
    selected: Option<usize>,
    focus: TabStripFocus,
    overflow: TabOverflow,
    drop_caret: Option<usize>,
    style: Option<&'a TabsTheme>,
}

impl TabStripResponse<'_> {
    /// The chip activated this frame by click, keyboard move, or overflow-menu pick.
    pub fn activated(&self) -> Option<usize> {
        self.clicked.or(self.keyed).or(self.menu_picked)
    }
}

impl<'a> TabStrip<'a> {
    /// A strip over `items`. Writes nothing back; outcomes arrive through [`TabStripResponse`].
    #[track_caller]
    pub fn new(items: &'a [TabItem]) -> Self {
        Self {
            // Focusable so a press lands the keyboard here; the scope takes only what `keyboard_travel` acts on, so bare Tab reaches the app's traversal.
            widget: Widget::vstack()
                .size((Sizing::FILL, Sizing::HUG))
                .focusable(true)
                .input_scope(KeyFilter::CARET | KeyFilter::CYCLE),
            items,
            selected: None,
            focus: TabStripFocus::Focused,
            overflow: TabOverflow::default(),
            drop_caret: None,
            style: None,
        }
    }

    /// Which chip wears the selection cap; unset caps nothing. An index past the end is coerced to the last chip.
    pub fn selected(mut self, selected: impl Into<Option<usize>>) -> Self {
        self.selected = selected.into();
        self
    }

    /// Whether this strip's view holds focus; `false` dims the cap to [`TabsTheme::cap_unfocused`]. Default `true`.
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focus = if focused {
            TabStripFocus::Focused
        } else {
            TabStripFocus::Unfocused
        };
        self
    }

    /// What the strip does with chips that do not fit. Default [`TabOverflow::Scroll`].
    pub const fn overflow(mut self, overflow: TabOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `tabs`.
    pub fn style(mut self, s: impl Into<Option<&'a TabsTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Mark the slot a carried chip would drop into, from [`Self::drop_slot`]; a slot past the
    /// end marks the append.
    pub(crate) const fn drop_caret(mut self, slot: Option<usize>) -> Self {
        self.drop_caret = slot;
        self
    }

    /// The chip id for `key` under a strip with id `strip`; the one derivation.
    pub fn chip_id(strip: WidgetId, key: u64) -> WidgetId {
        strip.with(("tab", key))
    }

    /// The close-button id for `key`. See [`Self::chip_id`].
    pub fn close_id(strip: WidgetId, key: u64) -> WidgetId {
        strip.with(("tab_close", key))
    }

    /// The scrolling band the chips pan inside: the clipped viewport the overflow chevron measures against.
    fn band_id(strip: WidgetId) -> WidgetId {
        strip.with("band")
    }

    /// What the chips keyed `keys` under `strip` reported last frame, read before the strip records,
    /// as a caller that acts a phase early does. The same fold as [`TabStripResponse`]'s.
    pub(crate) fn scan(ui: &Ui, strip: WidgetId, keys: impl IntoIterator<Item = u64>) -> ChipHits {
        let mut hits = ChipHits::default();
        for (slot, key) in keys.into_iter().enumerate() {
            let close = ui.response_for(Self::close_id(strip, key));
            hits.add(
                slot,
                &ui.response_for(Self::chip_id(strip, key)),
                Some(&close),
            );
        }
        hits
    }

    /// Where the chip keyed `key` would be uncut: its arranged rect under its transform, so a chip
    /// the band pans out of sight keeps its whole extent.
    fn chip_extent(ui: &Ui, strip: WidgetId, key: u64) -> Option<Rect> {
        let chip = ui.response_for(Self::chip_id(strip, key));
        chip.layout_rect.map(|rect| chip.transform.apply_rect(rect))
    }

    /// The slot a drop at `x` addresses among the chips keyed `keys`, from last frame's uncut
    /// extents: a chip the band clips still counts at its true center.
    pub(crate) fn drop_slot(
        ui: &Ui,
        strip: WidgetId,
        keys: impl IntoIterator<Item = u64>,
        x: f32,
    ) -> usize {
        insertion_slot(
            keys.into_iter()
                .filter_map(|key| Self::chip_extent(ui, strip, key)),
            x,
        )
    }

    /// Records the strip, and its overflow menu when open. `content` records what each chip shows
    /// between its padding and its close button, [`TabChip::show_label`] for the name alone: the
    /// app draws a tab, the strip keeps its frame, as Zed's `Item::tab_content` splits it.
    pub fn show(
        self,
        ui: &mut Ui,
        mut content: impl FnMut(&mut Ui, &TabChip),
    ) -> TabStripResponse<'_> {
        let theme = Rc::clone(ui.theme());
        let t = self.style.unwrap_or(&theme.tabs);
        let ambient = theme.text;
        let Self {
            mut widget,
            items,
            selected,
            focus,
            overflow,
            drop_caret,
            style: _,
        } = self;
        let selected = selected.and_then(|i| domain::idx(i, items.len()));
        let response = widget.response(ui);
        let id = response.id;
        let strip_chrome = t.strip.clone();
        let rule = Chrome::fill(t.rule);
        let rule_thickness = t.rule_thickness;

        let mut pass = StripPass {
            theme: t,
            ambient,
            strip: id,
            items,
            selected,
            focus,
            hits: StripHits::default(),
        };
        widget.record(ui, Some(&strip_chrome), |ui| {
            let row = Widget::hstack()
                .id(id.with("row"))
                .size((Sizing::FILL, Sizing::HUG))
                .gap(t.gap)
                .child_align(Align::v(VAlign::Bottom));
            let reveal = pass.reveal_delta(ui);
            row.record(ui, None, |ui| {
                Scroll::horizontal()
                    .id(Self::band_id(id))
                    .size((Sizing::FILL, Sizing::HUG))
                    .hide_bars()
                    .pan_by(reveal)
                    .padding(t.strip_padding)
                    .gap(t.gap)
                    .child_align(Align::v(VAlign::Bottom))
                    .show(ui, |ui| {
                        for slot in 0..items.len() {
                            pass.chip(ui, slot, &mut content);
                        }
                    });
                if overflow == TabOverflow::Menu {
                    pass.overflow_menu(ui);
                }
            });
            if rule_thickness > 0.0 {
                let rule_leaf = Widget::leaf()
                    .id(id.with("rule"))
                    .size((Sizing::FILL, Sizing::fixed(rule_thickness)));
                rule_leaf.record(ui, Some(&rule), |_| {});
            }
            if let Some(slot) = drop_caret {
                pass.caret(ui, slot);
            }
            pass.keyboard_travel(ui);
        });
        let hits = pass.hits;
        if let Some(slot) = hits.keyed.or(hits.menu_picked) {
            ui.with_state::<StripState, _>(id, |_, s| s.reveal = Some(items[slot].key));
        }

        let StripHits {
            chips:
                ChipHits {
                    clicked,
                    closed,
                    drag_started,
                    drag_stopped,
                },
            keyed,
            menu_picked,
        } = hits;
        TabStripResponse {
            response: Response::new(ui, response),
            clicked,
            keyed,
            menu_picked,
            closed,
            drag_started,
            drag_stopped,
        }
    }
}

impl Configure for TabStrip<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// A chip a keyboard move or menu pick selected, which the next frame pans into sight.
#[derive(Debug, Default)]
struct StripState {
    reveal: Option<u64>,
}

/// The slot an insertion at `x` addresses: the count of chips whose center it has passed; `chips.len()` appends.
fn insertion_slot(chips: impl IntoIterator<Item = Rect>, x: f32) -> usize {
    chips.into_iter().filter(|c| c.center().x < x).count()
}

/// What a strip's chips reported, by slot. Every field is a one-frame edge, and the first chip to
/// report an edge keeps it.
#[derive(Debug, Default)]
pub(crate) struct ChipHits {
    /// The chip a click activated.
    pub(crate) clicked: Option<usize>,
    /// The chip whose close button was clicked.
    pub(crate) closed: Option<usize>,
    /// The chip whose drag latched.
    pub(crate) drag_started: Option<usize>,
    /// The chip whose drag released.
    pub(crate) drag_stopped: Option<usize>,
}

impl ChipHits {
    /// Fold in the chip at `slot`. The close button wins over the click the same press reads as.
    fn add(&mut self, slot: usize, chip: &ResponseState, close: Option<&ResponseState>) {
        if close.is_some_and(ResponseState::clicked) {
            self.closed.get_or_insert(slot);
        } else if chip.clicked() {
            self.clicked.get_or_insert(slot);
        }
        if chip.left.drag.started() {
            self.drag_started.get_or_insert(slot);
        }
        if chip.left.drag.stopped() {
            self.drag_stopped.get_or_insert(slot);
        }
    }
}

#[derive(Debug, Default)]
struct StripHits {
    chips: ChipHits,
    keyed: Option<usize>,
    menu_picked: Option<usize>,
}

/// One record of a strip: what every chip reads, and what the chips and keys did.
#[derive(Debug)]
struct StripPass<'a> {
    theme: &'a TabsTheme,
    ambient: TextStyle,
    strip: WidgetId,
    items: &'a [TabItem],
    selected: Option<usize>,
    focus: TabStripFocus,
    hits: StripHits,
}

impl StripPass<'_> {
    /// The part of the band a chip can be seen in: its rect clipped by ancestors and deflated by the band's padding.
    fn band_clip(&self, ui: &Ui) -> Option<Rect> {
        let band = ui.response_for(TabStrip::band_id(self.strip)).rect?;
        Some(band.deflated_by(self.theme.strip_padding))
    }

    /// The pan bringing the selected chip into the band's clip, from last frame's rects; forgets the request.
    fn reveal_delta(&self, ui: &mut Ui) -> Vec2 {
        let strip = self.strip;
        let Some(key) = ui.state::<StripState>(strip).and_then(|state| state.reveal) else {
            return Vec2::ZERO;
        };
        ui.with_state::<StripState, _>(strip, |_, s| s.reveal = None);
        let (Some(clip), Some(chip)) = (self.band_clip(ui), TabStrip::chip_extent(ui, strip, key))
        else {
            return Vec2::ZERO;
        };
        if chip.min.x < clip.min.x {
            Vec2::new(chip.min.x - clip.min.x, 0.0)
        } else if chip.max().x > clip.max().x {
            Vec2::new(chip.max().x - clip.max().x, 0.0)
        } else {
            Vec2::ZERO
        }
    }

    /// A small glyph button wearing [`TabsTheme::close`], from its last frame's state.
    fn glyph_button(&self, ui: &Ui, id: WidgetId) -> GlyphButton {
        let response = ResponseSnapshot {
            id,
            state: ui.response_for(id),
        };
        let look = self.theme.close.pick(&response, Engagement::Pressed);
        GlyphButton {
            chrome: look.chrome.clone(),
            ink: look.text.apply(&self.ambient).color,
            response,
        }
    }

    /// The chip at `slot`: cap-bearing outer box, filled inner box, and the content and close button inside.
    fn chip(&mut self, ui: &mut Ui, slot: usize, content: &mut impl FnMut(&mut Ui, &TabChip)) {
        let t = self.theme;
        let item = self.items[slot];
        let selected = self.selected == Some(slot);
        let chip_id = TabStrip::chip_id(self.strip, item.key);
        let close_id = TabStrip::close_id(self.strip, item.key);
        let sense = if item.draggable {
            Sense::CLICK | Sense::DRAG
        } else {
            Sense::CLICK
        };
        let cap = if selected { t.cap_thickness } else { 0.0 };
        let mut widget = Widget::hstack()
            .id(chip_id)
            .size((Sizing::HUG, Sizing::HUG))
            .min_size(Size::new(t.min_width, 0.0))
            .max_size(Size::new(t.max_width, f32::INFINITY))
            .padding(Spacing::new(0.0, cap, 0.0, 0.0))
            .sense(sense);
        let state = widget.response(ui);
        // The look paints the inner box, so the widget wears the plan for spacing and animation while the chrome goes a level down (the `ToggleRow` shape).
        let plan = t.plan(&state, TabSelection::from(selected), self.ambient);
        let look = widget.wear(ui, plan);
        let cap_chrome = if selected {
            Chrome::rounded(t.cap(self.focus), Corners::top(t.radius))
        } else {
            Chrome::NONE
        };
        let inner_chrome = Chrome {
            corners: Corners::top((t.radius - cap).max(0.0)),
            ..look.chrome
        };
        // The selected chip lifts its inner top inset by the cap, so height is unchanged; a close button trades the right inset for its box (`TabsTheme::trailing_inset`).
        let [pad_l, pad_t, pad_r, pad_b] = t.chip_padding.to_array();
        let trailing = if item.closable {
            t.trailing_inset
        } else {
            pad_r
        };
        let padding = Spacing::new(pad_l, (pad_t - cap).max(0.0), trailing, pad_b);

        let chip = TabChip {
            slot,
            key: item.key,
            label: item.label,
            selected,
            text: look.text,
            id: chip_id,
        };
        let closable = item.closable;
        // A chip that hides its button keeps the button's box, so the chip's width does not change
        // as the button comes and goes; the box senses nothing, so the pointer there reaches the
        // chip, which then shows the button.
        let close_shown = match t.close_shown {
            CloseShown::Always => true,
            // Within, not on: the pointer on the button or on a widget of the content's
            // still holds the chip.
            CloseShown::SelectedOrHovered => selected || ui.is_hover_within(chip_id),
        };
        let close = (closable && close_shown).then(|| self.glyph_button(ui, close_id));
        let close_slot = closable && !close_shown;

        // Every chip is at least as tall as a close button, closable or not, so the strip keeps
        // its height as a closable tab comes and goes beside a pinned one.
        let inner = Widget::hstack()
            .id(chip_id.with("fill"))
            .size((Sizing::HUG, Sizing::HUG))
            .min_size(Size::new(0.0, t.close_size + padding.sums().h))
            .padding(padding)
            .gap(t.label_gap)
            .child_align(Align::v(VAlign::Center));
        widget.record(ui, Some(&cap_chrome), |ui| {
            inner.record(ui, Some(&inner_chrome), |ui| {
                content(ui, &chip);
                if let Some(close) = &close {
                    let button = Widget::zstack()
                        .id(close_id)
                        .size((Sizing::fixed(t.close_size), Sizing::fixed(t.close_size)))
                        .sense(Sense::CLICK)
                        .align(Align::v(VAlign::Center))
                        .child_align(Align::CENTER);
                    button.record(ui, Some(&close.chrome), |ui| {
                        close.cross(ui, t.close_size, t.close_glyph, t.close_stroke);
                    });
                }
                if close_slot {
                    Widget::leaf()
                        .id(close_id.with("slot"))
                        .size((Sizing::fixed(t.close_size), Sizing::fixed(t.close_size)))
                        .record(ui, None, |_| {});
                }
            });
        });

        self.hits
            .chips
            .add(slot, &state, close.as_ref().map(|c| &c.response.state));
    }

    /// The insertion mark at the boundary of `slot`: centered in the gutter before its chip, or
    /// after the last chip for the append, and held inside the band where that boundary is panned
    /// out of sight. Recorded in the tooltip layer, over whatever the drag crosses.
    fn caret(&self, ui: &mut Ui, slot: usize) {
        let t = self.theme;
        let strip = self.strip;
        let Some(clip) = self.band_clip(ui) else {
            return;
        };
        let extent = |slot: usize| {
            let item = self.items.get(slot)?;
            TabStrip::chip_extent(ui, strip, item.key)
        };
        let slot = slot.min(self.items.len());
        let half_gap = domain::length_at_least(t.gap, 0.0) * 0.5;
        let x = match (extent(slot), slot.checked_sub(1).and_then(extent)) {
            (Some(next), _) => next.min.x - half_gap,
            (None, Some(prev)) => prev.max().x + half_gap,
            (None, None) => clip.min.x,
        };
        let width = domain::length_at_least(t.caret_width, 0.0);
        let x = x.clamp(clip.min.x, clip.max().x.max(clip.min.x));
        let mark = Rect::new(x - width * 0.5, clip.min.y, width, clip.size.h.max(0.0));
        let fill = Chrome::fill(t.caret);
        ui.layer(Layer::Tooltip)
            .fixed_at(mark.min)
            .max_size(mark.size)
            .show(|ui| {
                Widget::leaf()
                    .id(strip.with("caret"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .record(ui, Some(&fill), |_| {});
            });
    }

    /// The trailing chevron and its menu, recorded only while a chip is scrolled out of the band. The menu lists every chip: visibility comes from last frame's rects, so listing only hidden ones would add and drop rows under the open menu.
    fn overflow_menu(&mut self, ui: &mut Ui) {
        let strip = self.strip;
        let items = self.items;
        // Compared at `EPS` so an exact fit can't flicker the chevron.
        let Some(clip) = self.band_clip(ui) else {
            return;
        };
        let hidden = |item: &TabItem| {
            TabStrip::chip_extent(ui, strip, item.key).is_some_and(|full| {
                full.min.x < clip.min.x - EPS || full.max().x > clip.max().x + EPS
            })
        };
        let button_id = strip.with("overflow");
        if !items.iter().any(hidden) && !PopupTrigger::is_open(ui, button_id) {
            return;
        }
        let chevron = self.glyph_button(ui, button_id);
        let t = self.theme;
        let [_, _, band_r, band_b] = t.strip_padding.to_array();
        let button = Widget::zstack()
            .id(button_id)
            .size((Sizing::fixed(t.close_size), Sizing::fixed(t.close_size)))
            .margin(Spacing::new(0.0, 0.0, band_r, band_b))
            .sense(Sense::CLICK)
            .align(Align::v(VAlign::Bottom))
            .child_align(Align::CENTER);
        button.record(ui, Some(&chevron.chrome), |ui| {
            chevron.ellipsis(ui, t.close_size, t.close_glyph, t.close_stroke);
        });
        let ui_theme = Rc::clone(ui.theme());
        // A menu button: the chevron's click toggles a menu below it.
        let picked = PopupTrigger::on(&chevron.response)
            .size((Sizing::HUG, Sizing::HUG))
            .layer(Layer::Menu)
            .menu_look(&ui_theme.context_menu)
            .show(ui, |ui, popup| {
                let mut picked = None;
                for (i, item) in items.iter().enumerate() {
                    if MenuItem::new(item.label)
                        .id(button_id.with(item.key))
                        .show(ui, popup)
                        .left
                        .clicked()
                    {
                        picked = Some(i);
                    }
                }
                picked
            });
        if let Some(slot) = picked.inner.flatten() {
            self.hits.menu_picked = Some(slot);
        }
    }

    /// Keyboard travel on the WAI-ARIA tab pattern: arrows step, `Home`/`End` jump, `Ctrl+Tab` cycles; each activates what it lands on. Only while focus is in the strip, read in its own record so its scope grants the press.
    fn keyboard_travel(&mut self, ui: &mut Ui) {
        let len = self.items.len();
        let selected = self.selected;
        if len == 0 || !ui.is_focus_within(self.strip) {
            return;
        }
        let step = |forward: bool| match selected.map(|here| here.min(len - 1)) {
            Some(here) if forward => (here + 1) % len,
            Some(here) => (here + len - 1) % len,
            None if forward => 0,
            None => len - 1,
        };
        // Every chord is sampled, not short-circuited, so `shortcut_pressed` keeps each subscribed for the wake gate. Modifier sets match exactly: `Ctrl+Tab` never fires on `Ctrl+Shift+Tab`.
        let back = ui.shortcut_pressed(Key::ArrowLeft);
        let forward = ui.shortcut_pressed(Key::ArrowRight);
        let first = ui.shortcut_pressed(Key::Home);
        let last = ui.shortcut_pressed(Key::End);
        let cycle = ui.shortcut_pressed(Shortcut::new(ShortcutMods::CTRL, Key::Tab));
        let cycle_back = ui.shortcut_pressed(Shortcut::new(ShortcutMods::CTRL_SHIFT, Key::Tab));
        let target = if back || cycle_back {
            Some(step(false))
        } else if forward || cycle {
            Some(step(true))
        } else if first {
            Some(0)
        } else if last {
            Some(len - 1)
        } else {
            None
        };
        if let Some(target) = target
            && Some(target) != selected
        {
            self.hits.keyed = Some(target);
        }
    }
}

/// A small glyph button: a chip's close cross and the overflow chevron, both wearing [`TabsTheme::close`].
///
/// Each glyph is drawn shapes in the look's text color, not a font's "×" or "⋯": a font glyph
/// sits on its baseline at the font's own weight, so it rode below the button's center.
#[derive(Debug)]
struct GlyphButton {
    response: ResponseSnapshot,
    chrome: Chrome,
    ink: RgbaF32,
}

impl GlyphButton {
    /// A cross `extent` wide with `stroke` arms, centered in the `side` box.
    fn cross(&self, ui: &mut Ui, side: f32, extent: f32, stroke: f32) {
        let center = Vec2::splat(side * 0.5);
        let half = extent * 0.5;
        for (from, to) in [
            (center - Vec2::splat(half), center + Vec2::splat(half)),
            (
                center + Vec2::new(half, -half),
                center + Vec2::new(-half, half),
            ),
        ] {
            ui.add_shape(Shape::line(from, to, Stroke::new(self.ink, stroke)).cap(LineCap::Round));
        }
    }

    /// Three dots centered in the `side` box, in the proportions of the cross they sit beside, as
    /// Lucide's and Feather's `ellipsis` sit beside their `x`: where the cross spans 12 grid units
    /// at a 2-unit stroke, the dots are 7 units apart and 4 across, a 1-unit circle stroked 2.
    fn ellipsis(&self, ui: &mut Ui, side: f32, extent: f32, stroke: f32) {
        let pitch = extent * 7.0 / 12.0;
        let diameter = 2.0 * stroke;
        let radius = stroke;
        for step in [-1.0, 0.0, 1.0] {
            let x = side * 0.5 + step * pitch;
            let y = side * 0.5;
            ui.add_shape(
                Shape::rect(Rect::new(x - radius, y - radius, diameter, diameter))
                    .corners(radius)
                    .fill(self.ink),
            );
        }
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::geometry::rect::Rect;

    pub(crate) fn insertion_slot(chips: impl IntoIterator<Item = Rect>, x: f32) -> usize {
        super::insertion_slot(chips, x)
    }
}
