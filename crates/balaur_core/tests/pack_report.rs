//! What an export can say about a pack: the bytes it weighs by section, and
//! the files nothing in it names. A file a scene still names must never be
//! reported, or `strip` ships a game missing its art.

use balaur_core::Pack;

/// A pack with a scene naming one texture and a second texture nothing names.
fn two_textures() -> Pack {
    let mut pack = Pack {
        manifest: "[application]\nname = \"p\"\n".to_string(),
        ..Pack::default()
    };
    pack.scenes.insert(
        "main.toml".to_string(),
        "[[nodes]]\nid = \"n\"\ntexture = \"art/hero.png\"\n".to_string(),
    );
    pack.assets.insert("art/hero.png".to_string(), vec![1; 64]);
    pack.assets.insert("art/spare.png".to_string(), vec![2; 32]);
    pack
}

#[test]
fn a_texture_a_scene_names_is_not_reported_as_unreferenced() {
    assert!(
        !two_textures()
            .unreferenced(&[])
            .contains(&"art/hero.png".to_string())
    );
}

#[test]
fn an_asset_no_scene_names_is_reported_as_unreferenced() {
    assert_eq!(two_textures().unreferenced(&[]), vec!["art/spare.png"]);
}

#[test]
fn stripping_removes_exactly_what_the_report_named() {
    let mut pack = two_textures();
    let report = pack.report();
    assert_eq!(pack.strip(&[]), report.unreferenced);
    assert!(pack.assets.contains_key("art/hero.png"));
    assert!(!pack.assets.contains_key("art/spare.png"));
    assert!(pack.unreferenced(&[]).is_empty());
}

#[test]
fn a_keep_glob_protects_an_asset_nothing_names() {
    let mut pack = two_textures();
    pack.assets
        .insert("sfx/deep/step.wav".to_string(), vec![3; 16]);
    let keep = vec!["sfx/**".to_string(), "art/sp*.png".to_string()];
    assert!(pack.unreferenced(&keep).is_empty());
    assert_eq!(pack.strip(&keep), Vec::<String>::new());
    assert_eq!(
        pack.unreferenced(&["art/**".to_string()]),
        vec!["sfx/deep/step.wav"]
    );
}

#[test]
fn a_path_in_a_script_literal_counts_as_a_reference() {
    let mut pack = two_textures();
    pack.scripts.insert(
        "player.rn".to_string(),
        b"pub fn init(node) { node.set_texture(\"art/spare.png\"); }".to_vec(),
    );
    assert!(pack.unreferenced(&[]).is_empty());
}

#[test]
fn a_script_that_is_bytecode_leaves_the_walk_alone() {
    let mut pack = two_textures();
    pack.scripts
        .insert("player.rn".to_string(), vec![0xff, 0xfe, 0x00, 0x01]);
    assert_eq!(pack.unreferenced(&[]), vec!["art/spare.png"]);
}

#[test]
fn a_directory_a_scene_names_covers_the_files_under_it() {
    let mut pack = two_textures();
    pack.scenes.insert(
        "sounds.toml".to_string(),
        "[bank]\nfolder = \"sfx\"\n".to_string(),
    );
    pack.assets.insert("sfx/step.wav".to_string(), vec![3; 16]);
    assert_eq!(pack.unreferenced(&[]), vec!["art/spare.png"]);
}

#[test]
fn a_reference_with_an_entry_still_names_its_file() {
    let mut pack = two_textures();
    pack.scenes.insert(
        "clip.toml".to_string(),
        "[clip]\nsource = \"art/spare.png#run\"\n".to_string(),
    );
    assert!(pack.unreferenced(&[]).is_empty());
}

#[test]
fn an_id_reference_resolves_through_the_asset_index() {
    let mut pack = two_textures();
    pack.scenes.insert(
        "assets/index.toml".to_string(),
        "abc123 = \"art/spare.png\"\n".to_string(),
    );
    assert_eq!(
        pack.unreferenced(&[]),
        vec!["art/spare.png"],
        "the index alone is a map, not a reference"
    );
    pack.scenes.insert(
        "props.toml".to_string(),
        "[node]\ntexture = \"id://abc123\"\n".to_string(),
    );
    assert!(pack.unreferenced(&[]).is_empty());
}

