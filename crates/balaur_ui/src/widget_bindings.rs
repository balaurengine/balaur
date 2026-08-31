//! `ui.*` script bindings, one function per widget family.
//!
//! Split out of `widgets.rs` so neither the file nor any single registration
//! function grows past the house limits. Registration order is the order
//! [`crate::widgets::install_ui_api`] calls these.

use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, CallbackId, Value};
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Layout, Margin, Rect, Sense, Stroke, StrokeKind,
};

use crate::bridge::{scale, scoped, with_ctx, with_ui};
use crate::theme::{self, parse_hex};
use crate::widgets::{
    code_editor, draw_image, left_pill, panel_frame, pill_radius, sc, text, text_field, Opts,
};
use crate::{UiConfig, UiState};

/// `ui.*` bindings: theme.
pub(crate) fn install_theme(m: &mut dyn Bindings<Engine>) {
    {
        // No reader by design (N8): `UiConfig::theme` already holds every
        // token the caller wrote, and scripts keep their own palette table;
        // add `ui.theme` when a caller needs to read it back.
        m.function("set_theme", |eng: &Engine, tokens: Value| {
            let Value::Map(entries) = &tokens else {
                anyhow::bail!("set_theme takes a table of tokens");
            };
            let config = eng.resource::<UiConfig>();
            let mut config = config.borrow_mut();
            config.theme.colors.clear();
            for (key, value) in entries {
                match (key.as_str(), value) {
                    ("dark", Value::Bool(dark)) => config.theme.dark = *dark,
                    (_, Value::Str(hex)) => {
                        if let Some(color) = parse_hex(hex) {
                            config.theme.colors.insert(key.clone(), color);
                        }
                    }
                    _ => {}
                }
            }
            config.changed = true;
            Ok(())
        });
    }
}

/// `ui.*` bindings: panels.
pub(crate) fn install_panels(m: &mut dyn Bindings<Engine>) {
    macro_rules! panel {
        ($name:literal, $ctor:ident, $size_key:literal) => {
            m.function(
                $name,
                |eng: &Engine, (id, opts, cb): (String, Option<Value>, CallbackId)| {
                    let opts = Opts(opts);
                    with_ui(|parent| {
                        let mut result = Ok(());
                        egui::Panel::$ctor(egui::Id::new(id))
                            .exact_size(opts.px($size_key, 32.0))
                            .resizable(false)
                            .frame(panel_frame(&opts))
                            .show_separator_line(opts.boolean("separator", true))
                            .show(parent, |ui| {
                                result = scoped(eng, ui, cb);
                            });
                        result
                    })
                },
            );
        };
    }
    panel!("top_panel", top, "height");
    panel!("bottom_panel", bottom, "height");
    panel!("left_panel", left, "width");
    panel!("right_panel", right, "width");

    m.function(
        "central_panel",
        |eng: &Engine, (opts, cb): (Option<Value>, CallbackId)| {
            let opts = Opts(opts);
            with_ui(|parent| {
                let mut result = Ok(());
                egui::CentralPanel::default()
                    .frame(panel_frame(&opts))
                    .show(parent, |ui| {
                        result = scoped(eng, ui, cb);
                    });
                result
            })
        },
    );
    // Chrome above the widget layer: panels share egui's background layer,
    // which every widget Area draws over, so overlays need a layer of their own.
    m.function(
        "overlay",
        |eng: &Engine, (id, opts, cb): (String, Option<Value>, CallbackId)| {
            let opts = Opts(opts);
            with_ctx(|ctx| {
                let x = opts.px("x", 0.0);
                let y = opts.px("y", 0.0);
                let w = opts.px("w", 0.0);
                let h = opts.px("h", 0.0);
                let mut result = Ok(());
                egui::Area::new(egui::Id::new(id))
                    .order(egui::Order::Foreground)
                    .fixed_pos(pos2(x, y))
                    .show(ctx, |ui| {
                        if w > 0.0 && h > 0.0 {
                            ui.set_min_size(vec2(w, h));
                            ui.set_max_size(vec2(w, h));
                        }
                        result = scoped(eng, ui, cb);
                    });
                result
            })
        },
    );
}

/// `ui.*` bindings: containers.
pub(crate) fn install_containers(m: &mut dyn Bindings<Engine>) {
    install_layout_containers(m);
    install_spacing_helpers(m);
}

