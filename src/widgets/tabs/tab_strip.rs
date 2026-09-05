//! The chip row on its own: geometry, the selection cap, the close
//! button, the badge, overflow, keyboard travel, and drag sensing.

use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::response::response_state::ResponseState;
use crate::input::sense::Sense;
use crate::input::shortcut::{Mods, Shortcut};
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::shape::Shape;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ConfigureWidget;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::response::Response;
use crate::widgets::scroll::Scroll;
use crate::widgets::tabs::tab_item::TabItem;
use crate::widgets::text::Text;
use crate::widgets::theme::tabs::TabsTheme;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::theme_slot::ThemeSlot;
use crate::widgets::widget::Widget;
use glam::Vec2;
use std::rc::Rc;

/// What a strip does with more chips than it has room for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabOverflow {
    /// The chips pan under the wheel. The default: no chrome of its own,
    /// so a strip that never overflows looks exactly as it would
    /// without it.
    #[default]
    Scroll,
    /// [`Self::Scroll`], plus a trailing chevron listing every chip.
    /// Recorded only while at least one chip is out of sight.
    Menu,
}

/// What one pass over a strip found — the chip the user acted on, by
/// slot in the item slice the strip was handed.
///
/// Every field is a one-frame edge, so a caller reads them and acts;
/// nothing latches.
///
/// Pointer and keyboard activation are reported apart. A caller that
/// polls the chips itself one phase earlier — the dock's navigation scan
/// does — already has the click, and acts on [`Self::keyed`] alone;
/// everyone else takes either.
#[derive(Debug)]
pub struct TabStripResponse<'a> {
    pub response: Response<'a>,
    /// The chip a pointer click activated.
    pub clicked: Option<usize>,
    /// The chip a keyboard move activated.
    pub keyed: Option<usize>,
    /// The chip whose close button was clicked. Wins over `clicked` —
    /// the button sits inside the chip, so one press reaches both.
    pub closed: Option<usize>,
    /// The chip whose drag latched this frame.
    pub drag_started: Option<usize>,
    /// The chip whose drag released this frame.
    pub drag_stopped: Option<usize>,
}

/// A row of tab chips, and nothing below it.
///
/// The shared half of [`TabbedView`](crate::TabbedView) and
/// [`DockView`](crate::DockView): both record this widget, with the same
/// [`TabsTheme`], the same chip ids and the same overflow behaviour.
/// Used alone it is a plain segmented selector that draws no content of
/// its own.
///
/// ```
/// # use palantir::{TabItem, TabStrip, Ui};
/// # fn demo(ui: &mut Ui, items: &[TabItem], active: usize) {
/// let hit = TabStrip::new(items).selected(active).show(ui);
/// if let Some(i) = hit.clicked {
///     // activate items[i]
/// }
/// # }
/// ```
///
/// **Chip ids come from [`TabItem::key`], never from the slot.** A
/// caller that reads chips outside `show` — the dock's navigation-phase
/// scan does — derives the same ids through [`Self::chip_id`] and
/// [`Self::close_id`].
#[derive(Debug)]
pub struct TabStrip<'a> {
    widget: Widget,
    items: &'a [TabItem],
    selected: Option<usize>,
    focused: bool,
    overflow: TabOverflow,
    style: Option<&'a TabsTheme>,
}

impl<'a> TabStrip<'a> {
    #[track_caller]
    pub fn new(items: &'a [TabItem]) -> Self {
        Self {
            // Focusable so a press on a chip lands the keyboard here and
            // the arrow keys below have somewhere to travel from; the
            // Motion scope keeps those arrows out of the application
            // while a chip holds focus, and lets every other class walk
            // straight past.
            widget: Widget::vstack()
                .size((Sizing::FILL, Sizing::HUG))
                .focusable(true)
                .input_scope(KeyFilter::MOTION),
            items,
            selected: None,
            focused: true,
            overflow: TabOverflow::default(),
            style: None,
        }
    }

