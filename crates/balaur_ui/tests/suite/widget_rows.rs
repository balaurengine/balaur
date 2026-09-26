//! The row kinds: what a `list`, a `tree` and a `table` draw, what they build
//! and what a click on a row leaves picked.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use egui::pos2;

#[test]
fn a_list_draws_only_the_rows_that_fit() {
    let (_dir, app) = app();
    let many: Vec<String> = (0..2000).map(|i| format!("Row{i}")).collect();
    let params =
        toml::toml! { kind = "list" x = 0.0 y = 0.0 width = 200.0 height = 120.0 options = (many) };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = texts(&pass(&app, &ctx, vec![]));
    let rows = drawn.iter().filter(|(t, _)| t.starts_with("Row")).count();
    assert!(
        rows > 0 && rows < 60,
        "2000 items, only a screenful built: {rows}"
    );
    assert!(
        drawn.iter().any(|(t, _)| t == "Row0"),
        "and it starts at the top: {drawn:?}"
    );
}

#[test]
fn a_tree_indents_a_row_by_its_leading_tabs() {
    let (_dir, app) = app();
    let params = toml::toml! {
        kind = "tree" x = 0.0 y = 0.0 width = 200.0 height = 120.0
        options = ["Root", "\tChild", "\t\tLeaf"]
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = texts(&pass(&app, &ctx, vec![]));
    let at = |name: &str| drawn.iter().find(|(t, _)| t == name).map(|(_, p)| p.x);
    let (root, child, leaf) = (
        at("Root").expect("root drawn"),
        at("Child").expect("child drawn"),
        at("Leaf").expect("leaf drawn"),
    );
    assert!(
        child > root && leaf > child,
        "each level steps right: {root} {child} {leaf}"
    );
}

#[test]
fn a_list_row_splits_into_icon_label_and_note() {
    let (_dir, app) = app();
    let params = toml::toml! {
        kind = "list" x = 0.0 y = 0.0 width = 240.0 height = 120.0
        options = ["*\u{1f}Named\u{1f}12 KB\u{1f}#ff0000", "Plain"]
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn: Vec<String> = texts(&pass(&app, &ctx, vec![]))
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    for want in ["*", "Named", "12 KB", "Plain"] {
        assert!(drawn.iter().any(|t| t == want), "{want} drawn: {drawn:?}");
    }
}

#[test]
fn picking_a_row_leaves_the_rows_under_it_where_they_were() {
    let ctx = egui::Context::default();
    // The theme a shell brings: a resting stroke is what egui takes out of a
    // button's margin and, on the hovered or picked one, puts back.
    ctx.all_styles_mut(|style| {
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::GRAY);
    });
    let rows = |chosen: &str| {
        let (_dir, app) = app();
        let params = toml::toml! {
            kind = "tree" x = 0.0 y = 0.0 width = 200.0 height = 200.0
            text = chosen
            options = ["One", "Two", "Three", "Four", "Five", "Six"]
        };
        add_widget(&app, &params.into());
        settle(&app, &ctx);
        texts(&pass(&app, &ctx, vec![]))
    };
    let resting = rows("");
    assert!(
        resting.iter().any(|(t, _)| t == "Six"),
        "every row drew: {resting:?}"
    );
    assert_eq!(
        resting,
        rows("Two"),
        "a picked row must not move itself or the rows under it"
    );
}

#[test]
fn a_tree_caret_folds_the_branch_under_it() {
    let (_dir, app) = app();
    let params = toml::toml! {
        kind = "tree" x = 0.0 y = 0.0 width = 200.0 height = 200.0
        options = ["Root", "\tChild", "\t\tLeaf", "After"]
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let shown = |out: &egui::FullOutput| {
        texts(out)
            .into_iter()
            .map(|(t, _)| t)
            .filter(|t| t != "▾" && t != "▸")
            .collect::<Vec<_>>()
    };
    let open = shown(&pass(&app, &ctx, vec![]));
    assert!(
        open.iter().any(|t| t == "Leaf"),
        "the branch starts open: {open:?}"
    );
    // The caret sits left of the row it folds.
    let (_, at) = texts(&pass(&app, &ctx, vec![]))
        .into_iter()
        .find(|(t, _)| t == "▾")
        .expect("a parent row draws a caret");
    let hit = pos2(at.x + 3.0, at.y + 3.0);
    pass(&app, &ctx, press(hit, true));
    pass(&app, &ctx, press(hit, false));
    let folded = shown(&pass(&app, &ctx, vec![]));
    assert!(
        !folded.iter().any(|t| t == "Child") && folded.iter().any(|t| t == "After"),
        "the branch folds and its sibling stays: {folded:?}"
    );
}

#[test]
fn a_table_draws_only_the_rows_that_fit() {
    let (_dir, app) = app();
    let many: Vec<String> = (0..2000)
        .map(|i| format!("Row{i}\u{1f}{i}\u{1f}ok"))
        .collect();
    let params = toml::toml! {
        kind = "table" x = 0.0 y = 0.0 width = 300.0 height = 120.0
        titles = ["name", "count>", "state"]
        options = (many)
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = texts(&pass(&app, &ctx, vec![]));
    let rows = drawn.iter().filter(|(t, _)| t.starts_with("Row")).count();
    assert!(
        rows > 0 && rows < 60,
        "2000 rows, only a screenful built: {rows}"
    );
    for head in ["name", "count", "state"] {
        assert!(
            drawn.iter().any(|(t, _)| t == head),
            "the header names its columns: {drawn:?}"
        );
    }
    assert!(
        !drawn.iter().any(|(t, _)| t.contains('>')),
        "the mark that aligns a column is not drawn: {drawn:?}"
    );
}

#[test]
fn a_table_row_splits_into_one_cell_a_column() {
    let (_dir, app) = app();
    let params = toml::toml! {
        kind = "table" x = 0.0 y = 0.0 width = 300.0 height = 120.0
        titles = ["name", "size"]
        options = ["hero.png\u{1f}12 KB"]
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = texts(&pass(&app, &ctx, vec![]));
    let at = |name: &str| {
        drawn
            .iter()
            .find(|(t, _)| t == name)
            .map_or_else(|| panic!("{name} drawn: {drawn:?}"), |(_, p)| *p)
    };
    assert!(
        at("12 KB").x > at("hero.png").x,
        "the second cell sits in the second column"
    );
    assert!(
        at("hero.png").y > at("name").y,
        "the body sits under the header"
    );
}

#[test]
fn dragging_a_seam_moves_the_column_beside_it() {
    let (_dir, mut app) = app();
    let params = toml::toml! {
        kind = "table" x = 0.0 y = 0.0 width = 300.0 height = 120.0
        titles = ["name", "size"]
        options = ["hero.png\u{1f}12 KB"]
    };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let cell = |out: &egui::FullOutput| {
        texts(out)
            .into_iter()
            .find(|(t, _)| t == "12 KB")
            .expect("the second cell is drawn")
            .1
            .x
    };
    let before = cell(&pass(&app, &ctx, vec![]));
    // The seam sits on the boundary between the two columns, in the header.
    let seam = pos2(before - 6.0, 6.0);
    pass(&app, &ctx, press(seam, true));
    pass(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(seam + egui::vec2(60.0, 0.0))],
    );
    pass(&app, &ctx, press(seam + egui::vec2(60.0, 0.0), false));
    // The shares go onto the widget, so the tick that takes the edit is what
    // moves the column: they are the scene's to keep, not the pass's.
    consume_input(&mut app);
    let after = cell(&pass(&app, &ctx, vec![]));
    assert!(
        after > before + 40.0,
        "the drag widened the first column: {before} to {after}"
    );
    let widths = property(&app, entity, "widths");
    let stated: Vec<f64> = widths
        .as_array()
        .expect("the shares are a list")
        .iter()
        .filter_map(|share| share.as_str().and_then(|text| text.parse().ok()))
        .collect();
    assert_eq!(stated.len(), 2, "one share a column: {widths:?}");
    assert!(
        stated[0] > stated[1],
        "and the dragged column has the bigger one: {stated:?}"
    );
}

#[test]
fn a_list_holding_many_takes_the_rows_a_command_click_adds() {
    let (_dir, mut app) = app();
    let params = toml::toml! {
        kind = "list" x = 0.0 y = 0.0 width = 200.0 height = 200.0 multi_select = true
        row_height = 18.0 options = ["One", "Two", "Three", "Four"]
    };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let row = |out: &egui::FullOutput, name: &str| {
        texts(out)
            .into_iter()
            .find(|(t, _)| t == name)
            .unwrap_or_else(|| panic!("{name} is drawn"))
            .1
    };
    let drawn = pass(&app, &ctx, vec![]);
    let (one, three) = (row(&drawn, "One"), row(&drawn, "Three"));
    click_held(&app, &ctx, one, egui::Modifiers::NONE);
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "selection"),
        toml::Value::Array(vec![toml::Value::String("One".into())]),
        "a plain click takes the row alone"
    );
    click_held(&app, &ctx, three, egui::Modifiers::COMMAND);
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "selection"),
        toml::Value::Array(vec![
            toml::Value::String("One".into()),
            toml::Value::String("Three".into()),
        ]),
        "the command key adds a row without dropping the first"
    );
    click_held(&app, &ctx, three, egui::Modifiers::COMMAND);
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "selection"),
        toml::Value::Array(vec![toml::Value::String("One".into())]),
        "and takes it off again"
    );
}

