//! What a label's markup can do beyond its look: the spans a click reports,
//! the spans that say something on hover, and the text a player may take.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use egui::pos2;

/// Every rect the pass painted, for finding a selection behind the glyphs.
fn rects(out: &egui::FullOutput) -> Vec<egui::Rect> {
    out.shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect) => Some(rect.rect),
            _ => None,
        })
        .collect()
}

/// A `[url]` span reports its target to the script, and the click is the
/// span's rather than the label's.
#[test]
fn a_url_span_calls_on_link_with_its_target() {
    let script = "pub fn init(this) {\n    this.went = \"\";\n}\n\
                  pub fn on_go(this, target) {\n    this.went = target;\n}\n\
                  pub fn went(this) {\n    this.went\n}\n";
    let (_dir, mut app) = app_with_script(script);
    let root = app.engine.root();
    let owner = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Owner", root);
    let host = app.engine.script_host().unwrap();
    host.attach(balaur::node_id_of(owner), "scripts/paint.rn")
        .unwrap();
    let label = toml::toml! {
        kind = "label" markup = true on_link = "on_go" x = 10.0 y = 10.0
        text = "read the [url=docs/ui]manual[/url] first"
    };
    let entity = add_child_widget(&app, owner, "Label", &label.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    // Over the linked word, which sits about halfway along the line.
    let rect = balaur_ui::widget_rect(entity).expect("the label drew");
    let at = pos2(rect.min.x + rect.width() * 0.55, rect.center().y);
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    consume_input(&mut app);
    assert_eq!(
        host.call_on(balaur::node_id_of(owner), "went", &[]),
        Some(balaur_script::Value::Str("docs/ui".into())),
        "the click on the link did not reach the handler with its target"
    );
}

/// A click away from the link leaves the handler alone: the span is what was
/// clicked, not the label around it.
#[test]
fn a_click_beside_a_url_span_reports_nothing() {
    let script = "pub fn init(this) {\n    this.went = \"\";\n}\n\
                  pub fn on_go(this, target) {\n    this.went = target;\n}\n\
                  pub fn went(this) {\n    this.went\n}\n";
    let (_dir, mut app) = app_with_script(script);
    let root = app.engine.root();
    let owner = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Owner", root);
    let host = app.engine.script_host().unwrap();
    host.attach(balaur::node_id_of(owner), "scripts/paint.rn")
        .unwrap();
    let label = toml::toml! {
        kind = "label" markup = true on_link = "on_go" x = 10.0 y = 10.0
        text = "read the [url=docs/ui]manual[/url] first"
    };
    let entity = add_child_widget(&app, owner, "Label", &label.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    // On the first word, which is not the link.
    let rect = balaur_ui::widget_rect(entity).expect("the label drew");
    let at = pos2(rect.min.x + 4.0, rect.center().y);
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    consume_input(&mut app);
    assert_eq!(
        host.call_on(balaur::node_id_of(owner), "went", &[]),
        Some(balaur_script::Value::Str(String::new())),
        "a click on the plain text reported a link"
    );
}

/// A `[hint]` span says its text while the pointer rests on it.
#[test]
fn a_hint_span_shows_its_text_on_hover() {
    let (_dir, app) = app();
    let label = toml::toml! {
        kind = "label" markup = true x = 10.0 y = 10.0
        text = "costs [hint=one a second]stamina[/hint] to run"
    };
    let entity = add_widget(&app, &label.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = balaur_ui::widget_rect(entity).expect("the label drew");
    let at = pos2(rect.min.x + rect.width() * 0.4, rect.center().y);
    pass_at(&app, &ctx, vec![egui::Event::PointerMoved(at)], Some(1.0));
    let mut shown = pass_at(&app, &ctx, vec![], Some(1.1));
    // A tooltip waits for the pointer to rest, which is time this pass owns.
    for tick in 2..14 {
        shown = pass_at(&app, &ctx, vec![], Some(1.0 + f64::from(tick) * 0.25));
    }
    assert!(
        texts(&shown).iter().any(|(text, _)| text == "one a second"),
        "the hint said nothing: {:?}",
        texts(&shown)
    );
}

/// A drag over a selectable label marks what it crossed, and the copy key
/// takes that text and no more.
#[test]
fn a_drag_over_a_selectable_label_selects_and_copies() {
    let (_dir, app) = app();
    let label = toml::toml! {
        kind = "label" selectable = true x = 10.0 y = 10.0 text = "Balaur engine"
    };
    let entity = add_widget(&app, &label.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let before = rects(&pass(&app, &ctx, vec![])).len();
    let rect = balaur_ui::widget_rect(entity).expect("the label drew");
    let from = pos2(rect.min.x + 1.0, rect.center().y);
    let to = pos2(rect.min.x + rect.width() * 0.5, rect.center().y);
    pass(&app, &ctx, press(from, true));
    pass(&app, &ctx, vec![egui::Event::PointerMoved(to)]);
    let dragged = pass(&app, &ctx, vec![egui::Event::PointerMoved(to)]);
    assert!(
        rects(&dragged).len() > before,
        "the drag painted no selection"
    );
    let copied = pass(
        &app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::C,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
    );
    let taken: Vec<String> = copied
        .platform_output
        .commands
        .iter()
        .filter_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(taken.len(), 1, "the copy key took nothing: {taken:?}");
    assert!(
        "Balaur engine".starts_with(taken[0].as_str()) && !taken[0].is_empty(),
        "the copy took {:?}, which is not what the drag crossed",
        taken[0]
    );
}

/// A `drag_value` with arrows steps by `step` and stops at `max`, and its
/// `suffix` follows the number the way `placeholder` leads it.
#[test]
fn a_drag_value_steps_from_its_arrows_and_wears_its_suffix() {
    let (_dir, mut app) = app();
    let params = toml::toml! {
        kind = "drag_value" value = 11.0 min = 0.0 max = 12.0 step = 1.0
        arrows = true placeholder = "W" suffix = "px" x = 10.0 y = 10.0
    };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let shown = texts(&pass(&app, &ctx, vec![]));
    assert!(
        shown.iter().any(|(text, _)| text.contains("px")),
        "the number wears no suffix: {shown:?}"
    );
    let up = shown
        .iter()
        .find(|(text, _)| text == "⏶")
        .map(|(_, at)| *at)
        .expect("the arrows drew");
    let at = egui::pos2(up.x + 4.0, up.y + 4.0);
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "value"),
        toml::Value::Float(12.0),
        "the up arrow did not step the number"
    );
    // Again, against the ceiling this time.
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "value"),
        toml::Value::Float(12.0),
        "the step ran past `max`"
    );
}

