//! Layout mechanics: Sizing (Fixed / Hug / Fill), child alignment with
//! per-child override, Justify, padding / margin / negative margin, gap,
//! and Visibility. The colored chips are demo content — they visualize
//! where layout puts each child.

use crate::support;
use crate::support::{section, swatch_bg, well_bg};
use palantir::{
    Align, Color, Configure, Frame, HAlign, Justify, Panel, Sizing, Ui, VAlign, Visibility,
};
use std::hash::Hash;

pub(crate) fn build(ui: &mut Ui) {
    Panel::hstack()
        .id_salt("columns")
        .gap(24.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            column(ui, "col-l", |ui| {
                sizing(ui);
                justify(ui);
                visibility(ui);
            });
            column(ui, "col-r", |ui| {
                alignment(ui);
                spacing(ui);
                gap(ui);
            });
        });
}

fn column(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .id_salt(id)
        .gap(support::PAGE_GAP)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, body);
}

fn sizing(ui: &mut Ui) {
    section(
        ui,
        "sizing",
        "sizing — Fixed exact px · Hug content · Fill splits leftover 1:2:1",
        |ui| {
            support::row(ui, "sz-fixed", |ui| {
                for (i, w) in [50.0, 100.0, 200.0].into_iter().enumerate() {
                    chip(
                        ui,
                        ("fx", i),
                        (Sizing::fixed(w), Sizing::fixed(32.0)),
                        support::B,
                    );
                }
            });
            support::row(ui, "sz-hug", |ui| {
                // Padded frames hug their empty content box — effectively
                // just padding, so the two boxes differ only by pad width.
                for (i, pad) in [20.0, 40.0].into_iter().enumerate() {
                    Frame::new()
                        .id_salt(("hug", i))
                        .size((Sizing::HUG, Sizing::fixed(32.0)))
                        .padding((pad, 0.0, pad, 0.0))
                        .background(swatch_bg(support::C))
                        .show(ui);
                }
            });
            support::row(ui, "sz-fill", |ui| {
                for (i, weight) in [1.0, 2.0, 1.0].into_iter().enumerate() {
                    chip(
                        ui,
                        ("fill", i),
                        (Sizing::fill(weight), Sizing::fixed(32.0)),
                        support::A,
                    );
                }
            });
        },
    );
}

fn justify(ui: &mut Ui) {
    section(
        ui,
        "justify",
        "justify — Start · Center · End · SpaceBetween · SpaceAround",
        |ui| {
            for (id, j) in [
                ("j-start", Justify::Start),
                ("j-center", Justify::Center),
                ("j-end", Justify::End),
                ("j-between", Justify::SpaceBetween),
                ("j-around", Justify::SpaceAround),
            ] {
                Panel::hstack()
                    .id_salt(id)
                    .size((Sizing::FILL, Sizing::fixed(32.0)))
                    .padding((6.0, 4.0, 6.0, 4.0))
                    .justify(j)
                    .background(well_bg())
                    .show(ui, |ui| {
                        for i in 0..3 {
                            chip(
                                ui,
                                (id, i),
                                (Sizing::fixed(36.0), Sizing::fixed(22.0)),
                                support::A,
                            );
                        }
                    });
            }
        },
    );
}

fn visibility(ui: &mut Ui) {
    section(
        ui,
        "visibility",
        "visibility — middle chip Visible · Hidden keeps its slot · Collapsed releases it",
        |ui| {
            for (id, vis) in [
                ("v-visible", Visibility::Visible),
                ("v-hidden", Visibility::Hidden),
                ("v-collapsed", Visibility::Collapsed),
            ] {
                Panel::hstack()
                    .id_salt(id)
                    .size((Sizing::FILL, Sizing::fixed(44.0)))
                    .padding(6.0)
                    .gap(12.0)
                    .background(well_bg())
                    .show(ui, |ui| {
                        for (key, c, v) in [
                            ("a", support::A, Visibility::Visible),
                            ("mid", support::B, vis),
                            ("c", support::C, Visibility::Visible),
                        ] {
                            Frame::new()
                                .id_salt((id, key))
                                .size((Sizing::fixed(70.0), Sizing::fixed(28.0)))
                                .visibility(v)
                                .background(swatch_bg(c))
                                .show(ui);
                        }
                    });
            }
        },
    );
}

