//! [`moved_since_press`]: whether a press's gesture left a value where it found it.

use crate::primitives::input::interaction::response::Response;
use crate::ui::Ui;

/// The value a press found, held in the widget's state row while the gesture lasts.
#[derive(Debug)]
struct PressStart<T>(T);

/// Whether `response`'s frame is the left press's release and leaves `after` apart from where
/// the press found it: `before` is held at the press edge, before the press writes, and compared
/// at the release, which drops it; any other frame answers false. A release whose press no frame
/// of this widget saw counts as moved, as nothing says otherwise.
pub(crate) fn moved_since_press<T: Copy + PartialEq + 'static>(
    ui: &mut Ui,
    response: &Response,
    before: T,
    after: T,
) -> bool {
    let id = response.id;
    if response.left.pressed() {
        ui.set_state(id, PressStart(before));
    }
    if !response.left.released() {
        return false;
    }
    ui.remove_state::<PressStart<T>>(id)
        .is_none_or(|start| start.0 != after)
}