/// `ui.*` bindings: text.
pub(crate) fn install_text(m: &mut dyn Bindings<Engine>) {
    m.function(
        "label",
        |_eng: &Engine, (s, opts): (String, Option<Value>)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let fam = opts.string("font").unwrap_or_else(|| "ui".into());
                let rt = text(
                    &s,
                    opts.px("size", 12.0),
                    &fam,
                    opts.opt_color("color"),
                    opts.boolean("strong", false),
                );
                let mut label = egui::Label::new(rt).selectable(false);
                if !opts.boolean("wrap", false) {
                    label = label.extend();
                }
                ui.add(label);
                Ok(())
            })
        },
    );
}

/// `ui.*` bindings: buttons.
pub(crate) fn install_buttons(m: &mut dyn Bindings<Engine>) {
    install_button_widgets(m);
    install_button_shapes(m);
}

/// `ui.*` bindings: controls.
pub(crate) fn install_controls(m: &mut dyn Bindings<Engine>) {
    install_toggle_and_slider(m);
    install_drag_value(m);
}

/// `ui.*` bindings: text input.
pub(crate) fn install_text_input(m: &mut dyn Bindings<Engine>) {
    {
        m.function(
            "text_field",
            |eng: &Engine, (id, placeholder, opts): (String, Option<String>, Option<Value>)| {
                let opts = Opts(opts);
                text_field(eng, &id, placeholder.as_deref().unwrap_or(""), &opts)
            },
        );
    }
    {
        m.function("set_text", |eng: &Engine, (id, value): (String, String)| {
            let state = eng.resource::<UiState>();
            let mut state = state.borrow_mut();
            // The seed is left alone on purpose: it records what the *source*
            // last was, which this write does not change, so the override
            // survives until the source itself moves.
            state.text_buffers.insert(id.clone(), value);
            state.focused_once.remove(&id);
            Ok(())
        });
    }
}

/// `ui.*` bindings: code.
pub(crate) fn install_code(m: &mut dyn Bindings<Engine>) {
    m.function(
        "code_line",
        |_eng: &Engine, (gutter, spans, opts): (String, Value, Option<Value>)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let size = opts.px("size", 12.5);
                let row_h = size * opts.f32("line_height", 1.78);
                let gutter_w = opts.px("gutter_width", 24.0);
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
                if let Some(fill) = opts.opt_color("highlight") {
                    ui.painter().rect_filled(rect, 0.0, fill);
                }
                let mono = FontId::new(size, theme::family("mono"));
                let gutter_color = opts.color("gutter_color", Color32::from_rgb(0x76, 0x7e, 0x88));
                ui.painter().text(
                    pos2(rect.min.x + gutter_w, rect.center().y),
                    Align2::RIGHT_CENTER,
                    gutter,
                    FontId::new((size - 1.5).max(8.0), theme::family("mono")),
                    gutter_color,
                );
                let mut job = egui::text::LayoutJob::default();
                // `spans` is a list of { text, color?, strong? } maps.
                let items: &[Value] = match &spans {
                    Value::List(items) => items,
                    _ => &[],
                };
                for span in items {
                    let field = |key: &str| match span {
                        Value::Map(entries) => {
                            entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
                        }
                        _ => None,
                    };
                    let text = match field("text") {
                        Some(Value::Str(s)) => s.clone(),
                        _ => String::new(),
                    };
                    let color = match field("color") {
                        Some(Value::Str(c)) => parse_hex(c).unwrap_or(Color32::WHITE),
                        _ => Color32::WHITE,
                    };
                    let mut format = egui::TextFormat::simple(mono.clone(), color);
                    if matches!(field("strong"), Some(Value::Bool(true))) {
                        format.font_id = FontId::new(size, theme::family("mono"));
                        format.underline = Stroke::NONE;
                        format.color = color;
                    }
                    job.append(&text, 0.0, format);
                }
                let galley = ui.painter().layout_job(job);
                let y = rect.center().y - galley.size().y / 2.0;
                ui.painter().galley(
                    pos2(rect.min.x + gutter_w + sc(12.0), y),
                    galley,
                    Color32::WHITE,
                );
                Ok(())
            })
        },
    );
}

