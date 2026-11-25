//! Class words past the screen's: a widget's `classes` puts a project's own
//! words in force for what it lays out, and a container's `fit` takes its
//! words in order until what it holds fits on one line. A role answers a
//! word with a table of its name, as it answers `narrow`; `show_text` and
//! `truncate` are what a word most often changes.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::hecs::Entity;

const WORDS: &str = r#"["wf_compact", "wf_one", "wf_two"]"#;

/// An app whose project declares `WORDS`, with `theme` at `themes/t.toml`.
fn app_with(theme: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/t.toml"),
        format!("type = \"widget_theme\"\n\n{theme}"),
    )
    .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let app = standard_app(config).unwrap();
    // As `project.toml` states it on a run.
    let words: toml::Value =
        toml::from_str::<toml::Table>(&format!("w = {WORDS}")).unwrap()["w"].clone();
    balaur_core::settings::set(&app.engine, "ui/classes", words);
    (dir, app)
}

/// Passes enough for a fit to climb every step it has.
fn settled(app: &App, ctx: &egui::Context) {
    for _ in 0..12 {
        pass(app, ctx, vec![]);
    }
}

fn rect(entity: Entity) -> egui::Rect {
    balaur_ui::widget_rect(entity).expect("it drew")
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.6
}

/// Three boxes of role `x`, 100 by 10, and narrower under each word.
const BOXES: &str = "[roles.x]\nwidth = 100.0\nheight = 10.0\n\
    [roles.x.wf_one]\nwidth = 60.0\n\
    [roles.x.wf_two]\nwidth = 30.0\n\
    [roles.bar]\nfit = [\"wf_one\", \"wf_two\"]\n";

/// A row of role `bar` holding three boxes, the whole width of a column
/// `width` wide, as a page's heading is.
fn bar_of(app: &App, width: f64, padding: toml::Value) -> (Entity, Entity, Vec<Entity>) {
    let page = add_widget(
        app,
        &toml::toml! {
            kind = "column" theme = "themes/t.toml" x = 0.0 y = 0.0 width = width height = 100.0
            padding = 0.0
        }
        .into(),
    );
    let row = add_child_widget(
        app,
        page,
        "bar",
        &toml::toml! {
            kind = "row" role = "bar" width_percent = 100.0 padding = padding
            widget_layout = { gap = [0.0, 0.0], align_items = "start" }
        }
        .into(),
    );
    let boxes = (0..3)
        .map(|i| {
            add_child_widget(
                app,
                row,
                &format!("b{i}"),
                &toml::toml! { kind = "label" text = "" role = "x" }.into(),
            )
        })
        .collect();
    (page, row, boxes)
}

#[test]
fn a_widget_s_classes_are_in_force_for_what_it_lays_out_and_nowhere_else() {
    let (_dir, app) =
        app_with("[roles.x]\nwidth = 100.0\nheight = 10.0\n[roles.x.wf_compact]\nwidth = 40.0\n");
    let page = add_widget(
        &app,
        &toml::toml! { kind = "column" theme = "themes/t.toml" x = 0.0 y = 0.0 padding = 0.0 }
            .into(),
    );
    let compact = add_child_widget(
        &app,
        page,
        "compact",
        &toml::toml! { kind = "row" classes = ["wf_compact"] padding = 0.0 }.into(),
    );
    let plain = add_child_widget(
        &app,
        page,
        "plain",
        &toml::toml! { kind = "row" padding = 0.0 }.into(),
    );
    let inside = add_child_widget(
        &app,
        compact,
        "inside",
        &toml::toml! { kind = "label" text = "" role = "x" }.into(),
    );
    let beside = add_child_widget(
        &app,
        plain,
        "beside",
        &toml::toml! { kind = "label" text = "" role = "x" }.into(),
    );
    let ctx = egui::Context::default();
    settled(&app, &ctx);
    assert!(close(rect(inside).width(), 40.0), "{:?}", rect(inside));
    assert!(close(rect(beside).width(), 100.0), "{:?}", rect(beside));

    // A script's write moves it the same way.
    balaur::components::patch(
        &app.engine,
        plain,
        "widget",
        &toml::toml! { classes = ["wf_compact"] }.into(),
    )
    .unwrap();
    settled(&app, &ctx);
    assert!(close(rect(beside).width(), 40.0), "{:?}", rect(beside));
}

