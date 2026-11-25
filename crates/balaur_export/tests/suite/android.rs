//! The files `[android]` names for the export to read, which the game's pack
//! leaves out on every target: their values reach the APK as resources.

use balaur_export::Options;

#[test]
fn the_google_services_file_stays_out_of_the_pack_and_the_games_own_files_ship() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\n\n[android]\ncapabilities = [\"firebase-messaging\"]\n\
         google_services = \"./config/google-services.json\"\n",
    )
    .unwrap();
    for (file, bytes) in [
        (
            "config/google-services.json",
            &br#"{ "project_info": {} }"#[..],
        ),
        ("art/ship.png", b"a png"),
        ("data/levels.json", b"[1, 2]"),
    ] {
        let path = dir.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let out = dir.path().join("t.bpak");
    balaur_export::export(&Options {
        path: dir.path().to_path_buf(),
        output: Some(out.clone()),
        ..Options::default()
    })
    .unwrap();
    let pack = balaur::Pack::decode(&std::fs::read(&out).unwrap()).unwrap();
    let keys: Vec<&String> = pack.assets.keys().collect();
    for kept in ["data/levels.json", "art/ship.png"] {
        assert!(pack.assets.contains_key(kept), "{kept}: {keys:?}");
    }
    assert!(
        !pack.assets.contains_key("config/google-services.json"),
        "{keys:?}"
    );
}
