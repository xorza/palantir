//! Effect draw records.

#![expect(
    clippy::expl_impl_clone_on_copy,
    reason = "`soa_rs`'s `Soars` derive writes `Clone` by hand for the `Copy` rows it generates"
)]

use crate::common::span::Span;
use crate::effect::contract;
use crate::primitives::identity::effect_id::EffectId;
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::shape_header::ShapeHeader;
use crate::renderer::render_buffer::template_cover::TemplateCover;
use soa_rs::Soars;

/// One effect draw row. SoA storage lets the backend upload `rows.instance()` in one write;
/// `key` picks the pipeline, and `images` its images in the record store's image column; the
/// instance's flags tell a material's box, which the effect's one stage draws through palantir's
/// material path.
#[derive(Soars, Clone, Copy, Debug, PartialEq)]
#[soa_derive(Debug)]
pub(crate) struct EffectDrawRow {
    /// The pipeline the instance draws through, set once by the composer.
    pub(crate) key: EffectKey,
    pub(crate) images: Span,
    pub(crate) instance: EffectInstance,
}

/// What an effect draw's pipeline is built for: its effect, the form its shape takes and what of
/// its template it covers, each pinned as a constant, as a fill's
/// [`FillForm`](crate::renderer::render_buffer::fill_form::FillForm) pins its own. Refused: the
/// form as bits of the instance's flags, which put a runtime `% n` in the template's vertex decode
/// where the constant folds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct EffectKey {
    pub(crate) effect: EffectId,
    pub(crate) form: ShapeForm,
    pub(crate) cover: TemplateCover,
}

impl EffectKey {
    /// The pipelines an effect's table holds: one a form and a cover.
    pub(crate) const FORMS: usize = ShapeForm::ALL.len() * TemplateCover::ALL.len();

    /// The key's place in a table of every effect's forms: its effect's run of [`Self::FORMS`],
    /// at its form's pair of covers, at its cover.
    pub(crate) const fn slot(self) -> usize {
        self.effect.idx() * Self::FORMS
            + self.form as usize * TemplateCover::ALL.len()
            + self.cover as usize
    }

    /// The key at `slot` of a table of every effect's forms, [`Self::slot`]'s inverse.
    pub(crate) fn of_slot(slot: usize) -> Self {
        let covers = TemplateCover::ALL.len();
        Self {
            effect: EffectId(u32::try_from(slot / Self::FORMS).unwrap()),
            form: ShapeForm::ALL[slot % Self::FORMS / covers],
            cover: TemplateCover::ALL[slot % covers],
        }
    }
}

/// Per-shape GPU state an effect's stages read (`effect/wrapper.wgsl`'s `PalantirInstance`).
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct EffectInstance {
    /// The shape the effect covers, physical px, before the antialiasing reach, its turn and its
    /// rounded clips, which palantir's stage takes past the effect's.
    pub(crate) header: ShapeHeader,
    /// Seconds into the shape's paint animation's pass.
    pub(crate) time: f32,
    /// What the coverage is multiplied by: a paint animation's fade, 1 when still.
    pub(crate) opacity: f32,
    /// Physical px per logical px.
    pub(crate) scale: f32,
    /// The word the instance's parameters start at in the window's parameter buffer.
    pub(crate) params: u32,
    /// A backdrop effect's row in the frame's backdrops, the slot its result reads; 0 for a color
    /// effect, whose stages read none.
    pub(crate) backdrop_idx: u32,
    /// [`Self::MATERIAL`] for a material's box; 0 for an app's draw.
    pub(crate) flags: u32,
    /// A material's border width, physical px, which the template keeps out of its interior; 0
    /// for an app's draw.
    pub(crate) border_width: f32,
    /// A halo's reach, physical px, where its template's outer chain lies; a pipeline that covers
    /// its template whole reads none.
    pub(crate) reach: f32,
}

impl EffectInstance {
    /// `flags`'s bit for a material's box.
    pub(crate) const MATERIAL: u32 = contract::MATERIAL_FLAG;
}

#[cfg(test)]
mod tests {
    use crate::primitives::identity::effect_id::EffectId;
    use crate::renderer::render_buffer::effect_draw_row::EffectKey;
    use crate::renderer::render_buffer::shape_form::ShapeForm;
    use crate::renderer::render_buffer::template_cover::TemplateCover;

    /// A key's slot is its effect's run of six, two covers each of three forms, at its form's pair
    /// and its cover: effect 2's triangle rim at `2 · 6 + 2 · 2 + 1 = 17`, its whole rect at 12,
    /// effect 0's whole triangle at 4; and every slot reads back to its key.
    #[test]
    fn a_key_takes_its_effects_run_at_its_form_and_cover() {
        let key = |effect, form, cover| EffectKey {
            effect: EffectId(effect),
            form,
            cover,
        };
        assert_eq!(key(2, ShapeForm::Triangle, TemplateCover::Rim).slot(), 17);
        assert_eq!(key(2, ShapeForm::Rect, TemplateCover::Whole).slot(), 12);
        assert_eq!(key(0, ShapeForm::Triangle, TemplateCover::Whole).slot(), 4);
        for slot in 0..18 {
            assert_eq!(EffectKey::of_slot(slot).slot(), slot);
        }
        assert_eq!(
            EffectKey::of_slot(9),
            key(1, ShapeForm::Box, TemplateCover::Rim)
        );
    }
}
