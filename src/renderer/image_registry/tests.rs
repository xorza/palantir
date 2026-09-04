use crate::primitives::color::srgba_u8::SrgbaU8;
use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::texture_id_source::TextureIdSource;
use glam::UVec2;

fn reg() -> ImageRegistry {
    ImageRegistry::new(TextureIdSource::default())
}

fn img(w: u32, h: u32) -> Image {
    Image::blank(UVec2::new(w, h))
}

#[test]
fn register_queues_one_upload_and_unique_ids() {
    let reg = reg();
    let a = reg.register(img(2, 3));
    let b = reg.register(img(4, 5));
    // Distinct registrations get distinct ids, both nonzero.
    assert_ne!(a.id(), b.id());
    assert_ne!(a.id().0, 0);
    assert_eq!(a.size(), UVec2::new(2, 3));
    // Both uploads are pending; draining hands the bytes over once.
    let mut uploaded = 0;
    reg.drain_pending(|_, _| uploaded += 1);
    assert_eq!(uploaded, 2);
    reg.drain_pending(|_, _| uploaded += 1);
    assert_eq!(uploaded, 2, "drain consumes pending");
}

#[test]
fn dimensions_above_u16_are_preserved() {
    const WIDTH: u32 = u16::MAX as u32 + 1;
    let handle = reg().register(img(WIDTH, 1));
    assert_eq!(handle.size(), UVec2::new(WIDTH, 1));
}

/// A 0×0 image is a logic error caught at construction — before it
/// can reach `register` and blow up a frame later in the GPU upload.
#[test]
#[should_panic(expected = "RGBA8 dimensions must be non-zero")]
fn zero_sized_image_panics_at_construction() {
    let _ = img(0, 0);
}

#[test]
fn dropping_last_handle_queues_release() {
    let reg = reg();
    let h = reg.register(img(1, 1));
    let id = h.id();
    reg.drain_pending(|_, _| {});
    // A live clone keeps it alive: no release queued yet.
    let clone = h.clone();
    drop(h);
    let mut freed = Vec::new();
    reg.drain_dropped(|id| freed.push(id));
    assert!(freed.is_empty(), "clone still holds it");
    // Last clone gone → id queued for GPU release exactly once.
    drop(clone);
    reg.drain_dropped(|id| freed.push(id));
    assert_eq!(freed, vec![id]);
    reg.drain_dropped(|id| freed.push(id));
    assert_eq!(freed, vec![id], "drain consumes dropped");
}

/// The first write hands out zeros of the image's size, and every later one
/// the texels the previous write left.
#[test]
fn a_write_sees_the_previous_write() {
    let h = reg().register(img(2, 2));
    {
        let texels = h.write();
        assert_eq!(texels.len(), 4);
        assert!(
            texels.iter().all(|t| *t == SrgbaU8::default()),
            "zeroed before the first write"
        );
    }
    h.write().fill(SrgbaU8::hex(0x4cd3ff));
    assert!(h.write().iter().all(|t| *t == SrgbaU8::hex(0x4cd3ff)));
}

/// The buffer is sized once: a second write neither grows nor moves it.
#[test]
fn the_write_buffer_is_allocated_once() {
    let h = reg().register(img(3, 2));
    let first = h.write().as_ptr();
    let second = h.write().as_ptr();
    assert_eq!(first, second);
    assert_eq!(h.write().len(), 6);
}

/// One write is one upload carrying the bytes the guard left. Two writes
/// between drains are still one, carrying the later ones, and a write after
/// the drain queues again.
#[test]
fn writes_between_drains_collapse_to_one_upload() {
    let reg = reg();
    let h = reg.register(img(1, 1));
    reg.drain_pending(|_, _| {});
    h.write()[0] = SrgbaU8::rgb(1, 2, 3);
    h.write()[0] = SrgbaU8::rgb(5, 6, 7);
    let mut uploads = Vec::new();
    reg.drain_refresh(|id, texels| uploads.push((id, texels.to_vec())));
    assert_eq!(uploads, vec![(h.id(), vec![5, 6, 7, 255])]);
    reg.drain_refresh(|_, _| panic!("drain consumes the queue"));

    h.write()[0] = SrgbaU8::rgb(9, 9, 9);
    let mut again = Vec::new();
    reg.drain_refresh(|_, texels| again.push(texels[0]));
    assert_eq!(again, vec![9]);
}

/// Every write moves the generation the shape hash reads, so a rewritten
/// texture repaints under its unchanged id.
#[test]
fn every_write_bumps_the_generation() {
    let h = reg().register(img(1, 1));
    let start = h.generation();
    h.write();
    h.write();
    assert_eq!(h.generation(), start.wrapping_add(2));
}

/// A handle dropped after it wrote is skipped at drain: its texture is on
/// its way out, and the queue entry must not keep the token alive.
#[test]
fn a_handle_dropped_after_writing_is_skipped() {
    let reg = reg();
    let h = reg.register(img(1, 1));
    let id = h.id();
    reg.drain_pending(|_, _| {});
    h.write();
    drop(h);
    reg.drain_refresh(|_, _| panic!("a dropped handle has nothing to upload"));
    let mut freed = Vec::new();
    reg.drain_dropped(|id| freed.push(id));
    assert_eq!(freed, vec![id]);
}

#[test]
fn ids_are_minted_in_registration_order() {
    let reg = reg();
    assert_eq!(reg.register(img(1, 1)).id(), TextureId(1));
    assert_eq!(reg.register(img(1, 1)).id(), TextureId(2));
}
