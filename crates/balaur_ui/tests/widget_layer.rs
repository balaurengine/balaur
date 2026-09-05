//! Where the widget layer puts things: containers, sizes, clicks.

mod support;

use balaur_core::hecs::Entity;
use egui::pos2;
#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use support::*;

#[test]
fn a_hidden_widget_draws_nothing_and_takes_no_clicks() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "button" text = "hit" x = 0.0 y = 0.0 };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    pass(&app, &ctx, vec![]);
    // A new Area is sized invisibly on its first frame, so shapes come later.
    pass(&app, &ctx, vec![]);
    let shown = pass(&app, &ctx, vec![]);
    assert!(!shown.shapes.is_empty(), "control: the button drew nothing");

    let target = pos2(10.0, 10.0);
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);
    assert!(
        clicked(&app, entity),
        "control: the visible button did not take the click"
    );

    let hide = toml::toml! { visible = false };
    balaur::components::patch(&app.engine, entity, "widget", &hide.into()).unwrap();
    let hidden = pass(&app, &ctx, vec![]);
    assert!(
        hidden.shapes.is_empty(),
        "a hidden widget still drew {} shapes",
        hidden.shapes.len()
    );
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);
    assert!(!clicked(&app, entity), "a hidden button took a click");
}

#[test]
fn a_panel_takes_an_explicit_size() {
    let (_dir, app) = app();
    let sized_params = toml::toml! { kind = "panel" text = "p" width = 200.0 height = 120.0 };
    let sized = add_widget(&app, &sized_params.into());
    let auto_params = toml::toml! { kind = "panel" text = "p" anchor = "bottom_left" };
    let auto = add_widget(&app, &auto_params.into());
    let ctx = egui::Context::default();
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);

    let rect = |entity: Entity| {
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))))
            .expect("the panel drew, so its area has a rect")
    };
    let sized_rect = rect(sized);
    assert!(
        sized_rect.width() >= 200.0 && sized_rect.height() >= 120.0,
        "the explicit size was not honoured: {sized_rect:?}"
    );
    // The control: a content-sized panel of the same text stays smaller,
    // so the explicit size, not the content, made the first one big.
    let auto_rect = rect(auto);
    assert!(
        auto_rect.width() < 200.0 && auto_rect.height() < 120.0,
        "the auto panel is as big as the sized one: {auto_rect:?}"
    );
}

#[test]
fn a_row_lays_its_children_out_along_its_own_direction() {
    let (_dir, in_a_row) = app();
    let row = add_widget(
        &in_a_row,
        &toml::toml! { kind = "row" x = 0.0 y = 0.0 gap = 10.0 }.into(),
    );
    for label in ["one", "two", "three"] {
        add_child_widget(
            &in_a_row,
            row,
            label,
            &toml::toml! { kind = "label" text = label }.into(),
        );
    }
    let ctx = egui::Context::default();
    settle(&in_a_row, &ctx);
    let row_rect = ctx
        .memory(|m| m.area_rect(egui::Id::new(("balaur-widget", row))))
        .expect("the row drew");

    // A column of the same three, to compare against.
    let (_dir2, app2) = app();
    let column = add_widget(
        &app2,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 gap = 10.0 }.into(),
    );
    for label in ["one", "two", "three"] {
        add_child_widget(
            &app2,
            column,
            label,
            &toml::toml! { kind = "label" text = label }.into(),
        );
    }
    let ctx2 = egui::Context::default();
    settle(&app2, &ctx2);
    let column_rect = ctx2
        .memory(|m| m.area_rect(egui::Id::new(("balaur-widget", column))))
        .expect("the column drew");

    assert!(
        row_rect.width() > column_rect.width(),
        "a row should be the wider of the two: row {row_rect:?} column {column_rect:?}"
    );
    assert!(
        column_rect.height() > row_rect.height(),
        "a column should be the taller of the two: row {row_rect:?} column {column_rect:?}"
    );
}

