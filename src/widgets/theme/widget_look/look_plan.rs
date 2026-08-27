//! Everything a themed widget takes out of its theme, owned, so the
//! borrow does not have to survive the `&mut Ui` its own body takes.

use crate::ui::Ui;
use crate::widgets::theme::widget_look::WidgetLook;
use crate::widgets::theme::widget_look::animated_look::AnimatedLook;
use crate::widgets::theme::widget_look::theme_slot::SlotDefaults;
use crate::widgets::widget::Widget;

/// Everything a themed widget takes out of its theme slot, held as owned
/// values so the borrow on [`Ui::theme`] can end.
///
/// The two halves of resolving a look need different borrows of the `Ui`:
/// picking the per-state [`WidgetLook`] and reading the spacing defaults read
/// the theme, while animating toward the result reborrows the `Ui` mutably.
/// This value is where they meet — the bundle fills it while the theme borrow
/// is live, then [`Self::apply`] consumes it after that borrow has ended.
///
/// **Built by [`ThemeSlot::plan`], never by hand.** The bundle names its own
/// per-state looks and box defaults once, in its `ThemeSlot` impl, so a
/// widget reaches a plan through one call rather than restating the quartet.
///
/// [`ThemeSlot::plan`]: crate::widgets::theme::widget_look::theme_slot::ThemeSlot::plan
#[derive(Debug)]
pub(crate) struct LookPlan {
    /// The flattened look to animate toward.
    pub(crate) target: AnimatedLook,
    /// What the bundle contributes to the node around that look.
    pub(crate) defaults: SlotDefaults,
}

impl LookPlan {
    /// Dress `widget` in this look: fill in the padding/margin its builder
    /// did not configure, then animate toward the planned look.
    ///
    /// Takes the whole [`Widget`] rather than its id and node separately —
    /// both halves come from it, and the animation row this keys is the same
    /// identity the node is about to record under.
    ///
    /// The returned look is **not** stashed on the widget, because it does not
    /// always belong to it: a toggle configures its *row* here and paints the
    /// look on the box node inside it (see `ToggleChrome::record_row`). Widgets that
    /// do wear it themselves pass `Some(&look.background)` to
    /// [`Widget::record`].
    ///
    /// **The only route from a theme bundle to a painted look** — `Button`,
    /// `ComboBox`, `DragValue`'s chip, `TextEdit`, `MenuItem`, and the three
    /// toggles (through `ToggleChrome::record_row`) all arrive here, so per-state
    /// precedence, spacing defaults, and transitions are one behaviour rather
    /// than one per widget.
    // This crosses the theme/widget codegen-unit boundary. Leaving it to the
    // default inliner kept the resolver plus its tiny trait accessors outlined
    // in release builds; the frame bench measured that path at 3.9% precise
    // self-time. Force the whole chain into each widget so state picking,
    // default resolution and target construction optimize as one block.
    #[inline(always)]
    pub(crate) fn apply(self, ui: &mut Ui, widget: &mut Widget) -> AnimatedLook {
        let Self {
            target,
            defaults:
                SlotDefaults {
                    padding,
                    margin,
                    anim,
                },
        } = self;
        let node = &mut widget.node;
        // `get_or_insert`, not `ThemeDefaults::default_padding` — same
        // "fill in only where the caller stayed silent" rule, and the trait's
        // body is this same guarded write. Routing through it would move a
        // 120-byte `Node` through two consuming builders that carry no
        // `#[inline]`, three copies deep, once per themed widget per frame —
        // on the path the note above says the default inliner already leaves
        // outlined.
        node.padding.get_or_insert(padding);
        node.margin.get_or_insert(margin);
        ui.animate(widget.id(), WidgetLook::SLOT_LOOK, target, anim)
    }
}
