//! Menus: the rows they open, where they open them, the submenus they
//! hang off, the chords that reach them shut, and the `context` menu any
//! widget can name.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use balaur_core::hecs::Entity;
use egui::pos2;

#[test]
fn a_menu_reports_the_item_that_was_picked() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "menu" text = "File" x = 0.0 y = 0.0 widget_menu = { options = ["Open", "Save"] } };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let head = pos2(20.0, 10.0);
    pass(&app, &ctx, press(head, true));
    pass(&app, &ctx, press(head, false));
    let open = pass(&app, &ctx, vec![]);
    let (_, at) = texts(&open)
        .into_iter()
        .find(|(t, _)| t == "Save")
        .expect("the list is open and holds its items");
    let item = pos2(at.x + 4.0, at.y + 4.0);
    pass(&app, &ctx, press(item, true));
    pass(&app, &ctx, press(item, false));
    consume_input(&mut app);
    assert_eq!(
        balaur_core::events::delivered_from(&app.engine, entity, balaur_ui::VALUE_CHANGED_EVENT),
        vec![balaur_script::Value::Str("Save".into())],
        "the pick is the menu's `change`"
    );
    assert_eq!(
        property(&app, entity, "text"),
        toml::Value::String("File".into()),
        "and the caption stays the menu's"
    );
}

// A menu's rows can be nodes: an icon, a shortcut and a tick are widgets
// like any other, which a flat list of strings cannot carry.
#[test]
fn a_menu_opens_its_child_rows() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "menu" text = "Menu" x = 10.0 y = 10.0 };
    let host = add_widget(&app, &params.into());
    let mut rows = Vec::new();
    for name in ["R0", "R1"] {
        let row = toml::toml! { kind = "button" text = "row" width = 173.0 height = 22.0 };
        rows.push(add_child_widget(&app, host, name, &row.into()));
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    // Counted by the boxes they take: a node caption shapes through the text
    // renderer, not an egui galley, so it is not a Text shape to find.
    let boxes = |out: &egui::FullOutput| {
        out.shapes
            .iter()
            .filter(|s| match &s.shape {
                // A row's box: the column holding them is as wide, and taller.
                egui::epaint::Shape::Rect(r) => {
                    (r.rect.width() - 173.0).abs() < 1.0 && (r.rect.height() - 22.0).abs() < 1.0
                }
                _ => false,
            })
            .map(|s| match &s.shape {
                egui::epaint::Shape::Rect(r) => r.rect,
                _ => unreachable!(),
            })
            .fold(Vec::<egui::Rect>::new(), |mut seen, rect| {
                // A button paints its frame and its fill over the same box.
                if !seen.contains(&rect) {
                    seen.push(rect);
                }
                seen
            })
    };
    assert!(
        boxes(&pass(&app, &ctx, vec![])).is_empty(),
        "a shut menu drew its rows"
    );
    let at = root_rect(&ctx, host).center();
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    let open = boxes(&pass(&app, &ctx, vec![]));
    assert_eq!(open.len(), 2, "the open menu drew {open:?}");
    assert!(open[1].min.y >= open[0].max.y, "the rows overlap: {open:?}");
    let second = open[1].center();
    pass(&app, &ctx, press(second, true));
    pass(&app, &ctx, press(second, false));
    consume_input(&mut app);
    assert!(
        clicked(&app, rows[1]),
        "a click on a row did not reach its node"
    );
    assert!(!clicked(&app, rows[0]), "the click reached the wrong row");
}

// An action row closes its menu; a toggle row says `keep_open` and stays,
// which is what a grid of panel ticks needs.
#[test]
fn a_row_closes_its_menu_unless_it_keeps_it_open() {
    for (keep, open_after) in [(false, false), (true, true)] {
        let (_dir, mut app) = app();
        let params = toml::toml! { kind = "menu" text = "Menu" x = 10.0 y = 10.0 };
        let host = add_widget(&app, &params.into());
        let row = toml::toml! {
            kind = "button" text = "row" width = 173.0 height = 22.0
            widget_child = { keep_open = keep }
        };
        add_child_widget(&app, host, "R0", &row.into());
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let at = root_rect(&ctx, host).center();
        pass(&app, &ctx, press(at, true));
        pass(&app, &ctx, press(at, false));
        // Shown the pass after the click, and hit-tested against that pass.
        pass(&app, &ctx, vec![]);
        let row_at = pos2(100.0, 51.0);
        pass(&app, &ctx, press(row_at, true));
        pass(&app, &ctx, press(row_at, false));
        consume_input(&mut app);
        let drawn = pass(&app, &ctx, vec![])
            .shapes
            .iter()
            .any(|s| match &s.shape {
                egui::epaint::Shape::Rect(r) => (r.rect.width() - 173.0).abs() < 1.0,
                _ => false,
            });
        assert_eq!(
            drawn, open_after,
            "keep_open = {keep}: menu open after the click = {drawn}"
        );
    }
}

