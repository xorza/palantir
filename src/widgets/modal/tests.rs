use crate::Ui;
use crate::input::keyboard::key::Key;
use crate::primitives::background::Background;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::scene::tree::node_id::NodeId;
use crate::ui::harness::UiHarness;
use crate::widgets::modal::Modal;
use crate::widgets::popup::Popup;
use glam::{UVec2, Vec2};

#[test]
fn explicit_zero_padding_and_minimum_override_card_theme() {
    let mut h = UiHarness::new(UVec2::new(400, 300));
    let root_id = WidgetId::from_hash("modal-explicit-zero");
    h.frame(|ui| {
        Modal::new()
            .id(root_id)
            .background(Background::NONE)
            .padding(Spacing::ZERO)
            .min_size(Size::ZERO)
            .show(ui, |_| {});
    });

    let card_id = root_id.with("card");
    let tree = h.ui.tree(Layer::Modal);
    let index = tree
        .records
        .widget_id()
        .iter()
        .position(|id| *id == card_id)
        .expect("modal card node");
    let node = NodeId(index as u32);
    assert_eq!(tree.records.layout()[index].padding, Spacing::ZERO);
    assert_eq!(tree.bounds(node).min_size, Size::ZERO);
}

/// A modal paints above every popup and eats pointer input through
/// its backdrop, so it must also be the one that hears Escape. That
/// only holds while the modal's scope outranks the popup's: a popup
/// holding keyboard capture instead empties the uncaptured stream
/// the modal reads from, leaving it undismissable for as long as the
/// popup stays open.
///
/// The control matters as much as the case — with no popup open the
/// modal has always dismissed, so asserting only the popup case
/// would not distinguish "layer ordering works" from "Escape works".
#[test]
fn modal_hears_escape_even_while_a_popup_below_holds_keyboard_claim() {
    fn escape_dismisses(with_popup: bool) -> bool {
        const SURFACE: UVec2 = UVec2::new(400, 300);
        let scene = |ui: &mut Ui, dismissed: &mut bool| {
            if with_popup {
                Popup::anchored_to(Vec2::ZERO)
                    .id(WidgetId::from_hash("under-modal"))
                    .show(ui, |_ui, _handle| {});
            }
            *dismissed |= Modal::new()
                .id(WidgetId::from_hash("modal-escape"))
                .show(ui, |_| {})
                .dismissed;
        };

        // Two frames: the keyboard wake-gate parks a press whose
        // shortcut nobody watched yet, so the first frame is what
        // registers the modal's interest in Escape and the press lands
        // after it.
        //
        // `|=` rather than `=` because a frame may record twice, and
        // `post_record` drains the key queue between passes — so pass B
        // sees no Escape and would overwrite pass A's result. Real
        // callers have the same shape: they flip an `open` flag on
        // dismissal rather than reading the last pass's return.
        let mut h = UiHarness::new(SURFACE);
        let mut dismissed = false;
        h.frame(|ui| scene(ui, &mut dismissed));
        h.key(Key::Escape);
        let mut dismissed = false;
        h.frame(|ui| scene(ui, &mut dismissed));
        dismissed
    }

    assert!(
        escape_dismisses(false),
        "control: a modal with no popup open must dismiss on Escape",
    );
    assert!(
        escape_dismisses(true),
        "a popup's keyboard capture must not silence the modal above it",
    );
}

/// A dismissed modal hands both streams back on the *next* frame,
/// not the one after.
///
/// Claims resolve at the end of a record pass and are read by the
/// following one, so a dismissing frame's claim can outlive the
/// overlay by a frame — long enough to swallow the click that lands
/// where the modal used to be.
///
/// **This does not pin `Ui::close_scope`**, and the difference is
/// worth recording: dismissal is action input, action input forces a
/// second record pass, and that pass re-records without the modal —
/// so the claim is already gone by `take_action_flag` whether or not
/// anything released it. Verified by disabling `Modal`'s release and
/// watching this still pass. The release is kept because it is
/// correct on a single-pass dismissal and because `Popup` has always
/// done it, not because it is observable here. `release` itself is
/// pinned directly in `input::tests::keyboard`.
///
/// What this *does* guard is the end-to-end contract, which would
/// break if the resolution timing or the replay ever changed. Both
/// streams in one test because their lifecycles are now one thing.
#[test]
fn a_dismissed_modal_stops_owning_input_on_the_very_next_frame() {
    use crate::input::watch::{KeyboardWake, PointerWake};
    const SURFACE: UVec2 = UVec2::new(400, 300);

    // Watches so `Main` has something to be cut off from, plus the
    // modal for as long as `open` says so.
    let scene = |ui: &mut Ui, open: &mut bool| {
        ui.watch_pointer(PointerWake::BUTTONS);
        ui.watch_keyboard(KeyboardWake::KEY);
        if *open {
            *open = !Modal::new()
                .id(WidgetId::from_hash("closing-modal"))
                .show(ui, |_| {})
                .dismissed;
        }
    };

    let mut h = UiHarness::new(SURFACE);
    let mut open = true;
    h.frame(|ui| scene(ui, &mut open));

    // Escape dismisses it during this frame's record.
    h.key(Key::Escape);
    h.frame(|ui| scene(ui, &mut open));
    assert!(!open, "Escape must dismiss the modal");

    // The frame after. A `Main`-layer widget presses and types; both
    // must reach it during the record, which is when widgets read.
    h.press_at(Vec2::new(20.0, 20.0));
    h.key(Key::Char('a'));
    // `max`, not `=`: a frame may record twice and `post_record`
    // drains the queues between passes, so pass B legitimately sees
    // nothing and would overwrite pass A's reading. Same hazard the
    // `|=` in `modal_hears_escape_…` guards against.
    let mut pointer = 0;
    let mut keyboard = 0;
    h.frame(|ui| {
        scene(ui, &mut open);
        pointer = pointer.max(ui.pointer_events().len());
        keyboard = keyboard.max(ui.keyboard_events().len());
    });
    assert_eq!(pointer, 1, "the dismissed modal still held the pointer");
    assert_eq!(keyboard, 1, "the dismissed modal still held the keyboard");
}

/// Exactly one overlay may act on a given Escape.
///
/// Layer-ordered *reads* alone were not enough: the modal saw Escape
/// and so did the popup that held capture, so one keypress closed
/// both. Ownership is now resolved topmost-first, so the modal takes
/// the keyboard and the popup beneath it sees nothing at all.
#[test]
fn escape_closes_only_the_topmost_overlay() {
    const SURFACE: UVec2 = UVec2::new(400, 300);
    let scene = |ui: &mut Ui, modal: &mut bool, popup: &mut bool| {
        *popup |= Popup::anchored_to(Vec2::ZERO)
            .id(WidgetId::from_hash("under-modal"))
            .show(ui, |_ui, _handle| {})
            .dismissed;
        *modal |= Modal::new()
            .id(WidgetId::from_hash("over-popup"))
            .show(ui, |_| {})
            .dismissed;
    };

    let mut h = UiHarness::new(SURFACE);
    let (mut m, mut p) = (false, false);
    h.frame(|ui| scene(ui, &mut m, &mut p));
    h.key(Key::Escape);
    let (mut modal_closed, mut popup_closed) = (false, false);
    h.frame(|ui| scene(ui, &mut modal_closed, &mut popup_closed));

    assert!(modal_closed, "the topmost overlay must take the Escape");
    assert!(
        !popup_closed,
        "the popup beneath the modal must not also consume it",
    );
}
