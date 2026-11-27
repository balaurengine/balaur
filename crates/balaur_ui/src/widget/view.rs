//! The `view` kind: the picture a `camera2d` naming the widget as its `view`
//! renders, drawn across the widget's box. The renderer renders it into a
//! texture of the box's size before the frame is presented and hands the
//! layer its id ([`crate::set_view_texture`]); the layer draws it where the
//! layout put the widget, under whatever is laid over it.

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use rustc_hash::FxHashMap;

/// Each `view` widget's texture, by its node.
#[derive(Default)]
pub(crate) struct ViewTextures(pub(crate) FxHashMap<Entity, egui::TextureId>);

pub(crate) fn paint(eng: &Engine, ui: &egui::Ui, entity: Entity, rect: egui::Rect) {
    let Some(texture) = eng
        .try_resource::<ViewTextures>()
        .and_then(|views| views.borrow().0.get(&entity).copied())
    else {
        return;
    };
    let whole = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    crate::widget::paint::Brush::of(ui).add(crate::widget::paint::picture(
        texture,
        rect,
        whole,
        egui::Color32::WHITE,
    ));
}