/// The gap is the thing a designer reaches for first, so it has to move the
/// layout rather than merely be stored.
#[test]
fn the_gap_widens_a_row() {
    let measure = |gap: f64| {
        let (_dir, app) = app();
        let row = add_widget(
            &app,
            &toml::toml! { kind = "row" x = 0.0 y = 0.0 gap = gap }.into(),
        );
        for label in ["a", "b", "c"] {
            add_child_widget(
                &app,
                row,
                label,
                &toml::toml! { kind = "label" text = label }.into(),
            );
        }
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", row))))
            .expect("the row drew")
            .width()
    };
    let tight = measure(0.0);
    let loose = measure(40.0);
    // Two gaps between three children, so 40 apiece is 80 more.
    assert!(
        loose > tight + 60.0,
        "the gap did not widen the row: {tight} then {loose}"
    );
}

/// A child is placed by its parent. Its own `x`, `y` and `anchor` are the
/// keys of a widget that stands alone, and a menu that moved when you nudged
/// one entry would not be a menu.
#[test]
fn a_childs_own_offset_is_ignored_inside_a_container() {
    let measure = |x: f64, y: f64| {
        let (_dir, app) = app();
        let column = add_widget(
            &app,
            &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into(),
        );
        add_child_widget(
            &app,
            column,
            "entry",
            &toml::toml! { kind = "label" text = "entry" x = x y = y anchor = "bottom_right" }
                .into(),
        );
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", column))))
            .expect("the column drew")
    };
    assert_eq!(measure(0.0, 0.0), measure(300.0, 200.0));
}

#[test]
fn a_button_inside_a_container_still_takes_its_click() {
    let (_dir, mut app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 padding = 0.0 gap = 0.0 }.into(),
    );
    let button = add_child_widget(
        &app,
        column,
        "Play",
        &toml::toml! { kind = "button" text = "Play" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = ctx
        .memory(|m| m.area_rect(egui::Id::new(("balaur-widget", column))))
        .expect("the column drew");

    let target = rect.center();
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);
    assert!(
        clicked(&app, button),
        "a laid-out button took no click at {target:?} inside {rect:?}"
    );
}

/// A panel with nothing in it is the panel it always was; one with children
/// lays them out. Both had to stay true, since every existing scene's panels
/// are the first kind.
#[test]
fn a_panel_with_children_grows_around_them() {
    let (_dir, app) = app();
    let bare = add_widget(&app, &toml::toml! { kind = "panel" text = "menu" }.into());
    let full = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "menu" anchor = "bottom_left" }.into(),
    );
    for label in ["New game", "Load", "Options", "Quit"] {
        add_child_widget(
            &app,
            full,
            label,
            &toml::toml! { kind = "label" text = label }.into(),
        );
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = |entity: Entity| {
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))))
            .expect("the panel drew")
    };
    assert!(
        rect(full).height() > rect(bare).height() + 40.0,
        "the panel did not grow around its children: bare {:?} full {:?}",
        rect(bare),
        rect(full)
    );
}

/// A `draw` widget is a rect the scene places and a script fills: the node
/// owns where it is, the script owns what is in it.
#[test]
fn a_draw_widget_hands_its_rect_to_a_script() {
    let script = "pub fn fill() {\n    ui::label(\"filled from a script\", #{ size: 20 });\n}\n";
    let shapes = |target: &str| {
        let (_dir, app) = app_with_script(script);
        let params = toml::toml! {
            kind = "draw"
            x = 0.0
            y = 0.0
            width = 200.0
            height = 80.0
            draw = target
        };
        let entity = add_widget(&app, &params.into());
        let ctx = egui::Context::default();
        pass(&app, &ctx, vec![]);
        pass(&app, &ctx, vec![]);
        let out = pass(&app, &ctx, vec![]);
        let rect = ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))));
        (out.shapes.len(), rect)
    };
    let (drawn, rect) = shapes("scripts/paint.rn:fill");
    let (empty, _) = shapes("");
    assert!(
        drawn > empty,
        "the script drew nothing into the rect: {drawn} shapes against {empty}"
    );
    let rect = rect.expect("the draw widget reserved a rect");
    assert!(
        (rect.width() - 200.0).abs() < 2.0 && (rect.height() - 80.0).abs() < 2.0,
        "the draw widget did not take the size it states: {rect:?}"
    );
}

