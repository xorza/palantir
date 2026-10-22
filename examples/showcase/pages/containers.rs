//! Container drivers: stacks, Canvas, wrap stacks and Grid with fixed, fill, hug and clamped tracks plus spanning.

use crate::support;
use crate::support::{DemoCell, Section, api};
use palantir::{
    Align, Block, Chrome, Configure, Corners, Grid, GridCell, Justify, Panel, RgbaF32, Sizing,
    Stroke, Text, TextStyle, Track, Ui,
};
use std::hash::Hash;

pub(crate) fn build(ui: &mut Ui) {
    Section::new("Stacks")
        .apis(&[
            api!(Panel::hstack),
            api!(Panel::vstack),
            api!(Panel::zstack),
            api!(Panel::canvas),
        ])
        .show(ui, |ui| {
            support::tiles(ui, |ui| {
                DemoCell::new("HStack — left to right").show(ui, |ui| {
                    Panel::hstack().gap(6.0).show(ui, |ui| {
                        for k in ["h-a", "h-b", "h-c"] {
                            sw(40.0, 40.0, support::A).id_salt(k).show(ui);
                        }
                    });
                });
                DemoCell::new("VStack — top to bottom").show(ui, |ui| {
                    Panel::vstack().gap(6.0).show(ui, |ui| {
                        for k in ["v-a", "v-b", "v-c"] {
                            sw(60.0, 24.0, support::A).id_salt(k).show(ui);
                        }
                    });
                });
                DemoCell::new("ZStack — stacked in record order").show(ui, |ui| {
                    Panel::zstack().child_align(Align::CENTER).show(ui, |ui| {
                        sw(96.0, 96.0, support::A).id_salt("z-back").show(ui);
                        sw(56.0, 56.0, support::B).id_salt("z-front").show(ui);
                    });
                });
                DemoCell::new("Canvas — positioned children").show(ui, |ui| {
                    Panel::canvas()
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            positioned(8.0, 8.0, support::A).id_salt("p1").show(ui);
                            positioned(64.0, 40.0, support::B).id_salt("p2").show(ui);
                            positioned(30.0, 88.0, support::C).id_salt("p3").show(ui);
                        });
                });
            });
        });

    Section::new("Wrapping flow")
        .apis(&[api!(Panel::wrap_hstack), api!(Panel::line_gap)])
        .show(ui, |ui| {
            support::note(
                ui,
                "Children flow along the main axis and wrap when the next one would not fit. \
                 The gap spaces siblings in a line, and the line gap spaces the lines.",
            );
            Panel::wrap_hstack()
                .size((Sizing::FILL, Sizing::HUG))
                .gap(8.0)
                .line_gap(8.0)
                .show(ui, |ui| {
                    for (i, t) in TAGS.iter().enumerate() {
                        chip(ui, ("tag", i), t);
                    }
                });
        });

    Section::new("Wrap axis")
        .apis(&[api!(Panel::wrap_vstack)])
        .show(ui, |ui| {
            support::note(
                ui,
                "A vertical wrap stack starts a new column. Justification applies to each \
                 line, not to the block of lines.",
            );
            support::tiles(ui, |ui| {
                DemoCell::new("WrapVStack — wraps into columns")
                    .size(200.0, 200.0)
                    .show(ui, |ui| {
                        Panel::wrap_vstack()
                            .size((Sizing::HUG, Sizing::FILL))
                            .gap(6.0)
                            .line_gap(12.0)
                            .show(ui, |ui| {
                                for i in 0..12 {
                                    badge(ui, ("v", i), 70.0);
                                }
                            });
                    });
                DemoCell::new("WrapHStack — Justify::Center per line")
                    .size(200.0, 200.0)
                    .show(ui, |ui| {
                        Panel::wrap_hstack()
                            .size((Sizing::FILL, Sizing::HUG))
                            .gap(10.0)
                            .line_gap(10.0)
                            .justify(Justify::Center)
                            .show(ui, |ui| {
                                for i in 0..7 {
                                    badge(ui, ("c", i), 52.0);
                                }
                            });
                    });
            });
        });

    Section::new("Grid")
        .apis(&[api!(type Track), api!(type GridCell)])
        .show(ui, |ui| {
            support::note(
                ui,
                "Two-axis tracks — fixed, Fill, Hug and clamped — and cells that span. Resize \
                 the window to see the Fill tracks divide the room again.",
            );
            support::tiles(ui, |ui| {
                // Three-column shell: fixed sidebar, flexible content, hugging right rail; the header spans all three.
                DemoCell::new("app shell — fixed | fill | hug, header spans")
                    .size(280.0, 200.0)
                    .show(ui, |ui| {
                        Grid::new()
                            .cols([Track::fixed(80.0), Track::FILL, Track::HUG])
                            .rows([Track::fixed(32.0), Track::FILL])
                            .line_gap(8.0)
                            .gap(8.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                GridTile::new(
                                    "header",
                                    GridCell::at(0, 0).with_span(1, 3),
                                    support::B,
                                )
                                .show(ui);
                                GridTile::new("nav", GridCell::at(1, 0), support::C).show(ui);
                                GridTile::new("content", GridCell::at(1, 1), support::A).show(ui);
                                GridTile::new("rail", GridCell::at(1, 2), support::D)
                                    .size((Sizing::fixed(56.0), Sizing::FILL))
                                    .show(ui);
                            });
                    });
                // The left Fill is bounded [110, 160]; the right Fill absorbs the rest.
                DemoCell::new("clamped track — Fill min 110 max 160 | Fill")
                    .size(280.0, 200.0)
                    .show(ui, |ui| {
                        Grid::new()
                            .cols([
                                Track::fill(1.0).with_min(110.0).with_max(160.0),
                                Track::fill(2.0),
                            ])
                            .rows([Track::FILL])
                            .line_gap(8.0)
                            .gap(8.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                GridTile::new("sidebar", GridCell::at(0, 0), support::A).show(ui);
                                GridTile::new("main", GridCell::at(0, 1), support::D).show(ui);
                            });
                    });
            });
        });
}

