//! What a theme role decides, and the three kinds that answer a reader
//! rather than a scene: a `switch`, a submitted `field`, and a label cut to
//! its box.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;

// A role sizes a container that states nothing, and never floors a node that asks for less than the
// role's size.
#[test]
fn a_node_s_own_size_wins_over_the_role_that_would_size_it() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/sized.toml"),
        "type = \"widget_theme\"\n\n[roles.tall]\nheight = 40.0\n",
    )
    .unwrap();
    let sheet = add_widget(
        &app,
        &toml::toml! { kind = "column" theme = "themes/sized.toml" x = 0.0 y = 0.0 width = 200.0 height = 300.0 }
            .into(),
    );
    let kid = |params: toml::Value| {
        let node = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Kid", sheet);
        balaur::components::add(&app.engine, node, "widget", Some(&params)).unwrap();
        node
    };
    let from_role = kid(toml::toml! { kind = "row" role = "tall" }.into());
    let stated = kid(toml::toml! { kind = "row" role = "tall" height = 20.0 }.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let high = balaur_ui::widget_rect(from_role).expect("it drew").height();
    let low = balaur_ui::widget_rect(stated).expect("it drew").height();
    assert!(
        (high - 40.0).abs() < 1.0,
        "the role did not size a container that states none: {high}"
    );
    assert!(
        (low - 20.0).abs() < 1.0,
        "the role overrode a node that stated its own height: {low}"
    );
}

// A role puts a label's caption where it wants it, so the inspector's fold carets can sit right of
// their column.
#[test]
fn a_role_aligns_a_label_s_caption() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/ends.toml"),
        "type = \"widget_theme\"\n\n[roles.tail]\ntext_align = \"end\"\n",
    )
    .unwrap();
    let at = |role: &str, y: f64| {
        add_widget(
            &app,
            &toml::toml! { kind = "label" text = "x" width = 120.0 role = (role) theme = "themes/ends.toml" x = 0.0 y = (y) }
                .into(),
        )
    };
    let _tail = at("tail", 0.0);
    let _plain = at("", 100.0);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    // A caption is shaped glyphs, not an egui galley, so its box is the mesh's.
    let x = |upper: bool| {
        out.shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Mesh(_) => shape.shape.visual_bounding_rect().into(),
                _ => None,
            })
            .find(|rect: &egui::Rect| (rect.min.y < 50.0) == upper)
            .map(|rect| rect.min.x)
            .expect("the caption drew")
    };
    assert!(
        x(true) > x(false) + 60.0,
        "the role did not push the caption to the end: {} vs {}",
        x(true),
        x(false)
    );
}

// A node's `icon_color` tints its glyph and wins over the role's. The
// inspector marks each component section in that component's own colour,
// which no role can hold, and it drew itself for want of this.
#[test]
fn a_node_s_icon_color_tints_its_glyph_over_the_role_s() {
    let (dir, app) = app_with_editor_fonts();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/tint.toml"),
        "type = \"widget_theme\"\n\n[fonts]\nicon = \"fonts/Phosphor.ttf\"\n\n[colors]\nmark = \"#ff0000\"\n\n[roles.marked]\nicon_color = \"mark\"\n",
    )
    .unwrap();
    // Phosphor's `cursor`, asked for by name and drawn as its character.
    let icon = "\u{e1dc}";
    let _from_role = add_widget(
        &app,
        &toml::toml! { kind = "button" role = "marked" theme = "themes/tint.toml" x = 0.0 y = 0.0 widget_button = { icon = "cursor" } }
            .into(),
    );
    let _stated = add_widget(
        &app,
        &toml::toml! { kind = "button" role = "marked" theme = "themes/tint.toml" x = 0.0 y = 100.0 widget_button = { icon = "cursor", icon_color = "#00ff00" } }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let tint = |upper: bool| {
        out.shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text)
                    if text.galley.text() == icon && (text.pos.y < 50.0) == upper =>
                {
                    Some(text.fallback_color)
                }
                _ => None,
            })
            .expect("the glyph drew")
    };
    assert_eq!(
        tint(true),
        egui::Color32::from_rgb(255, 0, 0),
        "the role did not tint the glyph"
    );
    assert_eq!(
        tint(false),
        egui::Color32::from_rgb(0, 255, 0),
        "the node did not override the role's tint"
    );
}