/// A container hands out the leftover along its own direction, so a shell
/// written as a tree lands on the rects the code used to compute.
#[test]
fn grow_divides_what_the_fixed_children_leave() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 gap = 0.0 width = 300.0 height = 400.0 }
            .into(),
    );
    let bar = add_child_widget(
        &app,
        column,
        "bar",
        &toml::toml! { kind = "panel" text = "" height = 40.0 }.into(),
    );
    let body = add_child_widget(
        &app,
        column,
        "body",
        &toml::toml! { kind = "panel" text = "" grow = 1.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);

    let seen = |entity: Entity| {
        app.engine
            .world()
            .get::<&balaur_ui::Widget>(entity)
            .is_ok_and(|w| w.kind == "panel")
    };
    assert!(seen(bar) && seen(body), "both panels are still panels");
    let column_rect = ctx
        .memory(|m| m.area_rect(egui::Id::new(("balaur-widget", column))))
        .expect("the column drew");
    // 400 tall less the 40 the bar states: the grower takes the rest, and
    // nothing overflows the box the column was given.
    assert!(
        (column_rect.height() - 400.0).abs() < 2.0,
        "the column did not hold its stated height: {column_rect:?}"
    );
}

/// A script's `draw_ui` reads this frame's rects, not last frame's. The
/// editor's whole shell is placed from them, and a frame behind is a frame
/// where every sheet is at 0,0.
#[test]
fn draw_ui_sees_the_rects_from_its_own_frame() {
    let script = "pub fn init(this) {\n    this.seen = 0.0;\n}\n\
                  pub fn draw_ui(this) {\n\
                  \x20   let r = ui::widget_rect(scene::get_node(\"Sheet\"));\n\
                  \x20   if r is Object { this.seen = r.w; }\n}\n";
    let (_dir, app) = app_with_script(script);
    let watcher =
        balaur::scene::spawn_node(&mut app.engine.world_mut(), "Watcher", app.engine.root());
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur::node_id_of(watcher), "scripts/paint.rn")
        .unwrap();
    let sheet = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Sheet", app.engine.root());
    balaur::components::add(
        &app.engine,
        sheet,
        "widget",
        Some(
            &toml::toml! { kind = "panel" text = "s" x = 0.0 y = 0.0 width = 200.0 height = 40.0 }
                .into(),
        ),
    )
    .unwrap();

    let ctx = egui::Context::default();
    // The first pass installs fonts and draws nothing; the second is the
    // first that draws, and `draw_ui` should already have the rect.
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);
    let seen = balaur::rune::rune_of(&app.engine).number_field(watcher, "seen");
    assert_eq!(
        seen,
        Some(200.0),
        "draw_ui read {seen:?} for a 200 px sheet drawn in the same frame"
    );
}

