//! The components beside `widget`: which holds a row, what a node without
//! one reads, and how a reader's edit lands on one.

use balaur_script::Value;

use crate::support::{add_widget, app, beside_property};

fn add(app: &balaur_core::App, params: &toml::Value) -> anyhow::Result<()> {
    let root = app.engine.root();
    let entity = balaur::scene::spawn_node(&mut app.engine.world_mut(), "W", root);
    balaur::components::add(&app.engine, entity, "widget", Some(params))
}

#[test]
fn a_row_moved_beside_the_widget_is_refused_on_it_by_name() {
    let (_dir, app) = app();
    let refused = add(&app, &toml::toml! { kind = "row" gap = [1.0, 1.0] }.into())
        .expect_err("`gap` is `widget_layout`'s");
    assert!(format!("{refused:#}").contains("`gap`"), "{refused:#}");
}

#[test]
fn a_class_row_a_kind_reads_nothing_of_is_refused() {
    let (_dir, app) = app();
    let refused = add(
        &app,
        &toml::toml! {
            kind = "label"
            [narrow]
            gap = [1.0, 1.0]
        }
        .into(),
    )
    .expect_err("a label lays nothing out");
    assert!(format!("{refused:#}").contains("narrow.gap"), "{refused:#}");
}

#[test]
fn a_class_row_of_the_wrong_shape_is_refused() {
    let (_dir, app) = app();
    let refused = add(
        &app,
        &toml::toml! {
            kind = "row"
            [narrow]
            gap = 4.0
        }
        .into(),
    )
    .expect_err("`gap` is a pair");
    assert!(format!("{refused:#}").contains("narrow.gap"), "{refused:#}");
}

#[test]
fn a_kind_written_alone_reads_what_the_new_kind_reads() {
    let (_dir, app) = app();
    let entity = add_widget(&app, &toml::toml! { kind = "label" }.into());
    balaur::components::set_property(
        &app.engine,
        entity,
        "widget",
        "kind",
        &toml::Value::String("slider".into()),
    )
    .unwrap();
    let world = app.engine.world();
    let widget = world.get::<&balaur_ui::Widget>(entity).unwrap();
    assert_eq!(widget.kind, "slider");
    assert!(
        (widget.max - 1.0).abs() < f32::EPSILON,
        "the slider has `widget_range`'s default top: {}",
        widget.max
    );
}

#[test]
fn a_component_taken_off_leaves_its_defaults() {
    let (_dir, app) = app();
    let entity = add_widget(
        &app,
        &toml::toml! { kind = "row" widget_layout = { gap = [3.0, 4.0] } }.into(),
    );
    balaur::components::remove(&app.engine, entity, "widget_layout").unwrap();
    let world = app.engine.world();
    let widget = world.get::<&balaur_ui::Widget>(entity).unwrap();
    assert!(
        widget
            .gap
            .iter()
            .all(|axis| (axis + 1.0).abs() < f32::EPSILON),
        "the theme's gap again: {:?}",
        widget.gap
    );
}

#[test]
fn one_row_written_leaves_the_other_rows_on_the_widget() {
    let (_dir, app) = app();
    let entity = add_widget(
        &app,
        &toml::toml! {
            kind = "list"
            widget_rows = { items = [{ label = "a" }, { label = "b" }], row_height = 30.0 }
        }
        .into(),
    );
    balaur::components::set_property(
        &app.engine,
        entity,
        "widget_rows",
        "selection",
        &toml::Value::Array(vec![toml::Value::String("b".into())]),
    )
    .unwrap();
    let world = app.engine.world();
    let widget = world.get::<&balaur_ui::Widget>(entity).unwrap();
    assert_eq!(widget.selection, ["b"]);
    assert!(
        (widget.row_height - 30.0).abs() < f32::EPSILON,
        "the row written left the pitch the scene gave"
    );
}

#[test]
fn a_value_a_kind_does_not_take_adds_nothing_beside_it() {
    let (_dir, mut app) = app();
    let entity = add_widget(&app, &toml::toml! { kind = "label" }.into());
    assert!(balaur_ui::edit(&app.engine, entity, &Value::Num(3.0)));
    app.tick(1.0 / 60.0);
    assert!(!balaur::components::has(
        &app.engine,
        entity,
        "widget_range"
    ));
}

#[test]
fn a_reader_s_tick_lands_on_a_component_the_node_says_it_carries() {
    let (_dir, mut app) = app();
    let entity = add_widget(&app, &toml::toml! { kind = "checkbox" text = "on" }.into());
    assert!(!balaur::components::has(
        &app.engine,
        entity,
        "widget_check"
    ));
    assert!(balaur_ui::click(&app.engine, entity, false));
    app.tick(1.0 / 60.0);
    assert!(balaur::components::has(&app.engine, entity, "widget_check"));
    assert_eq!(
        beside_property(&app, entity, "widget_check", "checked"),
        toml::Value::Boolean(true)
    );
}
