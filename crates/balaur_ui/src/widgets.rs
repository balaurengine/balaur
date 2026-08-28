//! The `ui` Lua module: panels, containers, and the design system's widget
//! shapes (pill, circle button, field, toggle, slider, code line, modal).
//! Every color arrives per call from the script's token table, so themes are
//! entirely script-defined.

use anyhow::Result;
use balaur_core::mlua::{self, Function, Table, Value};
use balaur_core::{App, Engine};
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, Rect, Sense, Stroke, StrokeKind,
    pos2, vec2,
};

use crate::bridge::{scale, scoped, with_ctx, with_ui};
use crate::theme::{self, parse_hex};
use crate::UiState;

// ---------------------------------------------------------------- opts

struct Opts(Option<Table>);

impl Opts {
    fn f32(&self, key: &str, default: f32) -> f32 {
        self.0
            .as_ref()
            .and_then(|t| t.get::<Option<f32>>(key).ok().flatten())
            .unwrap_or(default)
    }
    fn boolean(&self, key: &str, default: bool) -> bool {
        self.0
            .as_ref()
            .and_then(|t| t.get::<Option<bool>>(key).ok().flatten())
            .unwrap_or(default)
    }
    fn string(&self, key: &str) -> Option<String> {
        self.0
            .as_ref()
            .and_then(|t| t.get::<Option<String>>(key).ok().flatten())
    }
    fn color(&self, key: &str, default: Color32) -> Color32 {
        self.string(key)
            .and_then(|s| parse_hex(&s))
            .unwrap_or(default)
    }
    fn opt_color(&self, key: &str) -> Option<Color32> {
        self.string(key).and_then(|s| parse_hex(&s))
    }
    /// A dimension in design pixels, multiplied by the global UI scale.
    fn px(&self, key: &str, default: f32) -> f32 {
        self.f32(key, default) * scale()
    }
    fn function(&self, key: &str) -> Option<Function> {
        self.0
            .as_ref()
            .and_then(|t| t.get::<Option<Function>>(key).ok().flatten())
    }
}

/// Scale a literal design dimension.
fn sc(v: f32) -> f32 {
    v * scale()
}

fn pill_radius(h: f32) -> CornerRadius {
    CornerRadius::same((h / 2.0).min(127.0) as u8)
}

fn panel_frame(opts: &Opts) -> egui::Frame {
    let px = opts.px("padding_x", 0.0);
    let py = opts.px("padding_y", 0.0);
    let mut frame = egui::Frame::new().inner_margin(Margin::symmetric(px as i8, py as i8));
    if let Some(fill) = opts.opt_color("fill") {
        frame = frame.fill(fill);
    } else if opts.boolean("transparent", false) {
        frame = frame.fill(Color32::TRANSPARENT);
    }
    frame
}

fn text(label: &str, size: f32, fam: &str, color: Option<Color32>, strong: bool) -> egui::RichText {
    let mut rt = egui::RichText::new(label).size(size).family(theme::family(fam));
    if let Some(color) = color {
        rt = rt.color(color);
    }
    if strong {
        rt = rt.strong();
    }
    rt
}

// ---------------------------------------------------------------- install

