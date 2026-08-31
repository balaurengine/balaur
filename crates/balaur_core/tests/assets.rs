//! The asset layer: shared game content, addressed by reference.
//!
//! Everything here is driven from Rust against a real `App`, because the
//! behaviour under test is what a scene file and a plugin see — which of the
//! three reference forms resolve, what gets shared, and what a bad reference
//! says when it fails.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::components::ComponentDef;
use balaur_core::{assets, components, scene, App, AppConfig, Engine, Pack};

/// What the test's asset type parses to. A plugin's own struct, opaque to
/// core: only the parser and the code downcasting it know this exists.
#[derive(Debug, PartialEq)]
struct Note {
    pitch: f64,
}

/// Records the reference every `apply` saw, so a test can assert that the
/// shared component path handed the hook a reference and not a table.
#[derive(Default)]
struct Heard(Vec<String>);

fn parse_note(body: &toml::Value) -> anyhow::Result<Rc<dyn Any>> {
    let pitch = body
        .get("pitch")
        .and_then(toml::Value::as_float)
        .ok_or_else(|| anyhow::anyhow!("a note needs a `pitch`"))?;
    Ok(Rc::new(Note { pitch }))
}

const INSTRUMENT_SCHEMA: &str = r#"
song = { type = "asset", asset = "note", default = "" }
"#;

fn app_in(project_root: &std::path::Path, pack: Option<Pack>) -> App {
    let mut app = App::new(AppConfig {
        project_root: project_root.to_path_buf(),
        pack,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.engine.insert_resource(Heard::default());
    app.register_asset_type("note", "notes", parse_note);
    // Writes nothing but the reference it was handed: the point is which
    // string reaches `apply`, not what a real plugin would build from it.
    app.register_component(
        "instrument",
        ComponentDef {
            schema: ComponentDef::parse_schema("instrument", INSTRUMENT_SCHEMA),
            apply: Box::new(|eng: &Engine, _, value: &toml::Value| {
                let reference = value
                    .get("song")
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                eng.resource::<Heard>().borrow_mut().0.push(reference);
                Ok(())
            }),
            remove: Box::new(|_, _| Ok(())),
            get: Box::new(|_, _| None),
        },
    );
    app
}

/// A project on disk with one asset library file in it.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("notes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"p\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("notes/scale.toml"),
        r#"type = "note"
pitch = 1.0

[entries.high]
pitch = 880.0

[entries.low]
pitch = 110.0
"#,
    )
    .unwrap();
    dir
}

fn heard(app: &App) -> Vec<String> {
    app.engine.resource::<Heard>().borrow().0.clone()
}

fn pitch_of(eng: &Engine, reference: &str) -> f64 {
    assets::load_typed::<Note>(eng, reference).unwrap().pitch
}

#[test]
fn a_reference_to_a_whole_file_resolves_to_that_file() {
    let dir = project();
    let app = app_in(dir.path(), None);
    assert!((pitch_of(&app.engine, "notes/scale.toml") - 1.0).abs() < f64::EPSILON);
}

#[test]
fn a_reference_to_a_named_entry_resolves_to_the_entry_inside_the_file() {
    let dir = project();
    let app = app_in(dir.path(), None);
    assert!((pitch_of(&app.engine, "notes/scale.toml#high") - 880.0).abs() < f64::EPSILON);
    assert!((pitch_of(&app.engine, "notes/scale.toml#low") - 110.0).abs() < f64::EPSILON);
}

#[test]
fn an_entry_inherits_the_asset_type_its_document_declares() {
    let dir = project();
    let app = app_in(dir.path(), None);
    // Only the document says `type = "note"`; the entry says nothing, and it
    // still finds the parser.
    let definition = assets::definition(&app.engine, "notes/scale.toml#high").unwrap();
    assert!(definition.get("type").is_none());
    assert!((pitch_of(&app.engine, "notes/scale.toml#high") - 880.0).abs() < f64::EPSILON);
}

#[test]
fn a_scene_asset_block_resolves_by_its_id_from_a_node_in_that_scene() {
    let dir = project();
    let mut app = app_in(dir.path(), None);
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(
        &app.engine,
        r##"
[[assets]]
id = "fanfare"
type = "note"
pitch = 440.0

[[nodes]]
id = "player"
name = "Player"
instrument = { song = "#fanfare" }
"##,
        root,
        false,
    )
    .unwrap();

    assert_eq!(heard(&app), vec!["#fanfare".to_string()]);
    assert!((pitch_of(&app.engine, "#fanfare") - 440.0).abs() < f64::EPSILON);
    app.tick(1.0 / 60.0);
}

