//! What a script hears when its file is rebuilt under a running scene.

#[test]
fn a_reload_tells_the_instances_of_that_file_and_keeps_their_state() {
    let dir = tempfile::tempdir().unwrap();
    let script = |class: &str| {
        format!(
            "pub class {class} {{ node: Node, pub reloads: int, pub kept: int }}\nimpl {class} {{\n    pub fn init() {{ self.kept = 7; }}\n    pub fn on_hot_reload() {{ self.reloads = self.reloads + 1; }}\n}}\n"
        )
    };
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"a\"\nname = \"A\"\nscript = { source = \"scripts/a.bs\" }\n\n[[nodes]]\nid = \"b\"\nname = \"B\"\nparent = \"a\"\nscript = { source = \"scripts/b.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/a.bs"), script("A")).unwrap();
    std::fs::write(dir.path().join("scripts/b.bs"), script("B")).unwrap();
    let mut app = balaur::standard_app(balaur::AppConfig::dev(
        dir.path().to_string_lossy().as_ref(),
    ))
    .unwrap();
    app.load_project().unwrap();
    let host = app.engine.script_host().unwrap();
    let field = |id: &str, name: &str| {
        let world = app.engine.world();
        let node = balaur_core::ids::find(&world, app.engine.root(), id).unwrap();
        host.script_field(balaur::node_id_of(node), name)
    };
    let int = balaur_script::Value::Int;

    host.reload(&["scripts/a.bs"]).unwrap();
    assert_eq!(
        field("a", "reloads"),
        Some(int(1)),
        "the file reloaded hears"
    );
    assert_eq!(field("a", "kept"), Some(int(7)), "with its state kept");
    assert_eq!(field("b", "reloads"), Some(int(0)), "another file does not");
    assert_eq!(
        field("b", "kept"),
        Some(int(7)),
        "though it was carried too"
    );
}

fn fixture_project(dir: &std::path::Path) {
    std::fs::write(
        dir.join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.toml"),
        "[[nodes]]\nid = \"a\"\nname = \"A\"\n",
    )
    .unwrap();
}

/// The lists `files_changed` carried in the pump just run.
fn said(app: &balaur::App) -> Vec<Vec<balaur_script::Value>> {
    balaur_core::events::delivered(&app.engine, balaur_core::events::FILES_CHANGED)
        .into_iter()
        .filter_map(|paths| match paths {
            balaur_script::Value::List(paths) => Some(paths),
            _ => None,
        })
        .collect()
}

