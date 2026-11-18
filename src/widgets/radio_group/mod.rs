//! [`RadioGroup`]: one pick from a set, over the shared value its options write through, as
//! WAI-ARIA's radio group pattern works it from the keyboard.

mod segment_option;
mod segment_track;

use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::icon_handle::IconHandle;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::key_class::KeyFilter;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::layout::track::Track;
use crate::primitives::math::domain;
use crate::primitives::text::text_input::TextInput;
use crate::scene::node::node_mode::NodeMode;
use crate::shape::Shape;
use crate::theme::Theme;
use crate::theme::segmented::SegmentedTheme;
use crate::theme::toggle::ToggleTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::radio_group::segment_option::SegmentOption;
use crate::widgets::radio_group::segment_track::SegmentTrack;
use crate::widgets::toggle_row::{Settled, ToggleBox, ToggleFace, ToggleRow};
use std::rc::Rc;

/// How a [`RadioGroup`]'s options draw, each with the theme slot it reads: `None` for the
/// theme's own, `Some` for a per-instance override. The keys work alike in each, so the look
/// changes what the group shows and never how it answers, as SwiftUI's `pickerStyle` restyles one
/// `Picker` (<https://developer.apple.com/documentation/swiftui/pickerstyle>). The slot rides in
/// the variant, so no look takes the other's slot.
#[derive(Clone, Copy, Debug)]
pub enum RadioLook<'a> {
    /// A round pip beside each label, in [`Theme::radio`](crate::Theme::radio) unless overridden.
    Pip(Option<&'a ToggleTheme>),
    /// The labels as equal segments of one track, a pill sliding under the checked one, in
    /// [`Theme::segmented`](crate::Theme::segmented) unless overridden, as Ark UI's segment group
    /// and Mantine's segmented control are radio groups. The options record straight into the
    /// track, each an equal share of it along [`RadioGroup::axis`], which is what places the pill.
    Segment(Option<&'a SegmentedTheme>),
}

/// One pick from a set: each option checks while `*selected` equals its value, and picking it
/// writes the value into `selected`. The keys follow WAI-ARIA's radio group
/// (<https://www.w3.org/WAI/ARIA/apg/patterns/radio/>): the group is one Tab stop, which lands
/// on the checked option, or the first while none is; an arrow moves focus to the next option
/// (Right, Down) or the previous (Left, Up) and checks it, wrapping at the ends; Space checks the
/// focused one.
///
/// The group is a stack along [`Self::axis`], vertical by default, as WinUI's `RadioButtons`
/// stacks; the body records its options through [`RadioOptions::option`], in any panels it
/// opens, so the options may sit in a grid or beside other widgets. Arrows step in record order.
/// Each option's id is the group's with its index; a test or an agent finds an option by its
/// label.
///
/// Refused: a free-standing radio button, a second route to a group that answers no arrow; a
/// group over a slice of options, which leaves no option's response to hang a tooltip on.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct RadioGroup<'a, T> {
    widget: Widget,
    axis: Axis,
    selected: &'a mut T,
    look: RadioLook<'a>,
}

impl<'a, T> RadioGroup<'a, T> {
    /// A group writing the picked option's value into `selected`.
    #[track_caller]
    pub fn new(selected: &'a mut T) -> Self {
        Self {
            widget: Widget::vstack().input_scope(KeyFilter::CARET),
            axis: Axis::Y,
            selected,
            look: RadioLook::Pip(None),
        }
    }

    /// The axis the options stack along. Default [`Axis::Y`].
    pub const fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// How the options draw, and the slot each reads. Default `RadioLook::Pip(None)`.
    pub const fn look(mut self, look: RadioLook<'a>) -> Self {
        self.look = look;
        self
    }
}

