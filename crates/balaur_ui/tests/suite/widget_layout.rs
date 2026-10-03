//! The taffy fields the `widget` component states besides a size: grid
//! tracks and placements, the gap pair, absolute children, margins, borders,
//! the alignment words and the flex overrides, each moving the layout.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use balaur_core::hecs::Entity;

/// A root container at the top-left corner with `children` labels under it,
/// each a box of a stated size, laid out and drawn.
fn laid_out(
    container: &toml::Value,
    children: &[toml::Value],
) -> (tempfile::TempDir, Vec<egui::Rect>, egui::Rect) {
    let (dir, app) = app();
    let root = add_widget(&app, container);
    let kids: Vec<Entity> = children
        .iter()
        .enumerate()
        .map(|(i, params)| add_child_widget(&app, root, &format!("c{i}"), params))
        .collect();
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rects = kids
        .iter()
        .map(|kid| balaur_ui::widget_rect(*kid).expect("the child drew"))
        .collect();
    let whole = balaur_ui::widget_rect(root).expect("the root drew");
    (dir, rects, whole)
}

fn block(width: f64, height: f64) -> toml::Value {
    toml::toml! { kind = "label" text = "" width = width height = height }.into()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.6
}

#[test]
fn a_grid_lays_its_children_into_the_columns_its_track_list_names() {
    let grid = toml::toml! {
        kind = "grid" grid_columns = "100 1fr" width = 300.0 gap = [10.0, 6.0] padding = 0.0 x = 0.0 y = 0.0
    };
    let (_dir, rects, _) = laid_out(
        &grid.into(),
        &[block(0.0, 20.0), block(0.0, 20.0), block(0.0, 30.0)],
    );
    assert!(
        close(rects[0].width(), 100.0),
        "a fixed track: {:?}",
        rects[0]
    );
    assert!(
        close(rects[1].width(), 190.0),
        "1fr takes what is left after the gap: {:?}",
        rects[1]
    );
    assert!(
        close(rects[1].min.x - rects[0].max.x, 10.0),
        "the first number is between columns"
    );
    assert!(
        close(rects[2].min.y - rects[0].max.y, 6.0),
        "the second is between rows: {rects:?}"
    );
    assert!(
        close(rects[2].min.x, rects[0].min.x),
        "the third child wraps to the first column"
    );
}

#[test]
fn a_grid_with_no_columns_named_draws_two_equal_ones() {
    let grid =
        toml::toml! { kind = "grid" width = 210.0 gap = [10.0, 0.0] padding = 0.0 x = 0.0 y = 0.0 };
    let (_dir, rects, _) = laid_out(&grid.into(), &[block(0.0, 20.0), block(0.0, 20.0)]);
    assert!(
        close(rects[0].width(), 100.0) && close(rects[1].width(), 100.0),
        "{rects:?}"
    );
}

#[test]
fn a_child_names_a_grid_area_and_takes_its_cells() {
    let grid = toml::toml! {
        kind = "grid" grid_columns = "50 50" grid_rows = "20 20" areas = ["head head", "side main"]
        gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0
    };
    let main = toml::toml! { kind = "label" text = "" row = "main" column = "main" };
    let head = toml::toml! { kind = "label" text = "" row = "head" column = "head" };
    let (_dir, rects, _) = laid_out(&grid.into(), &[main.into(), head.into()]);
    assert!(
        close(rects[0].min.x, 50.0) && close(rects[0].min.y, 20.0),
        "main: {:?}",
        rects[0]
    );
    assert!(
        close(rects[1].width(), 100.0) && close(rects[1].min.y, 0.0),
        "head spans both: {:?}",
        rects[1]
    );
}

#[test]
fn a_child_placed_by_line_and_span_lands_there() {
    let grid = toml::toml! {
        kind = "grid" grid_columns = "repeat(3, 40)" gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0
    };
    let wide = toml::toml! { kind = "label" text = "" column = "2 / span 2" height = 10.0 };
    let (_dir, rects, _) = laid_out(&grid.into(), &[wide.into()]);
    assert!(
        close(rects[0].min.x, 40.0) && close(rects[0].width(), 80.0),
        "{:?}",
        rects[0]
    );
}