/// `ui.*` bindings: modal.
pub(crate) fn install_modal(m: &mut dyn Bindings<Engine>) {
    m.function(
        "modal",
        |eng: &Engine, (id, opts, cb): (String, Option<Value>, CallbackId)| {
            let opts = Opts(opts);
            with_ctx(|ctx| {
                let screen = ctx.viewport_rect();
                let scrim = opts.color("scrim", Color32::from_rgba_unmultiplied(23, 25, 28, 107));
                let mut scrim_clicked = false;
                egui::Area::new(egui::Id::new(format!("{id}-scrim")))
                    .order(egui::Order::Foreground)
                    .fixed_pos(screen.min)
                    .show(ctx, |ui| {
                        ui.painter().rect_filled(screen, 0.0, scrim);
                        let response = ui.allocate_rect(screen, Sense::click());
                        scrim_clicked = response.clicked();
                    });
                let width = opts.px("width", 560.0).min(screen.width() * 0.92);
                let top = screen.height() * opts.f32("top", 0.11);
                let mut result = Ok(());
                egui::Area::new(egui::Id::new(id))
                    .order(egui::Order::Foreground)
                    .fixed_pos(pos2(screen.center().x - width / 2.0, top))
                    .show(ctx, |ui| {
                        let mut frame = egui::Frame::new()
                            .corner_radius(pill_radius(sc(32.0)))
                            .fill(opts.color("fill", Color32::from_rgb(0x20, 0x24, 0x2a)));
                        if let Some(stroke) = opts.opt_color("stroke") {
                            frame = frame.stroke(Stroke::new(1.0, stroke));
                        }
                        frame.show(ui, |ui| {
                            ui.set_width(width);
                            result = scoped(eng, ui, cb);
                        });
                    });
                result?;
                Ok(scrim_clicked)
            })
        },
    );
}

/// `ui.*` bindings: widget layer.
pub(crate) fn install_widget_layer(m: &mut dyn Bindings<Engine>) {
    {
        // No reader by design (N8): the `WidgetLayerConfig` entry already
        // holds the flag and the rect; add `ui.widget_layer` when a caller
        // needs to read them back.
        m.function(
            "set_widget_layer",
            |eng: &Engine, (enabled, x, y, w, h): (
                    bool,
                    Option<f32>,
                    Option<f32>,
                    Option<f32>,
                    Option<f32>,
                )| {
                    let layer = eng.resource::<crate::WidgetLayerConfig>();
                    let mut layer = layer.borrow_mut();
                    layer.enabled = enabled;
                    layer.rect = match (x, y, w, h) {
                        (Some(x), Some(y), Some(w), Some(h)) => Some([x, y, w, h]),
                        _ => None,
                    };
                    Ok(())
                },
            );
    }
}

/// `ui.*` bindings: scale.
pub(crate) fn install_scale(m: &mut dyn Bindings<Engine>) {
    m.function("scale", |eng: &Engine, ()| {
        let config = eng.resource::<UiConfig>();
        let v = config.borrow().scale;
        Ok(v)
    });
    m.function("set_scale", |eng: &Engine, f: f32| {
        let config = eng.resource::<UiConfig>();
        config.borrow_mut().scale = f.clamp(0.5, 3.0);
        Ok(())
    });
}

/// `ui.*` bindings: code editor.
pub(crate) fn install_code_editor(m: &mut dyn Bindings<Engine>) {
    {
        m.function(
            "code_editor",
            |eng: &Engine, (id, source, opts): (String, String, Option<Value>)| {
                let opts = Opts(opts);
                code_editor(eng, &id, &source, &opts)
            },
        );
    }
}

/// `ui.*` bindings: drop-down select.
pub(crate) fn install_dropdown_select(m: &mut dyn Bindings<Engine>) {
    m.function(
        "select",
        |_eng: &Engine, (id, current, options, opts): (String, String, Value, Option<Value>)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let items: Vec<String> = match &options {
                    Value::List(vs) => vs
                        .iter()
                        .filter_map(|v| match v {
                            Value::Str(s) => Some(s.clone()),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                let w = opts.px("width", 160.0);
                let size = opts.px("size", 12.0);
                let text_color = opts.color("color", Color32::WHITE);
                let mut selected = current.clone();
                ui.scope(|ui| {
                    // Pill-shaped shell and menu items for this widget only.
                    let radius = pill_radius(opts.px("height", 28.0));
                    let visuals = &mut ui.style_mut().visuals;
                    visuals.widgets.inactive.corner_radius = radius;
                    visuals.widgets.hovered.corner_radius = radius;
                    visuals.widgets.active.corner_radius = radius;
                    visuals.widgets.open.corner_radius = radius;
                    ui.spacing_mut().button_padding = vec2(sc(11.0), sc(6.0));
                    egui::ComboBox::from_id_salt(id)
                        .width(w)
                        .selected_text(
                            egui::RichText::new(&current)
                                .size(size)
                                .family(theme::family("ui"))
                                .color(text_color),
                        )
                        .show_ui(ui, |ui| {
                            for item in &items {
                                ui.selectable_value(
                                    &mut selected,
                                    item.clone(),
                                    egui::RichText::new(item)
                                        .size(size)
                                        .family(theme::family("ui")),
                                );
                            }
                        });
                });
                let changed = selected != current;
                Ok((selected, changed))
            })
        },
    );
}

