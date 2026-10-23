//! The `ui` script module: what a script asks of the UI and tells it. Where
//! a widget node drew, focus, the screen's classes, the scale, the theme, the
//! clipboard, pacing and the pooled rows. Nothing here draws: a UI is
//! `widget` nodes in a scene, and [`crate::widget`] draws them.

use anyhow::Result;
use balaur_core::Engine;
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, Value};

use crate::bridge::with_ctx;
use crate::theme;
use crate::vocabulary::words as w;
use crate::{UiConfig, UiState};

/// Screen anchors for the `widget` scene component, so a script says
/// `ui.ANCHOR_TOP_LEFT` rather than spelling the string.
pub const ANCHORS: &[(&str, &str)] = &[
    ("ANCHOR_TOP_LEFT", w::TOP_LEFT),
    ("ANCHOR_TOP_RIGHT", w::TOP_RIGHT),
    ("ANCHOR_BOTTOM_LEFT", w::BOTTOM_LEFT),
    ("ANCHOR_BOTTOM_RIGHT", w::BOTTOM_RIGHT),
    ("ANCHOR_CENTER", w::CENTER),
    ("ANCHOR_CENTER_LEFT", w::CENTER_LEFT),
    ("ANCHOR_CENTER_RIGHT", w::CENTER_RIGHT),
    ("ANCHOR_CENTER_TOP", w::CENTER_TOP),
    ("ANCHOR_CENTER_BOTTOM", w::CENTER_BOTTOM),
    ("ANCHOR_FILL", w::FILL),
    ("ANCHOR_FILL_TOP", w::FILL_TOP),
    ("ANCHOR_FILL_BOTTOM", w::FILL_BOTTOM),
    ("ANCHOR_FILL_LEFT", w::FILL_LEFT),
    ("ANCHOR_FILL_RIGHT", w::FILL_RIGHT),
    ("ANCHOR_FILL_ACROSS", w::FILL_ACROSS),
    ("ANCHOR_FILL_DOWN", w::FILL_DOWN),
];

/// Widget kinds the layer draws.
pub const WIDGET_KINDS: &[(&str, &str)] = &[
    ("WIDGET_LABEL", w::LABEL),
    ("WIDGET_BUTTON", w::BUTTON),
    ("WIDGET_PANEL", w::PANEL),
    ("WIDGET_ROW", w::ROW),
    ("WIDGET_COLUMN", w::COLUMN),
    ("WIDGET_SCROLL", w::SCROLL),
    ("WIDGET_TABS", w::TABS),
    ("WIDGET_IMAGE", w::IMAGE),
    ("WIDGET_TEXT_FIELD", w::TEXT_FIELD),
    ("WIDGET_TEXT_AREA", w::TEXT_AREA),
    ("WIDGET_CHECKBOX", w::CHECKBOX),
    ("WIDGET_SWITCH", w::SWITCH),
    ("WIDGET_COLOR_PICKER", w::COLOR_PICKER),
    ("WIDGET_DROPDOWN", w::DROPDOWN),
    ("WIDGET_MENU", w::MENU),
    ("WIDGET_LIST", w::LIST),
    ("WIDGET_TREE", w::TREE),
    ("WIDGET_TABLE", w::TABLE),
    ("WIDGET_INSPECTOR", w::INSPECTOR),
    ("WIDGET_SLIDER", w::SLIDER),
    ("WIDGET_NUMBER_FIELD", w::NUMBER_FIELD),
    ("WIDGET_PROGRESS_BAR", w::PROGRESS_BAR),
    ("WIDGET_GRID", w::GRID),
    ("WIDGET_FLOW", w::FLOW),
    ("WIDGET_FOLD", w::FOLD),
    ("WIDGET_DIALOG", w::DIALOG),
    ("WIDGET_TOAST", w::TOAST),
    ("WIDGET_WINDOW", w::WINDOW),
    ("WIDGET_SEPARATOR", w::SEPARATOR),
    ("WIDGET_CODE", w::CODE),
    ("WIDGET_STACK", w::STACK),
];

/// Where a container puts its children, and where text sits in its width.
pub const ALIGNS: &[(&str, &str)] = &[
    ("ALIGN_START", w::START),
    ("ALIGN_CENTER", w::CENTER),
    ("ALIGN_END", w::END),
];

/// The `align_items` words text has no use for: children filling the
/// container's cross axis, and children on their first baseline.
pub const ITEM_ALIGNS: &[(&str, &str)] = &[
    ("ALIGN_STRETCH", w::STRETCH),
    ("ALIGN_BASELINE", w::BASELINE),
];