pub fn install(app: &mut App) -> Result<()> {
    let host = app.engine.scripts().expect("script host present");
    let lua = host.lua();
    let m = host.module("ui")?;
    let t = m.table().clone();
    let engine = app.engine.clone();

    // ---- theme -------------------------------------------------------
    {
        let eng = engine.clone();
        t.set(
            "set_theme",
            lua.create_function(move |_, tokens: Table| {
                let state = eng.resource::<UiState>();
                let mut state = state.borrow_mut();
                state.theme.colors.clear();
                state.theme.dark = tokens.get::<Option<bool>>("dark")?.unwrap_or(true);
                tokens.for_each(|k: String, v: Value| {
                    if let Value::String(s) = v {
                        if let Some(color) = parse_hex(&s.to_str()?) {
                            state.theme.colors.insert(k, color);
                        }
                    }
                    Ok(())
                })?;
                state.theme_dirty = true;
                Ok(())
            })?,
        )?;
    }

    // ---- panels ------------------------------------------------------
    macro_rules! panel {
        ($name:literal, $ctor:ident, $size_key:literal) => {
            t.set(
                $name,
                lua.create_function(
                    move |_, (id, opts, cb): (String, Option<Table>, Function)| {
                        let opts = Opts(opts);
                        with_ui(|parent| {
                            let mut result = Ok(());
                            egui::Panel::$ctor(egui::Id::new(id))
                                .exact_size(opts.px($size_key, 32.0))
                                .resizable(false)
                                .frame(panel_frame(&opts))
                                .show_separator_line(opts.boolean("separator", true))
                                .show(parent, |ui| {
                                    result = scoped(ui, &cb);
                                });
                            result
                        })
                    },
                )?,
            )?;
        };
    }
    panel!("top_panel", top, "height");
    panel!("bottom_panel", bottom, "height");
    panel!("left_panel", left, "width");
    panel!("right_panel", right, "width");

    t.set(
        "central_panel",
        lua.create_function(move |_, (opts, cb): (Option<Table>, Function)| {
            let opts = Opts(opts);
            with_ui(|parent| {
                let mut result = Ok(());
                egui::CentralPanel::default()
                    .frame(panel_frame(&opts))
                    .show(parent, |ui| {
                        result = scoped(ui, &cb);
                    });
                result
            })
        })?,
    )?;

    // ---- containers --------------------------------------------------
    t.set(
        "horizontal",
        lua.create_function(move |_, (opts, cb): (Option<Table>, Function)| {
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
                    result = scoped(ui, &cb);
                });
                result
            })
        })?,
    )?;
    t.set(
        "vertical",
        lua.create_function(move |_, cb: Function| {
            with_ui(|ui| {
                let mut result = Ok(());
                ui.vertical(|ui| {
                    result = scoped(ui, &cb);
                });
                result
            })
        })?,
    )?;
    // Right-aligned run of widgets (declared left to right in Lua).
    t.set(
        "right",
        lua.create_function(move |_, cb: Function| {
            with_ui(|ui| {
                let mut result = Ok(());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    result = scoped(ui, &cb);
                });
                result
            })
        })?,
    )?;
    t.set(
        "frame",
        lua.create_function(move |_, (opts, cb): (Option<Table>, Function)| {
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
                    result = scoped(ui, &cb);
                });
                result
            })
        })?,
    )?;
    t.set(
        "scroll",
        lua.create_function(move |_, (id, opts, cb): (String, Option<Table>, Function)| {
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
                area.show(ui, |ui| {
                    result = scoped(ui, &cb);
                });
                result
            })
        })?,
    )?;
    t.set(
        "add_space",
        lua.create_function(move |_, px: f32| with_ui(|ui| Ok(ui.add_space(sc(px)))))?,
    )?;
    t.set(
        "separator",
        lua.create_function(move |_, color: Option<String>| {
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
        })?,
    )?;
    t.set(
        "spacing",
        lua.create_function(move |_, (x, y): (f32, f32)| {
            with_ui(|ui| {
                ui.spacing_mut().item_spacing = vec2(sc(x), sc(y));
                Ok(())
            })
        })?,
    )?;

    // ---- text --------------------------------------------------------
    t.set(
        "label",
        lua.create_function(move |_, (s, opts): (String, Option<Table>)| {
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
        })?,
    )?;

    // ---- buttons -----------------------------------------------------
    t.set(
        "pill",
        lua.create_function(move |_, (s, opts): (String, Option<Table>)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                if opts.string("align").as_deref() == Some("left") {
                    return left_pill(ui, &s, &opts);
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
                let mut button = egui::Button::new(rt)
                    .fill(fill)
                    .corner_radius(pill_radius(h))
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
                if let Some(menu) = opts.function("menu") {
                    response.context_menu(|ui| {
                        let _ = scoped(ui, &menu);
                    });
                }
                Ok(response.clicked())
            })
        })?,
    )?;
    // A row inside a context menu; clicking runs and closes the menu.
    t.set(
        "menu_item",
        lua.create_function(move |_, (s, opts): (String, Option<Table>)| {
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
                ui.painter().galley(pos2(rect.min.x + sc(10.0), y), galley, color);
                if response.clicked() {
                    ui.close();
                    return Ok(true);
                }
                Ok(false)
            })
        })?,
    )?;
    t.set(
        "circle_button",
        lua.create_function(move |_, (glyph, opts): (String, Option<Table>)| {
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
        })?,
    )?;
    t.set(
        "dot",
        lua.create_function(move |_, (color, d): (String, f32)| {
            with_ui(|ui| {
                let d = sc(d);
                let (rect, _) = ui.allocate_exact_size(vec2(d, d), Sense::hover());
                if let Some(color) = parse_hex(&color) {
                    ui.painter().circle_filled(rect.center(), d / 2.0, color);
                }
                Ok(())
            })
        })?,
    )?;

    // ---- controls ----------------------------------------------------
    t.set(
        "toggle",
        lua.create_function(move |_, (on, opts): (bool, Option<Table>)| {
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
        })?,
    )?;
    t.set(
        "hslider",
        lua.create_function(
            move |_, (value, min, max, opts): (f32, f32, f32, Option<Table>)| {
                let opts = Opts(opts);
                with_ui(|ui| {
                    let w = {
                        let w = opts.px("width", 0.0);
                        if w > 0.0 { w } else { ui.available_width().max(24.0) }
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
                        pos2(rail.min.x + rail.width() * t, rail.max.y),
                    );
                    let accent = opts.color("fill", Color32::from_rgb(0xd5, 0x81, 0x4e));
                    ui.painter().rect_filled(fill_rect, 2.5, accent);
                    let knob_x = rect.min.x + rect.width() * t;
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
        )?,
    )?;
    t.set(
        "drag_value",
        lua.create_function(move |_, (value, opts): (f64, Option<Table>)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let h = opts.px("height", 28.0);
                let w = {
                    let w = opts.px("width", 0.0);
                    if w > 0.0 { w } else { ui.available_width().max(24.0) }
                };
                let (rect, response) =
                    ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
                let mut value = value;
                let mut changed = false;
                if response.dragged() {
                    let delta = response.drag_delta().x as f64 * opts.f32("speed", 0.05) as f64;
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
                    Stroke::new(1.0, opts.color("stroke", Color32::from_rgb(0x2b, 0x30, 0x37))),
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
                let display = opts
                    .string("suffix")
                    .map(|s| format!("{value:.decimals$}{s}"))
                    .unwrap_or_else(|| format!("{value:.decimals$}"));
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
        })?,
    )?;

    // ---- text input --------------------------------------------------
    {
        let eng = engine.clone();
        t.set(
            "text_field",
            lua.create_function(
                move |_, (id, placeholder, opts): (String, Option<String>, Option<Table>)| {
                    let opts = Opts(opts);
                    text_field(&eng, &id, placeholder.as_deref().unwrap_or(""), &opts)
                },
            )?,
        )?;
    }
    {
        let eng = engine.clone();
        t.set(
            "set_text",
            lua.create_function(move |_, (id, value): (String, String)| {
                let state = eng.resource::<UiState>();
                let mut state = state.borrow_mut();
                state.text_buffers.insert(id.clone(), value);
                state.focused_once.remove(&id);
                Ok(())
            })?,
        )?;
    }

    // ---- code --------------------------------------------------------
    t.set(
        "code_line",
        lua.create_function(move |_, (gutter, spans, opts): (String, Table, Option<Table>)| {
            let opts = Opts(opts);
            with_ui(|ui| {
                let size = opts.px("size", 12.5);
                let row_h = size * opts.f32("line_height", 1.78);
                let gutter_w = opts.px("gutter_width", 24.0);
                let (rect, _) = ui
                    .allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
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
                spans.for_each(|_: i64, span: Table| {
                    let s: String = span.get("text").unwrap_or_default();
                    let color = span
                        .get::<Option<String>>("color")
                        .ok()
                        .flatten()
                        .and_then(|c| parse_hex(&c))
                        .unwrap_or(Color32::WHITE);
                    let mut format = egui::TextFormat::simple(mono.clone(), color);
                    if span.get::<Option<bool>>("strong").ok().flatten().unwrap_or(false) {
                        format.font_id = FontId::new(size, theme::family("mono"));
                        format.underline = Stroke::NONE;
                        format.color = color;
                    }
                    job.append(&s, 0.0, format);
                    Ok(())
                })?;
                let galley = ui.painter().layout_job(job);
                let y = rect.center().y - galley.size().y / 2.0;
                ui.painter()
                    .galley(pos2(rect.min.x + gutter_w + sc(12.0), y), galley, Color32::WHITE);
                Ok(())
            })
        })?,
    )?;

    // ---- modal -------------------------------------------------------
    t.set(
        "modal",
        lua.create_function(move |_, (id, opts, cb): (String, Option<Table>, Function)| {
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
                            result = scoped(ui, &cb);
                        });
                    });
                result?;
                Ok(scrim_clicked)
            })
        })?,
    )?;

    // ---- widget layer ------------------------------------------------
    {
        let eng = engine.clone();
        t.set(
            "set_widget_layer",
            lua.create_function(
                move |_,
                      (enabled, x, y, w, h): (
                    bool,
                    Option<f32>,
                    Option<f32>,
                    Option<f32>,
                    Option<f32>,
                )| {
                    let layer = eng.resource::<crate::WidgetLayer>();
                    let mut layer = layer.borrow_mut();
                    layer.enabled = enabled;
                    layer.rect = match (x, y, w, h) {
                        (Some(x), Some(y), Some(w), Some(h)) => Some([x, y, w, h]),
                        _ => None,
                    };
                    Ok(())
                },
            )?,
        )?;
    }

    // ---- scale -------------------------------------------------------
    {
        let eng = engine.clone();
        t.set(
            "set_scale",
            lua.create_function(move |_, f: f32| {
                let state = eng.resource::<UiState>();
                state.borrow_mut().scale = f.clamp(0.5, 3.0);
                Ok(())
            })?,
        )?;
    }

    // ---- code editor -------------------------------------------------
    {
        let eng = engine.clone();
        t.set(
            "code_editor",
            lua.create_function(
                move |_, (id, source, opts): (String, String, Option<Table>)| {
                    let opts = Opts(opts);
                    code_editor(&eng, &id, &source, &opts)
                },
            )?,
        )?;
    }

    // ---- images and overlay shapes ------------------------------------
    {
        let eng = engine.clone();
        t.set(
            "image",
            lua.create_function(move |_, (path, opts): (String, Option<Table>)| {
                let opts = Opts(opts);
                draw_image(&eng, &path, &opts)
            })?,
        )?;
    }
    // An outline rectangle painted at (x, y, w, h) in design px relative to
    // the current panel (viewport overlays: safe area, guides).
    t.set(
        "rect_stroke",
        lua.create_function(
            move |_, (x, y, w, h, opts): (f32, f32, f32, f32, Option<Table>)| {
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
                            ui.painter().add(egui::Shape::dashed_line(
                                pair,
                                stroke,
                                sc(6.0),
                                sc(5.0),
                            ));
                        }
                    } else {
                        ui.painter().rect_stroke(rect, 0.0, stroke, StrokeKind::Inside);
                    }
                    Ok(())
                })
            },
        )?,
    )?;

    // ---- queries -----------------------------------------------------
    // Queries return design pixels (real points divided by the UI scale), so
    // scripts compute layout in one consistent unit.
    t.set(
        "available_width",
        lua.create_function(move |_, ()| with_ui(|ui| Ok(ui.available_width() / scale())))?,
    )?;
    t.set(
        "available_height",
        lua.create_function(move |_, ()| with_ui(|ui| Ok(ui.available_height() / scale())))?,
    )?;
    t.set(
        "screen_size",
        lua.create_function(move |_, ()| {
            with_ctx(|ctx| {
                let rect = ctx.viewport_rect();
                Ok((rect.width() / scale(), rect.height() / scale()))
            })
        })?,
    )?;
    t.set(
        "shortcut",
        lua.create_function(move |_, (mods, key): (String, String)| {
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
        })?,
    )?;
    t.set(
        "wants_keyboard",
        lua.create_function(move |_, ()| with_ctx(|ctx| Ok(ctx.egui_wants_keyboard_input())))?,
    )?;

    Ok(())
}

