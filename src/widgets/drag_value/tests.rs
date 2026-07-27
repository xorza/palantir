use crate::Ui;
use crate::input::InputEvent;
use crate::input::keyboard::Key;
use crate::input::pointer::PointerButton;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Configure;
use crate::scene::tree::node::NodeId;
use crate::ui::harness::UiHarness;
use crate::widgets::drag_value::{DragNum, DragValue, DragValueState, round_to_decimals};
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

#[derive(Debug)]
struct Signals {
    changed: bool,
    committed: bool,
    /// How many record passes reported `committed` this frame. A frame
    /// records twice on action input; a commit must fire in exactly one
    /// pass or a per-pass consumer (an undo pusher) double-applies.
    commits: u32,
}

/// Drive one frame of a `DragValue` through a commit-deferring caller:
/// the draft re-seeds from `canonical` every record pass and is adopted
/// only on `committed` — the undo-aware consumption pattern the commit
/// signal exists for. `changed`/`committed` OR-accumulate across the
/// frame's record passes (one-frame edges only show in the first pass);
/// `commits` counts per pass so a double-fire is visible.
fn deferred_frame(
    h: &mut UiHarness,
    id: WidgetId,
    canonical: &mut f64,
    editable: bool,
    disabled: bool,
) -> Signals {
    let mut s = Signals {
        changed: false,
        committed: false,
        commits: 0,
    };
    h.frame(|ui| {
        let mut draft = *canonical;
        let r = DragValue::new(&mut draft)
            .editable(editable)
            .disabled(disabled)
            .speed(1.0)
            .decimals(2)
            .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
            .id(id)
            .show(ui);
        s.changed |= r.changed;
        if r.committed {
            s.committed = true;
            s.commits += 1;
            *canonical = draft;
        }
    });
    s
}

fn key(ui: &mut Ui, k: Key) {
    ui.on_input(InputEvent::KeyDown {
        key: k,
        repeat: false,
        physical: Key::Other,
    });
}

fn edit_buffer(ui: &mut Ui, id: WidgetId) -> &mut String {
    match ui.state_mut::<DragValueState>(id) {
        DragValueState::Editing { buffer } => buffer,
        state => panic!("expected DragValue edit state, got {state:?}"),
    }
}

#[test]
fn scrub_commits_once_on_release_for_deferred_caller() {
    let id = WidgetId::from_hash("dv-scrub-commit");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 10.0_f64;

    // Settle a layout frame so the cascade exists for pointer routing.
    deferred_frame(&mut h, id, &mut canonical, false, false);

    // Press at x=50 inside the 100×40 chip, drag 20px right:
    // draft = anchor 10 + 20px * speed 1 = 30. Live write, no commit,
    // and the deferred caller leaves canonical untouched.
    h.press_at(Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.changed && !s.committed, "mid-drag: live write, no commit");
    assert_eq!(canonical, 10.0, "deferred caller ignores mid-drag writes");

    // 5px more: anchor math re-derives 10 + 25 = 35 even though the
    // caller re-seeded the stale 10 into the draft.
    h.drag_to(Vec2::new(75.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.changed && !s.committed);
    assert_eq!(canonical, 10.0);

    // Release: exactly one commit (in exactly one record pass), carrying
    // the final scrubbed value into the stale-seeded draft.
    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.committed, "release commits the scrub");
    assert_eq!(s.commits, 1, "one commit, one record pass");
    assert_eq!(canonical, 35.0);

    // Idle frame after: no residual signals — one commit per gesture.
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.changed && !s.committed);
    assert_eq!(canonical, 35.0);
}

