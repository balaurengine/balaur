//! Widget input while a recording plays: the recording's clicks are the ones
//! that land, and what the draw saw meanwhile is not kept for later.

use balaur_core::replay::{self, Session};
use egui::{Modifiers, pos2};

use crate::support::{
    add_child_widget, add_widget, app, click_held, clicked, consume_input, settle,
};

#[test]
fn a_click_drawn_while_a_recording_plays_does_not_land_after_it() {
    let (dir, mut app) = app();
    let params = toml::toml! {
        kind = "button" text = "Go" x = 0.0 y = 0.0 width = 100.0 height = 40.0
    };
    let button = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let on_it = pos2(20.0, 20.0);
    click_held(&app, &ctx, on_it, Modifiers::NONE);
    consume_input(&mut app);
    assert!(
        clicked(&app, button),
        "the draw's click lands on a live tick"
    );

    let session = dir.path().join("session.blr");
    replay::start_recording(&app.engine, &session, ".", "", false).unwrap();
    for _ in 0..3 {
        app.advance(1.0 / 60.0);
    }
    replay::stop_recording(&app.engine, "test");

    replay::begin(&app.engine, Session::read(&session).unwrap());
    replay::play(&app.engine);
    let mut replayed = 0;
    while replay::is_running(&app.engine) {
        click_held(&app, &ctx, on_it, Modifiers::NONE);
        app.advance(1.0 / 60.0);
        assert!(
            !clicked(&app, button),
            "a replayed tick took the draw's click"
        );
        replayed += 1;
    }
    assert_eq!(replayed, 3, "the replay ran every recorded tick");
    replay::end(&app.engine);
    consume_input(&mut app);
    assert!(
        !clicked(&app, button),
        "a click drawn during the replay landed after it"
    );
}

// The editor plays its game in one subtree, the one `debugger.set_scope`
// names, and replays only that: its own widgets beside it take the pointer.
#[test]
fn a_widget_outside_the_replayed_scope_takes_clicks_while_a_recording_plays() {
    let (dir, mut app) = app();
    let root = app.engine.root();
    let game = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Game", root);
    app.engine.set_debug_scope(Some(game));
    let tool = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "Tool" x = 0.0 y = 0.0 width = 100.0 height = 40.0 }
            .into(),
    );
    let played = add_child_widget(
        &app,
        game,
        "Go",
        &toml::toml! { kind = "button" text = "Go" x = 0.0 y = 100.0 width = 100.0 height = 40.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let (on_tool, on_played) = (pos2(20.0, 20.0), pos2(20.0, 120.0));
    click_held(&app, &ctx, on_played, Modifiers::NONE);
    consume_input(&mut app);
    assert!(
        clicked(&app, played),
        "the game's button takes a live click"
    );

    let session = dir.path().join("session.blr");
    replay::start_recording(&app.engine, &session, ".", "", false).unwrap();
    for _ in 0..3 {
        app.advance(1.0 / 60.0);
    }
    replay::stop_recording(&app.engine, "test");

    replay::begin(&app.engine, Session::read(&session).unwrap());
    replay::play(&app.engine);
    let mut tool_clicks = 0;
    while replay::is_running(&app.engine) {
        click_held(&app, &ctx, on_tool, Modifiers::NONE);
        click_held(&app, &ctx, on_played, Modifiers::NONE);
        app.advance(1.0 / 60.0);
        tool_clicks += usize::from(clicked(&app, tool));
        assert!(
            !clicked(&app, played),
            "a replayed game took the draw's click"
        );
    }
    assert_eq!(tool_clicks, 3, "the tool beside the game took every click");
}
