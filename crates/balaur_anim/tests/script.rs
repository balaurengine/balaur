//! The `animation` script module, in both languages.
//!
//! The same clip, the same four calls, one declaration list: nothing in this
//! crate names Luau or Rune, and the two halves of every test below differ
//! only in the syntax of the script. That is the seam working — a third
//! language costs its backend and nothing here.

use balaur_anim::AnimationPlugin;
use balaur_core::hecs::Entity;
use balaur_core::{node_id_of, scene, App, AppConfig, ScriptHostFactory};

/// A library both languages address by name, written where a project would
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
const HERO_LUAU: &str = r#"
local M = {}
function M:init()
  animation.play(self.node, "hop", { speed = 1.0 })
end
function M:on_animation_finished(name)
  -- The clip that ended is the argument, so the handler branches on it
  -- rather than asking. Anything but `hop` would leave the node idle and
  -- fail the assertion.
  if name == "hop" then
    animation.play(self.node, "wave")
  end
end
return M
"#;

const HERO_RUNE: &str = r#"
pub fn init(this) {
    animation::play(this.node, "hop", #{ "speed": 1.0 });
}
pub fn on_animation_finished(this, name) {
    if name == "hop" {
        animation::play(this.node, "wave");
    }
}
"#;

fn project(language: &str, script: (&str, &str)) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("[project]\nname = \"anim\"\nlanguage = \"{language}\"\n"),
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("animations")).unwrap();
    std::fs::write(dir.path().join("animations/hero.toml"), LIBRARY).unwrap();
    std::fs::write(dir.path().join(script.0), script.1).unwrap();
    dir
}

fn app_in(dir: &std::path::Path, backend: ScriptHostFactory) -> App {
    let mut app = App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(backend),
    })
    .unwrap();
    app.add_plugin(AnimationPlugin).unwrap();
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

