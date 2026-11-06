//! The drawn widgets as a screen reader hears them: an AccessKit tree built
//! while a reader listens, handed over where it changed, and a reader's
//! actions applied to the widgets.

use crate::support::*;
use balaur_core::hecs::Entity;
use egui::accesskit::{Action, ActionData, ActionRequest, NodeId, Role, Toggled, TreeId};

/// A column of the kinds a reader names differently.
fn form(app: &balaur_core::App) -> Vec<Entity> {
    let column = add_widget(app, &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into());
    [
        toml::toml! { kind = "label" text = "Score" },
        toml::toml! { kind = "button" text = "Play" tooltip = "Start a round" },
        toml::toml! { kind = "checkbox" text = "Sound" widget_check = { checked = true } },
        toml::toml! {
            kind = "slider" text = "Volume"
            widget_range = { value = 0.25, min = 0.0, max = 1.0, step = 0.05 }
        },
        toml::toml! { kind = "text_field" text = "" widget_field = { placeholder = "Name" } },
    ]
    .into_iter()
    .enumerate()
    .map(|(n, params)| add_child_widget(app, column, &format!("W{n}"), &params.into()))
    .collect()
}

fn id(entity: Entity) -> NodeId {
    NodeId(entity.to_bits().get())
}

#[test]
fn a_reader_hears_each_widget_s_role_name_value_and_state() {
    let (_dir, app) = app();
    let widgets = form(&app);
    balaur_ui::enable_access(&app.engine);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let update = balaur_ui::take_access_update(&app.engine).expect("a first update");
    assert!(update.tree.is_some(), "the first update describes the tree");
    let node = |entity: Entity| {
        update
            .nodes
            .iter()
            .find(|(at, _)| *at == id(entity))
            .map(|(_, node)| node)
            .expect("every drawn widget is a node")
    };
    assert_eq!(node(widgets[0]).role(), Role::Label);
    assert_eq!(node(widgets[0]).label(), Some("Score"));
    assert_eq!(node(widgets[1]).role(), Role::Button);
    assert_eq!(node(widgets[1]).description(), Some("Start a round"));
    assert!(node(widgets[1]).supports_action(Action::Click));
    assert_eq!(node(widgets[2]).toggled(), Some(Toggled::True));
    assert_eq!(node(widgets[3]).role(), Role::Slider);
    assert_eq!(node(widgets[3]).numeric_value(), Some(0.25));
    assert_eq!(node(widgets[3]).max_numeric_value(), Some(1.0));
    assert_eq!(node(widgets[4]).role(), Role::TextInput);
    assert_eq!(node(widgets[4]).placeholder(), Some("Name"));
    assert!(
        node(widgets[3]).bounds().is_some(),
        "a node says where it was drawn"
    );
}

#[test]
fn only_what_changed_is_handed_over_again() {
    let (_dir, app) = app();
    let widgets = form(&app);
    balaur_ui::enable_access(&app.engine);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    balaur_ui::take_access_update(&app.engine).expect("a first update");
    pass(&app, &ctx, vec![]);
    assert!(
        balaur_ui::take_access_update(&app.engine).is_none(),
        "a pass that changed nothing hands nothing over"
    );
    let off = toml::toml! { checked = false };
    balaur::components::patch(&app.engine, widgets[2], "widget_check", &off.into()).unwrap();
    pass(&app, &ctx, vec![]);
    let update = balaur_ui::take_access_update(&app.engine).expect("the change");
    let ids: Vec<NodeId> = update.nodes.iter().map(|(at, _)| *at).collect();
    assert_eq!(ids, [id(widgets[2])], "only the unticked box");
    assert_eq!(update.nodes[0].1.toggled(), Some(Toggled::False));
}