fn text_field(
    engine: &Engine,
    id: &str,
    placeholder: &str,
    opts: &Opts,
) -> mlua::Result<(String, bool, bool)> {
    let state = engine.resource::<UiState>();
    let mut buffer = {
        let state = state.borrow();
        state.text_buffers.get(id).cloned().unwrap_or_default()
    };
    let autofocus = opts.boolean("autofocus", false) && {
        let mut state = state.borrow_mut();
        state.focused_once.insert(id.to_string())
    };
    let id_owned = id.to_string();
    let result = with_ui(|ui| {
        let size = opts.px("size", 13.0);
        let mut edit = egui::TextEdit::singleline(&mut buffer)
            .id(egui::Id::new(id_owned.clone()))
            .frame(egui::Frame::NONE)
            .hint_text(placeholder)
            .font(FontId::new(size, theme::family("ui")));
        if let Some(color) = opts.opt_color("color") {
            edit = edit.text_color(color);
        }
        let w = opts.px("width", 0.0);
        if w > 0.0 {
            edit = edit.desired_width(w);
        }
        let response = ui.add(edit);
        if autofocus {
            response.request_focus();
        }
        let submitted =
            response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        Ok((response.changed(), submitted))
    })?;
    let (changed, submitted) = result;
    state
        .borrow_mut()
        .text_buffers
        .insert(id.to_string(), buffer.clone());
    Ok((buffer, changed, submitted))
}