#[test]
fn shift_on_a_list_takes_the_run_from_the_last_row_clicked() {
    let (_dir, mut app) = app();
    let params = toml::toml! {
        kind = "list" x = 0.0 y = 0.0 width = 200.0 height = 200.0 multi_select = true
        row_height = 18.0 options = ["One", "Two", "Three", "Four"]
    };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = pass(&app, &ctx, vec![]);
    let at = |name: &str| {
        texts(&drawn)
            .into_iter()
            .find(|(t, _)| t == name)
            .unwrap_or_else(|| panic!("{name} is drawn"))
            .1
    };
    click_held(&app, &ctx, at("Two"), egui::Modifiers::NONE);
    consume_input(&mut app);
    click_held(&app, &ctx, at("Four"), egui::Modifiers::SHIFT);
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "selection"),
        toml::Value::Array(vec![
            toml::Value::String("Two".into()),
            toml::Value::String("Three".into()),
            toml::Value::String("Four".into()),
        ]),
        "shift takes every row between the two"
    );
}

#[test]
fn a_list_holding_one_row_still_says_what_is_picked() {
    let (_dir, mut app) = app();
    let params = toml::toml! {
        kind = "list" x = 0.0 y = 0.0 width = 200.0 height = 200.0
        row_height = 18.0 options = ["One", "Two"]
    };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = pass(&app, &ctx, vec![]);
    let two = texts(&drawn)
        .into_iter()
        .find(|(t, _)| t == "Two")
        .expect("the row is drawn")
        .1;
    click_held(&app, &ctx, two, egui::Modifiers::COMMAND);
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "text"),
        toml::Value::String("Two".into()),
        "the pick is the row, as it always was"
    );
    assert_eq!(
        property(&app, entity, "selection"),
        toml::Value::Array(vec![toml::Value::String("Two".into())]),
        "and `selection` says so too, so nothing reading it branches on `multi`"
    );
}