fn watched_app(project: &std::path::Path) -> balaur::App {
    fixture_project(project);
    let mut app =
        balaur::standard_app(balaur::AppConfig::dev(project.to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app
}

/// The most ticks a wait takes, a minute and more of them at the pause below:
/// a cap for a stuck watch, never what a passing run waits for.
const CAP: usize = 12_000;

/// Tick until `done` holds, with a pause between ticks: a watch's events come
/// from the OS's own thread, and a burst ends once writes have been quiet.
fn until(app: &mut balaur::App, mut done: impl FnMut(&balaur::App) -> bool) -> bool {
    for _ in 0..CAP {
        app.tick(1.0 / 60.0);
        if done(app) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

#[test]
fn a_watched_directory_says_what_changed_and_reloads_its_scripts() {
    let project = tempfile::tempdir().unwrap();
    let mut app = watched_app(project.path());
    let game = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(game.path().join("scripts")).unwrap();
    let file = game.path().join("scripts/visitor.bs");
    let source = |version: u32| {
        format!(
            "pub class Visitor {{ node: Node, pub reloads: int }}\nimpl Visitor {{\n    pub fn on_hot_reload() {{ self.reloads = self.reloads + 1; }}\n    pub fn version() -> int {{ {version} }}\n}}\n"
        )
    };
    std::fs::write(&file, source(0)).unwrap();
    balaur::file_api::add_root(&app.engine, game.path());
    let host = app.engine.script_host().unwrap();
    let key = game.path().to_string_lossy().into_owned();
    let real = game.path().canonicalize().unwrap();
    assert!(host.watch(&real, &key, &[]), "this run follows files");
    let script = file.to_string_lossy().into_owned();
    let world = app.engine.world();
    let node = balaur_core::ids::find(&world, app.engine.root(), "a").unwrap();
    drop(world);
    let node = balaur::node_id_of(node);
    host.attach_with_props(node, &script, &[]).unwrap();

    let told = balaur_script::Value::Str(format!("{key}/scripts/visitor.bs"));
    let mut heard = false;
    let mut reloaded = false;
    // Once: a watch hears what is written the moment it returns.
    std::fs::write(&file, source(1)).unwrap();
    until(&mut app, |app| {
        heard |= said(app).iter().any(|paths| paths.contains(&told));
        reloaded |= !matches!(
            host.script_field(node, "reloads"),
            Some(balaur_script::Value::Int(0)) | None
        );
        heard && reloaded
    });
    assert!(
        heard && reloaded,
        "heard the change: {heard}, reloaded the script: {reloaded}"
    );
}

#[test]
fn a_script_in_a_subfolder_of_the_project_reloads_when_written() {
    let project = tempfile::tempdir().unwrap();
    let source = |version: u32| {
        format!(
            "pub class Player {{ node: Node, pub reloads: int }}\nimpl Player {{\n    pub fn on_hot_reload() {{ self.reloads = self.reloads + 1; }}\n    pub fn version() -> int {{ {version} }}\n}}\n"
        )
    };
    let file = project.path().join("scripts/player/player.bs");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, source(0)).unwrap();
    std::fs::write(
        project.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("main.toml"),
        "[[nodes]]\nid = \"a\"\nname = \"A\"\nscript = { source = \"scripts/player/player.bs\" }\n",
    )
    .unwrap();
    let mut app = balaur::standard_app(balaur::AppConfig::dev(
        project.path().to_string_lossy().as_ref(),
    ))
    .unwrap();
    app.load_project().unwrap();
    let host = app.engine.script_host().unwrap();
    let world = app.engine.world();
    let node = balaur_core::ids::find(&world, app.engine.root(), "a").unwrap();
    drop(world);
    let node = balaur::node_id_of(node);
    let reloads = || host.script_field(node, "reloads");
    assert_eq!(
        reloads(),
        Some(balaur_script::Value::Int(0)),
        "the script ran"
    );

    std::fs::write(&file, source(1)).unwrap();
    let reloaded = until(&mut app, |_| {
        !matches!(reloads(), Some(balaur_script::Value::Int(0)) | None)
    });
    assert!(reloaded, "scripts/player/player.bs was never reloaded");
}

#[test]
fn any_file_saved_in_the_project_is_named_and_moves_the_asset_generation() {
    let project = tempfile::tempdir().unwrap();
    let mut app = watched_app(project.path());
    let image = "art/heroes/hero.png";
    let custom = "data/levels/one.level";
    let hidden = ".tool/state.txt";
    for rel in [image, custom, hidden] {
        let at = project.path().join(rel);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, "0").unwrap();
    }
    let before = balaur_core::assets::generation(&app.engine);
    let mut named: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let heard = |named: &std::collections::BTreeSet<String>| {
        named.contains(image) && named.contains(custom)
    };
    // Again until heard: the watch may begin after the first writes.
    let mut writes = 0;
    until(&mut app, |app| {
        for paths in said(app) {
            named.extend(paths.into_iter().filter_map(|path| match path {
                balaur_script::Value::Str(path) => Some(path),
                _ => None,
            }));
        }
        writes += 1;
        if writes % 200 == 0 {
            for rel in [image, custom, hidden] {
                std::fs::write(project.path().join(rel), writes.to_string()).unwrap();
            }
        }
        heard(&named)
    });
    assert!(heard(&named), "named only {named:?}");
    assert!(
        !named.iter().any(|path| path.starts_with('.')),
        "a dot folder is a tool's: {named:?}"
    );
    assert_ne!(
        balaur_core::assets::generation(&app.engine),
        before,
        "what was derived from a file is derived again"
    );
}

#[test]
fn a_toml_asset_saved_on_disk_is_read_again() {
    let project = tempfile::tempdir().unwrap();
    let file = project.path().join("materials/stone.toml");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let body = |roughness: f64| format!("type = \"material\"\nroughness = {roughness}\n");
    std::fs::write(&file, body(0.25)).unwrap();
    let mut app = watched_app(project.path());
    let roughness = |app: &balaur::App| {
        balaur_core::assets::definition(&app.engine, "materials/stone.toml")
            .ok()
            .and_then(|table| table.get("roughness").and_then(toml::Value::as_float))
    };
    assert_eq!(roughness(&app), Some(0.25), "the asset was read");
    let mut writes = 0;
    let read_again = until(&mut app, |app| {
        writes += 1;
        if writes % 200 == 1 {
            std::fs::write(&file, body(0.75)).unwrap();
        }
        roughness(app) == Some(0.75)
    });
    assert!(
        read_again,
        "materials/stone.toml kept {:?}",
        roughness(&app)
    );
}

#[test]
fn ten_thousand_files_changed_at_once_arrive_in_a_handful_of_lists() {
    const FILES: usize = 10_000;
    let project = tempfile::tempdir().unwrap();
    let mut app = watched_app(project.path());
    let game = tempfile::tempdir().unwrap();
    balaur::file_api::add_root(&app.engine, game.path());
    let host = app.engine.script_host().unwrap();
    let key = game.path().to_string_lossy().into_owned();
    let real = game.path().canonicalize().unwrap();
    assert!(host.watch(&real, &key, &["bs".to_string()]));

    std::fs::write(game.path().join("probe.bs"), "// a probe").unwrap();
    assert!(
        until(&mut app, |app| !said(app).is_empty()),
        "the watch never began"
    );

    for at in 0..FILES {
        std::fs::write(game.path().join(format!("f{at}.bs")), "// a file").unwrap();
    }
    std::fs::write(game.path().join("skipped.png"), "not a script").unwrap();

    let mut lists = 0;
    let mut named: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut lost_count = false;
    until(&mut app, |app| {
        for paths in said(app) {
            lists += 1;
            lost_count |= paths.is_empty();
            named.extend(paths.into_iter().map(|path| match path {
                balaur_script::Value::Str(path) => path,
                other => panic!("not a path: {other:?}"),
            }));
        }
        lost_count || named.len() >= FILES
    });
    assert!(
        lost_count || named.len() >= FILES,
        "{} of {FILES} heard",
        named.len()
    );
    assert!(lists <= FILES / 100, "{lists} lists for {FILES} files");
    assert!(!named.contains(&format!("{key}/skipped.png")));
}

#[test]
fn a_run_with_watching_off_follows_nothing() {
    let project = tempfile::tempdir().unwrap();
    fixture_project(project.path());
    let mut config = balaur::AppConfig::dev(project.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = balaur::standard_app(config).unwrap();
    app.load_project().unwrap();
    let host = app.engine.script_host().unwrap();
    let real = project.path().canonicalize().unwrap();
    assert!(!host.watch(&real, "elsewhere", &[]));
}

// macOS gives a process about 512 FSEvents streams; past them a new watch
// starts and hears nothing. An app that outlived its owner kept its stream.
#[test]
fn dropping_an_app_frees_its_script_host_and_its_watch() {
    let project = tempfile::tempdir().unwrap();
    let app = watched_app(project.path());
    let host = std::rc::Rc::downgrade(&app.engine.script_host().unwrap());
    drop(app);
    assert!(host.upgrade().is_none(), "the script host outlived its app");
}
