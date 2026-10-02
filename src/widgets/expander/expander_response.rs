//! What an expander reports about its frame.

use crate::widgets::response::Response;

/// What one pass over an [`Expander`](crate::Expander) produced.
#[derive(Debug)]
pub struct ExpanderResponse<'a, R> {
    /// The header's response — the whole row is the hit target.
    pub response: Response<'a>,
    /// What the body closure returned, or `None` on a frame the body did
    /// not record. A collapsed [`Expander::keep_body`](crate::Expander::keep_body) section still
    /// records, so it still answers `Some`.
    pub inner: Option<R>,
    /// The header was activated this frame, by click or by key.
    pub toggled: bool,
    /// `0.0` closed, `1.0` open, in between while the reveal animates.
    pub openness: f32,
}
