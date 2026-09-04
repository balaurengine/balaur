//! The `animation` script module, driven from Rune.
//!
//! One clip, the same handful of calls, one declaration list: nothing in this
//! crate names a language, and every script below reaches the module through
//! the language-neutral seam. What a script writes on `this` is read back the
//! same way a rollback snapshot would read it, so no backend accessor is
//! needed to see what the module told it.

use balaur_anim::AnimationPlugin;
use balaur_core::hecs::Entity;
use balaur_core::{node_id_of, scene, App, AppConfig};
use balaur_script::Value;

/// A library the scripts address by name, written where a project would
/// keep it.
const LIBRARY: &str = r#"
type = "animation_clip"

[clips.hop]
length = 0.5
tracks = [
  { property = "position", keys = [
    { t = 0.0, value = [0.0, 0.0, 0.0] },
    { t = 0.5, value = [0.0, 4.0, 0.0] },
  ] },
]

[clips.wave]
length = 1.0
loop = "loop"
tracks = [
  { property = "position", keys = [
    { t = 0.0, value = [1.0, 0.0, 0.0] },
    { t = 1.0, value = [3.0, 0.0, 0.0] },
  ] },
]
"#;

/// Plays `hop`, and when it ends starts the looping `wave`.
///
/// Written to be observable from Rust with no language-specific field
/// accessor: what the finished signal reached is proven by what is playing
/// afterwards.
const HERO: &str = r#"
pub fn init(this) {
    animation::play(this.node, "hop", #{ "speed": 1.0 });
}
pub fn on_animation_finished(this, name) {
    // The clip that ended is the argument, so the handler branches on it
    // rather than asking. Anything but `hop` would leave the node idle.
    if name == "hop" {
        animation::play(this.node, "wave");
    }
}
"#;

fn project(script: (&str, &str)) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[project]\nname = \"anim\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("animations")).unwrap();
    std::fs::write(dir.path().join("animations/hero.toml"), LIBRARY).unwrap();
    std::fs::write(dir.path().join(script.0), script.1).unwrap();
    dir
}

fn app_in(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur::rune::factory()),
    })
    .unwrap();
    balaur_plugin::load(&mut app, &mut AnimationPlugin::default()).unwrap();
    app
}

/// A node with an `animation` component naming the library, and `script`
/// attached to it.
fn hero(app: &App, script: &str) -> Entity {
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "Hero", root);
    let params: toml::Value = toml::from_str(r#"library = "animations/hero.toml""#).unwrap();
    balaur_core::components::add(&app.engine, entity, "animation", Some(&params)).unwrap();
    app.engine
        .script_host()
        .unwrap()
        .attach(node_id_of(entity), script)
        .unwrap();
    entity
}

fn tick(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
}

fn height(app: &App, entity: Entity) -> f32 {
    app.engine
        .world()
        .get::<&scene::Transform>(entity)
        .unwrap()
        .position
        .y
}

/// What the script wrote on `this` under `name`, read back through the
/// snapshot the host takes for rollback.
fn field(app: &App, entity: Entity, name: &str) -> Value {
    let id = node_id_of(entity);
    let state = app
        .engine
        .script_host()
        .unwrap()
        .save_state()
        .into_iter()
        .find_map(|(node, state)| (node == id).then_some(state));
    let Some(Value::Map(fields)) = state else {
        panic!("the script instance has no state to read")
    };
    fields
        .into_iter()
        .find_map(|(key, value)| (key == name).then_some(value))
        .unwrap_or_else(|| panic!("the script never wrote `{name}`"))
}

/// Calls `method` on the script, the way a method track or a signal would.
fn call(app: &App, entity: Entity, method: &str) {
    app.engine
        .script_host()
        .unwrap()
        .call_on(node_id_of(entity), method, &[]);
}

