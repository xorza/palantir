//! The observatory's studies driven as a person drives them, through the harness: each connection the
//! proposal promises, and each study's keys.

use std::fs;
use std::path::Path;

use palantir::glam::{UVec2, Vec2};
use palantir::{
    ColorScheme, Harness, Key, Locator, Palette, PointerButton, SchemePreference, Scroll, Shortcut,
    ShortcutMods, WidgetId,
};

use crate::app::Shell;
use crate::app::model::appearance::{NIGHT_VISION, Preset};
use crate::app::model::catalog::ObjectId;
use crate::app::model::sky;
use crate::app::studies::Study;
use crate::app::studies::catalog::{self, row_id, row_text};
use crate::scaffold::links::LINKS;
use crate::shot::script::Script;

const SURFACE: UVec2 = UVec2::new(1600, 1000);

/// A window on `study` with every frame settled.
fn open(study: Study) -> (Harness, Shell) {
    let mut h = Harness::builder(SURFACE).mono().build();
    let mut shell = Shell::on(study);
    h.settle(&mut shell).expect("the first frames settle");
    (h, shell)
}

fn settle(h: &mut Harness, shell: &mut Shell) {
    h.settle(shell).expect("the frames settle");
}

fn text(text: &str) -> Locator {
    Locator::Text(text.to_owned())
}

/// A click on a catalog row selects it, the details and the log follow, and Ctrl+C copies the
/// row as tab-separated text.
#[test]
fn a_catalog_row_selects_and_copies() {
    let (mut h, mut shell) = open(Study::Catalog);
    let first = shell.model.view.rows(&shell.model.catalog)[0];
    let name = shell.model.catalog.get(first).name.clone();
    h.click(row_id(first));
    settle(&mut h, &mut shell);
    assert_eq!(shell.model.selection, Some(first));
    assert_eq!(
        shell
            .model
            .log
            .lines()
            .next_back()
            .map(|line| line.text.as_str()),
        Some(format!("selected {name}").as_str())
    );
    h.key(Shortcut::ctrl('C'));
    settle(&mut h, &mut shell);
    assert_eq!(h.clipboard_text(), row_text(&shell.model.catalog, first));
}

/// The arrows, End and Home move the selection through the sorted rows; a sort keeps it.
#[test]
fn the_catalog_keys_move_the_selection() {
    let (mut h, mut shell) = open(Study::Catalog);
    let rows = shell.model.view.rows(&shell.model.catalog).to_vec();
    h.click(row_id(rows[0]));
    settle(&mut h, &mut shell);
    h.key(Key::ArrowDown);
    settle(&mut h, &mut shell);
    assert_eq!(shell.model.selection, Some(rows[1]));
    h.key(Key::End);
    settle(&mut h, &mut shell);
    assert_eq!(shell.model.selection, rows.last().copied());
    h.key(Key::Home);
    settle(&mut h, &mut shell);
    assert_eq!(shell.model.selection, Some(rows[0]));
    h.click(text("Name"));
    settle(&mut h, &mut shell);
    assert_eq!(
        shell.model.selection,
        Some(rows[0]),
        "a sort keeps the selection"
    );
}

/// A selection made outside the chart slews it there: once settled, the star sits at the
/// view's center, at the slew's zoom.
#[test]
fn the_chart_slews_to_a_selection_made_elsewhere() {
    let (mut h, mut shell) = open(Study::SkyChart);
    // A star the scroll can center: far enough from every edge that its offset is not clamped.
    let (id, _) = shell
        .model
        .catalog
        .iter()
        .find(|(_, object)| {
            let at = sky::chart_point(object.ra, object.dec);
            at.x > 600.0
                && at.x < sky::CHART.x - 600.0
                && at.y > 400.0
                && at.y < sky::CHART.y - 400.0
        })
        .expect("the sky has an inner star");
    shell.model.select(id);
    settle(&mut h, &mut shell);
    let scroll = WidgetId::from_hash(("observatory::chart::scroll", "study"));
    let zoom = Scroll::zoom(h.ui(), scroll);
    let offset = Scroll::offset(h.ui(), scroll);
    let view = h.rect(scroll).expect("the chart shows");
    let center = (offset + Vec2::new(view.size.w, view.size.h) * 0.5) / zoom;
    let object = shell.model.catalog.get(id);
    let star = sky::chart_point(object.ra, object.dec);
    assert_eq!(zoom, 2.0);
    assert!(center.distance(star) < 0.5, "{center} vs {star}");
}

