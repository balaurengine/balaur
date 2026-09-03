//! Rollback with a script in the loop.
//!
//! The Rust-side tests in `balaur_core` prove the world comes back. This one
//! proves a script's own fields do: they are captured through the host's
//! `save_state` and put back by `load_state`, which is the half of a snapshot
//! core cannot see into.

use balaur_core::components::StableId;
use balaur_core::rollback::Session;
use balaur_core::{App, AppConfig};
use balaur_script::Value;

const PLAYER: u32 = 1;

/// A script that folds the player's input into a field of its own, so the
/// only place the answer lives is script state.
const COUNTER: &str = "\
pub fn init(this) { this.total = 0; }\n\
pub fn update(this, dt) { this.total = this.total + rollback::input(1); }\n";

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), "[project]\nname = \"t\"\n").unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    dir
}

/// An app whose one node runs `COUNTER`.
fn app_with_counter(dir: &std::path::Path) -> (App, hecs::Entity) {
    let app = App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_rune::factory()),
    })
    .unwrap();
    let root = app.engine.root();
    let node = {
        let mut world = app.engine.world_mut();
        let entity = balaur_core::scene::spawn_node(&mut world, "Counter", root);
        world
            .insert_one(entity, StableId(String::from("n_counter")))
            .unwrap();
        entity
    };
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur_core::node_id_of(node), "counter.rn")
        .unwrap();
    (app, node)
}

fn total(app: &App, node: hecs::Entity) -> f64 {
    let host = app.engine.script_host().unwrap();
    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .expect("the app is running Rune");
    rune.number_field(node, "total")
        .expect("the script keeps a total")
}

/// Tick 3 unlike its neighbours, so repeating tick 2 is certainly wrong.
const INPUTS: [i64; 6] = [10, 20, 33, 40, 50, 60];

fn input_at(tick: u64) -> Value {
    Value::Int(INPUTS[tick as usize - 1])
}

#[test]
fn a_late_input_rolls_back_a_scripts_own_fields() {
    let dir = project(&[("counter.rn", COUNTER)]);
    let (mut straight, straight_node) = app_with_counter(dir.path());
    let mut session = Session::new(&[PLAYER], 16);
    for tick in 1..=6 {
        session.submit(PLAYER, tick, input_at(tick));
        session.advance(&mut straight);
    }
    let expected = total(&straight, straight_node);
    assert!(expected > 0.0, "the script ran at all");

    let (mut app, node) = app_with_counter(dir.path());
    let mut session = Session::new(&[PLAYER], 16);
    for tick in [1, 2] {
        session.submit(PLAYER, tick, input_at(tick));
        session.advance(&mut app);
    }
    // Tick 3's input has not arrived, so this tick runs on the prediction.
    session.advance(&mut app);
    for tick in [4, 5] {
        session.submit(PLAYER, tick, input_at(tick));
        session.advance(&mut app);
    }
    assert_ne!(
        total(&app, node).to_bits(),
        expected.to_bits(),
        "the misprediction has to reach the script, or the test proves nothing"
    );

    session.submit(PLAYER, 3, input_at(3));
    session.submit(PLAYER, 6, input_at(6));
    session.advance(&mut app);

    assert_eq!(
        total(&app, node).to_bits(),
        expected.to_bits(),
        "the script's field came back and re-ran on the real input"
    );
    assert_eq!(session.stale_inputs(), 0);
}

/// A script can tell a re-run from a first run, which is how it avoids doing
/// anything twice that must happen once.
#[test]
fn a_script_can_see_that_a_tick_is_being_resimulated() {
    let dir = project(&[(
        "counter.rn",
        "pub fn init(this) { this.total = 0; this.reruns = 0; }\n\
         pub fn update(this, dt) {\n\
             this.total = this.total + rollback::input(1);\n\
             if rollback::is_resimulating() { this.reruns = this.reruns + 1; }\n\
         }\n",
    )]);
    let (mut app, node) = app_with_counter(dir.path());
    let mut session = Session::new(&[PLAYER], 16);
    for tick in [1, 2] {
        session.submit(PLAYER, tick, input_at(tick));
        session.advance(&mut app);
    }
    session.advance(&mut app);
    session.submit(PLAYER, 3, input_at(3));
    session.advance(&mut app);

    let host = app.engine.script_host().unwrap();
    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    // Tick 3 re-ran, and `reruns` survived the restore that preceded it
    // because the snapshot put the whole instance back, not one field.
    assert_eq!(rune.number_field(node, "reruns"), Some(1.0));
}