#[test]
fn an_inline_table_is_a_definition_and_a_string_is_a_reference() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let root = app.engine.root();
    let inline = scene::spawn_node(&mut app.engine.world_mut(), "Inline", root);
    let external = scene::spawn_node(&mut app.engine.world_mut(), "External", root);

    let table: toml::Value = toml::from_str("song = { pitch = 220.0 }").unwrap();
    components::add(&app.engine, inline, "instrument", Some(&table)).unwrap();
    let reference: toml::Value = toml::from_str("song = \"notes/scale.toml#low\"").unwrap();
    components::add(&app.engine, external, "instrument", Some(&reference)).unwrap();

    let seen = heard(&app);
    // The shared component path rewrote the table into a reference, so the
    // hook read one shape for both spellings.
    assert!(
        seen[0].starts_with("#!"),
        "an inline definition should reach `apply` as a reference, got {:?}",
        seen[0]
    );
    assert_eq!(seen[1], "notes/scale.toml#low");
    assert!((pitch_of(&app.engine, &seen[0]) - 220.0).abs() < f64::EPSILON);
    assert!((pitch_of(&app.engine, &seen[1]) - 110.0).abs() < f64::EPSILON);
}

#[test]
fn an_empty_asset_property_is_no_asset_rather_than_a_broken_reference() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let root = app.engine.root();
    let node = scene::spawn_node(&mut app.engine.world_mut(), "Silent", root);
    components::add(&app.engine, node, "instrument", None).unwrap();
    assert_eq!(heard(&app), vec![String::new()]);
}

#[test]
fn two_nodes_naming_one_path_share_one_parsed_object() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let first = assets::load(&app.engine, "notes/scale.toml#high").unwrap();
    let second = assets::load(&app.engine, "notes/scale.toml#high").unwrap();
    assert!(
        Rc::ptr_eq(&first, &second),
        "sharing is the default: one path, one parsed object"
    );
}

#[test]
fn duplicate_hands_back_a_private_copy_rather_than_the_shared_one() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let shared = assets::load(&app.engine, "notes/scale.toml#high").unwrap();
    let private = assets::duplicate(&app.engine, "notes/scale.toml#high").unwrap();
    assert!(
        !Rc::ptr_eq(&shared, &private),
        "duplicate returned the cache"
    );
    assert_eq!(
        private.downcast_ref::<Note>(),
        Some(&Note { pitch: 880.0 }),
        "a private copy still holds the same content"
    );
    // And taking a copy must not evict what everyone else is holding.
    let again = assets::load(&app.engine, "notes/scale.toml#high").unwrap();
    assert!(Rc::ptr_eq(&shared, &again));
}

#[test]
fn one_inline_definition_written_twice_is_cached_once() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let root = app.engine.root();
    let table: toml::Value = toml::from_str("song = { pitch = 220.0 }").unwrap();
    for name in ["A", "B"] {
        let node = scene::spawn_node(&mut app.engine.world_mut(), name, root);
        components::add(&app.engine, node, "instrument", Some(&table)).unwrap();
    }
    let seen = heard(&app);
    assert_eq!(seen[0], seen[1], "one definition, two references");
    let first = assets::load(&app.engine, &seen[0]).unwrap();
    let second = assets::load(&app.engine, &seen[1]).unwrap();
    assert!(Rc::ptr_eq(&first, &second));
}

#[test]
fn reload_forgets_a_file_so_the_next_load_reads_it_again() {
    let dir = project();
    let app = app_in(dir.path(), None);
    assert!((pitch_of(&app.engine, "notes/scale.toml#high") - 880.0).abs() < f64::EPSILON);
    std::fs::write(
        dir.path().join("notes/scale.toml"),
        "type = \"note\"\npitch = 1.0\n\n[entries.high]\npitch = 990.0\n",
    )
    .unwrap();
    // Still the old value: sharing means the parse is kept until told otherwise.
    assert!((pitch_of(&app.engine, "notes/scale.toml#high") - 880.0).abs() < f64::EPSILON);
    assets::reload(&app.engine, "notes/scale.toml").unwrap();
    assert!((pitch_of(&app.engine, "notes/scale.toml#high") - 990.0).abs() < f64::EPSILON);
}

