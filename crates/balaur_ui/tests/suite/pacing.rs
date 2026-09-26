//! What a loop sleeping under `[window] low_processor` is owed.

use std::time::Duration;

use balaur_ui::NextFrame;

use crate::pass::draw_with;

fn sleeps_at_most(next: &NextFrame, most: Duration) -> bool {
    matches!(next, NextFrame::Sleep(Some(left)) if *left <= most && *left > most.saturating_sub(Duration::from_secs(5)))
}

#[test]
fn a_ui_nobody_asked_to_redraw_lets_the_loop_sleep() {
    let (_dir, app, ctx, errors) = draw_with("this.drew = true;");
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(
        balaur_ui::next_frame(&app.engine, &ctx),
        NextFrame::Sleep(None)
    );
}

#[test]
fn a_script_s_request_is_owed_a_frame_now() {
    let (_dir, app, ctx, errors) = draw_with("ui::request_repaint();");
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(balaur_ui::next_frame(&app.engine, &ctx), NextFrame::Now);
}

#[test]
fn a_request_spent_by_its_own_pass_still_owes_the_next_frame() {
    let (_dir, app, ctx, errors) = draw_with("ui::request_repaint();");
    assert!(errors.is_empty(), "{errors:?}");
    balaur_ui::honour_lazy(&app.engine);
    assert!(balaur_ui::wants_pass(&app.engine, &ctx, false, false));
    assert_eq!(balaur_ui::next_frame(&app.engine, &ctx), NextFrame::Now);
    assert_eq!(
        balaur_ui::next_frame(&app.engine, &ctx),
        NextFrame::Sleep(None)
    );
}

#[test]
fn a_repaint_scheduled_ahead_is_how_long_the_loop_may_sleep() {
    let (_dir, app, ctx, errors) = draw_with("ui::request_repaint(#{ after: 30.0 });");
    assert!(errors.is_empty(), "{errors:?}");
    let next = balaur_ui::next_frame(&app.engine, &ctx);
    assert!(sleeps_at_most(&next, Duration::from_secs(30)), "{next:?}");
}

#[test]
fn egui_s_own_requests_wake_the_loop_when_due() {
    let (_dir, app, _, _) = draw_with("");
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