/// A panel of `draw` nodes should need one script, not one each: the rect is
/// the node's, and what fills it belongs to whatever owns the state.
#[test]
fn a_draw_node_asks_the_nearest_scripted_ancestor() {
    let script = "pub fn fill(this) {\n    ui::label(\"from the ancestor\", #{ size: 20 });\n}\n";
    let (_dir, app) = app_with_script(script);
    let owner = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Owner", app.engine.root());
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur::node_id_of(owner), "scripts/paint.rn")
        .unwrap();
    let column = add_child_widget(
        &app,
        owner,
        "column",
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 width = 200.0 height = 120.0 }.into(),
    );
    add_child_widget(
        &app,
        column,
        "body",
        &toml::toml! { kind = "draw" draw = "fill" grow = 1.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let drew = out.shapes.iter().any(|shape| match &shape.shape {
        egui::epaint::Shape::Text(text) => text.galley.text() == "from the ancestor",
        _ => false,
    });
    assert!(
        drew,
        "the draw node did not reach the script two levels above it"
    );
}

/// Two roots, two surfaces: the whole reason the editor's own chrome cannot
/// share the rect a game's HUD is confined to.
#[test]
fn a_root_draws_on_the_surface_it_names() {
    let (_dir, app) = app();
    let hud = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "hud" x = 0.0 y = 0.0 width = 40.0 height = 20.0 }
            .into(),
    );
    let chrome = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "chrome" x = 0.0 y = 0.0 width = 40.0 height = 20.0 layer = "shell" }
            .into(),
    );
    // The default surface is pushed into a corner, the way the editor points
    // it at the viewport. The named one is placed on the whole screen.
    {
        let config = app.engine.resource::<balaur_ui::WidgetLayerConfig>();
        let mut config = config.borrow_mut();
        config.rect = Some([300.0, 200.0, 100.0, 100.0]);
        config.layers.insert(
            "shell".into(),
            balaur_ui::Surface {
                enabled: true,
                rect: None,
            },
        );
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = |entity: Entity| {
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))))
            .expect("it drew")
    };
    assert!(
        rect(hud).min.x >= 299.0,
        "the default surface did not move the hud: {:?}",
        rect(hud)
    );
    assert!(
        rect(chrome).min.x < 10.0,
        "a named surface should not follow the default one: {:?}",
        rect(chrome)
    );

    // Turning the default surface off is what play does; the chrome stays.
    {
        let config = app.engine.resource::<balaur_ui::WidgetLayerConfig>();
        config.borrow_mut().enabled = false;
    }
    let out = pass(&app, &ctx, vec![]);
    let drew: Vec<String> = out
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text().to_string()),
            _ => None,
        })
        .collect();
    assert!(
        drew.iter().any(|t| t == "chrome") && !drew.iter().any(|t| t == "hud"),
        "turning the default surface off should not take the chrome with it: {drew:?}"
    );
}

/// The point of measuring rather than remembering: a container divides its
/// room by what its children need *now*, not by what they drew last frame.
#[test]
fn a_container_sizes_to_a_label_that_changed_this_frame() {
    let (_dir, app) = app();
    let row = add_widget(
        &app,
        &toml::toml! { kind = "row" x = 0.0 y = 0.0 width = 400.0 height = 60.0 gap = 0.0 }.into(),
    );
    let label = add_child_widget(
        &app,
        row,
        "label",
        &toml::toml! { kind = "label" text = "hi" }.into(),
    );
    add_child_widget(
        &app,
        row,
        "rest",
        &toml::toml! { kind = "panel" text = "X" grow = 1.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);

    // Where the grower's caption sits says where the grower starts, and that
    // is what the label's width decides.
    let caption_x = |out: &egui::FullOutput| {
        out.shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == "X" => Some(text.pos.x),
                _ => None,
            })
            .expect("the grower drew its caption")
    };
    let before = caption_x(&pass(&app, &ctx, vec![]));

    // One pass after the change, not two: nothing here has drawn the longer
    // caption yet, so a remembered width would still be the short one.
    balaur::components::add(
        &app.engine,
        label,
        "widget",
        Some(&toml::toml! { kind = "label" text = "a considerably longer caption" }.into()),
    )
    .unwrap();
    let after = caption_x(&pass(&app, &ctx, vec![]));
    assert!(
        after > before + 80.0,
        "the row did not re-divide for the new label in the same frame: {before} then {after}"
    );
}

