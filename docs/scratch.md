checkbox

ShapeRecord color to coloru8

framearena to encpsulate handle

image eviction from cache

      let gradients = ui.caches.gradients.clone();
                ui.text.selection_rects(
                    text_ptr,
                    range,
                    ctx.font_size,
                    ctx.line_height_px,
                    ctx.wrap_target,
                    ctx.family,
                    ctx.halign,
                    |x, y, w, h| {
                        forest.add_shape(
                            Shape::RoundedRect {
                                local_rect: Some(Rect::new(
                                    ctx.padding.left() + offset.x + x - scroll.x,
                                    ctx.padding.top() + offset.y + y - scroll.y,
                                    w,
                                    h,
                                )),
                                radius: Default::default(),
                                fill: sel_color.into(),
                                stroke: Stroke::ZERO,
                            },
                            &mut arena_handle.borrow_mut(),
                            &gradients,
                        );
                    },
                );
            }