/// `ui.*` bindings: images and overlay shapes.
pub(crate) fn install_images(m: &mut dyn Bindings<Engine>) {
    {
        m.function(
            "image",
            |eng: &Engine, (path, opts): (String, Option<Value>)| {
                let opts = Opts(opts);
                draw_image(eng, &path, &opts)
            },
        );
    }
    // An outline rectangle painted at (x, y, w, h) in design px relative to
    // the current panel (viewport overlays: safe area, guides).
    m.function(
        "rect_stroke",
        |_eng: &Engine, (x, y, w, h, opts): (f32, f32, f32, f32, Option<Value>)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let origin = ui.max_rect().min;
                let rect = Rect::from_min_size(
                    pos2(origin.x + sc(x), origin.y + sc(y)),
                    vec2(sc(w), sc(h)),
                );
                let stroke = Stroke::new(
                    opts.f32("width", 1.5),
                    opts.color("color", Color32::from_rgb(0xd5, 0x81, 0x4e)),
                );
                if opts.boolean("dashed", false) {
                    let corners = [
                        rect.left_top(),
                        rect.right_top(),
                        rect.right_bottom(),
                        rect.left_bottom(),
                        rect.left_top(),
                    ];
                    for pair in corners.windows(2) {
                        ui.painter()
                            .add(egui::Shape::dashed_line(pair, stroke, sc(6.0), sc(5.0)));
                    }
                } else {
                    ui.painter()
                        .rect_stroke(rect, 0.0, stroke, StrokeKind::Inside);
                }
                Ok(())
            })
        },
    );
}

/// `ui.*` bindings: queries.
pub(crate) fn install_queries(m: &mut dyn Bindings<Engine>) {
    // Queries return design pixels (real points divided by the UI scale), so
    // scripts compute layout in one consistent unit.
    m.function("available_width", |_eng: &Engine, ()| {
        with_ui(|ui| Ok(ui.available_width() / scale()))
    });
    m.function("available_height", |_eng: &Engine, ()| {
        with_ui(|ui| Ok(ui.available_height() / scale()))
    });
    m.function("screen_size", |_eng: &Engine, ()| {
        with_ctx(|ctx| {
            let rect = ctx.viewport_rect();
            Ok((rect.width() / scale(), rect.height() / scale()))
        })
    });
    m.function(
        "shortcut",
        |_eng: &Engine, (mods, key): (String, String)| {
            with_ctx(|ctx| {
                let Some(key) = egui::Key::from_name(&key) else {
                    return Ok(false);
                };
                let modifiers = match mods.as_str() {
                    "cmd" => egui::Modifiers::COMMAND,
                    "ctrl" => egui::Modifiers::CTRL,
                    "alt" => egui::Modifiers::ALT,
                    "shift" => egui::Modifiers::SHIFT,
                    _ => egui::Modifiers::NONE,
                };
                Ok(ctx.input_mut(|i| i.consume_key(modifiers, key)))
            })
        },
    );
    m.function("wants_keyboard", |_eng: &Engine, ()| {
        with_ctx(|ctx| Ok(ctx.egui_wants_keyboard_input()))
    });
}

