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
use crate::widgets::block::Block;

/// What stands between an overlay's body and the layers below it. Pointer and keys
/// are one decision: an overlay takes both or neither.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Backdrop {
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

#[derive(Debug)]
#[must_use]
pub(crate) struct OverlayTurn<R> {
    pub(crate) inner: R,
    pub(crate) escape: bool,
    pub(crate) outside: bool,
}

/// One overlay's turn on a layer, from stamping its root until it closes.
/// [`Popup`](crate::Popup), [`Modal`](crate::Modal) and [`Tooltip`](crate::Tooltip)
/// share this lifecycle; two steps have ordering constraints (see [`Self::record`],
/// [`Self::withdraw`]). A scope silences the layers strictly *below* its own, never
/// its own body, so a `TextEdit` in a popup keeps the keyboard.
#[derive(Debug)]
pub(crate) struct OverlayScope {
    owner: WidgetId,
    /// Where the body records; retained so scope and body cannot land on different
    /// layers.
    layer: Layer,
    /// `None` takes the layer's default: the surface origin, whole surface
    /// available.
    anchor: Option<Anchor>,
    backdrop: Backdrop,
}

impl OverlayScope {
    /// Claim `layer` for `root`, stamping it as the node taking the layer's key
    /// scope when there is a backdrop; the owner is the root's resolved id. Every
    /// class but [`KeyFilter::FOCUS`] is taken, so a popup under a modal does not
    /// dismiss on the same Escape; `FOCUS` stays out so Tab walks the overlay's own
    /// stops.
    pub(crate) fn claim(
        ui: &mut Ui,
        layer: Layer,
        anchor: Option<Anchor>,
        backdrop: Backdrop,
        root: &mut Widget,
    ) -> Self {
        if backdrop.owns_input() {
            root.configure()
                .input_scope(KeyFilter::ALL.difference(KeyFilter::FOCUS));
        }
        let owner = root.resolve(ui);
        Self {
            owner,
            layer,
            anchor,
            backdrop,
        }
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
    pub(crate) fn record<R>(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> OverlayTurn<R> {
        if let Backdrop::Eater(id) = self.backdrop {
            ui.layer(self.layer).show(|ui| {
                Block::new()
                    .id(id)
                    .size((Sizing::FILL, Sizing::FILL))
                    .sense(Sense::ABSORB_POINTER)
                    .show(ui);
            });
        }
        let owns_input = self.backdrop.owns_input();
        let scope = ui.layer(self.layer);
        let scope = match self.anchor {
            Some(anchor) => scope.anchor(anchor),
            None => scope,
        };
        let (inner, escape) = scope.show(|ui| {
            let inner = body(ui);
            (
                inner,
                owns_input && ui.key_pressed(Shortcut::key(Key::Escape)),
            )
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
        let id = match self.backdrop {
            Backdrop::None => return false,
            Backdrop::Root => self.owner,
            Backdrop::Eater(id) => id,
        };
        ui.response_for(id).any_clicked()
    }

    /// Give the scope back once the overlay has resolved that it closed. **The
    /// current pass is unaffected**: scope paths resolve against a cascade a frame
    /// old, so merely ceasing to record would keep owning input one more frame and
    /// swallow the click where it was. Takes `self`, so the claim is spent.
    pub(crate) fn withdraw(self, ui: &mut Ui, closed: bool) {
        if closed && self.backdrop.owns_input() {
            ui.release_input_scope(self.owner);
        }
    }
}
