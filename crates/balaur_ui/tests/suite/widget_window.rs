//! The `window` kind: dragged by its title bar, shut by its cross.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use egui::pos2;

fn property(app: &balaur_core::App, entity: balaur_core::hecs::Entity, key: &str) -> toml::Value {
    balaur::components::get(&app.engine, entity, "widget")
        .expect("the widget component is still on the node")
        .get(key)
        .cloned()
        .unwrap_or_else(|| panic!("the widget has no `{key}`"))
}

fn number(app: &balaur_core::App, entity: balaur_core::hecs::Entity, key: &str) -> f64 {
    property(app, entity, key)
        .as_float()
        .unwrap_or_else(|| panic!("{key} is not a number"))
}

#[test]
fn a_window_moves_with_its_title_bar_and_shuts_on_its_cross() {
    let (_dir, mut app) = app();
    let window = add_widget(
        &app,
        &toml::toml! { kind = "window" text = "Debug" movable = true x = 100.0 y = 100.0 width = 200.0 height = 120.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);

    // Drag the title 30 right and 20 down.
    let grab = pos2(118.0, 112.0);
    pass(&app, &ctx, vec![egui::Event::PointerMoved(grab)]);
    pass(&app, &ctx, press(grab, true));
    pass(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(pos2(
            grab.x + 15.0,
            grab.y + 10.0,
        ))],
    );
    pass(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(pos2(
            grab.x + 30.0,
            grab.y + 20.0,
        ))],
    );
    pass(&app, &ctx, press(pos2(grab.x + 30.0, grab.y + 20.0), false));
    consume_input(&mut app);
    assert!(
        (number(&app, window, "x") - 130.0).abs() < 2.0
            && (number(&app, window, "y") - 120.0).abs() < 2.0,
        "the drag moved the window with the pointer: {}, {}",
        number(&app, window, "x"),
        number(&app, window, "y")
    );

    settle(&app, &ctx);
    let drawn = pass(&app, &ctx, vec![]);
    let cross = drawn
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == "×" => {
                Some(text.pos + text.galley.size() / 2.0)
            }
            _ => None,
        })
        .expect("the title bar draws its cross");
    pass(&app, &ctx, press(cross, true));
    pass(&app, &ctx, press(cross, false));
    consume_input(&mut app);
    assert_eq!(
        crate::support::beside_property(&app, window, "widget_window", "open").as_bool(),
        Some(false),
        "the cross shuts the window"
    );
}

// A dialog dims what is behind it unless it says clear: `backdrop_color`
// with no alpha is no dim, left unset it is its theme's.
#[test]
fn a_clear_backdrop_dims_nothing() {
    for (asked, dims) in [
        (
            toml::toml! { kind = "dialog" text = "Sure?" width = 200.0 height = 100.0 },
            true,
        ),
        (
            toml::toml! { kind = "dialog" text = "Sure?" width = 200.0 height = 100.0 widget_window = { backdrop_color = [0.0, 0.0, 0.0, 0.0] } },
            false,
        ),
    ] {
        let (_dir, app) = app();
        add_widget(&app, &asked.into());
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let out = pass(&app, &ctx, vec![]);
        let dimmed = rects(&out)
            .iter()
            .any(|(rect, fill)| rect.width() >= 640.0 && fill.a() > 0);
        assert_eq!(dimmed, dims, "dimmed: {dimmed}, asked for {dims}");
    }
}

// A dialog open in a played game dims and holds the game's viewport alone:
// the editor's shell beside it still takes clicks, the game's own controls
// under the dim do not, and a click on the dim shuts a dismissable dialog.
#[test]
fn a_dialog_in_a_viewport_holds_the_viewport_and_leaves_the_rest_clickable() {
    let (_dir, mut app) = app();
    let shell = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "Shell" x = 20.0 y = 20.0 width = 80.0 height = 30.0 }.into(),
    );
    let screen = add_widget(
        &app,
        &toml::toml! { kind = "viewport" x = 300.0 y = 100.0 width = 300.0 height = 240.0 }.into(),
    );
    let under = add_child_widget(
        &app,
        screen,
        "Under",
        &toml::toml! { kind = "button" text = "Game" x = 10.0 y = 10.0 width = 80.0 height = 30.0 }
            .into(),
    );
    let dialog = add_child_widget(
        &app,
        screen,
        "Ask",
        &toml::toml! { kind = "dialog" text = "Sure?" width = 100.0 height = 60.0 widget_window = { open = true } }
            .into(),
    );
    let inside = add_child_widget(
        &app,
        dialog,
        "Yes",
        &toml::toml! { kind = "button" text = "Yes" width = 60.0 height = 24.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let click_at = |app: &mut balaur_core::App, at: egui::Pos2| {
        pass(app, &ctx, press(at, true));
        pass(app, &ctx, press(at, false));
        consume_input(app);
    };
    click_at(
        &mut app,
        balaur_ui::widget_rect(shell).expect("drawn").center(),
    );
    assert!(
        clicked(&app, shell),
        "the shell beside the viewport took no click"
    );
    assert!(
        !balaur_ui::takes_point(&app.engine, pos2(150.0, 400.0)),
        "a point in the window outside the viewport is behind no dim"
    );
    for _ in 0..3 {
        pass(&app, &ctx, vec![]);
    }
    click_at(
        &mut app,
        balaur_ui::widget_rect(inside).expect("drawn").center(),
    );
    assert!(
        clicked(&app, inside),
        "the dialog's own button sat under its dim"
    );
    click_at(
        &mut app,
        balaur_ui::widget_rect(under).expect("drawn").center(),
    );
    assert!(
        !clicked(&app, under),
        "a control under the dim took a click"
    );
    assert_eq!(
        crate::support::beside_property(&app, dialog, "widget_window", "open").as_bool(),
        Some(false),
        "a click on the dim shuts the dialog"
    );
}

// Escape is the viewport's dialog's only while the pointer is over the
// viewport: over the shell beside it, the key is the shell's.
#[test]
fn escape_shuts_a_viewport_dialog_only_with_the_pointer_over_the_viewport() {
    let (_dir, mut app) = app();
    let screen = add_widget(
        &app,
        &toml::toml! { kind = "viewport" x = 300.0 y = 100.0 width = 300.0 height = 240.0 }.into(),
    );
    let dialog = add_child_widget(
        &app,
        screen,
        "Ask",
        &toml::toml! { kind = "dialog" text = "Sure?" width = 100.0 height = 60.0 widget_window = { open = true } }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let open = |app: &balaur_core::App| {
        crate::support::beside_property(app, dialog, "widget_window", "open").as_bool()
    };
    for (at, shut) in [(pos2(20.0, 20.0), false), (pos2(320.0, 120.0), true)] {
        pass(&app, &ctx, vec![egui::Event::PointerMoved(at)]);
        pass(&app, &ctx, key(egui::Key::Escape));
        consume_input(&mut app);
        assert_eq!(open(&app), Some(!shut), "Escape with the pointer at {at:?}");
    }
}