/// A scroll holds its box whatever is inside it: that is the difference
/// between a list that scrolls and one that stretches its panel.
#[test]
fn a_scroll_keeps_its_box_however_long_its_contents() {
    let measure = |kind: &str| {
        let (_dir, app) = app();
        let holder = add_widget(
            &app,
            &toml::toml! { kind = kind x = 0.0 y = 0.0 width = 200.0 height = 120.0 gap = 0.0 }
                .into(),
        );
        for n in 0..40 {
            let text = format!("line {n}");
            add_child_widget(
                &app,
                holder,
                "line",
                &toml::toml! { kind = "label" text = text }.into(),
            );
        }
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", holder))))
            .expect("it drew")
            .height()
    };
    let scrolled = measure("scroll");
    // The control: the same forty labels in a column, which does stretch.
    let stretched = measure("column");
    assert!(
        scrolled < 200.0,
        "the scroll grew with its contents: {scrolled}"
    );
    assert!(
        stretched > scrolled + 200.0,
        "control: a column of the same labels should be far taller ({stretched} against {scrolled})"
    );
}

/// A tab shows one page and names the rest, so adding a page is adding a node.
#[test]
fn a_tab_shows_the_page_it_names_and_only_that_one() {
    let show = |active: &str| {
        let (_dir, app) = app();
        let tabs = add_widget(
            &app,
            &toml::toml! { kind = "tab" x = 0.0 y = 0.0 width = 300.0 height = 200.0 active = active }
                .into(),
        );
        for label in ["First", "Second"] {
            add_child_widget(
                &app,
                tabs,
                label,
                &toml::toml! { kind = "panel" text = label width = 120.0 height = 60.0 }.into(),
            );
        }
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let out = pass(&app, &ctx, vec![]);
        // Every page's name is on the strip; only the shown one draws twice,
        // as its own caption.
        let mut seen = std::collections::BTreeMap::new();
        for shape in &out.shapes {
            if let egui::epaint::Shape::Text(text) = &shape.shape {
                *seen.entry(text.galley.text().to_string()).or_insert(0) += 1;
            }
        }
        seen
    };
    let first = show("First");
    assert_eq!(
        first.get("First"),
        Some(&2),
        "the named page is not showing"
    );
    assert_eq!(
        first.get("Second"),
        Some(&1),
        "the other page is more than a tab: {first:?}"
    );
    let second = show("Second");
    assert_eq!(
        second.get("Second"),
        Some(&2),
        "naming the other page did not switch to it: {second:?}"
    );
}

/// A seam only takes a drag when one of the pair states a size to write it
/// to; between two growers there is nothing a drag could mean.
#[test]
fn a_handle_is_only_a_grab_where_a_neighbour_states_a_size() {
    let (_dir, mut app) = app();
    let row = add_widget(
        &app,
        &toml::toml! { kind = "row" x = 0.0 y = 0.0 width = 400.0 height = 100.0 gap = 8.0 handle = 8.0 }
            .into(),
    );
    let fixed = add_child_widget(
        &app,
        row,
        "fixed",
        &toml::toml! { kind = "panel" text = "" width = 120.0 }.into(),
    );
    add_child_widget(
        &app,
        row,
        "rest",
        &toml::toml! { kind = "panel" text = "" grow = 1.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let width_of = |entity: Entity| {
        balaur::components::get(&app.engine, entity, "widget")
            .and_then(|w| w.get("width").and_then(toml::Value::as_float))
            .expect("the widget reports its width")
    };
    assert!(
        (width_of(fixed) - 120.0).abs() < 0.5,
        "control: it starts at 120"
    );

    // The seam sits in the gap after the first child.
    let seam = pos2(124.0, 50.0);
    pass(&app, &ctx, press(seam, true));
    pass(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(pos2(seam.x + 40.0, seam.y))],
    );
    pass(&app, &ctx, press(pos2(seam.x + 40.0, seam.y), false));
    consume_input(&mut app);
    let width_of = |entity: Entity| {
        balaur::components::get(&app.engine, entity, "widget")
            .and_then(|w| w.get("width").and_then(toml::Value::as_float))
            .expect("the widget reports its width")
    };
    let after = width_of(fixed);
    assert!(
        after > 130.0,
        "dragging the seam did not widen the child that states a size: {after}"
    );
}

/// A grouping node with no widget of its own should not break the chain: a
/// menu is usually a panel with an empty node or two inside it.
#[test]
fn a_plain_node_between_container_and_child_is_seen_through() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into(),
    );
    let group = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Group", column);
    let entry = add_child_widget(
        &app,
        group,
        "entry",
        &toml::toml! { kind = "label" text = "a long entry to measure" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let laid = ctx
        .memory(|m| m.area_rect(egui::Id::new(("balaur-widget", column))))
        .expect("the column drew");
    // The child gets no area of its own: it is laid out, not placed.
    let child_area = ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entry))));
    assert!(
        child_area.is_none(),
        "the child was placed on its own at {child_area:?}"
    );
    assert!(laid.width() > 60.0, "the column did not adopt it: {laid:?}");
}

