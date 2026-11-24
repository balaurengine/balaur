use super::{Abi, AndroidConfig, payload_files, version_key};

/// A layout carrying every ABI the template ships.
fn layout(dir: &std::path::Path) -> &std::path::Path {
    for abi in ["arm64-v8a", "armeabi-v7a", "x86", "x86_64"] {
        std::fs::create_dir_all(dir.join("lib").join(abi)).unwrap();
        std::fs::write(dir.join("lib").join(abi).join("libmain.so"), b"so").unwrap();
    }
    dir
}

#[test]
fn a_project_that_names_no_abi_keeps_every_one() {
    let dir = tempfile::tempdir().unwrap();
    let layout = layout(dir.path());
    AndroidConfig::default().prune(layout).unwrap();
    assert_eq!(super::carried(&layout.join("lib")).len(), 4);
}

#[test]
fn the_abis_a_project_names_are_the_ones_that_survive() {
    let dir = tempfile::tempdir().unwrap();
    let layout = layout(dir.path());
    let config = AndroidConfig {
        abis: vec![Abi::Arm64V8a, Abi::X86_64],
        ..Default::default()
    };
    config.prune(layout).unwrap();
    assert_eq!(super::carried(&layout.join("lib")), ["arm64-v8a", "x86_64"]);
}

#[test]
fn an_abi_the_template_does_not_carry_names_the_ones_it_does() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("lib/arm64-v8a")).unwrap();
    let config = AndroidConfig {
        abis: vec![Abi::X86],
        ..Default::default()
    };
    let err = config
        .prune(dir.path())
        .expect_err("an ABI with no library")
        .to_string();
    assert!(err.contains("x86"), "{err}");
    assert!(err.contains("arm64-v8a"), "{err}");
}

/// The manifest scripts/package_runtime.sh stages, activity and all.
const TEMPLATE_WITH_ACTIVITY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
package="org.balaur.template"
android:versionCode="1"
android:versionName="1.0">
  <uses-sdk android:minSdkVersion="26" android:targetSdkVersion="36" />
  <application android:label="Balaur" android:hasCode="true">
<activity
    android:name="android.app.NativeActivity"
    android:exported="true"
    android:configChanges="orientation|keyboardHidden|screenSize">
</activity>
  </application>
</manifest>
"#;

/// The manifest scripts/package_runtime.sh stages.
pub(super) const TEMPLATE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
package="org.balaur.template"
android:versionCode="1"
android:versionName="1.0">
  <uses-sdk android:minSdkVersion="26" android:targetSdkVersion="36" />
  <application android:label="Balaur" android:hasCode="true">
  </application>
</manifest>
"#;

/// Everything an export needs named, as a project declares it.
pub(super) fn named() -> AndroidConfig {
    AndroidConfig {
        application_id: "com.studio.tide".into(),
        title: "Tide".into(),
        version: "1.0".into(),
        version_code: 1,
        ..AndroidConfig::default()
    }
}

