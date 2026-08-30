//! What a pick-one widget reports about the selection it writes through.

use crate::widgets::response::Response;

/// What a widget that picks one option out of several reports about the
/// index it writes through.
///
/// Separate from [`ValueResponse`](crate::ValueResponse) because a pick
/// has no draft: the selection is written by the click that makes it, so
/// `changed` is already the commit and a second signal would repeat it.
///
/// The [`Response`] is the *trigger*'s. A [`ComboBox`](crate::ComboBox)
/// writes from a row inside its dropdown, so the trigger's own
/// `clicked()` reports that the list opened and never that the selection
/// moved — which is the whole reason this type exists.
#[derive(Debug)]
pub struct SelectResponse<'a> {
    pub response: Response<'a>,
    /// A different option was chosen this frame. Re-picking the option
    /// already selected leaves it `false`.
    pub changed: bool,
}