#[test]
fn a_missing_file_fails_with_a_message_naming_the_reference() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let error = assets::load(&app.engine, "notes/nothing.toml")
        .unwrap_err()
        .to_string();
    assert!(error.contains("notes/nothing.toml"), "unhelpful: {error}");
}

#[test]
fn a_missing_entry_fails_with_a_message_naming_the_reference() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let error = assets::load(&app.engine, "notes/scale.toml#middle")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("notes/scale.toml#middle"),
        "unhelpful: {error}"
    );
    assert!(error.contains("middle"), "unhelpful: {error}");
}

#[test]
fn a_malformed_reference_fails_with_a_message_naming_the_reference() {
    let dir = project();
    let app = app_in(dir.path(), None);
    for bad in ["", "#", "a.toml#b#c"] {
        let error = assets::load(&app.engine, bad).unwrap_err().to_string();
        assert!(
            error.contains("asset reference"),
            "'{bad}' failed unhelpfully: {error}"
        );
    }
}

/// A reference that resolves to nothing is a broken link, not a broken scene:
/// the node still loads and the message still names the reference, so a tool
/// can open a project whose files it cannot reach and show what is missing.
#[test]
fn an_unknown_scene_asset_id_is_named_in_a_warning_and_the_scene_still_loads() {
    balaur_core::logbuf::capture_for_test();
    let dir = project();
    let app = app_in(dir.path(), None);
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(
        &app.engine,
        "[[nodes]]\nid = \"n\"\nname = \"N\"\ninstrument = { song = \"#nope\" }\n",
        root,
        false,
    )
    .unwrap();
    assert!(scene::find_node(&app.engine.world(), root, "N").is_some());
    let complained = balaur_core::logbuf::recent(200)
        .into_iter()
        .any(|entry| entry.message.contains("#nope"));
    assert!(complained, "nothing said which reference was missing");
}

#[test]
fn an_asset_property_given_neither_a_string_nor_a_table_says_so() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let root = app.engine.root();
    let node = scene::spawn_node(&mut app.engine.world_mut(), "N", root);
    let params: toml::Value = toml::from_str("song = 3").unwrap();
    let error = format!(
        "{:#}",
        components::add(&app.engine, node, "instrument", Some(&params)).unwrap_err()
    );
    assert!(error.contains("song"), "unhelpful: {error}");
    assert!(error.contains("reference string"), "unhelpful: {error}");
}

#[test]
fn an_asset_type_no_plugin_registered_says_which_type_was_asked_for() {
    let dir = project();
    std::fs::write(
        dir.path().join("notes/alien.toml"),
        "type = \"hologram\"\npitch = 1.0\n",
    )
    .unwrap();
    let app = app_in(dir.path(), None);
    let error = assets::load(&app.engine, "notes/alien.toml")
        .unwrap_err()
        .to_string();
    assert!(error.contains("hologram"), "unhelpful: {error}");
    assert!(
        error.contains("note"),
        "the known types are worth saying: {error}"
    );
}

#[test]
fn a_definition_with_no_type_cannot_choose_a_parser_and_says_so() {
    let dir = project();
    std::fs::write(dir.path().join("notes/bare.toml"), "pitch = 1.0\n").unwrap();
    let app = app_in(dir.path(), None);
    let error = assets::load(&app.engine, "notes/bare.toml")
        .unwrap_err()
        .to_string();
    assert!(error.contains("notes/bare.toml"), "unhelpful: {error}");
    assert!(error.contains("type"), "unhelpful: {error}");
}

#[test]
fn exists_answers_for_all_three_reference_forms() {
    let dir = project();
    let app = app_in(dir.path(), None);
    assert!(assets::exists(&app.engine, "notes/scale.toml"));
    assert!(assets::exists(&app.engine, "notes/scale.toml#high"));
    assert!(!assets::exists(&app.engine, "notes/scale.toml#middle"));
    assert!(!assets::exists(&app.engine, "notes/missing.toml"));
    assert!(!assets::exists(&app.engine, "#never_declared"));
}

/// A host that serves files out of a pack and does nothing else.
///
/// This is how a packed run reaches an asset file: `scene_source` is the one
/// method the asset layer uses, and a real backend answers it from the pack.
struct PackedHost {
    pack: Pack,
    modules: RefCell<Vec<String>>,
}