// A child that grows takes the room its siblings leave, inside a root that
// fills the surface. The start screen's page got nothing.
#[test]
fn a_grow_child_of_a_fill_root_takes_what_is_left() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/rooms.toml"),
        "type = \"widget_theme\"\n\n[roles.ground]\npadding = 14.0\ngap = 10.0\n\n[roles.strip]\nwidth = 620.0\nheight = 34.0\n\n[roles.wide]\nwidth = 620.0\n",
    )
    .unwrap();
    // Two fill roots on the window, as the editor's shell and start screen
    // are: the second is what the start screen found empty.
    let _under = add_widget(
        &app,
        &toml::toml! { kind = "column" anchor = "fill" theme = "themes/rooms.toml" }.into(),
    );
    let root = add_widget(
        &app,
        &toml::toml! { kind = "column" anchor = "fill" theme = "themes/rooms.toml" role = "ground" safe_area = ["left", "top", "right", "bottom"] }
            .into(),
    );
    let kid = |name: &str, params: toml::Value| {
        let node = balaur::scene::spawn_node(&mut app.engine.world_mut(), name, root);
        balaur::components::add(&app.engine, node, "widget", Some(&params)).unwrap();
        node
    };
    let strip = kid("Strip", toml::toml! { kind = "row" role = "strip" }.into());
    let rest = kid(
        "Rest",
        toml::toml! { kind = "column" grow = 1.0 role = "wide" }.into(),
    );
    // The page inside it, growing into what is left, as the start screen's
    // body does.
    let page = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Page", rest);
    balaur::components::add(
        &app.engine,
        page,
        "widget",
        Some(&toml::toml! { kind = "column" grow = 1.0 }.into()),
    )
    .unwrap();
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    // Written to every pass, the way the start screen shows itself. A write
    // puts the node in `touched`, and the root's own solve must survive it.
    balaur::components::patch(
        &app.engine,
        root,
        "widget",
        &toml::toml! { enabled = true }.into(),
    )
    .unwrap();
    settle(&app, &ctx);
    let strip_h = balaur_ui::widget_rect(strip)
        .expect("the strip drew")
        .height();
    let rest_h = balaur_ui::widget_rect(rest)
        .expect("the rest drew")
        .height();
    assert!(
        (strip_h - 34.0).abs() < 1.0,
        "the strip did not take its role's height: {strip_h}"
    );
    assert!(
        rest_h > 100.0,
        "the grown child took nothing: {rest_h} beside a strip of {strip_h}"
    );
}

// A label that states `truncate` ends a caption too long for its box with
// an ellipsis, rather than leaving the painter to clip it mid-glyph.
#[test]
fn a_truncated_label_ends_in_an_ellipsis() {
    let (_dir, app) = app();
    let long = "a/very/long/path/that/will/not/fit/in/the/room/it/was/given.toml";
    let cut = add_widget(
        &app,
        &toml::toml! { kind = "label" text = (long) width = 120.0 x = 0.0 y = 0.0 widget_text = { truncate = true } }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let drawn = out
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Mesh(_) => Some(shape.shape.visual_bounding_rect().width()),
            _ => None,
        })
        .fold(0.0_f32, f32::max);
    assert!(
        drawn <= 121.0,
        "the caption was not cut to its box: {drawn} wide in 120"
    );
    assert!(drawn > 40.0, "the caption was cut to nothing: {drawn} wide");
    let _ = cut;
}

