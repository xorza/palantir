//! Click-to-edit: the draft buffer, and every way it can end.

use crate::Ui;
use crate::input::keyboard::key::Key;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widgets::drag_value::tests::support::deferred_frame;
use crate::widgets::drag_value::{DragValue, DragValueState};
use glam::{UVec2, Vec2};

#[test]
fn click_to_edit_types_and_commits_on_enter() {
    // The real pointer path: a plain click opens the editor seeded from
    // the current value; typing + Enter commits once.
    let id = WidgetId::from_hash("dv-click-edit");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 5.0_f64;
    deferred_frame(&mut h, id, &mut canonical, true, false);

    h.press_at(Vec2::new(50.0, 20.0));
    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(!s.a().committed, "the click itself commits nothing");

    // Editor frame: entry seeds the buffer from the value.
    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(!s.a().committed);
    assert_eq!(edit_buffer(&mut h.ui, id), "5.0", "seeded on entry");

    // First keystroke replaces the select-all'd seed; second appends.
    h.key(Key::Char('7'));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    h.key(Key::Char('2'));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    assert_eq!(canonical, 5.0, "typing is a live draft, not a commit");

    h.key(Key::Enter);
    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(
        s.a().committed && s.count_where(|e| e.committed) == 1,
        "Enter commits once"
    );
    assert_eq!(canonical, 72.0);

    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(!s.a().committed, "commit is a one-frame edge");
}

/// Escape ends the edit with the value it opened on and commits nothing.
/// A live binding holds the typed 42 until then, so the revert is a
/// change; a deferred caller never adopted 42, so it sees neither.
#[test]
fn escape_reverts_the_draft_without_a_commit() {
    let id = WidgetId::from_hash("dv-escape-revert");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 5.0_f64;
    deferred_frame(&mut h, id, &mut canonical, true, false);
    h.set_focus(id);
    deferred_frame(&mut h, id, &mut canonical, true, false);
    for c in ['4', '2'] {
        h.key(Key::Char(c));
        deferred_frame(&mut h, id, &mut canonical, true, false);
    }
    h.key(Key::Escape);
    for _ in 0..2 {
        let s = deferred_frame(&mut h, id, &mut canonical, true, false);
        assert!(
            !s.a().changed && !s.a().committed && s.count_where(|e| e.committed) == 0,
            "{s:?}"
        );
    }
    assert_eq!(canonical, 5.0);
    assert!(h.ui.focused_id().is_none(), "Escape blurs");

    let live_id = WidgetId::from_hash("dv-escape-live");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut value = 5.0_f64;
    let live = |h: &mut UiHarness, value: &mut f64| {
        h.frame_value(|ui| {
            let r = DragValue::new(&mut *value)
                .editable(true)
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .id(live_id)
                .show(ui);
            (r.changed, r.committed)
        })
    };
    live(&mut h, &mut value);
    h.set_focus(live_id);
    live(&mut h, &mut value);
    for c in ['4', '2'] {
        h.key(Key::Char(c));
        live(&mut h, &mut value);
    }
    assert_eq!(value, 42.0, "the typed text is written live");
    h.key(Key::Escape);
    assert_eq!(live(&mut h, &mut value), (true, false), "the revert");
    assert_eq!(value, 5.0);
    assert_eq!(live(&mut h, &mut value), (false, false), "no residue");
}

#[test]
fn programmatic_focus_seeds_a_fresh_buffer() {
    // Regression: the buffer used to be seeded only by the click path, so
    // set_focus re-opened the previous session's stale text and
    // committed it over an externally-changed value.
    let id = WidgetId::from_hash("dv-fresh-seed");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 5.0_f64;
    deferred_frame(&mut h, id, &mut canonical, true, false);

    // First session commits 42 and leaves "42" in the buffer state.
    h.set_focus(id);
    deferred_frame(&mut h, id, &mut canonical, true, false);
    h.key(Key::Char('4'));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    h.key(Key::Char('2'));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    h.key(Key::Enter);
    deferred_frame(&mut h, id, &mut canonical, true, false);
    assert_eq!(canonical, 42.0);

    // The value changes externally; a new focus must show 99, not 42.
    canonical = 99.0;
    h.set_focus(id);
    deferred_frame(&mut h, id, &mut canonical, true, false);
    assert_eq!(edit_buffer(&mut h.ui, id), "99.0");

    h.key(Key::Enter);
    deferred_frame(&mut h, id, &mut canonical, true, false);
    assert_eq!(canonical, 99.0, "no stale-buffer revert to 42");
}