impl balaur_script::ScriptHost<Engine> for PackedHost {
    fn module(&self, name: &str) -> anyhow::Result<Box<dyn balaur_script::Bindings<Engine>>> {
        self.modules.borrow_mut().push(name.to_string());
        Ok(Box::new(balaur_script::NoBindings))
    }
    fn attach(&self, _: balaur_script::NodeId, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn detach(&self, _: balaur_script::NodeId) {}
    fn update(&self, _: f32) {}
    fn pump_reloads(&self) {}
    fn reload(&self, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn call_on(&self, _: balaur_script::NodeId, _: &str, _: &[balaur_script::Value]) {}
    fn call_all(&self, _: &str) {}
    fn scene_source(&self, rel: &str) -> Option<String> {
        self.pack.scenes.get(rel).cloned()
    }
    fn instance_count(&self) -> usize {
        0
    }
    fn invoke(
        &self,
        _: balaur_script::CallbackId,
        _: &[balaur_script::Value],
    ) -> anyhow::Result<balaur_script::Value> {
        Ok(balaur_script::Value::Nil)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A shipped game has no source tree, so an asset that only resolves off disk
struct NoScripts;

impl balaur_script::ScriptCompiler for NoScripts {
    fn extensions(&self) -> &[&str] {
        &[]
    }

    fn compile(&self, _: &str, _: &str) -> anyhow::Result<Vec<u8>> {
        Ok(Vec::new())
    }
}

/// is an asset that works in development and vanishes on release.
#[test]
fn a_packed_run_resolves_an_asset_exactly_as_a_dev_run_does() {
    let dir = project();
    let dev = app_in(dir.path(), None);
    let from_disk = assets::definition(&dev.engine, "notes/scale.toml#high").unwrap();

    let pack = Pack::build(dir.path(), &NoScripts).unwrap();
    assert!(pack.scenes.contains_key("notes/scale.toml"));

    // A project root that does not exist, so nothing can fall back to disk.
    let elsewhere = dir.path().join("shipped");
    let packed = app_in(&elsewhere, Some(pack.clone()));
    packed.engine.set_script_host(Rc::new(PackedHost {
        pack,
        modules: RefCell::new(Vec::new()),
    }));

    let from_pack = assets::definition(&packed.engine, "notes/scale.toml#high").unwrap();
    assert_eq!(from_pack, from_disk);
    assert!((pitch_of(&packed.engine, "notes/scale.toml#high") - 880.0).abs() < f64::EPSILON);
}

#[test]
fn a_schema_declaring_an_asset_property_without_naming_its_type_is_rejected() {
    let panic = std::panic::catch_unwind(|| {
        ComponentDef::parse_schema("instrument", "song = { type = \"asset\", default = \"\" }")
    })
    .unwrap_err();
    let message = panic
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_else(|| "not a string panic".to_string());
    assert!(message.contains("asset"), "unhelpful: {message}");
}

#[test]
fn the_asset_key_belongs_only_to_an_asset_property() {
    let panic = std::panic::catch_unwind(|| {
        ComponentDef::parse_schema(
            "instrument",
            "volume = { type = \"float\", asset = \"note\", default = 1.0 }",
        )
    })
    .unwrap_err();
    let message = panic
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_else(|| "not a string panic".to_string());
    assert!(message.contains("asset"), "unhelpful: {message}");
}

/// An inline definition has no file behind it, so the cache entry is its only
/// copy — asking for a private one, or reloading it, must not lose it.
#[test]
fn an_inline_definition_survives_being_duplicated_and_reloaded() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let root = app.engine.root();
    let node = scene::spawn_node(&mut app.engine.world_mut(), "N", root);
    let table: toml::Value = toml::from_str("song = { pitch = 330.0 }").unwrap();
    components::add(&app.engine, node, "instrument", Some(&table)).unwrap();
    let reference = heard(&app).remove(0);

    let copy = assets::duplicate_definition(&app.engine, &reference).unwrap();
    assert_eq!(
        copy.get("pitch").and_then(toml::Value::as_float),
        Some(330.0)
    );
    assets::reload(&app.engine, &reference).unwrap();
    assert!((pitch_of(&app.engine, &reference) - 330.0).abs() < f64::EPSILON);
}

/// An inline table is as free to be a *library* of named assets as a file is.
/// Without this an inline clip could never be named — and so never be what a
/// component's `autoplay` starts.
#[test]
fn an_entry_inside_an_inline_definition_resolves_by_its_name() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let root = app.engine.root();
    let node = scene::spawn_node(&mut app.engine.world_mut(), "N", root);
    let table: toml::Value =
        toml::from_str("song = { entries = { high = { pitch = 440.0 } } }").unwrap();
    components::add(&app.engine, node, "instrument", Some(&table)).unwrap();
    let reference = heard(&app).remove(0);

    let entry = format!("{reference}#high");
    assert!((pitch_of(&app.engine, &entry) - 440.0).abs() < f64::EPSILON);
    assert!(assets::exists(&app.engine, &entry));
}

#[test]
fn an_entry_an_inline_definition_does_not_have_says_so() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let root = app.engine.root();
    let node = scene::spawn_node(&mut app.engine.world_mut(), "N", root);
    let table: toml::Value =
        toml::from_str("song = { entries = { high = { pitch = 440.0 } } }").unwrap();
    components::add(&app.engine, node, "instrument", Some(&table)).unwrap();
    let reference = heard(&app).remove(0);

    let missing = format!("{reference}#low");
    let err = assets::load_typed::<Note>(&app.engine, &missing).unwrap_err();
    let message = format!("{err:#}");
    assert!(message.contains(&missing), "unhelpful: {message}");
    assert!(message.contains("low"), "unhelpful: {message}");
}

/// The editor's write-back path: a tool edits a definition table and puts it
/// back where it came from, without knowing how the file is encoded.
#[test]
fn saving_an_asset_writes_the_file_and_the_next_load_reads_it() {
    let dir = project();
    let app = app_in(dir.path(), None);
    assert!((pitch_of(&app.engine, "notes/scale.toml#high") - 880.0).abs() < f64::EPSILON);

    let mut document = assets::definition(&app.engine, "notes/scale.toml").unwrap();
    document["entries"]["high"]["pitch"] = toml::Value::Float(1760.0);
    assets::save(&app.engine, "notes/scale.toml", &document).unwrap();

    // On disk, not just in the cache: a second app reading the same directory
    // is what a re-run of the game is.
    let reopened = app_in(dir.path(), None);
    assert!((pitch_of(&reopened.engine, "notes/scale.toml#high") - 1760.0).abs() < f64::EPSILON);
    // And the entry cut from the old text is gone from the live cache too.
    assert!((pitch_of(&app.engine, "notes/scale.toml#high") - 1760.0).abs() < f64::EPSILON);
}

/// Only a file can be saved. An entry belongs to a document and an inline
/// table has no file at all, so both are refused by name rather than written
/// somewhere the caller did not ask for.
#[test]
fn saving_something_that_is_not_a_file_says_which_reference_it_was() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let body = toml::Value::Table(toml::map::Map::new());