#[test]
fn scrub_distance_is_scale_invariant() {
    use crate::primitives::transform::TranslateScale;

    let id = WidgetId::from_hash("scaled-drag-value");
    for scale in [0.5, 1.0, 2.0] {
        let mut h = UiHarness::new(UVec2::new(300, 120));
        let mut value = 10.0_f64;
        let build = |ui: &mut Ui, value: &mut f64| {
            Panel::zstack()
                .id(WidgetId::from_hash("scaled-drag-value-parent"))
                .transform(TranslateScale::from_scale(scale))
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .show(ui, |ui| {
                    DragValue::new(value)
                        .editable(false)
                        .speed(1.0)
                        .decimals(2)
                        .id(id)
                        .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                        .show(ui);
                });
        };
        h.frame(|ui| build(ui, &mut value));

        let response = h.ui.response_for(id);
        let layout = response.layout_rect.expect("drag value arranged");
        let press = response
            .transform
            .apply_point(layout.min + Vec2::new(50.0, 20.0));
        let drag = response
            .transform
            .apply_point(layout.min + Vec2::new(70.0, 20.0));
        h.press_at(press);
        h.move_to(drag);
        h.frame(|ui| build(ui, &mut value));

        assert_eq!(value, 30.0, "20 logical px at {scale}× must add exactly 20",);
    }
}

#[test]
fn pointer_leaving_surface_does_not_split_the_gesture() {
    // Mid-scrub window exit must not fire a premature commit, and the
    // resumed drag's remainder must still commit on the real release.
    let id = WidgetId::from_hash("dv-pointer-leave");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 10.0_f64;
    deferred_frame(&mut h, id, &mut canonical, false, false);

    h.press_at(Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    deferred_frame(&mut h, id, &mut canonical, false, false);

    // Pointer crosses the window edge: drag unobservable, but latched.
    h.pointer_left();
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.committed, "window exit is not a release");
    assert_eq!(canonical, 10.0);

    // Re-enter with the button held and keep scrubbing: 10 + 25 = 35.
    h.move_to(Vec2::new(75.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.changed && !s.committed, "resumed drag keeps writing");

    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.committed && s.commits == 1);
    assert_eq!(canonical, 35.0, "one gesture, one commit, full travel");
}

#[test]
fn transient_disable_does_not_swallow_the_gesture() {
    let id = WidgetId::from_hash("dv-transient-disable");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 10.0_f64;
    deferred_frame(&mut h, id, &mut canonical, false, false);

    h.press_at(Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    deferred_frame(&mut h, id, &mut canonical, false, false);

    // One disabled frame mid-drag: no write, but the gesture survives.
    let s = deferred_frame(&mut h, id, &mut canonical, false, true);
    assert!(!s.changed && !s.committed, "disabled frame writes nothing");

    // Re-enabled with the button still held: one settle frame (the
    // cascaded disabled flag is one frame stale), then scrubbing resumes.
    deferred_frame(&mut h, id, &mut canonical, false, false);
    h.drag_to(Vec2::new(75.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.changed, "scrub resumes after the disable blip");

    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.committed && s.commits == 1, "release still commits");
    assert_eq!(canonical, 35.0);
}

#[test]
fn release_while_disabled_drops_the_gesture() {
    let id = WidgetId::from_hash("dv-disabled-release");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 10.0_f64;
    deferred_frame(&mut h, id, &mut canonical, false, false);

    h.press_at(Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    deferred_frame(&mut h, id, &mut canonical, false, false);

    // Released on a disabled frame: a locked control emits no edit, and
    // the gesture is over — a later enabled frame must not revive it.
    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, false, true);
    assert!(!s.committed, "disabled release drops the gesture");
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.committed && !s.changed);
    assert_eq!(canonical, 10.0);
}

#[test]
fn non_left_drags_do_not_scrub() {
    // A right-button drag over the chip is someone else's gesture
    // (context menu, breaker) — it must neither write nor commit.
    let id = WidgetId::from_hash("dv-right-drag");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 10.0_f64;
    deferred_frame(&mut h, id, &mut canonical, false, false);

    h.press_button_at(PointerButton::Right, Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.changed && !s.committed, "right drag must not scrub");

    h.release_button(PointerButton::Right);
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.committed, "right release must not commit");
    assert_eq!(canonical, 10.0);
}

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
    assert!(!s.committed, "the click itself commits nothing");

    // Editor frame: entry seeds the buffer from the value.
    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(!s.committed);
    assert_eq!(edit_buffer(&mut h.ui, id), "5.0", "seeded on entry");

    // First keystroke replaces the select-all'd seed; second appends.
    key(&mut h.ui, Key::Char('7'));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    key(&mut h.ui, Key::Char('2'));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    assert_eq!(canonical, 5.0, "typing is a live draft, not a commit");

    key(&mut h.ui, Key::Enter);
    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(s.committed && s.commits == 1, "Enter commits once");
    assert_eq!(canonical, 72.0);

    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(!s.committed, "commit is a one-frame edge");
}