// A truncating label given a box a hair short of its line (a layout's sums
// leave one 99.9 for a line of 99.94) draws the whole line, not an ellipsis.
#[test]
fn a_truncated_label_a_hair_short_draws_its_whole_line() {
    let (_dir, app) = app();
    let free = add_widget(
        &app,
        &toml::toml! { kind = "label" text = "Hangman" x = 0.0 y = 0.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, vec![]);
    let natural = root_rect(&ctx, free).width();
    let snug = natural - 0.4;
    let held = add_widget(
        &app,
        &toml::toml! { kind = "label" text = "Hangman" width = snug x = 0.0 y = 100.0 widget_text = { truncate = true } }
            .into(),
    );
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let at = root_rect(&ctx, held);
    let drawn = out
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Mesh(_) => {
                let bounds = shape.shape.visual_bounding_rect();
                (bounds.center().y > at.top() && bounds.center().y < at.bottom())
                    .then_some(bounds.width())
            }
            _ => None,
        })
        .fold(0.0_f32, f32::max);
    assert!(
        drawn > natural - 2.0,
        "the line was cut: {drawn} drawn of {natural}"
    );
}

// A form's row has no script of its own, so its submit is reported the way a button reports its
// click: true for one frame.
#[test]
fn a_field_reports_its_submit_for_one_frame() {
    let (_dir, app) = app();
    let field = add_widget(
        &app,
        &toml::toml! { kind = "text_field" text = "one" width = 200.0 x = 0.0 y = 0.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let mut app = app;
    let said = |app: &balaur::App| {
        balaur_core::events::delivered_from(&app.engine, field, "submitted").len()
    };
    app.tick(1.0 / 60.0);
    assert_eq!(said(&app), 0, "it reported a submit nobody made");
    balaur_ui::submit(&app.engine, field, "two");
    app.tick(1.0 / 60.0);
    assert_eq!(said(&app), 1, "the submit was not reported");
    assert_eq!(
        property(&app, field, "text").as_str(),
        Some("two"),
        "the text did not land"
    );
    app.tick(1.0 / 60.0);
    assert_eq!(said(&app), 0, "the submit was still said a frame later");
}

// A `switch` takes its track from the role while off and from the role's `active` table while on.
#[test]
fn a_switch_flips_and_wears_its_role_s_two_states() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/flip.toml"),
        "type = \"widget_theme\"\n\n[roles.flip]\nheight = 18.0\nfill = \"#101215\"\ntext_color = \"#767e88\"\n[roles.flip.checked]\nfill = \"#d5814e\"\ntext_color = \"#f9f4ed\"\n",
    )
    .unwrap();
    let flip = add_widget(
        &app,
        &toml::toml! { kind = "switch" role = "flip" theme = "themes/flip.toml" x = 0.0 y = 0.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let track = |out: &egui::FullOutput| {
        out.shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(rect) if rect.rect.width() > rect.rect.height() => {
                    Some(rect.fill)
                }
                _ => None,
            })
            .expect("the track drew")
    };
    assert_eq!(
        track(&pass(&app, &ctx, vec![])),
        egui::Color32::from_rgb(0x10, 0x12, 0x15),
        "the off track is not the role's"
    );
    let mut app = app;
    assert!(balaur_ui::click(&app.engine, flip, false));
    app.tick(1.0 / 60.0);
    assert_eq!(
        beside_property(&app, flip, "widget_check", "checked").as_bool(),
        Some(true),
        "the click did not flip it"
    );
    settle(&app, &ctx);
    assert_eq!(
        track(&pass(&app, &ctx, vec![])),
        egui::Color32::from_rgb(0xd5, 0x81, 0x4e),
        "the on track is not the role's `active`"
    );
}

