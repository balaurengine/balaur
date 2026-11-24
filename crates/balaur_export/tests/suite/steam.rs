//! A `--steam` export ships Valve's `steam_api` beside the game, from beside
//! the runtime; any other export ships none, `[steam]` table or not.

use std::path::{Path, PathBuf};

use balaur_export::Options;

const APP_ID: &str = "[steam]\napp_id = 480\n";
const ELF: &[u8] = b"\x7fELF\x02\x01";
const PE: &[u8] = b"MZ\x90\x00";

fn project(manifest: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("[application]\nname = \"t\"\n\n{manifest}"),
    )
    .unwrap();
    dir
}

/// A runtime, with Valve's `library` beside it as a release stages it,
/// `balaur-runtime-<target>.<library>`.
fn runtime(dir: &Path, target: &str, exe: &str, header: &[u8], library: &str) -> PathBuf {
    let runtimes = dir.join("runtimes");
    std::fs::create_dir_all(&runtimes).unwrap();
    let path = runtimes.join(format!("balaur-runtime-{target}{exe}"));
    std::fs::write(&path, [header, b"\0code\0"].concat()).unwrap();
    std::fs::write(balaur_export::steam_api_beside(&path, library), b"valve").unwrap();
    path
}

fn export(
    dir: &Path,
    target: &str,
    runtime: PathBuf,
    game: &str,
    steam: bool,
    dev: bool,
) -> anyhow::Result<()> {
    balaur_export::export(&Options {
        path: dir.to_path_buf(),
        output: Some(dir.join("out").join(game)),
        target: Some(target.into()),
        runtime: Some(runtime),
        steam,
        dev,
        ..Options::default()
    })
}

#[test]
fn a_steam_export_carries_the_library_and_no_app_id() {
    let dir = project(APP_ID);
    let runtime = runtime(dir.path(), "linux-x64", "", ELF, "libsteam_api.so");
    export(dir.path(), "linux-x64", runtime, "game", true, false).unwrap();
    let out = dir.path().join("out");
    assert_eq!(
        std::fs::read(out.join("libsteam_api.so")).unwrap(),
        b"valve"
    );
    assert!(!out.join("steam_appid.txt").exists());
}

#[test]
fn an_app_id_without_steam_ships_no_valve_file() {
    let dir = project(APP_ID);
    let runtime = runtime(dir.path(), "linux-x64", "", ELF, "libsteam_api.so");
    export(dir.path(), "linux-x64", runtime, "game", false, false).unwrap();
    let out = dir.path().join("out");
    assert!(out.join("game").is_file(), "the game itself is there");
    assert!(!out.join("libsteam_api.so").exists());
    assert!(!out.join("steam_appid.txt").exists());
}

#[test]
fn a_dev_export_writes_the_app_id_beside_the_game() {
    let dir = project(APP_ID);
    let runtime = runtime(dir.path(), "linux-x64", "", ELF, "libsteam_api.so");
    export(dir.path(), "linux-x64", runtime, "game", true, true).unwrap();
    let out = dir.path().join("out");
    assert_eq!(
        std::fs::read_to_string(out.join("steam_appid.txt")).unwrap(),
        "480"
    );
    assert!(out.join("libsteam_api.so").is_file());
}

#[test]
fn a_windows_game_gets_the_64_bit_dll_beside_its_exe() {
    let dir = project(APP_ID);
    let runtime = runtime(dir.path(), "windows-x64", ".exe", PE, "steam_api64.dll");
    export(dir.path(), "windows-x64", runtime, "game.exe", true, false).unwrap();
    let out = dir.path().join("out");
    assert!(out.join("steam_api64.dll").is_file());
    assert!(!out.join("steam_appid.txt").exists());
}

#[test]
fn a_steam_export_needs_an_app_id() {
    let dir = project("");
    let runtime = runtime(dir.path(), "linux-x64", "", ELF, "libsteam_api.so");
    let why = export(dir.path(), "linux-x64", runtime, "game", true, false).unwrap_err();
    assert!(format!("{why:#}").contains("app_id"), "{why:#}");
}

#[test]
fn a_dev_export_needs_steam() {
    let dir = project(APP_ID);
    let runtime = runtime(dir.path(), "linux-x64", "", ELF, "libsteam_api.so");
    let why = export(dir.path(), "linux-x64", runtime, "game", false, true).unwrap_err();
    assert!(format!("{why:#}").contains("--steam"), "{why:#}");
}

#[test]
fn a_runtime_without_the_library_refuses_a_steam_export() {
    let dir = project(APP_ID);
    let runtimes = dir.path().join("runtimes");
    std::fs::create_dir_all(&runtimes).unwrap();
    let bare = runtimes.join("balaur-runtime-linux-x64");
    std::fs::write(&bare, ELF).unwrap();
    let why = export(dir.path(), "linux-x64", bare, "game", true, false).unwrap_err();
    assert!(format!("{why:#}").contains("libsteam_api.so"), "{why:#}");
}

#[test]
fn a_mac_app_carries_the_library_in_contents_macos_and_the_overlay_entitlements() {
    let manifest = format!(
        "[apple]\nbundle_id = \"org.balaur.t\"\nversion = \"1.0\"\nbuild_number = \"1\"\n\n{APP_ID}"
    );
    let dir = project(&manifest);
    let runtimes = dir.path().join("runtimes");
    std::fs::create_dir_all(&runtimes).unwrap();
    let path = runtimes.join("balaur-runtime-macos-universal");
    // A Mac signs what it bundles, so there the runtime and the library are
    // real Mach-O files; elsewhere the bundle stays unsigned.
    let sdk = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../balaur_steam/sdk/redistributable_bin/osx/libsteam_api.dylib");
    if cfg!(target_os = "macos") {
        std::fs::copy("/usr/bin/true", &path).unwrap();
    } else {
        std::fs::write(&path, [0xca, 0xfe, 0xba, 0xbe]).unwrap();
    }
    std::fs::copy(
        &sdk,
        balaur_export::steam_api_beside(&path, "libsteam_api.dylib"),
    )
    .unwrap();
    for (steam, out) in [(true, "steam"), (false, "plain")] {
        balaur_export::export(&Options {
            path: dir.path().to_path_buf(),
            output: Some(dir.path().join(out).join("t.app")),
            target: Some("macos-universal".into()),
            runtime: Some(path.clone()),
            app: true,
            steam,
            ..Options::default()
        })
        .unwrap();
    }
    let macos = dir.path().join("steam/t.app/Contents/MacOS");
    assert!(
        macos.join("libsteam_api.dylib").is_file(),
        "Valve's library sits beside the executable"
    );
    let entitlements =
        std::fs::read_to_string(entitlements_in(&dir.path().join("steam")).unwrap()).unwrap();
    assert!(entitlements.contains("com.apple.security.cs.disable-library-validation"));
    assert!(entitlements.contains("com.apple.security.cs.allow-dyld-environment-variables"));
    assert!(
        !dir.path()
            .join("plain/t.app/Contents/MacOS/libsteam_api.dylib")
            .exists()
    );
    let plain = entitlements_in(&dir.path().join("plain"))
        .map(|path| std::fs::read_to_string(path).unwrap())
        .unwrap_or_default();
    assert!(
        !plain.contains("disable-library-validation") && !plain.contains("allow-dyld"),
        "an export without --steam asks for neither exception: {plain}"
    );
}

/// The entitlements an export wrote beside its bundle, named for the project.
fn entitlements_in(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|e| e == "entitlements"))
}
