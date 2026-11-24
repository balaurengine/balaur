//! A UI's direction moving the layout: a right-to-left row, an Arabic UI
//! mirrored from its roots down, and a subtree that keeps its own direction.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;

use crate::widget_layout::{block, close, laid_out, laid_out_in};

#[test]
fn a_right_to_left_row_starts_at_its_right_edge() {
    let row = toml::toml! { kind = "row" width = 200.0 padding = 0.0 x = 0.0 y = 0.0 widget_layout = { direction = "right_to_left", gap = [0.0, 0.0] } };
    let (_dir, rects, _) = laid_out(&row.into(), &[block(30.0, 10.0), block(30.0, 10.0)]);
    assert!(
        close(rects[0].max.x, 200.0),
        "the first child at the right: {rects:?}"
    );
    assert!(
        rects[1].max.x <= rects[0].min.x + 0.5,
        "the second to its left: {rects:?}"
    );
}

#[test]
fn a_row_in_an_arabic_ui_runs_from_its_right_edge() {
    let row = toml::toml! { kind = "row" width = 200.0 padding = 0.0 x = 0.0 y = 0.0 widget_layout = { gap = [10.0, 0.0] } };
    let (_dir, rects, whole) =
        laid_out_in("ar", &row.into(), &[block(30.0, 10.0), block(40.0, 10.0)]);
    assert!(
        close(rects[0].max.x, whole.max.x),
        "the first child at the right: {rects:?} in {whole:?}"
    );
    assert!(
        close(rects[0].min.x - rects[1].max.x, 10.0),
        "the second to its left, the gap between: {rects:?}"
    );
}

#[test]
fn an_arabic_ui_puts_a_root_anchored_top_left_at_the_top_right() {
    let row =
        toml::toml! { kind = "row" width = 200.0 height = 20.0 padding = 0.0 x = 0.0 y = 0.0 };
    let (_dir, _, english) = laid_out(&row.clone().into(), &[block(30.0, 10.0)]);
    let (_dir, _, arabic) = laid_out_in("ar", &row.into(), &[block(30.0, 10.0)]);
    assert!(close(english.min.x, 0.0), "{english:?}");
    assert!(
        arabic.min.x > english.min.x + 100.0,
        "against the far edge: {arabic:?}"
    );
}

#[test]
fn a_mirrored_container_swaps_its_padding_s_sides_and_a_margin_s() {
    let column = toml::toml! {
        kind = "column" width = 200.0 x = 0.0 y = 0.0
        padding = [20.0, 0.0, 0.0, 0.0]
        widget_layout = { align_items = "start", gap = [0.0, 0.0] }
    };
    let pushed = toml::toml! { kind = "label" text = "" width = 30.0 height = 10.0 margin = [5.0, 0.0, 0.0, 0.0] };
    let (_dir, rects, whole) = laid_out_in("ar", &column.into(), &[pushed.into()]);
    assert!(
        close(whole.max.x - rects[0].max.x, 25.0),
        "the start padding and the start margin are at the right: {rects:?} in {whole:?}"
    );
}

#[test]
fn start_and_end_swap_in_a_mirrored_column() {
    let column = toml::toml! { kind = "column" width = 100.0 padding = 0.0 x = 0.0 y = 0.0 widget_layout = { align_items = "start", gap = [0.0, 0.0] } };
    let ended =
        toml::toml! { kind = "label" text = "" width = 20.0 height = 10.0 align_self = "end" };
    let (_dir, rects, whole) =
        laid_out_in("ar", &column.into(), &[block(20.0, 10.0), ended.into()]);
    assert!(
        close(rects[0].max.x, whole.max.x),
        "start is the right: {rects:?}"
    );
    assert!(
        close(rects[1].min.x, whole.min.x),
        "end the left: {rects:?}"
    );
}

#[test]
fn a_left_to_right_subtree_keeps_its_order_in_a_mirrored_ui() {
    let outer = toml::toml! { kind = "row" width = 200.0 padding = 0.0 x = 0.0 y = 0.0 widget_layout = { gap = [0.0, 0.0] } };
    let (dir, app) = app();
    balaur_core::strings::set_locale(&app.engine, "ar");
    let root = add_widget(&app, &outer.into());
    let board = add_child_widget(
        &app,
        root,
        "board",
        &toml::toml! { kind = "row" width = 100.0 padding = 0.0 widget_layout = { direction = "left_to_right", gap = [0.0, 0.0] } }.into(),
    );
    let a = add_child_widget(&app, board, "a", &block(30.0, 10.0));
    let b = add_child_widget(&app, board, "b", &block(30.0, 10.0));
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = |e| balaur_ui::widget_rect(e).expect("drew");
    let (whole, board, a, b) = (rect(root), rect(board), rect(a), rect(b));
    assert!(
        close(board.max.x, whole.max.x),
        "the board at the right of the mirrored row: {board:?}"
    );
    assert!(
        close(a.min.x, board.min.x) && b.min.x > a.min.x,
        "its letters left to right: {a:?} {b:?}"
    );
    drop(dir);
}

#[test]
fn a_right_to_left_subtree_mirrors_what_is_under_it_too() {
    let row = toml::toml! { kind = "row" width = 200.0 padding = 0.0 x = 0.0 y = 0.0 widget_layout = { direction = "right_to_left", gap = [0.0, 0.0] } };
    let (dir, app) = app();
    let root = add_widget(&app, &row.into());
    let inner = add_child_widget(
        &app,
        root,
        "inner",
        &toml::toml! { kind = "row" width = 100.0 padding = 0.0 widget_layout = { gap = [0.0, 0.0] } }.into(),
    );
    let a = add_child_widget(&app, inner, "a", &block(30.0, 10.0));
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = |e| balaur_ui::widget_rect(e).expect("drew");
    let (whole, inner, a) = (rect(root), rect(inner), rect(a));
    assert!(close(inner.max.x, whole.max.x), "{inner:?} in {whole:?}");
    assert!(
        close(a.max.x, inner.max.x),
        "the direction is inherited, as CSS's: {a:?} in {inner:?}"
    );
    drop(dir);
}
