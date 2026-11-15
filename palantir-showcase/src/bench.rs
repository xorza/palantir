//! The tour's first page as the scene palantir's frame-level benches run.

use palantir::bench::scene::Scene;
use palantir::{App, Ui, WidgetId};

use crate::pages::controls;
use crate::shell::{MAIN_WINDOW, Shell};

/// The Controls page, the rail beside it, as a person opens the tour.
#[derive(Debug)]
pub struct Showcase {
    shell: Shell,
}

impl Default for Showcase {
    fn default() -> Self {
        Self {
            shell: Shell::open(Shell::page_idx("Controls").expect("the tour has a Controls page")),
        }
    }
}

impl Scene for Showcase {
    fn record(&mut self, ui: &mut Ui) {
        self.shell.record(MAIN_WINDOW, ui);
    }

    fn hover_targets(&self) -> [WidgetId; 2] {
        [controls::metered_id(), controls::apply_id()]
    }

    fn field(&self) -> WidgetId {
        controls::device_name_id()
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
    /// The field's text is what its widget records, as the inspection reads it.
    #[test]
    fn the_targets_answer_on_the_bench_surface() {
        let mut h = Harness::builder(BENCH_SURFACE)
            .system_scale(BENCH_DPR)
            .build();
        let mut scene = Showcase::default();
        h.frame(|ui| scene.record(ui));
        for id in scene.hover_targets() {
            let rect = h.ui().response_for(id).rect.expect("the target shows");
            h.hover(Locator::point(rect.center()));
            h.frame(|ui| scene.record(ui));
            assert!(h.ui().response_for(id).hovered(), "{id:?}");
        }
        let field = scene.field();
        let field_text = |h: &Harness| {
            let mut widgets = Vec::new();
            h.ui().inspect(&mut widgets);
            widgets
                .into_iter()
                .find(|widget| widget.id == field)
                .expect("the field shows")
                .text
        };
        assert_eq!(field_text(&h), "", "the field starts empty");
        h.ui_mut().set_focus(field);
        h.frame(|ui| scene.record(ui));
        h.key(Key::Char('x'));
        h.frame(|ui| scene.record(ui));
        assert_eq!(field_text(&h), "x", "the key lands");
    }
}
