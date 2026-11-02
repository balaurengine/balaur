//! The parsers that read bytes somebody else wrote, against bytes nobody wrote.
//!
//! A pack arrives with a downloaded game, and a scene is a file on disk. None of them is
//! the engine's own output once a project leaves the machine that built it,
//! so each has to answer a corrupted input with an error rather than a panic.
//!
//! Not a fuzzer: `cargo fuzz` wants a nightly toolchain and this workspace is
//! pinned to stable. The mutations are a fixed-seed walk over a valid input,
//! so a failure here reproduces from the seed and the case number alone.

use balaur_core::{App, AppConfig, Pack};

#[path = "../support/corrupt.rs"]
mod corrupt;
use corrupt::corruptions;

fn a_pack() -> Pack {
    let mut pack = Pack {
        manifest: String::from("[application]\nname = \"robust\"\n"),
        ..Pack::default()
    };
    pack.scenes.insert(
        String::from("main.toml"),
        String::from("[[nodes]]\nid = \"n\"\nname = \"Root\"\n"),
    );
    pack.scripts
        .insert(String::from("s.bs"), vec![1, 2, 3, 4, 5, 6, 7, 8]);
    pack.assets
        .insert(String::from("art/a.png"), vec![0x89, b'P', b'N', b'G']);
    pack
}

#[test]
fn a_corrupted_pack_is_an_error_and_never_a_panic() {
    let valid = a_pack().encode();
    assert!(
        Pack::decode(&valid).is_ok(),
        "the corpus itself must decode"
    );
    // Past the magic and the length header, which is where a corruption has
    // to land to test anything: a count of nothing would mean every case was
    // turned away at the first five bytes.
    let mut decoded_anyway = 0;
    for (case, bytes) in corruptions(&valid, 2000).into_iter().enumerate() {
        // The contract is "no panic": a pack that still decodes after a
        // flipped byte is fine, and one that refuses is the point.
        let Ok(decoded) = std::panic::catch_unwind(|| Pack::decode(&bytes)) else {
            panic!(
                "case {case}: Pack::decode panicked on {} bytes",
                bytes.len()
            )
        };
        if decoded.is_ok() {
            decoded_anyway += 1;
        }
    }
    assert!(
        decoded_anyway > 10,
        "only {decoded_anyway} corrupted packs got past the header, so the \
         entries themselves were barely read"
    );
}

/// A scene names components, properties and parents, and a corrupted one
/// names things that are not there. It has to report that, not fall over.
#[test]
fn a_corrupted_scene_is_an_error_and_never_a_panic() {
    let valid = br#"
[[nodes]]
id = "root"
name = "Root"
[nodes.transform]
position = [1.0, 2.0, 3.0]

[[nodes]]
id = "child"
name = "Child"
parent = "root"
[nodes.transform]
scale = [2.0, 2.0, 2.0]
"#;
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::bare(dir.path().to_path_buf())).unwrap();
    let root = app.engine.root();
    assert!(
        balaur_core::project::instantiate_scene(
            &app.engine,
            std::str::from_utf8(valid).unwrap(),
            root,
            false,
        )
        .is_ok(),
        "the corpus itself must instantiate"
    );

    // A corruption that stops the document parsing never reaches the scene
    // reader, so the count of those that do is what says this tested it.
    let mut past_the_parse = 0;
    for (case, bytes) in corruptions(valid, 1500).into_iter().enumerate() {
        // Only the ones that are still text: a scene is a TOML document, and
        // bytes that are not UTF-8 never reach the reader.
        let Ok(source) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if toml::from_str::<toml::Value>(source).is_ok() {
            past_the_parse += 1;
        }
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            balaur_core::project::instantiate_scene(&app.engine, source, root, false)
        }));
        assert!(
            built.is_ok(),
            "case {case}: instantiate_scene panicked on {} bytes",
            bytes.len()
        );
    }
    assert!(
        past_the_parse > 100,
        "only {past_the_parse} corruptions parsed as TOML, so the scene reader \
         itself was barely exercised"
    );
}
