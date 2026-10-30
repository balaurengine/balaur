//! The `animation` script module, driven from Balaur Script.
//!
//! One clip, the same handful of calls, one declaration list: nothing in this
//! crate names a language, and every script below reaches the module through
//! the language-neutral seam. What a script writes to its fields is read back the
//! same way a rollback snapshot would read it, so no backend accessor is
//! needed to see what the module told it.

use balaur_animation::AnimationPlugin;
use balaur_core::hecs::Entity;
use balaur_core::{App, AppConfig, node_id_of, scene};
use balaur_script::Value;

/// A library the scripts address by name, written where a project would
/// keep it.
const LIBRARY: &str = r#"
type = "animation_library"

[clips.hop]
length = 0.5
tracks = [
  { property = "position", keys = [
    { time = 0.0, value = [0.0, 0.0, 0.0] },
    { time = 0.5, value = [0.0, 4.0, 0.0] },
  ] },
]

[clips.wave]
length = 1.0
loop_mode = "linear"
tracks = [
  { property = "position", keys = [
    { time = 0.0, value = [1.0, 0.0, 0.0] },
    { time = 1.0, value = [3.0, 0.0, 0.0] },
  ] },
]
"#;

/// Plays `hop`, and when it ends starts the looping `wave`.
///
/// Written to be observable from Rust with no language-specific field
/// accessor: what the finished signal reached is proven by what is playing
/// afterwards.
const HERO: &str = r#"
pub class Hero { node: Node }
impl Hero {
    fn init() {
        self.node.animation.play("hop", map().with("speed_scale", 1.0));
    }
    pub fn on_animation_finished(name: str) {
        // The clip that ended is the argument, so the handler branches on it
        // rather than asking. Anything but `hop` would leave the node idle.
        if name == "hop" {
            self.node.animation.play("wave");
        }
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

/// The `.bs` backend alone, as a bare app takes one.
pub(crate) fn bs_backend() -> balaur_core::ScriptHostFactory {
    Box::new(|setup| {
        let host = balaur::script_bs::BsHost::new(
            setup.engine.clone(),
            setup.project_root,
            setup.pack.clone(),
            setup.watch,
        );
        host.register(setup.engine);
        Ok(std::rc::Rc::new(host))
    })
}

fn app_in(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig {
        script_backend: Some(bs_backend()),
        ..AppConfig::bare(dir.to_path_buf())
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

/// What the script holds in its field `name`, read back through the
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
    let dir = project(("hero.bs", HERO));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "hero.bs");

    tick(&mut app, 15);
    assert!(
        balaur_animation::is_playing(&app.engine, entity),
        "the script's `animation::play` did not start anything"
    );
    assert_eq!(
        balaur_animation::current_clip(&app.engine, entity).as_deref(),
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
        balaur_animation::current_clip(&app.engine, entity).as_deref(),
        Some("wave"),
        "`on_animation_finished` did not reach the script with the name of the \
         clip that ended"
    );
    assert!(
        balaur_animation::is_playing(&app.engine, entity),
        "the clip the handler started is not running"
    );
}

#[test]
fn a_script_reads_the_playhead_back_through_the_module() {
    let script = r#"
pub class Watch { node: Node, clip: str, playing: bool, elapsed: float, ended: str }
impl Watch {
    fn init() {
        self.node.animation.play("hop");
        self.node.animation.seek(0.25);
    }
    fn update(dt: float) {
        self.clip = self.node.animation.current_clip().or("");
        self.playing = self.node.animation.is_playing();
        self.elapsed = self.node.animation.time();
        let ended = self.node.animation.just_finished();
        if ended.is_some() {
            self.ended = ended.unwrap();
        }
    }
}
"#;
    let dir = project(("watch.bs", script));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "watch.bs");

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
        balaur_animation::current_clip(&app.engine, entity),
        None,
        "what the script read back and what Rust reads back must agree"
    );
}

