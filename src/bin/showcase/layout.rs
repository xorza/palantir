//! Layout mechanics on one page: Sizing (Fixed / Hug / Fill), child
//! alignment with per-child override, Justify, padding / margin /
//! negative margin, gap, and Visibility. The colored chips are demo
//! content — they visualize where layout puts each child.

use crate::support;
use crate::support::{panel_bg, section, swatch_bg};
use aperture::{
    Align, Color, Configure, Frame, HAlign, Justify, Panel, Sizing, Ui, VAlign, Visibility,
};
use std::hash::Hash;

pub fn build(ui: &mut Ui) {
    support::page(ui, |ui| {
        support::header(
            ui,
            "Layout mechanics — sizing, justify, visibility (left) · alignment, \
             spacing, gap (right). Colored chips visualize where layout puts each child.",
        );
        Panel::hstack()
            .auto_id()
            .gap(24.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::vstack()
                    .id_salt("col-l")
                    .gap(16.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        sizing(ui);
                        justify(ui);
                        visibility(ui);
                    });
                Panel::vstack()
                    .id_salt("col-r")
                    .gap(16.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        alignment(ui);
                        spacing(ui);
                        gap(ui);
                    });
            });
    });
}

fn sizing(ui: &mut Ui) {
    section(
        ui,
        "sizing",
        "sizing — Fixed exact px (orange) · Hug content (green) · Fill splits leftover 1:2:1 (teal)",
        |ui| {
            support::row(ui, "sz-fixed", |ui| {
                for (i, w) in [50.0, 100.0, 200.0].into_iter().enumerate() {
                    chip(
                        ui,
                        ("fx", i),
                        (Sizing::Fixed(w), Sizing::Fixed(32.0)),
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
                        .size((Sizing::Hug, Sizing::Fixed(32.0)))
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
                        (Sizing::Fill(weight), Sizing::Fixed(32.0)),
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
        "justify — Start / Center / End / SpaceBetween / SpaceAround",
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
                    .size((Sizing::FILL, Sizing::Fixed(32.0)))
                    .padding((6.0, 4.0, 6.0, 4.0))
                    .justify(j)
                    .background(panel_bg())
                    .show(ui, |ui| {
                        for i in 0..3 {
                            chip(
                                ui,
                                (id, i),
                                (Sizing::Fixed(36.0), Sizing::Fixed(22.0)),
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
        "visibility — middle chip Visible / Hidden (keeps its slot) / Collapsed (releases it)",
        |ui| {
            for (id, vis) in [
                ("v-visible", Visibility::Visible),
                ("v-hidden", Visibility::Hidden),
                ("v-collapsed", Visibility::Collapsed),
            ] {
                Panel::hstack()
                    .id_salt(id)
                    .size((Sizing::FILL, Sizing::Fixed(44.0)))
                    .padding(6.0)
                    .gap(12.0)
                    .background(panel_bg())
                    .show(ui, |ui| {
                        for (key, c, v) in [
                            ("a", support::A, Visibility::Visible),
                            ("mid", support::B, vis),
                            ("c", support::C, Visibility::Visible),
                        ] {
                            Frame::new()
                                .id_salt((id, key))
                                .size((Sizing::Fixed(70.0), Sizing::Fixed(28.0)))
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
        "child_align on the container — the orange chip overrides per-child",
        |ui| {
            // HStack: children inherit VAlign::Center; orange opts out to Bottom.
            Panel::hstack()
                .id_salt("al-h")
                .size((Sizing::FILL, Sizing::Fixed(96.0)))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::v(VAlign::Center))
                .background(panel_bg())
                .show(ui, |ui| {
                    aligned_chip(ui, "a", support::A, Align::default());
                    aligned_chip(ui, "b", support::A, Align::default());
                    aligned_chip(ui, "c-self-bot", support::B, Align::v(VAlign::Bottom));
                    aligned_chip(ui, "d", support::A, Align::default());
                });
            // VStack: children packed to the right edge; orange opts out to Left.
            Panel::vstack()
                .id_salt("al-v")
                .size((Sizing::FILL, Sizing::Fixed(110.0)))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::h(HAlign::Right))
                .background(panel_bg())
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
        "spacing — padding reserves space inside the parent (top) · margin shrinks the \
         child's slot (middle) · negative margin overlaps the neighbor (bottom)",
        |ui| {
            Panel::hstack()
                .id_salt("p-row")
                .size((Sizing::FILL, Sizing::Fixed(60.0)))
                .padding(20.0)
                .gap(8.0)
                .background(panel_bg())
                .show(ui, |ui| {
                    for i in 0..3 {
                        chip(
                            ui,
                            ("p", i),
                            (Sizing::Fixed(40.0), Sizing::FILL),
                            support::A,
                        );
                    }
                });
            Panel::hstack()
                .id_salt("m-row")
                .size((Sizing::FILL, Sizing::Fixed(60.0)))
                .gap(8.0)
                .background(panel_bg())
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("m1")
                        .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                        .margin(8.0)
                        .background(swatch_bg(support::A))
                        .show(ui);
                    Frame::new()
                        .id_salt("m2")
                        .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                        .margin((16.0, 16.0, 0.0, 0.0))
                        .background(swatch_bg(support::A))
                        .show(ui);
                });
            // The orange box is anchored after the teal one, but its left
            // margin pulls it backwards 30 px so the two overlap.
            Panel::hstack()
                .id_salt("neg-row")
                .size((Sizing::FILL, Sizing::Fixed(60.0)))
                .padding(8.0)
                .background(panel_bg())
                .show(ui, |ui| {
                    chip(
                        ui,
                        ("neg", "a"),
                        (Sizing::Fixed(80.0), Sizing::Fixed(40.0)),
                        support::A,
                    );
                    Frame::new()
                        .id_salt(("neg", "b"))
                        .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                        .margin((-30.0, 0.0, 0.0, 0.0))
                        .background(swatch_bg(support::B))
                        .show(ui);
                });
        },
    );
}

fn gap(ui: &mut Ui) {
    section(ui, "gap", "gap — 0 / 8 / 24 px between siblings", |ui| {
        for g in [0.0, 8.0, 24.0] {
            Panel::hstack()
                .id_salt(("gap", g as u32))
                .size((Sizing::FILL, Sizing::Fixed(40.0)))
                .padding(6.0)
                .gap(g)
                .background(panel_bg())
                .show(ui, |ui| {
                    for i in 0..5 {
                        chip(
                            ui,
                            ("gap-tile", g as u32, i),
                            (Sizing::Fixed(32.0), Sizing::Fixed(24.0)),
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
        .size((Sizing::Fixed(56.0), Sizing::Fixed(24.0)))
        .align(align)
        .background(swatch_bg(c))
        .show(ui);
}