// A checkbox and a dropdown are egui's widgets: the kind's table, and a
// checkbox's `checked` table once it is ticked, reach the box egui paints.
// Before, both wore egui's grey whatever the theme said.
#[test]
fn a_checkbox_and_a_dropdown_wear_their_kind_s_table() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/boxes.toml"),
        "type = \"widget_theme\"\n\n[checkbox]\nfill = \"#101215\"\n[checkbox.checked]\nfill = \"#d5814e\"\n[dropdown]\nfill = \"#203040\"\n",
    )
    .unwrap();
    let tick = add_widget(
        &app,
        &toml::toml! { kind = "checkbox" text = "Remember" theme = "themes/boxes.toml" x = 0.0 y = 0.0 }
            .into(),
    );
    add_widget(
        &app,
        &toml::toml! {
            kind = "dropdown" text = "English" theme = "themes/boxes.toml" x = 0.0 y = 60.0
            widget_dropdown = { options = ["English", "Română"] }
        }
        .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let off = fills(&pass(&app, &ctx, vec![]));
    assert!(
        off.contains(&egui::Color32::from_rgb(0x10, 0x12, 0x15)),
        "the box wears the checkbox table's fill: {off:?}"
    );
    assert!(
        off.contains(&egui::Color32::from_rgb(0x20, 0x30, 0x40)),
        "the dropdown wears its table's fill: {off:?}"
    );
    let mut app = app;
    assert!(balaur_ui::click(&app.engine, tick, false));
    app.tick(1.0 / 60.0);
    settle(&app, &ctx);
    let on = fills(&pass(&app, &ctx, vec![]));
    assert!(
        on.contains(&egui::Color32::from_rgb(0xd5, 0x81, 0x4e)),
        "and its `checked` table once ticked: {on:?}"
    );
}

// A UI page shows each state side by side, and its pictures must not hang on
// where the pointer is: `state` draws a widget in its theme's `hover`,
// `active` or `focus` table with no pointer near it, and clicks it as ever.
#[test]
fn a_held_state_dresses_a_widget_as_the_pointer_would() {
    let (dir, mut app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/states.toml"),
        "type = \"widget_theme\"\n\n[button]\nfill = \"#101010\"\n[button.hover]\nfill = \"#202020\"\n[button.active]\nfill = \"#303030\"\n[checkbox]\nfill = \"#404040\"\n[checkbox.hover]\nfill = \"#505050\"\n",
    )
    .unwrap();
    let held = |state: &str, kind: &str, y: f64| {
        let mut asked = toml::toml! { theme = "themes/states.toml" x = 0.0 text = "Go" };
        asked.insert("kind".into(), kind.into());
        asked.insert("state".into(), state.into());
        asked.insert("y".into(), y.into());
        add_widget(&app, &asked.into())
    };
    held("none", "button", 0.0);
    let hovered = held("hover", "button", 50.0);
    held("active", "button", 100.0);
    held("hover", "checkbox", 150.0);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let painted = fills(&pass(&app, &ctx, vec![]));
    for (hex, shows) in [
        (0x10, "a button left to the pointer rests"),
        (0x20, "`hover` wears the hover table"),
        (0x30, "`active` wears the active table"),
        (0x50, "and egui's own checkbox wears its hover table"),
    ] {
        let color = egui::Color32::from_rgb(hex, hex, hex);
        assert!(painted.contains(&color), "{shows}: {painted:?}");
    }
    assert!(balaur_ui::click(&app.engine, hovered, false));
    app.tick(1.0 / 60.0);
    assert!(clicked(&app, hovered), "a held state still clicks");
}

// The site's card rises 2 px under the pointer over 120 ms, and its colours
// run there rather than jump: `transition` eases a state's colours, its
// shadow and its `offset`, which moves what is drawn and leaves the layout.
#[test]
fn a_transition_eases_into_hover_and_an_offset_moves_what_is_drawn() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/eased.toml"),
        "type = \"widget_theme\"\n\n[button]\nfill = \"#000000\"\ntransition = 0.2\n[button.hover]\nfill = \"#ffffff\"\noffset = [0, -2]\n",
    )
    .unwrap();
    let button = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "Go" theme = "themes/eased.toml" x = 20.0 y = 20.0 width = 100.0 height = 40.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rest = root_rect(&ctx, button);
    let plate = |out: &egui::FullOutput| {
        fills(out)
            .into_iter()
            .find(|fill| fill.r() == fill.g() && fill.a() == 255)
            .expect("the button paints its plate")
    };
    let over = vec![egui::Event::PointerMoved(rest.center())];
    let first = plate(&pass(&app, &ctx, over));
    assert!(
        first.r() > 0 && first.r() < 255,
        "a frame after the pointer came, the fill is on its way: {first:?}"
    );
    let mut last = pass(&app, &ctx, vec![]);
    for _ in 0..30 {
        last = pass(&app, &ctx, vec![]);
    }
    assert_eq!(plate(&last), egui::Color32::WHITE, "and arrives");
    let moved = rects(&last)
        .into_iter()
        .find(|(_, fill)| *fill == egui::Color32::WHITE)
        .map(|(rect, _)| rect)
        .expect("the hover plate");
    assert!(
        (moved.min.y - (rest.min.y - 2.0)).abs() < 0.5,
        "drawn 2 px up: {moved:?} from {rest:?}"
    );
    assert!(
        (root_rect(&ctx, button).min.y - rest.min.y).abs() < 0.5,
        "while the layout keeps it where it was"
    );
}