// ---------------------------------------------------------------- code editor

struct SyntaxColors {
    key: Color32,
    string: Color32,
    number: Color32,
    comment: Color32,
    ident: Color32,
    builtin: Color32,
    punct: Color32,
}

impl SyntaxColors {
    fn from_opts(opts: &Opts) -> Self {
        SyntaxColors {
            key: opts.color("k_key", Color32::from_rgb(0xf0, 0xa2, 0x73)),
            string: opts.color("k_str", Color32::from_rgb(0x8f, 0xbc, 0xae)),
            number: opts.color("k_num", Color32::from_rgb(0xff, 0xc7, 0xa8)),
            comment: opts.color("k_com", Color32::from_rgb(0x76, 0x7e, 0x88)),
            ident: opts.color("k_fn", Color32::from_rgb(0xee, 0xf1, 0xf4)),
            builtin: opts.color("k_type", Color32::from_rgb(0xb6, 0xd8, 0xcc)),
            punct: opts.color("k_punc", Color32::from_rgb(0x98, 0xa1, 0xaa)),
        }
    }
}

const LUAU_KEYWORDS: &[&str] = &[
    "local", "function", "end", "if", "then", "else", "elseif", "while", "for",
    "in", "do", "return", "and", "or", "not", "true", "false", "nil", "break",
    "continue", "repeat", "until", "type", "export",
];
const LUAU_BUILTINS: &[&str] = &[
    "engine", "scene", "input", "physics", "render", "audio", "rng", "ui",
    "log", "fs", "toml", "math", "string", "table", "self", "require",
];

