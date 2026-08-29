//! Scene-tree UI elements: the `widget` component.
//!
//! A node carrying a `widget` component is a piece of game UI (label,
//! button, or panel) anchored to the screen. The component is registered
//! through the standard component registry, so widgets are addable and
//! editable in the editor and show up in the scene tree like any node.
//! Buttons record clicks into the component (`clicked` in `get_component`,
//! reset each frame).

use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine};
use egui::{pos2, vec2, Align2, Color32, Stroke};

use crate::theme::{family, parse_hex};

#[derive(Clone)]
pub struct Widget {
    pub kind: String,
    pub text: String,
    pub anchor: String,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub color: String,
    /// Method on this node's script, called when the widget is clicked.
    /// Empty means nothing is connected. A name rather than a function value:
    /// scene files cannot hold closures, and a name works on any backend.
    pub on_click: String,
    pub clicked: bool,
}

/// Where and whether the widget layer draws. Games leave the default (full
/// window); editors point it at their viewport and enable it during play.
pub struct WidgetLayer {
    pub enabled: bool,
    /// Design-px rect (x, y, w, h); None = whole screen.
    pub rect: Option<[f32; 4]>,
}

impl Default for WidgetLayer {
    fn default() -> Self {
        Self {
            enabled: true,
            rect: None,
        }
    }
}

pub(crate) fn register(app: &mut App) {
    app.register_component(
        "widget",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                r##"kind = { kind = "enum", default = "label", options = ["label", "button", "panel"] }
text = { kind = "str", default = "label" }
anchor = { kind = "enum", default = "top_left", options = ["top_left", "top_right", "bottom_left", "bottom_right", "center"] }
x = { kind = "float", default = 16.0 }
y = { kind = "float", default = 16.0 }
size = { kind = "float", default = 16.0, min = 6.0 }
color = { kind = "str", default = "#eef1f4" }
on_click = { kind = "str", default = "" }"##,
            ),
            apply: Box::new(|eng, entity, params| {
                let s = |key: &str, default: &str| {
                    params
                        .get(key)
                        .and_then(|v| v.as_str())
                        .unwrap_or(default)
                        .to_string()
                };
                let f = |key: &str, default: f64| {
                    params.get(key).and_then(balaur_core::components::as_f64).unwrap_or(default) as f32
                };
                let widget = Widget {
                    kind: s("kind", "label"),
                    text: s("text", "label"),
                    anchor: s("anchor", "top_left"),
                    x: f("x", 16.0),
                    y: f("y", 16.0),
                    size: f("size", 16.0),
                    color: s("color", "#eef1f4"),
                    on_click: s("on_click", ""),
                    clicked: false,
                };
                eng.world_mut()
                    .insert_one(entity, widget)
                    .map_err(|_| anyhow::anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Widget>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let widget = world.get::<&Widget>(entity).ok()?;
                let mut map = toml::map::Map::new();
                map.insert("kind".into(), toml::Value::String(widget.kind.clone()));
                map.insert("text".into(), toml::Value::String(widget.text.clone()));
                map.insert("anchor".into(), toml::Value::String(widget.anchor.clone()));
                map.insert("x".into(), toml::Value::Float(f64::from(widget.x)));
                map.insert("y".into(), toml::Value::Float(f64::from(widget.y)));
                map.insert("size".into(), toml::Value::Float(f64::from(widget.size)));
                map.insert("color".into(), toml::Value::String(widget.color.clone()));
                map.insert("clicked".into(), toml::Value::Boolean(widget.clicked));
                map.insert(
                    "on_click".into(),
                    toml::Value::String(widget.on_click.clone()),
                );
                Some(toml::Value::Table(map))
            }),
        },
    );
}

