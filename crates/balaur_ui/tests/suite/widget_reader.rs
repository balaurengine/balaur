//! The keys a `widget` answers without building its whole table.
//!
//! A pooled control asks for one property a frame, and the registry falls back
//! to building all forty keys for any the reader does not cover. That fallback
//! is invisible: it answers correctly and costs a table. So the guard is that
//! every key the reader claims agrees with the whole table, and that the keys
//! the editor reads every frame are claimed at all.

use balaur_core::components;

use crate::support::{add_widget, app};

/// Read every one of these two ways and insist they agree: a reader that
/// drifts from `get` is a control that reports one thing to a script and
/// another to a save.
#[test]
fn every_key_the_reader_answers_agrees_with_the_whole_table() {
    let (_dir, app) = app();
    let entity = add_widget(
        &app,
        &toml::toml! { kind = "text_field" text = "hello" role = "danger" visible = true }.into(),
    );
    let whole = components::get(&app.engine, entity, "widget").expect("the node has a widget");
    let table = whole.as_table().expect("a component is a table");
    let index = components::index_of(&app.engine, "widget").expect("widget is registered");

    let mut answered = 0;
    for (key, from_table) in table {
        if !components::answers_alone(&app.engine, entity, index, key) {
            continue;
        }
        let Some(fast) = components::property_at(&app.engine, entity, index, key) else {
            continue;
        };
        assert_eq!(
            &fast, from_table,
            "`widget.{key}` reads one way on its own and another in the whole table"
        );
        answered += 1;
    }
    assert!(
        answered >= 8,
        "only {answered} keys answered on their own, so this proved almost nothing"
    );
}

/// The keys the editor asks for on every pooled control every frame. Missing
/// `submitted` alone was four in five of the editor's whole-table builds, and
/// nothing said so: the fallback answers correctly and costs forty keys.
#[test]
fn the_keys_a_pooled_control_reads_every_frame_are_answered_on_their_own() {
    let (_dir, app) = app();
    let entity = add_widget(
        &app,
        &toml::toml! { kind = "text_field" text = "" role = "danger" }.into(),
    );
    let index = components::index_of(&app.engine, "widget").expect("widget is registered");
    for key in [
        "submitted",
        "clicked",
        "role",
        "visible",
        "text",
        "value",
        "checked",
    ] {
        assert!(
            components::answers_alone(&app.engine, entity, index, key),
            "`widget.{key}` is read every frame and still builds the whole table"
        );
    }
}

/// The control: a key the reader does not claim reads back through the table,
/// so the assertion above is about the reader and not about `get`.
#[test]
fn a_key_the_reader_does_not_claim_still_answers_from_the_table() {
    let (_dir, app) = app();
    let entity = add_widget(&app, &toml::toml! { kind = "text_field" text = "" }.into());
    let index = components::index_of(&app.engine, "widget").expect("widget is registered");
    assert!(
        !components::answers_alone(&app.engine, entity, index, "on_submit"),
        "this key was claimed since; pick another the reader leaves to the table"
    );
    assert!(
        components::property_at(&app.engine, entity, index, "on_submit").is_some(),
        "a key the reader skips still has to answer"
    );
}