/// Line-based Luau highlighting into a LayoutJob (used by the editable code
/// editor's layouter; mirrors the design handoff's tokenizer rules).
fn highlight_luau(text_src: &str, font: &FontId, colors: &SyntaxColors) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let fmt = |color: Color32| egui::TextFormat::simple(font.clone(), color);
    for (i, line) in text_src.split('\n').enumerate() {
        if i > 0 {
            job.append("\n", 0.0, fmt(colors.punct));
        }
        if line.trim_start().starts_with("--") {
            job.append(line, 0.0, fmt(colors.comment));
            continue;
        }
        let bytes = line.as_bytes();
        let mut pos = 0;
        while pos < bytes.len() {
            let rest = &line[pos..];
            let c = rest.chars().next().unwrap();
            let (token_len, color) = if c.is_whitespace() {
                (rest.chars().take_while(|c| c.is_whitespace()).map(char::len_utf8).sum(), colors.ident)
            } else if c == '"' || c == '\'' {
                let quote = c;
                let mut len = c.len_utf8();
                for ch in rest[len..].chars() {
                    len += ch.len_utf8();
                    if ch == quote {
                        break;
                    }
                }
                (len, colors.string)
            } else if c.is_ascii_digit() {
                (
                    rest.chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.')
                        .map(char::len_utf8)
                        .sum(),
                    colors.number,
                )
            } else if c.is_alphabetic() || c == '_' {
                let len: usize = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .map(char::len_utf8)
                    .sum();
                let word = &rest[..len];
                let color = if LUAU_KEYWORDS.contains(&word) {
                    colors.key
                } else if LUAU_BUILTINS.contains(&word) {
                    colors.builtin
                } else if word.chars().next().is_some_and(|c| c.is_uppercase()) {
                    colors.builtin
                } else {
                    colors.ident
                };
                (len, color)
            } else {
                (c.len_utf8(), colors.punct)
            };
            job.append(&rest[..token_len], 0.0, fmt(color));
            pos += token_len;
        }
    }
    job
}