    /// Which chip wears the selection cap. Out of range, or unset, caps
    /// nothing.
    pub fn selected(mut self, selected: impl Into<Option<usize>>) -> Self {
        self.selected = selected.into();
        self
    }

    /// Whether this strip's own view holds the application's focus.
    /// `false` dims the cap to [`TabsTheme::accent_idle`], so one strip
    /// among several reads as the one actions go to. Default `true` — a
    /// lone strip is always the live one.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// What the strip does with chips that do not fit. Default
    /// [`TabOverflow::Scroll`].
    pub fn overflow(mut self, overflow: TabOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `tabs`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    pub fn style(mut self, s: impl Into<Option<&'a TabsTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// The chip id for `key` under a strip recorded with `strip` as its
    /// id. The one derivation, so a caller polling last frame's
    /// responses asks the same question the strip answered.
    pub fn chip_id(strip: WidgetId, key: u64) -> WidgetId {
        strip.with(("tab", key))
    }

    /// The close-button id for `key`. See [`Self::chip_id`].
    pub fn close_id(strip: WidgetId, key: u64) -> WidgetId {
        strip.with(("tab_close", key))
    }

    /// The scrolling band the chips pan inside. The strip's own rect is
    /// what a drop classification measures against; this is the clipped
    /// viewport, which is how the overflow chevron knows a chip is out
    /// of sight.
    fn band_id(strip: WidgetId) -> WidgetId {
        strip.with("band")
    }

    /// The slot an insertion at `x` addresses, over the strip's chip
    /// rects in slot order: the count of chips whose centre it has
    /// passed. `chips.len()` appends.
    ///
    /// Takes an iterator rather than a slice so a caller with the rects
    /// already buffered and one reading them straight out of
    /// [`Ui::response_for`] both reach the same rule without either
    /// allocating for the other's shape.
    pub(crate) fn insertion_slot(chips: impl IntoIterator<Item = Rect>, x: f32) -> usize {
        chips.into_iter().filter(|c| c.center().x < x).count()
    }

    pub fn show(self, ui: &mut Ui) -> TabStripResponse<'_> {
        let theme = Rc::clone(ui.theme());
        let t = self.style.unwrap_or(&theme.tabs);
        let ambient = theme.text;
        let Self {
            mut widget,
            items,
            selected,
            focused,
            overflow,
            style: _,
        } = self;
        let id = widget.resolve(ui);
        let response = widget.response(ui);
        let strip_bg = t.strip.clone();
        let rule = Background::fill(t.hline);
        let rule_thickness = t.hline_thickness;

        let mut hits = StripHits::default();
        widget.record(ui, Some(&strip_bg), |ui| {
            let row = Widget::hstack()
                .id(id.with("row"))
                .size((Sizing::FILL, Sizing::HUG))
                .gap(t.gap)
                .child_align(Align::v(VAlign::Bottom));
            row.record(ui, None, |ui| {
                Scroll::horizontal()
                    .id(Self::band_id(id))
                    .size((Sizing::FILL, Sizing::HUG))
                    .hide_bars()
                    .padding(t.strip_padding)
                    .gap(t.gap)
                    .child_align(Align::v(VAlign::Bottom))
                    .show(ui, |ui| {
                        let chip = ChipCtx {
                            theme: t,
                            ambient,
                            strip: id,
                            focused,
                        };
                        for (i, item) in items.iter().enumerate() {
                            chip.record(ui, item, Some(i) == selected, i, &mut hits);
                        }
                    });
                if overflow == TabOverflow::Menu {
                    overflow_menu(ui, id, items, t, ambient, &mut hits);
                }
            });
            if rule_thickness > 0.0 {
                let rule_leaf = Widget::leaf()
                    .id(id.with("rule"))
                    .size((Sizing::FILL, Sizing::fixed(rule_thickness)));
                rule_leaf.record(ui, Some(&rule), |_| {});
            }
            keyboard_travel(ui, id, items.len(), selected, &mut hits);
        });

        let StripHits {
            clicked,
            keyed,
            closed,
            drag_started,
            drag_stopped,
        } = hits;
        TabStripResponse {
            response: Response::eager(id, ui, response),
            clicked,
            keyed,
            closed,
            drag_started,
            drag_stopped,
        }
    }
}

impl Configure for TabStrip<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

/// The edges one pass over the chips collected, in slot order.
#[derive(Debug, Default)]
struct StripHits {
    clicked: Option<usize>,
    keyed: Option<usize>,
    closed: Option<usize>,
    drag_started: Option<usize>,
    drag_stopped: Option<usize>,
}

/// One strip's shared draw state, threaded through its chips.
#[derive(Debug)]
struct ChipCtx<'a> {
    theme: &'a TabsTheme,
    /// The ambient text style an unset look inherits.
    ambient: TextStyle,
    /// The strip every chip id derives from.
    strip: WidgetId,
    /// Whether the strip's view holds focus — the cap's full-or-dim
    /// question.
    focused: bool,
}

