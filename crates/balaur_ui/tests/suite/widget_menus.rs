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
    let params =
        toml::toml! { kind = "menu" text = "File" options = ["Open", "Save"] x = 0.0 y = 0.0 };
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
        balaur_core::events::delivered_from(&app.engine, entity, balaur_ui::CHANGE_EVENT),
        vec![balaur_script::Value::Str("Save".into())],
        "the pick is the menu's `change`"
    );
    assert_eq!(
        property(&app, entity, "text"),
        toml::Value::String("File".into()),
        "and the caption stays the menu's"
    );
}

/// A menu's rows can be nodes: an icon, a shortcut and a tick are widgets
/// like any other, which a flat list of strings cannot carry.
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
                egui::epaint::Shape::Rect(r) => (r.rect.width() - 173.0).abs() < 1.0,
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

/// An action row closes its menu; a toggle row says `keep_open` and stays,
/// which is what a grid of panel ticks needs.
#[test]
fn a_row_closes_its_menu_unless_it_keeps_it_open() {
    for (keep, open_after) in [(false, false), (true, true)] {
        let (_dir, mut app) = app();
        let params = toml::toml! { kind = "menu" text = "Menu" x = 10.0 y = 10.0 };
        let host = add_widget(&app, &params.into());
        let row = toml::toml! {
            kind = "button" text = "row" width = 173.0 height = 22.0 keep_open = keep
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

/// `showing` holds a menu's rows up with no click, which is the only way an
/// offscreen run or a tutorial can show one.
#[test]
fn a_showing_menu_is_open_without_a_click() {
    let (_dir, app) = app();
    let params = toml::toml! { kind = "menu" text = "Menu" showing = true x = 10.0 y = 10.0 };
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

/// A menu whose rows are nodes is measured as the button it draws: its picture
/// and caret included, or whatever sits after it in a row draws over it.
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
        &toml::toml! { kind = "row" gap = 0.0 theme = "themes/row.toml" x = 0.0 y = 0.0 }.into(),
    );
    let menu = add_child_widget(
        &app,
        strip,
        "M",
        &toml::toml! { kind = "menu" role = "m" text = "Balaur" trailing = "▾" }.into(),
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
fn rows_drawn(out: &egui::FullOutput, width: f32) -> Option<egui::Rect> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::epaint::Shape::Rect(r) if (r.rect.width() - width).abs() < 1.0 => Some(r.rect),
        _ => None,
    })
}

/// A button naming a hidden menu as its `context`, and that menu's row.
fn context_scene(app: &balaur_core::App, kind: &str) -> Entity {
    let target = toml::toml! {
        kind = kind text = "Target" x = 10.0 y = 10.0 width = 120.0 height = 40.0 context = "cm"
    };
    let target = add_widget(app, &target.into());
    let menu = toml::toml! { kind = "menu" text = "Hidden" visible = false x = 300.0 y = 300.0 };
    let menu = add_child_widget(app, app.engine.root(), "cm", &menu.into());
    let row = toml::toml! { kind = "button" text = "Cut" width = 173.0 height = 22.0 };
    add_child_widget(app, menu, "R0", &row.into());
    target
}

/// A secondary click on a widget opens the menu its `context` names, at the
/// pointer, without the menu's own button ever drawing; and the widget's own
/// click is not reported, since only the primary button clicks.
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

/// A finger held on a widget past egui's click length is the same press as a
/// secondary click, on a label as much as on a button: the sensor under the
/// kind is what egui holds the touch on.
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

/// A press on a child that names a menu is the child's; the parent naming
/// another opens nothing for the same press.
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
        let menu = toml::toml! { kind = "menu" text = name visible = false x = 300.0 y = 300.0 };
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

/// Where a menu opens is the node's to say: under its button, above it, at
/// the pointer, or over the middle of the screen.
#[test]
fn a_menu_opens_where_its_placement_says() {
    for placement in ["below", "above", "pointer", "center"] {
        let (_dir, app) = app();
        let params = toml::toml! {
            kind = "menu" text = "Menu" placement = placement x = 10.0 y = 200.0
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

/// A menu inside a menu is a submenu: it opens to the side when the pointer
/// rests on its row, and the menu it hangs off stays up.
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

/// A row's chord clicks it while its menu is shut, and the row draws the
/// chord against its far edge without being told to twice.
#[test]
fn a_menu_row_fires_from_its_shortcut_with_the_menu_shut() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "menu" text = "File" x = 10.0 y = 10.0 };
    let host = add_widget(&app, &params.into());
    let row = toml::toml! {
        kind = "button" text = "Save" width = 173.0 height = 22.0 shortcut = "cmd+s"
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
    let showing = toml::toml! { kind = "menu" text = "File" showing = true x = 10.0 y = 200.0 };
    let open = add_widget(&app, &showing.into());
    let labelled = toml::toml! {
        kind = "button" text = "Save" width = 173.0 height = 22.0 shortcut = "cmd+s"
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