// Every state eases, as CSS's `transition` runs every property: a toggle
// button turning on, not only the pointer arriving.
#[test]
fn a_transition_eases_a_checked_change_too() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/eased.toml"),
        "type = \"widget_theme\"\n\n[button]\nfill = \"#000000\"\ntransition = 0.2\n[button.checked]\nfill = \"#ffffff\"\n",
    )
    .unwrap();
    let button = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "Snap" theme = "themes/eased.toml" x = 20.0 y = 20.0 width = 100.0 height = 40.0 }
            .into(),
    );
    balaur::components::add(&app.engine, button, "widget_check", None).unwrap();
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let plate = |out: &egui::FullOutput| {
        fills(out)
            .into_iter()
            .find(|fill| fill.r() == fill.g() && fill.a() == 255)
            .expect("the button paints its plate")
    };
    assert_eq!(plate(&pass(&app, &ctx, vec![])), egui::Color32::BLACK);
    let on = toml::toml! { checked = true };
    balaur::components::patch(&app.engine, button, "widget_check", &on.into()).unwrap();
    let first = plate(&pass(&app, &ctx, vec![]));
    assert!(
        first.r() > 0 && first.r() < 255,
        "a frame after it turned on, the fill is on its way: {first:?}"
    );
    let mut last = pass(&app, &ctx, vec![]);
    for _ in 0..30 {
        last = pass(&app, &ctx, vec![]);
    }
    assert_eq!(plate(&last), egui::Color32::WHITE, "and arrives");
}

// Under a fixed step an easing runs on the engine's clock, so a picture taken
// mid-change is the same every run: on the wall clock the same frame of a
// `--fixed-tick` run came out three different greys.
#[test]
fn under_a_fixed_step_an_easing_runs_on_the_engine_clock() {
    let (dir, mut app) = app();
    app.set_fixed_dt(Some(1.0 / 60.0));
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/eased.toml"),
        "type = \"widget_theme\"\n\n[button]\nfill = \"#000000\"\ntransition = 0.2\n[button.hover]\nfill = \"#ffffff\"\n",
    )
    .unwrap();
    let button = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "Go" theme = "themes/eased.toml" x = 20.0 y = 20.0 width = 100.0 height = 40.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let plate = |out: &egui::FullOutput| {
        fills(out)
            .into_iter()
            .find(|fill| fill.r() == fill.g() && fill.a() == 255)
            .expect("the button paints its plate")
    };
    let over = vec![egui::Event::PointerMoved(root_rect(&ctx, button).center())];
    assert_eq!(
        plate(&pass(&app, &ctx, over)),
        egui::Color32::BLACK,
        "the engine has not stepped, so neither has the easing"
    );
    for _ in 0..5 {
        assert_eq!(plate(&pass(&app, &ctx, vec![])), egui::Color32::BLACK);
    }
    app.tick(0.1);
    let half = plate(&pass(&app, &ctx, vec![]));
    // Half of 0.2 s, eased out: 1 - 0.5^3 of the way, 223 of 255.
    assert!(half.r().abs_diff(223) <= 1, "half way in time: {half:?}");
    app.tick(0.1);
    assert_eq!(plate(&pass(&app, &ctx, vec![])), egui::Color32::WHITE);
}