/// The two controls that return their edited value plus whether this frame
/// changed it, so a script can write back only on a real edit.
fn install_toggle_and_slider(m: &mut dyn Bindings<Engine>) {
    m.function(
        "toggle",
        |_eng: &Engine, (on, opts): (bool, Option<Value>)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(sc(42.0), sc(24.0)), Sense::click());
                let on = if response.clicked() { !on } else { on };
                let track = if on {
                    opts.color("on_fill", Color32::from_rgb(0xd5, 0x81, 0x4e))
                } else {
                    opts.color("off_fill", Color32::from_rgb(0x10, 0x12, 0x15))
                };
                let knob = if on {
                    opts.color("on_knob", Color32::from_rgb(0xf9, 0xf4, 0xed))
                } else {
                    opts.color("off_knob", Color32::from_rgb(0x76, 0x7e, 0x88))
                };
                ui.painter().rect_filled(rect, sc(12.0), track);
                let x = if on {
                    rect.max.x - sc(12.0)
                } else {
                    rect.min.x + sc(12.0)
                };
                ui.painter()
                    .circle_filled(pos2(x, rect.center().y), sc(9.0), knob);
                Ok((on, response.clicked()))
            })
        },
    );
    m.function(
        "slider",
        |_eng: &Engine, (value, min, max, opts): (f32, f32, f32, Option<Value>)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let w = {
                    let w = opts.px("width", 0.0);
                    if w > 0.0 {
                        w
                    } else {
                        ui.available_width().max(24.0)
                    }
                };
                let (rect, response) =
                    ui.allocate_exact_size(vec2(w, sc(28.0)), Sense::click_and_drag());
                let mut value = value;
                if response.dragged() || response.clicked() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let t = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                        value = min + t * (max - min);
                    }
                }
                let t = if max > min {
                    ((value - min) / (max - min)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let rail = Rect::from_min_max(
                    pos2(rect.min.x, rect.center().y - sc(2.5)),
                    pos2(rect.max.x, rect.center().y + sc(2.5)),
                );
                ui.painter().rect_filled(
                    rail,
                    2.5,
                    opts.color("rail", Color32::from_rgb(0x10, 0x12, 0x15)),
                );
                let fill_rect = Rect::from_min_max(
                    rail.min,
                    pos2(rail.width().mul_add(t, rail.min.x), rail.max.y),
                );
                let accent = opts.color("fill", Color32::from_rgb(0xd5, 0x81, 0x4e));
                ui.painter().rect_filled(fill_rect, 2.5, accent);
                let knob_x = rect.width().mul_add(t, rect.min.x);
                ui.painter().circle_filled(
                    pos2(knob_x, rect.center().y),
                    sc(6.5),
                    opts.color("knob", Color32::from_rgb(0x17, 0x19, 0x1c)),
                );
                ui.painter().circle_stroke(
                    pos2(knob_x, rect.center().y),
                    sc(6.5),
                    Stroke::new(2.0, accent),
                );
                Ok((value, response.dragged() || response.clicked()))
            })
        },
    );
}

/// `ui.drag_value` and its keyboard handling.
fn install_drag_value(m: &mut dyn Bindings<Engine>) {
    m.function(
        "drag_value",
        |_eng: &Engine, (value, opts): (f64, Option<Value>)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let h = opts.px("height", 28.0);
                let w = {
                    let w = opts.px("width", 0.0);
                    if w > 0.0 {
                        w
                    } else {
                        ui.available_width().max(24.0)
                    }
                };
                let (rect, response) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
                let mut value = value;
                let mut changed = false;
                if response.dragged() {
                    let delta =
                        f64::from(response.drag_delta().x) * f64::from(opts.f32("speed", 0.05));
                    if delta != 0.0 {
                        value += delta;
                        changed = true;
                    }
                }
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                ui.painter().rect(
                    rect,
                    pill_radius(h),
                    opts.color("fill", Color32::from_rgb(0x10, 0x12, 0x15)),
                    Stroke::new(
                        1.0,
                        opts.color("stroke", Color32::from_rgb(0x2b, 0x30, 0x37)),
                    ),
                    StrokeKind::Inside,
                );
                let mut x = rect.min.x + sc(11.0);
                if let Some(prefix) = opts.string("prefix") {
                    let color = opts.color("prefix_color", Color32::from_rgb(0xf0, 0xa2, 0x73));
                    let galley = ui.painter().layout_no_wrap(
                        prefix,
                        FontId::new(sc(10.0), theme::family("heading")),
                        color,
                    );
                    let y = rect.center().y - galley.size().y / 2.0;
                    ui.painter().galley(pos2(x, y), galley, color);
                    x += sc(14.0);
                }
                let decimals = opts.f32("decimals", 1.0) as usize;
                let display = opts.string("suffix").map_or_else(
                    || format!("{value:.decimals$}"),
                    |s| format!("{value:.decimals$}{s}"),
                );
                let color = opts.color("color", Color32::from_rgb(0xee, 0xf1, 0xf4));
                let galley = ui.painter().layout_no_wrap(
                    display,
                    FontId::new(sc(11.5), theme::family("mono")),
                    color,
                );
                let y = rect.center().y - galley.size().y / 2.0;
                ui.painter().galley(pos2(x, y), galley, color);
                Ok((value, changed))
            })
        },
    );
}