#[test]
fn a_script_plays_a_clip_and_hears_it_finish() {
    let dir = project(("hero.rn", HERO));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "hero.rn");

    tick(&mut app, 15);
    assert!(
        balaur_anim::is_playing(&app.engine, entity),
        "the script's `animation::play` did not start anything"
    );
    assert_eq!(
        balaur_anim::current(&app.engine, entity).as_deref(),
        Some("hop")
    );
    let quarter = height(&app, entity);
    assert!(
        quarter > 1.0 && quarter < 3.0,
        "a quarter of the way up the clip the node is at {quarter}"
    );

    // Past the end of `hop`: the finished signal reaches the script, which
    // answers by playing `wave`.
    tick(&mut app, 30);
    assert_eq!(
        balaur_anim::current(&app.engine, entity).as_deref(),
        Some("wave"),
        "`on_animation_finished` did not reach the script with the name of the \
         clip that ended"
    );
    assert!(
        balaur_anim::is_playing(&app.engine, entity),
        "the clip the handler started is not running"
    );
}

#[test]
fn a_script_reads_the_playhead_back_through_the_module() {
    let script = r#"
pub fn init(this) {
    animation::play(this.node, "hop");
    animation::seek(this.node, 0.25);
}
pub fn update(this, dt) {
    this.clip = animation::current(this.node);
    this.playing = animation::is_playing(this.node);
    this.elapsed = animation::time(this.node);
    let ended = animation::just_finished(this.node);
    if !(ended is Tuple) {
        this.ended = ended;
    }
}
"#;
    let dir = project(("watch.rn", script));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "watch.rn");

    tick(&mut app, 6);
    assert_eq!(field(&app, entity, "clip"), Value::Str("hop".into()));
    assert_eq!(field(&app, entity, "playing"), Value::Bool(true));
    let Value::Num(elapsed) = field(&app, entity, "elapsed") else {
        panic!("`time` did not answer with a number")
    };
    assert!(
        elapsed > 0.25,
        "`seek` should have moved the playhead forward, not reset it: {elapsed}"
    );

    tick(&mut app, 60);
    assert_eq!(
        field(&app, entity, "ended"),
        Value::Str("hop".into()),
        "`just_finished` never named the clip that ended"
    );
    assert_eq!(field(&app, entity, "playing"), Value::Bool(false));
    assert_eq!(
        balaur_anim::current(&app.engine, entity),
        None,
        "what the script read back and what Rust reads back must agree"
    );
}

#[test]
fn a_script_defines_a_clip_of_its_own_and_plays_it() {
    let script = r#"
pub fn init(this) {
    animation::define(this.node, "hurt", #{
        "length": 1.0,
        "tracks": [ #{
            "property": "position",
            "keys": [
                #{ "t": 0.0, "value": [0.0, 0.0, 0.0] },
                #{ "t": 1.0, "value": [0.0, 8.0, 0.0] },
            ],
        } ],
    });
    animation::play(this.node, "hurt");
}
"#;
    let dir = project(("define.rn", script));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "define.rn");

    tick(&mut app, 30);
    assert_eq!(
        balaur_anim::current(&app.engine, entity).as_deref(),
        Some("hurt"),
        "a definition table written in a script did not reach the asset layer"
    );

    tick(&mut app, 31);
    assert!(
        (height(&app, entity) - 8.0).abs() < 1e-2,
        "the clip the script defined did not drive the node: {}",
        height(&app, entity)
    );
}

#[test]
fn a_script_queues_a_clip_behind_the_one_playing() {
    let script = r#"
pub fn init(this) {
    animation::play(this.node, "hop");
    animation::queue(this.node, "wave");
}
"#;
    let dir = project(("queue.rn", script));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "queue.rn");

    tick(&mut app, 10);
    assert_eq!(
        balaur_anim::current(&app.engine, entity).as_deref(),
        Some("hop")
    );
    tick(&mut app, 40);
    assert_eq!(
        balaur_anim::current(&app.engine, entity).as_deref(),
        Some("wave")
    );
}

#[test]
fn a_script_pauses_and_resumes_a_clip() {
    let script = r#"
pub fn init(this) {
    this.held = false;
    this.woken = false;
    animation::play(this.node, "wave");
}
pub fn update(this, dt) {
    if animation::time(this.node) > 0.1 && !this.held {
        this.held = true;
        animation::pause(this.node);
    } else if this.held && !this.woken {
        this.woken = true;
        animation::resume(this.node);
    }
}
"#;
    let dir = project(("hold.rn", script));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "hold.rn");

    tick(&mut app, 20);

    assert!(
        balaur_anim::is_playing(&app.engine, entity),
        "`resume` did not undo `pause`"
    );
    assert_eq!(
        balaur_anim::current(&app.engine, entity).as_deref(),
        Some("wave")
    );
}