#[test]
fn a_secondary_click_picks_the_row_it_lands_on() {
    let (_dir, mut app) = app();
    let params = toml::toml! {
        kind = "list" x = 0.0 y = 0.0 width = 200.0 height = 200.0
        row_height = 18.0 options = ["One", "Two", "Three"]
    };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let two = texts(&pass(&app, &ctx, vec![]))
        .into_iter()
        .find(|(t, _)| t == "Two")
        .expect("the row is drawn")
        .1;
    pass(
        &app,
        &ctx,
        press_with(two, egui::PointerButton::Secondary, true),
    );
    pass(
        &app,
        &ctx,
        press_with(two, egui::PointerButton::Secondary, false),
    );
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "text"),
        toml::Value::String("Two".into()),
        "a right click picks the row under the pointer, for the menu it opens"
    );
    assert!(
        !clicked(&app, entity),
        "and it is not a click: the widget's own on_click stays quiet"
    );
}

#[test]
fn a_secondary_click_on_a_picked_row_keeps_the_set() {
    let (_dir, mut app) = app();
    let params = toml::toml! {
        kind = "list" x = 0.0 y = 0.0 width = 200.0 height = 200.0 multi_select = true
        row_height = 18.0 options = ["One", "Two", "Three"]
        selection = ["One", "Two"] text = "Two"
    };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let one = texts(&pass(&app, &ctx, vec![]))
        .into_iter()
        .find(|(t, _)| t == "One")
        .expect("the row is drawn")
        .1;
    pass(
        &app,
        &ctx,
        press_with(one, egui::PointerButton::Secondary, true),
    );
    pass(
        &app,
        &ctx,
        press_with(one, egui::PointerButton::Secondary, false),
    );
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "selection"),
        toml::Value::Array(vec![
            toml::Value::String("One".into()),
            toml::Value::String("Two".into()),
        ]),
        "a menu opened over one of several picked rows is opened over all of them"
    );
}

