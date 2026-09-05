//! [`OverlayScope`] — a dismissible overlay's claim on its layer.

use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::layout::types::placement::Placement;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::frame::Frame;
use crate::widgets::widget::Widget;

/// What stands between an overlay's body and the layers below it.
///
/// The pointer and the keys are one decision, so this is one value: an
/// overlay either takes both from the layers below or neither. Taking one
/// without the other leaves a host that is half-dead in a way nothing at
/// the call site would explain.
#[derive(Clone, Copy, Debug)]
pub(super) enum Backdrop {
    /// Nothing. The overlay annotates rather than interrupts: it wants
    /// the layer for paint order and its flip-to-fit placement, and the
    /// host underneath stays live. A tooltip recorded unconditionally
    /// every frame would otherwise cut off every layer below it forever.
    None,
    /// The overlay's own root, which the caller records itself. A modal
    /// dims the surface, absorbs stray pointer events, and centres its
    /// card in the one node.
    Root,
    /// A full-surface eater [`OverlayScope::record`] lays down under this
    /// id, ahead of the body. A *placed* overlay cannot nest its body
    /// inside its backdrop — the placement is what flips the body to fit
    /// — so the backdrop is a sibling recorded first.
    Eater(WidgetId),
}

impl Backdrop {
    /// Whether the overlay takes input from the layers below.
    ///
    /// The keys and the pointer together — the one decision this type
    /// exists to hold, so the three sites that gate on it read it here
    /// rather than each testing the variant.
    fn owns_input(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// What one overlay turn produced: whatever the body returned, and the
/// two dismissal edges the turn observed.
///
/// `inner` rides along the way [`Widget::record`](crate::Widget::record)
/// returns its body's value — without it every host would have to smuggle
/// the result out of the closure through an `Option` it then unwraps.
#[derive(Debug)]
#[must_use]
pub(super) struct OverlayTurn<R> {
    pub(super) inner: R,
    /// Escape landed inside the scope.
    pub(super) escape: bool,
    /// A press landed on the backdrop rather than on the body.
    pub(super) outside: bool,
}

/// One overlay's turn on a layer, from the moment its root is stamped
/// until it resolves that it closed.
///
/// [`Popup`](crate::Popup), [`Modal`](crate::Modal) and
/// [`Tooltip`](crate::Tooltip) share the whole lifecycle — claim the
/// layer, lay a backdrop, record a body inside it, dismiss on Escape or
/// an outside press, hand the scope back — and two of those steps have an
/// ordering constraint that is invisible at the call site and wrong by
/// default. Holding them as one type is what keeps a fourth overlay from
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
    /// the body placed on another — every caller knows the whole
    /// placement at claim time anyway.
    layer: Layer,
    placement: Placement,
    backdrop: Backdrop,
}

impl OverlayScope {
    /// Claim `layer` for `owner`, stamping `root` as the node that takes
    /// the layer's entire key scope when the overlay has a backdrop.
    ///
    /// [`KeyFilter::ALL`] rather than something narrower because an
    /// overlay *owns* input while it is up: it does not merely outrank
    /// the layers below it, it cuts them off. That is what stops a popup
    /// underneath a modal from dismissing alongside it on one Escape.
    pub(super) fn claim(
        owner: WidgetId,
        layer: Layer,
        placement: impl Into<Placement>,
        backdrop: Backdrop,
        root: &mut Widget,
    ) -> Self {
        if backdrop.owns_input() {
            root.configure().input_scope(KeyFilter::ALL);
        }
        Self {
            owner,
            layer,
            placement: placement.into(),
            backdrop,
        }
    }

    /// Lay the backdrop down, record `body` into the claimed layer at
    /// `placement`, and report both dismissal edges.
    ///
    /// The Escape read happens in here, before the layer closes, and it
    /// has to: outside the layer the ambient scope sits below this
    /// overlay's, so an `escape_pressed()` call made after the fact is
    /// silenced by the very scope the overlay just declared — and the
    /// overlay never sees its own dismiss key. A backdrop-less scope
    /// reports `false` without asking: it has no dismiss key, and
    /// `escape_pressed` auto-watches the chord for wake-up, which an
    /// always-recorded overlay would re-arm every frame for nothing.
    ///
    /// A recorded eater goes down first, so it paints *under* the body.
    /// Hit-test runs reverse-iter, so the body's leaves still win inside
    /// its rect. It senses all four pointer interactions, so the overlay
    /// is truly modal over the layers below: pan-drag, scroll and pinch
    /// over the surrounding area cannot leak through to the host — a
    /// graph canvas that pans on middle-drag and zooms on scroll, say.
    /// `Sense::CLICK` is the dismiss trigger; the other three never
    /// produce visible behaviour on the eater itself, and are absorbed
    /// and discarded.
    pub(super) fn record<R>(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> OverlayTurn<R> {
        if let Backdrop::Eater(id) = self.backdrop {
            ui.layer(self.layer).show(|ui| {
                Frame::new()
                    .id(id)
                    .size((Sizing::FILL, Sizing::FILL))
                    .sense(Sense::ABSORB_POINTER)
                    .show(ui);
            });
        }
        let owns_input = self.backdrop.owns_input();
        let (inner, escape) = ui.layer(self.layer).placement(self.placement).show(|ui| {
            let inner = body(ui);
            (inner, owns_input && ui.escape_pressed())
        });
        OverlayTurn {
            inner,
            escape,
            outside: self.backdrop_clicked(ui),
        }
    }

    /// Whether a press landed on the backdrop rather than the body.
    ///
    /// The backdrop absorbs all four pointer interactions, so a secondary
    /// press outside is absorbed either way — see
    /// `ResponseState::any_clicked` for why absorbing it and ignoring it
    /// is the case that matters.
    fn backdrop_clicked(&self, ui: &Ui) -> bool {
        let id = match self.backdrop {
            Backdrop::None => return false,
            Backdrop::Root => self.owner,
            Backdrop::Eater(id) => id,
        };
        ui.response_for(id).any_clicked()
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
        if closed && self.backdrop.owns_input() {
            ui.close_scope(self.owner);
        }
    }
}