/// An editable, syntax-highlighted code editor with a line-number gutter.
/// The buffer persists per `id` in `UiState`; returns (text, changed).
fn code_editor(
    engine: &Engine,
    id: &str,
    source: &str,
    opts: &Opts,
) -> mlua::Result<(String, bool)> {
    let state = engine.resource::<UiState>();
    let mut buffer = {
        let cached = state.borrow().text_buffers.get(id).cloned();
        match cached {
            Some(b) => b,
            None => {
                state
                    .borrow_mut()
                    .text_buffers
                    .insert(id.to_string(), source.to_string());
                source.to_string()
            }
        }
    };
    let size = opts.px("size", 12.5);
    let gutter_w = opts.px("gutter_width", 34.0);
    let gutter_color = opts.color("gutter_color", Color32::from_rgb(0x76, 0x7e, 0x88));
    let colors = SyntaxColors::from_opts(opts);
    let font = FontId::new(size, theme::family("mono"));
    let changed = with_ui(|ui| {
        let font = font.clone();
        let row_h = ui.painter().layout_no_wrap("0".into(), font.clone(), gutter_color).size().y;
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            // Gutter.
            let n_lines = buffer.split('\n').count().max(1);
            let (gutter_rect, _) = ui.allocate_exact_size(
                vec2(gutter_w, row_h * n_lines as f32),
                Sense::hover(),
            );
            for i in 0..n_lines {
                ui.painter().text(
                    pos2(
                        gutter_rect.max.x - sc(6.0),
                        gutter_rect.min.y + row_h * (i as f32 + 0.5),
                    ),
                    Align2::RIGHT_CENTER,
                    (i + 1).to_string(),
                    FontId::new((size - sc(1.5)).max(8.0), theme::family("mono")),
                    gutter_color,
                );
            }
            ui.add_space(sc(12.0));
            // Editor.
            let mut layouter =
                |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                    let mut job = highlight_luau(buf.as_str(), &font, &colors);
                    job.wrap.max_width = f32::INFINITY;
                    ui.fonts_mut(|f| f.layout_job(job))
                };
            let response = ui.add(
                egui::TextEdit::multiline(&mut buffer)
                    .id(egui::Id::new(id.to_string()))
                    .frame(egui::Frame::NONE)
                    .desired_width(ui.available_width())
                    .layouter(&mut layouter),
            );
            changed = response.changed();
        });
        Ok(changed)
    })?;
    state
        .borrow_mut()
        .text_buffers
        .insert(id.to_string(), buffer.clone());
    Ok((buffer, changed))
}


