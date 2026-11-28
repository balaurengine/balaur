//! The `view` kind: the picture a `camera2d` naming the widget as its `view`
//! renders, drawn across the widget's box. The renderer renders it into a
//! texture of the box's size before the frame is presented and hands the
//! layer its id ([`crate::set_view_texture`]); the layer draws it where the
//! layout put the widget, under whatever is laid over it.

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use balaur_script::Value;
use rustc_hash::FxHashMap;

use crate::vocabulary::keys as k;
use crate::vocabulary::words::view_pointer as vp;

/// Each `view` widget's texture, by its node.
#[derive(Default)]
pub(crate) struct ViewTextures(pub(crate) FxHashMap<Entity, egui::TextureId>);

/// What the pointer did on a `view`: a click, which says `view_clicked`, or
/// the pointer over it, its wheel and a pinch, which say `view_pointer`
/// with what it carries.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Pointed {
    Click([f32; 2]),
    Over(Value),
}

/// A `view` across the box the layout gave it, sensing a click where one is
/// heard: `pointer_click`, and `view_clicked` where it landed; and where its
/// `view_pointer` is heard, the pointer over it, its wheel, which it takes
/// from the page it scrolls in, and a pinch.
pub(crate) fn view(ui: &mut egui::Ui, at: &mut crate::widget::layer::Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let (entity, widget) = (placed.entity, &placed.widget);
    let size = crate::widget::arrange::solved_of(widget, &at.style(index), at.assigned);
    let sense = if widget.hears_click {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    paint(at.eng, ui, entity, rect);
    crate::widget::arrange::record_view(entity, ui.layer_id());
    // The pointer reaches the view where it is drawn, moved as what holds
    // it was last frame, as `view_point` finds its points.
    let reach = crate::widget::arrange::view_reach(entity, rect);
    let id = crate::widget::layer::sensor_id(ui, "balaur-view", entity);
    let response = ui.interact(reach, id, widget.sense(sense));
    let on = |point: Option<egui::Pos2>| {
        point
            .and_then(|point| crate::widget::arrange::view_point_in(entity, point))
            .is_some_and(|point| rect.contains(point))
    };
    let mut said = Vec::new();
    if let Some(point) = response
        .interact_pointer_pos()
        .filter(|point| response.clicked() && on(Some(*point)))
    {
        at.clicked.push(entity);
        said.push(Pointed::Click(point.into()));
    }
    let over = response.contains_pointer() && on(ui.input(|i| i.pointer.hover_pos()));
    if widget.hears_pointer && !widget.pointer_through && over {
        said.extend(pointed(ui).into_iter().map(Pointed::Over));
    }
    at.edits.extend(
        said.into_iter()
            .map(|said| (entity, crate::widget::layer::Edit::View(said))),
    );
}

/// What the pointer over a view did this pass: moved, turned the wheel,
/// pinched. The wheel is taken from whatever scrolls around the view, which
/// reads what is left once the view has drawn.
fn pointed(ui: &mut egui::Ui) -> Vec<Value> {
    ui.input_mut(|i| {
        let at = i.pointer.hover_pos().unwrap_or_default();
        let held = i.modifiers;
        let mut said = Vec::new();
        if i.events
            .iter()
            .any(|event| matches!(event, egui::Event::PointerMoved(_)))
        {
            said.push(pointer(vp::MOVE, at, held, []));
        }
        for event in &i.events {
            match event {
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } => {
                    let unit = match unit {
                        egui::MouseWheelUnit::Point => vp::PIXEL,
                        egui::MouseWheelUnit::Line => vp::LINE,
                        egui::MouseWheelUnit::Page => vp::PAGE,
                    };
                    // The keys held as the wheel turned, which the turn carries.
                    let turned = [
                        (vp::TURN, Value::Vec2([delta.x, delta.y])),
                        (vp::UNIT, Value::Str(unit.to_owned())),
                    ];
                    said.push(pointer(vp::WHEEL, at, *modifiers, turned));
                }
                egui::Event::Zoom(scale) => {
                    said.push(pointer(
                        vp::PINCH,
                        at,
                        held,
                        [(k::SCALE, Value::Num(f64::from(*scale)))],
                    ));
                }
                _ => {}
            }
        }
        if let Some(touch) = i
            .multi_touch()
            .filter(|touch| (touch.zoom_delta - 1.0).abs() > f32::EPSILON)
        {
            let scale = [(k::SCALE, Value::Num(f64::from(touch.zoom_delta)))];
            said.push(pointer(vp::PINCH, touch.center_pos, held, scale));
        }
        // The wheel over the view is the view's: the page around it scrolls
        // by none of it while the pointer stays over the view, nor by what
        // egui spreads a turn over the frames after.
        i.smooth_scroll_delta = egui::Vec2::ZERO;
        said
    })
}

/// What `view_pointer` carries: what the pointer did, where, the keys held,
/// and what else the doing says.
fn pointer<const N: usize>(
    what: &str,
    at: egui::Pos2,
    held: egui::Modifiers,
    rest: [(&str, Value); N],
) -> Value {
    use crate::vocabulary::words as w;
    let mut said = vec![
        (vp::WHAT.into(), Value::Str(what.to_owned())),
        (w::AT.into(), Value::Vec2([at.x, at.y])),
        (w::SHIFT.into(), Value::Bool(held.shift)),
        (w::CTRL.into(), Value::Bool(held.ctrl)),
        (w::ALT.into(), Value::Bool(held.alt)),
        (w::CMD.into(), Value::Bool(held.command)),
    ];
    said.extend(rest.map(|(key, value)| (key.into(), value)));
    Value::Map(said)
}

fn paint(eng: &Engine, ui: &egui::Ui, entity: Entity, rect: egui::Rect) {
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
