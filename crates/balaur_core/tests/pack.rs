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
    dir
}

#[test]
fn a_pack_round_trips_through_bytes() {
    let dir = project();
    let pack = Pack::build(dir.path(), &Reversing).unwrap();
    let decoded = Pack::decode(&pack.encode()).unwrap();

    assert_eq!(decoded.manifest, pack.manifest);
    assert_eq!(decoded.scenes, pack.scenes);
    assert_eq!(decoded.scripts, pack.scripts);
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

/// A file the compiler does not claim is not a script, so it must not end up
/// in the pack as one.
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

/// A broken script must fail the export rather than ship a pack missing it.
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

/// The order is not merely stable, it is sorted — the property that lets a CI
/// matrix compare a pack digest built on Linux against one built on macOS.
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