impl ChipCtx<'_> {
    /// One chip: the cap-bearing outer box, the filled inner box, and
    /// the icon, label, badge and close button inside it.
    fn record(
        &self,
        ui: &mut Ui,
        item: &TabItem,
        selected: bool,
        slot: usize,
        hits: &mut StripHits,
    ) {
        let t = self.theme;
        let chip_id = TabStrip::chip_id(self.strip, item.key);
        let close_id = TabStrip::close_id(self.strip, item.key);
        let sense = if item.draggable {
            Sense::CLICK | Sense::DRAG
        } else {
            Sense::CLICK
        };
        // The cap is the outer box's own fill, showing through the top
        // inset the inner box leaves it. Rounded to the full radius
        // while the inner takes a tighter one, so the band follows the
        // corner instead of cutting across it.
        let cap = if selected { t.accent_thickness } else { 0.0 };
        let mut widget = Widget::hstack()
            .id(chip_id)
            .size((Sizing::HUG, Sizing::HUG))
            .min_size(Size::new(t.min_width, 0.0))
            .max_size(Size::new(t.max_width, f32::INFINITY))
            .padding(Spacing::new(0.0, cap, 0.0, 0.0))
            .sense(sense);
        let state = widget.response(ui);
        // The chip's own look paints the *inner* box, so the plan is
        // applied for its spacing defaults and its animation row while
        // the background travels one level down — the `ToggleChrome`
        // shape.
        let look = t
            .plan(&state, selected, self.ambient)
            .apply(ui, &mut widget);
        let cap_bg = if selected {
            Background::rounded(t.cap(self.focused), Corners::top(t.corner))
        } else {
            Background::NONE
        };
        let inner_bg = Background {
            corners: Corners::top((t.corner - cap).max(0.0)),
            ..look.background
        };
        // The selected chip lifts its inner top inset by the cap, so the
        // cap adds no height and every label sits on the same line. A
        // chip carrying a badge or a close button trades its right inset
        // for that glyph's own box — see `TabsTheme::trailing_inset`.
        let [pad_l, pad_t, pad_r, pad_b] = t.chip_padding.as_array();
        let trailing = if item.badge.reserved() || item.closable {
            t.trailing_inset
        } else {
            pad_r
        };
        let padding = Spacing::new(pad_l, (pad_t - cap).max(0.0), trailing, pad_b);

        let text = look.text;
        let TabItem {
            badge,
            icon,
            label,
            closable,
            ..
        } = *item;
        // Probed once, and only for a chip that has one: the look, the
        // glyph style and the click all come off the same response.
        let close = closable.then(|| GlyphButton::resolve(ui, t, close_id, self.ambient));

        let inner = Widget::hstack()
            .id(chip_id.with("fill"))
            .size((Sizing::HUG, Sizing::HUG))
            .padding(padding)
            .gap(t.label_gap)
            .child_align(Align::v(VAlign::Center));
        widget.record(ui, Some(&cap_bg), |ui| {
            inner.record(ui, Some(&inner_bg), |ui| {
                if let Some(handle) = icon {
                    let side = text.font_size_px;
                    let art = Widget::leaf()
                        .id(chip_id.with("icon"))
                        .size((Sizing::fixed(side), Sizing::fixed(side)));
                    art.record(ui, None, |ui| {
                        ui.add_shape(Shape::icon(handle));
                    });
                }
                // The label senses nothing, so a press on it falls
                // through to the chip — which is where both the click
                // and the drag edges are read.
                Text::new(label)
                    .id(chip_id.with("label"))
                    .style(&text)
                    .text_wrap(TextWrap::Ellipsis)
                    .show(ui);
                if badge.reserved() {
                    let fill = if badge.inked() {
                        Background::rounded(t.badge, Corners::all(t.badge_size * 0.5))
                    } else {
                        // Not a transparent fill: the default paints no
                        // quad at all, which is what "reserve the space,
                        // draw nothing" means here.
                        Background::NONE
                    };
                    let dot = Widget::leaf()
                        .id(chip_id.with("badge"))
                        .size((Sizing::fixed(t.badge_size), Sizing::fixed(t.badge_size)))
                        .align(Align::v(VAlign::Center));
                    dot.record(ui, Some(&fill), |_| {});
                }
                if let Some(close) = &close {
                    let button = Widget::zstack()
                        .id(close_id)
                        .size((Sizing::fixed(t.close_size), Sizing::fixed(t.close_size)))
                        .sense(Sense::CLICK)
                        .align(Align::v(VAlign::Center))
                        .child_align(Align::CENTER);
                    button.record(ui, Some(&close.background), |ui| {
                        close.glyph(ui, close_id, "\u{00d7}");
                    });
                }
            });
        });

        if close.is_some_and(|c| c.state.left.clicked()) {
            hits.closed.get_or_insert(slot);
        } else if state.left.clicked() {
            hits.clicked.get_or_insert(slot);
        }
        if state.left.drag.started() {
            hits.drag_started.get_or_insert(slot);
        }
        if state.left.drag.stopped() {
            hits.drag_stopped.get_or_insert(slot);
        }
    }
}