#[test]
fn a_table_without_column_names_reads_its_first_row_as_the_header() {
    let (_dir, app) = app();
    let params = toml::toml! {
        kind = "table" x = 0.0 y = 0.0 width = 300.0 height = 120.0
        options = ["name\u{1f}size", "hero.png\u{1f}12 KB"]
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = texts(&pass(&app, &ctx, vec![]));
    let at = |name: &str| {
        drawn
            .iter()
            .find(|(t, _)| t == name)
            .map_or_else(|| panic!("{name} drawn: {drawn:?}"), |(_, p)| *p)
    };
    assert!(
        at("hero.png").y > at("name").y,
        "the first row is the header and the rest is the body"
    );
}

#[test]
fn a_table_row_wears_the_colour_it_carries_past_its_last_cell() {
    let (_dir, app) = app();
    let params = toml::toml! {
        kind = "table" x = 0.0 y = 0.0 width = 300.0 height = 120.0
        titles = ["name", "size"]
        options = ["plain\u{1f}1 KB", "loud\u{1f}2 KB\u{1f}#ff0000"]
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let ink = |want: &str| {
        out.shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == want => {
                    Some(text.fallback_color)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{want} is drawn"))
    };
    assert_eq!(ink("loud"), egui::Color32::from_rgb(255, 0, 0));
    assert_ne!(
        ink("plain"),
        egui::Color32::from_rgb(255, 0, 0),
        "and a row that carries no colour keeps the theme's"
    );
}

/// The fill of every rect a pass painted, for the plates a row view lays down.
fn fills(out: &egui::FullOutput) -> Vec<egui::Color32> {
    fn walk(shape: &egui::Shape, into: &mut Vec<egui::Color32>) {
        match shape {
            egui::epaint::Shape::Rect(rect) => into.push(rect.fill),
            egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|one| walk(one, into)),
            _ => {}
        }
    }
    let mut painted = Vec::new();
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut painted);
    }
    painted
}

/// How many straight lines a pass drew: a table's column rules and the rule
/// under its header, and the guides down a tree's indent.
fn rules(out: &egui::FullOutput) -> usize {
    fn walk(shape: &egui::Shape, count: &mut usize) {
        match shape {
            egui::epaint::Shape::LineSegment { .. } => *count += 1,
            egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|one| walk(one, count)),
            _ => {}
        }
    }
    let mut count = 0;
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut count);
    }
    count
}

/// A table drawn under a theme, which the project writes as an asset the
/// widget names.
fn themed(theme: &str, params: &toml::Value) -> egui::FullOutput {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/t.toml"),
        format!("type = \"widget_theme\"\n{theme}"),
    )
    .unwrap();
    add_widget(&app, params);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, vec![])
}

fn a_table(theme: bool) -> toml::Value {
    let mut params = toml::toml! {
        kind = "table" x = 0.0 y = 0.0 width = 300.0 height = 120.0
        titles = ["name", "size"]
        options = ["hero.png\u{1f}12 KB", "map.png\u{1f}3 KB"]
    };
    if theme {
        params.insert("theme".into(), toml::Value::String("themes/t.toml".into()));
    }
    params.into()
}

#[test]
fn a_theme_that_hides_a_tables_rules_draws_none() {
    let ruled = rules(&themed(
        "[colors]\ntable_rule = \"#808080\"\n",
        &a_table(true),
    ));
    let bare = rules(&themed(
        "[colors]\ntable_rule = \"#00000000\"\n",
        &a_table(true),
    ));
    assert!(ruled > 0, "a table rules its columns by default: {ruled}");
    assert_eq!(bare, 0, "and a theme that hides them draws none");
}

#[test]
fn a_theme_names_the_plate_a_picked_row_wears() {
    let want = egui::Color32::from_rgb(0, 128, 64);
    let mut params = a_table(true);
    params.as_table_mut().unwrap().insert(
        "text".into(),
        toml::Value::String("map.png\u{1f}3 KB".into()),
    );
    let painted = fills(&themed("[colors]\nrow_selected = \"#008040\"\n", &params));
    assert!(
        painted.contains(&want),
        "the picked row wears the theme's colour: {painted:?}"
    );
}

