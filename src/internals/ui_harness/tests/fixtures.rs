//! The non-frame affordances: the arena, the clipboard, and resources shared
//! between harnesses.

use crate::internals::ui_harness::tests::support::SURFACE;
use crate::internals::ui_harness::*;

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
    // Lowered through the real path: `InternedStr` is a span plus an epoch and owns nothing, so only the store resolves it, which is what this harness makes reachable without a frame.
    let store = &harness.ui.forest().record_store;
    let recorded = store.record_text(interned);
    assert_eq!(store.interned_text().resolve(recorded.span), "label");
}

/// Two harnesses built from one `UiResources` share what it holds; the clipboard shows it.
#[test]
fn from_resources_shares_one_resource_set_between_harnesses() {
    let shared = UiResources::isolated_text();
    let mut first = UiHarness::from_resources(shared.clone(), SURFACE);
    let second = UiHarness::from_resources(shared, SURFACE);

    first.set_clipboard_text("shared");
    assert_eq!(
        second.clipboard_text(),
        "shared",
        "one UiResources, one clipboard",
    );
}