/// A small glyph button in the strip — a chip's close cross, and the
/// overflow chevron.
///
/// Both wear [`TabsTheme::close`] and both centre a single glyph in a
/// square box, so both resolve here: one probe of the response, feeding
/// the look, the glyph style and the click alike. Only the box's
/// placement differs, and that stays at each site.
#[derive(Debug)]
struct GlyphButton {
    state: ResponseState,
    background: Background,
    /// The picked look's text, with the leading removed — a glyph this
    /// much shorter than its line would otherwise ride high in the box.
    text: TextStyle,
}

impl GlyphButton {
    fn resolve(ui: &Ui, t: &TabsTheme, id: WidgetId, ambient: TextStyle) -> Self {
        let state = ui.response_for(id);
        let look = t.close.pick(&state, state.pressed());
        Self {
            background: look.background.clone(),
            text: TextStyle {
                line_height_mult: 1.0,
                ..look.text.unwrap_or(ambient)
            },
            state,
        }
    }

    /// The glyph, centred in a box the caller has already placed.
    fn glyph(&self, ui: &mut Ui, id: WidgetId, glyph: &'static str) {
        Text::new(glyph)
            .id(id.with("glyph"))
            .style(&self.text)
            .text_align(Align::CENTER)
            .show(ui);
    }
}

