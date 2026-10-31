//! The five studies, each a screen of the tool over the one model, and the details view the
//! workspace and the eyepiece share.

pub(crate) mod appearance;
pub(crate) mod catalog;
pub(crate) mod session;
pub(crate) mod sky_chart;
pub(crate) mod workspace;

use palantir::{Configure, Panel, Sizing, Text, Ui, fmt};

use crate::app::model::Observatory;
use crate::app::model::sky::{Dms, Hms};
use crate::app::studies::sky_chart::ChartRequest;
use crate::scaffold::links::LINKS;
use crate::scaffold::tokens::Tokens;

/// A study the rail picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Study {
    SkyChart,
    Catalog,
    Session,
    Workspace,
    Appearance,
}

impl Study {
    /// Every study, in the rail's order, which Ctrl+1 … Ctrl+5 follow.
    pub(crate) const ALL: [Self; 5] = [
        Self::SkyChart,
        Self::Catalog,
        Self::Session,
        Self::Workspace,
        Self::Appearance,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::SkyChart => "sky chart",
            Self::Catalog => "catalog",
            Self::Session => "session",
            Self::Workspace => "workspace",
            Self::Appearance => "appearance",
        }
    }

    /// The showcase pages that show this study's pieces alone, from the table both examples read.
    pub(crate) fn showcase_pages(self) -> impl Iterator<Item = &'static str> {
        LINKS
            .iter()
            .filter(move |link| link.study == self.label())
            .map(|link| link.page)
    }

    pub(crate) fn header(self, ui: &mut Ui, t: Tokens) {
        match self {
            Self::SkyChart => sky_chart::header(ui, t),
            Self::Catalog => catalog::header(ui, t),
            Self::Session => session::header(ui, t),
            Self::Workspace => workspace::header(ui, t),
            Self::Appearance => appearance::header(ui, t),
        }
    }

    /// Record the study; the study to switch to, when one asks.
    pub(crate) fn show(self, ui: &mut Ui, model: &mut Observatory, t: Tokens) -> Option<Self> {
        match self {
            Self::SkyChart => {
                sky_chart::show(ui, model, t).map(|ChartRequest::ShowInCatalog| Self::Catalog)
            }
            Self::Catalog => {
                catalog::show(ui, model, t);
                None
            }
            Self::Session => {
                session::show(ui, model, t);
                None
            }
            Self::Workspace => workspace::show(ui, model, t),
            Self::Appearance => {
                appearance::show(ui, model, t);
                None
            }
        }
    }
}

/// The selection's facts: name, kind, coordinates, magnitude, note; a hint when nothing is
/// selected.
pub(crate) fn details(ui: &mut Ui, model: &Observatory, t: Tokens) {
    Panel::vstack()
        .id_salt("details")
        .size((Sizing::FILL, Sizing::HUG))
        .gap(6.0)
        .show(ui, |ui| {
            let Some(id) = model.selection else {
                Text::new("Select a star in the chart or a row in the catalog.")
                    .style(&t.dim())
                    .show(ui);
                return;
            };
            let object = model.catalog.get(id);
            Text::new(fmt!(ui, "{}", object.name))
                .style(&t.title())
                .show(ui);
            let rows = [
                ("kind", fmt!(ui, "{}", object.kind.label())),
                ("RA", fmt!(ui, "{}", Hms(object.ra))),
                ("Dec", fmt!(ui, "{}", Dms(object.dec))),
                ("magnitude", fmt!(ui, "{:.2}", object.mag)),
                (
                    "planned",
                    ui.intern(if model.plan.contains(id) { "yes" } else { "no" }),
                ),
            ];
            for (label, value) in rows {
                Panel::hstack().id_salt(label).gap(8.0).show(ui, |ui| {
                    Text::new(label)
                        .style(&t.caption())
                        .size((Sizing::fixed(80.0), Sizing::HUG))
                        .show(ui);
                    Text::new(value).style(&t.mono(t.ink)).show(ui);
                });
            }
            if !object.note.is_empty() {
                Text::new(fmt!(ui, "{}", object.note))
                    .style(&t.dim())
                    .show(ui);
            }
        });
}

/// The eyepiece window: the selection's details on the window's ground.
pub(crate) fn eyepiece(ui: &mut Ui, model: &Observatory, t: Tokens) {
    Panel::vstack()
        .id_salt("eyepiece")
        .size((Sizing::FILL, Sizing::FILL))
        .padding(16.0)
        .gap(12.0)
        .show(ui, |ui| details(ui, model, t));
}