impl<T: PartialEq> RadioGroup<'_, T> {
    /// Records the group and the options `body` records. `changed` and `committed` report a pick
    /// this frame by click, Space or arrow; a pick commits at once.
    pub fn show(
        mut self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &mut RadioOptions<'_, T>),
    ) -> ValueResponse {
        let response = self.widget.response(ui);
        let id = response.id;
        // An `Rc` bump, so the slots outlive the `&mut Ui` the record takes.
        let theme = Rc::clone(ui.theme());
        let Self {
            mut widget,
            axis,
            selected,
            look,
        } = self;
        let changed = ui.with_state::<GroupMemory, _>(id, |ui, memory| match look {
            RadioLook::Pip(slot) => {
                widget.node.mode = NodeMode::Resolved(LayoutMode::Stack(axis));
                let run = OptionsRun {
                    group: id,
                    group_disabled: response.disabled,
                    selected,
                    look: OptionLook::Pip(slot.unwrap_or(&theme.radio)),
                };
                widget.record(ui, None, |ui| run.record(ui, memory, body))
            }
            RadioLook::Segment(slot) => {
                let slot = slot.unwrap_or(&theme.segmented);
                SegmentTrack { id, slot, axis }.record(ui, widget, memory, |ui, memory, pill_at| {
                    OptionsRun {
                        group: id,
                        group_disabled: response.disabled,
                        selected,
                        look: OptionLook::Segment {
                            slot,
                            pill_at,
                            axis,
                        },
                    }
                    .record(ui, memory, body)
                })
            }
        });
        ValueResponse::pick(response, changed)
    }
}

/// One record of a group's options: the group, whether it is disabled, the value its options
/// write and the look they take.
#[derive(Debug)]
struct OptionsRun<'r, T> {
    group: WidgetId,
    group_disabled: bool,
    selected: &'r mut T,
    look: OptionLook<'r>,
}

impl<T: PartialEq> OptionsRun<'_, T> {
    /// Records the options `body` records against last frame's `memory`, refills it with this
    /// frame's, and returns whether a pick changed the value.
    fn record(
        self,
        ui: &mut Ui,
        memory: &mut GroupMemory,
        body: impl FnOnce(&mut Ui, &mut RadioOptions<'_, T>),
    ) -> bool {
        let Self {
            group,
            group_disabled,
            selected,
            look,
        } = self;
        let target = if group_disabled || !ui.is_focus_within(group) {
            None
        } else {
            arrow_target(ui, group, memory)
        };
        let entry = match memory.checked {
            Some(checked) if !memory.disabled[checked as usize] => None,
            _ => memory.first_enabled(),
        };
        memory.disabled.clear();
        let mut options = RadioOptions {
            selected,
            group,
            look,
            target,
            entry,
            checked: None,
            changed: false,
            disabled: &mut memory.disabled,
        };
        body(ui, &mut options);
        let RadioOptions {
            checked, changed, ..
        } = options;
        if let Some(target) = target
            && checked == Some(target)
        {
            ui.set_focus(option_id(group, target));
        }
        memory.checked = checked;
        changed
    }
}

impl<T> Configure for RadioGroup<'_, T> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// The option an arrow pressed this frame moves to, from last frame's options: the next enabled
/// one after the focused one for Right or Down, the previous for Left or Up, wrapping, as APG's
/// radio group skips a disabled radio. `None` without an arrow, while focus sits on no option, as
/// on a widget the body recorded beside them, or with no other option enabled.
fn arrow_target(ui: &mut Ui, group: WidgetId, memory: &GroupMemory) -> Option<u32> {
    // Every key is read, not short-circuited, so each stays watched for the wake gate.
    let next = [Key::ArrowRight, Key::ArrowDown].map(|key| ui.shortcut_pressed(key));
    let prev = [Key::ArrowLeft, Key::ArrowUp].map(|key| ui.shortcut_pressed(key));
    let count = memory.disabled.len();
    let step = match (next.contains(&true), prev.contains(&true)) {
        (true, false) => 1,
        (false, true) => count.checked_sub(1)?,
        _ => return None,
    };
    let focused = (0..count).find(|&idx| ui.is_focus_within(option_id(group, idx as u32)))?;
    (1..count)
        .map(|hop| (focused + hop * step) % count)
        .find(|&idx| !memory.disabled[idx])
        .map(|idx| idx as u32)
}