/// The trailing chevron and the menu behind it — recorded only while at
/// least one chip is scrolled out of the band.
///
/// The menu lists **every** chip, not only the hidden ones. Which chips
/// are out of sight is read from last frame's rects, as every
/// measurement during a record is, so a list of only those would drop
/// and re-add rows as the band scrolls under the open menu. What that
/// staleness costs here is one frame of the chevron itself, which is a
/// button appearing rather than a row moving under the pointer.
fn overflow_menu(
    ui: &mut Ui,
    strip: WidgetId,
    items: &[TabItem],
    t: &TabsTheme,
    ambient: TextStyle,
    hits: &mut StripHits,
) {
    let Some(band) = ui.response_for(TabStrip::band_id(strip)).rect else {
        return;
    };
    let hidden = |item: &TabItem| match ui.response_for(TabStrip::chip_id(strip, item.key)).rect {
        Some(chip) => chip.min.x < band.min.x || chip.max().x > band.max().x,
        None => false,
    };
    let menu_id = strip.with("overflow_menu");
    if !items.iter().any(hidden) && !ContextMenu::is_open(ui, menu_id) {
        return;
    }
    let button_id = strip.with("overflow");
    let chevron = GlyphButton::resolve(ui, t, button_id, ambient);
    // The chevron sits outside the scrolling band, so it takes the
    // strip's own trailing inset as a margin rather than inheriting it.
    let [_, _, band_r, band_b] = t.strip_padding.as_array();
    let button = Widget::zstack()
        .id(button_id)
        .size((Sizing::fixed(t.close_size), Sizing::fixed(t.close_size)))
        .margin(Spacing::new(0.0, 0.0, band_r, band_b))
        .sense(Sense::CLICK)
        .align(Align::v(VAlign::Bottom))
        .child_align(Align::CENTER);
    button.record(ui, Some(&chevron.background), |ui| {
        chevron.glyph(ui, button_id, "\u{22ef}");
    });
    if chevron.state.left.clicked()
        && let Some(rect) = chevron.state.rect
    {
        ContextMenu::open(ui, menu_id, Vec2::new(rect.min.x, rect.max().y));
    }
    let picked = ContextMenu::for_id(menu_id)
        .size((Sizing::HUG, Sizing::HUG))
        .show(ui, |ui, popup| {
            let mut picked = None;
            for (i, item) in items.iter().enumerate() {
                if MenuItem::new(item.label)
                    .id(menu_id.with(item.key))
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
        hits.clicked = Some(slot);
        ContextMenu::close(ui, menu_id);
    }
}

/// Keyboard travel along the strip, on the WAI-ARIA tab pattern: arrows
/// step, `Home` / `End` jump to the ends, and `Ctrl+Tab` cycles. Each of
/// them activates what it lands on, so the caller handles a keyboard
/// move exactly as it handles a click.
///
/// Only while focus is inside the strip, and read inside the strip's own
/// record so the scope it declares is the one that grants the press.
fn keyboard_travel(
    ui: &mut Ui,
    strip: WidgetId,
    len: usize,
    selected: Option<usize>,
    hits: &mut StripHits,
) {
    if len == 0 || !ui.focus_within(strip) {
        return;
    }
    let here = selected.unwrap_or(0).min(len - 1);
    let step = |forward: bool| {
        if forward {
            (here + 1) % len
        } else {
            (here + len - 1) % len
        }
    };
    // Every chord is sampled, not short-circuited: `key_pressed` both
    // reads the press and keeps the chord subscribed for the wake gate,
    // so one of them firing must not drop the others' subscription that
    // frame. Modifier sets match exactly, so `Ctrl+Tab` never fires on
    // `Ctrl+Shift+Tab` and the two orders below cannot cross.
    let back = ui.key_pressed(Shortcut::key(Key::ArrowLeft));
    let forward = ui.key_pressed(Shortcut::key(Key::ArrowRight));
    let first = ui.key_pressed(Shortcut::key(Key::Home));
    let last = ui.key_pressed(Shortcut::key(Key::End));
    let cycle = ui.key_pressed(Shortcut::new(Mods::CTRL, Key::Tab));
    let cycle_back = ui.key_pressed(Shortcut::new(Mods::CTRL_SHIFT, Key::Tab));
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
        hits.keyed = Some(target);
    }
}