/// Draw every widget entity. Runs inside the frame's egui pass, after the
/// scripts' `draw_ui`.
pub(crate) fn draw(engine: &Engine, ctx: &egui::Context, scale: f32) {
    let Some(layer) = engine.try_resource::<WidgetLayer>() else {
        return;
    };
    let (enabled, rect) = {
        let layer = layer.borrow();
        (layer.enabled, layer.rect)
    };
    if !enabled {
        return;
    }
    let screen = ctx.viewport_rect();
    let area = match rect {
        Some([x, y, w, h]) => {
            egui::Rect::from_min_size(pos2(x * scale, y * scale), vec2(w * scale, h * scale))
        }
        None => screen,
    };
    let widgets: Vec<(Entity, Widget)> = {
        let world = engine.world();
        let mut query = world.query::<(Entity, &Widget)>();
        let collected: Vec<(Entity, Widget)> = query.iter().map(|(e, w)| (e, w.clone())).collect();
        collected
    };
    let mut clicked: Vec<Entity> = Vec::new();
    for (entity, widget) in &widgets {
        let ox = widget.x * scale;
        let oy = widget.y * scale;
        let pos = match widget.anchor.as_str() {
            "top_right" => pos2(area.max.x - ox, area.min.y + oy),
            "bottom_left" => pos2(area.min.x + ox, area.max.y - oy),
            "bottom_right" => pos2(area.max.x - ox, area.max.y - oy),
            "center" => pos2(area.center().x + ox, area.center().y + oy),
            _ => pos2(area.min.x + ox, area.min.y + oy),
        };
        let align = match widget.anchor.as_str() {
            "top_right" => Align2::RIGHT_TOP,
            "bottom_left" => Align2::LEFT_BOTTOM,
            "bottom_right" => Align2::RIGHT_BOTTOM,
            "center" => Align2::CENTER_CENTER,
            _ => Align2::LEFT_TOP,
        };
        let color = parse_hex(&widget.color).unwrap_or(Color32::WHITE);
        let font = egui::FontId::new(widget.size * scale, family("ui"));
        egui::Area::new(egui::Id::new(("balaur-widget", entity)))
            .order(egui::Order::Middle)
            .pivot(align)
            .fixed_pos(pos)
            .show(ctx, |ui| match widget.kind.as_str() {
                "button" => {
                    let response = ui.add(
                        egui::Button::new(
                            egui::RichText::new(&widget.text)
                                .font(font.clone())
                                .color(color),
                        )
                        .corner_radius(egui::CornerRadius::same(
                            (widget.size * scale).min(120.0) as u8
                        ))
                        .stroke(Stroke::new(1.0, color)),
                    );
                    if response.clicked() {
                        clicked.push(*entity);
                    }
                }
                "panel" => {
                    egui::Frame::new()
                        .fill(Color32::from_black_alpha(96))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&widget.text)
                                    .font(font.clone())
                                    .color(color),
                            );
                        });
                }
                _ => {
                    ui.label(
                        egui::RichText::new(&widget.text)
                            .font(font.clone())
                            .color(color),
                    );
                }
            });
    }
    settle_clicks(engine, &widgets, &clicked);
}

/// Record this frame's clicks on each widget, then fire their `on_click`.
///
/// Dispatch happens after the world borrow is released: a handler may spawn,
/// free or reparent nodes, and it must not do that mid-iteration.
fn settle_clicks(engine: &Engine, widgets: &[(Entity, Widget)], clicked: &[Entity]) {
    let mut signals: Vec<(Entity, String)> = Vec::new();
    {
        let world = engine.world();
        for (entity, _) in widgets {
            if let Ok(mut w) = world.get::<&mut Widget>(*entity) {
                w.clicked = clicked.contains(entity);
                if w.clicked && !w.on_click.is_empty() {
                    signals.push((*entity, w.on_click.clone()));
                }
            }
        }
    }
    if let Some(host) = engine.scripts() {
        for (entity, method) in signals {
            host.call_on(balaur_core::node_id_of(entity), &method);
        }
    }
}