#[test]
fn focusing_mid_scrub_cannot_overwrite_the_typed_commit() {
    // Edit entry must replace the scrub state; otherwise its release can
    // overwrite the typed value with the stale scrubbed result.
    let id = WidgetId::from_hash("dv-latch-vs-edit");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 10.0_f64;
    deferred_frame(&mut h, id, &mut canonical, true, false);

    // Scrub 10 → 30, then focus the editor mid-drag.
    h.press_at(Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    h.set_focus(id);
    deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(matches!(
        h.state::<DragValueState>(id),
        DragValueState::Editing { .. }
    ));

    // The release lands on an editor frame, so no scrub commit may surface
    // now or later.
    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(
        !s.a().committed,
        "disarmed scrub must not commit into the edit"
    );

    // A typed draft (hand-set: the still-held-then-released button placed
    // a caret, so simulated keystrokes wouldn't select-all-replace here;
    // the typed path is covered by `click_to_edit_types_and_commits_on_enter`)
    // + Enter: the draft wins, exactly one commit — the stale scrubbed 30
    // must not overwrite it from the same-frame chip pass.
    *edit_buffer(&mut h.ui, id) = "42".to_string();
    h.key(Key::Enter);
    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(s.a().committed && s.count_where(|e| e.committed) == 1);
    assert_eq!(canonical, 42.0, "typed value, not the stale scrub");

    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(
        !s.a().committed && !s.a().changed,
        "no residual scrub commit"
    );
    assert_eq!(canonical, 42.0);
}

#[test]
fn unparseable_and_non_finite_drafts_commit_without_writing() {
    let id = WidgetId::from_hash("dv-bad-drafts");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 42.0_f64;
    deferred_frame(&mut h, id, &mut canonical, true, false);

    // Hand-set the buffer after entry (simulates typed garbage); the
    // blur resolve must not clobber the value with junk, NaN, or inf —
    // non-finite parses poison every later scrub, so they're rejected.
    for bad in ["junk", "nan", "inf", "-inf"] {
        h.set_focus(id);
        deferred_frame(&mut h, id, &mut canonical, true, false);
        *edit_buffer(&mut h.ui, id) = bad.to_string();
        h.clear_focus();
        let s = deferred_frame(&mut h, id, &mut canonical, true, false);
        assert!(
            s.a().committed && !s.a().changed,
            "{bad:?}: commit reported, nothing written"
        );
        assert_eq!(canonical, 42.0, "{bad:?} must not land");
    }
}

#[test]
fn disabling_mid_edit_discards_the_draft() {
    let id = WidgetId::from_hash("dv-disable-mid-edit");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 5.0_f64;
    deferred_frame(&mut h, id, &mut canonical, true, false);

    h.set_focus(id);
    deferred_frame(&mut h, id, &mut canonical, true, false);
    h.key(Key::Char('9'));
    deferred_frame(&mut h, id, &mut canonical, true, false);

    // The widget is disabled while the user edits: focus is kicked, the
    // draft is discarded — a locked control must not emit an edit.
    let s = deferred_frame(&mut h, id, &mut canonical, true, true);
    assert!(!s.a().committed, "locked control emits no commit");
    assert_eq!(h.focused_id(), None, "disable kicks the editor's focus");
    assert_eq!(canonical, 5.0);
    assert!(matches!(
        h.state::<DragValueState>(id),
        DragValueState::Idle
    ));

    // Re-enabled later: no phantom replay of the stale "9".
    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(!s.a().committed && !s.a().changed);
    assert_eq!(canonical, 5.0);
}

#[test]
fn toggling_editable_off_mid_edit_cannot_replay_the_draft() {
    // A pending draft must not survive a read-only frame and replay when
    // the caller later enables editing again.
    let id = WidgetId::from_hash("dv-editable-toggle");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 5.0_f64;
    deferred_frame(&mut h, id, &mut canonical, true, false);

    h.set_focus(id);
    deferred_frame(&mut h, id, &mut canonical, true, false);
    h.key(Key::Char('9'));
    h.key(Key::Char('9'));
    h.key(Key::Char('9'));
    deferred_frame(&mut h, id, &mut canonical, true, false);

    // Rendered read-only mid-edit: the pending draft is discarded.
    h.clear_focus();
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.a().committed, "read-only frame commits nothing");
    assert!(matches!(
        h.state::<DragValueState>(id),
        DragValueState::Idle
    ));

    // Back to editable, focus elsewhere: nothing to replay.
    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(
        !s.a().committed && !s.a().changed,
        "no phantom commit of '999'"
    );
    assert_eq!(canonical, 5.0);
}

/// The frame a click opens the editor, the returned response already
/// reports `focused`.
///
/// `DragValue` calls `Ui::set_focus` on itself mid-`show`, but its
/// entry snapshot was taken before that — so without setting
/// `ResponseState::focused` itself the widget would hand back a response
/// denying the focus it had just taken, and a caller keying off
/// `response.focused` would lag a frame behind the editor appearing.
#[test]
fn click_to_edit_reports_focus_on_the_same_frame() {
    let id = WidgetId::from_hash("dv-focus-sync");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut value = 5.0_f64;

    let focused_of = |h: &mut UiHarness, value: &mut f64| -> bool {
        h.frame_value(|ui| {
            let mut draft = *value;
            DragValue::new(&mut draft)
                .editable(true)
                .speed(1.0)
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .id(id)
                .show(ui)
                .response
                .focused
        })
    };

    assert!(!focused_of(&mut h, &mut value), "at rest: not focused");

    // The click lands and requests focus inside this very `show`.
    h.press_at(Vec2::new(50.0, 20.0));
    h.release();
    assert!(
        focused_of(&mut h, &mut value),
        "the response must report the focus the click just took",
    );
}

fn edit_buffer(ui: &mut Ui, id: WidgetId) -> &mut String {
    match ui.state_or_default::<DragValueState>(id) {
        DragValueState::Editing { buffer, .. } => buffer,
        state => panic!("expected DragValue edit state, got {state:?}"),
    }
}