/// `ui.horizontal`, `ui.vertical`, `ui.right` and `ui.frame`.
fn install_layout_containers(m: &mut dyn Bindings<Engine>) {
    m.function(
        "horizontal",
        |eng: &Engine, (opts, cb): (Option<Value>, CallbackId)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let mut result = Ok(());
                let layout = Layout::left_to_right(Align::Center);
                let height = opts.px("height", 0.0);
                // tight: hug the content instead of claiming the full row;
                // width: exact width (needed inside right-to-left layouts,
                // where growing children would overlap neighbors).
                let fixed = opts.px("width", 0.0);
                let width = if fixed > 0.0 {
                    fixed
                } else if opts.boolean("tight", false) {
                    0.0
                } else {
                    ui.available_width()
                };
                ui.allocate_ui_with_layout(vec2(width, height.max(0.0)), layout, |ui| {
                    if height > 0.0 {
                        ui.set_min_height(height);
                    }
                    // egui advances the parent by the child's actual size;
                    // pin the min width so fixed columns really consume it.
                    if fixed > 0.0 {
                        ui.set_min_width(fixed);
                    }
                    result = scoped(eng, ui, cb);
                });
                result
            })
        },
    );
    m.function("vertical", |eng: &Engine, cb: CallbackId| {
        with_ui(|ui| {
            let mut result = Ok(());
            ui.vertical(|ui| {
                result = scoped(eng, ui, cb);
            });
            result
        })
    });
    // Right-aligned run of widgets (declared left to right in script).
    m.function("right", |eng: &Engine, cb: CallbackId| {
        with_ui(|ui| {
            let mut result = Ok(());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                result = scoped(eng, ui, cb);
            });
            result
        })
    });
    m.function(
        "frame",
        |eng: &Engine, (opts, cb): (Option<Value>, CallbackId)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let mut result = Ok(());
                let mut frame = egui::Frame::new()
                    .inner_margin(Margin::symmetric(
                        opts.px("padding_x", 0.0) as i8,
                        opts.px("padding_y", 0.0) as i8,
                    ))
                    .corner_radius(pill_radius(opts.px("radius", 0.0) * 2.0));
                if let Some(fill) = opts.opt_color("fill") {
                    frame = frame.fill(fill);
                }
                if let Some(stroke) = opts.opt_color("stroke") {
                    frame = frame.stroke(Stroke::new(1.0, stroke));
                }
                frame.show(ui, |ui| {
                    result = scoped(eng, ui, cb);
                });
                result
            })
        },
    );
}

/// `ui.scroll`, spacing and separators.
fn install_spacing_helpers(m: &mut dyn Bindings<Engine>) {
    m.function(
        "scroll",
        |eng: &Engine, (id, opts, cb): (String, Option<Value>, CallbackId)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let mut result = Ok(());
                let mut area = egui::ScrollArea::vertical()
                    .id_salt(id)
                    .auto_shrink([false, false]);
                let max_h = opts.px("max_height", 0.0);
                if max_h > 0.0 {
                    area = area.max_height(max_h);
                }
                // A log follows what is arriving unless the reader has scrolled
                // away from the end, which egui tracks for us.
                if opts.boolean("stick_to_bottom", false) {
                    area = area.stick_to_bottom(true);
                }
                area.show(ui, |ui| {
                    result = scoped(eng, ui, cb);
                });
                result
            })
        },
    );
    m.function("add_space", |_eng: &Engine, px: f32| {
        with_ui(|ui| {
            let _: () = ui.add_space(sc(px));
            Ok(())
        })
    });
    m.function("separator", |_eng: &Engine, color: Option<String>| {
        with_ui(|ui| {
            match color.and_then(|c| parse_hex(&c)) {
                Some(color) => {
                    let rect = ui
                        .allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover())
                        .0;
                    ui.painter().rect_filled(rect, 0.0, color);
                }
                None => {
                    ui.separator();
                }
            }
            Ok(())
        })
    });
    m.function("spacing", |_eng: &Engine, (x, y): (f32, f32)| {
        with_ui(|ui| {
            ui.spacing_mut().item_spacing = vec2(sc(x), sc(y));
            Ok(())
        })
    });
}