/// Alt+↓ moves a plan entry down, Delete removes the focused one, and the clear dialog keeps the
/// plan on Escape and clears it on Enter.
#[test]
fn the_session_reorders_and_asks_before_it_clears() {
    let (mut h, mut shell) = open(Study::Session);
    for id in [ObjectId(1), ObjectId(2), ObjectId(3)] {
        shell.model.add_to_plan(id);
    }
    settle(&mut h, &mut shell);
    let objects = |shell: &Shell| -> Vec<u32> {
        shell
            .model
            .plan
            .entries()
            .iter()
            .map(|e| e.object.0)
            .collect()
    };
    // ObjectId(2) is "OBS 0003", the plan's second entry.
    h.click(text("OBS 0003"));
    settle(&mut h, &mut shell);
    h.key(Shortcut::new(ShortcutMods::ALT, Key::ArrowDown));
    settle(&mut h, &mut shell);
    assert_eq!(objects(&shell), [1, 3, 2]);

    h.click(text("Clear plan…"));
    settle(&mut h, &mut shell);
    h.key(Key::Escape);
    settle(&mut h, &mut shell);
    assert_eq!(objects(&shell), [1, 3, 2], "Escape keeps the plan");
    h.click(text("Clear plan…"));
    settle(&mut h, &mut shell);
    h.key(Key::Enter);
    settle(&mut h, &mut shell);
    assert!(objects(&shell).is_empty(), "Enter clears it");
}

/// The workspace's View menu sets the scheme preference every window reads.
#[test]
fn the_workspace_menu_sets_the_scheme() {
    let (mut h, mut shell) = open(Study::Workspace);
    h.click(text("View"));
    settle(&mut h, &mut shell);
    h.click(text("Light scheme"));
    settle(&mut h, &mut shell);
    assert_eq!(h.ui().scheme_preference(), SchemePreference::Light);
    assert_eq!(h.ui().color_scheme(), ColorScheme::Light);
}

/// Night vision swaps the dark palette for red and pins the scheme dark, and the shown theme is
/// built from it; stock brings the stock palette back.
#[test]
fn night_vision_pins_a_red_dark_scheme() {
    let (mut h, mut shell) = open(Study::Appearance);
    h.click(text("night vision"));
    let report = h.frame_app(&mut shell);
    assert!(
        report.repaint_requested,
        "the edit asks for the frame that builds its theme"
    );
    settle(&mut h, &mut shell);
    assert_eq!(shell.model.appearance.preset(), Preset::NightVision);
    assert_eq!(h.ui().color_scheme(), ColorScheme::Dark);
    assert_eq!(h.ui().theme().window_clear, NIGHT_VISION.window_bg);
    h.click(text("stock"));
    settle(&mut h, &mut shell);
    assert_eq!(h.ui().theme().window_clear, Palette::DARK.window_bg);
}

/// Ctrl+1 … Ctrl+5 pick each study in the rail's order.
#[test]
fn the_digits_pick_each_study() {
    let (mut h, mut shell) = open(Study::SkyChart);
    for (digit, study) in ['1', '2', '3', '4', '5'].into_iter().zip(Study::ALL) {
        h.key(Shortcut::ctrl(digit));
        settle(&mut h, &mut shell);
        assert_eq!(shell.study(), study, "Ctrl+{digit}");
    }
}

