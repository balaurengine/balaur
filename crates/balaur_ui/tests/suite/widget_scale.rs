//! The UI scale is egui's zoom factor and nothing else.
//!
//! Before it was, balaur multiplied its own sizes by a scale of its own while
//! egui's built-in controls took only the display's, so the two drifted apart
//! as the scale rose. These two tests are that drift, and its absence.

use egui::vec2;

use crate::support::{add_child_widget, add_widget, app, set_scale, settle};

/// The rect a widget drew at, in design pixels. A child has no `Area` of its
/// own, so this is where the layer says it put it.
fn drew_at(entity: balaur_core::hecs::Entity) -> egui::Rect {
    balaur_ui::widget_rect(entity).expect("the widget drew")
}

/// The height of a `button` and of a `drag_value`, drawn side by side with
/// neither given a size, at one scale.
fn heights(scale: f32) -> (f32, f32) {
    let (_dir, app) = app();
    let row = add_widget(&app, &toml::toml! { kind = "row" x = 0.0 y = 0.0 }.into());
    let button = add_child_widget(
        &app,
        row,
        "b",
        &toml::toml! { kind = "button" text = "ok" }.into(),
    );
    let number = add_child_widget(
        &app,
        row,
        "n",
        &toml::toml! { kind = "drag_value" value = 1.0 }.into(),
    );
    let ctx = egui::Context::default();
    set_scale(&app, &ctx, scale);
    settle(&app, &ctx);
    (drew_at(button).height(), drew_at(number).height())
}

/// egui's own control and balaur's answer to the same scale. The `drag_value`
/// is egui's `DragValue`; the `button` is drawn by this crate.
#[test]
fn a_button_and_a_drag_value_are_one_height_at_any_scale() {
    for scale in [1.0, 2.0] {
        let (button, number) = heights(scale);
        assert!(
            (button - number).abs() < 1.0,
            "at scale {scale} the button is {button} high and the drag value {number}"
        );
    }
}

/// A design pixel is a point, so a stated size is the same number at every
/// scale; what the scale changes is how many screen pixels a point costs.
#[test]
fn a_stated_size_is_the_same_points_at_every_scale_and_more_pixels() {
    let measure = |scale: f32| {
        let (_dir, app) = app();
        let panel = add_widget(
            &app,
            &toml::toml! { kind = "panel" x = 0.0 y = 0.0 width = 120.0 height = 40.0 }.into(),
        );
        let ctx = egui::Context::default();
        set_scale(&app, &ctx, scale);
        settle(&app, &ctx);
        (drew_at(panel).size(), ctx.pixels_per_point())
    };
    let (single, one_point) = measure(1.0);
    let (double, two_points) = measure(2.0);
    assert!(
        (single - vec2(120.0, 40.0)).length() < 1.0,
        "the panel drew {single} where it stated 120 by 40"
    );
    assert!(
        (single - double).length() < 1.0,
        "the scale moved the design size: {single} then {double}"
    );
    assert!(
        (two_points - one_point * 2.0).abs() < f32::EPSILON,
        "a point should cost twice the pixels: {one_point} then {two_points}"
    );
}

/// `[ui] scale` seeds the scale on the first tick, which is after a script's
/// `init`. A scale the script asked for by then is what it keeps.
#[test]
fn a_scale_a_script_asked_for_survives_the_first_tick() {
    let (_dir, mut app) = app();
    {
        let config = app.engine.resource::<balaur_ui::UiConfig>();
        let mut config = config.borrow_mut();
        config.scale = 1.7;
        config.asked = true;
    }
    app.tick(1.0 / 60.0);
    let kept = app.engine.resource::<balaur_ui::UiConfig>().borrow().scale;
    assert!((kept - 1.7).abs() < f32::EPSILON, "the seed overwrote the ask: {kept}");
}

/// A widget's class table, and what it takes to make one apply.
mod classes {
    use balaur_core::facts::DeviceFacts;

    use crate::support::{add_widget, app, pass, settle};

