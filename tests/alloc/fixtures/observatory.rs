//! The observatory example's studies, settled: a frame of each records with no allocation, so a
//! study built on palantir's widgets holds palantir's own per-frame rule.

use palantir::App;

use crate::app::studies::Study;
use crate::app::{MAIN_WINDOW, Shell};
use crate::harness::Audit;

#[test]
fn settled_observatory_studies_alloc_free() {
    for study in Study::ALL {
        let mut shell = Shell::on(study);
        Audit::new().run(move |ui| shell.record(MAIN_WINDOW, ui));
    }
}
