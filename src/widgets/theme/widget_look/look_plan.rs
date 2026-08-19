use crate::animation::AnimSpec;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
use crate::ui::Ui;
use crate::widgets::theme::widget_look::WidgetLook;
use crate::widgets::theme::widget_look::animated_look::AnimatedLook;

/// Everything a themed widget takes out of its theme slot, held as owned
/// values so the borrow on [`Ui::theme`] can end.
///
/// The two halves of resolving a look need different borrows of the `Ui`:
/// picking the per-state [`WidgetLook`] and reading the spacing defaults reads
/// the theme, while animating toward the result reborrows the `Ui` mutably.
/// Splitting them at this value is what lets the widget name its theme slot
/// **once** — it holds the slot across both the scalars it copies out for
/// itself and the plan it builds here, instead of copying scalars off one
/// naming and handing a second naming to a resolver as a fallback closure.
///
/// Build with [`WidgetTheme::plan`](crate::widgets::theme::WidgetTheme::plan)
/// and consume with [`Self::apply`]; between the two, every theme borrow is
/// released.
///
/// Built by struct literal rather than through a constructor: `padding` and
/// `margin` are the same type and sit next to each other, so a positional
/// constructor is a swap that compiles.
#[derive(Debug)]
pub(crate) struct LookPlan {
    /// The flattened look to animate toward.
    pub(crate) target: AnimatedLook,
    pub(crate) padding: Spacing,
    pub(crate) margin: Spacing,
    pub(crate) anim: Option<AnimSpec>,
}

impl LookPlan {
    /// Fill in the padding/margin the caller did not configure, then animate
    /// toward the planned look.
    ///
    /// **The only route from a theme bundle to a painted look** — `Button`,
    /// `ComboBox`, `DragValue`'s chip, `TextEdit`, `MenuItem`, and the three
    /// toggles (through `toggle::toggle_row`) all arrive here, so per-state
    /// precedence, spacing defaults, and transitions are one behaviour rather
    /// than one per widget.
    // This crosses the theme/widget codegen-unit boundary. Leaving it to the
    // default inliner kept the resolver plus its tiny trait accessors outlined
    // in release builds; the frame bench measured that path at 3.9% precise
    // self-time. Force the whole chain into each widget so state picking,
    // default resolution and target construction optimize as one block.
    #[inline(always)]
    pub(crate) fn apply(self, ui: &mut Ui, id: WidgetId, node: &mut Node) -> AnimatedLook {
        let Self {
            target,
            padding,
            margin,
            anim,
        } = self;
        // `get_or_insert`, not `ThemeDefaults::default_padding` — same
        // "fill in only where the caller stayed silent" rule, and the trait's
        // body is this same guarded write. Routing through it would move a
        // 120-byte `Node` through two consuming builders that carry no
        // `#[inline]`, three copies deep, once per themed widget per frame —
        // on the path the note above says the default inliner already leaves
        // outlined.
        node.padding.get_or_insert(padding);
        node.margin.get_or_insert(margin);
        ui.animate(id, WidgetLook::SLOT_LOOK, target, anim)
    }
}
