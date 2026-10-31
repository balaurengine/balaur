//! Which exports read `[application] icon`: a game a runtime carries does, and
//! a bare pack, which has no platform to show one, does not.

use balaur_export::Options;

fn project_naming(icon: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("[application]\nname = \"t\"\nicon = \"{icon}\"\n"),
    )
    .unwrap();
    dir
}

#[test]
fn a_bare_pack_exports_whatever_the_icon_says() {
    let dir = project_naming("art/missing.png");
    let out = dir.path().join("t.bpak");
    balaur_export::export(&Options {
        path: dir.path().to_path_buf(),
        output: Some(out.clone()),
        ..Options::default()
    })
    .unwrap();
    assert!(out.is_file());
}

#[test]
fn a_game_on_a_runtime_refuses_an_icon_it_cannot_read() {
    let dir = project_naming("art/missing.png");
    let runtime = dir.path().join("balaur-runtime-linux-x64");
    std::fs::write(&runtime, b"a runtime").unwrap();
    let why = balaur_export::export(&Options {
        path: dir.path().to_path_buf(),
        output: Some(dir.path().join("game")),
        target: Some("linux-x64".into()),
        runtime: Some(runtime),
        ..Options::default()
    })
    .unwrap_err();
    assert!(format!("{why:#}").contains("art/missing.png"), "{why:#}");
    assert!(!dir.path().join("game").exists());
}