#[test]
fn escape_blur_commits_pending_draft_once() {
    let id = WidgetId::from_hash("dv-escape-blur");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 5.0_f64;
    deferred_frame(&mut h, id, &mut canonical, true, false);

    h.request_focus(Some(id));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    key(&mut h.ui, Key::Char('4'));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    key(&mut h.ui, Key::Char('2'));
    deferred_frame(&mut h, id, &mut canonical, true, false);

    // Escape blurs the editor (typing left no selection, so one Escape).
    // The pending draft resolves on the first chip record after the blur —
    // the same frame when it re-records, the next frame otherwise — with
    // exactly one commit either way.
    key(&mut h.ui, Key::Escape);
    let a = deferred_frame(&mut h, id, &mut canonical, true, false);
    let b = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(a.committed || b.committed, "blur commits the draft");
    assert_eq!(a.commits + b.commits, 1, "exactly one commit");
    assert_eq!(canonical, 42.0);

    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(!s.committed);
}

#[test]
fn programmatic_focus_seeds_a_fresh_buffer() {
    // Regression: the buffer used to be seeded only by the click path, so
    // request_focus re-opened the previous session's stale text and
    // committed it over an externally-changed value.
    let id = WidgetId::from_hash("dv-fresh-seed");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 5.0_f64;
    deferred_frame(&mut h, id, &mut canonical, true, false);

    // First session commits 42 and leaves "42" in the buffer state.
    h.request_focus(Some(id));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    key(&mut h.ui, Key::Char('4'));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    key(&mut h.ui, Key::Char('2'));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    key(&mut h.ui, Key::Enter);
    deferred_frame(&mut h, id, &mut canonical, true, false);
    assert_eq!(canonical, 42.0);

    // The value changes externally; a new focus must show 99, not 42.
    canonical = 99.0;
    h.request_focus(Some(id));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    assert_eq!(edit_buffer(&mut h.ui, id), "99.0");

    key(&mut h.ui, Key::Enter);
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
    h.request_focus(Some(id));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(matches!(
        h.ui.state_mut::<DragValueState>(id),
        DragValueState::Editing { .. }
    ));

    // The release lands on an editor frame, so no scrub commit may surface
    // now or later.
    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(!s.committed, "disarmed scrub must not commit into the edit");

    // A typed draft (hand-set: the still-held-then-released button placed
    // a caret, so simulated keystrokes wouldn't select-all-replace here;
    // the typed path is covered by `click_to_edit_types_and_commits_on_enter`)
    // + Enter: the draft wins, exactly one commit — the stale scrubbed 30
    // must not overwrite it from the same-frame chip pass.
    *edit_buffer(&mut h.ui, id) = "42".to_string();
    key(&mut h.ui, Key::Enter);
    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(s.committed && s.commits == 1);
    assert_eq!(canonical, 42.0, "typed value, not the stale scrub");

    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(!s.committed && !s.changed, "no residual scrub commit");
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
        h.request_focus(Some(id));
        deferred_frame(&mut h, id, &mut canonical, true, false);
        *edit_buffer(&mut h.ui, id) = bad.to_string();
        h.request_focus(None);
        let s = deferred_frame(&mut h, id, &mut canonical, true, false);
        assert!(
            s.committed && !s.changed,
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

    h.request_focus(Some(id));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    key(&mut h.ui, Key::Char('9'));
    deferred_frame(&mut h, id, &mut canonical, true, false);

    // The widget is disabled while the user edits: focus is kicked, the
    // draft is discarded — a locked control must not emit an edit.
    let s = deferred_frame(&mut h, id, &mut canonical, true, true);
    assert!(!s.committed, "locked control emits no commit");
    assert_eq!(h.focused_id(), None, "disable kicks the editor's focus");
    assert_eq!(canonical, 5.0);
    assert!(matches!(
        h.ui.state_mut::<DragValueState>(id),
        DragValueState::Idle
    ));

    // Re-enabled later: no phantom replay of the stale "9".
    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(!s.committed && !s.changed);
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

    h.request_focus(Some(id));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    key(&mut h.ui, Key::Char('9'));
    key(&mut h.ui, Key::Char('9'));
    key(&mut h.ui, Key::Char('9'));
    deferred_frame(&mut h, id, &mut canonical, true, false);

    // Rendered read-only mid-edit: the pending draft is discarded.
    h.request_focus(None);
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.committed, "read-only frame commits nothing");
    assert!(matches!(
        h.ui.state_mut::<DragValueState>(id),
        DragValueState::Idle
    ));

    // Back to editable, focus elsewhere: nothing to replay.
    let s = deferred_frame(&mut h, id, &mut canonical, true, false);
    assert!(!s.committed && !s.changed, "no phantom commit of '999'");
    assert_eq!(canonical, 5.0);
}

