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
use crate::widget_core::configure::Configure;
use crate::widget_core::overlay_response::{OverlayClose, OverlayResponse};
use crate::widget_core::widget::Widget;

/// What stands between an overlay's body and the layers below it, and what a press on it and
/// Escape do. Pointer and keys are one decision: an overlay takes both or neither, and one that
/// takes them says what a press outside does, so no backdrop passes a press through and no
/// overlay without one blocks it.
#[derive(Clone, Copy, Debug)]
pub enum Backdrop {
    /// Nothing. The overlay annotates rather than interrupts: it wants the layer
    /// for paint order and flip-to-fit placement; the host underneath stays live, and a press
    /// outside or Escape reaches it.
    None,
    /// The overlay's own root, recorded by the caller (a modal dims the surface and
    /// absorbs stray pointer events here).
    Root(ClickOutside),
    /// A full-surface barrier [`OverlayScope::record`] lays down under this id ahead of the
    /// body, as Flutter's `ModalBarrier`; a *placed* overlay cannot nest its body in its
    /// backdrop, as placement flips it to fit.
    Barrier(WidgetId, ClickOutside),
}

impl Backdrop {
    /// What a press on the backdrop and Escape do; `None` with no backdrop to press.
    const fn click_outside(self) -> Option<ClickOutside> {
        match self {
            Self::None => None,
            Self::Root(click_outside) | Self::Barrier(_, click_outside) => Some(click_outside),
        }
    }
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
    /// What stands between the body and the layers below it, and what a press on it does.
    pub backdrop: Backdrop,
}

/// One overlay's turn on a layer, from stamping its root until it closes.
/// [`Popup`](crate::Popup), [`Modal`](crate::Modal) and [`Tooltip`](crate::Tooltip)
/// share this lifecycle; two steps have ordering constraints (see [`Self::record`],
/// [`Self::finish`]). A claim silences the layers strictly *below* its own, never
/// its own body, so a `TextEdit` in a popup keeps the keyboard.
///
/// A custom overlay follows the same order: [`Self::claim`] with the root before
/// recording, [`Self::record`] the body, then [`Self::finish`] with its response,
/// which withdraws on the frame it closes.
#[derive(Debug)]
pub struct OverlayScope {
    owner: WidgetId,
    spec: OverlaySpec,
    /// The owner recorded no rect last frame, so the overlay opens this frame.
    opened: bool,
}

impl OverlayScope {
    /// Claim `spec.layer` for `root`, stamping it as the node taking its layer's key
    /// scope when there is a backdrop, which cuts every plane below off the keyboard and the
    /// pointer streams; the owner is the root's resolved id. Every
    /// class but [`KeyFilter::FOCUS`] is taken, so a popup under a modal does not
    /// dismiss on the same Escape; `FOCUS` stays out so Tab walks the overlay's own
    /// stops.
    pub fn claim(ui: &mut Ui, spec: OverlaySpec, root: &mut Widget) -> Self {
        if spec.backdrop.click_outside().is_some() {
            root.configure()
                .input_scope(KeyFilter::ALL.difference(KeyFilter::FOCUS))
                .blocks_below();
        }
        let owner = root.resolve(ui);
        let opened = ui.response_for(owner).rect.is_none();
        Self {
            owner,
            spec,
            opened,
        }
    }

