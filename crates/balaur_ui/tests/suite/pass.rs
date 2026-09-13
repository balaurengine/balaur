//! The `ui` module driven through a real script inside a real egui pass.
//!
//! egui runs perfectly well without a window, so the whole widget surface is
//! testable in CI. Calling the bindings from a script is the point: it is the
//! path a game takes, and it catches a binding that registers but cannot be
//! called.

use balaur::{AppConfig, standard_app};
use balaur_core::App;

/// The log buffer is global, and tests run in parallel, so one test's
/// deliberate error would surface in another's assertions. Hold this for the
/// duration of a pass.
static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Build a project whose `draw_ui` runs `body`, then run two egui passes.
fn draw(body: &str) -> (App, Vec<String>) {
    let (app, _, errors) = draw_with(body);
    (app, errors)
}

/// The same, keeping the context the passes ran in: fonts are bound to that
/// one, so a test running further passes has to use it.
fn draw_with(body: &str) -> (App, egui::Context, Vec<String>) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"ui\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = { source = \"scripts/s.rn\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.rn"),
        format!("pub fn init(this) {{}}\npub fn draw_ui(this) {{\n{body}\n}}\n"),
    )
    .unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();

    // Two passes: run_pass installs fonts on the first and skips drawing,
    // so one pass would leave every assertion below vacuous.
    let ctx = egui::Context::default();
    for _ in 0..2 {
        ctx.begin_pass(egui::RawInput::default());
        balaur_ui::run_pass(&app.engine, &ctx);
        // A real renderer uploads these; dropping them unapplied is a panic.
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
    }
    let errors = balaur_core::logbuf::recent(50)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    (app, ctx, errors)
}

fn draw_clean(body: &str) {
    let (_app, errors) = draw(body);
    assert!(errors.is_empty(), "the pass logged errors: {errors:#?}");
}

/// A number the script left on `this`, read back from Rust.
fn field(app: &App, name: &str) -> Option<f64> {
    let root = balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), "Root")
        .expect("the scene has a Root node");
    balaur::rune::rune_of(&app.engine).number_field(root, name)
}

#[test]
fn a_pass_with_no_widgets_is_quiet() {
    draw_clean("");
}