/// A row's own `padding` has to reach the measure as well as the draw, or the
/// parent places the sibling after it twice the padding too soon.
#[test]
fn a_containers_own_padding_is_measured_as_well_as_drawn() {
    let width = |padding: f64| {
        let (_dir, app) = app();
        let column = add_widget(
            &app,
            &toml::toml! { kind = "column" x = 0.0 y = 0.0 gap = 0.0 }.into(),
        );
        let row = add_child_widget(
            &app,
            column,
            "row",
            &toml::toml! { kind = "row" padding = padding gap = 0.0 }.into(),
        );
        add_child_widget(
            &app,
            row,
            "entry",
            &toml::toml! { kind = "label" text = "an entry wide enough to measure" }.into(),
        );
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", column))))
            .expect("the column drew")
    };
    let tight = width(0.0);
    let padded = width(20.0);
    // 20 each side, on both axes, and the column has to grow around it.
    assert!(
        padded.width() > tight.width() + 30.0,
        "the row's padding did not widen its parent: {tight:?} then {padded:?}"
    );
    assert!(
        padded.height() > tight.height() + 30.0,
        "the row's padding did not heighten its parent: {tight:?} then {padded:?}"
    );
}

/// A panel's `padding` is documented as applying; it used to reach the frame
/// only through the theme.
#[test]
fn a_panels_own_padding_applies() {
    let height = |padding: f64| {
        let (_dir, app) = app();
        let panel = add_widget(
            &app,
            &toml::toml! { kind = "panel" text = "p" x = 0.0 y = 0.0 padding = padding }.into(),
        );
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", panel))))
            .expect("the panel drew")
            .height()
    };
    // The built-in is 8 a side; 40 a side is 64 more than that.
    assert!(
        height(40.0) > height(0.0) + 50.0,
        "the panel's own padding did not reach the frame: {} then {}",
        height(0.0),
        height(40.0)
    );
}

/// Two pages showing the same text are told apart by their node names, which
/// is what the schema says `active` holds.
#[test]
fn clicking_a_tab_writes_the_pages_node_name() {
    let (_dir, mut app) = app();
    let tabs = add_widget(
        &app,
        &toml::toml! { kind = "tab" x = 0.0 y = 0.0 width = 300.0 height = 200.0 }.into(),
    );
    for name in ["first", "second"] {
        add_child_widget(
            &app,
            tabs,
            name,
            &toml::toml! { kind = "panel" text = "Same" width = 120.0 height = 60.0 }.into(),
        );
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);

    // Both strip buttons read "Same", so the second one is found by where it
    // was drawn rather than by what it says.
    let out = pass(&app, &ctx, vec![]);
    let mut strip: Vec<egui::Pos2> = out
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == "Same" => Some(text.pos),
            _ => None,
        })
        .collect();
    strip.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    assert!(strip.len() >= 2, "the strip drew {} labels", strip.len());
    let target = pos2(strip[1].x + 4.0, strip[1].y + 4.0);

    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);
    let active = balaur::components::get(&app.engine, tabs, "widget")
        .and_then(|w| w.get("active").and_then(|v| v.as_str().map(str::to_owned)))
        .expect("the tab reports its active page");
    assert_eq!(
        active, "second",
        "a tab click wrote the page's label, not its node name"
    );
}