#[test]
fn a_first_update_waits_for_a_pass_and_holds_the_whole_tree() {
    let (_dir, app) = app();
    let widgets = form(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    balaur_ui::enable_access(&app.engine);
    assert!(
        balaur_ui::take_access_update(&app.engine).is_none(),
        "a reader takes a first update whole, so one with no nodes must wait for the pass"
    );
    pass(&app, &ctx, vec![]);
    let update = balaur_ui::take_access_update(&app.engine).expect("the pass's tree");
    let root = update.tree.expect("the tree's description").root;
    let held: std::collections::BTreeMap<NodeId, _> = update.nodes.iter().cloned().collect();
    let mut reached = vec![root];
    let mut at = 0;
    while let Some(id) = reached.get(at).copied() {
        let node = held.get(&id).expect("every node named is in the update");
        reached.extend(node.children());
        at += 1;
    }
    assert_eq!(reached.len(), held.len(), "every node hangs from the root");
    for widget in widgets {
        assert!(reached.contains(&id(widget)));
    }
}

#[test]
fn a_focus_move_that_changes_no_node_is_handed_over() {
    let (_dir, app) = app();
    let widgets = form(&app);
    balaur_ui::enable_access(&app.engine);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    balaur_ui::take_access_update(&app.engine).expect("a first update");
    let ask = ActionRequest {
        action: Action::Focus,
        target_tree: TreeId::ROOT,
        target_node: id(widgets[1]),
        data: None,
    };
    balaur_ui::act(&app.engine, &ask);
    pass(&app, &ctx, vec![]);
    let update = balaur_ui::take_access_update(&app.engine).expect("the focus moved");
    assert_eq!(update.focus, id(widgets[1]));
    assert!(
        update.tree.is_none(),
        "only a first update describes the tree"
    );
    pass(&app, &ctx, vec![]);
    assert!(
        balaur_ui::take_access_update(&app.engine).is_none(),
        "said once"
    );
}

#[test]
fn a_reader_leaving_stops_the_tree_and_another_gets_it_whole() {
    let (_dir, app) = app();
    let widgets = form(&app);
    balaur_ui::enable_access(&app.engine);
    assert!(balaur_ui::access_enabled(&app.engine));
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    balaur_ui::take_access_update(&app.engine).expect("a first update");

    balaur_ui::disable_access(&app.engine);
    assert!(!balaur_ui::access_enabled(&app.engine));
    let off = toml::toml! { checked = false };
    balaur::components::patch(&app.engine, widgets[2], "widget_check", &off.into()).unwrap();
    pass(&app, &ctx, vec![]);
    assert!(balaur_ui::take_access_update(&app.engine).is_none());

    balaur_ui::enable_access(&app.engine);
    pass(&app, &ctx, vec![]);
    let update = balaur_ui::take_access_update(&app.engine).expect("a first update again");
    assert!(update.tree.is_some(), "a new reader knows nothing");
    assert!(
        update.nodes.iter().any(|(at, _)| *at == id(widgets[4])),
        "and is told about what did not change too"
    );
}

#[test]
fn the_last_widget_going_empties_the_tree() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into(),
    );
    balaur_ui::enable_access(&app.engine);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let update = balaur_ui::take_access_update(&app.engine).expect("a first update");
    let root = update.tree.expect("the tree's description").root;
    assert!(update.nodes.iter().any(|(at, _)| *at == id(column)));

    balaur_core::scene::free_node(&app.engine, column);
    pass(&app, &ctx, vec![]);
    let update = balaur_ui::take_access_update(&app.engine).expect("the widget went");
    let (_, node) = update
        .nodes
        .iter()
        .find(|(at, _)| *at == root)
        .expect("the root changed");
    assert!(node.children().is_empty());
}

#[test]
fn nothing_is_built_until_a_reader_asks() {
    let (_dir, app) = app();
    form(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    assert!(balaur_ui::take_access_update(&app.engine).is_none());
}

#[test]
fn a_reader_presses_a_button_and_sets_a_slider() {
    let (_dir, mut app) = app();
    let widgets = form(&app);
    balaur_ui::enable_access(&app.engine);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let ask = |action: Action, target: Entity, data: Option<ActionData>| ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: id(target),
        data,
    };
    balaur_ui::act(&app.engine, &ask(Action::Click, widgets[1], None));
    pass(&app, &ctx, vec![]);
    consume_input(&mut app);
    assert!(clicked(&app, widgets[1]), "a reader's click is a click");
    balaur_ui::act(
        &app.engine,
        &ask(
            Action::SetValue,
            widgets[3],
            Some(ActionData::NumericValue(0.75)),
        ),
    );
    consume_input(&mut app);
    assert_eq!(
        beside_property(&app, widgets[3], "widget_range", "value").as_float(),
        Some(0.75)
    );
    balaur_ui::act(&app.engine, &ask(Action::Focus, widgets[4], None));
    pass(&app, &ctx, vec![]);
    assert_eq!(focused(&app), Some(widgets[4]));
}
