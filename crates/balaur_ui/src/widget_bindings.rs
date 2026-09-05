//! `ui.*` script bindings, one function per widget family.
//!
//! Split out of `widgets.rs` so neither the file nor any single registration
//! function grows past the house limits. Registration order is the order
//! [`crate::widgets::install_ui_api`] calls these.

use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, CallbackId, Value};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::bridge::{scale, scoped, with_ctx, with_ui};
use crate::theme::{self, parse_hex};
use crate::vocabulary::{keys as k, words as w};
use crate::widgets::{
    Opts, code_editor, draw_image, panel_frame, pill_radius, sc, text, text_field,
};
use crate::{UiConfig, UiState};

/// `ui.*` bindings: theme.
pub(crate) fn install_theme(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "set_theme",
        &[],
        "", "Replace the theme: `name = \"#rrggbb\"` colour tokens, `dark = true|false`, and a `roles` table of named looks a widget takes with `role:`.",
    )]);
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
            config.theme.roles.clear();
            // Colours first: a role names them, so they have to resolve.
            let mut hex: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for (key, value) in entries {
                match (key.as_str(), value) {
                    ("dark", Value::Bool(dark)) => config.theme.dark = *dark,
                    (_, Value::Str(text)) => {
                        if let Some(color) = parse_hex(text) {
                            config.theme.colors.insert(key.clone(), color);
                            hex.insert(key.clone(), text.clone());
                        }
                    }
                    _ => {}
                }
            }
            for (key, value) in entries {
                let (Value::Map(roles), "roles") = (value, key.as_str()) else {
                    continue;
                };
                for (name, body) in roles {
                    let Value::Map(fields) = body else { continue };
                    let resolved = fields
                        .iter()
                        .map(|(k, v)| {
                            let v = match v {
                                Value::Str(text) => match hex.get(text) {
                                    Some(found) => Value::Str(found.clone()),
                                    None => v.clone(),
                                },
                                other => other.clone(),
                            };
                            (k.clone(), v)
                        })
                        .collect();
                    config.theme.roles.insert(name.clone(), resolved);
                }
            }
            config.changed = true;
            Ok(())
        });
    }
}