// A theme's `outline` rings any focused kind outside its edge, as CSS's
// `outline`, and leaves the edge as it is: a field kept its border and gained
// a ring on the site, and took its focus colour as its border here.
#[test]
fn an_outline_rings_a_focused_field_and_keeps_its_edge() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/ringed.toml"),
        "type = \"widget_theme\"\n\n[text_field]\nstroke = \"#808080\"\nstroke_width = 1\n[text_field.focus]\noutline = \"#ff8800\"\noutline_offset = 6\n",
    )
    .unwrap();
    let field = add_widget(
        &app,
        &toml::toml! { kind = "text_field" text = "ana" theme = "themes/ringed.toml" state = "focus" x = 20.0 y = 20.0 width = 160.0 height = 32.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let edges = strokes(&out);
    let orange = egui::Color32::from_rgb(0xff, 0x88, 0x00);
    let ring = edges
        .iter()
        .find(|(_, colour)| *colour == orange)
        .map_or_else(|| panic!("no ring among {edges:?}"), |(rect, _)| *rect);
    let boxed = root_rect(&ctx, field);
    assert!(
        (boxed.min.x - ring.min.x - 6.0).abs() < 1.0
            && (ring.max.x - boxed.max.x - 6.0).abs() < 1.0,
        "the ring sits its `outline_offset` outside the field: {ring:?} round {boxed:?}"
    );
    assert!(
        edges
            .iter()
            .any(|(_, colour)| *colour == egui::Color32::from_gray(0x80)),
        "and the field keeps its own edge: {edges:?}"
    );
}

// A role's capitals and letter spacing reach the shaper, so the label
// measures what it draws: the site's eyebrow, set apart and in capitals.
#[test]
fn a_role_s_capitals_and_letter_spacing_size_the_label() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/eyebrow.toml"),
        "type = \"widget_theme\"\n\n[roles.eyebrow]\ntext_transform = \"uppercase\"\nletter_spacing = 3\n",
    )
    .unwrap();
    let plain = add_widget(
        &app,
        &toml::toml! { kind = "label" text = "ships" theme = "themes/eyebrow.toml" x = 20.0 y = 20.0 }
            .into(),
    );
    let set_apart = add_widget(
        &app,
        &toml::toml! { kind = "label" text = "ships" role = "eyebrow" theme = "themes/eyebrow.toml" x = 20.0 y = 80.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let width = |node| balaur_ui::widget_rect(node).expect("it drew").width();
    assert!(
        width(set_apart) > width(plain) + 4.0 * 3.0,
        "five capitals three apart: {} beside {}",
        width(set_apart),
        width(plain)
    );
    assert_eq!(
        property(&app, set_apart, "text").as_str(),
        Some("ships"),
        "the text stays as written"
    );
}

// A `grow` label is cut at the box the layout left it, not at its own
// length. The inspector's Source row ran its path under the verb beside it:
// only a stated width was a column, and a grown child never states one.
#[test]
fn a_grown_label_is_cut_at_the_box_it_was_given() {
    let (_dir, app) = app();
    let row = add_widget(
        &app,
        &toml::toml! { kind = "row" x = 0.0 y = 0.0 width = 300.0 height = 30.0 }.into(),
    );
    let kid = |name: &str, params: toml::Value| {
        let node = balaur::scene::spawn_node(&mut app.engine.world_mut(), name, row);
        balaur::components::add(&app.engine, node, "widget", Some(&params)).unwrap();
        node
    };
    let long = "a/very/long/path/that/will/not/fit/in/the/room/it/was/given.toml";
    kid(
        "Path",
        toml::toml! { kind = "label" text = (long) grow = 1.0 }.into(),
    );
    let verb = kid(
        "Verb",
        toml::toml! { kind = "button" text = "make inline" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    // The glyphs are shaped at their full length and the painter cuts them,
    // so the clip is what says where the column ends. Unclipped, every mesh
    // answers with the whole area.
    let verb_left = balaur_ui::widget_rect(verb).expect("the verb drew").min.x;
    let cuts: Vec<f32> = out
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Mesh(_) => Some(shape.clip_rect.max.x),
            _ => None,
        })
        .collect();
    assert!(
        cuts.iter().any(|cut| *cut <= verb_left + 1.0),
        "the path was not cut at its box: clips {cuts:?}, the verb starts at {verb_left}"
    );
}

#[test]
fn a_role_pads_a_container_across_and_down_apart() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/padded.toml"),
        "type = \"widget_theme\"\n\n[roles.list]\npadding = 2.0\npadding_x = 10.0\npadding_y = 4.0\n",
    )
    .unwrap();
    let list = add_widget(
        &app,
        &toml::toml! { kind = "column" role = "list" theme = "themes/padded.toml" x = 0.0 y = 0.0 width = 200.0 height = 100.0 }
            .into(),
    );
    let node = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Kid", list);
    balaur::components::add(
        &app.engine,
        node,
        "widget",
        Some(&toml::toml! { kind = "row" height = 10.0 grow = 1 }.into()),
    )
    .unwrap();
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let outer = balaur_ui::widget_rect(list).expect("it drew");
    let inner = balaur_ui::widget_rect(node).expect("it drew");
    assert!(
        (inner.min.x - outer.min.x - 10.0).abs() < 0.5,
        "padding_x sets the space inside the left edge: {outer:?} {inner:?}"
    );
    assert!(
        (inner.min.y - outer.min.y - 4.0).abs() < 0.5,
        "padding_y sets the space inside the top edge: {outer:?} {inner:?}"
    );
}

