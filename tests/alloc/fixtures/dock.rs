//! A settled dock frame at a real editor's scale: three panes, two dividers, several chips per strip.
//!
//! Audits the composition: the response scan, the recursive `Splitter` walk, a `TabStrip` per pane rebuilt each frame, and a closed per-chip context menu.

use palantir::internals::frame_fixture::dock_fixture::DockFixture;

use crate::harness::Audit;

#[test]
fn settled_dock_frame_alloc_free() {
    let mut dock = DockFixture::default();
    Audit::new().run(move |ui| dock.record(ui));
}

/// The two-call surface allocates no more than the one-call one: the caller's op buffer is the same reused `Vec`.
#[test]
fn scan_then_record_is_alloc_free_too() {
    let mut dock = DockFixture::default();
    Audit::new().run(move |ui| dock.record_scanned(ui));
}