#[test]
fn a_script_defines_a_clip_of_its_own_and_plays_it() {
    let script = r#"
pub class Define { node: Node }
impl Define {
    fn init() {
        let keys = [
            map().with("time", 0.0).with("value", [0.0, 0.0, 0.0]),
            map().with("time", 1.0).with("value", [0.0, 8.0, 0.0]),
        ];
        let track = map().with("property", "position").with("keys", keys);
        self.node.animation.add_clip("hurt", map().with("length", 1.0).with("tracks", [track]));
        self.node.animation.play("hurt");
    }
}
"#;
    let dir = project(("define.bs", script));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "define.bs");

    tick(&mut app, 30);
    assert_eq!(
        balaur_animation::current_clip(&app.engine, entity).as_deref(),
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
pub class Queue { node: Node }
impl Queue {
    fn init() {
        self.node.animation.play("hop");
        self.node.animation.queue("wave");
    }
}
"#;
    let dir = project(("queue.bs", script));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "queue.bs");

    tick(&mut app, 10);
    assert_eq!(
        balaur_animation::current_clip(&app.engine, entity).as_deref(),
        Some("hop")
    );
    tick(&mut app, 40);
    assert_eq!(
        balaur_animation::current_clip(&app.engine, entity).as_deref(),
        Some("wave")
    );
}

#[test]
fn a_script_pauses_and_resumes_a_clip() {
    let script = r#"
pub class Hold { node: Node, held: bool, woken: bool }
impl Hold {
    fn init() {
        self.node.animation.play("wave");
    }
    fn update(dt: float) {
        if self.node.animation.time() > 0.1 && !self.held {
            self.held = true;
            self.node.animation.pause();
        } else if self.held && !self.woken {
            self.woken = true;
            self.node.animation.resume();
        }
    }
}
"#;
    let dir = project(("hold.bs", script));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "hold.bs");

    tick(&mut app, 20);

    assert!(
        balaur_animation::is_playing(&app.engine, entity),
        "`resume` did not undo `pause`"
    );
    assert_eq!(
        balaur_animation::current_clip(&app.engine, entity).as_deref(),
        Some("wave")
    );
}

#[test]
fn a_script_stops_a_clip_and_nothing_is_current_afterwards() {
    let script = r#"
pub class Halt { node: Node }
impl Halt {
    fn init() {
        self.node.animation.play("wave");
    }
    fn update(dt: float) {
        if self.node.animation.time() > 0.1 {
            animation::stop(self.node);
        }
    }
}
"#;
    let dir = project(("halt.bs", script));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "halt.bs");

    tick(&mut app, 20);

    assert!(!balaur_animation::is_playing(&app.engine, entity));
    assert_eq!(balaur_animation::current_clip(&app.engine, entity), None);
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
pub class Mover { node: Node, handle: int, running: bool }
impl Mover {
    fn init() {
        let rise = map().with("property", "position").with("to", [0.0, 6.0, 0.0])
            .with("duration", 0.5).with("ease", "out_back");
        self.handle = animation::tween(self.node, map().with("steps", [rise, map().with("call", "on_landed")]));
    }
    fn update(dt: float) {
        self.running = animation::is_tween_running(self.handle);
    }
    pub fn on_landed() {
        let step = map().with("property", "position").with("to", [0.0, 9.0, 0.0])
            .with("duration", 0.2).with("ease", "linear");
        animation::tween(self.node, map().with("steps", [step]));
    }
    pub fn halt() {
        animation::stop(self.handle);
    }
}
"#;

#[test]
fn a_script_tweens_a_node_and_hears_the_call_at_the_end() {
    let dir = project(("mover.bs", TWEEN));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "mover.bs");

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
    let state = app.engine.resource::<balaur_animation::AnimationState>();
    assert!(
        state.borrow().tweens.is_empty(),
        "a finished tween was left behind"
    );
}

#[test]
fn a_script_reads_a_tween_handle_back_and_stops_by_it() {
    let dir = project(("mover.bs", TWEEN));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "mover.bs");

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
    assert!(!balaur_animation::tween::is_running(&app.engine, handle));
}

