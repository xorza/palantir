#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and colour channels"
)]

use crate::input::scroll_targets::ScrollTargets;
use crate::widgets::block::Block;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::state::ScrollState;
use crate::widgets::text_edit::tests::*;
use std::fmt::Write;

/// Fixed-size editor: scroll stays at zero while text fits, grows to keep the
/// caret visible on overflow, and snaps back when the caret returns home. Mono
/// fallback (8 px/char at 16 px font, 1.5 px caret). Inner width is
/// `280 - 2*(5 + 1.5)` = 267 px: theme padding 5 px plus the 1.5 px stroke that
/// `Tree::open_node` folds into padding, which TextEdit mirrors.
#[test]
fn scroll_keeps_caret_inside_visible_inner_rect() {
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("scroll-ed"))
                .size((Sizing::fixed(280.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    }

    let ed_id = WidgetId::from_hash("scroll-ed");

    let mut h = UiHarness::new(NARROW);

    let mut buf = String::from("hello");
    h.frame(|ui| body(ui, &mut buf));
    h.ui.with_state::<TextEditState, _>(ed_id, |_, s| s.edit.caret = 5);
    h.frame(|ui| body(ui, &mut buf));
    let scroll = h.state::<TextEditState>(ed_id).view.scroll.offset;
    assert_eq!(scroll, Vec2::ZERO, "text fits — no scroll");

    // Long text: caret at end (100) -> x = 800 px. The trailing clamp leaves a
    // caret-width sliver: scroll.x = (800 + 1.5) - (267 - 1.5) = 536.
    let mut long = "a".repeat(100);
    h.frame(|ui| body(ui, &mut long));
    h.ui.with_state::<TextEditState, _>(ed_id, |_, s| s.edit.caret = 100);
    h.frame(|ui| body(ui, &mut long));
    let scroll = h.state::<TextEditState>(ed_id).view.scroll.offset;
    assert_eq!(scroll.x, 536.0, "scroll.x = {}", scroll.x);
    assert_eq!(scroll.y, 0.0, "single-line never scrolls y");

    // Caret home: scroll.x snaps back to show the text start.
    h.ui.with_state::<TextEditState, _>(ed_id, |_, s| s.edit.caret = 0);
    h.frame(|ui| body(ui, &mut long));
    let scroll = h.state::<TextEditState>(ed_id).view.scroll.offset;
    assert_eq!(scroll.x, 0.0, "scroll snaps to 0 when caret moves home");
}

/// A `Hug`-width single-line editor shows its *whole* text as it grows, with no
/// permanent left clip. Pins (1) the editor reserves the caret sliver in its
/// desired width, and (2) `update_scroll` upper-clamps `scroll.x` to the content
/// end so the transient scroll from the one-frame-stale rect settles to zero.
/// Mono fallback: 8 px/char at 16 px, 1.5 px caret, 5 px padding + 1.5 px stroke.
#[test]
fn hug_width_editor_shows_full_text_after_growth() {
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("hug-ed"))
                .size((Sizing::HUG, Sizing::fixed(40.0)))
                .show(ui);
        });
    }

    let ed_id = WidgetId::from_hash("hug-ed");

    let mut h = UiHarness::new(WIDE);

    let mut buf = String::from("1");
    h.frame(|ui| body(ui, &mut buf));
    h.ui.with_state::<TextEditState, _>(ed_id, |_, s| s.edit.caret = 1);
    h.frame(|ui| body(ui, &mut buf));

    // Grow the buffer with the caret pinned at the end. The first frame sees last
    // frame's narrower rect and scrolls left chasing the caret: the transient.
    buf.push_str("2345");
    h.ui.with_state::<TextEditState, _>(ed_id, |_, s| s.edit.caret = buf.len());
    h.frame(|ui| body(ui, &mut buf));
    // Settle: the widened rect is now visible to update_scroll.
    h.frame(|ui| body(ui, &mut buf));

    let scroll = h.state::<TextEditState>(ed_id).view.scroll.offset;
    assert_eq!(
        scroll.x, 0.0,
        "hug editor must show its whole text (no left clip); scroll.x = {}",
        scroll.x,
    );
}