// `showing` holds a menu's rows up with no click, which is the only way an
// offscreen run or a tutorial can show one.
#[test]
fn a_showing_menu_is_open_without_a_click() {
    let (_dir, app) = app();
    let params = toml::toml! { kind = "menu" text = "Menu" x = 10.0 y = 10.0 widget_menu = { showing = true } };
    let host = add_widget(&app, &params.into());
    let row = toml::toml! { kind = "button" text = "row" width = 173.0 height = 22.0 };
    add_child_widget(&app, host, "R0", &row.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = pass(&app, &ctx, vec![])
        .shapes
        .iter()
        .any(|s| match &s.shape {
            egui::epaint::Shape::Rect(r) => (r.rect.width() - 173.0).abs() < 1.0,
            _ => false,
        });
    assert!(drawn, "a showing menu drew no rows");
}

// A menu whose rows are nodes is measured as the button it draws: its picture
// and caret included, or whatever sits after it in a row draws over it.
#[test]
fn a_menu_button_holds_its_room_in_a_row() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/row.toml"),
        "type = \"widget_theme\"\n\n[roles.m]\nfill = \"#ff0000\"\n\n[roles.n]\nfill = \"#00ff00\"\n",
    )
    .unwrap();
    let strip = add_widget(
        &app,
        &toml::toml! { kind = "row" theme = "themes/row.toml" x = 0.0 y = 0.0 widget_layout = { gap = [0.0, 0.0] } }
            .into(),
    );
    let menu = add_child_widget(
        &app,
        strip,
        "M",
        &toml::toml! { kind = "menu" role = "m" text = "Balaur" widget_button = { trailing = "▾" } }.into(),
    );
    let row = toml::toml! { kind = "button" text = "row" };
    add_child_widget(&app, menu, "R0", &row.into());
    add_child_widget(
        &app,
        strip,
        "N",
        &toml::toml! { kind = "button" role = "n" text = "next" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let boxed = |fill: egui::Color32| {
        out.shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::epaint::Shape::Rect(r) if r.fill == fill => Some(r.rect),
                _ => None,
            })
            .expect("the button drew")
    };
    let (mine, next) = (boxed(egui::Color32::RED), boxed(egui::Color32::GREEN));
    assert!(
        mine.max.x <= next.min.x + 0.5,
        "the next button draws over the menu: {mine:?} {next:?}"
    );
}

/// The rows of a hidden menu, with their width, if any were drawn.
pub(crate) fn rows_drawn(out: &egui::FullOutput, width: f32) -> Option<egui::Rect> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::epaint::Shape::Rect(r) if (r.rect.width() - width).abs() < 1.0 => Some(r.rect),
        _ => None,
    })
}

/// A button naming a hidden menu as its `context`, and that menu's row.
pub(crate) fn context_scene(app: &balaur_core::App, kind: &str) -> Entity {
    let target = toml::toml! {
        kind = kind text = "Target" x = 10.0 y = 10.0 width = 120.0 height = 40.0 context = "cm"
    };
    let target = add_widget(app, &target.into());
    let menu = toml::toml! { kind = "menu" x = 300.0 y = 300.0 };
    let menu = add_child_widget(app, app.engine.root(), "cm", &menu.into());
    let row = toml::toml! { kind = "button" text = "Cut" width = 173.0 height = 22.0 };
    add_child_widget(app, menu, "R0", &row.into());
    target
}

