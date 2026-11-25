//! The files `[android]` names for the export to read, which the game's pack
//! leaves out on every target: their values reach the APK as resources.

use balaur_export::Options;

#[test]
fn the_google_services_file_stays_out_of_the_pack_and_the_games_own_json_ships() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\n\n[android]\ncapabilities = [\"firebase-messaging\"]\n\
         google_services = \"./config/google-services.json\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::write(
        dir.path().join("config/google-services.json"),
        r#"{ "project_info": {} }"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::write(dir.path().join("data/levels.json"), "[1, 2]").unwrap();
    let out = dir.path().join("t.bpak");
    balaur_export::export(&Options {
        path: dir.path().to_path_buf(),
        output: Some(out.clone()),
        ..Options::default()
    })
    .unwrap();
    let pack = balaur::Pack::decode(&std::fs::read(&out).unwrap()).unwrap();
    assert!(pack.assets.contains_key("data/levels.json"), "{:?}", pack.assets.keys());
    assert!(
        !pack.assets.contains_key("config/google-services.json"),
        "{:?}",
        pack.assets.keys()
    );
}
