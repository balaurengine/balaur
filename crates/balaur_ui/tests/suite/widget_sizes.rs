//! One layout owner: every kind is drawn in the box it measured, so a row
//! sized from the measure holds what is painted in it (PLAN-ui-gaps §2).

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;

/// Each leaf kind, as a scene writes it with nothing sizing it.
fn kinds() -> Vec<toml::Value> {
    vec![
        toml::toml! { kind = "label" text = "Hello there" }.into(),
        toml::toml! { kind = "button" text = "Press me" }.into(),
        toml::toml! { kind = "text_field" text = "typed" }.into(),
        toml::toml! { kind = "text_field" text = "a longer line than a dozen letters" widget_field = { clip_text = false } }.into(),
        toml::toml! { kind = "text_field" text = "12" widget_field = { prefix = "$", suffix = "kg" } }.into(),
        toml::toml! { kind = "text_area" text = "one\ntwo" }.into(),
        toml::toml! { kind = "checkbox" text = "Music" }.into(),
        toml::toml! { kind = "switch" }.into(),
        toml::toml! { kind = "dropdown" text = "One" widget_dropdown = { options = ["One", "Two", "Three"] } }.into(),
        toml::toml! { kind = "menu" text = "File" widget_menu = { options = ["Open", "Save"] } }.into(),
        toml::toml! { kind = "slider" widget_range = { min = 0.0, max = 10.0 } }.into(),
        toml::toml! { kind = "number_field" widget_range = { value = 2.0 } }.into(),
        toml::toml! { kind = "number_field" widget_range = { value = 11.0, min = 0.0, max = 12.0, arrows = true, prefix = "W", suffix = "px" } }.into(),
        toml::toml! { kind = "progress_bar" text = "Loading" widget_range = { value = 0.5 } }.into(),
        toml::toml! { kind = "separator" }.into(),
        toml::toml! { kind = "color_picker" }.into(),
        toml::toml! { kind = "code" text = "fn go() {\n    x = 1;\n}" }.into(),
        toml::toml! { kind = "fold" text = "Advanced" widget_fold = { open = false } }.into(),
        toml::toml! { kind = "image" widget_image = { alt_text = "no picture", image = "missing.png" } }.into(),
        toml::toml! { kind = "color_picker" widget_color_picker = { inline = true } }.into(),
        toml::toml! { kind = "list" width = 160.0 height = 120.0 widget_rows = { items = [{ label = "first" }, { label = "second" }] } }.into(),
        toml::toml! { kind = "panel" text = "Title" }.into(),
        toml::toml! { kind = "window" text = "Debug" width = 200.0 height = 120.0 }.into(),
    ]
}

#[test]
fn every_kind_draws_in_the_box_it_measured() {
    let mut wrong = Vec::new();
    for params in kinds() {
        let (_dir, app) = app();
        let column = add_widget(
            &app,
            &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into(),
        );
        let child = add_child_widget(&app, column, "Kind", &params);
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        pass(&app, &ctx, vec![]);
        let given = balaur_ui::widget_rect(child).expect("the kind drew").size();
        let drawn = balaur_ui::widget_drawn_size(child).expect("the kind drew");
        if (given - drawn).abs().max_elem() > 0.5 {
            wrong.push(format!("{params}: measured {given:?}, drew {drawn:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// A container with what it holds, as a scene writes it with nothing sizing
// it: its box is what it measured, children and all.
#[test]
fn every_container_draws_in_the_box_it_measured() {
    let label = || -> toml::Value { toml::toml! { kind = "label" text = "Inside" }.into() };
    let holders: Vec<(toml::Value, Vec<toml::Value>)> = vec![
        (
            toml::toml! { kind = "fold" text = "Advanced" widget_fold = { open = true } }.into(),
            vec![label()],
        ),
        (
            toml::toml! { kind = "row" }.into(),
            vec![label(), toml::toml! { kind = "button" text = "Go" }.into()],
        ),
        (
            toml::toml! { kind = "panel" text = "Title" }.into(),
            vec![label()],
        ),
        (toml::toml! { kind = "tabs" }.into(), vec![label(), label()]),
        (toml::toml! { kind = "stack" }.into(), vec![label()]),
        (
            toml::toml! { kind = "grid" }.into(),
            vec![label(), label(), label()],
        ),
        (toml::toml! { kind = "flow" }.into(), vec![label(), label()]),
        (
            toml::toml! { kind = "window" text = "Debug" }.into(),
            vec![label()],
        ),
        (
            toml::toml! { kind = "scroll" width = 120.0 height = 40.0 }.into(),
            vec![label(), label(), label()],
        ),
        (
            toml::toml! { kind = "menu" text = "File" }.into(),
            vec![toml::toml! { kind = "button" text = "Open" }.into()],
        ),
        (
            toml::toml! { kind = "panel" text = "A title wider than what it holds" }.into(),
            vec![label()],
        ),
    ];
    let mut wrong = Vec::new();
    for (params, children) in holders {
        let (_dir, app) = app();
        let column = add_widget(
            &app,
            &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into(),
        );
        let holder = add_child_widget(&app, column, "Holder", &params);
        for (slot, child) in children.iter().enumerate() {
            add_child_widget(&app, holder, &format!("Child{slot}"), child);
        }
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        pass(&app, &ctx, vec![]);
        let given = balaur_ui::widget_rect(holder).map(|rect| rect.size());
        let drawn = balaur_ui::widget_drawn_size(holder);
        let agree = given
            .zip(drawn)
            .is_some_and(|(given, drawn)| (given - drawn).abs().max_elem() <= 0.5);
        if !agree {
            wrong.push(format!("{params}: measured {given:?}, drew {drawn:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
