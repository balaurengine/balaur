//! The size pass over a pack of the engine's own assets, proved on real files
//! rather than on generated ones.
//!
//! Only the no-op case; `recode`'s own tests cover each mode's encoder.

use std::path::PathBuf;

use balaur::Pack;
use balaur_export::ExportConfig;
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
        "texture = \"assets/balaur-logo.png\"\nfont = \"fonts/SourceSans3-Regular.ttf\"\n".into(),
    );
    pack.assets.insert(
        "assets/balaur-logo.png".into(),
        repo_file("editor/assets/balaur-logo.png"),
    );
    pack.assets.insert(
        "fonts/SourceSans3-Regular.ttf".into(),
        repo_file("editor/fonts/SourceSans3-Regular.ttf"),
    );
    pack
}

/// An export told to keep what it was given ships exactly the bytes the author
/// wrote.
#[test]
fn an_export_that_keeps_the_originals_changes_nothing() {
    let mut pack = editor_pack();
    let before = pack.encode();
    let config = ExportConfig {
        image_recode: balaur_export::recode::ImageMode::Original,
        audio_recode: balaur_export::recode::AudioMode::Original,
        ..ExportConfig::default()
    };
    let summary = size::prepare(&mut pack, &config).expect("the size pass");
    assert_eq!(pack.encode(), before);
    assert_eq!(summary.total_saved(), 0);
}

/// With nothing stated an image goes to lossless WebP where that is smaller.
#[test]
fn an_export_that_states_nothing_recodes_what_it_can_without_loss() {
    let mut pack = editor_pack();
    let summary = size::prepare(&mut pack, &ExportConfig::default()).expect("the size pass");
    assert!(summary.total_saved() > 0, "the logo came out no smaller");
}