/// Slant, for `font_style`.
pub const FONT_STYLES: &[(&str, &str)] = &[
    ("FONT_STYLE_NORMAL", w::NORMAL),
    ("FONT_STYLE_ITALIC", w::ITALIC),
    ("FONT_STYLE_OBLIQUE", w::OBLIQUE),
];

/// Font families the theme registers.
pub const FONTS: &[(&str, &str)] = &[("FONT_MONO", w::MONO), ("FONT_HEADING", w::HEADING)];

/// The screen classes, as `ui.width_class` and `ui.height_class` answer them
/// and as a widget's own class table names them. The words are the engine's
/// and a project cannot add to them: a theme, a scene and an addon share
/// them, so one that meant something else somewhere would not be a word.
pub const CLASSES: &[(&str, &str)] = &[
    ("WIDTH_NARROW", balaur_core::facts::NARROW),
    ("WIDTH_MEDIUM", balaur_core::facts::MEDIUM),
    ("WIDTH_WIDE", balaur_core::facts::WIDE),
    ("HEIGHT_SHORT", balaur_core::facts::SHORT),
    ("HEIGHT_TALL", balaur_core::facts::TALL),
    ("INPUT_TOUCH", balaur_core::tags::TOUCH),
    ("INPUT_POINTER", balaur_core::tags::POINTER),
];

/// Keyboard modifiers accepted by shortcut bindings.
pub const MODIFIERS: &[(&str, &str)] = &[
    ("MODIFIER_CMD", w::CMD),
    ("MODIFIER_CTRL", w::CTRL),
    ("MODIFIER_ALT", w::ALT),
    ("MODIFIER_SHIFT", w::SHIFT),
];

/// Declare `ui.*`. Takes the `Registry` rather than a module because it opens
/// the module itself: through the registry, so a scriptless app builds the
/// plugin instead of panicking.
pub(crate) fn install_ui_api(reg: &mut Registry<'_>) -> Result<()> {
    let mut m = reg.script_module("ui")?;
    let m: &mut dyn Bindings<Engine> = &mut *m;

    m.module_doc(
        "What a script asks of the UI and tells it: where a `widget` node drew, focus, the screen's classes, the scale, the theme, the clipboard, when a pass runs and the pooled rows. Nothing here draws; a UI is `widget` nodes in a scene.",
    );

    for (name, value) in ANCHORS
        .iter()
        .chain(WIDGET_KINDS)
        .chain(ALIGNS)
        .chain(ITEM_ALIGNS)
        .chain(FONT_STYLES)
        .chain(FONTS)
        .chain(CLASSES)
        .chain(MODIFIERS)
    {
        m.constant(name, Value::Str((*value).to_string()));
    }
    install_theme(m);
    crate::contrast::install(m);
    install_widget_layer(m);
    crate::widget::pool::install_pool(m);
    install_scale(m);
    install_classes(m);
    crate::pacing::install(m);
    crate::loading::install(m);
    install_queries(m);

    Ok(())
}

/// `ui.*` bindings: theme.
fn install_theme(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "set_theme",
            &[],
            "(theme: table)",
            "Set the theme the UI falls back on where a node's `widget_theme` says nothing, from a theme document: source colours under `colors` and sizes under `sizes`. What the document leaves out is derived from its sources.",
        ),
        (
            "complete_theme",
            &[],
            "(theme: table) -> map",
            "The theme document with every colour and size it leaves out derived from its sources, and `dark` stated.",
        ),
    ]);
    // No reader by design (N8): the caller holds the document it passed.
    m.function("set_theme", |eng: &Engine, doc: Value| {
        let doc = balaur_core::node_api::to_toml(&doc)?;
        let config = eng.resource::<UiConfig>();
        let mut config = config.borrow_mut();
        config.theme = theme::ThemeTokens::from_doc(&doc);
        config.changed = true;
        Ok(())
    });
    m.function("complete_theme", |_: &Engine, doc: Value| {
        let doc = balaur_core::node_api::to_toml(&doc)?;
        balaur_core::node_api::from_toml(&crate::palette::complete(&doc))
    });
}

