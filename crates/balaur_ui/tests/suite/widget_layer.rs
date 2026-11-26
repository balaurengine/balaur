//! Where the widget layer puts things: containers, sizes, clicks.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use balaur_core::App;
use balaur_core::hecs::Entity;
use egui::pos2;

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

    set_shown(&app, entity, false);
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

// The four edge anchors, which the corners and the centre did not cover and
// which every Godot `Control` preset short of the wide ones needs.
#[test]
fn an_edge_anchor_pins_one_axis_and_centres_the_other() {
    let (_dir, app) = app();
    let at = |anchor: &str| {
        let params = toml::toml! { kind = "label" text = "x" x = 0.0 y = 0.0 anchor = anchor };
        add_widget(&app, &params.into())
    };
    let (left, right, top, bottom, middle) = (
        at("center_left"),
        at("center_right"),
        at("center_top"),
        at("center_bottom"),
        at("center"),
    );
    let ctx = egui::Context::default();
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);

    let rect = |entity: Entity| {
        ctx.memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", entity))))
            .expect("the label drew, so its area has a rect")
    };
    let (l, r, t, b, m) = (
        rect(left),
        rect(right),
        rect(top),
        rect(bottom),
        rect(middle),
    );
    assert!(l.center().x < m.center().x, "center_left sits left: {l:?}");
    assert!(
        r.center().x > m.center().x,
        "center_right sits right: {r:?}"
    );
    assert!(t.center().y < m.center().y, "center_top sits high: {t:?}");
    assert!(b.center().y > m.center().y, "center_bottom sits low: {b:?}");
    // The axis the anchor does not pin stays centred, which is the whole
    // difference between an edge anchor and the corner beside it.
    for (name, edge) in [("center_left", l), ("center_right", r)] {
        assert!(
            (edge.center().y - m.center().y).abs() < 1.0,
            "{name} moved off the vertical centre: {edge:?}"
        );
    }
    for (name, edge) in [("center_top", t), ("center_bottom", b)] {
        assert!(
            (edge.center().x - m.center().x).abs() < 1.0,
            "{name} moved off the horizontal centre: {edge:?}"
        );
    }
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
        ctx.memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", entity))))
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
        &toml::toml! { kind = "row" x = 0.0 y = 0.0 widget_layout = { gap = [10.0, 10.0] } }.into(),
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
        .memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", row))))
        .expect("the row drew");

    // A column of the same three, to compare against.
    let (_dir2, app2) = app();
    let column = add_widget(
        &app2,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 widget_layout = { gap = [10.0, 10.0] } }
            .into(),
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
        .memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", column))))
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

// The gap is the thing a designer reaches for first, so it has to move the
// layout rather than merely be stored.
#[test]
fn the_gap_widens_a_row() {
    let measure = |gap: f64| {
        let (_dir, app) = app();
        let row = add_widget(
            &app,
            &toml::toml! { kind = "row" x = 0.0 y = 0.0 widget_layout = { gap = [gap, gap] } }
                .into(),
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
        ctx.memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", row))))
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

// A child is placed by its parent. Its own `x`, `y` and `anchor` are the
// keys of a widget that stands alone, and a menu that moved when you nudged
// one entry would not be a menu.
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
        ctx.memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", column))))
            .expect("the column drew")
    };
    assert_eq!(measure(0.0, 0.0), measure(300.0, 200.0));
}

#[test]
fn a_button_inside_a_container_still_takes_its_click() {
    let (_dir, mut app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 padding = 0.0 widget_layout = { gap = [0.0, 0.0] } }.into(),
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
        .memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", column))))
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

// A panel with nothing in it is the panel it always was; one with children
// lays them out. Both had to stay true, since every existing scene's panels
// are the first kind.
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
        ctx.memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", entity))))
            .expect("the panel drew")
    };
    assert!(
        rect(full).height() > rect(bare).height() + 40.0,
        "the panel did not grow around its children: bare {:?} full {:?}",
        rect(bare),
        rect(full)
    );
}