/// A wrapping label is as tall as its box is narrow, and the row hugging it
/// takes that height rather than one line's: the settings screen's help text
/// ran under the row below it.
#[test]
fn a_row_hugging_a_wrapped_label_is_as_tall_as_the_wrap() {
    let (_dir, app) = app();
    let column = toml::toml! { kind = "column" x = 0.0 y = 0.0 width = 300.0 height = 400.0 };
    let host = add_widget(&app, &column.into());
    let row = toml::toml! { kind = "row" };
    let row = add_child_widget(&app, host, "Row", &row.into());
    let cells = toml::toml! { kind = "row" grow = 1.0 };
    let cells = add_child_widget(&app, row, "Cells", &cells.into());
    let label = toml::toml! {
        kind = "label" grow = 1.0 wrap = true
        text = "A project-relative picture shown over the first frames, on every target."
    };
    let label = add_child_widget(&app, cells, "Note", &label.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let said = balaur_ui::widget_rect(label).expect("the label drew");
    let box_ = balaur_ui::widget_rect(row).expect("the row drew");
    assert!(
        said.height() > 20.0,
        "the label wrapped to more than one line: {said:?}"
    );
    assert!(
        box_.height() >= said.height(),
        "the row is as tall as what it holds: row {box_:?} label {said:?}"
    );
}