#[test]
fn a_theme_that_hides_the_striping_lays_no_plate_under_a_row() {
    let stripe = egui::Color32::from_rgb(20, 30, 40);
    let striped = fills(&themed(
        "[colors]\nrow_stripe = \"#141e28\"\n",
        &a_table(true),
    ));
    assert!(
        striped.contains(&stripe),
        "every other row is plated: {striped:?}"
    );
    let bare = fills(&themed(
        "[colors]\nrow_stripe = \"#00000000\"\n",
        &a_table(true),
    ));
    assert!(
        !bare.contains(&stripe),
        "and a theme that hides it lays none"
    );
}

#[test]
fn a_theme_dresses_the_box_a_row_view_draws_in() {
    let want = egui::Color32::from_rgb(16, 16, 32);
    let bare = fills(&themed("[colors]\nink = \"#101020\"\n", &a_table(true)));
    assert!(
        !bare.contains(&want),
        "a table with no fill of its own paints no box: {bare:?}"
    );
    let dressed = fills(&themed(
        "[colors]\nink = \"#101020\"\n[table]\nfill = \"ink\"\ncorner_radius = 6\n",
        &a_table(true),
    ));
    assert!(
        dressed.contains(&want),
        "and a theme that gives it one paints it: {dressed:?}"
    );
}

#[test]
fn a_list_takes_the_row_colours_its_theme_names() {
    let want = egui::Color32::from_rgb(200, 30, 30);
    let params = toml::toml! {
        kind = "list" x = 0.0 y = 0.0 width = 200.0 height = 200.0
        row_height = 18.0 options = ["One", "Two"] text = "Two"
        theme = "themes/t.toml"
    };
    let painted = fills(&themed(
        "[colors]\nrow_selected = \"#c81e1e\"\n",
        &params.into(),
    ));
    assert!(
        painted.contains(&want),
        "a list's picked row wears it too: {painted:?}"
    );
}

#[test]
fn a_table_takes_the_column_shares_the_scene_states() {
    let (_dir, app) = app();
    let params = toml::toml! {
        kind = "table" x = 0.0 y = 0.0 width = 300.0 height = 120.0
        titles = ["name", "size"]
        widths = ["3", "1"]
        options = ["hero.png\u{1f}12 KB"]
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = texts(&pass(&app, &ctx, vec![]));
    let at = |name: &str| {
        drawn
            .iter()
            .find(|(t, _)| t == name)
            .map_or_else(|| panic!("{name} drawn: {drawn:?}"), |(_, p)| *p)
    };
    // Three quarters of the 300 the table was given is 225, and the second
    // column starts there.
    let second = at("12 KB").x - at("hero.png").x;
    assert!(
        (200.0..250.0).contains(&second),
        "the stated shares divide the width: {second}"
    );
}

#[test]
fn a_table_without_a_header_draws_every_row() {
    let rows = |header: bool| {
        let (_dir, app) = app();
        let mut params = toml::toml! {
            kind = "table" x = 0.0 y = 0.0 width = 300.0 height = 200.0
            options = ["one\u{1f}1", "two\u{1f}2", "three\u{1f}3"]
        };
        params.insert("header".into(), toml::Value::Boolean(header));
        add_widget(&app, &params.into());
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let drawn = texts(&pass(&app, &ctx, vec![]));
        drawn.iter().filter(|(t, _)| t == "one").count()
    };
    assert_eq!(rows(true), 1, "the first row names the columns");
    assert_eq!(
        rows(false),
        1,
        "and without a header it is a row like any other"
    );
}

#[test]
fn a_table_sorts_by_the_column_it_names() {
    let order = |sort: &str, reverse: bool| {
        let (_dir, app) = app();
        let mut params = toml::toml! {
            kind = "table" x = 0.0 y = 0.0 width = 300.0 height = 200.0
            titles = ["name", "size>"]
            options = ["b\u{1f}3 KB", "a\u{1f}12 KB", "c\u{1f}1 KB"]
        };
        params.insert("sort".into(), toml::Value::String(sort.into()));
        params.insert("reverse".into(), toml::Value::Boolean(reverse));
        add_widget(&app, &params.into());
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let mut rows: Vec<(String, f32)> = texts(&pass(&app, &ctx, vec![]))
            .into_iter()
            .filter(|(t, _)| ["a", "b", "c"].contains(&t.as_str()))
            .map(|(t, p)| (t, p.y))
            .collect();
        rows.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        rows.into_iter().map(|(t, _)| t).collect::<Vec<_>>()
    };
    assert_eq!(order("", false), ["b", "a", "c"], "as given");
    assert_eq!(order("name", false), ["a", "b", "c"], "by name");
    assert_eq!(order("name", true), ["c", "b", "a"], "and the other way");
    // A cell that starts with a number sorts as one: 1, 3, 12, not 1, 12, 3.
    assert_eq!(order("size", false), ["c", "b", "a"], "by size, as numbers");
}

#[test]
fn a_click_on_a_sortable_header_orders_by_that_column() {
    let (_dir, mut app) = app();
    let params = toml::toml! {
        kind = "table" x = 0.0 y = 0.0 width = 300.0 height = 200.0
        titles = ["name", "size"]
        sortable = true
        options = ["b\u{1f}3", "a\u{1f}12"]
    };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let head = texts(&pass(&app, &ctx, vec![]))
        .into_iter()
        .find(|(t, _)| t == "name")
        .expect("the header names its column")
        .1;
    let hit = head + egui::vec2(4.0, 4.0);
    pass(&app, &ctx, press(hit, true));
    pass(&app, &ctx, press(hit, false));
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "sort"),
        toml::Value::String("name".into()),
        "the click sorts by the column it landed on"
    );
    assert_eq!(
        property(&app, entity, "reverse"),
        toml::Value::Boolean(false)
    );
    pass(&app, &ctx, press(hit, true));
    pass(&app, &ctx, press(hit, false));
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "reverse"),
        toml::Value::Boolean(true),
        "and the same column again turns it round"
    );
}

