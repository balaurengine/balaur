//! Export packs: what a shipped game runs from. A pack that does not decode to
//! what was encoded is a game that does not start.

use balaur_core::Pack;
use balaur_script::ScriptCompiler;

/// Claims `.txt` and "compiles" by reversing, so the test proves the pack
/// stores whatever the backend produced rather than the source.
struct Reversing;

impl ScriptCompiler for Reversing {
    fn extensions(&self) -> &[&str] {
        &["txt"]
    }

    fn compile(&self, _rel: &str, source: &str) -> anyhow::Result<Vec<u8>> {
        Ok(source.bytes().rev().collect())
    }
}

struct Failing;

impl ScriptCompiler for Failing {
    fn extensions(&self) -> &[&str] {
        &["txt"]
    }

    fn compile(&self, rel: &str, _source: &str) -> anyhow::Result<Vec<u8>> {
        Err(anyhow::anyhow!("no good: {rel}"))
    }
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"p\"\nmain_scene = \"m.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("m.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("s.txt"), "abc").unwrap();
    // A texture that has to travel, and a note that must not.
    std::fs::create_dir_all(dir.path().join("art")).unwrap();
    std::fs::write(dir.path().join("art/hero.png"), PNG).unwrap();
    std::fs::write(dir.path().join("art/notes.md"), "not shippable").unwrap();
    dir
}

/// The smallest valid PNG: one transparent pixel.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

#[test]
fn a_pack_round_trips_through_bytes() {
    let dir = project();
    let pack = Pack::build(dir.path(), &Reversing).unwrap();
    let decoded = Pack::decode(&pack.encode()).unwrap();

    assert_eq!(decoded.manifest, pack.manifest);
    assert_eq!(decoded.scenes, pack.scenes);
    assert_eq!(decoded.scripts, pack.scripts);
    assert_eq!(decoded.assets, pack.assets);
}

/// The gap this section closes: before it, a shipped game's textures and
/// sounds stayed behind on the build machine.
#[test]
fn binary_assets_travel_inside_the_pack() {
    let dir = project();
    let pack = Pack::build(dir.path(), &Reversing).unwrap();

    assert_eq!(
        pack.assets.get("art/hero.png").map(Vec::as_slice),
        Some(PNG),
        "the texture should be in the pack byte for byte"
    );
    assert!(
        !pack.assets.contains_key("art/notes.md"),
        "only shippable extensions travel"
    );
    let decoded = Pack::decode(&pack.encode()).unwrap();
    assert_eq!(
        decoded.assets.get("art/hero.png").map(Vec::as_slice),
        Some(PNG)
    );
}

#[test]
fn a_corrupted_asset_is_caught_by_its_hash() {
    let dir = project();
    let mut bytes = Pack::build(dir.path(), &Reversing).unwrap().encode();
    // Flip the last byte of the payload, which is inside the asset section.
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;

    let err = Pack::decode(&bytes).expect_err("a corrupted asset must not decode");
    assert!(
        err.to_string().contains("art/hero.png"),
        "the error should name the file that failed: {err}"
    );
}

#[test]
fn project_files_serve_a_packed_asset_without_touching_disk() {
    use balaur_core::project::{AssetSource, ProjectFiles};
    let dir = project();
    let pack = Pack::build(dir.path(), &Reversing).unwrap();
    // A root that does not exist: anything found had to come from the pack.
    let files = ProjectFiles::packed(
        "/nonexistent".into(),
        pack.assets.clone(),
        AssetSource::Embedded,
    );

    assert_eq!(files.read("art/hero.png").unwrap(), PNG);
    assert_eq!(files.list("art"), vec!["art/hero.png".to_string()]);
    assert!(files.read("art/missing.png").is_err());
}

/// The default is strict on purpose: a shipped game that quietly reads the
/// working directory works on the machine that built it and nowhere else.
#[test]
fn an_embedded_game_will_not_read_a_file_beside_it() {
    use balaur_core::project::{AssetSource, ProjectFiles};
    let dir = project();
    // Packed with nothing, pointed at a real directory that has the file.
    let strict = ProjectFiles::packed(
        dir.path().to_path_buf(),
        std::collections::BTreeMap::new(),
        AssetSource::Embedded,
    );
    let err = strict
        .read("art/hero.png")
        .expect_err("embedded must not fall through to disk");
    assert!(
        err.to_string().contains("embedded+files"),
        "the error should say how to allow it: {err}"
    );

    let permissive = ProjectFiles::packed(
        dir.path().to_path_buf(),
        std::collections::BTreeMap::new(),
        AssetSource::EmbeddedThenFiles,
    );
    assert_eq!(
        permissive.read("art/hero.png").unwrap(),
        PNG,
        "embedded+files falls through to the directory"
    );
}