/// Option `idx` of `group`.
pub(crate) fn option_id(group: WidgetId, idx: u32) -> WidgetId {
    group.with(("option", idx))
}

/// What a group keeps from its last frame: which of the options its body recorded were disabled,
/// in record order, and the one that showed checked. An arrow steps among last frame's options,
/// and while no enabled one is checked the first enabled one is the Tab stop. The mask is cleared and
/// filled each frame, so it allocates only when the group grows. A segment track keeps its pill's
/// and its grid's too.
#[derive(Debug, Default)]
struct GroupMemory {
    disabled: Vec<bool>,
    checked: Option<u32>,
    pill: PillMemory,
    /// A segment track's grid tracks, one per option, refilled each frame.
    tracks: Vec<Track>,
}

/// What a segment track's pill keeps between frames: the segment it last headed for, and how many
/// times it has moved, which its fade while motion is reduced heads for.
#[derive(Debug, Default)]
struct PillMemory {
    at: Option<u32>,
    moves: u32,
}

impl GroupMemory {
    /// The first option last frame left enabled.
    fn first_enabled(&self) -> Option<u32> {
        self.disabled
            .iter()
            .position(|&disabled| !disabled)
            .map(|idx| idx as u32)
    }
}

/// A group's slot, resolved once for every option, and for a segment track where its pill is.
#[derive(Clone, Copy, Debug)]
enum OptionLook<'s> {
    Pip(&'s ToggleTheme),
    Segment {
        slot: &'s SegmentedTheme,
        /// The pill's place along the track this frame, in segments from the first; `None` while
        /// it draws none.
        pill_at: Option<f32>,
        /// The axis the track runs along.
        axis: Axis,
    },
}

/// The options of a [`RadioGroup`], as its body records them through [`Self::option`].
#[derive(Debug)]
pub struct RadioOptions<'g, T> {
    selected: &'g mut T,
    group: WidgetId,
    look: OptionLook<'g>,
    /// The option an arrow checks this frame; while set, a click checks nothing, so one option
    /// shows checked.
    target: Option<u32>,
    /// The Tab stop while last frame checked no enabled option: the first enabled one.
    entry: Option<u32>,
    checked: Option<u32>,
    changed: bool,
    /// Each recorded option's own disabled flag, in record order; its length is the next index.
    disabled: &'g mut Vec<bool>,
}

impl<'g, T: PartialEq> RadioOptions<'g, T> {
    /// The next option, checked while the group's value equals `value`.
    #[track_caller]
    pub fn option(&mut self, value: T) -> RadioOption<'_, 'g, T> {
        let widget = match self.look {
            OptionLook::Pip(slot) => {
                let mut row = ToggleRow::new();
                row.style = Some(slot);
                OptionWidget::Pip(row, None)
            }
            OptionLook::Segment {
                slot,
                pill_at,
                axis,
            } => OptionWidget::Segment(SegmentOption::new(slot, pill_at, axis)),
        };
        RadioOption {
            options: self,
            value,
            widget,
        }
    }
}

/// One option of a [`RadioGroup`], from [`RadioOptions::option`]. Its id is the group's with its
/// index, which arrows step by, so it takes none of its own, and the group sets its Tab stop.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct RadioOption<'o, 'g, T> {
    options: &'o mut RadioOptions<'g, T>,
    value: T,
    widget: OptionWidget<'o>,
}

