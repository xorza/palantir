//! Layout mechanics: Sizing, child alignment with per-child override, Justify, padding/margin, gap, Visibility. The chips are demo content.

#![expect(
    clippy::cast_sign_loss,
    reason = "the showcase casts non-negative sizes, ids and color channels"
)]

use crate::support;
use crate::support::{Column, Section, api};
use palantir::{
    Align, Block, Configure, HAlign, Justify, Panel, RgbaF32, Sizing, Spacing, Ui, VAlign,
    Visibility,
};

pub(crate) fn build(ui: &mut Ui) {
    support::columns(ui, |ui, column| match column {
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
    Section::new("Sizing")
        .apis(&[api!(Sizing::fixed), api!(Sizing::HUG), api!(Sizing::fill)])
        .show(ui, |ui| {
            support::note(
                ui,
                "Fixed takes exact pixels, Hug takes its content's size — here only padding \
                 of 20 and 40 — and Fill shares the leftover by weight, 1 : 2 : 1.",
            );
            support::row(ui, |ui| {
                for (i, w) in [50.0, 100.0, 200.0].into_iter().enumerate() {
                    support::swatch(support::B)
                        .id_salt(("fx", i))
                        .size((Sizing::fixed(w), Sizing::fixed(32.0)))
                        .show(ui);
                }
            });
            support::row(ui, |ui| {
                for (i, pad) in [20.0, 40.0].into_iter().enumerate() {
                    Block::new()
                        .id_salt(i)
                        .size((Sizing::HUG, Sizing::fixed(32.0)))
                        .padding((pad, 0.0, pad, 0.0))
                        .background(support::swatch_bg(support::C))
                        .show(ui);
                }
            });
            support::row(ui, |ui| {
                for (i, weight) in [1.0, 2.0, 1.0].into_iter().enumerate() {
                    support::swatch(support::A)
                        .id_salt(("fill", i))
                        .size((Sizing::fill(weight), Sizing::fixed(32.0)))
                        .show(ui);
                }
            });
        });
}

fn justify(ui: &mut Ui) {
    Section::new("Justify")
        .apis(&[api!(Panel::justify)])
        .show(ui, |ui| {
            support::note(
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
                    .background(support::well_bg())
                    .show(ui, |ui| {
                        for i in 0..3 {
                            support::swatch(support::A)
                                .id_salt((id, i))
                                .size((Sizing::fixed(36.0), Sizing::fixed(22.0)))
                                .show(ui);
                        }
                    });
            }
        });
}

fn visibility(ui: &mut Ui) {
    Section::new("Visibility")
        .apis(&[api!(Block::visibility)])
        .show(ui, |ui| {
            support::note(
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
                    .background(support::well_bg())
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
                                .background(support::swatch_bg(c))
                                .show(ui);
                        }
                    });
            }
        });
}

fn alignment(ui: &mut Ui) {
    Section::new("Alignment")
        .apis(&[api!(Panel::child_align), api!(Block::align)])
        .show(ui, |ui| {
            support::note(
                ui,
                "child_align places a container's children across its axis: centred in the \
                 row, right in the column. The orange chip overrides it with its own align.",
            );
            Panel::hstack()
                .size((Sizing::FILL, Sizing::fixed(96.0)))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::v(VAlign::Center))
                .background(support::well_bg())
                .show(ui, |ui| {
                    aligned_chip(support::A, Align::default())
                        .id_salt("a")
                        .show(ui);
                    aligned_chip(support::A, Align::default())
                        .id_salt("b")
                        .show(ui);
                    aligned_chip(support::B, Align::v(VAlign::Bottom))
                        .id_salt("c-self-bot")
                        .show(ui);
                    aligned_chip(support::A, Align::default())
                        .id_salt("d")
                        .show(ui);
                });
            Panel::vstack()
                .size((Sizing::FILL, Sizing::fixed(110.0)))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::h(HAlign::Right))
                .background(support::well_bg())
                .show(ui, |ui| {
                    aligned_chip(support::A, Align::default())
                        .id_salt("a-vs")
                        .show(ui);
                    aligned_chip(support::B, Align::h(HAlign::Left))
                        .id_salt("b-self-left")
                        .show(ui);
                    aligned_chip(support::A, Align::default())
                        .id_salt("c-vs")
                        .show(ui);
                });
        });
}

fn spacing(ui: &mut Ui) {
    Section::new("Padding and margin")
        .apis(&[api!(type Spacing)])
        .show(ui, |ui| {
            support::note(
                ui,
                "Padding reserves room inside the parent, a margin shrinks the child's slot, \
                 and a negative margin overlaps the neighbour.",
            );
            Panel::hstack()
                .size((Sizing::FILL, Sizing::fixed(60.0)))
                .padding(20.0)
                .gap(8.0)
                .background(support::well_bg())
                .show(ui, |ui| {
                    for i in 0..3 {
                        support::swatch(support::A)
                            .id_salt(("p", i))
                            .size((Sizing::fixed(40.0), Sizing::FILL))
                            .show(ui);
                    }
                });
            Panel::hstack()
                .size((Sizing::FILL, Sizing::fixed(60.0)))
                .gap(8.0)
                .background(support::well_bg())
                .show(ui, |ui| {
                    Block::new()
                        .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                        .margin(8.0)
                        .background(support::swatch_bg(support::A))
                        .show(ui);
                    Block::new()
                        .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                        .margin((16.0, 16.0, 0.0, 0.0))
                        .background(support::swatch_bg(support::A))
                        .show(ui);
                });
            // The orange box's left margin pulls it back 30 px so the two overlap.
            Panel::hstack()
                .size((Sizing::FILL, Sizing::fixed(60.0)))
                .padding(8.0)
                .background(support::well_bg())
                .show(ui, |ui| {
                    support::swatch(support::A)
                        .id_salt(("neg", "a"))
                        .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                        .show(ui);
                    Block::new()
                        .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                        .margin((-30.0, 0.0, 0.0, 0.0))
                        .background(support::swatch_bg(support::B))
                        .show(ui);
                });
        });
}

fn gap(ui: &mut Ui) {
    Section::new("Gap")
        .apis(&[api!(Panel::gap)])
        .show(ui, |ui| {
            support::note(ui, "0, 8 and 24 px between siblings.");
            for g in [0.0, 8.0, 24.0] {
                Panel::hstack()
                    .id_salt(("gap", g as u32))
                    .size((Sizing::FILL, Sizing::fixed(40.0)))
                    .padding(6.0)
                    .gap(g)
                    .background(support::well_bg())
                    .show(ui, |ui| {
                        for i in 0..5 {
                            support::swatch(support::A)
                                .id_salt(("gap-tile", g as u32, i))
                                .size((Sizing::fixed(32.0), Sizing::fixed(24.0)))
                                .show(ui);
                        }
                    });
            }
        });
}

fn aligned_chip(c: RgbaF32, align: Align) -> Block {
    support::swatch(c)
        .size((Sizing::fixed(56.0), Sizing::fixed(24.0)))
        .align(align)
}
