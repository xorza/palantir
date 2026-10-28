//! A backdrop pane's draw: its quad and who it is across frames.

use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::paint::backdrop_filter::Lens;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;

/// A pane's quad, `FillKind::BACKDROP` (see [`DrawQuadPayload::backdrop`]), the key its cached
/// blur hangs off, and its lens, which the quad's lanes have no room for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawPanePayload {
    pub(crate) quad: DrawQuadPayload,
    pub(crate) key: PaneKey,
    pub(crate) lens: Option<Lens>,
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