/// `docs/PLAN-ui-layout.md` §5 promises one frame between a widget being
/// placed and a script reading its rect back; two would be a size change the
/// script never catches up with.
#[test]
fn a_script_reads_a_widget_rect_no_more_than_one_frame_late() {
    // `render.set_camera_2d` is the observation channel: nothing else in this
    // test writes it, and it is readable from Rust without a window.
    let script = "pub fn draw_ui(this) {\n    \
        let r = ui::widget_rect(this.node);\n    \
        if r is Object {\n        \
        render::set_camera_2d(r.w, 0.0, 60.0);\n    \
        }\n}\n";
    let (_dir, app) = app_with_script(script);
    let owner = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Owner", app.engine.root());
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur::node_id_of(owner), "scripts/paint.rn")
        .unwrap();
    balaur::components::add(
        &app.engine,
        owner,
        "widget",
        Some(
            &toml::toml! { kind = "panel" text = "" x = 0.0 y = 0.0 width = 100.0 height = 40.0 }
                .into(),
        ),
    )
    .unwrap();
    let seen = || {
        app.engine
            .resource::<balaur::render::CameraConfig2d>()
            .borrow()
            .center[0]
    };
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    assert!(
        (seen() - 100.0).abs() < 2.0,
        "control: the script never read the panel's rect ({})",
        seen()
    );

    balaur::components::patch(
        &app.engine,
        owner,
        "widget",
        &toml::toml! { width = 300.0 }.into(),
    )
    .unwrap();
    // One pass to draw at the new width, one for the script to have read it.
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);
    assert!(
        (seen() - 300.0).abs() < 2.0,
        "the rect a script reads is more than one frame stale ({})",
        seen()
    );
}

/// A script asking for focus is the pad's route in, so what it asks for has
/// to survive to the next draw.
#[test]
fn a_pending_move_from_a_script_is_taken_at_the_next_draw() {
    let (_dir, app) = app();
    let (_column, buttons) = menu(&app);
    keyboard(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    app.engine
        .resource::<balaur_ui::UiFocus>()
        .borrow_mut()
        .pending = Some(balaur_ui::Move::Next);
    pass(&app, &ctx, vec![]);
    assert_eq!(focused(&app), Some(buttons[0]));
}

#[test]
fn a_label_is_drawn_as_shaped_glyphs_not_egui_text() {
    let (_dir, app) = app();
    let params = toml::toml! { kind = "label" text = "Hello" x = 0.0 y = 0.0 };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);
    let shown = pass(&app, &ctx, vec![]);
    let meshes = shown
        .shapes
        .iter()
        .filter(|s| matches!(s.shape, egui::Shape::Mesh(_)))
        .count();
    let texts = shown
        .shapes
        .iter()
        .filter(|s| matches!(s.shape, egui::Shape::Text(_)))
        .count();
    assert!(meshes >= 1, "the label paints a glyph mesh");
    assert_eq!(texts, 0, "nothing goes through egui's own text layout");
}

#[test]
fn typing_into_a_field_lands_on_its_text_the_next_tick() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "field" text = "" x = 0.0 y = 0.0 width = 200.0 };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);
    let target = pos2(20.0, 12.0);
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    pass(&app, &ctx, vec![egui::Event::Text("hi".into())]);
    consume_input(&mut app);
    let text = balaur::components::get(&app.engine, entity, "widget")
        .unwrap()
        .get("text")
        .and_then(toml::Value::as_str)
        .map(str::to_string);
    assert_eq!(text.as_deref(), Some("hi"));
}