// A secondary click on a widget opens the menu its `context` names, at the
// pointer, without the menu's own button ever drawing; and the widget's own
// click is not reported, since only the primary button clicks.
#[test]
fn a_secondary_click_opens_the_named_menu_at_the_pointer() {
    let (_dir, mut app) = app();
    let target = context_scene(&app, "button");
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let at = root_rect(&ctx, target).center();
    let before = pass(&app, &ctx, vec![]);
    assert!(
        rows_drawn(&before, 173.0).is_none(),
        "the menu opened unasked"
    );
    assert!(
        !texts(&before).iter().any(|(text, _)| text == "Hidden"),
        "a hidden menu drew its button"
    );
    pass(
        &app,
        &ctx,
        press_with(at, egui::PointerButton::Secondary, true),
    );
    pass(
        &app,
        &ctx,
        press_with(at, egui::PointerButton::Secondary, false),
    );
    // Shown the pass after the click, as a menu's rows are.
    let after = pass(&app, &ctx, vec![]);
    let rows = rows_drawn(&after, 173.0).expect("the secondary click opened no menu");
    assert!(
        rows.min.distance(at) < 24.0,
        "the menu opened at {:?}, not at the pointer {at:?}",
        rows.min
    );
    consume_input(&mut app);
    assert!(
        !clicked(&app, target),
        "a secondary click counted as a click"
    );
}

// Two widgets naming one menu: the click on the second draws the rows in both
// popups in one pass, the first's closing and the second's opening, and a
// debug egui panics on an id it sees on two layers in one pass.
#[test]
fn a_menu_two_contexts_name_opens_at_each_in_turn() {
    opens_at_each_in_turn(|app, menu| {
        let row = toml::toml! {
            kind = "button" text = "Copy" width = 150.0 height = 22.0 tooltip = "Copy it"
        };
        add_child_widget(app, menu, "R1", &row.into());
    });
}

#[test]
fn a_menu_row_holding_a_field_opens_at_each_context_in_turn() {
    opens_at_each_in_turn(|app, menu| {
        let row = toml::toml! { kind = "text_field" text = "Find" width = 150.0 };
        add_child_widget(app, menu, "R1", &row.into());
    });
}

#[test]
fn a_menu_row_split_by_a_seam_opens_at_each_context_in_turn() {
    opens_at_each_in_turn(|app, menu| {
        let row = toml::toml! {
            kind = "row" width = 150.0 height = 22.0 widget_layout = { splitter_width = 8.0 }
        };
        let row = add_child_widget(app, menu, "R1", &row.into());
        // The first states its width, so the seam between them is a grab.
        let fixed = toml::toml! { kind = "panel" text = "" width = 60.0 };
        add_child_widget(app, row, "fixed", &fixed.into());
        let rest = toml::toml! { kind = "panel" text = "" grow = 1.0 };
        add_child_widget(app, row, "rest", &rest.into());
    });
}

#[test]
fn a_menu_row_holding_code_opens_at_each_context_in_turn() {
    opens_at_each_in_turn(|app, menu| {
        let row = toml::toml! { kind = "code" text = "let a = 1;" width = 150.0 height = 22.0 };
        add_child_widget(app, menu, "R1", &row.into());
    });
}

/// Two labels naming one menu, opened at each in turn: its rows are a `Cut`
/// button 173 wide and what `rows` puts under the menu after it.
fn opens_at_each_in_turn(rows: impl FnOnce(&balaur_core::App, Entity)) {
    let (_dir, app) = app();
    let targets: Vec<Entity> = [10.0, 200.0]
        .into_iter()
        .map(|y| {
            let target = toml::toml! {
                kind = "label" text = "Target" x = 10.0 y = y width = 120.0 height = 40.0 context = "cm"
            };
            add_widget(&app, &target.into())
        })
        .collect();
    let menu = toml::toml! { kind = "menu" x = 300.0 y = 300.0 };
    let menu = add_child_widget(&app, app.engine.root(), "cm", &menu.into());
    let cut = toml::toml! { kind = "button" text = "Cut" width = 173.0 height = 22.0 };
    add_child_widget(&app, menu, "R0", &cut.into());
    rows(&app, menu);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    for target in targets {
        let at = root_rect(&ctx, target).center();
        let secondary = |down| press_with(at, egui::PointerButton::Secondary, down);
        pass(&app, &ctx, secondary(true));
        pass(&app, &ctx, secondary(false));
        let rows = rows_drawn(&pass(&app, &ctx, vec![]), 173.0).expect("no menu opened");
        assert!(
            rows.min.distance(at) < 24.0,
            "the menu opened at {:?}, not at the pointer {at:?}",
            rows.min
        );
    }
}

