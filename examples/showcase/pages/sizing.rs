//! Layout mechanics: Sizing (Fixed / Hug / Fill), child alignment with
//! per-child override, Justify, padding / margin / negative margin, gap,
//! and Visibility. The colored chips are demo content — they visualize
//! where layout puts each child.

#![expect(
    clippy::cast_sign_loss,
    reason = "the showcase casts non-negative sizes, ids and colour channels"
)]

use crate::support;
use crate::support::{Column, api, columns, note, section, swatch_bg, well_bg};
use palantir::{
    Align, Block, Configure, HAlign, Justify, Panel, RgbaF32, Sizing, Spacing, Ui, VAlign,
    Visibility,
};

pub(crate) fn build(ui: &mut Ui) {
    columns(ui, |ui, column| match column {
        Column::Left => {
            sizing(ui);
            justify(ui);
            visibility(ui);
        }
        Column::Right => {
            alignment(ui);
            spacing(ui);
            gap(ui);
        }
    });
}

fn sizing(ui: &mut Ui) {
    section(
        ui,
        "Sizing",
        &[api!(Sizing::fixed), api!(Sizing::HUG), api!(Sizing::fill)],
        |ui| {
            note(
                ui,
                "Fixed takes exact pixels, Hug takes its content's size — here only padding \
                 of 20 and 40 — and Fill shares the leftover by weight, 1 : 2 : 1.",
            );
            support::row(ui, |ui| {
                for (i, w) in [50.0, 100.0, 200.0].into_iter().enumerate() {
                    support::swatch(
                        ui,
                        ("fx", i),
                        (Sizing::fixed(w), Sizing::fixed(32.0)),
                        support::B,
                    );
                }
            });
            support::row(ui, |ui| {
                // Padded frames hug their empty content box — effectively
                // just padding, so the two boxes differ only by pad width.
                for (i, pad) in [20.0, 40.0].into_iter().enumerate() {
                    Block::new()
                        .id_salt(i)
                        .size((Sizing::HUG, Sizing::fixed(32.0)))
                        .padding((pad, 0.0, pad, 0.0))
                        .background(swatch_bg(support::C))
                        .show(ui);
                }
            });
            support::row(ui, |ui| {
                for (i, weight) in [1.0, 2.0, 1.0].into_iter().enumerate() {
                    support::swatch(
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
    section(ui, "Justify", &[api!(Panel::justify)], |ui| {
        note(
            ui,
            "Where a stack puts its leftover room along its axis: Start, Center, End, \
                 SpaceBetween and SpaceAround, top to bottom.",
        );
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
                        support::swatch(
                            ui,
                            (id, i),
                            (Sizing::fixed(36.0), Sizing::fixed(22.0)),
                            support::A,
                        );
                    }
                });
        }
    });
}

fn visibility(ui: &mut Ui) {
    section(ui, "Visibility", &[api!(Block::visibility)], |ui| {
        note(
            ui,
            "The orange chip is Visible, then Hidden — it keeps its slot — then \
                 Collapsed, which gives the slot up.",
        );
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
                        Block::new()
                            .id_salt((id, key))
                            .size((Sizing::fixed(70.0), Sizing::fixed(28.0)))
                            .visibility(v)
                            .background(swatch_bg(c))
                            .show(ui);
                    }
                });
        }
    });
}

fn alignment(ui: &mut Ui) {
    section(
        ui,
        "Alignment",
        &[api!(Panel::child_align), api!(Block::align)],
        |ui| {
            note(
                ui,
                "child_align places a container's children across its axis: centred in the \
                 row, right in the column. The orange chip overrides it with its own align.",
            );
            // HStack: children inherit VAlign::Center; orange opts out to Bottom.
            Panel::hstack()
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
    section(ui, "Padding and margin", &[api!(type Spacing)], |ui| {
        note(
            ui,
            "Padding reserves room inside the parent, a margin shrinks the child's slot, \
                 and a negative margin overlaps the neighbour.",
        );
        Panel::hstack()
            .size((Sizing::FILL, Sizing::fixed(60.0)))
            .padding(20.0)
            .gap(8.0)
            .background(well_bg())
            .show(ui, |ui| {
                for i in 0..3 {
                    support::swatch(
                        ui,
                        ("p", i),
                        (Sizing::fixed(40.0), Sizing::FILL),
                        support::A,
                    );
                }
            });
        Panel::hstack()
            .size((Sizing::FILL, Sizing::fixed(60.0)))
            .gap(8.0)
            .background(well_bg())
            .show(ui, |ui| {
                Block::new()
                    .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                    .margin(8.0)
                    .background(swatch_bg(support::A))
                    .show(ui);
                Block::new()
                    .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                    .margin((16.0, 16.0, 0.0, 0.0))
                    .background(swatch_bg(support::A))
                    .show(ui);
            });
        // The orange box is anchored after the teal one, but its left
        // margin pulls it backwards 30 px so the two overlap.
        Panel::hstack()
            .size((Sizing::FILL, Sizing::fixed(60.0)))
            .padding(8.0)
            .background(well_bg())
            .show(ui, |ui| {
                support::swatch(
                    ui,
                    ("neg", "a"),
                    (Sizing::fixed(80.0), Sizing::fixed(40.0)),
                    support::A,
                );
                Block::new()
                    .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                    .margin((-30.0, 0.0, 0.0, 0.0))
                    .background(swatch_bg(support::B))
                    .show(ui);
            });
    });
}

fn gap(ui: &mut Ui) {
    section(ui, "Gap", &[api!(Panel::gap)], |ui| {
        note(ui, "0, 8 and 24 px between siblings.");
        for g in [0.0, 8.0, 24.0] {
            Panel::hstack()
                .id_salt(("gap", g as u32))
                .size((Sizing::FILL, Sizing::fixed(40.0)))
                .padding(6.0)
                .gap(g)
                .background(well_bg())
                .show(ui, |ui| {
                    for i in 0..5 {
                        support::swatch(
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

fn aligned_chip(ui: &mut Ui, id: &'static str, c: RgbaF32, align: Align) {
    Block::new()
        .id_salt(id)
        .size((Sizing::fixed(56.0), Sizing::fixed(24.0)))
        .align(align)
        .background(swatch_bg(c))
        .show(ui);
}