    /// Put a screen of this many design pixels under the layer. The backend
    /// publishes these every frame; a test says them itself.
    fn screen(app: &balaur_core::App, width: f32, height: f32) {
        balaur_core::facts::update_device(&app.engine, |facts: &mut DeviceFacts| {
            facts.screen_size = [width, height];
            facts.ui_scale = 1.0;
        });
    }

    /// The commonest thing a narrow screen asks for: a row that is not worth
    /// drawing when there is no room for it.
    #[test]
    fn a_narrow_screen_takes_the_narrow_table() {
        let shown = |width: f32| {
            let (_dir, app) = app();
            add_widget(
                &app,
                &toml::toml! {
                    kind = "panel" x = 0.0 y = 0.0 width = 200.0 height = 30.0
                    [narrow]
                    visible = false
                }
                .into(),
            );
            screen(&app, width, 900.0);
            let ctx = egui::Context::default();
            // A new Area is sized invisibly on its first frame, so the shapes
            // of a widget that does draw arrive on a later one.
            settle(&app, &ctx);
            !pass(&app, &ctx, vec![]).shapes.is_empty()
        };
        assert!(shown(1200.0), "a wide screen hid a widget it should draw");
        assert!(!shown(390.0), "the narrow table did not hide the widget");
    }

    /// The tables are read broad to narrow, so the narrowest word named wins
    /// the key it shares.
    #[test]
    fn the_narrowest_class_named_wins_the_key() {
        let width_at = |screen_width: f32| {
            let (_dir, app) = app();
            let widget = add_widget(
                &app,
                &toml::toml! {
                    kind = "panel" x = 0.0 y = 0.0 width = 300.0 height = 30.0
                    [pointer]
                    width = 250.0
                    [narrow]
                    width = 100.0
                }
                .into(),
            );
            screen(&app, screen_width, 900.0);
            let ctx = egui::Context::default();
            settle(&app, &ctx);
            balaur_ui::widget_rect(widget).expect("the widget drew").width()
        };
        // A desktop is `pointer` and `wide`: only the input class is named.
        assert!((width_at(1200.0) - 250.0).abs() < 1.0, "the pointer table did not apply");
        // Narrow is read after the input class, so it takes the key back.
        assert!((width_at(390.0) - 100.0).abs() < 1.0, "narrow did not outrank pointer");
    }

    /// A class table is the one place a typo cannot be the game's own space,
    /// because nothing but the layer ever reads it.
    #[test]
    fn a_key_a_class_table_invents_is_refused() {
        let (_dir, app) = app();
        let root = app.engine.root();
        let entity = balaur::scene::spawn_node(&mut app.engine.world_mut(), "W", root);
        let params: toml::Value = toml::toml! {
            kind = "panel"
            [narrow]
            widht = 100.0
        }
        .into();
        let refused = balaur::components::add(&app.engine, entity, "widget", Some(&params));
        let why = format!("{:#}", refused.expect_err("an invented key must be refused"));
        assert!(why.contains("narrow.widht"), "the error did not name the key: {why}");
    }

    /// A widget cannot become another kind halfway down a resize: its state
    /// and its children belong to the kind it is.
    #[test]
    fn a_class_table_cannot_change_the_kind() {
        let (_dir, app) = app();
        let root = app.engine.root();
        let entity = balaur::scene::spawn_node(&mut app.engine.world_mut(), "W", root);
        let params: toml::Value = toml::toml! {
            kind = "row"
            [narrow]
            kind = "column"
        }
        .into();
        let why = format!(
            "{:#}",
            balaur::components::add(&app.engine, entity, "widget", Some(&params))
                .expect_err("a kind that changes with the screen must be refused")
        );
        assert!(why.contains("kind"), "the error did not name the kind: {why}");
    }