// A finger held on a widget past egui's click length is the same press as a
// secondary click, on a label as much as on a button: the sensor under the
// kind is what egui holds the touch on.
#[test]
fn a_long_touch_opens_the_named_menu() {
    let (_dir, app) = app();
    let target = context_scene(&app, "label");
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let at = root_rect(&ctx, target).center();
    pass_at(&app, &ctx, touch(at, true), Some(1.0));
    pass_at(&app, &ctx, vec![], Some(1.1));
    // Past `max_click_duration`, still down and unmoved: the long touch.
    pass_at(&app, &ctx, vec![], Some(2.5));
    let held = pass_at(&app, &ctx, vec![], Some(2.55));
    assert!(
        rows_drawn(&held, 173.0).is_some(),
        "a long touch opened no menu"
    );
    pass_at(&app, &ctx, touch(at, false), Some(2.6));
    let released = pass_at(&app, &ctx, vec![], Some(2.65));
    assert!(
        rows_drawn(&released, 173.0).is_some(),
        "lifting the finger closed the menu it opened"
    );
}

// A press on a child that names a menu is the child's; the parent naming
// another opens nothing for the same press.
#[test]
fn the_innermost_context_takes_the_press() {
    let (_dir, app) = app();
    let panel = toml::toml! {
        kind = "column" x = 10.0 y = 10.0 width = 200.0 height = 100.0 context = "outer"
        padding = [0.0, 0.0, 0.0, 0.0]
    };
    let panel = add_widget(&app, &panel.into());
    let child =
        toml::toml! { kind = "label" text = "Inner" width = 100.0 height = 30.0 context = "inner" };
    let child = add_child_widget(&app, panel, "child", &child.into());
    let root = app.engine.root();
    for (name, width) in [("outer", 150.0), ("inner", 173.0)] {
        let menu = toml::toml! { kind = "menu" x = 300.0 y = 300.0 };
        let menu = add_child_widget(&app, root, name, &menu.into());
        let row = toml::toml! { kind = "button" text = "Row" width = width height = 22.0 };
        add_child_widget(&app, menu, "R0", &row.into());
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let at = balaur_ui::widget_rect(child)
        .expect("the child drew")
        .center();
    pass(
        &app,
        &ctx,
        press_with(at, egui::PointerButton::Secondary, true),
    );
    pass(
        &app,
        &ctx,
        press_with(at, egui::PointerButton::Secondary, false),
    );
    let after = pass(&app, &ctx, vec![]);
    assert!(
        rows_drawn(&after, 173.0).is_some(),
        "the child's menu did not open"
    );
    assert!(
        rows_drawn(&after, 150.0).is_none(),
        "the parent's menu opened for the child's press"
    );
}

// Where a menu opens is the node's to say: under its button, above it, at
// the pointer, or over the middle of the screen.
#[test]
fn a_menu_opens_where_its_placement_says() {
    for placement in ["below", "above", "pointer", "center"] {
        let (_dir, app) = app();
        let params = toml::toml! {
            kind = "menu" text = "Menu" x = 10.0 y = 200.0
            widget_menu = { placement = placement }
        };
        let host = add_widget(&app, &params.into());
        let row = toml::toml! { kind = "button" text = "row" width = 173.0 height = 22.0 };
        add_child_widget(&app, host, "R0", &row.into());
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let button = root_rect(&ctx, host);
        let at = button.center();
        pass(&app, &ctx, press(at, true));
        pass(&app, &ctx, press(at, false));
        // One pass opens the popup at its guessed size, the next places it.
        pass(&app, &ctx, vec![]);
        let open = pass(&app, &ctx, vec![]);
        let rows = rows_drawn(&open, 173.0)
            .unwrap_or_else(|| panic!("placement = {placement}: the menu did not open"));
        match placement {
            "above" => assert!(
                rows.max.y <= button.min.y + 1.0,
                "above: the rows are at {rows:?}, under the button at {button:?}"
            ),
            "pointer" => assert!(
                rows.min.distance(at) < 24.0,
                "pointer: the rows are at {:?}, not at the pointer {at:?}",
                rows.min
            ),
            "center" => assert!(
                rows.center().distance(egui::pos2(320.0, 240.0)) < 8.0,
                "center: the rows are at {:?}, not over the middle of the screen",
                rows.center()
            ),
            _ => assert!(
                rows.min.y >= button.max.y - 1.0,
                "below: the rows are at {rows:?}, over the button at {button:?}"
            ),
        }
    }
}

// A menu hung from the end of a button at the screen's edge opens as wide
// as its rows, its end under the button's, all of it on the screen: it is
// not measured in the sliver between the button and the edge.
#[test]
fn a_menu_below_the_end_of_a_button_at_the_edge_stays_on_the_screen() {
    let (_dir, app) = app();
    let params = toml::toml! {
        kind = "menu" text = "Me" x = 590.0 y = 10.0 width = 40.0
        widget_menu = { placement = "below_end" }
    };
    let host = add_widget(&app, &params.into());
    let row = toml::toml! { kind = "button" text = "row" width = 173.0 height = 22.0 };
    add_child_widget(&app, host, "R0", &row.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let button = root_rect(&ctx, host);
    let at = button.center();
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    pass(&app, &ctx, vec![]);
    let open = pass(&app, &ctx, vec![]);
    let rows = rows_drawn(&open, 173.0).expect("the menu did not open");
    assert!(
        rows.max.x <= 640.5,
        "the rows at {rows:?} run past the screen's edge at 640"
    );
    assert!(
        (rows.max.x - button.max.x).abs() < 16.0,
        "the rows end at {}, not under the button's end at {}",
        rows.max.x,
        button.max.x
    );
}

// A menu inside a menu is a submenu: it opens to the side when the pointer
// rests on its row, and the menu it hangs off stays up.
#[test]
fn a_menu_row_that_is_a_menu_opens_to_the_side() {
    let (_dir, app) = app();
    let params = toml::toml! { kind = "menu" text = "File" x = 10.0 y = 10.0 };
    let host = add_widget(&app, &params.into());
    let branch = toml::toml! { kind = "menu" text = "More" width = 173.0 height = 22.0 };
    let branch = add_child_widget(&app, host, "More", &branch.into());
    let leaf = toml::toml! { kind = "button" text = "Deep" width = 151.0 height = 22.0 };
    add_child_widget(&app, branch, "Deep", &leaf.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let at = root_rect(&ctx, host).center();
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    pass(&app, &ctx, vec![]);
    let open = pass(&app, &ctx, vec![]);
    let rows = rows_drawn(&open, 173.0).expect("the menu did not open");
    // The pointer resting on the row is what opens a submenu in egui: one
    // pass sees the hover, the next opens at its guessed size, the third
    // draws it.
    let over = rows.center();
    for _ in 0..2 {
        pass(&app, &ctx, vec![egui::Event::PointerMoved(over)]);
    }
    let deep = pass(&app, &ctx, vec![egui::Event::PointerMoved(over)]);
    let inner = rows_drawn(&deep, 151.0).expect("the row's own menu did not open");
    assert!(
        inner.min.x >= rows.max.x - 2.0,
        "the submenu is at {inner:?}, not beside the rows at {rows:?}"
    );
    assert!(
        rows_drawn(&deep, 173.0).is_some(),
        "opening the submenu closed the menu it hangs off"
    );
}

// A row's chord clicks it while its menu is shut, and the row draws the
// chord against its far edge without being told to twice.
#[test]
fn a_menu_row_fires_from_its_shortcut_with_the_menu_shut() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "menu" text = "File" x = 10.0 y = 10.0 };
    let host = add_widget(&app, &params.into());
    let row = toml::toml! {
        kind = "button" text = "Save" width = 173.0 height = 22.0
        widget_shortcut = { shortcut = "cmd+KeyS" }
    };
    let row = add_child_widget(&app, host, "Save", &row.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    assert!(
        rows_drawn(&pass(&app, &ctx, vec![]), 173.0).is_none(),
        "the menu is open before anything asked for it"
    );
    pass(
        &app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::S,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
    );
    consume_input(&mut app);
    assert!(clicked(&app, row), "the chord did not click the shut row");
    // What the row shows beside its caption: the chord, written the way this
    // platform writes it.
    let showing = toml::toml! { kind = "menu" text = "File" x = 10.0 y = 200.0 widget_menu = { showing = true } };
    let open = add_widget(&app, &showing.into());
    let labelled = toml::toml! {
        kind = "button" text = "Save" width = 173.0 height = 22.0
        widget_shortcut = { shortcut = "cmd+KeyS" }
    };
    add_child_widget(&app, open, "Save", &labelled.into());
    settle(&app, &ctx);
    let drawn = texts(&pass(&app, &ctx, vec![]));
    let chord = ctx.format_shortcut(&egui::KeyboardShortcut::new(
        egui::Modifiers::COMMAND,
        egui::Key::S,
    ));
    assert!(
        drawn.iter().any(|(text, _)| *text == chord),
        "the row drew no shortcut: {drawn:?}"
    );
}

// The outliner's case: a `tree` naming a hidden menu opens it over the row a
// right click lands on, as a button naming one does.
#[test]
fn a_secondary_click_on_a_tree_row_opens_the_named_menu() {
    let (_dir, app) = app();
    let tree = toml::toml! {
        kind = "tree" x = 10.0 y = 10.0 width = 200.0 height = 200.0 context = "cm"
        widget_rows = { row_height = 18.0, items = [{ label = "One" }, { label = "Two", depth = 1 }, { label = "Three" }] }
    };
    add_widget(&app, &tree.into());
    let menu = toml::toml! { kind = "menu" x = 300.0 y = 300.0 };
    let menu = add_child_widget(&app, app.engine.root(), "cm", &menu.into());
    let row = toml::toml! { kind = "button" text = "Delete" width = 173.0 height = 22.0 };
    add_child_widget(&app, menu, "R0", &row.into());
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
    let after = pass(&app, &ctx, vec![]);
    assert!(
        rows_drawn(&after, 173.0).is_some(),
        "a right click on a tree row opened no menu"
    );
}

/// A menu at (10, 10) with a sheet head and two rows, its `widget` keys
/// extended by `extra`, on a screen `width` design pixels wide (egui's is
/// the harness's 640 by 480 whatever it says).
fn sheet_menu(
    extra: toml::Table,
    width: f32,
) -> (
    tempfile::TempDir,
    balaur_core::App,
    [Entity; 4],
    egui::Context,
) {
    let (dir, app) = app();
    balaur_core::facts::update_device(&app.engine, |facts| {
        facts.screen_size = [width, 480.0];
        facts.ui_scale = 1.0;
    });
    let mut params = toml::toml! { kind = "menu" text = "Language" x = 10.0 y = 10.0 };
    params.extend(extra);
    let host = add_widget(&app, &params.into());
    let head = toml::toml! { kind = "label" text = "Pick one" width = 120.0 height = 20.0 widget_child = { title_bar = true } };
    let head = add_child_widget(&app, host, "Head", &head.into());
    let mut rows = Vec::new();
    for name in ["R0", "R1"] {
        let row = toml::toml! { kind = "button" text = "row" width = 173.0 height = 22.0 };
        rows.push(add_child_widget(&app, host, name, &row.into()));
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    (dir, app, [host, head, rows[0], rows[1]], ctx)
}

/// The rect a node was drawn in this pass, if it was.
fn drawn_at(out: &egui::FullOutput, entity: Entity) -> Option<egui::Rect> {
    let rect = balaur_ui::widget_rect(entity)?;
    out.shapes
        .iter()
        .any(|s| match &s.shape {
            egui::epaint::Shape::Rect(r) => r.rect.intersects(rect),
            _ => false,
        })
        .then_some(rect)
}

// A phone's list: the rows along the foot of the screen over a dim, its
// head first, rather than hung off the button (daisyUI's `modal-bottom`).
#[test]
fn a_sheet_opens_its_rows_along_the_foot_of_the_screen_under_its_head() {
    let (_dir, app, [host, head, first, last], ctx) =
        sheet_menu(toml::toml! { widget_menu = { placement = "sheet" } }, 640.0);
    let at = root_rect(&ctx, host).center();
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    let out = pass(&app, &ctx, vec![]);
    let top = balaur_ui::widget_rect(head).expect("the head drew");
    let bottom = balaur_ui::widget_rect(last).expect("the rows drew");
    assert!(
        bottom.max.y > 440.0,
        "the rows sit on the screen's foot: {bottom:?}"
    );
    assert!(
        top.max.y <= balaur_ui::widget_rect(first).unwrap().min.y,
        "the head comes first"
    );
    let dims = |r: &egui::epaint::RectShape| {
        let [red, green, blue, alpha] = r.fill.to_array();
        // Faded in over egui's fade time, so dim at all on its first frame.
        red == 0 && green == 0 && blue == 0 && alpha > 0 && r.rect.width() >= 640.0
    };
    assert!(
        out.shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::epaint::Shape::Rect(r) if dims(r))),
        "the page behind is dimmed"
    );
}

// Where the menu drops down, its sheet head is left out: the button says
// what it picks.
#[test]
fn a_dropdown_leaves_its_sheet_head_out() {
    let (_dir, app, [host, head, first, _], ctx) = sheet_menu(toml::Table::new(), 640.0);
    let at = root_rect(&ctx, host).center();
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    let out = pass(&app, &ctx, vec![]);
    assert!(drawn_at(&out, first).is_some(), "the rows drop down");
    assert!(
        balaur_ui::widget_rect(first).unwrap().max.y < 200.0,
        "under the button"
    );
    let title = balaur_ui::widget_rect(head);
    assert!(
        title.is_none_or(|rect| rect.width() <= 0.0
            || !rect.intersects(balaur_ui::widget_rect(first).unwrap().expand(40.0))),
        "the head is not drawn in the dropdown: {title:?}"
    );
}

// A click on the dim shuts the sheet, as on a dialog; so does a row.
#[test]
fn the_dim_and_a_row_shut_a_sheet() {
    for shut_by_row in [false, true] {
        let (_dir, mut app, [host, _, first, _], ctx) =
            sheet_menu(toml::toml! { widget_menu = { placement = "sheet" } }, 640.0);
        let at = root_rect(&ctx, host).center();
        pass(&app, &ctx, press(at, true));
        pass(&app, &ctx, press(at, false));
        pass(&app, &ctx, vec![]);
        let target = if shut_by_row {
            balaur_ui::widget_rect(first).unwrap().center()
        } else {
            pos2(320.0, 20.0)
        };
        pass(&app, &ctx, press(target, true));
        pass(&app, &ctx, press(target, false));
        consume_input(&mut app);
        pass(&app, &ctx, vec![]);
        let out = pass(&app, &ctx, vec![]);
        let still = out.shapes.iter().any(|s| match &s.shape {
            egui::epaint::Shape::Rect(r) => (r.rect.width() - 173.0).abs() < 1.0,
            _ => false,
        });
        assert!(
            !still,
            "a click on the {} left the sheet up",
            if shut_by_row { "row" } else { "dim" }
        );
    }
}

// A phone's own list: a `[narrow]` table makes the menu a sheet there and
// leaves it a dropdown on a wider screen, as `max-sm:modal-bottom` does.
#[test]
fn a_narrow_table_makes_the_menu_a_sheet_on_a_phone() {
    for (width, sheeted) in [(390.0, true), (1024.0, false)] {
        let (_dir, app, [host, _, _, last], ctx) = sheet_menu(
            toml::toml! {
                [narrow]
                placement = "sheet"
            },
            width,
        );
        let at = root_rect(&ctx, host).center();
        pass(&app, &ctx, press(at, true));
        pass(&app, &ctx, press(at, false));
        pass(&app, &ctx, vec![]);
        let row = balaur_ui::widget_rect(last).expect("the rows drew");
        assert_eq!(
            row.max.y > 440.0,
            sheeted,
            "{width} wide: the last row at {row:?}"
        );
    }
}

/// Whether the page behind is dimmed this pass, as a sheet or a dialog dims it.
fn dimmed(out: &egui::FullOutput) -> bool {
    rects(out).iter().any(|(rect, fill)| {
        let [red, green, blue, alpha] = fill.to_array();
        red == 0 && green == 0 && blue == 0 && alpha > 0 && rect.width() >= 640.0
    })
}

// A sheet left open while the window widens is shut when it narrows again:
// it is not up for a pass it was not drawn in.
#[test]
fn a_sheet_left_open_at_a_wider_class_comes_back_shut() {
    let (_dir, app, [host, ..], ctx) = sheet_menu(
        toml::toml! {
            [narrow]
            placement = "sheet"
        },
        390.0,
    );
    let at = root_rect(&ctx, host).center();
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    assert!(dimmed(&pass(&app, &ctx, vec![])), "the sheet opened");
    for width in [1024.0, 390.0] {
        balaur_core::facts::update_device(&app.engine, |facts| facts.screen_size = [width, 480.0]);
        for _ in 0..3 {
            pass(&app, &ctx, vec![]);
        }
    }
    assert!(
        !dimmed(&pass(&app, &ctx, vec![])),
        "back on the phone, it stays shut"
    );
}

// `close_on = "never"` leaves a sheet to Escape: the dim does not shut it.
#[test]
fn a_sheet_that_closes_on_nothing_shuts_on_escape_alone() {
    let (_dir, mut app, [host, ..], ctx) = sheet_menu(
        toml::toml! { widget_menu = { placement = "sheet", close_on = "never" } },
        640.0,
    );
    let at = root_rect(&ctx, host).center();
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    pass(&app, &ctx, vec![]);
    let dim = pos2(320.0, 20.0);
    pass(&app, &ctx, press(dim, true));
    pass(&app, &ctx, press(dim, false));
    consume_input(&mut app);
    pass(&app, &ctx, vec![]);
    assert!(dimmed(&pass(&app, &ctx, vec![])), "the dim left it up");
    pass(&app, &ctx, key(egui::Key::Escape));
    pass(&app, &ctx, vec![]);
    assert!(!dimmed(&pass(&app, &ctx, vec![])), "Escape shut it");
}

// A menu of strings opens as a sheet too, its options along the foot.
#[test]
fn a_menu_of_options_opens_as_a_sheet() {
    let (_dir, app) = app();
    balaur_core::facts::update_device(&app.engine, |facts| {
        facts.screen_size = [640.0, 480.0];
        facts.ui_scale = 1.0;
    });
    let host = add_widget(
        &app,
        &toml::toml! { kind = "menu" text = "Pick" x = 10.0 y = 10.0 widget_menu = { placement = "sheet", options = ["Alpha", "Beta"] } }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let at = root_rect(&ctx, host).center();
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    let out = pass(&app, &ctx, vec![]);
    assert!(dimmed(&out), "over the dim");
    let beta = texts(&out).into_iter().find(|(text, _)| text == "Beta");
    assert!(
        beta.as_ref().is_some_and(|(_, at)| at.y > 400.0),
        "its options along the foot: {beta:?}"
    );
}

// A menu near a viewport's foot opens above its button, inside the viewport:
// the window has room below, so egui alone would hang it there, over
// whatever sits under the viewport.
#[test]
fn a_menu_near_a_viewports_foot_opens_inside_the_viewport() {
    let (_dir, app) = app();
    let screen = add_widget(
        &app,
        &toml::toml! { kind = "viewport" x = 100.0 y = 20.0 width = 300.0 height = 260.0 }.into(),
    );
    let options: Vec<String> = (0..8).map(|n| format!("Opt{n}")).collect();
    let mut menu = toml::toml! { kind = "menu" text = "File" x = 10.0 y = 200.0 };
    menu.insert(
        "widget_menu".into(),
        toml::toml! { showing = true options = [] }.into(),
    );
    menu["widget_menu"]["options"] =
        toml::Value::Array(options.iter().map(|o| o.as_str().into()).collect());
    add_child_widget(&app, screen, "File", &menu.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, vec![]);
    let out = pass(&app, &ctx, vec![]);
    let rows: Vec<egui::Pos2> = texts(&out)
        .into_iter()
        .filter(|(text, _)| text.starts_with("Opt"))
        .map(|(_, at)| at)
        .collect();
    assert_eq!(rows.len(), options.len(), "control: every row drew");
    let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 20.0), egui::vec2(300.0, 260.0));
    assert!(
        rows.iter().all(|at| viewport.contains(*at)),
        "a row opened outside its viewport: {rows:?}"
    );
}

