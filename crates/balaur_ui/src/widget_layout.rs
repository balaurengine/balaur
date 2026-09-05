//! `ui.*` script bindings for layout: the containers that arrange other
//! widgets, the spacing helpers between them, and the buttons.
//!
//! Split out of [`crate::widget_bindings`] so neither file grows past the
//! house limits; `install_containers` and `install_buttons` there are the
//! only callers.

use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, CallbackId, Value};
use egui::{Align, Color32, FontId, Layout, Margin, Sense, Stroke, pos2, vec2};

use crate::bridge::{scoped, with_ui};
use crate::theme::{self, parse_hex};
use crate::vocabulary::{keys as k, words as w};
use crate::widgets::{Opts, left_pill, pill_radius, sc, text};

/// `ui.horizontal`, `ui.vertical`, `ui.right` and `ui.frame`.
pub(crate) fn install_layout_containers(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("horizontal", &[], "", "Lay the callback's widgets out in a row; `width`, `height` and `tight` size it, in design pixels."),
        ("vertical", &[], "", "Lay the callback's widgets out in a column."),
        ("right", &[], "", "Lay the callback's widgets out against the right edge, still declared left to right."),
        ("frame", &[], "", "Wrap the callback in a box with optional `fill`, `stroke`, `radius` and padding, in design pixels."),
    ]);
    m.function(
        "horizontal",
        |eng: &Engine, (opts, cb): (Option<Value>, CallbackId)| {
            let opts = Opts::with_roles(opts);
            with_ui(|ui| {
                let mut result = Ok(());
                let layout = Layout::left_to_right(Align::Center);
                let height = opts.px(k::HEIGHT, 0.0);
                // tight: hug the content instead of claiming the full row;
                // width: exact width (needed inside right-to-left layouts,
                // where growing children would overlap neighbors).
                let fixed = opts.px(k::WIDTH, 0.0);
                let width = if fixed > 0.0 {
                    fixed
                } else if opts.boolean(k::TIGHT, false) {
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
            let opts = Opts::with_roles(opts);
            with_ui(|ui| {
                let mut result = Ok(());
                let mut frame = egui::Frame::new()
                    .inner_margin(Margin::symmetric(
                        opts.px(k::PADDING_X, 0.0) as i8,
                        opts.px(k::PADDING_Y, 0.0) as i8,
                    ))
                    .corner_radius(pill_radius(opts.px(k::RADIUS, 0.0) * 2.0));
                if let Some(fill) = opts.opt_color(k::FILL) {
                    frame = frame.fill(fill);
                }
                if let Some(stroke) = opts.opt_color(k::STROKE) {
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
pub(crate) fn install_spacing_helpers(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("scroll", &[], "", "Put the callback in a vertical scroll area; `max_height` caps it and `stick_to_bottom` follows new content."),
        ("add_space", &[], "", "Insert blank space along the current layout, in design pixels."),
        ("separator", &[], "", "Draw a one-pixel rule across the container, in the given `#rrggbb` colour when one is passed."),
        ("spacing", &[], "", "Set the gap between the current container's widgets, in design pixels."),
    ]);
    m.function(
        "scroll",
        |eng: &Engine, (id, opts, cb): (String, Option<Value>, CallbackId)| {
            let opts = Opts::with_roles(opts);
            with_ui(|ui| {
                let mut result = Ok(());
                let mut area = egui::ScrollArea::vertical()
                    .id_salt(id)
                    .auto_shrink([false, false]);
                let max_h = opts.px(k::MAX_HEIGHT, 0.0);
                if max_h > 0.0 {
                    area = area.max_height(max_h);
                }
                // A log follows what is arriving unless the reader has scrolled
                // away from the end, which egui tracks for us.
                if opts.boolean(k::STICK_TO_BOTTOM, false) {
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

/// Add a button, greyed out and inert when the caller said `disabled`.
fn enabled_add(ui: &mut egui::Ui, button: egui::Button<'_>, opts: &Opts) -> egui::Response {
    ui.add_enabled(!opts.boolean(k::DISABLED, false), button)
}

/// A tooltip that still shows on a disabled button, which is where it
/// explains why the button is off.
fn hover_text(response: egui::Response, opts: &Opts, tip: String) -> egui::Response {
    if opts.boolean(k::DISABLED, false) {
        response.on_disabled_hover_text(tip)
    } else {
        response.on_hover_text(tip)
    }
}

/// `ui.pill` and `ui.menu_item`.
pub(crate) fn install_button_widgets(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("pill", &[], "", "Draw a rounded button, or a left-aligned row when `align = \"left\"`; true on the frame it was clicked. `disabled` greys it out and swallows the click."),
        ("menu_item", &[], "", "Draw a row inside a context menu; true on the frame it was clicked, which also closes the menu."),
    ]);
    m.function(
        "pill",
        |eng: &Engine, (s, opts): (String, Option<Value>)| {
            let opts = Opts::with_roles(opts);
            with_ui(|ui| {
                if opts.string(k::ALIGN).as_deref() == Some(w::LEFT) {
                    return left_pill(eng, ui, &s, &opts);
                }
                let h = opts.px(k::HEIGHT, 27.0);
                let fam = opts.string(k::FONT).unwrap_or_else(|| "ui".into());
                let mut display = String::new();
                if let Some(icon) = opts.string(k::ICON) {
                    display.push_str(&icon);
                    if !s.is_empty() {
                        display.push_str("  ");
                    }
                }
                display.push_str(&s);
                let rt = text(
                    &display,
                    opts.px(k::SIZE, 12.0),
                    &fam,
                    opts.opt_color(k::COLOR),
                    opts.boolean(k::STRONG, false),
                );
                let fill = opts.color(k::FILL, Color32::TRANSPARENT);
                // Tiles by default: a shell of capsules reads as loose and
                // never lines up. `round` is for the few controls that are
                // genuinely circular.
                let radius = opts.px(k::RADIUS, 0.0);
                let corner = if radius > 0.0 {
                    pill_radius(radius * 2.0)
                } else if opts.boolean(k::ROUND, false) {
                    pill_radius(h)
                } else {
                    pill_radius(sc(5.0) * 2.0)
                };
                let mut button = egui::Button::new(rt)
                    .fill(fill)
                    .corner_radius(corner)
                    .min_size(vec2(opts.px(k::MIN_WIDTH, 0.0), h));
                button = match opts.opt_color(k::STROKE) {
                    Some(color) => button.stroke(Stroke::new(1.0, color)),
                    None => button.stroke(Stroke::NONE),
                };
                let mut response = enabled_add(ui, button, &opts);
                if let Some(tip) = opts.string(k::TOOLTIP) {
                    response = hover_text(response, &opts, tip);
                }
                // Right-click context menu: the callback draws menu items.
                if let Some(menu) = opts.callback(k::MENU) {
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
            let opts = Opts::with_roles(opts);
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
                let color = opts.color(k::COLOR, Color32::WHITE);
                let galley = ui.painter().layout_no_wrap(
                    s.clone(),
                    FontId::new(opts.px(k::SIZE, 12.5), theme::family("ui")),
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
pub(crate) fn install_button_shapes(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("circle_button", &[], "", "Draw a round button holding one glyph, `d` design pixels across; true on the frame it was clicked. `disabled` greys it out and swallows the click."),
        ("dot", &[], "", "Draw a filled circle in a `#rrggbb` colour, `d` design pixels across."),
    ]);
    m.function(
        "circle_button",
        |_eng: &Engine, (glyph, opts): (String, Option<Value>)| {
            let opts = Opts::with_roles(opts);
            with_ui(|ui| {
                let d = opts.px(k::D, 32.0);
                let rt = text(
                    &glyph,
                    opts.px(k::SIZE, 14.0),
                    &opts.string(k::FONT).unwrap_or_else(|| "ui".into()),
                    opts.opt_color(k::COLOR),
                    opts.boolean(k::STRONG, false),
                );
                let mut button = egui::Button::new(rt)
                    .fill(opts.color(k::FILL, Color32::TRANSPARENT))
                    .corner_radius(pill_radius(d))
                    .min_size(vec2(d, d));
                button = match opts.opt_color(k::STROKE) {
                    Some(color) => button.stroke(Stroke::new(1.0, color)),
                    None => button.stroke(Stroke::NONE),
                };
                let mut response = enabled_add(ui, button, &opts);
                if let Some(tip) = opts.string(k::TOOLTIP) {
                    response = hover_text(response, &opts, tip);
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
