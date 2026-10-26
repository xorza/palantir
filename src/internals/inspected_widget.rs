//! [`InspectedWidget`]: the last frame's interactive widgets, as a test or an agent driving the UI
//! from outside reads them.

use crate::cascade::Cascade;
use crate::input::sense::Sense;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdSet};
use crate::scene::forest::Forest;
use crate::scene::tree::Tree;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;

/// One widget of the last frame that senses the pointer or takes focus: where it is, what it
/// reacts to, and the text it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectedWidget {
    /// The widget, as its `Response` and the harness's input helpers name it.
    pub id: WidgetId,
    /// The visible screen rect: after transforms, cut by the ancestors' clips.
    pub rect: Rect,
    /// The pointer interactions it takes, as the hit test reads them.
    pub sense: Sense,
    /// Whether a press or Tab can focus it.
    pub focusable: bool,
    /// Disabled itself or under a disabled ancestor.
    pub disabled: bool,
    /// The ancestors' clips hide all of it, so no pointer reaches it.
    pub clipped: bool,
    /// The text its subtree records, in record order and joined by a space, clipped or not. The
    /// text of a nested widget that is listed itself is that widget's, so a button's label is the
    /// button's alone.
    pub text: String,
}

impl InspectedWidget {
    /// Every interactive widget of the frame `cascade` was built from, in paint order, into `out`.
    pub(crate) fn collect(forest: &Forest, cascade: &Cascade, out: &mut Vec<Self>) {
        out.clear();
        let rows = cascade.hit_rows();
        let listed: WidgetIdSet = rows.iter().map(|row| row.widget_id).collect();
        out.extend(rows.iter().map(|row| {
            let endpoint = cascade
                .endpoint(row.widget_id)
                .expect("a hit row's widget was recorded this frame");
            Self {
                id: row.widget_id,
                rect: row.rect,
                sense: row.sense,
                focusable: row.focusable,
                disabled: row.disabled,
                clipped: row.rect.is_paint_empty(),
                text: own_text(
                    &forest.trees[endpoint.layer],
                    &forest.record_store,
                    endpoint.node.idx(),
                    &listed,
                ),
            }
        }));
    }
}

/// The text shapes under node `root`, leaving out the subtrees of the listed widgets inside it.
fn own_text(tree: &Tree, store: &RecordStore, root: usize, listed: &WidgetIdSet) -> String {
    let ids = tree.records.widget_id();
    let spans = tree.records.shape_span();
    let end = tree.subtree_end_of(root);
    // The shape spans of the nearest listed descendants, ascending: pre-order puts a node's span
    // after its earlier siblings' and inside its parent's.
    let mut skipped = Vec::new();
    let mut node = root + 1;
    while node < end {
        if listed.contains(&ids[node]) {
            skipped.push(spans[node].range());
            node = tree.subtree_end_of(node);
        } else {
            node += 1;
        }
    }
    let mut text = String::new();
    let mut skip = skipped.iter().peekable();
    for idx in spans[root].range() {
        while skip.next_if(|range| range.end <= idx).is_some() {}
        if skip.peek().is_some_and(|range| range.contains(&idx)) {
            continue;
        }
        if let ShapeRecord::Text { text: recorded, .. } = &tree.shapes.records[idx] {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(store.recorded_str(*recorded));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use crate::internals::inspected_widget::InspectedWidget;
    use crate::internals::ui_harness::UiHarness;
    use crate::prelude::*;

    /// A button labelled "Save", a disabled button "Off", a clickable row holding the text "Open"
    /// and a nested button "Inner", a text field holding "draft", and a button scrolled out of a
    /// 40-unit-tall scroll area. Each is listed once with the rect the pointer hits; the row keeps
    /// "Open" and leaves "Inner" to the inner button; only "Off" is disabled; the scrolled-out
    /// button is clipped.
    #[test]
    fn interactive_widgets_are_listed_with_their_own_text() {
        let save = WidgetId::from_hash("inspect.save");
        let off = WidgetId::from_hash("inspect.off");
        let row = WidgetId::from_hash("inspect.row");
        let inner = WidgetId::from_hash("inspect.inner");
        let edit = WidgetId::from_hash("inspect.edit");
        let hidden = WidgetId::from_hash("inspect.hidden");
        let mut draft = String::from("draft");
        let mut scene = |ui: &mut Ui| {
            Panel::vstack().id_salt("inspect.root").show(ui, |ui| {
                Button::new().id(save).label("Save").show(ui);
                Button::new().id(off).label("Off").disabled(true).show(ui);
                Panel::hstack().id(row).sense(Sense::CLICK).show(ui, |ui| {
                    Text::new("Open").show(ui);
                    Button::new().id(inner).label("Inner").show(ui);
                });
                TextEdit::new(&mut draft).id(edit).show(ui);
                Scroll::vertical()
                    .id_salt("inspect.scroll")
                    .size((Sizing::fixed(120.0), Sizing::fixed(40.0)))
                    .show(ui, |ui| {
                        Panel::vstack()
                            .id_salt("inspect.spacer")
                            .size((Sizing::fixed(100.0), Sizing::fixed(200.0)))
                            .show(ui, |_| {});
                        Button::new().id(hidden).label("Hidden").show(ui);
                    });
            });
        };
        let mut h = UiHarness::with_text(UVec2::new(400, 300));
        h.prime(2, &mut scene);

        let mut out = Vec::new();
        h.ui().inspect(&mut out);
        let find = |id: WidgetId| -> &InspectedWidget {
            let found: Vec<_> = out.iter().filter(|widget| widget.id == id).collect();
            assert_eq!(found.len(), 1, "{id:?} is listed once: {out:#?}");
            found[0]
        };

        for (id, text) in [
            (save, "Save"),
            (off, "Off"),
            (row, "Open"),
            (inner, "Inner"),
            (edit, "draft"),
            (hidden, "Hidden"),
        ] {
            let widget = find(id);
            assert_eq!(widget.text, text, "{id:?}");
            assert_eq!(
                Some(widget.rect),
                h.rect(id),
                "{id:?}: the rect the pointer hits"
            );
        }
        assert!(Sense::clicks(find(save).sense), "a button takes clicks");
        assert!(Sense::clicks(find(row).sense), "the row was given CLICK");
        assert!(find(edit).focusable, "a text field takes focus");
        assert!(find(off).disabled && !find(save).disabled && !find(inner).disabled);
        assert!(!find(save).clipped && !find(edit).clipped);
        assert!(find(hidden).clipped, "200 units down a 40-unit scroll area");
    }
}