#[test]
fn grow_divides_what_the_fixed_children_leave() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 width = 300.0 height = 400.0 widget_layout = { gap = [0.0, 0.0] } }
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
        .memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", column))))
        .expect("the column drew");
    // 400 tall less the 40 the bar states: the grower takes the rest, and
    // nothing overflows the box the column was given.
    assert!(
        (column_rect.height() - 400.0).abs() < 2.0,
        "the column did not hold its stated height: {column_rect:?}"
    );
}

/// Text the pass drew, in draw order.
fn drew_texts(out: &egui::FullOutput) -> Vec<String> {
    out.shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

/// A viewport at (300, 200), 100 by 100, with `hud` in it, and `chrome` on the
/// window beside it: an editor's stage and its own shell.
fn screen_and_window(app: &App) -> (Entity, Entity, Entity) {
    let screen = add_widget(
        app,
        &toml::toml! { kind = "viewport" x = 300.0 y = 200.0 width = 100.0 height = 100.0 }.into(),
    );
    let hud = add_child_widget(
        app,
        screen,
        "Hud",
        &toml::toml! { kind = "panel" text = "hud" x = 0.0 y = 0.0 width = 40.0 height = 20.0 }
            .into(),
    );
    let chrome = add_widget(
        app,
        &toml::toml! { kind = "panel" text = "chrome" x = 0.0 y = 0.0 width = 40.0 height = 20.0 }
            .into(),
    );
    (screen, hud, chrome)
}

// A widget in a viewport is placed against it as a root is against the
// window; one beside it keeps the window.
#[test]
fn a_widget_in_a_viewport_is_placed_against_the_viewport() {
    let (_dir, app) = app();
    let (_screen, hud, chrome) = screen_and_window(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = |entity: Entity| {
        ctx.memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", entity))))
            .expect("it drew")
    };
    assert!(
        (rect(hud).min - pos2(300.0, 200.0)).length() < 1.0,
        "the hud is not at the viewport's corner: {:?}",
        rect(hud)
    );
    assert!(
        rect(chrome).min.x < 10.0,
        "a root beside the viewport moved with it: {:?}",
        rect(chrome)
    );
}

// A viewport with no size is a screen that is off: what is in it draws
// nothing, and the window's roots draw as before.
#[test]
fn a_viewport_with_no_size_draws_nothing_in_it() {
    let (_dir, app) = app();
    let (screen, _hud, _chrome) = screen_and_window(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let before = drew_texts(&pass(&app, &ctx, vec![]));
    assert!(
        before.iter().any(|t| t == "hud"),
        "control: the hud drew: {before:?}"
    );
    let zero = toml::Value::Float(0.0);
    balaur::components::set_property(&app.engine, screen, "widget", "width", &zero).unwrap();
    settle(&app, &ctx);
    let after = drew_texts(&pass(&app, &ctx, vec![]));
    assert!(
        after.iter().any(|t| t == "chrome") && !after.iter().any(|t| t == "hud"),
        "a viewport with no size still drew what is in it: {after:?}"
    );
}

// What runs past a viewport's edge is cut there, and a click past the edge
// reaches nothing in it.
#[test]
fn a_widget_past_a_viewports_edge_is_clipped_and_takes_no_click() {
    let (_dir, mut app) = app();
    let screen = add_widget(
        &app,
        &toml::toml! { kind = "viewport" x = 100.0 y = 100.0 width = 100.0 height = 100.0 }.into(),
    );
    let strip = add_child_widget(
        &app,
        screen,
        "Strip",
        &toml::toml! { kind = "row" x = 0.0 y = 0.0 width = 300.0 height = 30.0 }.into(),
    );
    let inside = add_child_widget(
        &app,
        strip,
        "Inside",
        &toml::toml! { kind = "button" text = "in" width = 60.0 height = 30.0 }.into(),
    );
    let _gap = add_child_widget(
        &app,
        strip,
        "Gap",
        &toml::toml! { kind = "row" width = 120.0 height = 30.0 }.into(),
    );
    let outside = add_child_widget(
        &app,
        strip,
        "Outside",
        &toml::toml! { kind = "button" text = "out" width = 60.0 height = 30.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let click_on = |app: &mut App, entity: Entity| {
        let at = balaur_ui::widget_rect(entity).expect("laid out").center();
        pass(app, &ctx, press(at, true));
        pass(app, &ctx, press(at, false));
        consume_input(app);
    };
    click_on(&mut app, inside);
    assert!(
        clicked(&app, inside),
        "control: a button inside the viewport takes a click"
    );
    let out_at = balaur_ui::widget_rect(outside).expect("laid out");
    assert!(
        out_at.min.x >= 200.0,
        "control: the second button is past the edge: {out_at:?}"
    );
    click_on(&mut app, outside);
    assert!(
        !clicked(&app, outside),
        "a button past the viewport's edge took a click"
    );
    let out = pass(&app, &ctx, vec![]);
    assert!(
        !out.shapes.is_empty(),
        "control: the viewport's widgets drew"
    );
    let past: Vec<egui::Rect> = out
        .shapes
        .iter()
        .map(|shape| shape.clip_rect)
        .filter(|clip| clip.max.x > 200.5)
        .collect();
    assert!(
        past.is_empty(),
        "shapes drew past the viewport's right edge: {past:?}"
    );
}

// A dialog in a viewport opens inside it, not in the middle of the window.
#[test]
fn a_dialog_in_a_viewport_opens_inside_it() {
    let (_dir, app) = app();
    let screen = add_widget(
        &app,
        &toml::toml! { kind = "viewport" x = 500.0 y = 300.0 width = 200.0 height = 160.0 }.into(),
    );
    let dialog = add_child_widget(
        &app,
        screen,
        "Ask",
        &toml::toml! { kind = "dialog" text = "sure?" width = 80.0 height = 40.0 widget_window = { open = true } }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = ctx
        .memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", dialog))))
        .expect("the dialog drew");
    let viewport = egui::Rect::from_min_size(pos2(500.0, 300.0), egui::vec2(200.0, 160.0));
    assert!(
        viewport.expand(1.0).contains_rect(drawn),
        "the dialog left its viewport: {drawn:?}"
    );
}

// The point of measuring rather than remembering: a container divides its
// room by what its children need *now*, not by what they drew last frame.
#[test]
fn a_container_sizes_to_a_label_that_changed_this_frame() {
    let (_dir, app) = app();
    let row = add_widget(
        &app,
        &toml::toml! { kind = "row" x = 0.0 y = 0.0 width = 400.0 height = 60.0 widget_layout = { gap = [0.0, 0.0] } }
            .into(),
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

// A scroll holds its box whatever is inside it: that is the difference
// between a list that scrolls and one that stretches its panel.
#[test]
fn a_scroll_keeps_its_box_however_long_its_contents() {
    let measure = |kind: &str| {
        let (_dir, app) = app();
        let holder = add_widget(
            &app,
            &toml::toml! {
                kind = kind x = 0.0 y = 0.0 width = 200.0 height = 120.0
                widget_layout = { gap = [0.0, 0.0] }
            }
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
        ctx.memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", holder))))
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

// A scroll that moves one way fills the other: a column in a vertical scroll
// is as wide as the scroll, with nothing inside it stating the width.
#[test]
fn a_one_way_scroll_fills_the_other_way() {
    let inner_width = |axis: &str| {
        let (_dir, app) = app();
        // A scroll that takes what its sheet leaves, as a dock's does: one
        // with a stated width would fill it whatever its axis said.
        let sheet = add_widget(
            &app,
            &toml::toml! { kind = "column" x = 0.0 y = 0.0 width = 300.0 height = 300.0 }.into(),
        );
        add_child_widget(
            &app,
            sheet,
            "head",
            &toml::toml! { kind = "panel" height = 40.0 }.into(),
        );
        let holder = add_child_widget(
            &app,
            sheet,
            "scroll",
            &toml::toml! { kind = "scroll" grow = 1.0 widget_scroll = { axis = axis } }.into(),
        );
        let column = add_child_widget(
            &app,
            holder,
            "form",
            &toml::toml! { kind = "column" }.into(),
        );
        // A filled box of a height nothing else has and no width of its own:
        // it is as wide as the column stretches it.
        add_child_widget(
            &app,
            column,
            "row",
            &toml::toml! { kind = "panel" height = 21.0 fill = "#ff0000" }.into(),
        );
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let out = pass(&app, &ctx, vec![]);
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::epaint::Shape::Rect(r) if (r.rect.height() - 21.0).abs() < 1.0 => {
                    Some(r.rect.width())
                }
                _ => None,
            })
            .fold(0.0_f32, f32::max)
    };
    let filled = inner_width("vertical");
    let hugged = inner_width("both");
    assert!(
        filled > 250.0,
        "a vertical scroll left its column hugging: {filled}"
    );
    assert!(
        hugged + 100.0 < filled,
        "control: a scroll free both ways should hug ({hugged} against {filled})"
    );
}

// A tab shows one page and names the rest, so adding a page is adding a node.
#[test]
fn a_tab_shows_the_page_it_names_and_only_that_one() {
    let show = |active: &str| {
        let (_dir, app) = app();
        let tabs = add_widget(
            &app,
            &toml::toml! { kind = "tabs" x = 0.0 y = 0.0 width = 300.0 height = 200.0 widget_tabs = { current_page = active } }
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

// A seam only takes a drag when one of the pair states a size to write it
// to; between two growers there is nothing a drag could mean.
#[test]
fn a_handle_is_only_a_grab_where_a_neighbour_states_a_size() {
    let (_dir, mut app) = app();
    let row = add_widget(
        &app,
        &toml::toml! { kind = "row" x = 0.0 y = 0.0 width = 400.0 height = 100.0 widget_layout = { gap = [8.0, 8.0], splitter_width = 8.0 } }
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

// The host is what sets the shape; this is the half egui carries.
#[test]
fn a_seam_asks_the_pointer_to_look_like_a_resize() {
    let (_dir, app) = app();
    let row = add_widget(
        &app,
        &toml::toml! { kind = "row" x = 0.0 y = 0.0 width = 400.0 height = 100.0 widget_layout = { gap = [8.0, 8.0], splitter_width = 8.0 } }
            .into(),
    );
    add_child_widget(
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
    let seam = pos2(124.0, 50.0);
    pass(&app, &ctx, vec![egui::Event::PointerMoved(seam)]);
    let over = pass(&app, &ctx, vec![egui::Event::PointerMoved(seam)]);
    assert_eq!(
        over.platform_output.cursor_icon,
        egui::CursorIcon::ResizeHorizontal,
        "the seam asked for no cursor of its own"
    );
    let away = pass(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(pos2(300.0, 50.0))],
    );
    assert_eq!(
        away.platform_output.cursor_icon,
        egui::CursorIcon::Default,
        "and it asks for nothing away from the seam"
    );
}

// A grouping node with no widget of its own should not break the chain: a
// menu is usually a panel with an empty node or two inside it.
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
        .memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", column))))
        .expect("the column drew");
    // The child gets no area of its own: it is laid out, not placed.
    let child_area = ctx.memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", entry))));
    assert!(
        child_area.is_none(),
        "the child was placed on its own at {child_area:?}"
    );
    assert!(laid.width() > 60.0, "the column did not adopt it: {laid:?}");
}

// A row's own `padding` has to reach the measure as well as the draw, or the
// parent places the sibling after it twice the padding too soon.
#[test]
fn a_containers_own_padding_is_measured_as_well_as_drawn() {
    let width = |padding: f64| {
        let (_dir, app) = app();
        let column = add_widget(
            &app,
            &toml::toml! { kind = "column" x = 0.0 y = 0.0 widget_layout = { gap = [0.0, 0.0] } }
                .into(),
        );
        let row = add_child_widget(
            &app,
            column,
            "row",
            &toml::toml! { kind = "row" padding = padding widget_layout = { gap = [0.0, 0.0] } }
                .into(),
        );
        add_child_widget(
            &app,
            row,
            "entry",
            &toml::toml! { kind = "label" text = "an entry wide enough to measure" }.into(),
        );
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        ctx.memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", column))))
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
        ctx.memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", panel))))
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

// Two pages showing the same text are told apart by their node names, which
// is what the schema says `active` holds.
#[test]
fn clicking_a_tab_writes_the_pages_node_name_and_says_so() {
    let (_dir, mut app) = app();
    let tabs = add_widget(
        &app,
        &toml::toml! { kind = "tabs" x = 0.0 y = 0.0 width = 300.0 height = 200.0 }.into(),
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
    let active = balaur::components::get(&app.engine, tabs, "widget_tabs")
        .and_then(|w| {
            w.get("current_page")
                .and_then(|v| v.as_str().map(str::to_owned))
        })
        .expect("the tab reports its active page");
    assert_eq!(
        active, "second",
        "a tab click wrote the page's label, not its node name"
    );
    assert_eq!(
        balaur_core::events::delivered_from(&app.engine, tabs, balaur_ui::VALUE_CHANGED_EVENT),
        vec![balaur_script::Value::Str("second".into())],
        "the page change is the tab's `change`, carrying the page's name"
    );
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
    let params = toml::toml! { kind = "text_field" text = "" x = 0.0 y = 0.0 width = 200.0 };
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

#[test]
fn a_surface_appears_at_full_alpha_rather_than_fading_in() {
    let (_dir, app) = app();
    let params = toml::toml! { kind = "button" text = "new" x = 0.0 y = 0.0 };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    // Fonts bind on the first pass, and a new area is sized invisibly on the
    // next, so the third is the first that draws.
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);
    let first = format!("{:?}", pass(&app, &ctx, vec![]).shapes);
    // The caption is shaped by the engine and painted as a glyph mesh.
    assert!(
        first.contains("Mesh"),
        "control: the button drew no caption"
    );
    let settled = format!("{:?}", pass(&app, &ctx, vec![]).shapes);
    assert_eq!(
        first, settled,
        "the surface's first drawn frame differs from its settled one, which is egui's fade-in"
    );

    // Hidden, then shown again: egui counts that as a new appearance, and
    // this is the path a collapsing panel takes.
    set_shown(&app, entity, false);
    pass(&app, &ctx, vec![]);
    set_shown(&app, entity, true);
    let shown = format!("{:?}", pass(&app, &ctx, vec![]).shapes);
    assert_eq!(
        shown, settled,
        "a surface shown again drew faded rather than at the alpha its theme set"
    );
}

// A hidden child takes no room, the way a hidden Control does not in a Godot container.
#[test]
fn a_hidden_child_leaves_its_room_to_the_others() {
    let column_height = |hide: bool| {
        let (_dir, app) = app();
        let column = add_widget(
            &app,
            &toml::toml! { kind = "column" x = 0.0 y = 0.0 widget_layout = { gap = [0.0, 0.0] } }
                .into(),
        );
        let first = toml::toml! { kind = "label" text = "First" height = 60.0 };
        let first = add_child_widget(&app, column, "First", &first.into());
        set_shown(&app, first, !hide);
        let second = toml::toml! { kind = "label" text = "Second" height = 60.0 };
        add_child_widget(&app, column, "Second", &second.into());
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        ctx.memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", column))))
            .expect("the column drew")
            .height()
    };
    let with = column_height(false);
    let without = column_height(true);
    assert!(
        without < with - 30.0,
        "hiding one of two 60px children takes its room off the column: \
         {without} against {with}"
    );
}

// Every root measures its bottom from the keyboard's top, but a backdrop,
// which stays behind it.
#[test]
fn a_root_keeps_above_the_on_screen_keyboard_and_a_backdrop_stays() {
    let (_dir, app) = app();
    let lifted = add_widget(
        &app,
        &toml::toml! {
            kind = "label" text = "send" anchor = "center_bottom" x = 0.0 y = 0.0
        }
        .into(),
    );
    let left = add_widget(&app, &toml::toml! { kind = "panel" anchor = "fill" }.into());
    let ctx = egui::Context::default();
    let bottom = |entity: Entity| {
        ctx.memory(|m| m.area_rect(egui::Id::unique(("balaur-widget", entity))))
            .expect("the label drew")
            .max
            .y
    };
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);
    let (before, unmoved) = (bottom(lifted), bottom(left));

    balaur_core::facts::update_device(&app.engine, |f| f.keyboard_height = 200.0);
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);
    assert!(
        (before - bottom(lifted) - 200.0).abs() < 1.0,
        "lifted by the keyboard: {before} -> {}",
        bottom(lifted)
    );
    assert!((bottom(left) - unmoved).abs() < 1.0, "the other stays put");
}

// Two viewports of different widths each lay out as their own size: a label's
// `[narrow]` table answers in the narrow one, its `[medium]` in the other.
#[test]
fn each_viewport_picks_its_own_class() {
    let (_dir, app) = app();
    let label: toml::Value = toml::toml! {
        kind = "label" text = "x" x = 10.0 y = 10.0 width = 40.0
        narrow = { width = 120.0 }
        medium = { width = 200.0 }
    }
    .into();
    let thin = add_widget(
        &app,
        &toml::toml! { kind = "viewport" x = 0.0 y = 0.0 width = 300.0 height = 100.0 }.into(),
    );
    let small = add_child_widget(&app, thin, "Thin", &label);
    let broad = add_widget(
        &app,
        &toml::toml! { kind = "viewport" x = 0.0 y = 200.0 width = 620.0 height = 100.0 }.into(),
    );
    let middling = add_child_widget(&app, broad, "Broad", &label);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);
    let width = |entity: Entity| balaur_ui::widget_rect(entity).expect("drawn").width();
    assert!(
        (width(small) - 120.0).abs() < 1.0,
        "the thin viewport's label took its narrow width: {}",
        width(small)
    );
    assert!(
        (width(middling) - 200.0).abs() < 1.0,
        "the broad viewport's label took its medium width: {}",
        width(middling)
    );
}

// A line is read against the viewport a widget sits in, not the window.
#[test]
fn hide_narrower_reads_the_viewport_a_widget_sits_in() {
    let (_dir, app) = app();
    let label: toml::Value =
        toml::toml! { kind = "label" text = "x" x = 10.0 y = 10.0 width = 40.0 hide_narrower = 400.0 }
            .into();
    let thin = add_widget(
        &app,
        &toml::toml! { kind = "viewport" x = 0.0 y = 0.0 width = 300.0 height = 100.0 }.into(),
    );
    let hidden = add_child_widget(&app, thin, "Thin", &label);
    let broad = add_widget(
        &app,
        &toml::toml! { kind = "viewport" x = 0.0 y = 200.0 width = 620.0 height = 100.0 }.into(),
    );
    let shown = add_child_widget(&app, broad, "Broad", &label);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);
    assert!(
        balaur_ui::widget_rect(shown).is_some(),
        "control: the label in the broad viewport draws"
    );
    assert!(
        balaur_ui::widget_rect(hidden).is_none(),
        "the label in a viewport narrower than its line still draws"
    );
}

// A line read in the pass a tree is built in, before the room it reads is
// laid out, is read again before that frame shows: a name too wide for the
// room a bar leaves it never draws, not even on the page's first frame.
#[test]
fn a_line_in_a_tree_built_this_tick_holds_on_its_first_frame() {
    let (_dir, app) = app();
    let ctx = egui::Context::default();
    let frame = |ctx: &egui::Context| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        };
        // A renderer uploads these; unapplied, dropping them panics.
        ctx.run_pass(input, |ctx| balaur_ui::run_pass(&app.engine, ctx))
            .textures_delta
            .clear();
    };
    // The fonts' frame, before any tree.
    frame(&ctx);
    let bar = add_widget(
        &app,
        &toml::toml! { kind = "row" x = 0.0 y = 0.0 width = 300.0 height = 40.0 padding = 0.0 widget_layout = { gap = [0.0, 0.0] } }
            .into(),
    );
    let brand = add_child_widget(
        &app,
        bar,
        "Brand",
        &toml::toml! { kind = "row" grow = 1.0 padding = 0.0 }.into(),
    );
    let name = add_child_widget(
        &app,
        brand,
        "Name",
        &toml::toml! { kind = "label" text = "Polyglot Pirates" hide_narrower = 128.0 }.into(),
    );
    add_child_widget(
        &app,
        bar,
        "End",
        &toml::toml! { kind = "label" text = "" width = 250.0 height = 10.0 }.into(),
    );
    frame(&ctx);
    assert!(
        balaur_ui::widget_rect(brand).is_some_and(|rect| rect.width() < 128.0),
        "control: the room the bar leaves is under the line: {:?}",
        balaur_ui::widget_rect(brand)
    );
    assert!(
        balaur_ui::widget_rect(name).is_none_or(|rect| rect.width() == 0.0),
        "the name drew on the tree's first frame: {:?}",
        balaur_ui::widget_rect(name)
    );
}