#[test]
fn a_script_stops_a_clip_and_nothing_is_current_afterwards() {
    let script = r#"
pub fn init(this) {
    animation::play(this.node, "wave");
}
pub fn update(this, dt) {
    if animation::time(this.node) > 0.1 {
        animation::stop(this.node);
    }
}
"#;
    let dir = project(("halt.rn", script));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "halt.rn");

    tick(&mut app, 20);

    assert!(!balaur_anim::is_playing(&app.engine, entity));
    assert_eq!(balaur_anim::current(&app.engine, entity), None);
}

/// A tween written the way a game would write one: a data table, no builder
/// object, and a handle that comes back as a plain number.
///
/// The callback at the end starts a second tween, which is how the script
/// proves the call arrived without exposing a variable to Rust — and it is
/// also the reentrancy check, since the handler runs inside the frame the
/// animation system is stepping. `halt` is what Rust calls to stop the tween
/// by its handle from outside.
const TWEEN: &str = r#"
pub fn init(this) {
    this.handle = animation::tween(this.node, #{
        "steps": [
            #{ "property": "position", "to": [0.0, 6.0, 0.0], "duration": 0.5, "ease": "out_back" },
            #{ "call": "on_landed" },
        ],
    });
}
pub fn update(this, dt) {
    this.running = animation::is_tween_running(this.handle);
}
pub fn on_landed(this) {
    animation::tween_to(this.node, "position", [0.0, 9.0, 0.0], 0.2, "linear");
}
pub fn halt(this) {
    animation::stop(this.handle);
}
"#;

#[test]
fn a_script_tweens_a_node_and_hears_the_call_at_the_end() {
    let dir = project(("mover.rn", TWEEN));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "mover.rn");

    tick(&mut app, 15);
    let quarter = height(&app, entity);
    assert!(
        quarter > 0.5,
        "the script's tween did not move the node: {quarter}"
    );

    tick(&mut app, 45);
    assert!(
        (height(&app, entity) - 9.0).abs() < 0.05,
        "`on_landed` did not reach the script and start the second tween: {}",
        height(&app, entity)
    );
    let state = app.engine.resource::<balaur_anim::AnimationState>();
    assert!(
        state.borrow().tweens.is_empty(),
        "a finished tween was left behind"
    );
}

#[test]
fn a_script_reads_a_tween_handle_back_and_stops_by_it() {
    let dir = project(("mover.rn", TWEEN));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "mover.rn");

    tick(&mut app, 10);
    assert_eq!(
        field(&app, entity, "running"),
        Value::Bool(true),
        "`is_tween_running` should answer for a tween that is under way"
    );
    let handle = match field(&app, entity, "handle") {
        Value::Int(id) => id as u64,
        Value::Num(id) => id as u64,
        other => panic!("a tween handle should be a plain number, not {other:?}"),
    };

    // `animation::stop` is one verb: the same name that ends a node's clip
    // ends a tween when it is handed the tween's handle.
    call(&app, entity, "halt");
    let stopped = height(&app, entity);
    tick(&mut app, 20);
    assert_eq!(
        height(&app, entity).to_bits(),
        stopped.to_bits(),
        "the tween kept going after being stopped by handle"
    );
    assert!(!balaur_anim::tween::is_running(&app.engine, handle));
}

#[test]
fn a_script_reaches_for_the_shorter_spelling() {
    let script = r#"
pub fn init(this) {
    animation::tween_to(this.node, "position", [0.0, 4.0, 0.0], 0.5, "in_quad");
}
"#;
    let dir = project(("hop.rn", script));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "hop.rn");

    tick(&mut app, 15);
    let quarter = height(&app, entity);
    assert!(
        quarter < 1.5,
        "in_quad is a quarter of the way at half the time, not {quarter}"
    );
    tick(&mut app, 20);
    assert!(
        (height(&app, entity) - 4.0).abs() < 0.05,
        "the sugar did not land: {}",
        height(&app, entity)
    );
}
