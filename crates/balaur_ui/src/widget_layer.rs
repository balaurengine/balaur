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

use crate::theme::family;

/// A component colour (`[r, g, b, a]` in 0..=1) as egui's 8-bit one.
fn rgba_color(rgba: [f32; 4]) -> Color32 {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(
        channel(rgba[0]),
        channel(rgba[1]),
        channel(rgba[2]),
        channel(rgba[3]),
    )
}

#[derive(Clone)]
pub struct Widget {
    pub kind: String,
    pub text: String,
    pub anchor: String,
    pub x: f32,
    pub y: f32,
    /// Height of the widget's text, in design pixels.
    pub font_size: f32,
    /// The text's colour as `[r, g, b, a]` in 0..=1, the same representation
    /// the `color` component uses.
    pub text_color: [f32; 4],
    /// Method on this node's script, called when the widget is clicked.
    /// Empty means nothing is connected. A name rather than a function value:
    /// scene files cannot hold closures, and a name works on any backend.
    pub on_click: String,
    pub clicked: bool,
}

/// Where and whether the widget layer draws. Games leave the default (full
/// window); editors point it at their viewport and enable it during play.
pub struct WidgetLayerConfig {
    pub enabled: bool,
    /// Design-px rect (x, y, w, h); None = whole screen.
    pub rect: Option<[f32; 4]>,
}

impl Default for WidgetLayerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rect: None,
        }
    }
}

/// The `widget` key, backed by exactly one `Widget` component on the node.
///
/// `clicked` is declared `readonly`: the widget layer writes it every frame
/// (see `settle_clicks`) and `apply` always clears it, but it is in the
/// schema so that `get`'s output round-trips and the inspector can see it.
pub(crate) fn register_widget_component(app: &mut App) {
    app.register_component(
        "widget",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                "widget",
                r#"kind = { type = "enum", default = "label", options = ["label", "button", "panel"], description = "The HUD element the widget layer draws" }
text = { type = "string", default = "label", description = "Label or button caption" }
anchor = { type = "enum", default = "top_left", options = ["top_left", "top_right", "bottom_left", "bottom_right", "center"], description = "Screen corner or center the offset is measured from" }
x = { type = "float", default = 16.0, description = "Horizontal offset from the anchor, in design pixels" }
y = { type = "float", default = 16.0, description = "Vertical offset from the anchor, in design pixels" }
font_size = { type = "float", default = 16.0, min = 6.0, description = "Text size in design pixels" }
text_color = { type = "color", default = [0.933, 0.945, 0.957, 1.0], description = "Text color" }
on_click = { type = "string", default = "", description = "Script method called on this node when the button is clicked" }
clicked = { type = "bool", default = false, readonly = true, description = "True on the frame the button was clicked" }"#,
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
                // Hex strings were expanded to floats by `merge_defaults`.
                let channel = |i: usize, default: f64| {
                    params
                        .get("text_color")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.get(i))
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(default) as f32
                };
                let widget = Widget {
                    kind: s("kind", "label"),
                    text: s("text", "label"),
                    anchor: s("anchor", "top_left"),
                    x: f("x", 16.0),
                    y: f("y", 16.0),
                    font_size: f("font_size", 16.0),
                    text_color: [
                        channel(0, 0.933),
                        channel(1, 0.945),
                        channel(2, 0.957),
                        channel(3, 1.0),
                    ],
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
                map.insert(
                    "font_size".into(),
                    toml::Value::Float(f64::from(widget.font_size)),
                );
                map.insert(
                    "text_color".into(),
                    toml::Value::Array(
                        widget
                            .text_color
                            .iter()
                            .map(|c| toml::Value::Float(f64::from(*c)))
                            .collect(),
                    ),
                );
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
pub(crate) fn draw(eng: &Engine, ctx: &egui::Context, scale: f32) {
    let Some(layer) = eng.try_resource::<WidgetLayerConfig>() else {
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
        let world = eng.world();
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
        let color = rgba_color(widget.text_color);
        let font = egui::FontId::new(widget.font_size * scale, family("ui"));
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
                            (widget.font_size * scale).min(120.0) as u8,
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
    settle_clicks(eng, &widgets, &clicked);
}

/// Record this frame's clicks on each widget, then fire their `on_click`.
///
/// Dispatch happens after the world borrow is released: a handler may spawn,
/// free or reparent nodes, and it must not do that mid-iteration.
fn settle_clicks(eng: &Engine, widgets: &[(Entity, Widget)], clicked: &[Entity]) {
    let mut signals: Vec<(Entity, String)> = Vec::new();
    {
        let world = eng.world();
        for (entity, _) in widgets {
            if let Ok(mut w) = world.get::<&mut Widget>(*entity) {
                w.clicked = clicked.contains(entity);
                if w.clicked && !w.on_click.is_empty() {
                    signals.push((*entity, w.on_click.clone()));
                }
            }
        }
    }
    if let Some(host) = eng.script_host() {
        for (entity, method) in signals {
            // No payload: the handler runs on the widget's own node, so
            // `self.node` already is the thing that was clicked.
            host.call_on(balaur_core::node_id_of(entity), &method, &[]);
        }
    }
}
