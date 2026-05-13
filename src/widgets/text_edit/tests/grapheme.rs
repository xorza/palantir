use super::*;

#[test]
fn backspace_deletes_whole_grapheme_cluster() {
    // Buffer: 'a', e + combining-acute (one grapheme, two codepoints,
    // 3 bytes), 'b'. Caret at end. One backspace must delete 'b',
    // a second must delete *both* bytes of the combining grapheme.
    let mut s = String::from("ae\u{0301}b");
    let mut state = TextEditState {
        caret: s.len(),
        ..Default::default()
    };
    apply_key(&mut s, &mut state, press(Key::Backspace));
    assert_eq!(s, "ae\u{0301}", "backspace removes 'b'");
    assert_eq!(state.caret, 4);
    apply_key(&mut s, &mut state, press(Key::Backspace));
    assert_eq!(
        s, "a",
        "backspace deletes e + combining acute as one grapheme",
    );
    assert_eq!(state.caret, 1);
}

#[test]
fn grapheme_boundary_helpers_step_whole_clusters() {
    // ASCII / single-codepoint graphemes: boundaries match the
    // codepoint walk one-for-one.
    let s = "héllo"; // NFC: é = U+00E9 = 2 bytes / 1 codepoint / 1 grapheme
    assert_eq!(next_grapheme_boundary(s, 0), 1);
    assert_eq!(next_grapheme_boundary(s, 1), 3);
    assert_eq!(prev_grapheme_boundary(s, 3), 1);
    assert_eq!(next_grapheme_boundary(s, s.len()), s.len());
    assert_eq!(prev_grapheme_boundary(s, 0), 0);

    // Combining mark: 'e' + U+0301 (combining acute) is one grapheme,
    // two codepoints, 3 bytes. Walks must step over both codepoints
    // in one shot — otherwise backspace would split the accent off.
    let s = "ae\u{0301}b";
    assert_eq!(next_grapheme_boundary(s, 0), 1, "past 'a'");
    assert_eq!(
        next_grapheme_boundary(s, 1),
        4,
        "skip e + combining acute as one grapheme",
    );
    assert_eq!(
        prev_grapheme_boundary(s, 4),
        1,
        "rewind back to the start of the e + combining grapheme",
    );
    assert_eq!(next_grapheme_boundary(s, 4), 5, "past 'b'");

    // ZWJ-joined family emoji: 7 codepoints, 1 grapheme cluster.
    // U+1F468 ZWJ U+1F469 ZWJ U+1F467 = 18 bytes.
    let s = "x👨\u{200D}👩\u{200D}👧y";
    let emoji_start = 1;
    let y_byte = s.find('y').unwrap();
    assert_eq!(
        next_grapheme_boundary(s, emoji_start),
        y_byte,
        "ZWJ-joined family emoji walks as one grapheme",
    );
    assert_eq!(prev_grapheme_boundary(s, y_byte), emoji_start);
}
