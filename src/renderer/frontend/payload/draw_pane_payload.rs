//! A backdrop pane's draw: its quad and what the frame does with it.

use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::pane_captures::PaneTicket;
use crate::shape::paint::lowered_backdrop::LoweredBackdrop;

/// A pane's quad, `FillKind::BACKDROP` (see [`DrawQuadPayload::backdrop`]), the ticket the pane
/// ledger wrote for it, and its backdrop as recorded, for the lens and the luminance ceiling the
/// quad's lanes have no room for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawPanePayload {
    pub(crate) quad: DrawQuadPayload,
    pub(crate) ticket: PaneTicket,
    pub(crate) backdrop: LoweredBackdrop,
}

impl DrawPanePayload {
    /// This pane faded by `by`, as a quad fades.
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        Self {
            quad: self.quad.faded(by),
            ..self
        }
    }

    /// Whether the pane draws nothing, as a quad does.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.quad.is_noop()
    }
}