fn alignment(ui: &mut Ui) {
    section(
        ui,
        "alignment",
        "alignment — child_align on the container, overridden per child by the orange chip",
        |ui| {
            // HStack: children inherit VAlign::Center; orange opts out to Bottom.
            Panel::hstack()
                .id_salt("al-h")
                .size((Sizing::FILL, Sizing::fixed(96.0)))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::v(VAlign::Center))
                .background(well_bg())
                .show(ui, |ui| {
                    aligned_chip(ui, "a", support::A, Align::default());
                    aligned_chip(ui, "b", support::A, Align::default());
                    aligned_chip(ui, "c-self-bot", support::B, Align::v(VAlign::Bottom));
                    aligned_chip(ui, "d", support::A, Align::default());
                });
            // VStack: children packed to the right edge; orange opts out to Left.
            Panel::vstack()
                .id_salt("al-v")
                .size((Sizing::FILL, Sizing::fixed(110.0)))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::h(HAlign::Right))
                .background(well_bg())
                .show(ui, |ui| {
                    aligned_chip(ui, "a-vs", support::A, Align::default());
                    aligned_chip(ui, "b-self-left", support::B, Align::h(HAlign::Left));
                    aligned_chip(ui, "c-vs", support::A, Align::default());
                });
        },
    );
}

fn spacing(ui: &mut Ui) {
    section(
        ui,
        "spacing",
        "spacing — padding reserves space inside the parent · margin shrinks the \
         child's slot · negative margin overlaps the neighbor",
        |ui| {
            Panel::hstack()
                .id_salt("p-row")
                .size((Sizing::FILL, Sizing::fixed(60.0)))
                .padding(20.0)
                .gap(8.0)
                .background(well_bg())
                .show(ui, |ui| {
                    for i in 0..3 {
                        chip(
                            ui,
                            ("p", i),
                            (Sizing::fixed(40.0), Sizing::FILL),
                            support::A,
                        );
                    }
                });
            Panel::hstack()
                .id_salt("m-row")
                .size((Sizing::FILL, Sizing::fixed(60.0)))
                .gap(8.0)
                .background(well_bg())
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("m1")
                        .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                        .margin(8.0)
                        .background(swatch_bg(support::A))
                        .show(ui);
                    Frame::new()
                        .id_salt("m2")
                        .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                        .margin((16.0, 16.0, 0.0, 0.0))
                        .background(swatch_bg(support::A))
                        .show(ui);
                });
            // The orange box is anchored after the teal one, but its left
            // margin pulls it backwards 30 px so the two overlap.
            Panel::hstack()
                .id_salt("neg-row")
                .size((Sizing::FILL, Sizing::fixed(60.0)))
                .padding(8.0)
                .background(well_bg())
                .show(ui, |ui| {
                    chip(
                        ui,
                        ("neg", "a"),
                        (Sizing::fixed(80.0), Sizing::fixed(40.0)),
                        support::A,
                    );
                    Frame::new()
                        .id_salt(("neg", "b"))
                        .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                        .margin((-30.0, 0.0, 0.0, 0.0))
                        .background(swatch_bg(support::B))
                        .show(ui);
                });
        },
    );
}

fn gap(ui: &mut Ui) {
    section(ui, "gap", "gap — 0 · 8 · 24 px between siblings", |ui| {
        for g in [0.0, 8.0, 24.0] {
            Panel::hstack()
                .id_salt(("gap", g as u32))
                .size((Sizing::FILL, Sizing::fixed(40.0)))
                .padding(6.0)
                .gap(g)
                .background(well_bg())
                .show(ui, |ui| {
                    for i in 0..5 {
                        chip(
                            ui,
                            ("gap-tile", g as u32, i),
                            (Sizing::fixed(32.0), Sizing::fixed(24.0)),
                            support::A,
                        );
                    }
                });
        }
    });
}

fn chip<H: Hash>(ui: &mut Ui, id: H, size: (Sizing, Sizing), c: Color) {
    Frame::new()
        .id_salt(id)
        .size(size)
        .background(swatch_bg(c))
        .show(ui);
}

fn aligned_chip(ui: &mut Ui, id: &'static str, c: Color, align: Align) {
    Frame::new()
        .id_salt(id)
        .size((Sizing::fixed(56.0), Sizing::fixed(24.0)))
        .align(align)
        .background(swatch_bg(c))
        .show(ui);
}