#[test]
fn round_to_decimals_snaps_and_formats_short() {
    // The reported long value snaps to its 3-decimal display and prints
    // without a tail — that's the whole point (edit_string shows this).
    let r = round_to_decimals(1.984_573_845_634_985_2, 3);
    assert_eq!(r, 1.985);
    assert_eq!(format!("{r:?}"), "1.985");
    // Fewer / zero decimals.
    assert_eq!(round_to_decimals(1.984_573_845_634_985_2, 2), 1.98);
    assert_eq!(round_to_decimals(1.984_573_845_634_985_2, 0), 2.0);
    // Classic float-noise inputs collapse to a clean short value.
    assert_eq!(format!("{:?}", round_to_decimals(0.1 + 0.2, 1)), "0.3");
    assert_eq!(round_to_decimals(12.3456, 2), 12.35);
    // Negative values keep their sign.
    assert_eq!(round_to_decimals(-1.6789, 1), -1.7);
}

#[test]
fn commit_drag_snaps_rounds_clamps_and_reports_change() {
    const INF: f64 = f64::INFINITY;
    // Float: snaps to `decimals`, unbounded is a no-op clamp; the write
    // reports the change.
    let mut f = 0.0;
    assert!(DragNum::from(&mut f).commit_drag(1.984_573_845_634_985_2, 3, -INF, INF));
    assert_eq!(f, 1.985);
    // Re-committing the same raw is a no-change write.
    assert!(!DragNum::from(&mut f).commit_drag(1.984_573_845_634_985_2, 3, -INF, INF));
    // Float: clamps into the range.
    let mut f = 0.0;
    assert!(DragNum::from(&mut f).commit_drag(50.0, 2, 0.0, 10.0));
    assert_eq!(f, 10.0);
    // A tiny negative wiggle at a 0.0 bound rounds to -0.0; the stored
    // value must be normalized to +0.0 (bit-exact) with no change report.
    let mut f = 0.0;
    assert!(!DragNum::from(&mut f).commit_drag(-0.004, 2, 0.0, 1.0));
    assert_eq!(f.to_bits(), 0.0_f64.to_bits(), "-0.0 normalized to +0.0");
    // Int: rounds to whole (decimals ignored), unbounded no-op clamp.
    let mut i = 0;
    assert!(DragNum::from(&mut i).commit_drag(7.6, 3, -INF, INF));
    assert_eq!(i, 8);
    assert!(!DragNum::from(&mut i).commit_drag(7.6, 3, -INF, INF));
    // Int: clamps into the range.
    let mut i = 0;
    assert!(DragNum::from(&mut i).commit_drag(500.0, 0, 0.0, 100.0));
    assert_eq!(i, 100);
}

#[test]
fn drag_num_get_reads_both_variants() {
    let mut f = 2.5_f64;
    assert_eq!(DragNum::from(&mut f).get(), 2.5);
    let mut i = 5_i64;
    assert_eq!(DragNum::from(&mut i).get(), 5.0);
}