    /// A theme states what a finger needs beside what a cursor needs, and the
    /// screen picks. This is the seam the touch floor is built on.
    #[test]
    fn a_theme_states_a_class_beside_the_look_it_qualifies() {
        let height_with = |touch: bool| {
            let (dir, app) = app();
            std::fs::create_dir_all(dir.path().join("themes")).unwrap();
            std::fs::write(
                dir.path().join("themes/t.toml"),
                "type = \"widget_theme\"\n[button]\nheight = 24.0\n[button.touch]\nheight = 44.0\n",
            )
            .unwrap();
            let widget = add_widget(
                &app,
                &toml::toml! {
                    kind = "button" text = "ok" x = 0.0 y = 0.0 theme = "themes/t.toml"
                }
                .into(),
            );
            balaur_core::facts::update_device(&app.engine, |facts: &mut DeviceFacts| {
                facts.screen_size = [1200.0, 900.0];
                facts.ui_scale = 1.0;
            });
            // The input class is a tag on the platform facts, which a test
            // states the way a phone would report it.
            let mut platform = balaur_core::facts::platform(&app.engine);
            platform.touchscreen = touch;
            app.engine
                .resource::<balaur_core::facts::Facts>()
                .borrow_mut()
                .0 = Some(platform);
            let ctx = egui::Context::default();
            settle(&app, &ctx);
            balaur_ui::widget_rect(widget)
                .expect("the button drew")
                .height()
        };
        let cursor = height_with(false);
        let finger = height_with(true);
        assert!((cursor - 24.0).abs() < 1.0, "the theme's height was {cursor}");
        assert!(
            (finger - 44.0).abs() < 1.0,
            "the touch table did not apply: {finger}"
        );
    }

    /// A finger never hovers, so a tooltip a cursor rests for is one a phone
    /// would otherwise never see. It opens on the hold and stays until the
    /// finger lifts.
    #[test]
    fn a_held_finger_opens_the_tooltip_a_cursor_rests_for() {
        use crate::support::{pass_at, touch};

        let (_dir, app) = app();
        add_widget(
            &app,
            &toml::toml! {
                kind = "button" text = "ok" x = 0.0 y = 0.0
                width = 80.0 height = 40.0 tooltip = "why"
            }
            .into(),
        );
        balaur_core::facts::update_device(&app.engine, |facts: &mut DeviceFacts| {
            facts.screen_size = [390.0, 844.0];
            facts.ui_scale = 1.0;
        });
        let mut platform = balaur_core::facts::platform(&app.engine);
        platform.touchscreen = true;
        app.engine
            .resource::<balaur_core::facts::Facts>()
            .borrow_mut()
            .0 = Some(platform);
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let at = egui::pos2(40.0, 20.0);
        let says_why = |out: &egui::FullOutput| {
            out.shapes.iter().any(|s| match &s.shape {
                egui::epaint::Shape::Text(text) => text.galley.text().contains("why"),
                _ => false,
            })
        };
        let down = pass_at(&app, &ctx, touch(at, true), Some(0.0));
        assert!(!says_why(&down), "the tooltip opened on the touch, not the hold");
        // Past egui's click length, which the project's long press sets.
        pass_at(&app, &ctx, vec![], Some(1.0));
        let held = pass_at(&app, &ctx, vec![], Some(1.1));
        assert!(says_why(&held), "holding the finger did not open the tooltip");
        let lifted = pass_at(&app, &ctx, touch(at, false), Some(1.2));
        assert!(!says_why(&lifted), "the tooltip outlived the finger");
    }