#[test]
fn a_fit_row_takes_its_words_in_order_until_it_fits_and_gives_them_back() {
    let (_dir, app) = app_with(BOXES);
    let (page, row, boxes) = bar_of(&app, 250.0, toml::Value::Float(0.0));
    let ctx = egui::Context::default();
    settled(&app, &ctx);
    // 300 does not fit 250; with the first word, 180 does.
    assert!(close(rect(boxes[0]).width(), 60.0), "{:?}", rect(boxes[0]));
    assert!(close(rect(row).width(), 250.0), "{:?}", rect(row));

    let narrower = toml::toml! { width = 100.0 };
    balaur::components::patch(&app.engine, page, "widget", &narrower.into()).unwrap();
    settled(&app, &ctx);
    assert!(
        close(rect(boxes[2]).width(), 30.0),
        "both words: {:?}",
        rect(boxes[2])
    );

    let wider = toml::toml! { width = 400.0 };
    balaur::components::patch(&app.engine, page, "widget", &wider.into()).unwrap();
    settled(&app, &ctx);
    assert!(
        close(rect(boxes[1]).width(), 100.0),
        "none: {:?}",
        rect(boxes[1])
    );
}

#[test]
fn a_fit_row_never_wraps_even_with_no_step_left() {
    let (_dir, app) = app_with(BOXES);
    let (_page, _row, boxes) = bar_of(&app, 50.0, toml::Value::Float(0.0));
    let ctx = egui::Context::default();
    settled(&app, &ctx);
    let tops: Vec<f32> = boxes.iter().map(|b| rect(*b).min.y).collect();
    assert!(
        tops.iter().all(|y| close(*y, tops[0])),
        "one line: {tops:?}"
    );
    assert!(close(rect(boxes[0]).width(), 30.0), "{:?}", rect(boxes[0]));
}

#[test]
fn the_room_a_fit_row_keeps_by_its_padding_counts() {
    let (_dir, app) = app_with(BOXES);
    // 350 wide, 100 of it kept at the end: 250 left, so the first word.
    let padding = toml::Value::Array(vec![0.0.into(), 0.0.into(), 100.0.into(), 0.0.into()]);
    let (_page, _row, boxes) = bar_of(&app, 350.0, padding);
    let ctx = egui::Context::default();
    settled(&app, &ctx);
    assert!(close(rect(boxes[0]).width(), 60.0), "{:?}", rect(boxes[0]));
}

#[test]
fn a_spring_in_a_fit_row_gives_way_before_any_word_is_taken() {
    let (_dir, app) = app_with(BOXES);
    let (_page, row, boxes) = bar_of(&app, 320.0, toml::Value::Float(0.0));
    let spring = add_child_widget(
        &app,
        row,
        "spring",
        &toml::toml! { kind = "row" grow = 1.0 }.into(),
    );
    let ctx = egui::Context::default();
    settled(&app, &ctx);
    assert!(close(rect(boxes[0]).width(), 100.0), "{:?}", rect(boxes[0]));
    assert!(close(rect(spring).width(), 20.0), "{:?}", rect(spring));
}

#[test]
fn a_column_fits_by_its_height_and_a_widget_s_own_fit_wins() {
    let (_dir, app) =
        app_with("[roles.y]\nwidth = 10.0\nheight = 100.0\n[roles.y.wf_one]\nheight = 50.0\n");
    let page = add_widget(
        &app,
        &toml::toml! {
            kind = "row" theme = "themes/t.toml" x = 0.0 y = 0.0 width = 100.0 height = 200.0
            padding = 0.0 widget_layout = { align_items = "stretch" }
        }
        .into(),
    );
    let column = add_child_widget(
        &app,
        page,
        "column",
        &toml::toml! {
            kind = "column" padding = 0.0 widget_layout = { gap = [0.0, 0.0], fit = ["wf_one"] }
        }
        .into(),
    );
    let first = add_child_widget(
        &app,
        column,
        "first",
        &toml::toml! { kind = "label" text = "" role = "y" }.into(),
    );
    for i in 0..2 {
        add_child_widget(
            &app,
            column,
            &format!("more{i}"),
            &toml::toml! { kind = "label" text = "" role = "y" }.into(),
        );
    }
    let ctx = egui::Context::default();
    settled(&app, &ctx);
    assert!(close(rect(first).height(), 50.0), "{:?}", rect(first));
}