    /// Lay the backdrop down, record `body` into the claimed layer, and report the overlay's
    /// frame: whether it opened, and how it closed. A [`Ui::close_overlay`] in the body closes it
    /// as [`OverlayClose::Requested`], which wins; else a press on the backdrop or Escape under
    /// [`ClickOutside::Dismiss`] closes it as [`OverlayClose::Dismissed`].
    ///
    /// Escape is read in here as the overlay's own scope, before the layer closes:
    /// afterwards the ambient scope sits below this overlay's and would silence it. Only a
    /// [`ClickOutside::Dismiss`] turn asks, as a read that closes nothing would re-arm a wake every
    /// frame. A barrier goes down first so it paints *under* the body (hit-testing
    /// runs in reverse, so the body still wins); it senses all four pointer
    /// interactions so pan, scroll and pinch cannot leak to the host, and only
    /// `Sense::CLICK` dismisses.
    pub fn record<R>(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> OverlayResponse<R> {
        let OverlaySpec {
            layer,
            anchor,
            backdrop,
        } = self.spec;
        if let Backdrop::Barrier(id, _) = backdrop {
            ui.layer(layer).show(|ui| {
                Widget::leaf()
                    .id(id)
                    .size((Sizing::FILL, Sizing::FILL))
                    .sense(Sense::ABSORB_POINTER)
                    .show(ui, None, |_| {});
            });
        }
        let dismisses = backdrop.click_outside() == Some(ClickOutside::Dismiss);
        ui.overlays().enter();
        let scope = ui.layer(layer);
        let scope = match anchor {
            Some(anchor) => scope.anchor(anchor),
            None => scope,
        };
        let mut escape_pressed = false;
        let inner = scope.show(|ui| {
            let inner = body(ui);
            escape_pressed = dismisses && ui.shortcut_pressed_as(self.owner, Key::Escape);
            inner
        });
        let requested = ui.overlays().leave();
        let dismissed = dismisses && (escape_pressed || self.backdrop_clicked(ui));
        OverlayResponse {
            id: self.owner,
            opened: self.opened,
            close: OverlayClose::of(requested, dismissed),
            inner,
        }
    }

    /// Whether a press landed on the backdrop rather than the body; all four
    /// pointer interactions are absorbed, so a secondary press counts (see
    /// `ResponseState::any_clicked`).
    fn backdrop_clicked(&self, ui: &Ui) -> bool {
        let id = match self.spec.backdrop {
            Backdrop::None => return false,
            Backdrop::Root(_) => self.owner,
            Backdrop::Barrier(id, _) => id,
        };
        ui.response_for(id).any_clicked()
    }

    /// End the turn with `response`: an overlay that closed gives its scope back, so no close
    /// path keeps owning input. **The current pass is unaffected**: scope paths resolve against
    /// a cascade a frame old, so merely ceasing to record would keep owning input one more frame
    /// and swallow the click where it was. Takes `self`, so the claim is spent.
    pub fn finish<R>(self, ui: &mut Ui, response: OverlayResponse<R>) -> OverlayResponse<R> {
        if response.closed() && self.spec.backdrop.click_outside().is_some() {
            ui.withdraw_input_scope(self.owner);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use crate::host::harness::Harness;
    use crate::input::keyboard::key::Key;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::layout::sizing::Sizing;
    use crate::scene::layer::Layer;
    use crate::ui::Ui;
    use crate::widget_core::click_outside::ClickOutside;
    use crate::widget_core::configure::Configure;
    use crate::widget_core::overlay_response::{OverlayClose, OverlayResponse};
    use crate::widget_core::overlay_scope::{Backdrop, OverlayScope, OverlaySpec};
    use crate::widget_core::widget::Widget;
    use glam::UVec2;

    /// A custom overlay over a root backdrop, its body closing through `Ui::close_overlay` when
    /// `close`.
    fn overlay(ui: &mut Ui, click_outside: ClickOutside, close: bool) -> OverlayResponse<()> {
        let mut root = Widget::zstack()
            .id(WidgetId::from_hash("test.overlay"))
            .size((Sizing::FILL, Sizing::FILL));
        let spec = OverlaySpec {
            layer: Layer::Modal,
            anchor: None,
            backdrop: Backdrop::Root(click_outside),
        };
        let scope = OverlayScope::claim(ui, spec, &mut root);
        scope.record(ui, |ui| {
            root.record(ui, None, |_| {});
            if close {
                ui.close_overlay();
            }
        })
    }

    /// The turn's close folds the body's request and the dismissal edges under the spec's policy:
    /// a request wins, Escape dismisses under `Dismiss` alone, and nothing closes a quiet turn.
    /// Only the first frame the overlay shows reads `opened`.
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
            let first = h.frame_value(|ui| overlay(ui, click_outside, false));
            assert!(first.opened, "{click_outside:?}: the first frame opens");
            h.frame(|ui| {
                overlay(ui, click_outside, false);
            });
            if escape {
                h.key(Key::Escape);
            }
            let got = h.frame_value(|ui| overlay(ui, click_outside, close));
            assert!(!got.opened, "{click_outside:?}: shown the frame before");
            assert_eq!(
                got.close, want,
                "{click_outside:?}, Escape {escape}, close {close}"
            );
        }
    }
}
