# Open issues
- The showcase's images and motion pages (`examples/showcase/pages/images.rs` `Sources::load`, `examples/showcase/pages/motion.rs`) register their images once and never check `ImageHandle::is_resident`, so after a device loss they paint nothing.
