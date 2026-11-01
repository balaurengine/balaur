//! `[export] runtime`: which runtime a target puts the game on, and the games
//! a one-world runtime refuses.

use balaur_export::Options;

fn project(manifest: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("[application]\nname = \"t\"\n\n{manifest}"),
    )
    .unwrap();
    dir
}

fn export_for(dir: &std::path::Path, target: &str) -> anyhow::Result<()> {
    balaur_export::export(&Options {
        path: dir.to_path_buf(),
        output: Some(dir.join("game")),
        target: Some(target.into()),
        runtime_roots: vec![dir.join("runtimes")],
        ..Options::default()
    })
}

fn install(dir: &std::path::Path, runtime: &str) {
    std::fs::create_dir_all(dir.join("runtimes")).unwrap();
    std::fs::write(dir.join("runtimes").join(runtime), b"a runtime").unwrap();
}

#[test]
fn a_target_goes_on_the_runtime_the_project_names() {
    let dir = project("[export]\nruntime = \"2d\"\n");
    install(dir.path(), "balaur-runtime-linux-x64");
    let missing = format!("{:#}", export_for(dir.path(), "linux-x64").unwrap_err());
    assert!(missing.contains("linux-x64-2d"), "{missing}");

    install(dir.path(), "balaur-runtime-linux-x64-2d");
    export_for(dir.path(), "linux-x64").unwrap();
    assert!(
        std::fs::read(dir.path().join("game"))
            .unwrap()
            .starts_with(b"a runtime")
    );
}

#[test]
fn a_server_keeps_its_own_runtime() {
    let dir = project("[export]\nruntime = \"3d\"\n");
    install(dir.path(), "balaur-runtime-linux-x64-server");
    export_for(dir.path(), "linux-x64-server").unwrap();
}

#[test]
fn a_target_override_picks_a_runtime_for_that_target_alone() {
    let dir = project("[export]\nruntime = \"2d\"\n\n[override.web.export]\nruntime = \"3d\"\n");
    assert_eq!(
        balaur_export::runtime_of(
            dir.path(),
            "web",
            &balaur_export::SmallestRuntime::default()
        )
        .unwrap(),
        "web-3d"
    );
    assert_eq!(
        balaur_export::runtime_of(
            dir.path(),
            "android",
            &balaur_export::SmallestRuntime::default()
        )
        .unwrap(),
        "android-2d"
    );
    assert_eq!(
        balaur_export::runtime_of(
            dir.path(),
            "linux-x64-server",
            &balaur_export::SmallestRuntime::default()
        )
        .unwrap(),
        "linux-x64-server"
    );
}

#[test]
fn a_one_world_runtime_named_as_a_target_points_at_the_setting() {
    let dir = project("");
    let why = format!("{:#}", export_for(dir.path(), "web-2d").unwrap_err());
    assert!(why.contains("runtime = \"2d\""), "{why}");
}

#[test]
fn a_2d_runtime_refuses_a_game_with_a_3d_body() {
    let dir = project("[export]\nruntime = \"2d\"\n");
    install(dir.path(), "balaur-runtime-linux-x64-2d");
    std::fs::create_dir(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("scenes/main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Crate\"\n\n[nodes.body3d]\nkind = \"dynamic\"\n",
    )
    .unwrap();
    let why = format!("{:#}", export_for(dir.path(), "linux-x64").unwrap_err());
    assert!(why.contains("node \"Crate\" has body3d"), "{why}");
    assert!(!dir.path().join("game").exists());
}

#[test]
fn a_game_goes_on_the_smallest_runtime_it_fits() {
    let with_scene = |nodes: &str| {
        let dir = project("");
        std::fs::write(dir.path().join("main.toml"), nodes).unwrap();
        dir
    };
    let runtime = |dir: &tempfile::TempDir| {
        balaur_export::runtime_of(
            dir.path(),
            "web",
            &balaur_export::SmallestRuntime::default(),
        )
        .unwrap()
    };
    assert_eq!(runtime(&with_scene("")), "web-2d", "no physics at all");
    let flat = "[[nodes]]\nid = \"a\"\nname = \"A\"\n[nodes.body2d]\nkind = \"dynamic\"\n";
    assert_eq!(runtime(&with_scene(flat)), "web-2d");
    let deep = "[[nodes]]\nid = \"b\"\nname = \"B\"\n[nodes.body3d]\nkind = \"dynamic\"\n";
    assert_eq!(runtime(&with_scene(deep)), "web-3d");
    assert_eq!(runtime(&with_scene(&format!("{flat}{deep}"))), "web");
    let named = project("[export]\nruntime = \"full\"\n");
    assert_eq!(runtime(&named), "web", "a project may still name one");
}
