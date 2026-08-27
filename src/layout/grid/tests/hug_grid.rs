//! A hugging grid: what an empty fill track collapses to, and what a nested
//! floor pushes back.

use crate::layout::types::{sizing::Sizing, track::Track};
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, grid::Grid, panel::Panel};
use glam::UVec2;

#[test]
fn grid_hug_grid_collapses_empty_fill_tracks() {
    let mut h = UiHarness::new(UVec2::new(400, 200));
    let mut grid_node = None;
    h.frame(|ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                grid_node = Some(
                    Grid::new()
                        .id(WidgetId::from_hash("hug-grid"))
                        .cols([Track::fixed(80.0), Track::fill()])
                        .rows([Track::fixed(40.0)])
                        .size((Sizing::HUG, Sizing::HUG))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("a"))
                                .grid_cell((0, 0))
                                .show(ui);
                            Frame::new()
                                .id(WidgetId::from_hash("b"))
                                .grid_cell((0, 1))
                                .show(ui);
                        })
                        .response
                        .node(),
                );
            });
    });
    let r = h
        .layout_rect(WidgetId::from_hash("hug-grid"))
        .expect("arranged");
    assert_eq!(r.size.w, 80.0, "empty Fill col contributes no floor");
    assert_eq!(r.size.h, 40.0);
}

#[test]
fn hug_grid_fill_track_contributes_nested_rigid_floor() {
    let mut h = UiHarness::new(UVec2::new(400, 100));
    let mut rigid_node = None;
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::fill()])
            .rows([Track::fixed(20.0)])
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("cell"))
                    .grid_cell((0, 0))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        rigid_node = Some(
                            Frame::new()
                                .id(WidgetId::from_hash("rigid"))
                                .size((Sizing::fixed(120.0), Sizing::fixed(20.0)))
                                .show(ui)
                                .node(),
                        );
                    });
            })
            .response
            .node()
    });

    let grid = h.ui.arranged_rect(Layer::Main, root);
    let cell = h.main_child_rects(root)[0];
    let rigid = h
        .layout_rect(WidgetId::from_hash("rigid"))
        .expect("arranged");
    assert_eq!(grid.size, Size::new(120.0, 20.0));
    assert_eq!(cell.size, Size::new(120.0, 20.0));
    assert_eq!(rigid.size, Size::new(120.0, 20.0));
}

#[test]
fn stack_fill_sibling_yields_to_grid_fill_track_rigid_floor() {
    let mut h = UiHarness::new(UVec2::new(300, 40));
    let mut rigid_node = None;
    let root = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Grid::new()
                    .id(WidgetId::from_hash("grid"))
                    .cols([Track::fill()])
                    .rows([Track::fill()])
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Panel::hstack()
                            .id(WidgetId::from_hash("cell"))
                            .grid_cell((0, 0))
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                rigid_node = Some(
                                    Frame::new()
                                        .id(WidgetId::from_hash("rigid"))
                                        .size((Sizing::fixed(200.0), Sizing::FILL))
                                        .show(ui)
                                        .node(),
                                );
                            });
                    });
                Frame::new()
                    .id(WidgetId::from_hash("shrinkable"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui);
            })
            .response
            .node()
    });

    let siblings = h.main_child_rects(root);
    let rigid = h
        .layout_rect(WidgetId::from_hash("rigid"))
        .expect("arranged");
    assert_eq!(siblings[0].size.w, 200.0);
    assert_eq!(siblings[1].min.x, 200.0);
    assert_eq!(siblings[1].size.w, 100.0);
    assert_eq!(rigid.size.w, 200.0);
}