#[test]
fn a_script_reaches_for_the_shorter_spelling() {
    let script = r#"
pub class Hop { node: Node }
impl Hop {
    fn init() {
        let step = map().with("property", "position").with("to", [0.0, 4.0, 0.0])
            .with("duration", 0.5).with("ease", "in_quad");
        animation::tween(self.node, map().with("steps", [step]));
    }
}
"#;
    let dir = project(("hop.bs", script));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "hop.bs");

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

/// The finished callback carries the handle, so a script holding several
/// tweens knows which one ran out; a value tween is read each frame.
const FINISHED: &str = r#"
pub class Mover { node: Node, handle: int, finished: int, meter: int, read: float, over: bool }
impl Mover {
    fn init() {
        let step = map().with("property", "position").with("to", [0.0, 6.0, 0.0])
            .with("duration", 0.25).with("ease", "linear");
        self.handle = animation::tween(self.node, map().with("steps", [step]));
        self.meter = animation::tween_value(0.0, 10.0, 0.5, "linear");
    }
    fn update(dt: float) {
        // The frame a value tween ends still answers its last value; after
        // it, `tween_value_of` answers nil, which a `float` reads as 0.
        if !self.over {
            self.read = animation::tween_value_of(self.meter);
        }
        self.over = !animation::is_tween_running(self.meter);
    }
    pub fn on_tween_finished(handle: int) {
        if handle == self.handle { self.finished = handle; }
    }
}
"#;

#[test]
fn a_script_hears_which_tween_finished_and_reads_a_value_tween() {
    let dir = project(("mover.bs", FINISHED));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "mover.bs");
    let number = |app: &App, name: &str| match field(app, entity, name) {
        Value::Num(n) => n,
        #[allow(clippy::cast_precision_loss, reason = "a test's small handle")]
        Value::Int(n) => n as f64,
        other => panic!("`{name}` is {other:?}"),
    };
    tick(&mut app, 10);
    assert!(number(&app, "finished") < 0.5, "not over yet");
    let read = number(&app, "read");
    assert!(read > 0.5 && read < 4.0, "the meter climbs: {read}");
    tick(&mut app, 10);
    assert!(
        number(&app, "finished") > 0.0,
        "`on_tween_finished` carried the handle"
    );
    tick(&mut app, 15);
    let read = number(&app, "read");
    assert!((read - 10.0).abs() < 0.05, "read to the end: {read}");
}

/// Spells every word through the module's constants rather than as a string.
const CONSTANTS: &str = r#"
pub class Constants { node: Node }
impl Constants {
    fn init() {
        let keys = [
            map().with("time", 0.0).with("value", [0.0, 0.0, 0.0]),
            map().with("time", 1.0).with("value", [0.0, 2.0, 0.0]).with("ease", animation::EASE_IN_OUT_SINE),
        ];
        let track = map().with("property", animation::PROPERTY_POSITION)
            .with("interpolation", animation::INTERPOLATION_CUBIC).with("keys", keys);
        self.node.animation.add_clip("swing", map().with("length", 1.0)
            .with("loop_mode", animation::LOOP_PINGPONG).with("tracks", [track]));
        self.node.animation.play("wave");
        self.node.animation.play("swing", map().with("blend_time", 0.5).with("ease", animation::EASE_IN_QUAD));
    }
}
"#;

#[test]
fn a_script_names_curves_and_modes_through_the_module_constants() {
    let dir = project(("constants.bs", CONSTANTS));
    let mut app = app_in(dir.path());
    let entity = hero(&app, "constants.bs");
    tick(&mut app, 2);

    let state = app.engine.resource::<balaur_animation::AnimationState>();
    let state = state.borrow();
    let playback = &state.players[&entity];
    assert_eq!(playback.clip_name, "swing");
    assert_eq!(
        playback.clip.as_ref().unwrap().loop_mode,
        balaur_animation::clip::LoopMode::PingPong
    );
    assert_eq!(playback.fades.len(), 1, "`wave` is fading out");
    assert_eq!(playback.fades[0].ease.name(), "in_quad");
}
