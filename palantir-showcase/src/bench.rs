//! The tour's first page as the scene palantir's frame-level benches run.

use palantir::bench::scene::Scene;
use palantir::{App, Ui, WidgetId};

use crate::pages::controls;
use crate::shell::{MAIN_WINDOW, Shell};

/// The Controls page, the rail beside it, as a person opens the tour.
#[derive(Debug)]
pub struct Showcase {
    shell: Shell,
    field_len: usize,
}

impl Default for Showcase {
    fn default() -> Self {
        Self {
            shell: Shell::open(0),
            field_len: 0,
        }
    }
}

impl Scene for Showcase {
    fn record(&mut self, ui: &mut Ui) {
        self.shell.record(MAIN_WINDOW, ui);
        self.field_len = controls::internals::device_name_len(ui);
    }

    fn hover_targets(&self) -> [WidgetId; 2] {
        [controls::metered_id(), controls::apply_id()]
    }

    fn field(&self) -> WidgetId {
        controls::device_name_id()
    }

    fn field_len(&self) -> usize {
        self.field_len
    }
}

#[cfg(test)]
mod tests {
    use palantir::Locator;
    use palantir::bench::scene::{BENCH_DPR, BENCH_SURFACE, Scene};
    use palantir::{Harness, Key};

    use crate::bench::Showcase;

    /// What the frame cases aim at answers on their surface: a pointer at the middle of a hover
    /// target hovers it, as one under the page's fold would not, and a key reaches the field.
    #[test]
    fn the_targets_answer_on_the_bench_surface() {
        let mut h = Harness::builder(BENCH_SURFACE)
            .system_scale(BENCH_DPR)
            .build();
        let mut scene = Showcase::default();
        h.frame(|ui| scene.record(ui));
        for id in scene.hover_targets() {
            let rect = h.ui().response_for(id).rect.expect("the target shows");
            h.hover(Locator::Point(rect.center()));
            h.frame(|ui| scene.record(ui));
            assert!(h.ui().response_for(id).hovered(), "{id:?}");
        }
        assert_eq!(scene.field_len(), 0, "the field starts empty");
        h.ui_mut().set_focus(scene.field());
        h.frame(|ui| scene.record(ui));
        h.key(Key::Char('x'));
        h.frame(|ui| scene.record(ui));
        assert_eq!(scene.field_len(), 1, "the key lands");
    }
}