/// An option's widget in its group's look.
#[derive(Debug)]
enum OptionWidget<'o> {
    /// The row, and the icon it shows between its box and its label.
    Pip(ToggleRow<'o>, Option<IconHandle>),
    Segment(SegmentOption<'o>),
}

impl<'o, T: PartialEq> RadioOption<'o, '_, T> {
    /// Label text.
    pub fn label(mut self, label: impl Into<TextInput<'o>>) -> Self {
        self.widget = match self.widget {
            OptionWidget::Pip(mut row, icon) => {
                row.label = label.into();
                OptionWidget::Pip(row, icon)
            }
            OptionWidget::Segment(mut segment) => {
                segment.label = label.into();
                OptionWidget::Segment(segment)
            }
        };
        self
    }

    /// An icon before the label, tinted in the label's ink and a text line tall: the pip's
    /// between its box and its label, the segment's centered with its label.
    pub const fn icon(mut self, icon: IconHandle) -> Self {
        match &mut self.widget {
            OptionWidget::Pip(_, shown) => *shown = Some(icon),
            OptionWidget::Segment(segment) => segment.icon = Some(icon),
        }
        self
    }

    /// Records the option. An option latches: `response.clicked()` is true on the checked one but
    /// `changed` is not.
    ///
    /// # Panics
    ///
    /// When the caller set the option's id, which its group sets.
    pub fn show(mut self, ui: &mut Ui) -> ValueResponse {
        let node = self.configure();
        let auto_id = node.has_auto_id();
        let disabled = node.widget.authored().disabled();
        assert!(auto_id, "a radio option takes its id from its group");
        let Self {
            options,
            value,
            widget,
        } = self;
        let idx = u32::try_from(options.disabled.len()).expect("a group holds under 2^32 options");
        options.disabled.push(disabled);
        let pick = Pick {
            selected: &mut *options.selected,
            value,
            arrow: options.target.map(|target| target == idx && !disabled),
        };
        let id = option_id(options.group, idx);
        let checked = pick.shows_checked();
        let tab_stop = (checked && !disabled) || options.entry == Some(idx);
        let shown = match widget {
            OptionWidget::Pip(mut row, icon) => {
                row.widget.configure().id(id).tab_stop(tab_stop);
                row.show(ui, pick, icon)
            }
            OptionWidget::Segment(mut segment) => {
                segment.widget.configure().id(id).tab_stop(tab_stop);
                segment.show(ui, idx, pick)
            }
        };
        if checked || shown.changed {
            options.checked = Some(idx);
        }
        options.changed |= shown.changed;
        shown
    }
}

impl<T> Configure for RadioOption<'_, '_, T> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        match &mut self.widget {
            OptionWidget::Pip(row, _) => row.widget.configure(),
            OptionWidget::Segment(segment) => segment.widget.configure(),
        }
    }
}

/// What one option does with this frame's activation: under an arrow, the arrow's target is
/// picked and no click counts, so one option shows checked; else a click or Space picks.
#[derive(Debug)]
struct Pick<'c, T> {
    selected: &'c mut T,
    value: T,
    /// Whether this option is the arrow's target, while an arrow moves the pick.
    arrow: Option<bool>,
}

impl<T: PartialEq> Pick<'_, T> {
    /// Checked before this frame's activation: the arrow's target, or the option holding the
    /// value.
    fn shows_checked(&self) -> bool {
        self.arrow.unwrap_or_else(|| *self.selected == self.value)
    }
}

impl<T: PartialEq> ToggleFace for Pick<'_, T> {
    /// An option latches: picking the checked one moves nothing.
    fn settle(self, activated: bool) -> Settled {
        let checked = self.shows_checked();
        let picked = self.arrow.unwrap_or(activated);
        let changed = picked && *self.selected != self.value;
        if changed {
            *self.selected = self.value;
        }
        Settled {
            checked: checked || changed,
            changed,
        }
    }

    fn slot(theme: &Theme) -> &ToggleTheme {
        &theme.radio
    }

    fn pill(slot: &ToggleTheme) -> Option<f32> {
        Some(slot.box_side() * 0.5)
    }

    /// A dot inside the round box while checked.
    fn paint(ui: &mut Ui, ToggleBox { slot, checked, .. }: ToggleBox<'_>) {
        if checked {
            let pip = slot.box_side();
            let inset = domain::length_at_least(slot.indicator_inset, 0.0);
            let dot = domain::length_at_least(pip - 2.0 * inset, 0.0);
            ui.add_shape(
                Shape::rect()
                    .at(Rect::new(inset, inset, dot, dot))
                    .corners(dot * 0.5),
                slot.indicator,
            );
        }
    }
}

#[cfg(test)]
mod tests;
