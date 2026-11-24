//! A Steam template's `steam_api`, shipped beside the game, and the
//! `steam_appid.txt` only a dev export writes.

use std::path::{Path, PathBuf};

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

/// A template importing `library`, with Valve's file beside it unless
/// `beside` is false.
fn template(dir: &Path, name: &str, header: &[u8], library: &str, beside: bool) -> PathBuf {
    let runtimes = dir.join("runtimes");
    std::fs::create_dir_all(&runtimes).unwrap();
    let path = runtimes.join(name);
    std::fs::write(&path, [header, b"\0", library.as_bytes(), b"\0"].concat()).unwrap();
    if beside {
        std::fs::write(runtimes.join(library), b"valve").unwrap();
    }
    path
}

fn export(dir: &Path, target: &str, runtime: PathBuf, game: &str, dev: bool) -> anyhow::Result<()> {
    balaur_export::export(&Options {
        path: dir.to_path_buf(),
        output: Some(dir.join("out").join(game)),
        target: Some(target.into()),
        runtime: Some(runtime),
        dev,
        ..Options::default()
    })
}

const ELF: &[u8] = b"\x7fELF\x02\x01";

#[test]
fn a_steam_export_carries_the_library_and_no_app_id() {
    let dir = project("[steam]\napp_id = 480\n");
    let runtime = template(dir.path(), "steam-linux", ELF, "libsteam_api.so", true);
    export(dir.path(), "linux-x64", runtime, "game", false).unwrap();
    let out = dir.path().join("out");
    assert_eq!(
        std::fs::read(out.join("libsteam_api.so")).unwrap(),
        b"valve"
    );
    assert!(!out.join("steam_appid.txt").exists());
}

#[test]
fn a_dev_export_writes_the_app_id_beside_the_game() {
    let dir = project("[steam]\napp_id = 480\n");
    let runtime = template(dir.path(), "steam-linux", ELF, "libsteam_api.so", true);
    export(dir.path(), "linux-x64", runtime, "game", true).unwrap();
    let out = dir.path().join("out");
    assert_eq!(
        std::fs::read_to_string(out.join("steam_appid.txt")).unwrap(),
        "480"
    );
    assert!(out.join("libsteam_api.so").is_file());
}

#[test]
fn a_windows_game_gets_the_64_bit_dll_beside_its_exe() {
    let dir = project("[steam]\napp_id = 480\n");
    let runtime = template(
        dir.path(),
        "steam.exe",
        b"MZ\x90\x00",
        "steam_api64.dll",
        true,
    );
    export(dir.path(), "windows-x64", runtime, "game.exe", false).unwrap();
    let out = dir.path().join("out");
    assert!(out.join("steam_api64.dll").is_file());
    assert!(!out.join("steam_appid.txt").exists());
}

#[test]
fn a_steam_template_without_its_library_is_refused() {
    let dir = project("[steam]\napp_id = 480\n");
    let runtime = template(dir.path(), "steam-linux", ELF, "libsteam_api.so", false);
    let why = export(dir.path(), "linux-x64", runtime, "game", false).unwrap_err();
    assert!(format!("{why:#}").contains("libsteam_api.so"), "{why:#}");
}

#[test]
fn a_plain_template_ships_no_steam_library() {
    let dir = project("[steam]\napp_id = 480\n");
    let runtime = template(dir.path(), "plain-linux", ELF, "libc.so.6", true);
    export(dir.path(), "linux-x64", runtime, "game", true).unwrap();
    let out = dir.path().join("out");
    assert!(!out.join("libsteam_api.so").exists());
    assert!(
        !out.join("steam_appid.txt").exists(),
        "no Steam, so no app id"
    );
}
