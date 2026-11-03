//! [`OverlayScope`] — a dismissible overlay's claim on its layer.

use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::click_outside::ClickOutside;
use crate::widget_core::close_handle::CloseHandle;
use crate::widget_core::configure::Configure;
use crate::widget_core::overlay_response::{OverlayClose, OverlayResponse};
use crate::widget_core::widget::Widget;

/// What stands between an overlay's body and the layers below it. Pointer and keys
/// are one decision: an overlay takes both or neither.
#[derive(Clone, Copy, Debug)]
pub enum Backdrop {
    /// Nothing. The overlay annotates rather than interrupts: it wants the layer
    /// for paint order and flip-to-fit placement; the host underneath stays live.
    None,
    /// The overlay's own root, recorded by the caller (a modal dims the surface and
    /// absorbs stray pointer events here).
    Root,
    /// A full-surface barrier [`OverlayScope::record`] lays down under this id ahead of the
    /// body, as Flutter's `ModalBarrier`; a *placed* overlay cannot nest its body in its
    /// backdrop, as placement flips it to fit.
    Barrier(WidgetId),
}

impl Backdrop {
    const fn owns_input(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// What one [`OverlayScope::record`] saw: the body's result, and how the overlay closed.
#[derive(Debug)]
#[must_use]
pub struct OverlayTurn<R> {
    /// What the body returned.
    pub inner: R,
    /// [`OverlayClose::Requested`] when the body's [`CloseHandle`] or an activated
    /// [`MenuItem`](crate::MenuItem) in it asked; else [`OverlayClose::Dismissed`] under
    /// [`ClickOutside::Dismiss`] for a press on the backdrop or Escape; else `None`, the overlay
    /// staying open.
    pub close: Option<OverlayClose>,
}

/// Where an overlay records, what stands under it and what a press there does: everything
/// [`OverlayScope::claim`] takes past the root.
#[derive(Clone, Copy, Debug)]
pub struct OverlaySpec {
    /// Where the body records; held by the scope so scope and body cannot land on different
    /// layers.
    pub layer: Layer,
    /// `None` takes the layer's default: the surface origin, whole surface available.
    pub anchor: Option<Anchor>,
    /// What stands between the body and the layers below it.
    pub backdrop: Backdrop,
    /// What a press on the backdrop and Escape do: [`ClickOutside::PassThrough`] exactly when
    /// [`Self::backdrop`] is [`Backdrop::None`], as nothing then stands under the body.
    pub click_outside: ClickOutside,
}

/// One overlay's turn on a layer, from stamping its root until it closes.
/// [`Popup`](crate::Popup), [`Modal`](crate::Modal) and [`Tooltip`](crate::Tooltip)
/// share this lifecycle; two steps have ordering constraints (see [`Self::record`],
/// [`Self::finish`]). A scope silences the layers strictly *below* its own, never
/// its own body, so a `TextEdit` in a popup keeps the keyboard.
///
/// A custom overlay follows the same order: [`Self::claim`] with the root before
/// recording, [`Self::record`] the body, then [`Self::finish`] with its response,
/// which withdraws on the frame it closes.
#[derive(Debug)]
pub struct OverlayScope {
    owner: WidgetId,
    spec: OverlaySpec,
}

impl OverlayScope {
    /// Claim `spec.layer` for `root`, stamping it as the node taking its layer's key
    /// scope when there is a backdrop; the owner is the root's resolved id. Every
    /// class but [`KeyFilter::FOCUS`] is taken, so a popup under a modal does not
    /// dismiss on the same Escape; `FOCUS` stays out so Tab walks the overlay's own
    /// stops.
    ///
    /// # Panics
    ///
    /// Panics unless [`OverlaySpec::click_outside`] is [`ClickOutside::PassThrough`] exactly when
    /// [`OverlaySpec::backdrop`] is [`Backdrop::None`].
    #[track_caller]
    pub fn claim(ui: &mut Ui, spec: OverlaySpec, root: &mut Widget) -> Self {
        assert_eq!(
            spec.backdrop.owns_input(),
            spec.click_outside != ClickOutside::PassThrough,
            "an overlay with a backdrop blocks or dismisses, and one without passes presses through"
        );
        if spec.backdrop.owns_input() {
            root.configure()
                .input_scope(KeyFilter::ALL.difference(KeyFilter::FOCUS));
        }
        let owner = root.resolve(ui);
        Self { owner, spec }
    }

    /// Lay the backdrop down, record `body` into the claimed layer with a [`CloseHandle`], and
    /// report how the overlay closed.
    ///
    /// Escape is read in here as the overlay's own scope, before the layer closes:
    /// afterwards the ambient scope sits below this overlay's and would silence it. Only a
    /// [`ClickOutside::Dismiss`] turn asks, as a read that closes nothing would re-arm a wake every
    /// frame. A barrier goes down first so it paints *under* the body (hit-testing
    /// runs in reverse, so the body still wins); it senses all four pointer
    /// interactions so pan, scroll and pinch cannot leak to the host, and only
    /// `Sense::CLICK` dismisses.
    pub fn record<R>(
        &self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &CloseHandle) -> R,
    ) -> OverlayTurn<R> {
        let OverlaySpec {
            layer,
            anchor,
            backdrop,
            click_outside,
        } = self.spec;
        if let Backdrop::Barrier(id) = backdrop {
            ui.layer(layer).show(|ui| {
                Widget::leaf()
                    .id(id)
                    .size((Sizing::FILL, Sizing::FILL))
                    .sense(Sense::ABSORB_POINTER)
                    .show(ui, None, |_| {});
            });
        }
        let dismisses = click_outside == ClickOutside::Dismiss;
        ui.overlays().enter();
        let scope = ui.layer(layer);
        let scope = match anchor {
            Some(anchor) => scope.anchor(anchor),
            None => scope,
        };
        let handle = CloseHandle::default();
        let mut escape_pressed = false;
        let inner = scope.show(|ui| {
            let inner = body(ui, &handle);
            escape_pressed = dismisses && ui.shortcut_pressed_as(self.owner, Key::Escape);
            inner
        });
        let row_requested = ui.overlays().leave();
        let requested = row_requested || handle.requested();
        let dismissed = dismisses && (escape_pressed || self.backdrop_clicked(ui));
        OverlayTurn {
            inner,
            close: OverlayClose::of(requested, dismissed),
        }
    }

    /// Whether a press landed on the backdrop rather than the body; all four
    /// pointer interactions are absorbed, so a secondary press counts (see
    /// `ResponseState::any_clicked`).
    fn backdrop_clicked(&self, ui: &Ui) -> bool {
        let id = match self.spec.backdrop {
            Backdrop::None => return false,
            Backdrop::Root => self.owner,
            Backdrop::Barrier(id) => id,
        };
        ui.response_for(id).any_clicked()
    }

    /// End the turn with `response`: an overlay that closed gives its scope back, so no close
    /// path keeps owning input. **The current pass is unaffected**: scope paths resolve against
    /// a cascade a frame old, so merely ceasing to record would keep owning input one more frame
    /// and swallow the click where it was. Takes `self`, so the claim is spent.
    pub fn finish<R>(self, ui: &mut Ui, response: OverlayResponse<R>) -> OverlayResponse<R> {
        if response.closed() && self.spec.backdrop.owns_input() {
            ui.withdraw_input_scope(self.owner);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use crate::host::harness::Harness;
    use crate::input::keyboard::key::Key;
    use crate::internals::panic_probe;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::layout::sizing::Sizing;
    use crate::scene::layer::Layer;
    use crate::ui::Ui;
    use crate::widget_core::click_outside::ClickOutside;
    use crate::widget_core::configure::Configure;
    use crate::widget_core::overlay_response::OverlayClose;
    use crate::widget_core::overlay_scope::{Backdrop, OverlayScope, OverlaySpec};
    use crate::widget_core::widget::Widget;
    use glam::UVec2;

    /// A custom overlay over a root backdrop, its body closing through the handle when `close`.
    fn overlay(ui: &mut Ui, click_outside: ClickOutside, close: bool) -> Option<OverlayClose> {
        let mut root = Widget::zstack()
            .id(WidgetId::from_hash("test.overlay"))
            .size((Sizing::FILL, Sizing::FILL));
        let spec = OverlaySpec {
            layer: Layer::Modal,
            anchor: None,
            backdrop: Backdrop::Root,
            click_outside,
        };
        let scope = OverlayScope::claim(ui, spec, &mut root);
        scope
            .record(ui, |ui, handle| {
                root.record(ui, None, |_| {});
                if close {
                    handle.close();
                }
            })
            .close
    }

    /// The turn's close folds the body's request and the dismissal edges under the spec's policy:
    /// a request wins, Escape dismisses under `Dismiss` alone, and nothing closes a quiet turn.
    #[test]
    fn a_turn_closes_by_request_then_by_dismissal_under_its_policy() {
        let cases = [
            (ClickOutside::Dismiss, false, false, None),
            (
                ClickOutside::Dismiss,
                true,
                false,
                Some(OverlayClose::Dismissed),
            ),
            (ClickOutside::Block, true, false, None),
            (
                ClickOutside::Block,
                false,
                true,
                Some(OverlayClose::Requested),
            ),
            (
                ClickOutside::Dismiss,
                true,
                true,
                Some(OverlayClose::Requested),
            ),
        ];
        for (click_outside, escape, close, want) in cases {
            let mut h = Harness::builder(UVec2::new(200, 100)).mono().build();
            h.prime(2, |ui| {
                overlay(ui, click_outside, false);
            });
            if escape {
                h.key(Key::Escape);
            }
            let got = h.frame_value(|ui| overlay(ui, click_outside, close));
            assert_eq!(
                got, want,
                "{click_outside:?}, Escape {escape}, close {close}"
            );
        }
    }

    /// A backdrop and a pass-through policy contradict each other, and so do no backdrop and a
    /// policy that needs one.
    #[test]
    fn a_spec_whose_backdrop_and_policy_disagree_panics() {
        let message = "an overlay with a backdrop blocks or dismisses, and one without passes presses through";
        for (backdrop, click_outside) in [
            (Backdrop::None, ClickOutside::Dismiss),
            (Backdrop::None, ClickOutside::Block),
            (Backdrop::Root, ClickOutside::PassThrough),
        ] {
            panic_probe::assert_panics_with(message, || {
                let mut h = Harness::builder(UVec2::new(20, 20)).mono().build();
                h.frame(|ui| {
                    let mut root = Widget::zstack().id(WidgetId::from_hash("test.bad"));
                    let spec = OverlaySpec {
                        layer: Layer::Popup,
                        anchor: None,
                        backdrop,
                        click_outside,
                    };
                    let _ = OverlayScope::claim(ui, spec, &mut root);
                });
            });
        }
    }
}
