//! The non-frame affordances: the arena, the clipboard, and a shared text
//! cache.

use crate::internals::harness::tests::support::SURFACE;
use crate::internals::harness::*;

#[test]
fn clipboard_round_trips_through_the_harness() {
    let mut harness = UiHarness::new(SURFACE);
    assert_eq!(harness.clipboard_text(), "");
    harness.set_clipboard_text("copied");
    assert_eq!(harness.clipboard_text(), "copied");
}

#[test]
fn arena_interns_without_ever_recording() {
    let mut harness = UiHarness::arena();
    let interned = harness.ui().intern("label");
    // Lowered through the real path rather than read off the handle:
    // `InternedStr` is a span plus an epoch and owns nothing, so the
    // store is the only thing that can resolve it — which is exactly the
    // property this harness exists to make reachable without a frame.
    let store = &harness.ui.forest().record_store;
    let recorded = store.record_text(interned);
    assert_eq!(store.interned_text().resolve(recorded.span), "label");
}

#[test]
fn from_resources_pairs_two_harnesses_onto_one_text_cache() {
    let shared = UiResources::isolated_text();
    let mut first = UiHarness::from_resources(shared.clone(), SURFACE);
    let second = UiHarness::from_resources(shared.clone(), SURFACE);

    first.set_clipboard_text("shared");
    assert_eq!(
        second.clipboard_text(),
        "shared",
        "one UiResources, one clipboard",
    );
}