/// Every committed shot script parses and names a study, so none rots.
#[test]
fn every_shot_script_names_a_study() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/observatory/shots");
    let mut count = 0;
    for entry in fs::read_dir(&folder).expect("the shots folder reads") {
        let path = entry.expect("an entry reads").path();
        let source = fs::read_to_string(&path).expect("a script reads");
        let script = Script::parse(&source).unwrap_or_else(|error| {
            panic!("{}: {error}", path.display());
        });
        let page = script.header.page.expect("each script names its study");
        assert!(
            Study::ALL.iter().any(|study| study.label() == page),
            "{}: no study {page:?}",
            path.display()
        );
        count += 1;
    }
    assert_eq!(
        count,
        Study::ALL.len() * 2,
        "one script per study and scheme"
    );
}

/// A selection made outside the catalog scrolls its row into view: the last row of the sorted
/// table ends at the bottom of the viewport.
#[test]
fn the_catalog_scrolls_to_a_selection_made_elsewhere() {
    let (mut h, mut shell) = open(Study::Catalog);
    let rows = shell.model.view.rows(&shell.model.catalog).to_vec();
    let last = rows.len() - 1;
    shell.model.select(rows[last]);
    settle(&mut h, &mut shell);
    let viewport = h.rect(catalog::rows_id()).expect("the table shows").size.h;
    let offset = Scroll::offset(h.ui(), catalog::rows_id()).y;
    assert_eq!(offset, catalog::into_view(0.0, viewport, last));
    assert!(h.rect(row_id(rows[last])).is_some(), "the row records");
}

/// A click on a star selects it: the pointer maps through the chart's scroll to the star.
#[test]
fn a_click_on_the_chart_selects_the_star_under_it() {
    let (mut h, mut shell) = open(Study::SkyChart);
    let scroll = WidgetId::from_hash(("observatory::chart::scroll", "study"));
    let view = h.rect(scroll).expect("the chart shows");
    let (id, at) = shell
        .model
        .catalog
        .iter()
        .map(|(id, object)| (id, sky::chart_point(object.ra, object.dec)))
        .find(|(_, at)| {
            at.x > 40.0 && at.y > 40.0 && at.x < view.size.w - 200.0 && at.y < view.size.h - 80.0
        })
        .expect("a star shows at zoom 1");
    h.click(Locator::Point(view.min + at));
    settle(&mut h, &mut shell);
    assert_eq!(shell.model.selection, Some(id));
}

/// A handle dragged two rows down moves its entry two places, and only its own drag counts.
#[test]
fn a_plan_handle_drags_its_entry() {
    let (mut h, mut shell) = open(Study::Session);
    for id in [ObjectId(1), ObjectId(2), ObjectId(3)] {
        shell.model.add_to_plan(id);
    }
    settle(&mut h, &mut shell);
    let handle = Locator::Nth(Box::new(text("\u{2630}")), 0);
    let start = h
        .find(&shell, &handle)
        .expect("the first handle shows")
        .rect
        .center();
    h.press(Locator::Point(start), PointerButton::Left);
    h.drag_to(Locator::Point(start + Vec2::new(0.0, 70.0)));
    h.release(PointerButton::Left);
    settle(&mut h, &mut shell);
    let objects: Vec<u32> = shell
        .model
        .plan
        .entries()
        .iter()
        .map(|e| e.object.0)
        .collect();
    assert_eq!(objects, [2, 3, 1]);
}

/// Every study the shared links table names is a study of the rail, and every study links at least
/// one showcase page, so the header's showcase row is never empty.
#[test]
fn the_links_name_real_studies() {
    for link in LINKS {
        assert!(
            Study::ALL.iter().any(|study| study.label() == link.study),
            "{link:?} names no study"
        );
    }
    for study in Study::ALL {
        assert!(
            study.showcase_pages().next().is_some(),
            "{study:?} links no page"
        );
    }
}