#[test]
fn a_column_flow_fills_down_before_across() {
    let grid = toml::toml! {
        kind = "grid" grid_columns = "30" grid_rows = "20 20" auto_flow = "column" auto_columns = "30"
        gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0
    };
    let (_dir, rects, _) = laid_out(
        &grid.into(),
        &[block(0.0, 0.0), block(0.0, 0.0), block(0.0, 0.0)],
    );
    assert!(
        close(rects[1].min.y, 20.0) && close(rects[1].min.x, 0.0),
        "down first: {rects:?}"
    );
    assert!(
        close(rects[2].min.x, 30.0) && close(rects[2].width(), 30.0),
        "then an auto column: {rects:?}"
    );
}

#[test]
fn a_track_list_that_does_not_read_is_refused_naming_its_key() {
    let (_dir, app) = app();
    let root = app.engine.root();
    let node = balaur::scene::spawn_node(&mut app.engine.world_mut(), "G", root);
    let err = balaur::components::add(
        &app.engine,
        node,
        "widget",
        Some(&toml::toml! { kind = "grid" grid_columns = "1fr wide" }.into()),
    )
    .expect_err("a word that is no track");
    assert!(format!("{err:#}").contains("grid_columns"), "{err:#}");
}

#[test]
fn a_gap_written_as_one_number_is_refused() {
    let (_dir, app) = app();
    let root = app.engine.root();
    let node = balaur::scene::spawn_node(&mut app.engine.world_mut(), "R", root);
    let err = balaur::components::add(
        &app.engine,
        node,
        "widget",
        Some(&toml::toml! { kind = "row" gap = 8.0 }.into()),
    )
    .expect_err("a number where a pair goes");
    assert!(format!("{err:#}").contains("gap"), "{err:#}");
}

#[test]
fn a_flow_spaces_its_children_across_and_its_lines_down_by_the_gap_pair() {
    let flow =
        toml::toml! { kind = "flow" width = 100.0 gap = [4.0, 12.0] padding = 0.0 x = 0.0 y = 0.0 };
    let (_dir, rects, _) = laid_out(
        &flow.into(),
        &[block(48.0, 10.0), block(48.0, 10.0), block(48.0, 10.0)],
    );
    assert!(
        close(rects[1].min.x - rects[0].max.x, 4.0),
        "across: {rects:?}"
    );
    assert!(
        close(rects[2].min.y - rects[0].max.y, 12.0),
        "down, between lines: {rects:?}"
    );
}

#[test]
fn an_absolute_child_sits_at_its_inset_and_takes_no_room() {
    let column = toml::toml! { kind = "column" width = 200.0 height = 100.0 gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0 };
    let pinned = toml::toml! {
        kind = "label" text = "" width = 30.0 height = 10.0 absolute = true inset = [-1.0, 5.0, 8.0, -1.0]
    };
    let (_dir, rects, _) = laid_out(&column.into(), &[pinned.into(), block(50.0, 20.0)]);
    assert!(
        close(rects[0].max.x, 192.0) && close(rects[0].min.y, 5.0),
        "from the right and the top: {:?}",
        rects[0]
    );
    assert!(
        close(rects[1].min.y, 0.0),
        "the next child is not pushed down: {:?}",
        rects[1]
    );
}

#[test]
fn max_width_caps_a_child_that_grows() {
    let row =
        toml::toml! { kind = "row" width = 300.0 gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0 };
    let capped =
        toml::toml! { kind = "label" text = "" grow = 1.0 max_width = 120.0 height = 10.0 };
    let (_dir, rects, _) = laid_out(&row.into(), &[capped.into()]);
    assert!(close(rects[0].width(), 120.0), "{:?}", rects[0]);
}

