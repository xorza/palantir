//! Colours reach the screen as the bytes they were written as.
//!
//! CPU hex-to-linear decode and the GPU's sRGB encode must be exact inverses, and every lane between must hold display precision. A cubic fit once read `#1a1a1a` back as `#171717`; only a readback showed it. The ramp sits in the dark range, where one display step is smallest in linear light.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and colour channels"
)]

use glam::{UVec2, Vec2};
use palantir::widget::{IconFit, Mesh, Shape};
use palantir::{Background, Block, Configure, FontFamily, Panel, RgbaF32, Sizing, Text, Ui};

use crate::fixtures::icon;
use crate::harness::Harness;
use palantir::Stroke;

const RAMP: [u8; 7] = [5, 8, 10, 16, 26, 32, 48];
const COLUMN: f32 = 20.0;
const BLOCK_Y: f32 = 0.0;
const TEXT_Y: f32 = 12.0;
const LINE_Y: f32 = 44.0;
const ICON_Y: f32 = 52.0;
const MESH_Y: f32 = 64.0;
const SURFACE: UVec2 = UVec2::new(COLUMN as u32 * RAMP.len() as u32, 76);

fn grey(v: u8) -> RgbaF32 {
    RgbaF32::hex(u32::from(v) * 0x01_01_01)
}

fn ramp(ui: &mut Ui) {
    let icons = ui.load_icons(icon::atlas());
    let solid = icons.by_name("solid").expect("fixture icon");
    Panel::canvas()
        .id_salt("ramp")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for (i, v) in RAMP.into_iter().enumerate() {
                let x = i as f32 * COLUMN;
                Block::new()
                    .id_salt(("block", v))
                    .position(Vec2::new(x, BLOCK_Y))
                    .size((Sizing::fixed(COLUMN), Sizing::fixed(10.0)))
                    .background(Background {
                        fill: grey(v).into(),
                        ..Default::default()
                    })
                    .show(ui);
                Text::new("\u{2588}")
                    .id_salt(("text", v))
                    .position(Vec2::new(x + 4.0, TEXT_Y))
                    .family(FontFamily::MONO)
                    .font_size(16.0)
                    .color(grey(v))
                    .show(ui);
                ui.add_shape(Shape::line(
                    Vec2::new(x + 2.0, LINE_Y),
                    Vec2::new(x + COLUMN - 2.0, LINE_Y),
                    Stroke::new(grey(v), 6.0),
                ));
                let (left, right) = (x + 2.0, x + COLUMN - 2.0);
                let quad = [
                    Vec2::new(left, MESH_Y),
                    Vec2::new(right, MESH_Y),
                    Vec2::new(right, MESH_Y + 10.0),
                    Vec2::new(left, MESH_Y + 10.0),
                ];
                ui.add_shape(Shape::mesh(&Mesh::filled_polygon(&quad, grey(v))));
                Panel::zstack()
                    .id_salt(("icon", v))
                    .position(Vec2::new(x + 5.0, ICON_Y))
                    .size((Sizing::fixed(10.0), Sizing::fixed(10.0)))
                    .show(ui, |ui| {
                        ui.add_shape(icons.shape(solid).fit(IconFit::Fill).tint(grey(v)));
                    });
            }
        });
}

#[test]
fn a_dark_ramp_reads_back_as_authored() {
    let img = Harness::new()
        .size(SURFACE)
        .clear(RgbaF32::BLACK)
        .frame(ramp)
        .image;
    for (i, v) in RAMP.into_iter().enumerate() {
        let x = i as u32 * COLUMN as u32;
        let want = [v, v, v, 255];
        let at = |dx: u32, y: f32| img.get_pixel(x + dx, y as u32 + 5).0;
        assert_eq!(
            at(10, BLOCK_Y),
            want,
            "a block filled with #{v:02x}{v:02x}{v:02x}"
        );
        assert_eq!(
            at(10, LINE_Y - 5.0),
            want,
            "a line stroked with #{v:02x}{v:02x}{v:02x}"
        );
        assert_eq!(
            at(10, ICON_Y),
            want,
            "an icon tinted #{v:02x}{v:02x}{v:02x}"
        );
        assert_eq!(
            at(10, MESH_Y),
            want,
            "a mesh coloured #{v:02x}{v:02x}{v:02x}"
        );
        let brightest = (x..x + COLUMN as u32)
            .flat_map(|px| (TEXT_Y as u32..LINE_Y as u32 - 4).map(move |py| (px, py)))
            .map(|(px, py)| img.get_pixel(px, py).0)
            .max_by_key(|p| p[0])
            .unwrap();
        assert_eq!(brightest, want, "a glyph coloured #{v:02x}{v:02x}{v:02x}");
    }
}
