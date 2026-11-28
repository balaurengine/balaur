//! A `scroll`'s rows as instances of a scene part, built only while in view:
//! two hundred items, a handful of rows, each told its item by `show(data)`.

use balaur_core::App;
use balaur_core::hecs::Entity;
use balaur_script::Value;

use crate::support::{add_widget, app_with_script, consume_input, pass, script_number};

/// A row: told its item, it takes the height the item names.
const ROW: &str = "pub class Row { node: Node, seen: int }
impl Row {
    pub fn show(data: any) {
        self.seen = data[\"n\"].as_int();
        self.node.widget.height = data[\"h\"].as_float();
    }
}
";

const PART: &str = "[[nodes]]\nid = \"row\"\nname = \"Row\"\nscript = { source = \"scripts/row.bs\" }\n\n[nodes.widget]\nkind = \"row\"\nheight = 30.0\n\n[[nodes]]\nid = \"word\"\nname = \"Word\"\nparent = \"row\"\n\n[nodes.widget]\nkind = \"label\"\ntext = \"word\"\n";

/// Items `n`, each `tall` high when `n` is odd and 30 when even.
fn items(count: i64, tall: f64) -> Vec<Value> {
    (0..count)
        .map(|n| {
            Value::Map(vec![
                ("n".into(), Value::Int(n)),
                ("h".into(), Value::Num(if n % 2 == 1 { tall } else { 30.0 })),
            ])
        })
        .collect()
}

/// A 300 high scroll whose rows are `parts/row.toml`, holding `items`.
fn scroll_of(items: Vec<Value>) -> (tempfile::TempDir, App, Entity, egui::Context) {
    let (dir, app) = app_with_script("row.bs", ROW);
    std::fs::create_dir_all(dir.path().join("parts")).unwrap();
    std::fs::write(dir.path().join("parts/row.toml"), PART).unwrap();
    let scroll = add_widget(
        &app,
        &toml::toml! { kind = "scroll" x = 0.0 y = 0.0 width = 200.0 height = 300.0 widget_scroll = { axis = "vertical" } }
            .into(),
    );
    balaur::components::add(
        &app.engine,
        scroll,
        "scene_rows",
        Some(&toml::toml! { scene = "parts/row.toml" ahead = 2 per_frame = 4 }.into()),
    )
    .unwrap();
    assert!(balaur_ui::set_rows(&app.engine, scroll, items));
    let ctx = egui::Context::default();
    (dir, app, scroll, ctx)
}

fn frames(app: &mut App, ctx: &egui::Context, count: usize) {
    for _ in 0..count {
        consume_input(app);
        pass(app, ctx, vec![]);
    }
}

/// How many rows have been built under the scroll.
fn built(app: &App, scroll: Entity) -> usize {
    let world = app.engine.world();
    let content = world
        .get::<&balaur_core::scene::Children>(scroll)
        .unwrap()
        .0[0];
    world
        .get::<&balaur_core::scene::Children>(content)
        .map_or(0, |rows| rows.0.len())
}

fn seen(app: &App, scroll: Entity, index: usize) -> Option<f64> {
    let row = balaur_ui::row_of(&app.engine, scroll, index)?;
    script_number(app, row, "seen")
}

#[test]
fn two_hundred_items_build_only_the_rows_in_view_and_near_it() {
    let (_dir, mut app, scroll, ctx) = scroll_of(items(200, 30.0));
    frames(&mut app, &ctx, 30);
    let rows = built(&app, scroll);
    assert!(
        (10..=16).contains(&rows),
        "{rows} rows built for a view of 10"
    );
    assert_eq!(seen(&app, scroll, 0), Some(0.0));
    assert_eq!(seen(&app, scroll, 9), Some(9.0));
    assert!(balaur_ui::row_of(&app.engine, scroll, 100).is_none());
    let content = app
        .engine
        .world()
        .get::<&balaur_core::scene::Children>(scroll)
        .unwrap()
        .0[0];
    let tall = balaur_ui::widget_rect(content)
        .expect("the rows' column drew")
        .height();
    assert!(
        (tall - 6000.0).abs() < 1.0,
        "the column holds every row's space: {tall}"
    );
}

#[test]
fn rows_scrolled_away_are_bound_again_to_the_rows_scrolled_to() {
    let (_dir, mut app, scroll, ctx) = scroll_of(items(200, 30.0));
    frames(&mut app, &ctx, 30);
    let before = built(&app, scroll);
    balaur::components::set_property(
        &app.engine,
        scroll,
        "widget_scroll",
        "scroll_offset",
        &toml::Value::Array(vec![toml::Value::Float(0.0), toml::Value::Float(3000.0)]),
    )
    .unwrap();
    frames(&mut app, &ctx, 30);
    assert_eq!(seen(&app, scroll, 100), Some(100.0));
    assert_eq!(seen(&app, scroll, 105), Some(105.0));
    assert!(
        balaur_ui::row_of(&app.engine, scroll, 0).is_none(),
        "the first row was let go of"
    );
    assert!(
        built(&app, scroll) <= before + 4,
        "rows were taken again rather than built"
    );
}

#[test]
fn rows_of_different_heights_hold_the_space_each_measured() {
    let (_dir, mut app, scroll, ctx) = scroll_of(items(20, 60.0));
    frames(&mut app, &ctx, 40);
    let top = |index: usize| {
        let row = balaur_ui::row_of(&app.engine, scroll, index).expect("the row is built");
        balaur_ui::widget_rect(row).expect("the row drew").top()
    };
    // 30, then 60, then 30: each row starts where the one before ended.
    assert!(
        (top(1) - top(0) - 30.0).abs() < 1.0,
        "{} {}",
        top(0),
        top(1)
    );
    assert!(
        (top(2) - top(1) - 60.0).abs() < 1.0,
        "{} {}",
        top(1),
        top(2)
    );
}

#[test]
fn rows_handed_no_items_are_let_go_of_and_hidden() {
    let (_dir, mut app, scroll, ctx) = scroll_of(items(50, 30.0));
    frames(&mut app, &ctx, 20);
    let row = balaur_ui::row_of(&app.engine, scroll, 0).expect("the first row is built");
    assert!(balaur_ui::set_rows(&app.engine, scroll, Vec::new()));
    frames(&mut app, &ctx, 3);
    assert!(balaur_ui::row_of(&app.engine, scroll, 0).is_none());
    let shown = app
        .engine
        .world()
        .get::<&balaur_core::scene::Appearance>(row)
        .map(|look| look.visible)
        .unwrap();
    assert!(!shown, "a row bound to nothing is hidden");
}
