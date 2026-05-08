use crate::primitives::rect::Rect;
use crate::tree::Layer;
use crate::tree::element::Configure;
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::panel::Panel;
use crate::widgets::theme::Surface;

/// What happens when the user presses outside the popup's body.
///
/// `Block` = clicks outside are ignored; the popup stays open until the
/// host explicitly closes it. `Dismiss` = clicks outside signal
/// dismissal; the host reads the signal off [`Response`] and flips its
/// open flag.
///
/// v1 stores the flag but does not yet emit the dismissal signal — see
/// `docs/popups.md` step 4. Both modes currently behave like `Block`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClickOutside {
    Block,
    Dismiss,
}

/// A side-layer container anchored to a screen rect. The body records
/// into the `Popup` layer so it draws above all `Main` siblings,
/// escapes ancestor clip, and hit-tests on top.
///
/// `anchor` is a caller-supplied screen rect — typically a trigger
/// widget's last-frame `Response.state.rect`. The popup's first frame
/// after opening is one frame stale (matches `Scroll`'s wheel-pan
/// posture); subsequent frames track the trigger.
///
/// Must be called at top-level recording (no node currently open). v1
/// rejects mid-`Panel::show` calls with a clear assert; the egui-style
/// pattern is to record `Main` content first and call
/// `Popup::anchored_to(...).show(ui, ...)` after the outer scope
/// closes. See the `Mid-recording layer changes` section of
/// `docs/popups.md` for v2 paths.
pub struct Popup {
    anchor: Rect,
    surface: Option<Surface>,
    padding: f32,
    click_outside: ClickOutside,
}

impl Popup {
    #[track_caller]
    pub fn anchored_to(anchor: Rect) -> Self {
        Self {
            anchor,
            surface: None,
            padding: 0.0,
            click_outside: ClickOutside::Dismiss,
        }
    }

    pub fn background(mut self, s: impl Into<Surface>) -> Self {
        self.surface = Some(s.into());
        self
    }

    pub fn padding(mut self, p: f32) -> Self {
        self.padding = p;
        self
    }

    pub fn click_outside(mut self, m: ClickOutside) -> Self {
        self.click_outside = m;
        self
    }

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> Response {
        let _ = self.click_outside;
        let surface = self.surface;
        let padding = self.padding;
        let mut response: Option<Response> = None;
        ui.layer(Layer::Popup, self.anchor, |ui| {
            let mut panel = Panel::vstack().padding(padding);
            if let Some(s) = surface {
                panel = panel.background(s);
            }
            response = Some(panel.show(ui, body));
        });
        response.expect("popup body did not record a root widget")
    }
}
