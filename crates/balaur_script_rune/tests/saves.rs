//! Save games: what a slot round-trips, and how a file written by an older
//! build is brought forward.

use balaur_core::{App, AppConfig};
use balaur_script::Value;

/// Each test writes to the real user data directory, keyed by the project
/// name, so every project() call needs a name of its own.
fn project(name: &str, manifest_extra: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("name = \"{name}\"\nmain_scene = \"main.toml\"\n{manifest_extra}"),
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    for (file, body) in files {
        let path = dir.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    dir
}

fn app_in(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_rune::factory()),
    })
    .unwrap();
    app.load_project().unwrap();
    app
}

/// A name nothing else in the suite uses, so two tests never share a slot.
fn unique(prefix: &str) -> String {
    format!("{prefix}_{}", std::process::id())
}

fn table(pairs: &[(&str, Value)]) -> Value {
    Value::Map(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect(),
    )
}

fn field(value: &Value, name: &str) -> Option<Value> {
    let Value::Map(fields) = value else {
        return None;
    };
    fields
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.clone())
}

#[test]
fn a_slot_round_trips_what_the_game_put_in_it() {
    let name = unique("roundtrip");
    let dir = project(&name, "", &[]);
    let app = app_in(dir.path());
    balaur_core::save::remove(&app.engine, "slot1").unwrap();

    let data = table(&[
        ("level", Value::Int(7)),
        ("health", Value::Num(0.5)),
        ("name", Value::Str("Vasilisa".into())),
    ]);
    balaur_core::save::write(&app.engine, "slot1", &data).unwrap();
    let back = balaur_core::save::read(&app.engine, "slot1").unwrap();

    assert_eq!(field(&back, "level"), Some(Value::Int(7)));
    assert_eq!(field(&back, "health"), Some(Value::Num(0.5)));
    assert_eq!(field(&back, "name"), Some(Value::Str("Vasilisa".into())));
    balaur_core::save::remove(&app.engine, "slot1").unwrap();
}

/// A game asking whether there is a save should not have to catch an error to
/// learn that there is not.
#[test]
fn an_unwritten_slot_reads_as_nil() {
    let dir = project(&unique("empty"), "", &[]);
    let app = app_in(dir.path());
    assert_eq!(
        balaur_core::save::read(&app.engine, "never_written").unwrap(),
        Value::Nil
    );
}

#[test]
fn a_slot_name_that_could_escape_the_directory_is_refused() {
    let dir = project(&unique("escape"), "", &[]);
    let app = app_in(dir.path());
    for bad in ["../secrets", "a/b", "", "with space"] {
        assert!(
            balaur_core::save::write(&app.engine, bad, &Value::Nil).is_err(),
            "'{bad}' should not be a slot name"
        );
    }
}

#[test]
fn slots_lists_what_was_written_and_remove_takes_it_back() {
    let name = unique("listing");
    let dir = project(&name, "", &[]);
    let app = app_in(dir.path());
    let (a, b) = (format!("{name}_a"), format!("{name}_b"));
    for slot in [&a, &b] {
        balaur_core::save::write(&app.engine, slot, &table(&[("n", Value::Int(1))])).unwrap();
    }
    let listed = balaur_core::save::slots(&app.engine);
    assert!(listed.contains(&a) && listed.contains(&b), "{listed:?}");

    balaur_core::save::remove(&app.engine, &a).unwrap();
    assert!(!balaur_core::save::slots(&app.engine).contains(&a));
    // Removing what is already gone is what the caller wanted, not an error.
    balaur_core::save::remove(&app.engine, &a).unwrap();
    balaur_core::save::remove(&app.engine, &b).unwrap();
}

const MIGRATION: &str = "\
// One step at a time: each call only knows how the shape changed between two
// adjacent versions.
pub fn migrate_save(version, data) {
    if version == 1 {
        data.hp = data.health * 100.0;
        data.remove(\"health\");
    } else if version == 2 {
        data.lives = 3;
    }
    data
}
";

#[test]
fn an_old_save_is_brought_forward_one_version_at_a_time() {
    let name = unique("migrate");
    let dir = project(
        &name,
        "\n[save]\nversion = 1\n",
        &[("scripts/saves.rn", MIGRATION)],
    );
    let app = app_in(dir.path());
    let slot = format!("{name}_slot");
    balaur_core::save::remove(&app.engine, &slot).unwrap();
    // Written by the version 1 build.
    balaur_core::save::write(&app.engine, &slot, &table(&[("health", Value::Num(0.75))])).unwrap();
    drop(app);

    // The same project, three versions on.
    std::fs::write(
        dir.path().join("project.toml"),
        format!(
            "name = \"{name}\"\nmain_scene = \"main.toml\"\n\n\
             [save]\nversion = 3\nmigrate = \"scripts/saves.rn\"\n"
        ),
    )
    .unwrap();
    let app = app_in(dir.path());
    let back = balaur_core::save::read(&app.engine, &slot).unwrap();

    assert_eq!(field(&back, "hp"), Some(Value::Num(75.0)), "{back:?}");
    assert_eq!(field(&back, "lives"), Some(Value::Int(3)), "{back:?}");
    assert_eq!(field(&back, "health"), None, "the old field was dropped");
    balaur_core::save::remove(&app.engine, &slot).unwrap();
}

/// An older build cannot guess what a newer one wrote, so it says so rather
/// than handing the game half a save.
#[test]
fn a_save_from_a_newer_build_is_refused() {
    let name = unique("newer");
    let dir = project(&name, "\n[save]\nversion = 5\n", &[]);
    let app = app_in(dir.path());
    let slot = format!("{name}_slot");
    balaur_core::save::write(&app.engine, &slot, &table(&[("n", Value::Int(1))])).unwrap();
    drop(app);

    std::fs::write(
        dir.path().join("project.toml"),
        format!("name = \"{name}\"\nmain_scene = \"main.toml\"\n\n[save]\nversion = 2\n"),
    )
    .unwrap();
    let app = app_in(dir.path());
    let err = balaur_core::save::read(&app.engine, &slot).unwrap_err();
    assert!(format!("{err:#}").contains("newer build"), "{err:#}");
    balaur_core::save::remove(&app.engine, &slot).unwrap();
}

/// A save that needs migrating and a project that declares no script for it:
/// the error names what is missing rather than handing back the old shape.
#[test]
fn a_migration_with_no_script_says_so() {
    let name = unique("nomigrate");
    let dir = project(&name, "\n[save]\nversion = 1\n", &[]);
    let app = app_in(dir.path());
    let slot = format!("{name}_slot");
    balaur_core::save::write(&app.engine, &slot, &table(&[("n", Value::Int(1))])).unwrap();
    drop(app);

    std::fs::write(
        dir.path().join("project.toml"),
        format!("name = \"{name}\"\nmain_scene = \"main.toml\"\n\n[save]\nversion = 2\n"),
    )
    .unwrap();
    let app = app_in(dir.path());
    let err = balaur_core::save::read(&app.engine, &slot).unwrap_err();
    assert!(format!("{err:#}").contains("migrate"), "{err:#}");
    balaur_core::save::remove(&app.engine, &slot).unwrap();
}