#[test]
fn an_orientation_reaches_the_activity() {
    let config = AndroidConfig {
        orientation: balaur::project::Orientation::Landscape,
        ..named()
    };
    let xml = config.manifest(TEMPLATE_WITH_ACTIVITY).unwrap();
    assert!(
        xml.contains(r#"android:screenOrientation="landscape""#),
        "{xml}"
    );
}

// A game that names none writes the manifest it always did.
#[test]
fn no_orientation_leaves_the_manifest_alone() {
    let xml = named().manifest(TEMPLATE_WITH_ACTIVITY).unwrap();
    assert!(!xml.contains("screenOrientation"), "{xml}");
}

#[test]
fn the_label_is_the_application_name_unless_display_name_says_otherwise() {
    let xml = named().manifest(TEMPLATE).unwrap();
    assert!(xml.contains(r#"package="com.studio.tide""#), "{xml}");
    assert!(xml.contains(r#"android:label="Tide""#), "{xml}");
    assert!(!xml.contains("org.balaur.template"), "{xml}");
    // The template's own floor, kept because the project named none.
    assert!(xml.contains(r#"android:minSdkVersion="26""#), "{xml}");
}

// An Android build reaches no server without INTERNET, so a game that
// says nothing has it; one that goes offline on purpose says so.
#[test]
fn the_manifest_declares_internet_unless_the_game_says_otherwise() {
    let xml = named().manifest(TEMPLATE).unwrap();
    assert!(
        xml.contains(r#"<uses-permission android:name="android.permission.INTERNET" />"#),
        "{xml}"
    );
    assert!(
        xml.find("uses-permission").unwrap() < xml.find("<application").unwrap(),
        "{xml}"
    );
    let offline = AndroidConfig {
        permissions: Vec::new(),
        ..named()
    };
    assert!(
        !offline
            .manifest(TEMPLATE)
            .unwrap()
            .contains("uses-permission")
    );
}

#[test]
fn every_example_exports_to_android() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut checked = 0;
    for entry in std::fs::read_dir(&examples).unwrap() {
        let project = entry.unwrap().path();
        let Ok(text) = std::fs::read_to_string(project.join("project.toml")) else {
            continue;
        };
        let manifest: toml::Table = toml::from_str(&text).unwrap();
        AndroidConfig::from_manifest(&manifest, &project)
            .and_then(|config| config.manifest(TEMPLATE))
            .unwrap_or_else(|err| panic!("{}: {err}", project.display()));
        checked += 1;
    }
    assert!(checked > 0, "no example under {}", examples.display());
}

// Nothing is invented: each key missing is refused by name, in the order
// a project would fill them in.
#[test]
fn a_game_that_declares_nothing_is_refused_key_by_key() {
    let refusal = |config: AndroidConfig| config.manifest(TEMPLATE).unwrap_err().to_string();
    let no_name = AndroidConfig {
        title: String::new(),
        ..named()
    };
    assert!(refusal(no_name).contains("[application] name"));
    let no_id = AndroidConfig {
        application_id: String::new(),
        ..named()
    };
    assert!(refusal(no_id).contains("application_id"));
    let no_version = AndroidConfig {
        version: String::new(),
        ..named()
    };
    assert!(refusal(no_version).contains("version is not set"));
    let no_code = AndroidConfig {
        version_code: 0,
        ..named()
    };
    assert!(refusal(no_code).contains("version_code"));
}

#[test]
fn play_games_without_its_project_id_is_refused() {
    let refusal = |config: AndroidConfig| config.manifest(TEMPLATE).unwrap_err().to_string();
    let games = AndroidConfig {
        capabilities: vec![super::play::Capability::PlayGames],
        ..named()
    };
    assert!(refusal(games.clone()).contains("games_project_id is not set"));
    let lettered = AndroidConfig {
        games_project_id: "12ab".into(),
        ..games.clone()
    };
    assert!(refusal(lettered).contains("all digits"));
    let stray = AndroidConfig {
        games_project_id: "123".into(),
        ..named()
    };
    assert!(refusal(stray).contains("does not name play-games"));
    let declared = AndroidConfig {
        games_project_id: "123456789012".into(),
        ..games
    };
    declared.manifest(TEMPLATE).unwrap();
}

#[test]
fn firebase_messaging_without_its_google_services_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let refusal = |config: AndroidConfig| format!("{:#}", config.manifest(TEMPLATE).unwrap_err());
    let push = AndroidConfig {
        capabilities: vec![super::play::Capability::FirebaseMessaging],
        project: dir.path().to_path_buf(),
        ..named()
    };
    assert!(refusal(push.clone()).contains("google_services is not set"));
    let missing = AndroidConfig {
        google_services: "google-services.json".into(),
        ..push.clone()
    };
    assert!(refusal(missing.clone()).contains("does not read"));
    std::fs::write(
        dir.path().join("google-services.json"),
        r#"{ "project_info": { "project_number": "1", "project_id": "p" },
             "client": [ { "client_info": { "mobilesdk_app_id": "1:1:android:a",
               "android_client_info": { "package_name": "com.studio.other" } },
               "api_key": [ { "current_key": "k" } ] } ] }"#,
    )
    .unwrap();
    assert!(refusal(missing.clone()).contains("no app for application_id \"com.studio.tide\""));
    let stray = AndroidConfig {
        google_services: "google-services.json".into(),
        project: dir.path().to_path_buf(),
        ..named()
    };
    assert!(refusal(stray).contains("does not name firebase-messaging"));
}

#[test]
fn a_misspelled_capability_is_refused_when_the_table_is_read() {
    let manifest: toml::Table =
        toml::from_str("[android]\ncapabilities = [\"play_games\"]\n").unwrap();
    let err = AndroidConfig::from_manifest(&manifest, std::path::Path::new(".")).unwrap_err();
    assert!(format!("{err:#}").contains("play-games"), "{err:#}");
}

#[test]
fn numbered_dex_files_ship_and_nothing_else_named_like_them() {
    for name in ["classes.dex", "classes2.dex", "classes17.dex"] {
        assert!(super::is_dex(name), "{name}");
    }
    for name in [
        "classes1.dex",
        "classes0.dex",
        "classesx.dex",
        "other.dex",
        "classes.dex.bak",
    ] {
        assert!(!super::is_dex(name), "{name}");
    }
}

#[test]
fn no_keystore_signs_with_the_debug_key_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let err = super::keystore_for(dir.path(), &named())
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("--sign debug"), "{err}");
}

#[test]
fn the_project_names_the_id_the_version_and_the_label() {
    let config = AndroidConfig {
        application_id: "com.studio.tide".into(),
        display_name: "Tide & Sand".into(),
        version: "2.3".into(),
        version_code: 17,
        target_sdk: 34,
        ..named()
    };
    let xml = config.manifest(TEMPLATE).unwrap();
    assert!(xml.contains(r#"package="com.studio.tide""#), "{xml}");
    assert!(xml.contains(r#"android:versionCode="17""#), "{xml}");
    assert!(xml.contains(r#"android:versionName="2.3""#), "{xml}");
    assert!(xml.contains(r#"android:targetSdkVersion="34""#), "{xml}");
    // An ampersand in a label is not an entity waiting to happen.
    assert!(xml.contains(r#"android:label="Tide &amp; Sand""#), "{xml}");
}

#[test]
fn an_id_play_would_refuse_is_refused_here() {
    for id in ["tide", "com.2studio.tide", "com..tide"] {
        let config = AndroidConfig {
            application_id: id.into(),
            ..named()
        };
        let err = config
            .manifest(TEMPLATE)
            .expect_err("an id Play would refuse")
            .to_string();
        assert!(err.contains(id), "{err}");
    }
}

#[test]
fn a_min_sdk_under_the_library_it_would_load_is_refused() {
    let config = AndroidConfig {
        min_sdk: 21,
        ..named()
    };
    let err = config
        .manifest(TEMPLATE)
        .expect_err("a floor under the template's")
        .to_string();
    assert!(err.contains("21") && err.contains("26"), "{err}");
}

#[test]
fn the_base_module_puts_aapt2s_output_where_bundletool_reads_it() {
    let dir = tempfile::tempdir().unwrap();
    let linked = dir.path().join("proto.apk");
    {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&linked).unwrap());
        let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
        for (name, body) in [
            ("AndroidManifest.xml", &b"proto"[..]),
            ("resources.pb", &b"table"[..]),
            ("res/mipmap-anydpi-v26/icon.xml", &b"adaptive"[..]),
        ] {
            zip.start_file(name, options).unwrap();
            std::io::Write::write_all(&mut zip, body).unwrap();
        }
        zip.finish().unwrap();
    }
    let layout = dir.path().join("layout");
    std::fs::create_dir_all(layout.join("lib/arm64-v8a")).unwrap();
    std::fs::create_dir_all(layout.join("assets")).unwrap();
    std::fs::write(layout.join("lib/arm64-v8a/libmain.so"), b"so").unwrap();
    std::fs::write(layout.join("assets/game.bpak"), b"pack").unwrap();
    std::fs::write(layout.join("classes.dex"), b"dex").unwrap();

    let module = dir.path().join("base.zip");
    super::write_module(&module, &linked, &layout).unwrap();

    let zip = zip::ZipArchive::new(std::fs::File::open(&module).unwrap()).unwrap();
    let mut names: Vec<String> = zip
        .file_names()
        .map(|name| name.unwrap().into_owned())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "assets/game.bpak",
            "dex/classes.dex",
            "lib/arm64-v8a/libmain.so",
            "manifest/AndroidManifest.xml",
            "res/mipmap-anydpi-v26/icon.xml",
            "resources.pb",
        ]
    );
}

#[test]
fn a_game_with_an_icon_names_it_on_its_application() {
    let xml = super::name_icon(TEMPLATE);
    assert!(xml.contains(r#"android:icon="@mipmap/icon""#), "{xml}");
    assert_eq!(
        super::name_icon(&xml),
        xml,
        "naming it twice changes nothing"
    );
}

#[test]
fn build_tools_sort_by_version_and_not_by_string() {
    let mut versions = [
        version_key("9.0.0"),
        version_key("34.0.0"),
        version_key("10.0.1"),
    ];
    versions.sort();
    assert_eq!(versions.last().unwrap(), &version_key("34.0.0"));
}

#[test]
fn the_payload_is_the_dex_the_library_and_the_pack_in_a_stable_order() {
    let dir = tempfile::tempdir().unwrap();
    let layout = dir.path();
    std::fs::create_dir_all(layout.join("lib/arm64-v8a")).unwrap();
    std::fs::create_dir_all(layout.join("assets")).unwrap();
    std::fs::write(layout.join("lib/arm64-v8a/libmain.so"), b"so").unwrap();
    std::fs::write(layout.join("assets/game.bpak"), b"pack").unwrap();
    std::fs::write(layout.join("AndroidManifest.xml"), b"<manifest/>").unwrap();
    std::fs::write(layout.join("classes.dex"), b"dex").unwrap();
    std::fs::write(layout.join("classes2.dex"), b"dex").unwrap();
    std::fs::create_dir_all(layout.join("play/res/00")).unwrap();

    let names: Vec<String> = payload_files(layout).into_iter().map(|(n, _)| n).collect();

    assert_eq!(
        names,
        [
            "assets/game.bpak",
            "classes.dex",
            "classes2.dex",
            "lib/arm64-v8a/libmain.so"
        ]
    );
}