/// `ui.*` bindings: widget layer.
fn install_widget_layer(m: &mut dyn Bindings<Engine>) {
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
        "set_keyboard_navigation",
        &[],
        "",
        "Let the arrows, Tab, Enter and Space move and activate the focused widget. Off unless asked for, so a game that moves with the arrows does not click its own HUD; `standard_app` turns it on for a project declaring the `ui_*` actions.",
    )]);
    {
        m.function("set_keyboard_navigation", |eng: &Engine, on: bool| {
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
                    crate::widget::node::Surface {
                        enabled,
                        rect: rect_of(x, y, w, h),
                    },
                );
                Ok(())
            },
        );
    }
    install_rects(m);
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
    use crate::widget::node::Move;
    m.describe(&[
        ("focused_widget", &[], "() -> node?", "The widget node focus is on, or nil."),
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
        (
            "click",
            &[],
            "(node: node, opts: map?) -> bool",
            "Click a widget node as the pointer would, at the next tick and with no window needed: what a test harness drives the game with. False for a node a pointer could not click, hidden, disabled or not a widget; `#{ hidden: true }` clicks a hidden one anyway, as a test emitting its signal would.",
        ),
        (
            "reread",
            &[],
            "(node: node) -> bool",
            "Fill every `inspector` widget at or under the node again from the node it shows, at the next pass: what a script says when it changed those nodes itself, naming one inspector or a whole panel. An inspector reads nothing back on its own; its widget written or a row edited fills it too. False where there is no inspector.",
        ),
        (
            "edit",
            &[],
            "(node: node, value) -> bool",
            "Change a widget node's value as the reader would, at the next tick, so it announces `change`: a number for a slider or a number field, a colour for a swatch, a boolean for a fold, text for a field, a dropdown's pick, a list's row or a tab. False for a disabled widget or a value its kind does not take.",
        ),
    ]);
    m.function("focused_widget", |eng: &Engine, ()| {
        let focus = eng.resource::<crate::UiFocus>();
        let focused = focus.borrow().focused;
        Ok(focused.map_or(balaur_script::Value::Nil, |e| {
            balaur_script::Value::Node(balaur_core::node_id_of(e).0)
        }))
    });
    m.function("set_focus", |eng: &Engine, node: balaur_script::NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let focus = eng.resource::<crate::UiFocus>();
        let mut focus = focus.borrow_mut();
        focus.focused = Some(entity);
        // Taken, not merely resting there: the next draw puts the caret in a
        // field and tells the node focus arrived.
        focus.taking = true;
        Ok(())
    });
    m.function("reread", |eng: &Engine, node: balaur_script::NodeId| {
        Ok(crate::widget::inspector::reread(
            eng,
            balaur_core::entity_of(node)?,
        ))
    });
    m.function(
        "edit",
        |eng: &Engine, (node, value): (balaur_script::NodeId, balaur_script::Value)| {
            Ok(crate::edit(eng, balaur_core::entity_of(node)?, &value))
        },
    );
    m.function(
        "click",
        |eng: &Engine, (node, opts): (balaur_script::NodeId, Option<balaur_script::Value>)| {
            let hidden = match opts {
                Some(balaur_script::Value::Map(fields)) => fields
                    .iter()
                    .any(|(k, v)| k == "hidden" && matches!(v, balaur_script::Value::Bool(true))),
                _ => false,
            };
            Ok(crate::click(eng, balaur_core::entity_of(node)?, hidden))
        },
    );
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

/// `ui.*` bindings: the screen classes.
fn install_classes(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "width_class",
            &[],
            "",
            "How much width this screen has, as `ui::NARROW`, `ui::MEDIUM` or `ui::WIDE`. Read every frame: a rotation changes it. A run with no window answers `ui::WIDE`.",
        ),
        (
            "height_class",
            &[],
            "",
            "How much height this screen has, as `ui::SHORT` or `ui::TALL`.",
        ),
    ]);
    m.function("width_class", |eng: &Engine, ()| {
        let facts = balaur_core::facts::device(eng);
        Ok(balaur_core::facts::width_class(
            facts.design_size()[0],
            crate::class_lines(eng),
        ))
    });
    m.function("height_class", |eng: &Engine, ()| {
        let facts = balaur_core::facts::device(eng);
        Ok(balaur_core::facts::height_class(
            facts.design_size()[1],
            crate::class_lines(eng),
        ))
    });
}

/// `ui.*` bindings: scale.
fn install_scale(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "scale",
            &[],
            "",
            "The global UI scale: how many screen pixels a design pixel is drawn at, on top of the display's own.",
        ),
        (
            "set_scale",
            &[],
            "",
            "Set the global UI scale, clamped to between 0.25 and 3.0. It is egui's zoom factor, so it grows every control and every font; `screen_size` already answers in the design pixels it leaves.",
        ),
    ]);
    m.function("scale", |eng: &Engine, ()| {
        let config = eng.resource::<UiConfig>();
        let v = config.borrow().scale;
        Ok(v)
    });
    m.function("set_scale", |eng: &Engine, f: f32| {
        let config = eng.resource::<UiConfig>();
        let mut config = config.borrow_mut();
        config.scale = f.clamp(0.25, 3.0);
        config.asked = true;
        Ok(())
    });
}