/// `ui.*` bindings: panels.
pub(crate) fn install_panels(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("top_panel", &[], "", "Dock a strip across the top of the window and draw the callback inside it; `height` is in design pixels. Answers the height it ended up with."),
        ("bottom_panel", &[], "", "Dock a strip across the bottom of the window and draw the callback inside it; `height` is in design pixels. Answers the height it ended up with."),
        ("left_panel", &[], "", "Dock a column down the left of the window and draw the callback inside it; `width` is in design pixels. Answers the width it ended up with."),
        ("right_panel", &[], "", "Dock a column down the right of the window and draw the callback inside it; `width` is in design pixels. Answers the width it ended up with."),
        ("central_panel", &[], "", "Draw the callback into whatever room the docked panels left over."),
        ("overlay", &[], "", "Draw the callback in a foreground area at `x`/`y` design pixels, above the panels and the widget layer. `w`/`h` fix its size, and `fill`, `stroke`, `radius` and padding make it a sheet."),
    ]);
    macro_rules! panel {
        ($name:literal, $ctor:ident, $size_key:expr, $span:ident) => {
            m.function(
                $name,
                |eng: &Engine, (id, opts, cb): (String, Option<Value>, CallbackId)| {
                    let opts = Opts::with_roles(opts);
                    with_ui(|parent| {
                        let mut result = Ok(());
                        let size = opts.px($size_key, 32.0);
                        let panel = egui::Panel::$ctor(egui::Id::new(id))
                            .frame(panel_frame(&opts))
                            .show_separator_line(opts.boolean(k::SEPARATOR, true));
                        // A resizable panel keeps its own size in egui's
                        // memory: the caller's is the starting one, `min`/`max`
                        // bound the drag, and the settled size is the answer.
                        let panel = if opts.boolean(k::RESIZABLE, false) {
                            panel
                                .resizable(true)
                                .default_size(size)
                                .min_size(opts.px(k::MIN, 80.0))
                                .max_size(opts.px(k::MAX, 4096.0))
                        } else {
                            panel.exact_size(size).resizable(false)
                        };
                        let shown = panel.show(parent, |ui| {
                            result = scoped(eng, ui, cb);
                        });
                        result.map(|()| f64::from(shown.response.rect.$span() / scale()))
                    })
                },
            );
        };
    }
    panel!("top_panel", top, k::HEIGHT, height);
    panel!("bottom_panel", bottom, k::HEIGHT, height);
    panel!("left_panel", left, k::WIDTH, width);
    panel!("right_panel", right, k::WIDTH, width);

    m.function(
        "central_panel",
        |eng: &Engine, (opts, cb): (Option<Value>, CallbackId)| {
            let opts = Opts::with_roles(opts);
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
    install_overlay(m);
}

/// `ui.overlay`: chrome above the widget layer. Panels share egui's background
/// layer, which every widget Area draws over, so overlays need one of their own.
fn install_overlay(m: &mut dyn Bindings<Engine>) {
    m.function(
        "overlay",
        |eng: &Engine, (id, opts, cb): (String, Option<Value>, CallbackId)| {
            let opts = Opts::with_roles(opts);
            with_ctx(|ctx| {
                let x = opts.px(k::X, 0.0);
                let y = opts.px(k::Y, 0.0);
                let w = opts.px(k::W, 0.0);
                let h = opts.px(k::H, 0.0);
                let mut result = Ok(());
                let pad_x = opts.px(k::PADDING_X, 0.0);
                let pad_y = opts.px(k::PADDING_Y, 0.0);
                let sized = w > 0.0 && h > 0.0;
                let area = egui::Area::new(egui::Id::new(id))
                    .order(egui::Order::Foreground)
                    .fixed_pos(pos2(x, y))
                    // A sized sheet sits where the layout put it. egui slides
                    // an area left to keep it on screen, so without this a row
                    // wider than the sheet moved the whole panel.
                    .constrain(!sized)
                    // Layout, not a popup: a sheet that replaces another, as
                    // a dock collapsing to its rail does, must not fade in.
                    .fade_in(false);
                area.show(ctx, |ui| {
                    // Clipped as well as capped: egui grows a container to
                    // its content, and one long row would stretch the fill.
                    if sized {
                        ui.set_max_size(vec2(w, h));
                        ui.shrink_clip_rect(egui::Rect::from_min_size(pos2(x, y), vec2(w, h)));
                    }
                    let mut frame = egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(pad_x as i8, pad_y as i8))
                        .corner_radius(pill_radius(opts.px(k::RADIUS, 0.0) * 2.0));
                    if let Some(fill) = opts.opt_color(k::FILL) {
                        frame = frame.fill(fill);
                    }
                    if let Some(stroke) = opts.opt_color(k::STROKE) {
                        frame = frame.stroke(Stroke::new(1.0, stroke));
                    }
                    frame.show(ui, |ui| {
                        // Padding comes out of the caller's size, not on
                        // top of it; the clip stops taller content
                        // spilling over the sheet below.
                        if sized {
                            let inner = vec2(w - 2.0 * pad_x, h - 2.0 * pad_y);
                            ui.set_min_size(inner);
                            ui.set_max_size(inner);
                            let at = ui.min_rect().min;
                            ui.shrink_clip_rect(egui::Rect::from_min_size(at, inner));
                        }
                        result = scoped(eng, ui, cb);
                    });
                });
                result
            })
        },
    );
}

/// `ui.*` bindings: containers.
pub(crate) fn install_containers(m: &mut dyn Bindings<Engine>) {
    crate::widget_layout::install_layout_containers(m);
    crate::widget_layout::install_spacing_helpers(m);
}

/// `ui.*` bindings: text.
pub(crate) fn install_text(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        w::LABEL,
        &[],
        "",
        "Draw a line of text; `size`, `font`, `color`, `strong`, `wrap` and `truncate` style it.",
    )]);
    m.function(
        "label",
        |_eng: &Engine, (s, opts): (String, Option<Value>)| {
            let opts = Opts::with_roles(opts);
            with_ui(|ui| {
                let fam = opts.string(k::FONT).unwrap_or_else(|| w::UI.into());
                let rt = text(
                    &s,
                    opts.px(k::SIZE, 12.0),
                    &fam,
                    opts.opt_color(k::COLOR),
                    opts.boolean(k::STRONG, false),
                );
                let mut label = egui::Label::new(rt).selectable(false);
                if !opts.boolean(k::WRAP, false) {
                    // `truncate`: an ellipsis at the container's edge, for
                    // text the layout cannot size to (a node name, a path, a
                    // joined list). Otherwise the text keeps its full width.
                    label = if opts.boolean(k::TRUNCATE, false) {
                        label.truncate()
                    } else {
                        label.extend()
                    };
                }
                ui.add(label);
                Ok(())
            })
        },
    );
}

