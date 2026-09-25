//! What a theme role decides, and the three kinds that answer a reader
//! rather than a scene: a `switch`, a submitted `field`, and a label cut to
//! its box.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;

/// A role sizes a container that states nothing, and a node that states a
/// size keeps it. The theme is where a size lives by default, never something
/// a node has to fight: the first cut of this floored the box with the role,
/// so a node asking for less was quietly given the role's.
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

/// A role puts a label's caption where it wants it. Only a button read
/// `align` before, so the inspector's fold carets could not be pushed to the
/// right of their column from the theme.
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

/// A node's `icon_color` tints its glyph and wins over the role's. The
/// inspector marks each component section in that component's own colour,
/// which no role can hold, and it drew itself for want of this.
#[test]
fn a_node_s_icon_color_tints_its_glyph_over_the_role_s() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/tint.toml"),
        "type = \"widget_theme\"\n\n[colors]\nmark = \"#ff0000\"\n\n[roles.marked]\nicon_color = \"mark\"\n",
    )
    .unwrap();
    let icon = "\u{e1dc}";
    let _from_role = add_widget(
        &app,
        &toml::toml! { kind = "button" icon = (icon) role = "marked" theme = "themes/tint.toml" x = 0.0 y = 0.0 }
            .into(),
    );
    let _stated = add_widget(
        &app,
        &toml::toml! { kind = "button" icon = (icon) role = "marked" icon_color = "#00ff00" theme = "themes/tint.toml" x = 0.0 y = 100.0 }
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

/// A child that grows takes the room its siblings leave, inside a root that
/// fills the surface. The start screen's page got nothing.
#[test]
fn a_grow_child_of_a_fill_root_takes_what_is_left() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/rooms.toml"),
        "type = \"widget_theme\"\n\n[roles.ground]\npadding = 14.0\ngap = 10.0\n\n[roles.strip]\nwidth = 620.0\nheight = 34.0\n\n[roles.wide]\nwidth = 620.0\n",
    )
    .unwrap();
    // Two fill roots on one named surface, as the editor's shell and start
    // screen are: the second is what the start screen found empty.
    let _under = add_widget(
        &app,
        &toml::toml! { kind = "column" anchor = "fill" theme = "themes/rooms.toml" layer = "shell" }
            .into(),
    );
    let root = add_widget(
        &app,
        &toml::toml! { kind = "column" anchor = "fill" theme = "themes/rooms.toml" layer = "shell" role = "ground" safe_area = ["left", "top", "right", "bottom"] }
            .into(),
    );
    // The default surface off and the shell's on, the way the start screen
    // points them while it is up.
    {
        let config = app.engine.resource::<balaur_ui::WidgetLayerConfig>();
        let mut config = config.borrow_mut();
        config.enabled = false;
        config.rect = Some([0.0, 0.0, 0.0, 0.0]);
        config.layers.insert(
            "shell".into(),
            balaur_ui::Surface {
                enabled: true,
                rect: None,
            },
        );
    }
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
    // The page inside it: a `draw` is sized by what a script painted, which
    // is what the start screen's body holds.
    let page = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Page", rest);
    balaur::components::add(
        &app.engine,
        page,
        "widget",
        Some(&toml::toml! { kind = "draw" draw = "paint" grow = 1.0 }.into()),
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
        &toml::toml! { visible = true }.into(),
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

/// A label that states `truncate` ends a caption too long for its box with
/// an ellipsis, rather than leaving the painter to clip it mid-glyph.
#[test]
fn a_truncated_label_ends_in_an_ellipsis() {
    let (_dir, app) = app();
    let long = "a/very/long/path/that/will/not/fit/in/the/room/it/was/given.toml";
    let cut = add_widget(
        &app,
        &toml::toml! { kind = "label" text = (long) width = 120.0 truncate = true x = 0.0 y = 0.0 }
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

/// A `field` reports its submit the way a button reports its click: true for
/// one frame, false the next. A pooled row has no script of its own, so
/// `on_submit` could not reach it and the editor drew those rows by hand.
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
    app.tick(1.0 / 60.0);
    assert!(
        !property(&app, field, "submitted").as_bool().unwrap(),
        "it reported a submit nobody made"
    );
    balaur_ui::submit(&app.engine, field, "two");
    app.tick(1.0 / 60.0);
    assert!(
        property(&app, field, "submitted").as_bool().unwrap(),
        "the submit was not reported"
    );
    assert_eq!(
        property(&app, field, "text").as_str(),
        Some("two"),
        "the text did not land"
    );
    app.tick(1.0 / 60.0);
    assert!(
        !property(&app, field, "submitted").as_bool().unwrap(),
        "the submit was still true a frame later"
    );
}

/// A `switch` holds `checked`, flips on a click, and takes its track from the
/// role while it is off and from that role's `active` table while it is on.
/// The editor drew three of these by hand for want of a kind.
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
        property(&app, flip, "checked").as_bool(),
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

/// A `grow` label is cut at the box the layout left it, not at its own
/// length. The inspector's Source row ran its path under the verb beside it:
/// only a stated width was a column, and a grown child never states one.
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