#[test]
fn an_aspect_ratio_sets_the_height_a_width_leaves_open() {
    let column = toml::toml! { kind = "column" gap = [0.0, 0.0] padding = 0.0 align_items = "start" x = 0.0 y = 0.0 };
    let shaped = toml::toml! { kind = "label" text = "" width = 120.0 aspect_ratio = 2.0 };
    let (_dir, rects, _) = laid_out(&column.into(), &[shaped.into()]);
    assert!(close(rects[0].height(), 60.0), "{:?}", rects[0]);
}

#[test]
fn a_margin_keeps_room_round_a_child() {
    let column = toml::toml! { kind = "column" gap = [0.0, 0.0] padding = 0.0 align_items = "start" x = 0.0 y = 0.0 };
    let spaced = toml::toml! { kind = "label" text = "" width = 40.0 height = 10.0 margin = [6.0, 9.0, 0.0, 3.0] };
    let (_dir, rects, whole) = laid_out(&column.into(), &[spaced.into(), block(40.0, 10.0)]);
    assert!(
        close(rects[0].min.x, 6.0) && close(rects[0].min.y, 9.0),
        "{:?}",
        rects[0]
    );
    assert!(
        close(rects[1].min.y, 22.0),
        "the bottom margin pushes the next: {:?}",
        rects[1]
    );
    assert!(
        close(whole.width(), 46.0),
        "the container holds the margin: {whole:?}"
    );
}

#[test]
fn a_border_keeps_its_band_clear_and_the_stroke_is_painted_that_wide() {
    let (_dir, app) = app();
    // Under a column, which records the row's whole box: a root hugs only
    // what it draws.
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" padding = 0.0 gap = [0.0, 0.0] x = 0.0 y = 0.0 }.into(),
    );
    let row = add_child_widget(
        &app,
        column,
        "row",
        &toml::toml! { kind = "row" stroke = "#ff0000" border = [4.0, 4.0, 4.0, 4.0] padding = 0.0 gap = [0.0, 0.0] align_items = "start" }.into(),
    );
    let kid = add_child_widget(&app, row, "c", &block(20.0, 10.0));
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = pass(&app, &ctx, vec![]);
    let inner = balaur_ui::widget_rect(kid).unwrap();
    let outer = balaur_ui::widget_rect(row).unwrap();
    assert!(
        close(inner.min.x - outer.min.x, 4.0),
        "the child starts inside the band: {inner:?} in {outer:?}"
    );
    assert!(
        close(outer.width(), 28.0) && close(outer.height(), 18.0),
        "the band on every side: {outer:?}"
    );
    let widths: Vec<f32> = drawn
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect) if rect.stroke.color == egui::Color32::RED => {
                Some(rect.stroke.width)
            }
            _ => None,
        })
        .collect();
    assert_eq!(widths, vec![4.0], "one outline, as wide as the band");
}

#[test]
fn content_box_sizing_puts_the_padding_outside_the_stated_width() {
    let column = toml::toml! { kind = "column" gap = [0.0, 0.0] padding = 0.0 align_items = "start" x = 0.0 y = 0.0 };
    let boxed = toml::toml! {
        kind = "row" width = 50.0 height = 10.0 padding = 5.0 box_sizing = "content"
    };
    let (_dir, rects, _) = laid_out(&column.into(), &[boxed.into()]);
    assert!(close(rects[0].width(), 60.0), "{:?}", rects[0]);
}

