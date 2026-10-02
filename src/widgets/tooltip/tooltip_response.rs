//! What a tooltip reports about its frame.

/// What one pass over a [`Tooltip`](crate::Tooltip) produced.
///
/// No [`Response`](crate::Response) here, unlike the other widget results:
/// the bubble senses nothing and does not record at all on the frames it is
/// down, so there is no node an application would ask about. Whether it is
/// up is the whole answer — for a trigger that wants to paint differently
/// while its hint is showing.
#[derive(Debug, Clone, Copy)]
pub struct TooltipResponse {
    /// The bubble recorded this frame.
    pub visible: bool,
}