/// One button 100 wide with a caption that does not fit, and what drew: its
/// box, and the widest run of glyphs.
fn narrow_button(extra: &str) -> (egui::Rect, f32) {
    let (_dir, app) = app();
    let mut params = toml::toml! {
        kind = "button" text = "Save every scene in the project" width = 100.0 x = 0.0 y = 0.0
    };
    if !extra.is_empty() {
        let mut rows = toml::Table::new();
        rows.insert(extra.into(), true.into());
        params.insert("widget_text".into(), rows.into());
    }
    let button = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let glyphs = out
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Mesh(_) => Some(shape.shape.visual_bounding_rect().width()),
            _ => None,
        })
        .fold(0.0_f32, f32::max);
    let rect = balaur_ui::widget_rect(button).expect("the button drew");
    (rect, glyphs)
}

#[test]
fn a_wrapping_button_breaks_its_caption_inside_its_width() {
    let (one_line, overflow) = narrow_button("");
    let (wrapped, glyphs) = narrow_button("wrap");
    assert!(
        overflow > 100.0,
        "the plain caption fits after all: {overflow}"
    );
    assert!(
        glyphs <= 100.0,
        "the wrapped caption ran {glyphs} wide in 100"
    );
    assert!((wrapped.width() - 100.0).abs() < 0.5, "{wrapped:?}");
    assert!(
        wrapped.height() > one_line.height() + 8.0,
        "the button did not grow for its second line: {wrapped:?} against {one_line:?}"
    );
}

#[test]
fn a_truncated_button_caption_ends_inside_its_width() {
    let (rect, glyphs) = narrow_button("truncate");
    assert!(glyphs <= 100.0, "the caption ran {glyphs} wide in 100");
    assert!(glyphs > 40.0, "the caption was cut to nothing: {glyphs}");
    assert!((rect.width() - 100.0).abs() < 0.5, "{rect:?}");
}

