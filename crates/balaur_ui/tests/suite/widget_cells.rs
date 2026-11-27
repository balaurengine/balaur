//! Where a grid's child sits across its cell: `justify_self`, and the text
//! of a label that fills its cell.

use crate::support::*;
use crate::widget_layout::{block, close, laid_out, laid_out_in};

/// A 30 by 10 label placed in its cell by `word`.
fn placed(word: &str) -> toml::Value {
    toml::toml! { kind = "label" text = "" width = 30.0 height = 10.0 widget_child = { justify_self = word } }.into()
}

// A grid child's `justify_self` places it across its cell at its own width,
// as CSS's `justify-self`: a number at its column's end. Unset stays where it was.
#[test]
fn a_grid_child_s_justify_self_places_it_across_its_cell() {
    let grid = toml::toml! {
        kind = "grid" padding = 0.0 x = 0.0 y = 0.0
        widget_grid = { grid_columns = "100 100 100" }
        widget_layout = { gap = [0.0, 0.0] }
    };
    let (_dir, rects, _) = laid_out(
        &grid.into(),
        &[placed("end"), placed("center"), block(30.0, 10.0)],
    );
    assert!(
        close(rects[0].min.x, 70.0) && close(rects[0].max.x, 100.0),
        "end: {:?}",
        rects[0]
    );
    assert!(close(rects[1].min.x, 135.0), "center: {:?}", rects[1]);
    assert!(
        close(rects[2].min.x, 200.0),
        "unset keeps the start: {:?}",
        rects[2]
    );
}

// Mirrored, `end` is the cell's left edge, as CSS's logical `end` is.
#[test]
fn a_mirrored_grid_puts_a_child_s_end_at_its_cell_s_left() {
    let grid = toml::toml! {
        kind = "grid" width = 100.0 padding = 0.0 x = 0.0 y = 0.0
        widget_grid = { grid_columns = "100" }
    };
    let (_dir, rects, whole) = laid_out_in("ar", &grid.into(), &[placed("end")]);
    // The grid sits against the screen's right edge, its one column 100 wide.
    assert!(
        close(rects[0].min.x, whole.max.x - 100.0),
        "{:?} in {whole:?}",
        rects[0]
    );
}

// A label in a grid cell, with no width of its own, is as wide as the cell,
// so its `end` text ends at the cell's edge; one with `justify_self = "end"`
// keeps its own width there.
#[test]
fn a_label_in_a_grid_cell_ends_its_text_or_itself_at_the_cell_s_edge() {
    let (_dir, app) = app();
    let grid = toml::toml! {
        kind = "grid" padding = 0.0 x = 0.0 y = 0.0
        widget_grid = { grid_columns = "200 200" }
        widget_layout = { gap = [0.0, 0.0] }
    };
    let root = add_widget(&app, &grid.into());
    let text_end =
        toml::toml! { kind = "label" text = "12.50" widget_text = { text_align = "end" } };
    let box_end =
        toml::toml! { kind = "label" text = "99.99" widget_child = { justify_self = "end" } };
    let first = add_child_widget(&app, root, "first", &text_end.into());
    let second = add_child_widget(&app, root, "second", &box_end.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let first = balaur_ui::widget_rect(first).expect("the first label drew");
    let second = balaur_ui::widget_rect(second).expect("the second label drew");
    assert!(close(first.width(), 200.0), "the cell's width: {first:?}");
    let last_glyph = glyphs(&out)
        .iter()
        .map(|(at, _)| at.max.x)
        .filter(|x| *x <= first.max.x)
        .fold(0.0_f32, f32::max);
    assert!(
        first.max.x - last_glyph < 2.0,
        "the text ends at {last_glyph} in {first:?}"
    );
    assert!(
        close(second.max.x, 400.0) && second.width() < 100.0,
        "its own width at the cell's end: {second:?}"
    );
}