#[test]
fn drag_num_edit_string_and_parse_round_trip() {
    const INF: f64 = f64::INFINITY;
    // Float keeps a trailing `.0` so it re-reads as a float, and a
    // fractional value survives verbatim (a same-value parse reports no
    // change).
    let mut f = 3.0_f64;
    assert_eq!(DragNum::from(&mut f).edit_string(), "3.0");
    let mut f = 2.5_f64;
    let s = DragNum::from(&mut f).edit_string();
    assert!(!DragNum::from(&mut f).parse_from(&s, -INF, INF));
    assert_eq!(f, 2.5);

    // Int formats and parses back exactly.
    let mut i = -42_i64;
    assert_eq!(DragNum::from(&mut i).edit_string(), "-42");

    // Unparseable text leaves the value untouched (partial input).
    let mut i = 9_i64;
    assert!(!DragNum::from(&mut i).parse_from("12x", -INF, INF));
    assert_eq!(i, 9);
    assert!(DragNum::from(&mut i).parse_from("15", -INF, INF));
    assert_eq!(i, 15);

    // Non-finite parses are rejected even unbounded — a committed NaN
    // would survive clamp and poison every subsequent scrub.
    let mut f = 7.5_f64;
    for bad in ["nan", "NaN", "inf", "-inf", "infinity"] {
        assert!(!DragNum::from(&mut f).parse_from(bad, -INF, INF), "{bad}");
        assert_eq!(f, 7.5, "{bad} must not land");
    }

    // Typed entry clamps into the range too.
    let mut i = 0_i64;
    assert!(DragNum::from(&mut i).parse_from("500", 0.0, 100.0));
    assert_eq!(i, 100);
    let mut f = 0.0_f64;
    assert!(!DragNum::from(&mut f).parse_from("-3.5", 0.0, 1.0));
    assert_eq!(f, 0.0);
    // A typed "-0.0" stores as +0.0 — sign-of-zero never leaks.
    let mut f = 1.0_f64;
    assert!(DragNum::from(&mut f).parse_from("-0.0", -INF, INF));
    assert_eq!(f.to_bits(), 0.0_f64.to_bits());
}

#[test]
fn editing_a_long_value_holds_the_field_width() {
    use super::DragValue;
    use crate::Ui;
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::widget_id::WidgetId;
    use crate::scene::layer::Layer;
    use crate::scene::node::Configure;
    use crate::widgets::panel::Panel;
    use glam::UVec2;

    let surface = UVec2::new(400, 120);
    let id = WidgetId::from_hash("dv-width");
    let mut v = 1.984_573_845_634_985_2_f64;

    // A `Hug` row makes the field's own content drive its width — the
    // condition where the width-cap matters. The chip shows "1.985"; the
    // editor seeds the full-precision value on entry and must scroll it
    // inside the chip's width rather than grow the row.
    let render = |ui: &mut Ui, v: &mut f64| -> NodeId {
        let mut node = None;
        Panel::hstack()
            .id(WidgetId::from_hash("dv-row"))
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                node = Some(
                    DragValue::new(v)
                        .editable(true)
                        .decimals(3)
                        .size((Sizing::fill(1.0), Sizing::HUG))
                        .min_size((40.0, 0.0))
                        .id(id)
                        .show(ui)
                        .response
                        .node(),
                );
            });
        node.unwrap()
    };

    let mut h = UiHarness::new(surface);
    let mut node = None;
    h.frame(|ui| node = Some(render(ui, &mut v)));
    let display_w = h.ui.layout[Layer::Main].rect[node.unwrap().idx()].size.w;

    // Enter edit mode; entry seeds the full-precision text.
    h.request_focus(Some(id));
    h.frame(|ui| node = Some(render(ui, &mut v)));
    let edit_w = h.ui.layout[Layer::Main].rect[node.unwrap().idx()].size.w;

    assert!(display_w >= 40.0, "min_size floor honored ({display_w})");
    assert_eq!(
        display_w, edit_w,
        "editing the full-precision value must not resize the field \
         (display {display_w}, edit {edit_w})"
    );
}

