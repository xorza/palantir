//! [`Observatory`]: the one model every study reads and writes.

pub(crate) mod appearance;
pub(crate) mod catalog;
pub(crate) mod log;
pub(crate) mod plan;
pub(crate) mod session;
pub(crate) mod sky;
pub(crate) mod view;

use crate::app::model::appearance::Appearance;
use crate::app::model::catalog::{Catalog, ObjectId, SEED};
use crate::app::model::log::Log;
use crate::app::model::plan::Plan;
use crate::app::model::session::{FILTERS, Session};
use crate::app::model::view::CatalogView;

/// Everything the studies show: the sky, what is selected, the night's plan and settings, the
/// log, and the look.
#[derive(Debug)]
pub(crate) struct Observatory {
    pub(crate) catalog: Catalog,
    pub(crate) view: CatalogView,
    pub(crate) selection: Option<ObjectId>,
    pub(crate) plan: Plan,
    pub(crate) session: Session,
    pub(crate) log: Log,
    pub(crate) appearance: Appearance,
}

impl Observatory {
    /// The example's one sky, nothing selected or planned, the stock look.
    pub(crate) fn new() -> Self {
        Self {
            catalog: Catalog::generate(SEED),
            view: CatalogView::new(),
            selection: None,
            plan: Plan::default(),
            session: Session::new(),
            log: Log::default(),
            appearance: Appearance::new(),
        }
    }

    /// Select `id`, logged once: selecting the selected object again records nothing.
    pub(crate) fn select(&mut self, id: ObjectId) {
        if self.selection == Some(id) {
            return;
        }
        self.selection = Some(id);
        let name = &self.catalog.get(id).name;
        self.log.push(format!("selected {name}"));
    }

    /// Add `id` to the plan through the session's filter, logged; `false` when it is planned
    /// already.
    pub(crate) fn add_to_plan(&mut self, id: ObjectId) -> bool {
        let filter = self.session.filter;
        if !self.plan.add(id, filter) {
            return false;
        }
        let name = &self.catalog.get(id).name;
        self.log
            .push(format!("planned {name} through {}", FILTERS[filter]));
        true
    }

    /// Empty the plan, logged.
    pub(crate) fn clear_plan(&mut self) {
        self.plan.clear();
        self.log.push("cleared the plan".to_owned());
    }
}

#[cfg(test)]
mod tests {
    use crate::app::model::Observatory;
    use crate::app::model::catalog::ObjectId;

    fn texts(model: &Observatory) -> Vec<&str> {
        model.log.lines().map(|line| line.text.as_str()).collect()
    }

    /// A selection and a plan entry each log once; a repeat changes nothing and logs nothing.
    #[test]
    fn selecting_and_planning_log_each_change_once() {
        let mut model = Observatory::new();
        model.select(ObjectId(0));
        model.select(ObjectId(0));
        assert_eq!(model.selection, Some(ObjectId(0)));
        assert!(model.add_to_plan(ObjectId(0)));
        assert!(!model.add_to_plan(ObjectId(0)));
        model.clear_plan();
        assert_eq!(
            texts(&model),
            [
                "selected OBS 0001",
                "planned OBS 0001 through L",
                "cleared the plan"
            ]
        );
        assert!(model.plan.entries().is_empty());
    }
}