/// After horizontal scroll, clicking the widget's left edge must hit the byte
/// *visibly* there, not byte 0: the input pass adds `state.scroll` back into
/// hit-test coords.
#[test]
fn click_hit_test_compensates_for_scroll() {
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("hit-ed"))
                .size((Sizing::fixed(280.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    }

    let ed_id = WidgetId::from_hash("hit-ed");

    let mut h = UiHarness::new(NARROW);
    let mut buf = "a".repeat(100);

    h.frame(|ui| body(ui, &mut buf));
    h.ui.with_state::<TextEditState, _>(ed_id, |_, s| s.edit.caret = 100);
    h.frame(|ui| body(ui, &mut buf));
    let scroll_x = h.state::<TextEditState>(ed_id).view.scroll.offset.x;
    assert!(scroll_x > 100.0, "precondition: editor is scrolled");

    // Click 8 px in (the inner rect's left edge). With scroll compensation, mono
    // hit-test sees x = scroll_x (~537.5), byte ~67 (scroll_x / 8); without, byte 0.
    h.press_at(Vec2::new(8.0, 20.0));
    h.frame(|ui| body(ui, &mut buf));
    h.release();
    h.frame(|ui| body(ui, &mut buf));

    let caret = h.state::<TextEditState>(ed_id).edit.caret;
    let expected = (scroll_x / 8.0).round() as usize;
    assert!(
        caret.abs_diff(expected) <= 1,
        "click should land near byte {expected} (visible left edge), got {caret}",
    );
}

/// The wheel pans a multi-line editor whose content overflows, and the caret
/// does not immediately drag the view back. Pins that the node carries a wheel
/// sense (else the delta never arrives) and that caret-follow is conditional
/// (else it undoes the scroll next frame).
///
/// Mono wraps by character count, so the fixture is sized by bytes: ~600 chars
/// over a ~33-char line is ~18 wrapped lines at 16 px, past the ~87 px inner
/// height of a 100 px editor.
#[test]
fn wheel_pans_a_multiline_editor_and_the_caret_does_not_snap_it_back() {
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("wheel-ed"))
                .multiline(true)
                .size((Sizing::fixed(280.0), Sizing::fixed(100.0)))
                .show(ui);
        });
    }

    let ed_id = WidgetId::from_hash("wheel-ed");

    let mut h = UiHarness::new(NARROW);
    let mut buf = String::new();
    for i in 0..100 {
        writeln!(buf, "line{i}").unwrap();
    }

    // Caret at the top, so a caret-follow would pull the view to zero and the
    // assertions couldn't pass by accident.
    h.frame(|ui| body(ui, &mut buf));
    h.ui.with_state::<TextEditState, _>(ed_id, |_, s| s.edit.caret = 0);
    h.frame(|ui| body(ui, &mut buf));
    assert_eq!(
        h.state::<TextEditState>(ed_id).view.scroll.offset.y,
        0.0,
        "precondition: caret at the top holds the view at the top",
    );

    // One wheel gesture over the editor.
    h.scroll_pixels_at(Vec2::new(140.0, 50.0), Vec2::new(0.0, 48.0));
    h.frame(|ui| body(ui, &mut buf));
    let scrolled = h.state::<TextEditState>(ed_id).view.scroll.offset.y;
    assert!(
        scrolled > 0.0,
        "wheel over a multi-line editor must pan it; scroll.y = {scrolled}",
    );

    // A frame with no input and no caret movement leaves the view where the wheel
    // put it, which a caret-follow-every-frame would break.
    h.frame(|ui| body(ui, &mut buf));
    assert_eq!(
        h.state::<TextEditState>(ed_id).view.scroll.offset.y,
        scrolled,
        "an idle frame must not drag the view back to the caret",
    );

    // Moving the caret *does* pull the view back: the wheel didn't disable it.
    h.ui.with_state::<TextEditState, _>(ed_id, |_, s| s.edit.caret = 0);
    h.ui.with_state::<TextEditState, _>(ed_id, |_, s| s.edit.caret = 1);
    h.frame(|ui| body(ui, &mut buf));
    assert_eq!(
        h.state::<TextEditState>(ed_id).view.scroll.offset.y,
        0.0,
        "a caret move must scroll it back into view",
    );
}

