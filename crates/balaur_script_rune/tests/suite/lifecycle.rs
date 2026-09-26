//! A script's life at its edges: a throw it repeats every frame, and the
//! `on_free` a batch of freed nodes owes.

use super::backend::{app_in, project, spawn};

#[test]
fn a_script_throwing_every_frame_is_rendered_once_then_counted() {
    let dir = project(&[(
        "thrower.rn",
        "pub fn init(this) { this.frames = 0.0; }\n\
         pub fn update(this, dt) { this.frames += 1.0; let empty = []; let _gone = empty[3]; }\n",
    )]);
    let mut app = app_in(dir.path());
    balaur_core::logbuf::capture_for_test();
    let node = spawn(&app, "Thrower");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "thrower.rn")
        .unwrap();

    for _ in 0..12 {
        app.tick(0.1);
    }

    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    assert_eq!(rune.number_field(node, "frames"), Some(12.0));
    let reported: Vec<String> = balaur_core::logbuf::recent(500)
        .into_iter()
        .filter(|e| e.level == "error" && e.message.contains("[thrower.rn] update"))
        .map(|e| e.message)
        .collect();
    assert_eq!(reported.len(), 2, "{reported:#?}");
    assert!(reported[0].contains("thrower.rn:2"), "{}", reported[0]);
    assert!(reported[1].contains("thrown 10 times"), "{}", reported[1]);
}

#[test]
fn freeing_scripted_siblings_runs_each_on_free_in_order_and_keeps_the_rest() {
    let dir = project(&[(
        "mortal.rn",
        "pub fn init(this) { this.ticks = 0.0; }\n\
         pub fn update(this, dt) { this.ticks += 1.0; }\n\
         pub fn on_free(this) { log::info(`on_free ${this.node.name()}`); }\n",
    )]);
    let mut app = app_in(dir.path());
    balaur_core::logbuf::capture_for_test();
    let host = app.engine.script_host().unwrap();
    let nodes: Vec<hecs::Entity> = ["DoomedA", "Kept", "DoomedB", "DoomedC"]
        .iter()
        .map(|name| {
            let node = spawn(&app, name);
            host.attach(balaur_core::node_id_of(node), "mortal.rn")
                .unwrap();
            node
        })
        .collect();

    balaur_core::scene::free_nodes(&app.engine, &[nodes[3], nodes[0], nodes[2]]);
    app.tick(0.1);

    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    assert_eq!(rune.instance_count(), 1);
    assert_eq!(rune.number_field(nodes[1], "ticks"), Some(1.0));
    let freed: Vec<String> = balaur_core::logbuf::recent(500)
        .into_iter()
        .filter_map(|e| {
            let at = e.message.find("on_free Doomed")?;
            Some(e.message[at..].to_string())
        })
        .collect();
    assert_eq!(
        freed,
        ["on_free DoomedC", "on_free DoomedA", "on_free DoomedB"]
    );
}