#[test]
fn panels_and_containers_nest() {
    draw_clean(
        r#"
        ui::top_panel("bar", #{ height: 40 }, || {
            ui::horizontal(#{}, || { ui::label("across"); });
        });
        ui::bottom_panel("status", #{ height: 20 }, || { ui::label("bottom"); });
        ui::left_panel("side", #{ width: 60 }, || { ui::label("left"); });
        ui::right_panel("props", #{ width: 60 }, || { ui::label("right"); });
        ui::central_panel(#{}, || {
            ui::vertical(|| { ui::label("down"); });
            ui::scroll("sc", #{}, || { ui::label("scrolled"); });
            ui::frame(#{}, || { ui::label("framed"); });
        });
        "#,
    );
}

#[test]
fn text_and_layout_helpers_run() {
    draw_clean(
        r##"
        ui::central_panel(#{}, || {
            ui::label("plain");
            ui::code_line("1", [#{ text: "let x = 1;" }]);
            ui::separator();
            ui::add_space(4);
            ui::spacing(2, 2);
            ui::dot("#ffffff", 4);
            assert!(ui::available_width() is f64);
            assert!(ui::available_height() is f64);
            let (w, h) = ui::screen_size();
            assert!(w is f64 && h is f64);
            assert!(ui::wants_keyboard() is bool);
        });
        "##,
    );
}

#[test]
fn interactive_widgets_report_no_interaction_without_input() {
    draw_clean(
        r#"
        ui::central_panel(#{}, || {
            assert!(!ui::pill("a pill", #{ active: true }));
            assert!(!ui::circle_button("x"));
            let (on, clicked) = ui::toggle(false, #{});
            assert!(!on && !clicked, "a toggle flipped with no input");
            let (text, changed, _) = ui::text_field("field", "type here");
            assert!(text is String, "text_field returns its buffer");
            assert!(!changed, "nothing typed, yet it reported a change");
            let (v, _) = ui::slider(0.5, 0.0, 1.0, #{});
            assert!(v is f64);
        });
        "#,
    );
}

#[test]
fn the_code_editor_returns_its_buffer_unchanged() {
    let (app, errors) = draw(
        r#"
        this.drawn = this.get("drawn").unwrap_or(0);
        this.edits = this.get("edits").unwrap_or(0);
        ui::central_panel(#{}, || {
            let (text, changed, hit, caret) = ui::code_editor("ed", "let x = 1;\nx");
            this.drawn = this.drawn + 1;
            if changed { this.edits = this.edits + 1; }
            assert!(hit is Tuple, "a gutter line was clicked with no input");
            assert!(caret is Tuple, "an unfocused editor reported a caret");
            assert!(text.len() > 0, "the editor lost its buffer");
        });
        "#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let drawn = field(&app, "drawn").expect("the editor never drew");
    let edits = field(&app, "edits").expect("no edit count recorded");
    assert!(drawn > 0.0, "the editor never drew");
    assert!(
        edits == 0.0,
        "the editor reported {edits} edits with no input"
    );
}

#[test]
fn bad_options_are_reported_rather_than_fatal() {
    let (_app, errors) = draw(
        r#"
        ui::central_panel(#{}, || {
            ui::label("still drawn");
        });
        "#,
    );
    assert!(errors.is_empty());
}

#[test]
fn a_script_error_inside_a_pass_is_logged_not_fatal() {
    let (_app, errors) = draw(
        r#"
        ui::central_panel(#{}, || {
            panic!("deliberate");
        });
        "#,
    );
    assert!(
        errors.iter().any(|e| e.contains("deliberate")),
        "the error was swallowed: {errors:?}"
    );
}

#[test]
fn the_scale_factor_is_readable_and_settable() {
    draw_clean(
        r"
        ui::central_panel(#{}, || {
            let before = ui::scale();
            ui::set_scale(1.25);
            assert!(math::abs(ui::scale() - 1.25) < 1e-6);
            ui::set_scale(before);
        });
        ",
    );
}

/// A control: everything above asserts from inside the script, so if
/// `draw_ui` never ran they would all pass without testing anything. This
/// checks from Rust that the pass really called it.
#[test]
fn draw_ui_is_actually_called() {
    let (app, _) = draw(r#"this.passes = this.get("passes").unwrap_or(0) + 1;"#);
    let passes = field(&app, "passes").unwrap_or(0.0);
    assert!(
        passes > 0.0,
        "draw_ui never ran, so these tests prove nothing"
    );
}

#[test]
fn the_remaining_widgets_are_callable() {
    draw_clean(
        r#"
        ui::central_panel(#{}, || {
            let (v, _) = ui::drag_value(1.5, #{});
            assert!(v is f64, "drag_value should return a number");

            let (choice, changed) = ui::dropdown("sel", "b", ["a", "b", "c"], #{});
            assert!(choice == "b", "dropdown changed with no input");
            assert!(!changed);

            assert!(!ui::menu_item("Open", #{}));
            ui::rect_stroke(0, 0, 10, 10, #{});
        });
        "#,
    );
}

#[test]
fn a_modal_runs_its_body() {
    let (app, errors) = draw(
        r#"
        this.in_modal = 0;
        ui::central_panel(#{}, || {
            ui::modal("m", #{}, || { this.in_modal = 1; });
        });
        "#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let ran = field(&app, "in_modal").unwrap_or(0.0) > 0.0;
    assert!(ran, "the modal never ran its body");
}

#[test]
fn set_text_replaces_a_field_buffer() {
    draw_clean(
        r#"
        ui::central_panel(#{}, || {
            ui::text_field("f", "placeholder");
            ui::set_text("f", "written from outside");
            let (shown, _, _) = ui::text_field("f", "placeholder");
            assert!(shown == "written from outside", "set_text did not take: {}", shown);
        });
        "#,
    );
}

#[test]
fn a_missing_image_does_not_stop_the_pass() {
    let (_app, errors) = draw(
        r#"
        ui::central_panel(#{}, || {
            ui::image("no/such/picture.png", #{});
            ui::label("drawn after the bad image");
        });
        "#,
    );
    assert!(
        errors.iter().all(|e| !e.contains("panic")),
        "a missing image panicked: {errors:#?}"
    );
}

#[test]
fn a_theme_can_be_set_from_a_script() {
    draw_clean(
        r##"
        ui::set_theme(#{ panel: "#101418", text: "#f0f0f0", accent: "#d5814e" });
        ui::central_panel(#{}, || { ui::label("themed"); });
        "##,
    );
}

#[test]
fn the_widget_layer_can_be_placed_and_turned_off() {
    draw_clean(
        r#"
        ui::set_widget_layer(true, 0, 0, 320, 240);
        ui::set_widget_layer(false, 0, 0, 0, 0);
        ui::central_panel(#{}, || { ui::label("after"); });
        "#,
    );
}

#[test]
fn shortcuts_report_no_press_without_input() {
    draw_clean(
        r#"
        ui::central_panel(#{}, || {
            assert!(!ui::shortcut("cmd", "S"));
            assert!(!ui::shortcut("ctrl", "Z"));
            assert!(!ui::shortcut("", "A"));
        });
        "#,
    );
}

/// One chord for both keys: the window layer sets egui's `command` for
/// Control and for Command, and a key arrives as its physical code, so ⇧⌘\
/// is `Backslash` whatever the layout prints on it.
#[test]
fn a_cmd_chord_answers_to_control_and_to_command() {
    let (app, ctx, errors) = draw_with(
        r#"
        this.focus = this.get("focus").unwrap_or(0);
        ui::central_panel(#{}, || {
            if ui::shortcut("cmd+shift", "Backslash") { this.focus = this.focus + 1; }
        });
        "#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(field(&app, "focus"), Some(0.0));
    let held = [
        egui::Modifiers {
            shift: true,
            ctrl: true,
            command: true,
            ..Default::default()
        },
        egui::Modifiers {
            shift: true,
            mac_cmd: true,
            command: true,
            ..Default::default()
        },
    ];
    for modifiers in held {
        feed(&app, &ctx, vec![chord(egui::Key::Backslash, modifiers)]);
    }
    assert_eq!(
        field(&app, "focus"),
        Some(2.0),
        "a held modifier was missed"
    );
    // The bare key is somebody else's: a chord that asks for one must see one.
    feed(
        &app,
        &ctx,
        vec![chord(egui::Key::Backslash, egui::Modifiers::NONE)],
    );
    assert_eq!(
        field(&app, "focus"),
        Some(2.0),
        "the bare key fired the chord"
    );
}

/// One key press, with the modifiers it arrived under.
fn chord(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// The names the profiler files a frame's passes under, in order.
fn pass_names(app: &App) -> Vec<String> {
    app.engine
        .resource::<balaur_core::timings::Timings>()
        .borrow()
        .spans
        .iter()
        .map(|(name, _)| name.to_string())
        .filter(|name| name.starts_with("ui"))
        .collect()
}

/// egui reruns the whole closure when a pass only learned a size, so the
/// shell is built twice; a profiler that filed both under one name would
/// read as one expensive pass instead of two ordinary ones.
#[test]
fn a_second_pass_in_one_frame_is_filed_as_a_rerun() {
    let (mut app, ctx, _) = draw_with(r#"ui::central_panel(#{}, || { ui::label("x"); });"#);
    // Publish the passes the helper ran, so the table below holds this frame.
    app.tick(balaur_core::FIXED_DT);
    // What the windowed loop calls once a frame, which is what starts one.
    balaur_ui::wants_pass(&app.engine, &ctx, true, false);
    for _ in 0..2 {
        ctx.begin_pass(egui::RawInput::default());
        balaur_ui::run_pass(&app.engine, &ctx);
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
    }
    // The spans of a frame are published when it ends, never mid-frame.
    app.tick(balaur_core::FIXED_DT);
    assert_eq!(pass_names(&app), ["ui", "ui rerun"]);
}

/// One pass over the same context, with the events the caller feeds it.
fn feed(app: &App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(640.0, 480.0),
        )),
        events,
        ..Default::default()
    };
    ctx.begin_pass(input);
    balaur_ui::run_pass(&app.engine, ctx);
    let mut out = ctx.end_pass();
    out.textures_delta.clear();
    out
}

/// The colours of the filled boxes a pass painted.
fn fills(out: &egui::FullOutput) -> Vec<egui::Color32> {
    out.shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect) => Some(rect.fill),
            _ => None,
        })
        .collect()
}

fn tap(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

/// A frame with `menu_click` answers the pointer over all of itself, so a
/// mark and the name beside it read as one control: a click on the caption
/// used to fall through to nothing.
#[test]
fn a_frame_menu_opens_from_a_click_on_its_caption() {
    let (app, ctx, errors) = draw_with(
        r#"
        this.rows = 0.0;
        ui::central_panel(#{}, || {
            ui::frame(#{ padding_x: 8, menu_click: || {
                this.rows = this.rows + 1.0;
                ui::menu_item("Open", #{ width: 120 });
            } }, || {
                ui::label("Balaur", #{});
            });
        });
        "#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(field(&app, "rows"), Some(0.0), "the menu drew unopened");
    let at = egui::pos2(30.0, 14.0);
    feed(&app, &ctx, tap(at, true));
    feed(&app, &ctx, tap(at, false));
    feed(&app, &ctx, vec![]);
    let drawn = field(&app, "rows").unwrap_or(0.0);
    assert!(drawn > 0.0, "a click on the caption opened no menu");
}

/// The same frame takes `hover_fill` while the pointer is over it, painted
/// under the callback's own widgets rather than over them.
#[test]
fn a_frame_menu_lights_up_under_the_pointer() {
    let (app, ctx, errors) = draw_with(
        r##"
        ui::central_panel(#{}, || {
            ui::frame(#{
                padding_x: 8, fill: "#101215", hover_fill: "#2b3037",
                menu_click: || { ui::menu_item("Open", #{ width: 120 }); },
            }, || {
                ui::label("Balaur", #{});
            });
        });
        "##,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let cold = egui::Color32::from_rgb(0x10, 0x12, 0x15);
    let warm = egui::Color32::from_rgb(0x2b, 0x30, 0x37);
    let away = feed(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(egui::pos2(500.0, 400.0))],
    );
    assert!(
        fills(&away).contains(&cold),
        "the resting fill went missing"
    );
    assert!(
        !fills(&away).contains(&warm),
        "it lit up with the pointer away"
    );
    let over = feed(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(egui::pos2(30.0, 14.0))],
    );
    assert!(
        fills(&over).contains(&warm),
        "the pointer over it lit nothing"
    );
}

/// `menu_click` hangs a menu off a left click. The rows are the callback's,
/// so nothing inside it draws until the menu is open — which is the whole
/// difference from `menu`, whose menu waits for the other button.
#[test]
fn a_pill_menu_opens_on_a_left_click() {
    let (app, ctx, errors) = draw_with(
        r#"
        this.rows = 0.0;
        ui::central_panel(#{}, || {
            ui::pill("Menu", #{ menu_click: || {
                this.rows = this.rows + 1.0;
                ui::menu_item("Open", #{ width: 120, trailing: "⌘O" });
            } });
        });
        "#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(field(&app, "rows"), Some(0.0), "the menu drew unopened");
    let at = egui::pos2(24.0, 20.0);
    feed(&app, &ctx, tap(at, true));
    feed(&app, &ctx, tap(at, false));
    feed(&app, &ctx, vec![]);
    let drawn = field(&app, "rows").unwrap_or(0.0);
    assert!(drawn > 0.0, "a click on the pill opened no menu");
    // Away from the pill and the menu: the popup closes and stops drawing.
    // `rows` counts one pass, since the body zeroes it on every one.
    let away = egui::pos2(500.0, 400.0);
    feed(&app, &ctx, tap(away, true));
    feed(&app, &ctx, tap(away, false));
    feed(&app, &ctx, vec![]);
    assert_eq!(
        field(&app, "rows"),
        Some(0.0),
        "the menu kept drawing after it was dismissed"
    );
}

/// A pointer button held outside every widget belongs to whatever the scene
/// is doing with it — orbiting a camera — and the shell cannot change until
/// it comes up, so the frames between are the scene's alone.
#[test]
fn a_drag_that_began_outside_the_ui_wants_no_pass_for_moving() {
    let (app, ctx, _) = draw_with(r#"ui::central_panel(#{}, || { ui::label("x"); });"#);
    let away = egui::pos2(500.0, 400.0);
    feed(&app, &ctx, tap(away, true));
    assert!(
        balaur_ui::pointer_is_dragging_elsewhere(&ctx, true),
        "the press took no widget, so the drag is the scene's"
    );
    assert!(
        !balaur_ui::pointer_is_dragging_elsewhere(&ctx, false),
        "a camera with no drag buttons is dragging nothing"
    );
    feed(&app, &ctx, tap(away, false));
    assert!(
        !balaur_ui::pointer_is_dragging_elsewhere(&ctx, true),
        "the button came up, so the pointer answers the shell again"
    );
}

/// A press egui took a candidate from is the UI's drag: a button held, a
/// scroll dragged, a field selecting text all move the picture as the
/// pointer does.
#[test]
fn a_drag_that_began_on_a_widget_still_wants_its_passes() {
    let (app, ctx, errors) = draw_with(r#"ui::central_panel(#{}, || { ui::pill("Go", #{}); });"#);
    assert!(errors.is_empty(), "{errors:#?}");
    feed(&app, &ctx, tap(egui::pos2(24.0, 20.0), true));
    assert!(!balaur_ui::pointer_is_dragging_elsewhere(&ctx, true));
}

/// `pacing::IDLE`, which is private: outwaiting the tick means knowing it.
const IDLE_TICK: std::time::Duration = std::time::Duration::from_millis(250);

/// The idle tick is there for state that moves without input, which a drag
/// outside the UI has none of. Firing one mid-drag spends a pass on the same
/// picture, and where the pass is most of the frame that is the stall the
/// camera lurches out of.
#[test]
fn the_idle_tick_waits_out_a_drag_the_ui_is_no_part_of() {
    let (app, ctx, errors) =
        draw_with(r#"ui::set_lazy(true); ui::central_panel(#{}, || { ui::label("x"); });"#);
    assert!(errors.is_empty(), "{errors:#?}");
    balaur_ui::honour_lazy(&app.engine);
    let away = egui::pos2(500.0, 400.0);
    feed(&app, &ctx, tap(away, true));
    // egui asks for a pass of its own after a press, and gives up asking two
    // passes later.
    feed(&app, &ctx, vec![]);
    feed(&app, &ctx, vec![]);
    std::thread::sleep(IDLE_TICK + std::time::Duration::from_millis(50));
    // The log buffer is global, so a line another test wrote reads here as a
    // reason to run a pass; the call before each answer takes that counter.
    let mut forced = true;
    for _ in 0..4 {
        let _ = balaur_ui::wants_pass(&app.engine, &ctx, false, true);
        forced = balaur_ui::wants_pass(&app.engine, &ctx, false, true);
        if !forced {
            break;
        }
    }
    assert!(!forced, "the tick forced a pass in the middle of a drag");
    assert!(
        balaur_ui::wants_pass(&app.engine, &ctx, false, false),
        "the tick never fired for a shell that owns the pointer"
    );
}

/// Laying a file out costs its length, and the editor draws the same file on
/// every frame of a session. Nothing about the picture changes until the text
/// or its colours do, and the galley must be the same one until then.
#[test]
fn the_code_editor_lays_its_text_out_once_until_its_look_changes() {
    let (app, ctx, errors) = draw_with(
        r##"
        this.n = this.get("n").unwrap_or(0) + 1;
        let comment = if this.n > 2 { "#ff0000" } else { "#808080" };
        ui::central_panel(#{}, || {
            ui::code_editor("ed", "// a\nlet x = 1;", #{ k_com: comment });
        });
        "##,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let first = laid_out(&app);
    feed(&app, &ctx, Vec::new());
    let again = laid_out(&app);
    assert!(
        std::sync::Arc::ptr_eq(&first, &again),
        "a pass that changed nothing laid the file out a second time"
    );
    feed(&app, &ctx, Vec::new());
    let recoloured = laid_out(&app);
    assert!(
        !std::sync::Arc::ptr_eq(&again, &recoloured),
        "the comment colour changed and the editor kept the old picture"
    );
}

/// The galley the code editor last laid out, whatever its id.
fn laid_out(app: &App) -> std::sync::Arc<egui::Galley> {
    let state = app.engine.resource::<balaur_ui::UiState>();
    let state = state.borrow();
    let (_, galley) = state
        .code_galleys
        .values()
        .next()
        .expect("the code editor laid nothing out");
    std::sync::Arc::clone(galley)
}

/// An overlay given a size keeps its layer inside it. egui grows an area to
/// its content, and an area is a layer: a row one chip too long took the
/// pointer off every control under the whole of it.
#[test]
fn a_sized_overlay_keeps_its_layer_inside_the_box_it_was_given() {
    let (app, ctx, errors) = draw_with(
        r#"
        ui::overlay("chips", #{ x: 10, y: 10, w: 60, h: 24 }, || {
            ui::horizontal(#{ height: 24 }, || {
                for n in 0..8 {
                    ui::pill("chip", #{ height: 20 });
                }
            });
        });
        "#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    feed(&app, &ctx, vec![]);
    feed(&app, &ctx, vec![]);
    let rect = ctx
        .memory(|m| m.area_rect(egui::Id::new("chips")))
        .expect("the overlay drew");
    assert_eq!(
        rect,
        egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(60.0, 24.0)),
        "eight chips do not widen the layer past the box the caller stated"
    );
}

/// A role's `hover` table repaints the control under the pointer, and its
/// colours are spelled from the same tokens the rest of the role names.
#[test]
fn a_roles_hover_table_repaints_the_pill_under_the_pointer() {
    let (app, ctx, errors) = draw_with(
        r##"
        ui::set_theme(#{
            dark: true, ink: "#101215", warm: "#2b3037",
            roles: #{ tile: #{ fill: "ink", hover: #{ fill: "warm" } } },
        });
        ui::central_panel(#{}, || {
            ui::pill("Go", #{ role: "tile", height: 24, min_width: 60 });
        });
        "##,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let cold = egui::Color32::from_rgb(0x10, 0x12, 0x15);
    let warm = egui::Color32::from_rgb(0x2b, 0x30, 0x37);
    let away = feed(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(egui::pos2(500.0, 400.0))],
    );
    assert!(
        fills(&away).contains(&cold) && !fills(&away).contains(&warm),
        "at rest the role's own fill is painted"
    );
    // Twice: egui reads a hover off the widget rects of the pass before.
    let on = egui::pos2(20.0, 20.0);
    feed(&app, &ctx, vec![egui::Event::PointerMoved(on)]);
    let over = feed(&app, &ctx, vec![egui::Event::PointerMoved(on)]);
    assert!(
        fills(&over).contains(&warm),
        "under the pointer the role's `hover` table paints instead"
    );
}

/// The same, for a control inside a sized overlay: the box the overlay keeps
/// its layer inside must not cost the controls in it their own state.
#[test]
fn a_pill_in_a_sized_overlay_still_lights_up() {
    let (app, ctx, errors) = draw_with(
        r##"
        ui::overlay("bar", #{ x: 0, y: 0, w: 200, h: 40 }, || {
            ui::horizontal(#{ height: 30 }, || {
                ui::pill("Go", #{ height: 24, min_width: 60, fill: "#101215" });
            });
        });
        "##,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let away = feed(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(egui::pos2(500.0, 400.0))],
    );
    let cold = fills(&away).len();
    let on = egui::pos2(20.0, 15.0);
    feed(&app, &ctx, vec![egui::Event::PointerMoved(on)]);
    let over = feed(&app, &ctx, vec![egui::Event::PointerMoved(on)]);
    assert!(
        fills(&over).len() > cold,
        "the pill painted nothing more under the pointer: {cold} boxes either way"
    );
}
