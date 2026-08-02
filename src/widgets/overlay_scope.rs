//! [`OverlayScope`] — a dismissible overlay's claim on its layer's
//! input scope.

use crate::input::key_class::KeyFilter;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::Node;
use crate::scene::tree::recording::Placement;
use crate::ui::Ui;

/// One overlay's ownership of its layer's key scope, held from the
/// moment its root node is stamped until it resolves that it closed.
///
/// [`Popup`](crate::Popup) and [`Modal`](crate::Modal) share the whole
/// lifecycle — claim the layer, record a body inside it, dismiss on
/// Escape, hand the scope back — and two of those steps have an
/// ordering constraint that is invisible at the call site and wrong by
/// default. Holding them as one type is what keeps a third overlay from
/// rediscovering them: see [`Self::record`] and [`Self::withdraw`].
///
/// A scope silences the layers strictly *below* its own, never its own
/// body, which is what lets a `TextEdit` inside a popup keep reading the
/// keyboard while everything under the popup stops.
#[derive(Debug)]
pub(super) struct OverlayScope {
    owner: WidgetId,
    /// Where the body records. Retained rather than passed to
    /// [`Self::record`] so the scope cannot be stamped on one layer and
    /// the body placed on another — both callers know the whole
    /// placement at claim time anyway.
    layer: Layer,
    placement: Placement,
}

impl OverlayScope {
    /// Stamp `root` as the node taking `layer`'s entire key scope.
    ///
    /// [`KeyFilter::ALL`] rather than something narrower because an
    /// overlay *owns* input while it is up: it does not merely outrank
    /// the layers below it, it cuts them off. That is what stops a popup
    /// underneath a modal from dismissing alongside it on one Escape.
    pub(super) fn claim(
        owner: WidgetId,
        layer: Layer,
        placement: impl Into<Placement>,
        root: &mut Node,
    ) -> Self {
        root.flags.set_key_filter(KeyFilter::ALL);
        Self {
            owner,
            layer,
            placement: placement.into(),
        }
    }

    /// Record `body` into the claimed layer at `placement` and report
    /// whether Escape landed **inside the scope**.
    ///
    /// The read happens in here, before the layer closes, and it has to:
    /// outside the layer the ambient scope sits below this overlay's, so
    /// an `escape_pressed()` call made after the fact is silenced by the
    /// very scope the overlay just declared — and the overlay never sees
    /// its own dismiss key.
    pub(super) fn record(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> bool {
        ui.layer(self.layer).placement(self.placement).show(|ui| {
            body(ui);
            ui.escape_pressed()
        })
    }

    /// Give the scope back once the overlay has resolved that it closed.
    ///
    /// **The pass this runs in is unaffected** — a scope path resolves
    /// once per pass against a cascade that is a frame old, so an
    /// overlay that merely stopped recording would go on owning input
    /// for the frame after it is gone: long enough to swallow the click
    /// that lands where it used to be.
    ///
    /// Takes `self`, so the claim is spent by the call. An overlay that
    /// stays open drops it instead, and one that closes cannot go on
    /// using a scope it has handed back.
    pub(super) fn withdraw(self, ui: &mut Ui, closed: bool) {
        if closed {
            ui.close_scope(self.owner);
        }
    }
}
