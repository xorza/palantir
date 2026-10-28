# Open issues

- An icon drawn resampled takes its quad size as `U16Vec2::new(size.x as u16, size.y as u16)`
  (`composer/session.rs`, `ComposeSession::icon`): a box past 65535 physical px, as a zoom makes,
  saturates to 65535, so the quad is smaller than the box its cull and damage rects use, and a box
  whose start lies far off screen draws nowhere on screen.