#[test]
fn a_theme_s_show_text_draws_a_button_without_its_caption() {
    let (_dir, app) = app_with("[roles.bare]\nshow_text = false\n");
    let row = add_widget(
        &app,
        &toml::toml! { kind = "row" theme = "themes/t.toml" x = 0.0 y = 0.0 padding = 0.0 }.into(),
    );
    let worded = add_child_widget(
        &app,
        row,
        "worded",
        &toml::toml! { kind = "button" text = "A long caption" }.into(),
    );
    let bare = add_child_widget(
        &app,
        row,
        "bare",
        &toml::toml! { kind = "button" text = "A long caption" role = "bare" }.into(),
    );
    let ctx = egui::Context::default();
    settled(&app, &ctx);
    assert!(
        rect(bare).width() + 40.0 < rect(worded).width(),
        "{:?} {:?}",
        rect(bare),
        rect(worded)
    );
}

#[test]
fn a_theme_s_truncate_lets_a_caption_give_way_in_its_row() {
    let (_dir, app) = app_with("[roles.cut]\ntruncate = true\n");
    let page = add_widget(
        &app,
        &toml::toml! {
            kind = "column" theme = "themes/t.toml" x = 0.0 y = 0.0 width = 120.0 padding = 0.0
        }
        .into(),
    );
    let row = add_child_widget(
        &app,
        page,
        "row",
        &toml::toml! { kind = "row" width_percent = 100.0 padding = 0.0 widget_layout = { gap = [0.0, 0.0] } }
            .into(),
    );
    let text = "A caption far longer than the row it sits in";
    let cut = add_child_widget(
        &app,
        row,
        "cut",
        &toml::toml! { kind = "label" text = text role = "cut" }.into(),
    );
    let ctx = egui::Context::default();
    settled(&app, &ctx);
    assert!(rect(cut).width() <= 120.6, "{:?}", rect(cut));
}

#[test]
fn a_table_named_for_a_word_no_project_declares_is_refused() {
    let (_dir, app) = app_with("");
    let node = add_widget(&app, &toml::toml! { kind = "row" }.into());
    let declared = toml::toml! { wf_compact = { width = 40.0 } };
    balaur::components::patch(&app.engine, node, "widget", &declared.into())
        .expect("a declared word's table");
    let undeclared = toml::toml! { wf_nowhere = { width = 40.0 } };
    let refused = balaur::components::patch(&app.engine, node, "widget", &undeclared.into());
    assert!(
        refused.is_err_and(|why| format!("{why:#}").contains("wf_nowhere")),
        "an undeclared word"
    );
    // A scene naming one in `classes` is refused too; a script's write of
    // one is said once and leaves it out.
    let other = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Other", app.engine.root());
    let named: toml::Value = toml::toml! { kind = "row" classes = ["wf_nowhere"] }.into();
    assert!(balaur::components::add(&app.engine, other, "widget", Some(&named)).is_err());
}

/// Three boxes of role `x`, 100 by 10, each word narrower; a fit row that
/// climbs all three to fit 100.
const LADDER: &str = "[roles.x]\nwidth = 100.0\nheight = 10.0\n\
    [roles.x.wf_compact]\nwidth = 80.0\n\
    [roles.x.wf_one]\nwidth = 60.0\n\
    [roles.x.wf_two]\nwidth = 30.0\n\
    [roles.bar]\nfit = [\"wf_compact\", \"wf_one\", \"wf_two\"]\n";

#[test]
fn a_fit_row_settles_every_step_before_its_frame_draws() {
    let (_dir, app) = app_with(LADDER);
    let (_page, _row, boxes) = bar_of(&app, 100.0, toml::Value::Float(0.0));
    let ctx = egui::Context::default();
    // The first frame installs the fonts.
    pass(&app, &ctx, vec![]);
    // One frame as a host runs it: egui reruns the pass while one asks.
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(640.0, 480.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_pass(input, |ctx| balaur_ui::run_pass(&app.engine, ctx));
    // A renderer uploads these; unapplied, dropping them panics.
    out.textures_delta.clear();
    assert!(
        close(rect(boxes[2]).width(), 30.0),
        "the last word, in the frame that drew: {:?}",
        rect(boxes[2])
    );
}
