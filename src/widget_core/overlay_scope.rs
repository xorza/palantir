//! [`OverlayScope`] — a dismissible overlay's claim on its layer.

use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
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
    /// A full-surface eater [`OverlayScope::record`] lays down under this id ahead
    /// of the body; a *placed* overlay cannot nest its body in its backdrop, as
    /// placement flips it to fit.
    Eater(WidgetId),
}

impl Backdrop {
    const fn owns_input(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// What one [`OverlayScope::record`] saw: the body's result and the two dismissal edges.
#[derive(Debug)]
#[must_use]
pub struct OverlayTurn<R> {
    /// What the body returned.
    pub inner: R,
    /// Escape was pressed while the overlay held the keys; always `false` without a backdrop.
    pub escape: bool,
    /// A press landed on the backdrop rather than the body; always `false` without one.
    pub outside: bool,
}

/// Where an overlay records and what stands under it: everything [`OverlayScope::claim`] takes
/// past the root.
#[derive(Clone, Copy, Debug)]
pub struct OverlaySpec {
    /// Where the body records; held by the scope so scope and body cannot land on different
    /// layers.
    pub layer: Layer,
    /// `None` takes the layer's default: the surface origin, whole surface available.
    pub anchor: Option<Anchor>,
    /// What stands between the body and the layers below it.
    pub backdrop: Backdrop,
}

/// One overlay's turn on a layer, from stamping its root until it closes.
/// [`Popup`](crate::Popup), [`Modal`](crate::Modal) and [`Tooltip`](crate::Tooltip)
/// share this lifecycle; two steps have ordering constraints (see [`Self::record`],
/// [`Self::withdraw`]). A scope silences the layers strictly *below* its own, never
/// its own body, so a `TextEdit` in a popup keeps the keyboard.
///
/// A custom overlay follows the same order: [`Self::claim`] with the root before
/// recording, [`Self::record`] the body, then [`Self::withdraw`] on the frame it
/// closes.
#[derive(Debug)]
pub struct OverlayScope {
    owner: WidgetId,
    spec: OverlaySpec,
}

impl OverlayScope {
    /// Claim `spec.layer` for `root`, stamping it as the node taking the layer's key
    /// scope when there is a backdrop; the owner is the root's resolved id. Every
    /// class but [`KeyFilter::FOCUS`] is taken, so a popup under a modal does not
    /// dismiss on the same Escape; `FOCUS` stays out so Tab walks the overlay's own
    /// stops.
    pub fn claim(ui: &mut Ui, spec: OverlaySpec, root: &mut Widget) -> Self {
        if spec.backdrop.owns_input() {
            root.configure()
                .input_scope(KeyFilter::ALL.difference(KeyFilter::FOCUS));
        }
        let owner = root.resolve(ui);
        Self { owner, spec }
    }

    /// Lay the backdrop down, record `body` into the claimed layer, and report both
    /// dismissal edges.
    ///
    /// Escape is read in here, before the layer closes: afterwards the ambient
    /// scope sits below this overlay's and would silence it. A backdrop-less scope
    /// reports `false` without asking, as `key_pressed` would re-arm a wake every
    /// frame. An eater goes down first so it paints *under* the body (hit-testing
    /// runs in reverse, so the body still wins); it senses all four pointer
    /// interactions so pan, scroll and pinch cannot leak to the host, and only
    /// `Sense::CLICK` dismisses.
    pub fn record<R>(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> OverlayTurn<R> {
        let OverlaySpec {
            layer,
            anchor,
            backdrop,
        } = self.spec;
        if let Backdrop::Eater(id) = backdrop {
            ui.layer(layer).show(|ui| {
                Widget::leaf()
                    .id(id)
                    .size((Sizing::FILL, Sizing::FILL))
                    .sense(Sense::ABSORB_POINTER)
                    .show(ui, None, |_| {});
            });
        }
        let owns_input = backdrop.owns_input();
        let scope = ui.layer(layer);
        let scope = match anchor {
            Some(anchor) => scope.anchor(anchor),
            None => scope,
        };
        let mut escape = false;
        let inner = scope.show(|ui| {
            let inner = body(ui);
            escape = owns_input && ui.key_pressed(Shortcut::key(Key::Escape));
            inner
        });
        OverlayTurn {
            inner,
            escape,
            outside: self.backdrop_clicked(ui),
        }
    }

    /// Whether a press landed on the backdrop rather than the body; all four
    /// pointer interactions are absorbed, so a secondary press counts (see
    /// `ResponseState::any_clicked`).
    fn backdrop_clicked(&self, ui: &Ui) -> bool {
        let id = match self.spec.backdrop {
            Backdrop::None => return false,
            Backdrop::Root => self.owner,
            Backdrop::Eater(id) => id,
        };
        ui.response_for(id).any_clicked()
    }

    /// Give the scope back once the overlay has resolved that it closed; an overlay
    /// still open drops its scope instead. **The current pass is unaffected**: scope
    /// paths resolve against a cascade a frame old, so merely ceasing to record would
    /// keep owning input one more frame and swallow the click where it was. Takes
    /// `self`, so the claim is spent.
    pub fn withdraw(self, ui: &mut Ui) {
        if self.spec.backdrop.owns_input() {
            ui.release_input_scope(self.owner);
        }
    }
}