/// Each wheel axis goes to the topmost row that pans it, as in browser scroll
/// chaining. A single-line field pans only x: vertical wheel scrolls the page
/// behind it, horizontal pans the field, even while its text overflows.
#[test]
fn a_vertical_wheel_over_a_field_scrolls_the_page_behind_it() {
    let page = WidgetId::from_hash("page");
    let ed_id = WidgetId::from_hash("chained-ed");
    let body = |ui: &mut Ui, buf: &mut String| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Scroll::vertical()
                .id(page)
                .size((Sizing::fixed(300.0), Sizing::fixed(200.0)))
                .show(ui, |ui| {
                    TextEdit::new(buf)
                        .id(ed_id)
                        .size((Sizing::fixed(280.0), Sizing::fixed(40.0)))
                        .show(ui);
                    Block::new()
                        .id_salt("filler")
                        .size((Sizing::fixed(280.0), Sizing::fixed(600.0)))
                        .show(ui);
                });
        });
    };
    let mut h = UiHarness::new(NARROW);
    // 100 glyphs at 8 px overflow the 267 px inner width.
    let mut long = "a".repeat(100);
    h.prime(2, |ui| body(ui, &mut long));
    h.move_to(Vec2::new(50.0, 20.0));
    assert_eq!(
        h.ui.input().scroll_targets,
        ScrollTargets {
            x: Some(ed_id),
            y: Some(page),
        },
    );

    h.scroll_pixels(Vec2::new(0.0, 10.0));
    h.frame(|ui| body(ui, &mut long));
    assert_eq!(h.state::<ScrollState>(page).offset, Vec2::new(0.0, 10.0));
    assert_eq!(
        h.state::<TextEditState>(ed_id).view.scroll.offset,
        Vec2::ZERO
    );

    // The page moved the field up 10 px, to y -10..30, so (50, 20) is still on it.
    h.scroll_pixels(Vec2::new(30.0, 0.0));
    h.frame(|ui| body(ui, &mut long));
    assert_eq!(h.state::<ScrollState>(page).offset, Vec2::new(0.0, 10.0));
    assert_eq!(
        h.state::<TextEditState>(ed_id).view.scroll.offset,
        Vec2::new(30.0, 0.0),
    );
}

/// With no row under the pointer that pans y, a single-line field takes a plain
/// vertical wheel turn as horizontal movement (done by routing; the field reads
/// only x). A field whose text fits senses no wheel axis.
#[test]
fn a_lone_field_with_overflowing_text_pans_on_a_vertical_wheel() {
    let ed_id = WidgetId::from_hash("lone-ed");
    let body = |ui: &mut Ui, buf: &mut String| {
        Panel::vstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(ed_id)
                .size((Sizing::fixed(280.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    };
    let mut h = UiHarness::new(NARROW);
    let mut short = String::from("hello");
    h.prime(2, |ui| body(ui, &mut short));
    h.move_to(Vec2::new(50.0, 20.0));
    assert_eq!(
        h.ui.input().scroll_targets,
        ScrollTargets::default(),
        "five glyphs fit, so the field leaves the wheel to what is behind it",
    );

    let mut long = "a".repeat(100);
    h.prime(2, |ui| body(ui, &mut long));
    assert_eq!(
        h.ui.input().scroll_targets,
        ScrollTargets {
            x: Some(ed_id),
            y: None,
        },
    );
    h.scroll_pixels(Vec2::new(0.0, 50.0));
    h.frame(|ui| body(ui, &mut long));
    assert_eq!(
        h.state::<TextEditState>(ed_id).view.scroll.offset,
        Vec2::new(50.0, 0.0),
    );
}