/// Draw a PNG from the project (cached as an egui texture by path).
fn draw_image(engine: &Engine, path: &str, opts: &Opts) -> mlua::Result<()> {
    let state = engine.resource::<UiState>();
    let cached = state.borrow().textures.get(path).cloned();
    with_ui(|ui| {
        let texture = match cached {
            Some(t) => t,
            None => {
                let full = match engine.try_resource::<balaur_core::project::ProjectRoot>() {
                    Some(root) if !std::path::Path::new(path).is_absolute() => {
                        root.borrow().0.join(path)
                    }
                    _ => std::path::PathBuf::from(path),
                };
                let dynamic = image::open(&full).map_err(mlua::Error::external)?;
                let rgba = dynamic.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                let handle =
                    ui.ctx()
                        .load_texture(path, color, egui::TextureOptions::LINEAR);
                state
                    .borrow_mut()
                    .textures
                    .insert(path.to_string(), handle.clone());
                handle
            }
        };
        let native = texture.size_vec2();
        let aspect = if native.y > 0.0 { native.x / native.y } else { 1.0 };
        let w = opts.px("width", 0.0);
        let h = opts.px("height", 0.0);
        let size = if w > 0.0 && h > 0.0 {
            vec2(w, h)
        } else if h > 0.0 {
            vec2(h * aspect, h)
        } else if w > 0.0 {
            vec2(w, w / aspect)
        } else {
            native
        };
        let mut img = egui::Image::new((texture.id(), size));
        let radius = opts.px("radius", 0.0);
        if radius > 0.0 {
            img = img.corner_radius(pill_radius(radius * 2.0));
        }
        ui.add(img);
        Ok(())
    })
}


/// A left-aligned pill row (tree rows, list rows, menu items): custom paint
/// so the icon and label hug the left edge instead of egui's centered
/// button text. Supports fill/stroke, a colored leading icon, an optional
/// trailing glyph on the right, tooltip and a context menu.
fn left_pill(ui: &mut egui::Ui, label: &str, opts: &Opts) -> mlua::Result<bool> {
    let h = opts.px("height", 27.0);
    let w = {
        let w = opts.px("min_width", 0.0);
        if w > 0.0 { w } else { ui.available_width().max(sc(40.0)) }
    };
    let (rect, mut response) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let hovered = response.hovered();
    let mut fill = opts.color("fill", Color32::TRANSPARENT);
    if hovered && fill == Color32::TRANSPARENT {
        if let Some(hover) = opts.opt_color("hover_fill") {
            fill = hover;
        }
    }
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, pill_radius(h), fill);
    }
    if let Some(stroke) = opts.opt_color("stroke") {
        ui.painter()
            .rect(rect, pill_radius(h), Color32::TRANSPARENT, Stroke::new(1.0, stroke), StrokeKind::Inside);
    }
    let fam = opts.string("font").unwrap_or_else(|| "ui".into());
    let size = opts.px("size", 12.0);
    let color = opts.color("color", Color32::WHITE);
    let mut x = rect.min.x + sc(10.0);
    if let Some(icon) = opts.string("icon") {
        let icon_color = opts.opt_color("icon_color").unwrap_or(color);
        let galley = ui.painter().layout_no_wrap(
            icon,
            FontId::new(opts.px("icon_size", 12.0), theme::family(&fam)),
            icon_color,
        );
        let y = rect.center().y - galley.size().y / 2.0;
        ui.painter().galley(pos2(x, y), galley, icon_color);
        x += sc(7.0) + opts.px("icon_size", 12.0);
    }
    let mut font = FontId::new(size, theme::family(&fam));
    if opts.boolean("strong", false) {
        font = FontId::new(size, theme::family(if fam == "ui" { "heading" } else { &fam }));
    }
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, color);
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter().galley(pos2(x, y), galley, color);
    if let Some(trailing) = opts.string("trailing") {
        let t_color = opts.opt_color("trailing_color").unwrap_or(color);
        let galley = ui.painter().layout_no_wrap(
            trailing,
            FontId::new(opts.px("trailing_size", 11.0), theme::family(&fam)),
            t_color,
        );
        let ty = rect.center().y - galley.size().y / 2.0;
        ui.painter().galley(
            pos2(rect.max.x - sc(11.0) - galley.size().x, ty),
            galley,
            t_color,
        );
    }
    if let Some(tip) = opts.string("tooltip") {
        response = response.on_hover_text(tip);
    }
    if let Some(menu) = opts.function("menu") {
        response.context_menu(|ui| {
            let _ = scoped(ui, &menu);
        });
    }
    Ok(response.clicked())
}
