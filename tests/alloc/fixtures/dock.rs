//! A settled dock frame, at the scale a real editor runs one: three
//! panes, two dividers, several chips per strip.
//!
//! The claim under audit is the composition, not any one widget. A dock
//! frame runs a scan over last frame's responses, a recursive walk onto
//! two `Splitter`s, a `TabStrip` per pane whose items are rebuilt every
//! frame from the application's own answers, and a per-chip context menu
//! that stays closed — every one of them a place a fresh `Vec` would be
//! easy to write and invisible to look at.

use palantir::internals::frame_fixture::dock_fixture::DockFixture;

use crate::harness::Audit;

#[test]
fn settled_dock_frame_alloc_free() {
    let mut dock = DockFixture::default();
    Audit::new().run(move |ui| dock.record(ui));
}

/// The two-call surface pays no more than the one-call one: the caller's
/// own op buffer is the same reused `Vec` `run` keeps internally.
#[test]
fn scan_then_record_is_alloc_free_too() {
    let mut dock = DockFixture::default();
    Audit::new().run(move |ui| dock.record_scanned(ui));
}