const TAGS: &[&str] = &[
    "rust",
    "wgpu",
    "layout",
    "intrinsic",
    "WrapHStack",
    "flexbox-ish",
    "grid",
    "tags",
    "demo",
    "hug",
    "fill",
    "fixed",
    "padding",
    "margin",
    "z-order",
    "sdf",
    "rounded",
    "stroke",
    "alpha",
    "linear",
];

fn sw(w: f32, h: f32, c: RgbaF32) -> Block {
    support::swatch(c).size((Sizing::fixed(w), Sizing::fixed(h)))
}

fn positioned(x: f32, y: f32, c: RgbaF32) -> Block {
    Block::new().pos((x, y)).size(44.0).chrome(Chrome::fill(c))
}

fn chip<H: Hash>(ui: &mut Ui, key: H, label: &'static str) {
    let a = support::A;
    Panel::hstack()
        .id_salt(("chip-row", &key))
        .padding((10.0, 4.0))
        .chrome(
            Chrome::rounded(a.with_alpha(0.20), Corners::all(10.0))
                .with_border(Stroke::new(a.with_alpha(0.45), 1.0)),
        )
        .show(ui, |ui| {
            Text::new(label)
                .id_salt(("chip-label", &key))
                .style(&TextStyle::default().with_font_size(12.0))
                .show(ui);
        });
}

fn badge<H: Hash>(ui: &mut Ui, key: H, w: f32) {
    support::swatch(support::A)
        .id_salt(("badge", &key))
        .size((Sizing::fixed(w), Sizing::fixed(24.0)))
        .show(ui);
}

/// A labeled swatch filling one grid cell, or a span of them.
#[derive(Debug)]
#[must_use]
struct GridTile {
    label: &'static str,
    cell: GridCell,
    size: Option<(Sizing, Sizing)>,
    color: RgbaF32,
}

impl GridTile {
    const fn new(label: &'static str, cell: GridCell, color: RgbaF32) -> Self {
        Self {
            label,
            cell,
            size: None,
            color,
        }
    }

    /// A size of its own in place of the cell's.
    const fn size(mut self, size: (Sizing, Sizing)) -> Self {
        self.size = Some(size);
        self
    }

    #[track_caller]
    fn show(self, ui: &mut Ui) {
        let mut tile = Panel::zstack()
            .auto_id()
            .padding(5.0)
            .grid_cell(self.cell)
            .chrome(support::swatch_chrome(self.color));
        if let Some(size) = self.size {
            tile = tile.size(size);
        }
        tile.show(ui, |ui| {
            Text::new(self.label)
                .style(&support::on_swatch_style().with_font_size(11.0))
                .show(ui);
        });
    }
}