    /// A number where the words are not fine enough: a widget states the
    /// surface it needs and is not drawn on one that cannot give it.
    #[test]
    fn a_widget_states_the_surface_it_needs_and_folds_away_without_it() {
        let drawn_at = |width: f32, height: f32, params: toml::Value| {
            let (_dir, app) = app();
            add_widget(&app, &params);
            screen(&app, width, height);
            let ctx = egui::Context::default();
            settle(&app, &ctx);
            !pass(&app, &ctx, vec![]).shapes.is_empty()
        };
        let minimap = toml::toml! {
            kind = "panel" x = 0.0 y = 0.0 width = 100.0 height = 100.0 hide_narrower = 600.0
        };
        assert!(drawn_at(1200.0, 900.0, minimap.clone().into()), "a wide screen lost the widget");
        assert!(!drawn_at(390.0, 900.0, minimap.into()), "a narrow screen still drew it");
        let thumb = toml::toml! {
            kind = "panel" x = 0.0 y = 0.0 width = 100.0 height = 100.0 hide_wider = 600.0
        };
        assert!(drawn_at(390.0, 900.0, thumb.clone().into()), "a phone lost its own control");
        assert!(!drawn_at(1200.0, 900.0, thumb.into()), "a desktop drew a phone's control");
        let stack = toml::toml! {
            kind = "panel" x = 0.0 y = 0.0 width = 100.0 height = 100.0 hide_shorter = 480.0
        };
        assert!(drawn_at(844.0, 844.0, stack.clone().into()), "an upright screen lost it");
        assert!(!drawn_at(844.0, 390.0, stack.into()), "a screen on its side still drew it");
    }

    /// A line is read against the room a widget is laid out in, not only
    /// the screen: a child of a stated box asks how wide that box is.
    #[test]
    fn a_line_reads_the_container_that_states_a_size() {
        use crate::support::add_child_widget;
        let child_drawn = |panel_w: f32| {
            let (_dir, app) = app();
            let panel = add_widget(
                &app,
                &toml::toml! { kind = "panel" x = 0.0 y = 0.0 width = panel_w height = 80.0 }.into(),
            );
            // A filled box of a width nothing else on screen has: a label is a
            // glyph mesh, which a shape search cannot read the text of.
            add_child_widget(
                &app,
                panel,
                "c",
                &toml::toml! {
                    kind = "panel" width = 77.0 height = 30.0 fill = "#ff0000" hide_narrower = 300.0
                }
                .into(),
            );
            screen(&app, 1200.0, 900.0);
            let ctx = egui::Context::default();
            // One more pass than usual: the room is where the box drew last.
            settle(&app, &ctx);
            let out = pass(&app, &ctx, vec![]);
            out.shapes.iter().any(|s| match &s.shape {
                egui::epaint::Shape::Rect(r) => (r.rect.width() - 77.0).abs() < 1.0,
                _ => false,
            })
        };
        assert!(child_drawn(400.0), "a box wide enough hid its child");
        assert!(!child_drawn(200.0), "a box too narrow still drew its child, on a wide screen");
    }

    /// A notch covers the top of the screen whatever the layout wants, so a
    /// root that asks is moved clear of it.
    #[test]
    fn a_root_that_asks_is_kept_clear_of_the_notch() {
        let top_of = |ask: bool| {
            let (_dir, app) = app();
            let widget = add_widget(
                &app,
                &toml::toml! {
                    kind = "panel" anchor = "top_left" x = 0.0 y = 0.0
                    width = 100.0 height = 40.0 safe_area = ask
                }
                .into(),
            );
            balaur_core::facts::update_device(&app.engine, |facts: &mut DeviceFacts| {
                facts.screen_size = [390.0, 844.0];
                facts.ui_scale = 1.0;
                facts.safe_area = [0.0, 47.0, 0.0, 34.0];
            });
            let ctx = egui::Context::default();
            settle(&app, &ctx);
            balaur_ui::widget_rect(widget).expect("it drew").min.y
        };
        assert!(top_of(false) < 10.0, "the control drew under the notch as asked");
        assert!(
            top_of(true) >= 47.0,
            "the root was not moved clear of the notch: {}",
            top_of(true)
        );
    }

    /// Saving a scene must not quietly drop what it was authored with.
    #[test]
    fn a_class_table_survives_being_read_back() {
        let (_dir, app) = app();
        let widget = add_widget(
            &app,
            &toml::toml! {
                kind = "panel" width = 300.0
                [narrow]
                width = 100.0
            }
            .into(),
        );
        let read = balaur::components::get(&app.engine, widget, "widget")
            .expect("the component reads back");
        let narrow = read
            .get("narrow")
            .and_then(toml::Value::as_table)
            .expect("the narrow table came back");
        assert_eq!(narrow.get("width").and_then(toml::Value::as_float), Some(100.0));
    }
}
