//! Drawing the touch controls `balaur_input` hit-tests.
//!
//! The split is the plan's: a control is a component so the tick can read it
//! headless, and only its picture needs a window. Nothing here decides
//! anything -- the placement and the pressed state were settled at the top of
//! the tick, and this reads them back.
//!
//! Drawn under the widget tree, so a pause menu covers the stick rather than
//! the stick floating over the menu.

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use balaur_input::{TouchButton, TouchStick};
use egui::{Color32, Order, pos2};

fn color(channels: [f32; 4]) -> Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(
        byte(channels[0]),
        byte(channels[1]),
        byte(channels[2]),
        byte(channels[3]),
    )
}

/// Paint every control in the scene. Called by the windowed backend after the
/// widget pass, on the layer below it.
pub(crate) fn draw(eng: &Engine, ctx: &egui::Context) {
    let world = eng.world();
    let mut buttons = world.query::<(Entity, &TouchButton)>();
    let mut sticks = world.query::<(Entity, &TouchStick)>();
    let mut painter = None;
    for (_, button) in &mut buttons {
        let Some((center, half)) = button.placement(eng) else {
            continue;
        };
        let painter = painter.get_or_insert_with(|| layer(ctx));
        let fill = color(if button.pressed {
            button.pressed_color
        } else {
            button.color
        });
        let at = pos2(center.0, center.1);
        if button.shape == balaur_input::touch_controls::Shape::Circle {
            painter.circle_filled(at, half.0.max(half.1), fill);
        } else {
            painter.rect_filled(
                egui::Rect::from_center_size(at, egui::vec2(half.0 * 2.0, half.1 * 2.0)),
                8.0,
                fill,
            );
        }
    }
    for (_, stick) in &mut sticks {
        let Some((base, knob)) = stick.placement(eng) else {
            continue;
        };
        let painter = painter.get_or_insert_with(|| layer(ctx));
        painter.circle_filled(pos2(base.0.0, base.0.1), base.1, color(stick.color));
        painter.circle_filled(pos2(knob.0.0, knob.0.1), knob.1, color(stick.knob_color));
    }
}

/// One layer for every control, made only when there is something to put on
/// it: an empty scene should not cost a layer per frame.
fn layer(ctx: &egui::Context) -> egui::Painter {
    ctx.layer_painter(egui::LayerId::new(
        Order::Background,
        egui::Id::new("balaur-touch-controls"),
    ))
}
