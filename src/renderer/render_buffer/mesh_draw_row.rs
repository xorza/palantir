//! Mesh payload spans and per-draw GPU instance data.

use crate::common::span::Span;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::clip_chain::ClipChain;
use glam::Vec2;
use palantir_derive::Soa;

/// One mesh draw within a group; vertex/index slices live in the recording's [`RecordStore::mesh_vertices`](crate::shape::record_store::RecordStore::mesh_vertices) and `mesh_idxs`, transform and tint in [`MeshDrawRow::instance`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MeshDraw {
    pub(crate) vertices: Span,
    pub(crate) idxs: Span,
}

/// One mesh draw row; the SoA split lets the backend upload `rows.instance()` in one `write_buffer`.
#[derive(Soa, Clone, Copy, Debug, PartialEq)]
#[soa(MeshDrawRows)]
pub(crate) struct MeshDrawRow {
    pub(crate) draw: MeshDraw,
    pub(crate) instance: MeshInstance,
}

/// Per-mesh GPU state in a `step_mode: Instance` buffer:
/// `physical = x_axis · pos.x + y_axis · pos.y + translate`, a linear map that fits, turns and
/// scales at once, and `out_color = vertex.color * tint`.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MeshInstance {
    pub(crate) translate: Vec2,
    /// Where a vertex's unit x lands: `scale · fit.x · (cos, sin)` of the turn.
    pub(crate) x_axis: Vec2,
    /// Where a vertex's unit y lands: `scale · fit.y · (−sin, cos)` of the turn.
    pub(crate) y_axis: Vec2,
    pub(crate) tint: RgbaF16,
    /// The rounded clips that cut the mesh.
    pub(crate) clip: ClipChain,
}

#[cfg(test)]
mod tests {
    use crate::common::span::Span;
    use crate::renderer::render_buffer::mesh_draw_row::{
        MeshDraw, MeshDrawRow, MeshDrawRows, MeshInstance,
    };
    use bytemuck::Zeroable as _;
    use glam::Vec2;

    /// A row told apart by `at`: its spans start there and its instance moves there.
    fn row(at: u32) -> MeshDrawRow {
        MeshDrawRow {
            draw: MeshDraw {
                vertices: Span::new(at, 3),
                idxs: Span::new(10 * at, 6),
            },
            instance: MeshInstance {
                translate: Vec2::splat(at as f32),
                ..MeshInstance::zeroed()
            },
        }
    }

    /// The columns hold each row a field a column, in order, through every way in and out: built
    /// from an array, pushed, inserted, extended and truncated, read back by iterator in both
    /// directions and a column at a time, written through every column at once, and cloned.
    #[test]
    fn the_columns_hold_each_row_a_field_a_column() {
        let mut rows = MeshDrawRows::from([row(0), row(1)]);
        rows.push(row(3));
        rows.insert(2, row(2));
        rows.extend([row(4), row(5)]);
        assert_eq!(rows.len(), 6);
        assert_eq!(rows.iter().len(), 6);
        assert!(rows.iter().eq((0..6).map(row)));
        assert!(rows.iter().rev().eq((0..6).rev().map(row)));
        let starts: Vec<u32> = rows.draw().iter().map(|draw| draw.vertices.start).collect();
        assert_eq!(starts, [0, 1, 2, 3, 4, 5]);
        assert_eq!(rows.instance()[5].translate, Vec2::splat(5.0));

        rows.truncate(9);
        assert_eq!(rows.len(), 6, "a longer truncate keeps every row");
        rows.truncate(3);
        let columns = rows.columns_mut();
        columns.draw.swap(0, 2);
        columns.instance.swap(0, 2);
        columns.draw[1] = row(7).draw;
        let written = [
            row(2),
            MeshDrawRow {
                draw: row(7).draw,
                ..row(1)
            },
            row(0),
        ];
        assert!(rows.iter().eq(written));
        let clone = rows.clone();
        assert!(clone.iter().eq(written));
        assert_eq!((&rows).into_iter().collect::<MeshDrawRows>().len(), 3);
        assert_eq!(
            format!("{:?}", MeshDrawRows::from([row(1)])),
            format!(
                "MeshDrawRows {{ draw: [{:?}], instance: [{:?}] }}",
                row(1).draw,
                row(1).instance
            ),
        );

        rows.clear();
        assert!(rows.is_empty());
        assert_eq!(rows.iter().next(), None);
        assert_eq!(rows.instance().len(), 0);
        assert!(clone.iter().eq(written), "a clone keeps its own buffers");
    }

    /// Every row survives each growth of the buffers, which a push, an insert at full capacity,
    /// an extend whose iterator hides its length, and a push onto a clone each cause: 1000 rows
    /// grow them from 4 through 8, 16 … 1024, and every row reads back where it went.
    #[test]
    fn every_row_survives_the_buffers_growing() {
        let mut rows = MeshDrawRows::default();
        for at in 0..500 {
            rows.push(row(at));
        }
        rows.insert(0, row(1000));
        rows.insert(250, row(1001));
        rows.insert(rows.len(), row(1002));
        let mut want: Vec<MeshDrawRow> = (0..500).map(row).collect();
        want.insert(0, row(1000));
        want.insert(250, row(1001));
        want.push(row(1002));
        assert!(rows.iter().eq(want.iter().copied()));

        let mut clone = rows.clone();
        rows.extend((2000..2497).filter(|_| true).map(row));
        want.extend((2000..2497).map(row));
        assert_eq!(rows.len(), 1000);
        assert!(rows.iter().eq(want.iter().copied()));
        assert!(
            rows.instance()
                .iter()
                .map(|i| i.translate)
                .eq(want.iter().map(|w| w.instance.translate))
        );

        clone.push(row(3000));
        assert_eq!(clone.len(), 504);
        assert_eq!(clone.iter().next_back(), Some(row(3000)));
        assert!(clone.iter().take(503).eq(want.iter().take(503).copied()));
    }

    /// An insert past the last row is refused.
    #[test]
    #[should_panic(expected = "insert at 2 past 1 rows")]
    fn an_insert_past_the_last_row_is_refused() {
        MeshDrawRows::from([row(0)]).insert(2, row(1));
    }
}
