//! The two tab widgets: a page view bound to an index, and the strip on
//! its own with content of the app's, close buttons and overflow.

use crate::scaffold::api;
use crate::scaffold::tokens::M;
use crate::support;
use crate::support::{Kit, Section};
use palantir::{
    Block, Button, Chrome, Configure, Corners, Panel, SizeSpec, Sizing, TabChip, TabItem, TabKey,
    TabMove, TabOverflow, TabStrip, TabbedView, Text, Ui, WidgetId, fmt,
};

/// The pages the view may hold; it opens on the first three, and "+" adds the next missing.
const POOL: [&str; 6] = [
    "Color", "Geometry", "Metadata", "History", "Notes", "Export",
];

const MANY: [&str; 9] = [
    "curves", "levels", "sharpen", "denoise", "rotate", "crop", "vignette", "grain", "export",
];

/// The last change the view asked of its pages.
#[derive(Clone, Copy, Debug)]
enum Edit {
    Added(&'static str),
    Closed(&'static str),
    Moved { page: &'static str, to: usize },
}

#[derive(Debug)]
struct State {
    page: usize,
    /// The view's pages, in their order, which a close, a drag and "+" change.
    pages: Vec<&'static str>,
    last: Option<Edit>,
    picked: usize,
    overflowing: usize,
    /// Chips the strip demo has closed, so a close reads as a real removal.
    open: Vec<u64>,
    /// The frame's chips, rebuilt every frame since labels are interned per frame, into this buffer to keep its capacity.
    items: Vec<TabItem>,
}

impl State {
    /// Apply what the view asked of the page list: a close, the last page excepted, and a move.
    fn apply(&mut self, closed: Option<usize>, reordered: Option<TabMove>) {
        if let Some(idx) = closed
            && self.pages.len() > 1
        {
            let page = self.pages.remove(idx);
            self.last = Some(Edit::Closed(page));
        }
        if let Some(moved) = reordered {
            let page = self.pages[moved.from];
            moved.apply(&mut self.pages);
            self.last = Some(Edit::Moved {
                page,
                to: moved.landing(),
            });
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            page: 0,
            pages: POOL[..3].to_vec(),
            last: None,
            picked: 1,
            overflowing: 0,
            open: (0..4).collect(),
            items: Vec::with_capacity(MANY.len()),
        }
    }
}

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    let state_id = WidgetId::from_hash("showcase::tabs::state");
    ui.with_state::<State, _>(state_id, |ui, s| {
        Section::new("Tabbed view")
            .apis(&[api!(type TabbedView)])
            .show(ui, k, |ui| {
                support::note(
                    ui,
                    k,
                    "A strip over a content area, bound to a &mut usize as a combo box is. Click a \
                     chip, or focus the strip and move with the arrow keys, Home and End. Drag a \
                     chip to reorder, close one with its button; the view names each change, and \
                     the page applies it to its own list.",
                );
                Panel::vstack()
                    .size((Sizing::FILL, Sizing::fixed(180.0)))
                    .padding(M.space.md)
                    .chrome(k.t.card())
                    .show(ui, |ui| tabbed_view(ui, k, s));
                support::row(ui, |ui| {
                    let next = POOL.into_iter().find(|page| !s.pages.contains(page));
                    if Button::new()
                        .label("+ Add page")
                        .disabled(next.is_none())
                        .show(ui)
                        .clicked()
                        && let Some(page) = next
                    {
                        s.pages.push(page);
                        s.page = s.pages.len() - 1;
                        s.last = Some(Edit::Added(page));
                    }
                    let last = match s.last {
                        None => ui.intern("—"),
                        Some(Edit::Added(page)) => fmt!(ui, "added {page}"),
                        Some(Edit::Closed(page)) => fmt!(ui, "closed {page}"),
                        Some(Edit::Moved { page, to }) => fmt!(ui, "moved {page} to slot {to}"),
                    };
                    support::readout(ui, k, "last edit", last);
                });
            });

        Section::new("The strip alone")
            .apis(&[api!(TabStrip::new), api!(TabChip::show_label)])
            .show(ui, k, |ui| {
                support::note(
                    ui,
                    k,
                    "TabStrip draws each chip's frame and close button, and the app records what \
                     the chip shows: here the label and, on the even layers, a dot in the \
                     chip's ink. Close a chip with its button.",
                );
                strip_demo(ui, k, s);
            });

        Section::new("Overflow")
            .apis(&[api!(TabOverflow::Menu)])
            .show(ui, k, |ui| {
                support::note(
                    ui,
                    k,
                    "More chips than room. They pan under the wheel, and the trailing button lists \
                 every tab so one that scrolled out is still reachable.",
                );
                Panel::vstack()
                    .size((Sizing::fixed(320.0), Sizing::HUG))
                    .padding(M.space.md)
                    .chrome(k.t.card())
                    .show(ui, |ui| {
                        s.items.clear();
                        for (i, label) in MANY.iter().enumerate() {
                            s.items.push(TabItem::new(TabKey::of(i), ui.intern(*label)));
                        }
                        let hit = TabStrip::new(&s.items)
                            .selected_idx(s.overflowing)
                            .overflow(TabOverflow::Menu)
                            .show(ui, |ui, chip| chip.show_label(ui));
                        if let Some(i) = hit.activated {
                            s.overflowing = i;
                        }
                    });
                support::readout(ui, k, "showing", MANY[s.overflowing]);
            });
    });
}

fn strip_demo(ui: &mut Ui, k: Kit, s: &mut State) {
    support::well(ui, k, |ui| {
        s.items.clear();
        for &key in &s.open {
            s.items.push(TabItem {
                closable: true,
                ..TabItem::new(TabKey::of(key), fmt!(ui, "layer {key}"))
            });
        }
        let open = &s.open;
        let hit = TabStrip::new(&s.items)
            .selected_idx(s.picked)
            .show(ui, |ui, chip| layer_chip(ui, chip, open[chip.idx]));
        if let Some(slot) = hit.closed
            && s.open.len() > 1
        {
            s.open.remove(slot);
            s.picked = s.picked.min(s.open.len() - 1);
        } else if let Some(slot) = hit.activated {
            s.picked = slot;
        }
    });
    let line = fmt!(ui, "{} open, slot {} selected", s.open.len(), s.picked);
    support::readout(ui, k, "chips", line);
}

/// The chip of `layer`: its label, and a dot on the even layers in the chip's ink.
fn layer_chip(ui: &mut Ui, chip: &TabChip, layer: u64) {
    chip.show_label(ui);
    if layer.is_multiple_of(2) {
        Block::new()
            .id(chip.id.with("dot"))
            .size((Sizing::fixed(6.0), Sizing::fixed(6.0)))
            .chrome(Chrome::rounded(chip.text.color, Corners::all(3.0)))
            .show(ui);
    }
}

/// The view over the page's own list, applying each close and move it reports; the last page
/// stays, so the view is never empty.
fn tabbed_view(ui: &mut Ui, k: Kit, s: &mut State) {
    let response = TabbedView::new(&mut s.page, &s.pages)
        .closable(true)
        .show(ui, |ui, page| {
            Panel::vstack()
                .size(SizeSpec::FILL)
                .padding(M.space.md)
                .gap(M.space.sm)
                .show(ui, |ui| {
                    Text::new(s.pages[page]).style(&k.t.body()).show(ui);
                    let line = fmt!(ui, "page index {page}");
                    Text::new(line).style(&k.t.caption()).show(ui);
                });
        });
    let (closed, reordered) = (response.closed, response.reordered);
    s.apply(closed, reordered);
}

#[cfg(test)]
mod tests {
    use palantir::TabMove;

    use crate::pages::tabs::State;

    /// A move addresses the list before it: "Color" from 0 to 2 passes "Geometry" and lands at
    /// 1; "Metadata" from 2 to 0 lands first; a close removes its page; the last page stays.
    #[test]
    fn the_view_s_edits_apply_to_the_page_list() {
        let mut s = State::default();
        s.apply(None, Some(TabMove { from: 0, to: 2 }));
        assert_eq!(s.pages, ["Geometry", "Color", "Metadata"]);
        s.apply(None, Some(TabMove { from: 2, to: 0 }));
        assert_eq!(s.pages, ["Metadata", "Geometry", "Color"]);
        s.apply(Some(1), None);
        assert_eq!(s.pages, ["Metadata", "Color"]);
        s.apply(Some(0), None);
        s.apply(Some(0), None);
        assert_eq!(s.pages, ["Color"], "the last page stays");
    }
}
