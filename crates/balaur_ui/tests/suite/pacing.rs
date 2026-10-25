//! What a loop sleeping under `[window] low_processor` is owed.

use std::time::Duration;

use balaur_ui::NextFrame;

use crate::pass::run_with;

fn sleeps_at_most(next: &NextFrame, most: Duration) -> bool {
    matches!(next, NextFrame::Sleep(Some(left)) if *left <= most && *left > most.saturating_sub(Duration::from_secs(5)))
}

#[test]
fn a_ui_nobody_asked_to_redraw_lets_the_loop_sleep() {
    let (_dir, app, ctx, errors) = run_with("");
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(
        balaur_ui::next_frame(&app.engine, &ctx),
        NextFrame::Sleep(None)
    );
}

/// Writing a widget node is what asks for a pass: a handler that changes a
/// label owes the sleeping loop a frame without calling `request_repaint`.
#[test]
fn a_widget_written_owes_the_sleeping_loop_a_frame() {
    let (_dir, app, ctx, errors) = run_with("");
    assert!(errors.is_empty(), "{errors:?}");
    crate::pass::feed(&app, &ctx, Vec::new());
    assert_eq!(
        balaur_ui::next_frame(&app.engine, &ctx),
        NextFrame::Sleep(None)
    );
    let root = app.engine.root();
    let label = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Label", root);
    let params = toml::toml! { kind = "label" text = "saved" };
    balaur::components::add(&app.engine, label, "widget", Some(&params.into())).unwrap();
    assert_eq!(balaur_ui::next_frame(&app.engine, &ctx), NextFrame::Now);
    // Stepped as the loop steps: egui owes a pass or two of its own after a
    // new widget, and then nothing is owed.
    let mut passes = 0;
    while balaur_ui::next_frame(&app.engine, &ctx) == NextFrame::Now && passes < 4 {
        let _ = balaur_ui::wants_pass(&app.engine, &ctx, false);
        crate::pass::feed(&app, &ctx, Vec::new());
        passes += 1;
    }
    assert_eq!(
        balaur_ui::next_frame(&app.engine, &ctx),
        NextFrame::Sleep(None),
        "a written widget kept the loop awake ({passes} passes)"
    );
}

#[test]
fn a_script_s_request_is_owed_a_frame_now() {
    let (_dir, app, ctx, errors) = run_with("ui::request_repaint();");
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(balaur_ui::next_frame(&app.engine, &ctx), NextFrame::Now);
}

#[test]
fn a_request_spent_by_its_own_pass_still_owes_the_next_frame() {
    let (_dir, app, ctx, errors) = run_with("ui::request_repaint();");
    assert!(errors.is_empty(), "{errors:?}");
    balaur_ui::honour_lazy(&app.engine);
    assert!(balaur_ui::wants_pass(&app.engine, &ctx, false));
    assert_eq!(balaur_ui::next_frame(&app.engine, &ctx), NextFrame::Now);
    assert_eq!(
        balaur_ui::next_frame(&app.engine, &ctx),
        NextFrame::Sleep(None)
    );
}

#[test]
fn a_repaint_asked_for_as_another_comes_due_is_kept() {
    let (_dir, app, ctx, errors) = run_with(
        r#"ui::request_repaint(map().with("after", 0.0)); ui::request_repaint(map().with("after", 30.0));"#,
    );
    assert!(errors.is_empty(), "{errors:?}");
    balaur_ui::honour_lazy(&app.engine);
    assert!(balaur_ui::wants_pass(&app.engine, &ctx, false));
    let next = balaur_ui::next_frame(&app.engine, &ctx);
    assert!(sleeps_at_most(&next, Duration::from_secs(30)), "{next:?}");
}

#[test]
fn a_repaint_scheduled_ahead_is_how_long_the_loop_may_sleep() {
    let (_dir, app, ctx, errors) = run_with(r#"ui::request_repaint(map().with("after", 30.0));"#);
    assert!(errors.is_empty(), "{errors:?}");
    let next = balaur_ui::next_frame(&app.engine, &ctx);
    assert!(sleeps_at_most(&next, Duration::from_secs(30)), "{next:?}");
}

#[test]
fn egui_s_own_requests_wake_the_loop_when_due() {
    let (_dir, app, _, _) = run_with("");
    // egui reports only a request sooner than one it already owes, and the
    // passes above left one; a context of its own starts owing nothing. The
    // first ask installs the hook it reports through.
    let ctx = egui::Context::default();
    assert_eq!(
        balaur_ui::next_frame(&app.engine, &ctx),
        NextFrame::Sleep(None)
    );
    ctx.request_repaint_after(Duration::from_secs(20));
    let next = balaur_ui::next_frame(&app.engine, &ctx);
    assert!(sleeps_at_most(&next, Duration::from_secs(20)), "{next:?}");
    ctx.request_repaint();
    assert_eq!(balaur_ui::next_frame(&app.engine, &ctx), NextFrame::Now);
}

/// One pass of a focused field at egui time `time`, with the loop's
/// `wants_pass` before it: how long egui lets the loop sleep after it.
fn focused_field(app: &balaur_core::App, ctx: &egui::Context, time: f64, input: bool) -> Duration {
    balaur_ui::wants_pass(&app.engine, ctx, input);
    ctx.begin_pass(egui::RawInput {
        time: Some(time),
        ..Default::default()
    });
    egui::Area::new(egui::Id::unique("field")).show(ctx, |ui| {
        let mut text = String::from("name");
        ui.text_edit_singleline(&mut text).request_focus();
    });
    let mut out = ctx.end_pass();
    out.textures_delta.clear();
    out.viewport_output[&egui::ViewportId::ROOT].repaint_delay
}

#[test]
fn a_focused_caret_stops_blinking_ten_seconds_after_the_last_input() {
    let (_dir, app, _, errors) = run_with("ui::set_lazy(true);");
    assert!(errors.is_empty(), "{errors:?}");
    balaur_ui::honour_lazy(&app.engine);
    let ctx = egui::Context::default();
    // A click focuses the field; its pass and the next answer it.
    focused_field(&app, &ctx, 0.0, true);
    focused_field(&app, &ctx, 0.1, true);
    // Then every blink is a pass egui owes, until the timeout.
    assert!(focused_field(&app, &ctx, 1.0, false) < Duration::from_secs(1));
    assert!(focused_field(&app, &ctx, 10.0, false) < Duration::from_secs(1));
    // Past it, the caret holds still and owes nothing: the loop sleeps.
    focused_field(&app, &ctx, 10.5, false);
    assert_eq!(focused_field(&app, &ctx, 11.0, false), Duration::MAX);
    assert_eq!(focused_field(&app, &ctx, 30.0, false), Duration::MAX);
    // A key or a click starts it blinking again.
    assert!(focused_field(&app, &ctx, 31.0, true) < Duration::from_secs(1));
}