/// `ui.*` bindings: buttons.
pub(crate) fn install_buttons(m: &mut dyn Bindings<Engine>) {
    crate::widget_layout::install_button_widgets(m);
    crate::widget_layout::install_button_shapes(m);
}

/// `ui.*` bindings: controls.
pub(crate) fn install_controls(m: &mut dyn Bindings<Engine>) {
    install_toggle_and_slider(m);
    install_drag_value(m);
}

/// `ui.*` bindings: text input.
pub(crate) fn install_text_input(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("text_field", &[], "", "Draw a single-line text box keyed by `id`; returns its text, whether it changed, and whether Enter was pressed."),
        ("set_text", &[], "", "Overwrite what the field with this `id` is editing, leaving the seed its `value` option last wrote alone."),
    ]);
    {
        m.function(
            "text_field",
            |eng: &Engine, (id, placeholder, opts): (String, Option<String>, Option<Value>)| {
                let opts = Opts::with_roles(opts);
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
    m.describe(&[(
        "code_line",
        &[],
        "", "Draw one read-only code row from a list of `{ text, color, strong }` spans, with a gutter label on the left.",
    )]);
    m.function(
        "code_line",
        |_eng: &Engine, (gutter, spans, opts): (String, Value, Option<Value>)| {
            let opts = Opts::with_roles(opts);
            with_ui(|ui| {
                let size = opts.px(k::SIZE, 12.5);
                let row_h = size * opts.f32(k::LINE_HEIGHT, 1.78);
                let gutter_w = opts.px(k::GUTTER_WIDTH, 24.0);
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
                if let Some(fill) = opts.opt_color(k::HIGHLIGHT) {
                    ui.painter().rect_filled(rect, 0.0, fill);
                }
                let mono = FontId::new(size, theme::family(w::MONO));
                let gutter_color = opts.color(k::GUTTER_COLOR, Color32::from_rgb(0x76, 0x7e, 0x88));
                ui.painter().text(
                    pos2(rect.min.x + gutter_w, rect.center().y),
                    Align2::RIGHT_CENTER,
                    gutter,
                    FontId::new((size - 1.5).max(8.0), theme::family(w::MONO)),
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
                    let text = match field(k::TEXT) {
                        Some(Value::Str(s)) => s.clone(),
                        _ => String::new(),
                    };
                    let color = match field(k::COLOR) {
                        Some(Value::Str(c)) => parse_hex(c).unwrap_or(Color32::WHITE),
                        _ => Color32::WHITE,
                    };
                    let mut format = egui::TextFormat::simple(mono.clone(), color);
                    if matches!(field(k::STRONG), Some(Value::Bool(true))) {
                        format.font_id = FontId::new(size, theme::family(w::MONO));
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
// A floating window: the surface a plugin gets when it asks for one. Unlike
// `overlay` it is dragged and resized by the user, and egui remembers where
// each `id` was left.
pub(crate) fn install_window(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "window",
        &[],
        "", "Draw the callback in a floating window the user drags and resizes; false once its close button is used.",
    )]);
    m.function(
        "window",
        |eng: &Engine, (id, opts, cb): (String, Option<Value>, CallbackId)| {
            let opts = Opts::with_roles(opts);
            with_ctx(|ctx| {
                let mut open = true;
                let mut window =
                    egui::Window::new(opts.string(k::TITLE).unwrap_or_else(|| id.clone()))
                        .id(egui::Id::new(&id))
                        .resizable(opts.boolean(k::RESIZABLE, true))
                        .collapsible(opts.boolean(k::COLLAPSIBLE, false));
                if opts.boolean(k::CLOSABLE, true) {
                    window = window.open(&mut open);
                }
                if let (Some(x), Some(y)) = (opts.opt_px(k::X), opts.opt_px(k::Y)) {
                    window = window.default_pos(pos2(x, y));
                }
                if let Some(width) = opts.opt_px(k::WIDTH) {
                    window = window.default_width(width);
                }
                if let Some(height) = opts.opt_px(k::HEIGHT) {
                    window = window.default_height(height);
                }
                let mut result = Ok(());
                window.show(ctx, |ui| {
                    result = scoped(eng, ui, cb);
                });
                result?;
                Ok(open)
            })
        },
    );
}

pub(crate) fn install_modal(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "modal",
        &[],
        "", "Draw the callback in a centered dialog over a dimming scrim; true on the frame the scrim was clicked. `width`, `height` and `top` size and place it, `fill`, `stroke` and `scrim` colour it; height follows the content when it is not given.",
    )]);
    m.function(
        "modal",
        |eng: &Engine, (id, opts, cb): (String, Option<Value>, CallbackId)| {
            let opts = Opts::with_roles(opts);
            with_ctx(|ctx| {
                let screen = ctx.viewport_rect();
                let scrim = opts.color(k::SCRIM, Color32::from_rgba_unmultiplied(23, 25, 28, 107));
                let mut scrim_clicked = false;
                // Above `Foreground`, which is where every overlay and dock
                // sheet already sits: a modal that shares their order is on
                // top only when egui's memory happens to say so.
                egui::Area::new(egui::Id::new(format!("{id}-scrim")))
                    .order(egui::Order::Tooltip)
                    .fixed_pos(screen.min)
                    // The scrim and its sheet appear together, so a fade on
                    // either shows the other through a half-lit screen.
                    .fade_in(false)
                    .show(ctx, |ui| {
                        ui.painter().rect_filled(screen, 0.0, scrim);
                        let response = ui.allocate_rect(screen, Sense::click());
                        scrim_clicked = response.clicked();
                    });
                let width = opts.px(k::WIDTH, 560.0).min(screen.width() * 0.92);
                let top = screen.height() * opts.f32(k::TOP, 0.11);
                let mut result = Ok(());
                egui::Area::new(egui::Id::new(id))
                    .order(egui::Order::Tooltip)
                    .fixed_pos(pos2(screen.center().x - width / 2.0, top))
                    .fade_in(false)
                    .show(ctx, |ui| {
                        let mut frame = egui::Frame::new()
                            .corner_radius(pill_radius(sc(32.0)))
                            .fill(opts.color(k::FILL, Color32::from_rgb(0x20, 0x24, 0x2a)));
                        if let Some(stroke) = opts.opt_color(k::STROKE) {
                            frame = frame.stroke(Stroke::new(1.0, stroke));
                        }
                        frame.show(ui, |ui| {
                            ui.set_width(width);
                            // `height` pins a sheet that would otherwise
                            // shrink as its page empties; the screen is the
                            // ceiling.
                            let height = opts.px(k::HEIGHT, 0.0);
                            if height > 0.0 {
                                ui.set_min_height(height.min(screen.height() - top - sc(32.0)));
                            }
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
    m.describe(&[(
        "set_widget_layer",
        &[],
        "", "Turn drawing of the scene's `widget` nodes on or off, and confine it to an x/y/w/h rect in design pixels.",
    ),
    (
        "set_widget_surface",
        &[],
        "",
        "The same for one named surface: roots whose `layer` is this name draw here instead. A name nothing has set takes the default surface.",
    ),
    (
        "widget_rect",
        &[],
        "",
        "Where a `widget` node was last drawn, as `#{ x, y, w, h }` in design pixels; empty until it has drawn once.",
    ),
    (
        "set_keyboard_focus",
        &[],
        "",
        "Let the arrows, Tab, Enter and Space move and activate the focused widget. Off unless asked for, so a game that moves with the arrows does not click its own HUD; `standard_app` turns it on for a project declaring the `ui_*` actions.",
    )]);
    {
        m.function("set_keyboard_focus", |eng: &Engine, on: bool| {
            eng.resource::<crate::WidgetLayerConfig>()
                .borrow_mut()
                .keyboard = on;
            Ok(())
        });
    }
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
                    layer.rect = rect_of(x, y, w, h);
                    Ok(())
                },
            );
        m.function(
            "set_widget_surface",
            |eng: &Engine,
             (name, enabled, x, y, w, h): (
                String,
                bool,
                Option<f32>,
                Option<f32>,
                Option<f32>,
                Option<f32>,
            )| {
                let layer = eng.resource::<crate::WidgetLayerConfig>();
                layer.borrow_mut().layers.insert(
                    name,
                    crate::widget_layer::Surface {
                        enabled,
                        rect: rect_of(x, y, w, h),
                    },
                );
                Ok(())
            },
        );
        m.function(
            "widget_rect",
            |_eng: &Engine, node: balaur_script::NodeId| {
                let scale = scale();
                Ok(
                    crate::widget_layer::drawn_at(balaur_core::entity_of(node)?).map_or(
                        Value::Nil,
                        |r| {
                            Value::Map(vec![
                                (k::X.into(), Value::Num(f64::from(r.min.x / scale))),
                                (k::Y.into(), Value::Num(f64::from(r.min.y / scale))),
                                (k::W.into(), Value::Num(f64::from(r.width() / scale))),
                                (k::H.into(), Value::Num(f64::from(r.height() / scale))),
                            ])
                        },
                    ),
                )
            },
        );
    }
    install_focus(m);
}

/// Four numbers or none: a partial rect is a caller that meant the screen.
fn rect_of(x: Option<f32>, y: Option<f32>, w: Option<f32>, h: Option<f32>) -> Option<[f32; 4]> {
    match (x, y, w, h) {
        (Some(x), Some(y), Some(w), Some(h)) => Some([x, y, w, h]),
        _ => None,
    }
}

/// `ui.*` bindings: which widget the keyboard and the pad are pointing at.
///
/// The keyboard drives focus on its own, through egui. These exist for the
/// pad: a game maps its `ui_next` / `ui_previous` / `ui_accept` actions onto
/// them, and `standard_app` does it for a project that declares those names.
fn install_focus(m: &mut dyn Bindings<Engine>) {
    use crate::widget_layer::Move;
    m.describe(&[
        ("focused", &[], "()", "The widget node focus is on, or nil."),
        (
            "set_focus",
            &[],
            "(node: node)",
            "Put focus on a widget node. A node focus cannot activate is refused at the next draw.",
        ),
        (
            "focus_next",
            &[],
            "()",
            "Move focus to the next widget in scene order, wrapping past the last.",
        ),
        (
            "focus_previous",
            &[],
            "()",
            "Move focus to the previous widget in scene order, wrapping past the first.",
        ),
        (
            "activate_focused",
            &[],
            "()",
            "Activate the focused widget, exactly as a click on it would.",
        ),
    ]);
    m.function("focused", |eng: &Engine, ()| {
        let focus = eng.resource::<crate::UiFocus>();
        let focused = focus.borrow().focused;
        Ok(focused.map_or(balaur_script::Value::Nil, |e| {
            balaur_script::Value::Node(balaur_core::node_id_of(e).0)
        }))
    });
    m.function("set_focus", |eng: &Engine, node: balaur_script::NodeId| {
        let entity = balaur_core::entity_of(node)?;
        eng.resource::<crate::UiFocus>().borrow_mut().focused = Some(entity);
        Ok(())
    });
    for (name, asked) in [
        ("focus_next", Move::Next),
        ("focus_previous", Move::Previous),
        ("activate_focused", Move::Accept),
    ] {
        m.function(name, move |eng: &Engine, ()| {
            eng.resource::<crate::UiFocus>().borrow_mut().pending = Some(asked);
            Ok(())
        });
    }
}

/// `ui.*` bindings: scale.
pub(crate) fn install_scale(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "scale",
            &[],
            "",
            "The global UI scale: real pixels per design pixel.",
        ),
        (
            "set_scale",
            &[],
            "",
            "Set the global UI scale, clamped to between 0.25 and 3.0 real pixels per design pixel; a design resolution is `screen_size` divided by it.",
        ),
    ]);
    m.function("scale", |eng: &Engine, ()| {
        let config = eng.resource::<UiConfig>();
        let v = config.borrow().scale;
        Ok(v)
    });
    m.function("set_scale", |eng: &Engine, f: f32| {
        let config = eng.resource::<UiConfig>();
        config.borrow_mut().scale = f.clamp(0.25, 3.0);
        Ok(())
    });
}

/// `ui.*` bindings: code editor.
pub(crate) fn install_code_editor(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "code_editor",
        &[],
        "", "Draw an editable, highlighted buffer with a gutter; returns the text, whether it changed, and any line clicked.",
    )]);
    {
        m.function(
            "code_editor",
            |eng: &Engine, (id, source, opts): (String, String, Option<Value>)| {
                let opts = Opts::with_roles(opts);
                code_editor(eng, &id, &source, &opts)
            },
        );
    }
}

/// `ui.*` bindings: drop-down select.
pub(crate) fn install_dropdown_select(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        w::DROPDOWN,
        &[],
        "", "Draw a pill-shaped select over a list of strings; returns the selection and whether it changed this frame.",
    )]);
    m.function(
        "dropdown",
        |_eng: &Engine, (id, current, options, opts): (String, String, Value, Option<Value>)| {
            let opts = Opts::with_roles(opts);
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
                let w = opts.px(k::WIDTH, 160.0);
                let size = opts.px(k::SIZE, 12.0);
                let text_color = opts.color(k::COLOR, Color32::WHITE);
                let mut selected = current.clone();
                ui.scope(|ui| {
                    // Pill-shaped shell and menu items for this widget only.
                    let radius = pill_radius(sc(5.0) * 2.0);
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
                                .family(theme::family(w::UI))
                                .color(text_color),
                        )
                        .show_ui(ui, |ui| {
                            for item in &items {
                                ui.selectable_value(
                                    &mut selected,
                                    item.clone(),
                                    egui::RichText::new(item)
                                        .size(size)
                                        .family(theme::family(w::UI)),
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
    m.describe(&[
        ("image", &[], "", "Draw a PNG from the project, sized by `width`/`height` in design pixels and cached by path."),
        ("rect_stroke", &[], "", "Outline a rectangle at x/y/w/h design pixels from the current panel's corner, `dashed` when asked."),
        ("cursor_y", &[], "", "How far down the current panel the next widget will land, in design pixels — the same origin `rect_stroke` measures from."),
    ]);
    {
        m.function(
            "image",
            |eng: &Engine, (path, opts): (String, Option<Value>)| {
                let opts = Opts::with_roles(opts);
                draw_image(eng, &path, &opts)
            },
        );
    }
    // Where the next widget lands, in `rect_stroke`'s coordinates, so a rule
    // can be drawn down through rows that have not been laid out yet.
    m.function("cursor_y", |_eng: &Engine, (): ()| {
        with_ui(|ui| {
            let top = ui.max_rect().min.y;
            Ok(f64::from((ui.cursor().min.y - top) / scale()))
        })
    });
    // An outline rectangle painted at (x, y, w, h) in design px relative to
    // the current panel (viewport overlays: safe area, guides).
    m.function(
        "rect_stroke",
        |_eng: &Engine, (x, y, w, h, opts): (f32, f32, f32, f32, Option<Value>)| {
            let opts = Opts::with_roles(opts);
            with_ui(|ui| {
                let origin = ui.max_rect().min;
                let rect = Rect::from_min_size(
                    pos2(origin.x + sc(x), origin.y + sc(y)),
                    vec2(sc(w), sc(h)),
                );
                let stroke = Stroke::new(
                    opts.f32(k::WIDTH, 1.5),
                    opts.color(k::COLOR, Color32::from_rgb(0xd5, 0x81, 0x4e)),
                );
                if opts.boolean(k::DASHED, false) {
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
    m.describe(&[
        ("available_width", &[], "", "The width left in the current container, in design pixels."),
        ("available_height", &[], "", "The height left in the current container, in design pixels."),
        ("central_rect", &[], "", "The x, y, width and height of the surface being drawn into, in design pixels."),
        ("screen_size", &[], "", "The window's width and height, in design pixels."),
        ("shortcut", &[], "", "Whether this chord was pressed this frame, consuming it; `mods` is `\"cmd+shift\"`, from the `MOD_*` constants."),
        ("set_clipboard", &[], "", "Copy text to the system clipboard."),
        ("clipboard", &[], "", "The text pasted this frame, empty otherwise: the platform clipboard is not readable on demand."),
        ("color", &[], "", "Draw a colour picker over `value`, an `[r, g, b, a]` of unit floats; returns the colour and whether it changed."),
        ("wants_keyboard", &[], "", "Whether a UI widget holds keyboard focus, so the game should leave this frame's key presses alone."),
    ]);
    // Queries return design pixels (real points divided by the UI scale), so
    // scripts compute layout in one consistent unit.
    m.function("available_width", |_eng: &Engine, ()| {
        with_ui(|ui| Ok(ui.available_width() / scale()))
    });
    m.function("available_height", |_eng: &Engine, ()| {
        with_ui(|ui| Ok(ui.available_height() / scale()))
    });
    // The rect of the surface being drawn into, in design px. Called inside
    // `central_panel` it is the hole the panels left, which the alternative
    // re-adds from every panel's size by hand.
    m.function("central_rect", |_eng: &Engine, ()| {
        with_ui(|ui| {
            let rect = ui.max_rect();
            let scale = scale();
            Ok((
                rect.min.x / scale,
                rect.min.y / scale,
                rect.width() / scale,
                rect.height() / scale,
            ))
        })
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
                // "shift+cmd" is one chord, not an unknown name: an unrecognised
                // string used to silently become the unmodified key.
                let mut modifiers = egui::Modifiers::NONE;
                for part in mods.split('+') {
                    modifiers |= match part.trim() {
                        w::CMD => egui::Modifiers::COMMAND,
                        w::CTRL => egui::Modifiers::CTRL,
                        w::ALT => egui::Modifiers::ALT,
                        w::SHIFT => egui::Modifiers::SHIFT,
                        _ => egui::Modifiers::NONE,
                    };
                }
                Ok(ctx.input_mut(|i| i.consume_key(modifiers, key)))
            })
        },
    );
    install_clipboard_and_color(m);
}

/// The clipboard pair and the colour picker: queries that answer about the
/// frame rather than about the layout.
fn install_clipboard_and_color(m: &mut dyn Bindings<Engine>) {
    // Copy and paste. egui hands a paste to the frame it arrived on, so
    // `clipboard` answers the pasted text on that frame and "" otherwise;
    // the platform's clipboard is not readable on demand.
    m.function("set_clipboard", |_eng: &Engine, text: String| {
        with_ctx(|ctx| {
            ctx.copy_text(text);
            Ok(())
        })
    });
    m.function("clipboard", |_eng: &Engine, ()| {
        with_ctx(|ctx| {
            Ok(ctx.input(|i| {
                i.events
                    .iter()
                    .rev()
                    .find_map(|e| match e {
                        egui::Event::Paste(text) => Some(text.clone()),
                        _ => None,
                    })
                    .unwrap_or_default()
            }))
        })
    });
    // A colour as `[r, g, b, a]` in unit floats, which is how a schema's
    // `color` property is stored. Four drag values were the alternative.
    m.function("color", |_eng: &Engine, value: Option<Value>| {
        let opts = Opts(value);
        let start = opts.unit_rgba(k::VALUE);
        with_ui(|ui| {
            let mut rgba =
                egui::Rgba::from_rgba_unmultiplied(start[0], start[1], start[2], start[3]);
            let response = egui::color_picker::color_edit_button_rgba(
                ui,
                &mut rgba,
                egui::color_picker::Alpha::OnlyBlend,
            );
            let out = rgba.to_rgba_unmultiplied();
            Ok((
                vec![
                    f64::from(out[0]),
                    f64::from(out[1]),
                    f64::from(out[2]),
                    f64::from(out[3]),
                ],
                response.changed(),
            ))
        })
    });
    m.function("wants_keyboard", |_eng: &Engine, ()| {
        with_ctx(|ctx| Ok(ctx.egui_wants_keyboard_input()))
    });
}

/// The two controls that return their edited value plus whether this frame
/// changed it, so a script can write back only on a real edit.
fn install_toggle_and_slider(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("toggle", &[], "", "Draw an on/off switch; returns the state after this frame and whether it was clicked."),
        ("slider", &[], "", "Draw a horizontal slider between `min` and `max`; returns the value after this frame and whether it moved."),
    ]);
    m.function(
        "toggle",
        |_eng: &Engine, (on, opts): (bool, Option<Value>)| {
            let opts = Opts::with_roles(opts);
            with_ui(|ui| {
                // Sized from the theme: a switch as tall as its row, not a
                // slab that dwarfs the fields beside it.
                let h = opts.px(k::HEIGHT, sc(18.0) / scale());
                let (rect, response) = ui.allocate_exact_size(vec2(h * 1.75, h), Sense::click());
                let on = if response.clicked() { !on } else { on };
                let track = if on {
                    opts.color(k::ON_FILL, Color32::from_rgb(0xd5, 0x81, 0x4e))
                } else {
                    opts.color(k::OFF_FILL, Color32::from_rgb(0x10, 0x12, 0x15))
                };
                let knob = if on {
                    opts.color(k::ON_KNOB, Color32::from_rgb(0xf9, 0xf4, 0xed))
                } else {
                    opts.color(k::OFF_KNOB, Color32::from_rgb(0x76, 0x7e, 0x88))
                };
                ui.painter().rect_filled(rect, h / 2.0, track);
                let inset = h / 2.0;
                let x = if on {
                    rect.max.x - inset
                } else {
                    rect.min.x + inset
                };
                ui.painter()
                    .circle_filled(pos2(x, rect.center().y), h * 0.34, knob);
                Ok((on, response.clicked()))
            })
        },
    );
    m.function(
        "slider",
        |_eng: &Engine, (value, min, max, opts): (f32, f32, f32, Option<Value>)| {
            let opts = Opts::with_roles(opts);
            with_ui(|ui| {
                let w = {
                    let w = opts.px(k::WIDTH, 0.0);
                    if w > 0.0 {
                        w
                    } else {
                        ui.available_width().max(24.0)
                    }
                };
                let (rect, response) =
                    ui.allocate_exact_size(vec2(w, sc(28.0)), Sense::click_and_drag());
                let mut value = value;
                if (response.dragged() || response.clicked())
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let t = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                    value = min + t * (max - min);
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
                    opts.color(k::RAIL, Color32::from_rgb(0x10, 0x12, 0x15)),
                );
                let fill_rect = Rect::from_min_max(
                    rail.min,
                    pos2(rail.width().mul_add(t, rail.min.x), rail.max.y),
                );
                let accent = opts.color(k::FILL, Color32::from_rgb(0xd5, 0x81, 0x4e));
                ui.painter().rect_filled(fill_rect, 2.5, accent);
                let knob_x = rect.width().mul_add(t, rect.min.x);
                ui.painter().circle_filled(
                    pos2(knob_x, rect.center().y),
                    sc(6.5),
                    opts.color(k::KNOB, Color32::from_rgb(0x17, 0x19, 0x1c)),
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
    m.describe(&[(
        "drag_value",
        &[],
        "", "Draw a number dragged sideways to change it; returns the value and whether this frame changed it.",
    )]);
    m.function(
        "drag_value",
        |_eng: &Engine, (value, opts): (f64, Option<Value>)| {
            let opts = Opts::with_roles(opts);
            with_ui(|ui| {
                let h = opts.px(k::HEIGHT, 28.0);
                let w = {
                    let w = opts.px(k::WIDTH, 0.0);
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
                        f64::from(response.drag_delta().x) * f64::from(opts.f32(k::SPEED, 0.05));
                    if delta != 0.0 {
                        value += delta;
                        changed = true;
                    }
                }
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                let radius = opts.px(k::RADIUS, 0.0);
                let corner = if radius > 0.0 {
                    pill_radius(radius * 2.0)
                } else {
                    pill_radius(sc(5.0) * 2.0)
                };
                ui.painter().rect(
                    rect,
                    corner,
                    opts.color(k::FILL, Color32::from_rgb(0x10, 0x12, 0x15)),
                    Stroke::new(
                        1.0,
                        opts.color(k::STROKE, Color32::from_rgb(0x2b, 0x30, 0x37)),
                    ),
                    StrokeKind::Inside,
                );
                let mut x = rect.min.x + sc(7.0);
                if let Some(prefix) = opts.string(k::PREFIX) {
                    let color = opts.color(k::PREFIX_COLOR, Color32::from_rgb(0xf0, 0xa2, 0x73));
                    let galley = ui.painter().layout_no_wrap(
                        prefix,
                        FontId::new(sc(10.0), theme::family(w::HEADING)),
                        color,
                    );
                    let y = rect.center().y - galley.size().y / 2.0;
                    ui.painter().galley(pos2(x, y), galley, color);
                    x += sc(14.0);
                }
                let decimals = opts.f32(k::DECIMALS, 1.0) as usize;
                let display = opts.string(k::SUFFIX).map_or_else(
                    || format!("{value:.decimals$}"),
                    |s| format!("{value:.decimals$}{s}"),
                );
                let color = opts.color(k::COLOR, Color32::from_rgb(0xee, 0xf1, 0xf4));
                let galley = ui.painter().layout_no_wrap(
                    display,
                    FontId::new(sc(11.5), theme::family(w::MONO)),
                    color,
                );
                let y = rect.center().y - galley.size().y / 2.0;
                ui.painter().galley(pos2(x, y), galley, color);
                Ok((value, changed))
            })
        },
    );
}
