//! [`Section`]: a titled block of demo content.

use palantir::{Configure, Panel, SizeSpec, Text, Ui, VAlign};

use crate::scaffold::chip::Chip;
use crate::scaffold::kit::Kit;
use crate::scaffold::tokens::M;
use crate::scaffold::{TITLE_GAP, section_style};

/// A titled block of demo content: a title, a chip per demonstrated API, then the body. A
/// [`note`](crate::scaffold::note) may name an API only when one of the section's [`api!`](crate::scaffold::api) chips checks it.
#[derive(Debug)]
#[must_use]
pub(crate) struct Section<'a> {
    title: &'static str,
    apis: &'a [&'static str],
}

impl<'a> Section<'a> {
    pub(crate) const fn new(title: &'static str) -> Self {
        Self { title, apis: &[] }
    }

    /// The APIs the section demonstrates, each spelled by [`api!`](crate::scaffold::api).
    pub(crate) const fn apis(mut self, apis: &'a [&'static str]) -> Self {
        self.apis = apis;
        self
    }

    /// Identity comes from the call site (`#[track_caller]` plus `.auto_id()`), so editing a title
    /// doesn't re-key the contents.
    #[track_caller]
    pub(crate) fn show(self, ui: &mut Ui, k: Kit, body: impl FnOnce(&mut Ui)) {
        Panel::vstack()
            .auto_id()
            .size(SizeSpec::FILL_W)
            .gap(TITLE_GAP)
            .show(ui, |ui| {
                Panel::wrap_hstack()
                    .size(SizeSpec::FILL_W)
                    .gap(M.space.xs)
                    .line_gap(M.space.xs)
                    .child_align(VAlign::Center)
                    .show(ui, |ui| {
                        Text::new(self.title)
                            .style(&section_style(k))
                            .margin((0.0, 0.0, M.space.xs, 0.0))
                            .show(ui);
                        for &api in self.apis {
                            Chip::Tag.show(ui, k.t, api);
                        }
                    });
                body(ui);
            });
    }
}