/// A missing asset has to say where it looked; "not found" alone is the least
/// useful sentence a shipped game can print.
#[test]
fn a_missing_asset_names_where_it_looked() {
    use balaur_core::project::{AssetSource, ProjectFiles};
    let files = ProjectFiles::packed(
        "/some/root".into(),
        std::collections::BTreeMap::new(),
        AssetSource::EmbeddedThenFiles,
    );
    let err = files.read("art/gone.png").unwrap_err().to_string();
    assert!(err.contains("art/gone.png"), "names the asset: {err}");
    assert!(err.contains("/some/root"), "names the directory: {err}");
}

#[test]
fn the_backend_decides_what_a_script_compiles_to() {
    let dir = project();
    let pack = Pack::build(dir.path(), &Reversing).unwrap();
    assert_eq!(
        pack.scripts.get("s.txt").map(Vec::as_slice),
        Some(b"cba".as_slice())
    );
}

#[test]
fn unclaimed_extensions_are_left_alone() {
    let dir = project();
    std::fs::write(dir.path().join("readme.md"), "hello").unwrap();
    let pack = Pack::build(dir.path(), &Reversing).unwrap();
    assert!(!pack.scripts.contains_key("readme.md"));
    assert!(!pack.scenes.contains_key("readme.md"));
}

#[test]
fn scenes_are_gathered_but_the_manifest_is_not_one_of_them() {
    let dir = project();
    let pack = Pack::build(dir.path(), &Reversing).unwrap();
    assert!(pack.scenes.contains_key("m.toml"));
    assert!(!pack.scenes.contains_key("project.toml"));
    assert!(pack.manifest.contains("name = \"p\""));
}

#[test]
fn a_compile_error_fails_the_build() {
    let dir = project();
    let err = Pack::build(dir.path(), &Failing).unwrap_err();
    assert!(err.to_string().contains("no good"), "unhelpful: {err}");
}

#[test]
fn a_project_without_a_manifest_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Pack::build(dir.path(), &Reversing).is_err());
}

#[test]
fn truncated_or_foreign_bytes_do_not_decode() {
    let dir = project();
    let bytes = Pack::build(dir.path(), &Reversing).unwrap().encode();
    assert!(Pack::decode(&bytes[..bytes.len() / 2]).is_err());
    assert!(Pack::decode(b"not a pack at all").is_err());
    assert!(Pack::decode(&[]).is_err());
}

/// Shipping the same sources twice must produce the same bytes, on any machine
/// that builds them. `encode` walks the scene and script maps, so a hashed map
/// would leak its iteration order into the file: before this was an ordered
/// map, ten exports of a three-script project produced five different files.
///
/// Insertion order is the lever a hashed map would react to, so the two packs
/// here are filled in opposite orders.
#[test]
fn a_pack_encodes_the_same_whatever_order_it_was_filled_in() {
    let entries = ["a.toml", "m.toml", "z.toml", "b.toml", "q.toml"];
    let mut forward = Pack {
        manifest: "name = \"p\"\n".into(),
        ..Default::default()
    };
    let mut backward = forward.clone();
    for name in entries {
        forward.scenes.insert((*name).into(), name.to_uppercase());
        forward
            .scripts
            .insert((*name).into(), name.as_bytes().into());
    }
    for name in entries.iter().rev() {
        backward.scenes.insert((*name).into(), name.to_uppercase());
        backward
            .scripts
            .insert((*name).into(), name.as_bytes().into());
    }
    assert_eq!(
        forward.encode(),
        backward.encode(),
        "pack bytes depend on the order entries were added"
    );
}

#[test]
fn a_pack_writes_its_entries_in_sorted_order() {
    let mut pack = Pack {
        manifest: "name = \"p\"\n".into(),
        ..Default::default()
    };
    for name in ["z.txt", "a.txt", "m.txt"] {
        pack.scripts.insert(name.into(), name.as_bytes().into());
    }
    let bytes = pack.encode();
    let positions: Vec<_> = ["a.txt", "m.txt", "z.txt"]
        .iter()
        .map(|n| {
            bytes
                .windows(n.len())
                .position(|w| w == n.as_bytes())
                .expect("every key is in the encoding")
        })
        .collect();
    let mut sorted = positions.clone();
    sorted.sort_unstable();
    assert_eq!(positions, sorted, "keys are not written in sorted order");
}
