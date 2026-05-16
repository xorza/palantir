use palantir::{Color, Configure, Mesh, Panel, Shape, Sizing, Ui};
use super::app_state::AppState;

pub fn build(ui: &mut Ui<AppState>) {
    Panel::hstack()
        .auto_id()
        .gap(24.0)
        .padding(24.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            cell(ui, "triangle", triangle);
            cell(ui, "polygon", polygon_star);
            cell(ui, "gradient quad", gradient_quad);
            cell(ui, "stress 5k", stress);
        });
}

fn cell<T>(ui: &mut Ui<T>, id: &'static str, paint: impl Fn(&mut Ui<T>)) {
    Panel::zstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(8.0)
        .show(ui, paint);
}

fn triangle<T>(ui: &mut Ui<T>) {
    let mut m = Mesh::new();
    let red = Color::rgb(1.0, 0.2, 0.2);
    let a = m.vertex(glam::Vec2::new(60.0, 10.0), red);
    let b = m.vertex(glam::Vec2::new(110.0, 100.0), red);
    let c = m.vertex(glam::Vec2::new(10.0, 100.0), red);
    m.triangle(a, b, c);
    ui.add_shape(Shape::Mesh {
        mesh: &m,
        local_rect: None,
        tint: Color::WHITE.into(),
    });
}

fn polygon_star<T>(ui: &mut Ui<T>) {
    // 5-pointed star sampled as a fan around the centroid.
    let cx = 60.0_f32;
    let cy = 60.0_f32;
    let r_outer = 55.0_f32;
    let r_inner = 22.0_f32;
    let mut pts = Vec::with_capacity(10);
    for i in 0..10 {
        let theta = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        let r = if i % 2 == 0 { r_outer } else { r_inner };
        pts.push(glam::Vec2::new(cx + r * theta.cos(), cy + r * theta.sin()));
    }
    // Fan triangulation around centroid — star is concave; fan-around-
    // first-point would clip; fan-around-centroid is correct here.
    let mut m = Mesh::new();
    let yellow = Color::rgb(1.0, 0.85, 0.2);
    let centroid = m.vertex(glam::Vec2::new(cx, cy), yellow);
    let first = m.vertex(pts[0], yellow);
    let mut prev = first;
    for p in &pts[1..] {
        let next = m.vertex(*p, yellow);
        m.triangle(centroid, prev, next);
        prev = next;
    }
    m.triangle(centroid, prev, first);
    ui.add_shape(Shape::Mesh {
        mesh: &m,
        local_rect: None,
        tint: Color::WHITE.into(),
    });
}

fn gradient_quad<T>(ui: &mut Ui<T>) {
    // Per-vertex colors create a 4-corner gradient.
    let mut m = Mesh::new();
    let tl = m.vertex(glam::Vec2::new(10.0, 10.0), Color::rgb(1.0, 0.0, 0.0));
    let tr = m.vertex(glam::Vec2::new(110.0, 10.0), Color::rgb(0.0, 1.0, 0.0));
    let br = m.vertex(glam::Vec2::new(110.0, 110.0), Color::rgb(0.0, 0.0, 1.0));
    let bl = m.vertex(glam::Vec2::new(10.0, 110.0), Color::rgb(1.0, 1.0, 0.0));
    m.triangle(tl, tr, br);
    m.triangle(tl, br, bl);
    ui.add_shape(Shape::Mesh {
        mesh: &m,
        local_rect: None,
        // White tint — pass-through, exercises the tint multiply path.
        tint: Color::rgb(1.0, 1.0, 1.0).into(),
    });
}

fn stress<T>(ui: &mut Ui<T>) {
    // 5000-vertex grid of triangles. Exercises the alloc-free claim
    // and the index buffer growth path. The grid renders as a teal
    // wash since every vertex shares the same color.
    const SIDE: u16 = 50; // 50x50 verts = 2500; pair-triangles = ~5000 verts after duplication.
    let teal = Color::rgb(0.2, 0.7, 0.7);
    let mut m = Mesh::with_capacity((SIDE as usize).pow(2), (SIDE as usize - 1).pow(2) * 6);
    let step = 2.0_f32;
    for j in 0..SIDE {
        for i in 0..SIDE {
            m.vertex(glam::Vec2::new(i as f32 * step, j as f32 * step), teal);
        }
    }
    for j in 0..SIDE - 1 {
        for i in 0..SIDE - 1 {
            let a = j * SIDE + i;
            let b = a + 1;
            let c = a + SIDE;
            let d = c + 1;
            m.triangle(a, b, d);
            m.triangle(a, d, c);
        }
    }
    ui.add_shape(Shape::Mesh {
        mesh: &m,
        local_rect: None,
        tint: Color::WHITE.into(),
    });
}