    let err = assets::save(&app.engine, "notes/scale.toml#high", &body).unwrap_err();
    let message = format!("{err:#}");
    assert!(
        message.contains("notes/scale.toml#high"),
        "unhelpful: {message}"
    );

    let inline =
        assets::define_inline(&app.engine, "note", toml::toml! { pitch = 5.0 }.into()).unwrap();
    let err = assets::save(&app.engine, &inline.to_string(), &body).unwrap_err();
    assert!(
        format!("{err:#}").contains("not a file"),
        "unhelpful: {err:#}"
    );
}

/// A subsystem holding a parsed asset re-resolves when this moves. It must not
/// move for a file nothing was ever cut from, or every `.toml` the file
/// watcher sees would cost every player a reload.
#[test]
fn the_generation_moves_only_when_a_reload_actually_dropped_something() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let untouched = assets::generation(&app.engine);

    assets::reload(&app.engine, "notes/scale.toml").unwrap();
    assert_eq!(
        assets::generation(&app.engine),
        untouched,
        "nothing was cached, so nothing was dropped"
    );

    let _ = assets::load_typed::<Note>(&app.engine, "notes/scale.toml#high").unwrap();
    assets::reload(&app.engine, "notes/scale.toml").unwrap();
    assert_ne!(
        assets::generation(&app.engine),
        untouched,
        "a cached entry was dropped and nobody was told"
    );
}

/// Where a type's files belong is the type's own business: a tool promoting an
/// inline definition to a file has nowhere else to learn it, because the
/// schema carries the type name and not its home.
#[test]
fn an_asset_type_says_where_its_files_belong() {
    let dir = project();
    let app = app_in(dir.path(), None);
    assert_eq!(assets::directory(&app.engine, "note"), "notes");
    assert_eq!(
        assets::directory(&app.engine, "not_registered"),
        "",
        "an unknown type has no home rather than a made-up one"
    );
}