/// What both languages must do, so the two tests below cannot drift apart.
fn a_script_drives_a_clip(dir: &tempfile::TempDir, backend: ScriptHostFactory, script: &str) {
    let mut app = app_in(dir.path(), backend);
    let entity = hero(&app, script);

    tick(&mut app, 15);
    assert!(
        balaur_anim::is_playing(&app.engine, entity),
        "the script's `animation.play` did not start anything"
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
fn a_luau_script_plays_a_clip_and_hears_it_finish() {
    let dir = project("luau", ("hero.luau", HERO_LUAU));
    a_script_drives_a_clip(&dir, balaur::luau::factory(), "hero.luau");
}

#[test]
fn a_rune_script_plays_the_same_clip_and_hears_the_same_signal() {
    let dir = project("rune", ("hero.rn", HERO_RUNE));
    a_script_drives_a_clip(&dir, balaur::rune::factory(), "hero.rn");
}

#[test]
fn a_luau_script_reads_the_playhead_back_through_the_module() {
    let script = r#"
local M = {}
function M:init()
  animation.play(self.node, "hop")
  animation.seek(self.node, 0.25)
end
function M:update(dt)
  _G.clip = animation.current(self.node)
  _G.playing = animation.is_playing(self.node)
  _G.elapsed = animation.time(self.node)
  local ended = animation.just_finished(self.node)
  if ended ~= nil then
    _G.ended = ended
  end
end
return M
"#;
    let dir = project("luau", ("watch.luau", script));
    let mut app = app_in(dir.path(), balaur::luau::factory());
    let entity = hero(&app, "watch.luau");

    tick(&mut app, 6);
    let lua = balaur::luau::lua_of(&app.engine);
    assert_eq!(lua.globals().get::<String>("clip").unwrap(), "hop");
    assert!(lua.globals().get::<bool>("playing").unwrap());
    let elapsed: f64 = lua.globals().get("elapsed").unwrap();
    assert!(
        elapsed > 0.25,
        "`seek` should have moved the playhead forward, not reset it: {elapsed}"
    );

    tick(&mut app, 60);
    let lua = balaur::luau::lua_of(&app.engine);
    assert_eq!(
        lua.globals().get::<String>("ended").unwrap(),
        "hop",
        "`just_finished` never named the clip that ended"
    );
    assert!(!lua.globals().get::<bool>("playing").unwrap());
    assert_eq!(
        balaur_anim::current(&app.engine, entity),
        None,
        "what the script read back and what Rust reads back must agree"
    );
}

#[test]
fn a_luau_script_defines_a_clip_of_its_own_and_plays_it() {
    let script = r#"
local M = {}
function M:init()
  animation.define(self.node, "hurt", {
    length = 1.0,
    tracks = { {
      property = "position",
      keys = {
        { t = 0.0, value = { 0.0, 0.0, 0.0 } },
        { t = 1.0, value = { 0.0, 8.0, 0.0 } },
      },
    } },
  })
  animation.play(self.node, "hurt")
end
return M
"#;
    let dir = project("luau", ("define.luau", script));
    let mut app = app_in(dir.path(), balaur::luau::factory());
    let entity = hero(&app, "define.luau");

    tick(&mut app, 30);
    assert_eq!(
        balaur_anim::current(&app.engine, entity).as_deref(),
        Some("hurt"),
        "the script's own clip is not the one playing"
    );

    tick(&mut app, 31);
    assert!(
        (height(&app, entity) - 8.0).abs() < 1e-2,
        "the clip the script defined did not drive the node: {}",
        height(&app, entity)
    );
}

#[test]
fn a_rune_script_defines_a_clip_of_its_own_and_plays_it() {
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
    let dir = project("rune", ("define.rn", script));
    let mut app = app_in(dir.path(), balaur::rune::factory());
    let entity = hero(&app, "define.rn");

    tick(&mut app, 30);
    assert_eq!(
        balaur_anim::current(&app.engine, entity).as_deref(),
        Some("hurt"),
        "a definition table written in Rune did not reach the asset layer"
    );

    tick(&mut app, 31);
    assert!(
        (height(&app, entity) - 8.0).abs() < 1e-2,
        "the clip the script defined did not drive the node: {}",
        height(&app, entity)
    );
}

#[test]
fn a_luau_script_queues_a_clip_behind_the_one_playing() {
    let script = r#"
local M = {}
function M:init()
  animation.play(self.node, "hop")
  animation.queue(self.node, "wave")
end
return M
"#;
    let dir = project("luau", ("queue.luau", script));
    let mut app = app_in(dir.path(), balaur::luau::factory());
    let entity = hero(&app, "queue.luau");

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
fn a_luau_script_pauses_and_resumes_a_clip() {
    let script = r#"
local M = {}
function M:init() animation.play(self.node, "wave") end
function M:update(dt)
  if animation.time(self.node) > 0.1 and not _G.held then
    _G.held = true
    animation.pause(self.node)
  elseif _G.held and not _G.woken then
    _G.woken = true
    animation.resume(self.node)
  end
end
return M
"#;
    let dir = project("luau", ("hold.luau", script));
    let mut app = app_in(dir.path(), balaur::luau::factory());
    let entity = hero(&app, "hold.luau");

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
fn a_luau_script_stops_a_clip_and_nothing_is_current_afterwards() {
    let script = r#"
local M = {}
function M:init() animation.play(self.node, "wave") end
function M:update(dt)
  if animation.time(self.node) > 0.1 then animation.stop(self.node) end
end
return M
"#;
    let dir = project("luau", ("halt.luau", script));
    let mut app = app_in(dir.path(), balaur::luau::factory());
    let entity = hero(&app, "halt.luau");

    tick(&mut app, 20);

    assert!(!balaur_anim::is_playing(&app.engine, entity));
    assert_eq!(balaur_anim::current(&app.engine, entity), None);
}

/// A tween written the way a game would write one: a data table, no builder
/// object, and a handle that comes back as a plain number.
///
/// The callback at the end starts a second tween, which is how both languages
/// prove the call arrived without either of them having to expose a variable
/// to Rust — and it is also the reentrancy check, since the handler runs
/// inside the frame the animation system is stepping.
const TWEEN_LUAU: &str = r#"
local M = {}
function M:init()
  _G.handle = animation.tween(self.node, {
    steps = {
      { property = "position", to = {0, 6, 0}, duration = 0.5, ease = "out_back" },
      { call = "on_landed" },
    },
  })
end
function M:update(dt)
  _G.running = animation.is_running(_G.handle)
end
function M:on_landed()
  animation.tween_to(self.node, "position", {0, 9, 0}, 0.2, "linear")
end
return M
"#;

const TWEEN_RUNE: &str = r#"
pub fn init(this) {
    this.handle = animation::tween(this.node, #{
        "steps": [
            #{ "property": "position", "to": [0.0, 6.0, 0.0], "duration": 0.5, "ease": "out_back" },
            #{ "call": "on_landed" },
        ],
    });
}
pub fn on_landed(this) {
    animation::tween_to(this.node, "position", [0.0, 9.0, 0.0], 0.2);
}
"#;

/// What both languages must get out of a tween, so the two tests below cannot
/// drift apart: the node moves, the callback at the end arrives and is free to
/// start another tween, and nothing is left behind afterwards.
fn a_script_drives_a_tween(dir: &tempfile::TempDir, backend: ScriptHostFactory, script: &str) {
    let mut app = app_in(dir.path(), backend);
    let entity = hero(&app, script);

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
fn a_luau_script_tweens_a_node_and_hears_the_call_at_the_end() {
    let dir = project("luau", ("mover.luau", TWEEN_LUAU));
    a_script_drives_a_tween(&dir, balaur::luau::factory(), "mover.luau");
}

#[test]
fn a_rune_script_tweens_the_same_node_the_same_way() {
    let dir = project("rune", ("mover.rn", TWEEN_RUNE));
    a_script_drives_a_tween(&dir, balaur::rune::factory(), "mover.rn");
}

#[test]
fn a_luau_script_reads_a_tween_handle_back_and_stops_by_it() {
    let dir = project("luau", ("mover.luau", TWEEN_LUAU));
    let mut app = app_in(dir.path(), balaur::luau::factory());
    let entity = hero(&app, "mover.luau");

    tick(&mut app, 10);
    let lua = balaur::luau::lua_of(&app.engine);
    assert!(
        lua.globals().get::<bool>("running").unwrap(),
        "`is_running` should answer for a tween that is under way"
    );
    let handle: u64 = lua.globals().get("handle").unwrap();
    drop(lua);

    // `animation.stop` is one verb: the same name that ends a node's clip
    // ends a tween when it is handed the tween's handle.
    balaur::luau::lua_of(&app.engine)
        .load("animation.stop(_G.handle)")
        .exec()
        .unwrap();
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
fn a_luau_script_reaches_for_the_shorter_spelling() {
    let script = r#"
local M = {}
function M:init()
  animation.tween_to(self.node, "position", {0, 4, 0}, 0.5, "in_quad")
end
return M
"#;
    let dir = project("luau", ("hop.luau", script));
    let mut app = app_in(dir.path(), balaur::luau::factory());
    let entity = hero(&app, "hop.luau");

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