/// `ui.pill` and `ui.menu_item`.
fn install_button_widgets(m: &mut dyn Bindings<Engine>) {
    m.function(
        "pill",
        |eng: &Engine, (s, opts): (String, Option<Value>)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                if opts.string("align").as_deref() == Some("left") {
                    return left_pill(eng, ui, &s, &opts);
                }
                let h = opts.px("height", 27.0);
                let fam = opts.string("font").unwrap_or_else(|| "ui".into());
                let mut display = String::new();
                if let Some(icon) = opts.string("icon") {
                    display.push_str(&icon);
                    if !s.is_empty() {
                        display.push_str("  ");
                    }
                }
                display.push_str(&s);
                let rt = text(
                    &display,
                    opts.px("size", 12.0),
                    &fam,
                    opts.opt_color("color"),
                    opts.boolean("strong", false),
                );
                let fill = opts.color("fill", Color32::TRANSPARENT);
                // Default rounding is fully-round, which is what a pill is;
                // `radius` is for the shapes that are tiles, not pills.
                let radius = opts.px("radius", 0.0);
                let corner = if radius > 0.0 {
                    pill_radius(radius * 2.0)
                } else {
                    pill_radius(h)
                };
                let mut button = egui::Button::new(rt)
                    .fill(fill)
                    .corner_radius(corner)
                    .min_size(vec2(opts.px("min_width", 0.0), h));
                button = match opts.opt_color("stroke") {
                    Some(color) => button.stroke(Stroke::new(1.0, color)),
                    None => button.stroke(Stroke::NONE),
                };
                let mut response = ui.add(button);
                if let Some(tip) = opts.string("tooltip") {
                    response = response.on_hover_text(tip);
                }
                // Right-click context menu: the callback draws menu items.
                if let Some(menu) = opts.callback("menu") {
                    response.context_menu(|ui| {
                        let _ = scoped(eng, ui, menu);
                    });
                }
                Ok(response.clicked())
            })
        },
    );
    // A row inside a context menu; clicking runs and closes the menu.
    m.function(
        "menu_item",
        |_eng: &Engine, (s, opts): (String, Option<Value>)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(sc(180.0), sc(26.0)), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        pill_radius(sc(26.0)),
                        Color32::from_white_alpha(10),
                    );
                }
                let color = opts.color("color", Color32::WHITE);
                let galley = ui.painter().layout_no_wrap(
                    s.clone(),
                    FontId::new(opts.px("size", 12.5), theme::family("ui")),
                    color,
                );
                let y = rect.center().y - galley.size().y / 2.0;
                ui.painter()
                    .galley(pos2(rect.min.x + sc(10.0), y), galley, color);
                if response.clicked() {
                    ui.close();
                    return Ok(true);
                }
                Ok(false)
            })
        },
    );
}

/// `ui.circle_button` and `ui.dot`.
fn install_button_shapes(m: &mut dyn Bindings<Engine>) {
    m.function(
        "circle_button",
        |_eng: &Engine, (glyph, opts): (String, Option<Value>)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let d = opts.px("d", 32.0);
                let rt = text(
                    &glyph,
                    opts.px("size", 14.0),
                    &opts.string("font").unwrap_or_else(|| "ui".into()),
                    opts.opt_color("color"),
                    opts.boolean("strong", false),
                );
                let mut button = egui::Button::new(rt)
                    .fill(opts.color("fill", Color32::TRANSPARENT))
                    .corner_radius(pill_radius(d))
                    .min_size(vec2(d, d));
                button = match opts.opt_color("stroke") {
                    Some(color) => button.stroke(Stroke::new(1.0, color)),
                    None => button.stroke(Stroke::NONE),
                };
                let mut response = ui.add(button);
                if let Some(tip) = opts.string("tooltip") {
                    response = response.on_hover_text(tip);
                }
                Ok(response.clicked())
            })
        },
    );
    m.function("dot", |_eng: &Engine, (color, d): (String, f32)| {
        with_ui(|ui| {
            let d = sc(d);
            let (rect, _) = ui.allocate_exact_size(vec2(d, d), Sense::hover());
            if let Some(color) = parse_hex(&color) {
                ui.painter().circle_filled(rect.center(), d / 2.0, color);
            }
            Ok(())
        })
    });
}