#[test]
fn editing_under_a_scaled_canvas_does_not_panic() {
    use super::DragValue;
    use crate::Ui;
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::transform::TranslateScale;
    use crate::primitives::widget_id::WidgetId;
    use crate::scene::node::Configure;
    use crate::widgets::panel::Panel;
    use glam::{UVec2, Vec2};

    let surface = UVec2::new(400, 120);
    let id = WidgetId::from_hash("dv-zoom");
    let mut v = 1.984_573_845_634_985_2_f64;

    // A scaled parent (0.5×) halves the chip's post-transform rect to ~60px
    // while `min_size` is 100 — the cap must read the pre-transform
    // (logical, 120) width and floor at `min_size`, else feeding the 60px
    // post-transform width makes `resolve_axis_size`'s `clamp(100, 60)`
    // panic.
    let mut h = UiHarness::new(surface);
    let draw = |ui: &mut Ui, v: &mut f64| {
        Panel::zstack()
            .id(WidgetId::from_hash("dv-zoom-row"))
            .transform(TranslateScale::new(Vec2::ZERO, 0.5))
            .size((Sizing::fixed(120.0), Sizing::fixed(60.0)))
            .show(ui, |ui| {
                DragValue::new(v)
                    .editable(true)
                    .decimals(3)
                    .size((Sizing::fill(1.0), Sizing::HUG))
                    .min_size((100.0, 0.0))
                    .id(id)
                    .show(ui);
            });
    };
    h.frame(|ui| draw(ui, &mut v));
    h.request_focus(Some(id));
    h.frame(|ui| draw(ui, &mut v));
}

/// Entering edit mode must not move, resize, or re-place the widget.
///
/// The chip and the inline editor are two different widgets sharing one
/// `WidgetId`, so every field of the caller's `Node` that positions the
/// widget in its parent has to be carried across the swap by hand. It
/// wasn't: padding, margin, alignment, canvas position and grid placement
/// were all dropped, so clicking to type visibly jumped the field.
///
/// Rather than enumerate the fields a second time, this records the same
/// configured `DragValue` twice — once as a chip, once focused as an
/// editor — and asserts the *recorded* layout matches. Any inherited field
/// that stops being carried shows up here as a divergence, including
/// fields added later.
#[test]
fn entering_edit_mode_preserves_the_callers_node_placement() {
    use crate::layout::types::align::Align;
    use crate::primitives::size::Size;
    use crate::primitives::spacing::Spacing;
    use crate::scene::layer::Layer;

    const POSITION: Vec2 = Vec2::new(23.0, 11.0);
    let padding = Spacing::all(7.0);
    let margin = Spacing::all(3.0);

    let id = WidgetId::from_hash("configured-drag-value");
    // A `Canvas` parent so `position` is honoured rather than ignored.
    let scene = |ui: &mut Ui| {
        let mut v = 1.5_f64;
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                DragValue::new(&mut v)
                    .editable(true)
                    .id(id)
                    .padding(padding)
                    .margin(margin)
                    .align(Align::CENTER)
                    .position(POSITION)
                    .min_size(Size::new(40.0, 20.0))
                    .max_size(Size::new(200.0, 60.0))
                    .show(ui);
            });
    };

    /// Recorded placement of `id` — the fields the swap must preserve.
    ///
    /// Padding and minimum height are excluded on purpose: both modes
    /// resolve those from their own theme and intrinsics (a text editor has
    /// a line-height floor a chip does not), so they are not carried by the
    /// node policy and comparing them would pin theme configuration rather
    /// than this fix.
    fn placement(ui: &Ui, id: WidgetId) -> (Spacing, Align, Vec2, Size) {
        let tree = &ui.forest.trees[Layer::Main];
        let index = tree
            .records
            .widget_id()
            .iter()
            .position(|w| *w == id)
            .expect("drag value node");
        let layout = tree.records.layout()[index];
        let bounds = tree.bounds(NodeId(index as u32));
        (
            layout.margin,
            layout.meta.align(),
            bounds.position,
            bounds.max_size,
        )
    }

    let mut h = UiHarness::new(UVec2::new(300, 100));
    h.frame(scene);
    let chip = placement(&h.ui, id);

    // Focus flips the same widget to its inline editor.
    h.request_focus(Some(id));
    h.frame(scene);
    let editor = placement(&h.ui, id);

    assert_eq!(
        chip, editor,
        "edit mode dropped part of the caller's node policy \
         (margin, align, position, max_size)",
    );
    // Guard against the test going inert if the editor path stops being
    // taken at all — then both frames would be chips and match trivially.
    assert!(
        matches!(
            h.ui.state_mut::<DragValueState>(id),
            DragValueState::Editing { .. }
        ),
        "second frame must have recorded the inline editor",
    );
}
