//! `ui.*` script bindings, one function per widget family.
//!
//! Split out of `widgets.rs` so neither the file nor any single registration
//! function grows past the house limits. Registration order is the order
//! [`crate::widgets::install`] calls these.

use anyhow::Result;
use balaur_core::mlua::{Function, Lua, Table, Value};
use balaur_core::Engine;
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Layout, Margin, Rect, Sense, Stroke, StrokeKind,
};

use crate::bridge::{scale, scoped, with_ctx, with_ui};
use crate::theme::{self, parse_hex};
use crate::widgets::{
    code_editor, draw_image, left_pill, panel_frame, pill_radius, sc, text, text_field, Opts,
};
use crate::UiState;

/// `ui.*` bindings: theme.
pub(crate) fn install_theme(lua: &Lua, t: &Table, engine: &Engine) -> Result<()> {
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

    Ok(())
}

/// `ui.*` bindings: panels.
pub(crate) fn install_panels(lua: &Lua, t: &Table) -> Result<()> {
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

    Ok(())
}

/// `ui.*` bindings: containers.
pub(crate) fn install_containers(lua: &Lua, t: &Table) -> Result<()> {
    install_layout_containers(lua, t)?;
    install_spacing_helpers(lua, t)?;

    Ok(())
}

/// `ui.*` bindings: text.
pub(crate) fn install_text(lua: &Lua, t: &Table) -> Result<()> {
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

    Ok(())
}

/// `ui.*` bindings: buttons.
pub(crate) fn install_buttons(lua: &Lua, t: &Table) -> Result<()> {
    install_button_widgets(lua, t)?;
    install_button_shapes(lua, t)?;

    Ok(())
}

/// `ui.*` bindings: controls.
pub(crate) fn install_controls(lua: &Lua, t: &Table) -> Result<()> {
    install_toggle_and_slider(lua, t)?;
    install_drag_value(lua, t)?;

    Ok(())
}

/// `ui.*` bindings: text input.
pub(crate) fn install_text_input(lua: &Lua, t: &Table, engine: &Engine) -> Result<()> {
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

    Ok(())
}

/// `ui.*` bindings: code.
pub(crate) fn install_code(lua: &Lua, t: &Table) -> Result<()> {
    t.set(
        "code_line",
        lua.create_function(
            move |_, (gutter, spans, opts): (String, Table, Option<Table>)| {
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
                    let gutter_color =
                        opts.color("gutter_color", Color32::from_rgb(0x76, 0x7e, 0x88));
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
                        if span
                            .get::<Option<bool>>("strong")
                            .ok()
                            .flatten()
                            .unwrap_or(false)
                        {
                            format.font_id = FontId::new(size, theme::family("mono"));
                            format.underline = Stroke::NONE;
                            format.color = color;
                        }
                        job.append(&s, 0.0, format);
                        Ok(())
                    })?;
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
        )?,
    )?;

    Ok(())
}

/// `ui.*` bindings: modal.
pub(crate) fn install_modal(lua: &Lua, t: &Table) -> Result<()> {
    t.set(
        "modal",
        lua.create_function(
            move |_, (id, opts, cb): (String, Option<Table>, Function)| {
                let opts = Opts(opts);
                with_ctx(|ctx| {
                    let screen = ctx.viewport_rect();
                    let scrim =
                        opts.color("scrim", Color32::from_rgba_unmultiplied(23, 25, 28, 107));
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
            },
        )?,
    )?;

    Ok(())
}

/// `ui.*` bindings: widget layer.
pub(crate) fn install_widget_layer(lua: &Lua, t: &Table, engine: &Engine) -> Result<()> {
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

    Ok(())
}

/// `ui.*` bindings: scale.
pub(crate) fn install_scale(lua: &Lua, t: &Table, engine: &Engine) -> Result<()> {
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

    Ok(())
}

/// `ui.*` bindings: code editor.
pub(crate) fn install_code_editor(lua: &Lua, t: &Table, engine: &Engine) -> Result<()> {
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

    Ok(())
}

/// `ui.*` bindings: drop-down select.
pub(crate) fn install_dropdown_select(lua: &Lua, t: &Table) -> Result<()> {
    t.set(
        "select",
        lua.create_function(
            move |_, (id, current, options, opts): (String, String, Table, Option<Table>)| {
                let opts = Opts(opts);
                with_ui(|ui| {
                    let mut items: Vec<String> = Vec::new();
                    options.for_each(|_: i64, v: String| {
                        items.push(v);
                        Ok(())
                    })?;
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
        )?,
    )?;

    Ok(())
}

/// `ui.*` bindings: images and overlay shapes.
pub(crate) fn install_images(lua: &Lua, t: &Table, engine: &Engine) -> Result<()> {
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
                        ui.painter()
                            .rect_stroke(rect, 0.0, stroke, StrokeKind::Inside);
                    }
                    Ok(())
                })
            },
        )?,
    )?;

    Ok(())
}

/// `ui.*` bindings: queries.
pub(crate) fn install_queries(lua: &Lua, t: &Table) -> Result<()> {
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

/// `ui.toggle` and `ui.hslider`.
fn install_toggle_and_slider(lua: &Lua, t: &Table) -> Result<()> {
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
        )?,
    )?;
    Ok(())
}

/// `ui.drag_value` and its keyboard handling.
fn install_drag_value(lua: &Lua, t: &Table) -> Result<()> {
    t.set(
        "drag_value",
        lua.create_function(move |_, (value, opts): (f64, Option<Table>)| {
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
        })?,
    )?;
    Ok(())
}

/// `ui.horizontal`, `ui.vertical`, `ui.right` and `ui.frame`.
fn install_layout_containers(lua: &Lua, t: &Table) -> Result<()> {
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
    Ok(())
}

/// `ui.scroll`, spacing and separators.
fn install_spacing_helpers(lua: &Lua, t: &Table) -> Result<()> {
    t.set(
        "scroll",
        lua.create_function(
            move |_, (id, opts, cb): (String, Option<Table>, Function)| {
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
            },
        )?,
    )?;
    t.set(
        "add_space",
        lua.create_function(move |_, px: f32| {
            with_ui(|ui| {
                let _: () = ui.add_space(sc(px));
                Ok(())
            })
        })?,
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
    Ok(())
}

/// `ui.pill` and `ui.menu_item`.
fn install_button_widgets(lua: &Lua, t: &Table) -> Result<()> {
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
                ui.painter()
                    .galley(pos2(rect.min.x + sc(10.0), y), galley, color);
                if response.clicked() {
                    ui.close();
                    return Ok(true);
                }
                Ok(false)
            })
        })?,
    )?;
    Ok(())
}

/// `ui.circle_button` and `ui.dot`.
fn install_button_shapes(lua: &Lua, t: &Table) -> Result<()> {
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
    Ok(())
}
