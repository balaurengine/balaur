//! The size pass over a pack of the engine's own assets: what an author
//! turning the `[export]` keys on actually gets, proved on real files rather
//! than on generated ones.

use std::path::PathBuf;

use balaur::Pack;
use balaur_export::ExportConfig;
use balaur_export::recode::{AudioMode, FontMode, ImageMode};
use balaur_export::size;

/// A file from the repository, read at test time.
fn repo_file(relative: &str) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    std::fs::read(&root).unwrap_or_else(|why| panic!("reading {}: {why}", root.display()))
}

/// A pack holding one scene that names both assets, so nothing is
/// unreferenced and only the re-encoding moves the numbers.
fn editor_pack() -> Pack {
    let mut pack = Pack {
        manifest: "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n".into(),
        ..Pack::default()
    };
    pack.scenes.insert(
        "scenes/main.toml".into(),
        "texture = \"assets/balaur-logo.png\"\nfont = \"fonts/ui-SourceSans3-Regular.ttf\"\n"
            .into(),
    );
    pack.assets.insert(
        "assets/balaur-logo.png".into(),
        repo_file("editor/assets/balaur-logo.png"),
    );
    pack.assets.insert(
        "fonts/ui-SourceSans3-Regular.ttf".into(),
        repo_file("editor/fonts/ui-SourceSans3-Regular.ttf"),
    );
    pack
}

fn every_key_on() -> ExportConfig {
    ExportConfig {
        images: ImageMode::Smallest,
        fonts: FontMode::Subset,
        audio: AudioMode::Flac,
        ..ExportConfig::default()
    }
}

#[test]
fn the_engines_own_assets_come_out_smaller_and_still_decode() {
    let mut pack = editor_pack();
    let before = pack.encode().len();
    // `Png` rather than `Smallest` here: this crate decodes PNG only, and the
    // WebP path proves the same invariant in `recode`'s own tests.
    let config = ExportConfig {
        images: ImageMode::Png,
        ..every_key_on()
    };
    let summary = size::prepare(&mut pack, &config).expect("the size pass");

    let image = &pack.assets["assets/balaur-logo.png"];
    let decoded = image::load_from_memory(image).expect("the re-encoded image still decodes");
    assert_eq!(
        (decoded.width(), decoded.height()),
        (512, 512),
        "a re-encode must not move a sprite's extent"
    );

    let face = &pack.assets["fonts/ui-SourceSans3-Regular.ttf"];
    assert!(
        face.len() < 200 * 1024,
        "the subset face is still {} KB",
        face.len() / 1024
    );
    assert!(
        pack.encode().len() < before,
        "the pack did not get smaller: {before} -> {}",
        pack.encode().len()
    );
    assert_eq!(summary.savings.len(), 2, "both assets should have shrunk");
}

/// The pack keys never change, whatever bytes they end up holding: a scene
/// naming `hero.png` keeps naming it after the file becomes a WebP.
#[test]
fn a_recoded_asset_keeps_the_name_its_scene_uses() {
    let mut pack = editor_pack();
    size::prepare(&mut pack, &every_key_on()).expect("the size pass");
    assert!(pack.assets.contains_key("assets/balaur-logo.png"));
    assert!(pack.assets.contains_key("fonts/ui-SourceSans3-Regular.ttf"));
}

/// Nothing is touched until a key asks for it, so an export that states no
/// policy ships exactly the bytes the author wrote.
#[test]
fn an_export_that_asks_for_nothing_changes_nothing() {
    let mut pack = editor_pack();
    let before = pack.encode();
    let summary = size::prepare(&mut pack, &ExportConfig::default()).expect("the size pass");
    assert_eq!(pack.encode(), before);
    assert_eq!(summary.total_saved(), 0);
}