#[test]
fn a_dragged_row_reports_where_it_was_let_go() {
    let script = "pub fn init(this) {\n    this.said = \"\";\n}\n\
                  pub fn on_move(this, what) {\n    this.said = `${what[0]}|${what[1]}|${what[2]}`;\n}\n\
                  pub fn said(this) {\n    this.said\n}\n";
    // Where in the row the drop lands: its top eighth is the gap above it,
    // and the middle of a tree's row is inside it.
    let dropped = |down: f32| {
        let (_dir, mut app) = app_with_script(script);
        let root = app.engine.root();
        let owner = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Owner", root);
        let host = app.engine.script_host().unwrap();
        host.attach(balaur::node_id_of(owner), "scripts/paint.rn")
            .unwrap();
        add_child_widget(
            &app,
            owner,
            "Rows",
            &toml::toml! {
                kind = "tree" x = 0.0 y = 0.0 width = 200.0 height = 200.0
                row_height = 20.0 reorderable = true on_move = "on_move"
                options = ["One", "Two", "Three"]
            }
            .into(),
        );
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let drawn = texts(&pass(&app, &ctx, vec![]));
        let at = |name: &str| {
            drawn
                .iter()
                .find(|(t, _)| t == name)
                .map_or_else(|| panic!("{name} is drawn"), |(_, p)| *p)
        };
        let from = at("One");
        let to = at("Three") + egui::vec2(0.0, down);
        // A press, a move past egui's drag threshold, then let go.
        pass(&app, &ctx, press(from, true));
        pass(&app, &ctx, vec![egui::Event::PointerMoved(to)]);
        pass(&app, &ctx, vec![egui::Event::PointerMoved(to)]);
        pass(&app, &ctx, press(to, false));
        consume_input(&mut app);
        match host.call_on(balaur::node_id_of(owner), "said", &[]) {
            Some(balaur_script::Value::Str(said)) => said,
            other => panic!("the script answered {other:?}"),
        }
    };
    assert_eq!(
        dropped(7.0),
        "One|Three|into",
        "the middle of a tree's row takes the dragged one inside it"
    );
    assert_eq!(
        dropped(1.0),
        "One|Three|before",
        "and the gap above it puts the row there"
    );
}