#[test]
fn a_bitmap_fonts_page_counts_as_referenced() {
    let mut pack = two_textures();
    pack.scenes.insert(
        "label.toml".to_string(),
        "[text]\nfont = \"fonts/pixel.fnt\"\n".to_string(),
    );
    pack.assets.insert(
        "fonts/pixel.fnt".to_string(),
        b"info face=\"pixel\" size=16\npage id=0 file=\"pixel_0.png\"\nchar id=65 x=0\n".to_vec(),
    );
    pack.assets
        .insert("fonts/pixel_0.png".to_string(), vec![4; 48]);
    assert_eq!(pack.unreferenced(&[]), vec!["art/spare.png"]);
}

#[test]
fn a_fnt_descriptor_is_an_extension_a_pack_ships() {
    assert!(balaur_core::pack::ASSET_EXTENSIONS.contains(&"fnt"));
}

#[test]
fn the_reports_section_bytes_add_up_to_the_encoded_pack() {
    let mut pack = two_textures();
    pack.scripts.insert("player.rn".to_string(), vec![7; 100]);
    let report = pack.report();
    assert_eq!(report.total, pack.encode().len());
    assert_eq!(
        report.sections.iter().map(|s| s.bytes).sum::<usize>(),
        report.total
    );
    assert_eq!(report.sections.len(), 4);
}

#[test]
fn the_report_counts_assets_by_extension_and_names_the_heaviest() {
    let mut pack = two_textures();
    pack.assets
        .insert("sfx/step.wav".to_string(), vec![3; 1000]);
    let report = pack.report();
    let png = report
        .extensions
        .iter()
        .find(|e| e.extension == "png")
        .expect("two png assets were packed");
    assert_eq!((png.entries, png.bytes), (2, 96));
    assert_eq!(report.extensions[0].extension, "wav");
    assert_eq!(report.largest[0].key, "sfx/step.wav");
    assert_eq!(report.unreferenced_bytes, 32 + 1000);
}

#[test]
fn the_reports_display_names_every_section_and_the_unreferenced_count() {
    let printed = two_textures().report().to_string();
    for expected in [
        "manifest",
        "scenes",
        "scripts",
        "assets",
        "png",
        "art/hero.png",
    ] {
        assert!(
            printed.contains(expected),
            "{expected} missing from\n{printed}"
        );
    }
    assert!(printed.contains("1 file nothing references, 32 B"));
}

/// The UI finds faces by listing `fonts/`, so no scene names one and a strip
/// that trusted references alone would take a project's text away.
#[test]
fn a_font_is_never_unreferenced_though_nothing_names_it() {
    let mut pack = Pack {
        manifest: "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n".into(),
        ..Pack::default()
    };
    pack.scenes
        .insert("scenes/main.toml".into(), "nodes = []\n".into());
    pack.assets
        .insert("fonts/ui-Regular.ttf".into(), vec![1, 2, 3]);
    pack.assets.insert("art/unused.png".into(), vec![4, 5, 6]);
    assert_eq!(pack.unreferenced(&[]), vec!["art/unused.png".to_string()]);
}
#[test]
fn print_a_real_report() {
    use balaur_core::pack::Pack;
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut pack = Pack {
        manifest: std::fs::read_to_string(root.join("editor/project.toml")).unwrap(),
        ..Pack::default()
    };
    for entry in std::fs::read_dir(root.join("editor/scripts")).unwrap() {
        let p = entry.unwrap().path();
        let name = format!("scripts/{}", p.file_name().unwrap().to_string_lossy());
        pack.scripts.insert(name, std::fs::read(&p).unwrap());
    }
    for entry in std::fs::read_dir(root.join("editor/scenes")).unwrap() {
        let p = entry.unwrap().path();
        let name = format!("scenes/{}", p.file_name().unwrap().to_string_lossy());
        pack.scenes
            .insert(name, std::fs::read_to_string(&p).unwrap());
    }
    for (dir, prefix) in [("editor/fonts", "fonts"), ("editor/assets", "assets")] {
        for entry in std::fs::read_dir(root.join(dir)).unwrap() {
            let p = entry.unwrap().path();
            let name = format!("{prefix}/{}", p.file_name().unwrap().to_string_lossy());
            pack.assets.insert(name, std::fs::read(&p).unwrap());
        }
    }
    println!("----8<----\n{}\n---->8----", pack.report());
}