/// A 100 by 40 button at (20, 20) in a theme of `source`, its own rows
/// extended by `extra`, hovered, after `frames` passes.
fn hovered_plate(source: &str, extra: toml::Table, frames: usize) -> (egui::Rect, egui::Rect) {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(dir.path().join("themes/moves.toml"), source).unwrap();
    let mut asked = toml::toml! { kind = "button" text = "Go" theme = "themes/moves.toml" x = 20.0 y = 20.0 width = 100.0 height = 40.0 };
    asked.extend(extra);
    let button = add_widget(&app, &asked.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rest = root_rect(&ctx, button);
    let mut out = pass(&app, &ctx, vec![egui::Event::PointerMoved(rest.center())]);
    for _ in 1..frames {
        out = pass(&app, &ctx, vec![]);
    }
    let plate = painted(&out, egui::Color32::from_rgb(0, 0, 0))
        .into_iter()
        .next()
        .expect("the plate");
    (rest, plate)
}

// A node's own `offset` adds to its role's hover table's, as CSS's
// `translate` over a `:hover` transform: a bar slid down still lifts.
#[test]
fn a_node_s_offset_adds_to_its_hover_table_s() {
    let (rest, plate) = hovered_plate(
        "type = \"widget_theme\"\n\n[button]\nfill = \"#000000\"\n[button.hover]\noffset = [0, -2]\n",
        toml::toml! { offset = [0.0, 30.0] },
        4,
    );
    assert!(
        (plate.min.y - (rest.min.y + 28.0)).abs() < 0.5,
        "30 down and 2 up: {plate:?} from {rest:?}"
    );
}

// A widget the pointer passes through is never hovered, so it wears no
// hover table: a badge on a card stays the badge under the pointer.
#[test]
fn a_widget_the_pointer_passes_through_keeps_its_rest_look() {
    let (rest, plate) = hovered_plate(
        "type = \"widget_theme\"\n\n[button]\nfill = \"#000000\"\n[button.hover]\noffset = [0, -2]\n",
        toml::toml! { interactive = false },
        4,
    );
    assert!(
        (plate.min.y - rest.min.y).abs() < 0.5,
        "lifted though it takes no pointer: {plate:?} from {rest:?}"
    );
}

// A hover table's `scale` grows over the transition, as its `offset` runs.
#[test]
fn a_transition_eases_a_hover_scale() {
    let theme = "type = \"widget_theme\"\n\n[button]\nfill = \"#000000\"\ntransition = 0.2\n[button.hover]\nscale = [2.0, 2.0]\n";
    let (_, early) = hovered_plate(theme, toml::Table::new(), 1);
    assert!(
        early.width() > 100.5 && early.width() < 199.5,
        "on its way a frame in: {early:?}"
    );
    let (_, late) = hovered_plate(theme, toml::Table::new(), 40);
    assert!((late.width() - 200.0).abs() < 1.0, "and arrives: {late:?}");
}

// A scale is `[x, y]`: a bare number is no scale, and the plate keeps its size.
#[test]
fn a_scale_written_as_one_number_is_dropped() {
    let theme =
        "type = \"widget_theme\"\n\n[button]\nfill = \"#000000\"\n[button.hover]\nscale = 2.0\n";
    let (rest, plate) = hovered_plate(theme, toml::Table::new(), 4);
    assert!(
        (plate.width() - rest.width()).abs() < 1.5,
        "{plate:?} from {rest:?}"
    );
}

// A hover table's `scale = [x, y]` runs each axis on its own: wider over the
// transition while its height stays.
#[test]
fn a_transition_eases_a_hover_scale_axis_by_axis() {
    let theme = "type = \"widget_theme\"\n\n[button]\nfill = \"#000000\"\ntransition = 0.2\n[button.hover]\nscale = [2.0, 1.0]\n";
    let (rest, early) = hovered_plate(theme, toml::Table::new(), 1);
    assert!(
        early.width() > 100.5 && early.width() < 198.5,
        "wider on its way a frame in: {early:?}"
    );
    assert!(
        (early.height() - rest.height()).abs() < 1.5,
        "as tall as it was: {early:?} from {rest:?}"
    );
    let (_, late) = hovered_plate(theme, toml::Table::new(), 40);
    // A stretched fill's opaque edge sits half a pixel inside, doubled.
    assert!((late.width() - 200.0).abs() < 2.5, "and arrives: {late:?}");
    assert!(
        (late.center() - rest.center()).length() < 1.0,
        "about its middle: {late:?} from {rest:?}"
    );
}