#[test]
fn a_list_that_does_not_reorder_reports_no_drop() {
    let script = "pub fn init(this) {\n    this.said = \"\";\n}\n\
                  pub fn on_move(this, what) {\n    this.said = `${what[0]}`;\n}\n\
                  pub fn said(this) {\n    this.said\n}\n";
    let (_dir, mut app) = app_with_script(script);
    let root = app.engine.root();
    let owner = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Owner", root);
    let host = app.engine.script_host().unwrap();
    host.attach(balaur::node_id_of(owner), "scripts/paint.rn")
        .unwrap();
    add_child_widget(
        &app,
        owner,
        "Rows",
        &toml::toml! {
            kind = "list" x = 0.0 y = 0.0 width = 200.0 height = 200.0
            row_height = 20.0 on_move = "on_move"
            options = ["One", "Two", "Three"]
        }
        .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = texts(&pass(&app, &ctx, vec![]));
    let at = |name: &str| {
        drawn
            .iter()
            .find(|(t, _)| t == name)
            .map_or_else(|| panic!("{name} is drawn"), |(_, p)| *p)
    };
    let (from, to) = (at("One"), at("Three"));
    pass(&app, &ctx, press(from, true));
    pass(&app, &ctx, vec![egui::Event::PointerMoved(to)]);
    pass(&app, &ctx, press(to, false));
    consume_input(&mut app);
    assert_eq!(
        host.call_on(balaur::node_id_of(owner), "said", &[]),
        Some(balaur_script::Value::Str(String::new())),
        "a view that does not reorder hears nothing from a drag over it"
    );
}

#[test]
fn a_card_dragged_out_of_its_list_reports_itself_where_it_was_let_go() {
    let script = "pub fn init(this) {\n    this.said = \"\";\n}\n\
                  pub fn on_drop(this, card) {\n    this.said = card;\n}\n\
                  pub fn said(this) {\n    this.said\n}\n";
    let dropped = |to: egui::Pos2| {
        let (_dir, mut app) = app_with_script(script);
        let root = app.engine.root();
        let owner = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Owner", root);
        let host = app.engine.script_host().unwrap();
        host.attach(balaur::node_id_of(owner), "scripts/paint.rn")
            .unwrap();
        add_child_widget(
            &app,
            owner,
            "Cards",
            &toml::toml! {
                kind = "list" x = 0.0 y = 0.0 width = 240.0 height = 120.0
                columns = 3 draggable = true on_drop = "on_drop"
                options = ["One", "Two", "Three"]
            }
            .into(),
        );
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let drawn = texts(&pass(&app, &ctx, vec![]));
        let from = drawn
            .iter()
            .find(|(t, _)| t == "One")
            .map_or_else(|| panic!("One is drawn"), |(_, p)| *p);
        pass(&app, &ctx, press(from, true));
        pass(&app, &ctx, vec![egui::Event::PointerMoved(to)]);
        pass(&app, &ctx, vec![egui::Event::PointerMoved(to)]);
        pass(&app, &ctx, press(to, false));
        consume_input(&mut app);
        match host.call_on(balaur::node_id_of(owner), "said", &[]) {
            Some(balaur_script::Value::Str(said)) => said,
            other => panic!("the script answered {other:?}"),
        }
    };
    assert_eq!(
        dropped(egui::pos2(500.0, 400.0)),
        "One",
        "a card let go outside its list names itself"
    );
    assert_eq!(
        dropped(egui::pos2(200.0, 20.0)),
        "",
        "and one let go back over the list is no drop"
    );
}

#[test]
fn a_table_puts_every_cell_where_its_text_align_says() {
    let places = |align: &str| {
        let (_dir, app) = app();
        let mut params = toml::toml! {
            kind = "table" x = 0.0 y = 0.0 width = 300.0 height = 120.0
            titles = ["name", "size>"]
            options = ["hero\u{1f}12 KB"]
        };
        params.insert("text_align".into(), toml::Value::String(align.into()));
        add_widget(&app, &params.into());
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let drawn = texts(&pass(&app, &ctx, vec![]));
        drawn
            .iter()
            .find(|(t, _)| t == "hero")
            .map_or_else(|| panic!("the cell is drawn: {drawn:?}"), |(_, p)| p.x)
    };
    let start = places("start");
    assert!(
        places("end") > start + 40.0,
        "the whole table reads against the right edge: {start} to {}",
        places("end")
    );
    assert!(
        places("center") > start && places("center") < places("end"),
        "and centred sits between the two"
    );
}