// A tooltip near a viewport's right edge with no side wide enough slides
// inside the viewport: the window has room to its right, so egui alone would not.
#[test]
fn a_tooltip_near_a_viewports_edge_stays_inside_it() {
    use crate::support::pass_at;

    let (_dir, app) = app();
    let screen = add_widget(
        &app,
        &toml::toml! { kind = "viewport" x = 100.0 y = 20.0 width = 300.0 height = 260.0 }.into(),
    );
    let button = add_child_widget(
        &app,
        screen,
        "Go",
        &toml::toml! {
            kind = "button" text = "go" x = 230.0 y = 100.0 width = 60.0 height = 30.0
            tooltip = "a tooltip long enough to run past the edge"
        }
        .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let at = balaur_ui::widget_rect(button).expect("drawn").center();
    let mut out = pass_at(&app, &ctx, vec![egui::Event::PointerMoved(at)], Some(0.0));
    for step in 1..=20 {
        out = pass_at(&app, &ctx, vec![], Some(f64::from(step) * 0.1));
    }
    let tip = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(text) if text.galley.text().contains("long enough") => {
                Some(text.visual_bounding_rect())
            }
            _ => None,
        })
        .expect("control: the tooltip drew");
    let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 20.0), egui::vec2(300.0, 260.0));
    assert!(
        viewport.expand(2.0).contains_rect(tip),
        "the tooltip ran out of its viewport: {tip:?}"
    );
}
