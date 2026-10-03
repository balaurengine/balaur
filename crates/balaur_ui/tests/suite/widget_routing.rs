//! Whose the pointer is: the UI's, from the top layer down, or the world's
//! under a widget that lets it through.

use balaur_core::App;
use balaur_core::hecs::Entity;
use balaur_input::InputSnapshot;
use egui::pos2;

use crate::support::{add_child_widget, add_widget, app, settle};

/// A screen-filling row that lets the pointer through, a docked panel with a
/// seam beside it, and a stage that lets it through too, with a button on it.
fn shell(app: &App) -> (Entity, Entity) {
    let root = add_widget(
        app,
        &toml::toml! { kind = "row" anchor = "fill" interactive = false splitter_width = 8.0 gap = [4.0, 4.0] }
            .into(),
    );
    let dock = add_child_widget(
        app,
        root,
        "Dock",
        &toml::toml! { kind = "panel" width = 150.0 }.into(),
    );
    let stage = add_child_widget(
        app,
        root,
        "Stage",
        &toml::toml! { kind = "column" grow = 1 interactive = false }.into(),
    );
    let button = add_child_widget(
        app,
        stage,
        "Go",
        &toml::toml! { kind = "button" text = "Go" }.into(),
    );
    (dock, button)
}

fn centre(entity: Entity) -> egui::Pos2 {
    balaur_ui::widget_rect(entity)
        .expect("the widget drew")
        .center()
}

#[test]
fn a_point_is_the_ui_s_over_a_widget_or_a_seam_and_the_world_s_elsewhere() {
    let (_dir, app) = app();
    let (dock, button) = shell(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let takes = |at| balaur_ui::takes_point(&app.engine, at);
    assert!(takes(centre(dock)), "the docked panel takes the pointer");
    assert!(takes(centre(button)), "and so does a button on the stage");
    let edge = balaur_ui::widget_rect(dock).unwrap().right() + 2.0;
    assert!(
        takes(pos2(edge, 240.0)),
        "the seam beside the dock is a handle"
    );
    assert!(
        !takes(pos2(500.0, 400.0)),
        "the open stage lets the pointer through to the world"
    );
    // A frame the pacing skipped still runs egui, which then forgets what
    // was visible; the picture on screen is still the last pass's.
    ctx.begin_pass(egui::RawInput::default());
    ctx.end_pass().textures_delta.clear();
    assert!(
        takes(centre(dock)),
        "a skipped pass leaves the dock taking the pointer"
    );
}

/// A press, a move and a release written into the snapshot, one frame each.
fn frame(app: &App, at: egui::Pos2, button: Option<bool>) -> bool {
    {
        let input = app.engine.resource::<InputSnapshot>();
        let mut input = input.borrow_mut();
        input.begin_frame();
        input.set_mouse_pos(at.x, at.y);
        if let Some(down) = button {
            input.mouse_button_event(0, down);
        }
    }
    balaur_ui::pointer_is_ui(&app.engine)
}

#[test]
fn a_press_keeps_the_layer_it_went_to_until_it_is_up() {
    let (_dir, app) = app();
    let (_, button) = shell(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let open = pos2(500.0, 400.0);
    assert!(
        frame(&app, centre(button), Some(true)),
        "pressed on the button"
    );
    assert!(frame(&app, open, None), "dragged off it, still the UI's");
    assert!(
        frame(&app, open, Some(false)),
        "and its release is the UI's"
    );
    assert!(
        !frame(&app, open, None),
        "up, the open stage is the world's"
    );

    assert!(!frame(&app, open, Some(true)), "pressed on the stage");
    assert!(
        !frame(&app, centre(button), None),
        "dragged over the button, still the world's"
    );
    assert!(!frame(&app, centre(button), Some(false)));
    assert!(
        frame(&app, centre(button), None),
        "up, the button is the UI's again"
    );
}

#[test]
fn an_open_dialog_takes_every_point() {
    let (_dir, app) = app();
    shell(&app);
    add_widget(
        &app,
        &toml::toml! { kind = "dialog" text = "Sure?" width = 200.0 height = 100.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    assert!(
        balaur_ui::takes_point(&app.engine, pos2(620.0, 460.0)),
        "a corner far from the dialog is behind its dim"
    );
}

#[test]
fn a_popup_the_widget_layer_did_not_draw_takes_its_own_area() {
    let (_dir, app) = app();
    shell(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    // An egui area over the stage in the same frame as the pass, as a combo
    // box's list opens one, drawn twice so egui knows its size.
    for _ in 0..2 {
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                pos2(0.0, 0.0),
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        });
        egui::Area::new(egui::Id::new("popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(400.0, 300.0))
            .show(&ctx, |ui| {
                ui.set_min_size(egui::vec2(120.0, 80.0));
                ui.label("popup");
            });
        balaur_ui::run_pass(&app.engine, &ctx);
        ctx.end_pass().textures_delta.clear();
    }
    assert!(
        balaur_ui::takes_point(&app.engine, pos2(450.0, 330.0)),
        "over the popup"
    );
    assert!(
        !balaur_ui::takes_point(&app.engine, pos2(300.0, 420.0)),
        "beside it, the stage"
    );
}