#[test]
fn a_right_to_left_row_starts_at_its_right_edge() {
    let row = toml::toml! { kind = "row" width = 200.0 direction = "right_to_left" gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0 };
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
fn overflow_scroll_keeps_the_scrollbar_width_clear() {
    let column = toml::toml! {
        kind = "column" width = 100.0 overflow = "scroll" scrollbar_width = 12.0 gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0
    };
    let (_dir, rects, _) = laid_out(&column.into(), &[block(0.0, 10.0)]);
    assert!(close(rects[0].width(), 88.0), "{:?}", rects[0]);
}

#[test]
fn align_self_moves_one_child_off_its_parent_s_alignment() {
    let column = toml::toml! { kind = "column" width = 100.0 align_items = "start" gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0 };
    let ended =
        toml::toml! { kind = "label" text = "" width = 20.0 height = 10.0 align_self = "end" };
    let (_dir, rects, _) = laid_out(&column.into(), &[block(20.0, 10.0), ended.into()]);
    assert!(close(rects[0].min.x, 0.0), "{rects:?}");
    assert!(close(rects[1].max.x, 100.0), "{rects:?}");
}

#[test]
fn align_content_centres_the_lines_a_flow_wrapped() {
    let flow = toml::toml! {
        kind = "flow" width = 100.0 height = 100.0 align_content = "center" gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0
    };
    let (_dir, rects, _) = laid_out(&flow.into(), &[block(60.0, 10.0), block(60.0, 10.0)]);
    assert!(
        close(rects[0].min.y, 40.0),
        "two 10 px lines in 100: {rects:?}"
    );
}

#[test]
fn safe_align_keeps_an_overflowing_child_s_start_in_view() {
    let centred = |safe: bool| {
        let column = toml::toml! {
            kind = "column" width = 50.0 align_items = "center" safe_align = safe gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0
        };
        let (_dir, rects, _) = laid_out(&column.into(), &[block(80.0, 10.0)]);
        rects[0].min.x
    };
    assert!(centred(false) < -1.0, "centred, it hangs off the start");
    assert!(close(centred(true), 0.0), "safe, it starts at the start");
}

#[test]
fn a_width_percent_takes_that_share_of_the_container() {
    let row =
        toml::toml! { kind = "row" width = 200.0 gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0 };
    let share = toml::toml! { kind = "label" text = "" width_percent = 25.0 height = 10.0 };
    let (_dir, rects, _) = laid_out(&row.into(), &[share.into()]);
    assert!(close(rects[0].width(), 50.0), "{:?}", rects[0]);
}

#[test]
fn a_reversed_row_lays_its_children_from_the_far_end() {
    let row = toml::toml! { kind = "row" width = 100.0 reverse = true gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0 };
    let (_dir, rects, _) = laid_out(&row.into(), &[block(20.0, 10.0), block(30.0, 10.0)]);
    assert!(
        close(rects[0].max.x, 100.0) && close(rects[1].max.x, 80.0),
        "{rects:?}"
    );
}

#[test]
fn a_row_told_to_wrap_does_and_balance_evens_its_lines() {
    let row = |wrap: &str| {
        let params = toml::toml! { kind = "row" width = 100.0 wrap_children = wrap gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0 };
        let four = [
            block(30.0, 10.0),
            block(30.0, 10.0),
            block(30.0, 10.0),
            block(30.0, 10.0),
        ];
        let (_dir, rects, _) = laid_out(&params.into(), &four);
        rects
    };
    let one_line = row("auto");
    assert!(
        close(one_line[3].min.y, 0.0),
        "a row keeps one line unless told: {one_line:?}"
    );
    let wrapped = row("wrap");
    assert!(
        close(wrapped[2].min.y, 0.0) && close(wrapped[3].min.y, 10.0),
        "three then one: {wrapped:?}"
    );
    let balanced = row("balance");
    assert!(
        close(balanced[2].min.y, 10.0),
        "balanced, two and two: {balanced:?}"
    );
}

#[test]
fn min_lines_spreads_a_balanced_row_over_that_many_lines() {
    let row = toml::toml! {
        kind = "row" width = 300.0 wrap_children = "balance" min_lines = 3 gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0
    };
    let (_dir, rects, _) = laid_out(
        &row.into(),
        &[block(40.0, 10.0), block(40.0, 10.0), block(40.0, 10.0)],
    );
    assert!(
        close(rects[2].min.y, 20.0),
        "one a line, where one line would hold them: {rects:?}"
    );
}

#[test]
fn basis_and_shrink_override_what_grow_derives() {
    let row =
        toml::toml! { kind = "row" width = 100.0 gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0 };
    let squeezed = toml::toml! { kind = "label" text = "" basis = 80.0 shrink = 1.0 height = 10.0 };
    let held = toml::toml! { kind = "label" text = "" basis = 80.0 shrink = 0.0 height = 10.0 };
    let (_dir, rects, _) = laid_out(&row.into(), &[squeezed.into(), held.into()]);
    assert!(
        close(rects[1].width(), 80.0),
        "one that does not shrink keeps its basis: {rects:?}"
    );
    assert!(
        close(rects[0].width(), 20.0),
        "the other gives up the shortfall: {rects:?}"
    );
}

#[test]
fn every_layout_key_reads_back_as_it_was_written() {
    let (_dir, app) = app();
    let written = toml::toml! {
        kind = "grid" absolute = true max_width = 300.0 max_height = 200.0 aspect_ratio = 1.5
        margin = [1.0, 2.0, 3.0, 4.0] border = [1.0, 1.0, 2.0, 2.0] box_sizing = "content"
        direction = "right_to_left" overflow = "hidden" scrollbar_width = 6.0 contain = ["layout", "paint"]
        align_self = "center" align_content = "between" safe_align = true width_percent = 50.0
        height_percent = 40.0 wrap_children = "balance" min_lines = 2 basis = 10.0 shrink = 2.0
        grid_columns = "100 1fr" grid_rows = "auto" auto_columns = "20" auto_rows = "30"
        auto_flow = "column_dense" areas = ["a b"] row = "1 / 3" column = "span 2" gap = [3.0, 5.0]
    };
    let entity = add_widget(&app, &written.clone().into());
    for (key, value) in &written {
        assert_eq!(
            &property(&app, entity, key),
            value,
            "`{key}` came back changed"
        );
    }
}

#[test]
fn a_panel_s_caption_keeps_its_children_below_it() {
    let (_dir, app) = app();
    let panel = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "Caption" padding = 6.0 gap = [0.0, 4.0] x = 0.0 y = 0.0 }.into(),
    );
    let kid = add_child_widget(
        &app,
        panel,
        "c",
        &toml::toml! { kind = "label" text = "inside" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = pass(&app, &ctx, vec![]);
    let caption = texts(&drawn)
        .into_iter()
        .find(|(text, _)| text == "Caption")
        .map(|(_, at)| at)
        .expect("the caption drew");
    let child = balaur_ui::widget_rect(kid).unwrap();
    assert!(
        child.min.y >= caption.y + 10.0,
        "the child starts below the caption: caption at {caption:?}, child {child:?}"
    );
}

#[test]
fn a_panel_s_caption_sits_at_its_top_in_a_centred_row() {
    let (_dir, app) = app();
    let row = add_widget(
        &app,
        &toml::toml! { kind = "row" align_items = "center" gap = [12.0, 12.0] x = 0.0 y = 0.0 }
            .into(),
    );
    add_child_widget(
        &app,
        row,
        "b",
        &toml::toml! { kind = "button" text = "tall" height = 40.0 }.into(),
    );
    let panel = add_child_widget(
        &app,
        row,
        "p",
        &toml::toml! { kind = "panel" text = "Caption" padding = 6.0 gap = [4.0, 4.0] }.into(),
    );
    let kid = add_child_widget(
        &app,
        panel,
        "c",
        &toml::toml! { kind = "label" text = "inside" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = pass(&app, &ctx, vec![]);
    let caption = texts(&drawn)
        .into_iter()
        .find(|(text, _)| text == "Caption")
        .map(|(_, at)| at)
        .expect("the caption drew");
    let top = balaur_ui::widget_rect(panel).unwrap().min.y;
    let child = balaur_ui::widget_rect(kid).unwrap();
    assert!(
        close(caption.y, top + 6.0),
        "at the top, inside the padding: {caption:?} in a box from {top}"
    );
    assert!(
        child.min.y > caption.y + 10.0,
        "and the child under it: {child:?}"
    );
}

#[test]
fn a_list_in_a_row_still_runs_its_rows_down() {
    let (_dir, app) = app();
    let row = add_widget(
        &app,
        &toml::toml! { kind = "row" align_items = "center" x = 0.0 y = 0.0 }.into(),
    );
    add_child_widget(
        &app,
        row,
        "l",
        &toml::toml! { kind = "list" options = ["first", "second", "third"] width = 160.0 height = 120.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = pass(&app, &ctx, vec![]);
    let at = |name: &str| {
        texts(&drawn)
            .into_iter()
            .find(|(text, _)| text == name)
            .map_or_else(|| panic!("{name} drew: {:?}", texts(&drawn)), |(_, at)| at)
    };
    assert!(at("second").y > at("first").y + 5.0, "under, not beside");
    assert!(close(at("second").x, at("first").x));
}

/// A row of `children` under a theme of `roles`, laid out and drawn: each
/// child's box, and what the pass painted.
fn themed_row(roles: &str, children: &[toml::Value]) -> (Vec<egui::Rect>, egui::FullOutput) {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/t.toml"),
        format!("type = \"widget_theme\"\n\n{roles}"),
    )
    .unwrap();
    let row = add_widget(
        &app,
        &toml::toml! { kind = "row" theme = "themes/t.toml" align_items = "start" gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0 }
            .into(),
    );
    let kids: Vec<Entity> = children
        .iter()
        .enumerate()
        .map(|(i, params)| add_child_widget(&app, row, &format!("c{i}"), params))
        .collect();
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let rects = kids
        .iter()
        .map(|kid| balaur_ui::widget_rect(*kid).expect("the child drew"))
        .collect();
    (rects, out)
}

#[test]
fn a_button_s_padding_is_counted_once_inside_a_row() {
    let (rects, _) = themed_row(
        "",
        &[
            toml::toml! { kind = "label" text = "Save" }.into(),
            toml::toml! { kind = "button" text = "Save" padding = [10.0, 4.0, 10.0, 4.0] }.into(),
        ],
    );
    let (label, button) = (rects[0], rects[1]);
    assert!(
        close(button.width(), label.width() + 20.0),
        "the caption and ten either side: {label:?} {button:?}"
    );
    assert!(
        close(button.height(), label.height() + 8.0),
        "and four above and below: {label:?} {button:?}"
    );
}

#[test]
fn a_role_s_padding_is_the_default_a_button_s_own_padding_overrides_side_by_side() {
    let (rects, _) = themed_row(
        "[roles.chip]\npadding_x = 10.0\npadding_y = 3.0\n",
        &[
            toml::toml! { kind = "label" text = "Go" }.into(),
            toml::toml! { kind = "button" text = "Go" role = "chip" }.into(),
            toml::toml! { kind = "button" text = "Go" role = "chip" padding = [2.0, -1.0, 6.0, -1.0] }
                .into(),
        ],
    );
    let (label, role, own) = (rects[0], rects[1], rects[2]);
    assert!(
        close(role.width(), label.width() + 20.0),
        "the role's ten either side: {label:?} {role:?}"
    );
    assert!(
        close(own.width(), label.width() + 8.0),
        "the button's own two and six: {label:?} {own:?}"
    );
    for button in [role, own] {
        assert!(
            close(button.height(), label.height() + 6.0),
            "the role's three above and below, which the button left: {label:?} {button:?}"
        );
    }
}

#[test]
fn a_label_s_padding_insets_its_text() {
    let (rects, out) = themed_row(
        "",
        &[toml::toml! { kind = "label" text = "Inset" padding = [12.0, 0.0, 0.0, 0.0] }.into()],
    );
    let left = out
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Mesh(mesh) => {
                mesh.vertices.iter().map(|v| v.pos.x).reduce(f32::min)
            }
            egui::epaint::Shape::Text(text) => Some(text.pos.x),
            _ => None,
        })
        .reduce(f32::min)
        .expect("the label drew its text");
    assert!(
        left >= rects[0].min.x + 11.5,
        "the text starts inside the padding: {left} in {:?}",
        rects[0]
    );
}