/// `ui.*` bindings: queries.
fn install_queries(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("window_size", &[], "", "The window's width and height, in design pixels; a headless run answers the `[window]` size the project states."),
        ("set_clipboard", &[], "", "Copy text to the system clipboard. The next UI pass hands it over, and asks for itself."),
        ("pasted_text", &[], "", "The text the last UI pass saw pasted, empty when it saw none: the platform clipboard is not readable on demand. The `pasted` event carries text no field took as it arrives."),
        ("wants_keyboard", &[], "", "Whether a UI widget held keyboard focus when the last pass ended, so the game should leave key presses alone."),
        ("wants_pointer", &[], "", "Whether the UI has the pointer or finger, so the game should leave it alone: what stops a tap on a HUD button also firing the shot behind it. A press is routed once, from the top: an open dialog, then a popup or window, then a widget that is not `interactive = false` or a seam between boxes, and the world last; it keeps that answer until the button comes up. False without a window."),
        ("takes_pointer_at", &[], "(x: number, y: number) -> bool", "Whether the UI would take a press at this point, in design pixels, by the same layers as `wants_pointer`: what an editor asks of a point it is about to act on."),
    ]);
    m.function("window_size", |eng: &Engine, ()| {
        // The backend publishes the screen before the tick, and the tick runs
        // before the first pass: until then egui's viewport is a placeholder.
        let [w, h] = balaur_core::facts::device(eng).design_size();
        if w > 0.0 && h > 0.0 {
            return Ok((w, h));
        }
        // No screen and no pass is a headless run, which answers the window
        // the project states: what it would open, the same on every machine.
        with_ctx(|ctx| {
            let rect = ctx.viewport_rect();
            Ok((rect.width(), rect.height()))
        })
        .or_else(|_| {
            let window = balaur_core::project::WindowSettings::from_settings(eng);
            #[allow(clippy::cast_precision_loss, reason = "a window's size in pixels")]
            Ok((window.width as f32, window.height as f32))
        })
    });
    install_clipboard(m);
}

/// The clipboard pair and who holds the pointer and the keyboard: queries
/// that answer about the frame.
fn install_clipboard(m: &mut dyn Bindings<Engine>) {
    m.function("set_clipboard", |eng: &Engine, text: String| {
        eng.resource::<UiState>().borrow_mut().clipboard = Some(text);
        crate::pacing::ask(eng);
        Ok(())
    });
    m.function("pasted_text", |eng: &Engine, ()| {
        Ok(eng.resource::<UiState>().borrow().pasted.clone())
    });
    m.function("wants_keyboard", |eng: &Engine, ()| {
        Ok(eng.resource::<UiState>().borrow().wants_keyboard)
    });
    m.function("wants_pointer", |eng: &Engine, ()| {
        Ok(crate::routing::pointer_is_ui(eng))
    });
    m.function("takes_pointer_at", |eng: &Engine, (x, y): (f64, f64)| {
        Ok(crate::routing::takes_point(
            eng,
            egui::pos2(x as f32, y as f32),
        ))
    });
}

/// A drawn box as the script sees it, or nil for one that has not drawn.
fn rect_value(rect: Option<egui::Rect>) -> Value {
    rect.map_or(Value::Nil, crate::widget::arrange::rect_value)
}

fn install_rects(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "widget_rect",
            &[],
            "-> Rect?",
            "Where a `widget` node was last drawn, as `#{ x, y, width, height }` in design pixels; empty until it has drawn once.",
        ),
        (
            "tab_rect",
            &[],
            "-> Rect?",
            "Where a `tabs` page's own button in the strip was last drawn, as `#{ x, y, width, height }` in design pixels; empty before the strip has drawn. `widget_rect` on the same node answers with the page body.",
        ),
    ]);
    m.records(&[(
        "Rect",
        "{ x: float, y: float, width: float, height: float }",
        "A drawn box: its top left corner and its size, in design pixels.",
    )]);
    m.function(
        "widget_rect",
        |_eng: &Engine, node: balaur_script::NodeId| {
            Ok(rect_value(crate::widget::arrange::drawn_at(
                balaur_core::entity_of(node)?,
            )))
        },
    );
    m.function("tab_rect", |_eng: &Engine, node: balaur_script::NodeId| {
        Ok(rect_value(crate::widget::arrange::tab_head_at(
            balaur_core::entity_of(node)?,
        )))
    });
}
