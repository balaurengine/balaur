//! The names the engine announces, as a checker reads them, and the events a
//! scene's rows hear and send, read from its text.

use balaur_core::project::row_names;
use balaur_core::{App, AppConfig, events, hooks};

const SAID: &[events::Said] = &[("tide_turned", "the tide's height", "The tide turned.")];

#[test]
fn a_declared_event_is_said_once_and_announced() {
    let app = App::new(AppConfig::bare(".")).unwrap();
    assert!(!events::names(&app.engine).announces("tide_turned"));
    events::declare(&app.engine, SAID);
    events::declare(&app.engine, SAID);
    let said = events::said(&app.engine);
    assert_eq!(said.iter().filter(|one| **one == SAID[0]).count(), 1);
    assert!(said.starts_with(events::SAID), "core's own come first");
    assert!(events::names(&app.engine).announces("tide_turned"));
}

#[test]
fn an_event_declared_on_nodes_is_offered_on_every_node() {
    let app = App::new(AppConfig::bare(".")).unwrap();
    events::declare_on_nodes(&app.engine, &[("sail_set", "the sail")]);
    let on_nodes = events::on_nodes(&app.engine);
    assert!(on_nodes.contains(&("sail_set", "the sail")));
    assert!(on_nodes.starts_with(balaur_core::node_api::NODE_EVENTS));
    assert!(events::names(&app.engine).calls("on_sail_set"));
}

#[test]
fn the_engines_names_hold_every_hook_and_every_core_event() {
    let app = App::new(AppConfig::bare(".")).unwrap();
    let names = events::names(&app.engine);
    for (hook, ..) in hooks::REFERENCE {
        assert!(names.calls(hook), "`{hook}` is a hook the engine calls");
    }
    for event in hooks::BINDABLE {
        assert!(names.announces(event), "`{event}` is a core hook's event");
    }
    for (event, _) in balaur_core::node_api::NODE_EVENTS {
        assert!(names.announces(event), "every node announces `{event}`");
    }
    assert!(names.announces(events::LOGGED));
    assert!(names.announces(balaur_core::timer::TIMEOUT));
    assert!(!names.calls("on_nothing_at_all"));
    assert!(!names.calls("nothing_at_all"));
}

#[test]
fn rows_are_read_wherever_a_scene_writes_them() {
    let inline = "[[nodes]]\nname = \"Door\"\nbindings = { rows = [\n\
                  \x20   { event = \"emitted:opened\", action = \"free\" },\n\
                  \x20   { event = \"pointer_click\", action = \"emit\", value = \"knocked\" },\n\
                  ] }\n";
    let found = row_names(inline);
    assert_eq!(found.heard, [("opened".to_string(), 4, 15)]);
    assert_eq!(found.sent, ["knocked"]);
    let tables = "[[nodes]]\nname = \"Door\"\n\n[[nodes.bindings.rows]]\n\
                  event = \"emitted:opened\"\naction = \"emit\"\nvalue = \"left\"\n";
    let found = row_names(tables);
    assert_eq!(found.heard, [("opened".to_string(), 5, 9)]);
    assert_eq!(found.sent, ["left"]);
    assert_eq!(
        row_names("[[nodes"),
        balaur_core::project::RowNames::default()
    );
}
